use crate::position::{normalize_path, path_from_uri, uri_from_path, utf16_length};
use grep::matcher::Matcher;
use grep::regex::RegexMatcherBuilder;
use grep::searcher::{sinks::UTF8, BinaryDetection, SearcherBuilder};
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct Occurrence {
    pub(crate) uri: String,
    pub(crate) path: PathBuf,
    pub(crate) line: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) line_text: String,
}

impl Occurrence {
    pub(crate) fn location(&self) -> Value {
        json!({
            "uri": self.uri,
            "range": {
                "start": {"line": self.line, "character": self.start},
                "end": {"line": self.line, "character": self.end}
            }
        })
    }

    pub(crate) fn key(&self) -> (String, usize, usize, usize) {
        (self.uri.clone(), self.line, self.start, self.end)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ServerConfig {
    pub(crate) max_results: usize,
    pub(crate) case_sensitive: bool,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_results: 1000,
            case_sensitive: true,
            include: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

pub(crate) fn is_word_character(character: Option<char>) -> bool {
    character
        .map(|value| value.is_alphanumeric() || value == '_')
        .unwrap_or(false)
}

pub(crate) fn find_matches(line: &str, symbol: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    let symbol_characters: Vec<char> = symbol.chars().collect();
    let line_characters: Vec<(usize, char)> = line.char_indices().collect();
    if symbol_characters.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for start_index in 0..line_characters.len() {
        if start_index + symbol_characters.len() > line_characters.len() {
            break;
        }
        let matches_symbol =
            symbol_characters
                .iter()
                .enumerate()
                .all(|(offset, symbol_character)| {
                    let candidate = line_characters[start_index + offset].1;
                    if case_sensitive {
                        candidate == *symbol_character
                    } else {
                        candidate.eq_ignore_ascii_case(symbol_character)
                            || candidate == *symbol_character
                    }
                });
        if !matches_symbol {
            continue;
        }
        let before = start_index
            .checked_sub(1)
            .map(|index| line_characters[index].1);
        let after = line_characters
            .get(start_index + symbol_characters.len())
            .map(|(_, value)| *value);
        if is_word_character(symbol_characters.first().copied()) && is_word_character(before) {
            continue;
        }
        if is_word_character(symbol_characters.last().copied()) && is_word_character(after) {
            continue;
        }
        let start = line_characters[start_index].0;
        let end = line_characters
            .get(start_index + symbol_characters.len())
            .map(|(byte_index, _)| *byte_index)
            .unwrap_or_else(|| line.len());
        matches.push((start, end));
    }
    matches
}

pub(crate) struct WorkspaceSearcher {
    pub(crate) root: PathBuf,
    pub(crate) config: ServerConfig,
}

impl WorkspaceSearcher {
    pub(crate) fn search(
        &self,
        symbol: &str,
        open_documents: &HashMap<String, String>,
    ) -> Vec<Occurrence> {
        if symbol.is_empty() {
            return Vec::new();
        }
        let mut results = self.search_with_ripgrep(symbol);
        let open_paths: HashSet<PathBuf> = open_documents
            .keys()
            .map(|uri| path_from_uri(uri))
            .collect();
        results.retain(|occurrence| !open_paths.contains(&occurrence.path));
        for (uri, text) in open_documents {
            results.extend(self.search_text(uri, &path_from_uri(uri), text, symbol));
        }

        let mut deduplicated = HashMap::new();
        for occurrence in results {
            deduplicated.insert(occurrence.key(), occurrence);
        }
        let mut results: Vec<Occurrence> = deduplicated.into_values().collect();
        results.sort_by(|left, right| {
            (&left.uri, left.line, left.start).cmp(&(&right.uri, right.line, right.start))
        });
        results.truncate(self.config.max_results);
        results
    }

    fn search_with_ripgrep(&self, symbol: &str) -> Vec<Occurrence> {
        let mut matcher_builder = RegexMatcherBuilder::new();
        matcher_builder
            .fixed_strings(true)
            .case_insensitive(!self.config.case_sensitive);
        let Ok(matcher) = matcher_builder.build(symbol) else {
            return Vec::new();
        };

        let mut walker = WalkBuilder::new(&self.root);
        walker.hidden(false).add_custom_ignore_filename(".rgignore");
        let mut overrides = OverrideBuilder::new(&self.root);
        if overrides.add("!.git/**").is_err() {
            return Vec::new();
        }
        for pattern in &self.config.include {
            if overrides.add(pattern).is_err() {
                return Vec::new();
            }
        }
        for pattern in &self.config.exclude {
            if overrides.add(&format!("!{pattern}")).is_err() {
                return Vec::new();
            }
        }
        let Ok(overrides) = overrides.build() else {
            return Vec::new();
        };
        walker.overrides(overrides);

        let searcher = SearcherBuilder::new()
            .line_number(true)
            .binary_detection(BinaryDetection::quit(b'\0'))
            .build();
        let mut results = Vec::new();

        for entry in walker.build() {
            let Ok(entry) = entry else {
                continue;
            };
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let path = normalize_path(entry.into_path());
            let uri = uri_from_path(&path);
            let mut file_results = Vec::new();
            let mut searcher = searcher.clone();
            let _ = searcher.search_path(
                &matcher,
                &path,
                UTF8(|line_number, line| {
                    let line_text = line.trim_end_matches(['\r', '\n']).to_string();
                    matcher
                        .find_iter(line.as_bytes(), |matched| {
                            let start = matched.start();
                            let end = matched.end();
                            if !matches_word_boundaries(line, symbol, start, end) {
                                return true;
                            }
                            file_results.push(Occurrence {
                                uri: uri.clone(),
                                path: path.clone(),
                                line: line_number.saturating_sub(1) as usize,
                                start: utf16_length(&line[..start]),
                                end: utf16_length(&line[..end]),
                                line_text: line_text.clone(),
                            });
                            true
                        })
                        .map_err(|error| io::Error::other(error.to_string()))?;
                    Ok(true)
                }),
            );
            results.extend(file_results);
        }
        results
    }

    fn search_text(&self, uri: &str, path: &Path, text: &str, symbol: &str) -> Vec<Occurrence> {
        let mut results = Vec::new();
        for (line_number, line) in text.split('\n').enumerate() {
            for (start, end) in find_matches(line, symbol, self.config.case_sensitive) {
                results.push(Occurrence {
                    uri: uri.to_string(),
                    path: path.to_path_buf(),
                    line: line_number,
                    start: utf16_length(&line[..start]),
                    end: utf16_length(&line[..end]),
                    line_text: line.to_string(),
                });
            }
        }
        results
    }
}

fn matches_word_boundaries(line: &str, symbol: &str, start: usize, end: usize) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[end..].chars().next();
    if is_word_character(symbol.chars().next()) && is_word_character(before) {
        return false;
    }
    if is_word_character(symbol.chars().last()) && is_word_character(after) {
        return false;
    }
    true
}
