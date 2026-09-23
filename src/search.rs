use crate::position::{normalize_path, path_from_uri, uri_from_path, utf16_length};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        let mut results = match self.search_with_rg(symbol) {
            Some(results) => results,
            None => {
                eprintln!("ripgrep was not found; using the built-in text search");
                self.search_with_builtin(symbol)
            }
        };
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

    fn search_with_rg(&self, symbol: &str) -> Option<Vec<Occurrence>> {
        let mut command = Command::new("rg");
        command.args([
            "--json",
            "--no-messages",
            "--color",
            "never",
            "--hidden",
            "--glob",
            "!.git/**",
        ]);
        for pattern in &self.config.include {
            command.args(["--glob", pattern]);
        }
        for pattern in &self.config.exclude {
            command.args(["--glob", &format!("!{pattern}")]);
        }
        if !self.config.case_sensitive {
            command.arg("--ignore-case");
        }
        let pattern = rg_pattern(symbol);
        let root = self.root.to_string_lossy().to_string();
        command.args(["--pcre2", "--"]).arg(pattern).arg(root);
        let output = command.output().ok()?;
        if !matches!(output.status.code(), Some(0) | Some(1)) {
            return None;
        }

        let mut results = Vec::new();
        for raw_line in output.stdout.split(|byte| *byte == b'\n') {
            let Ok(payload) = serde_json::from_slice::<Value>(raw_line) else {
                continue;
            };
            if payload.get("type").and_then(Value::as_str) != Some("match") {
                continue;
            }
            let data = &payload["data"];
            let Some(path_text) = data["path"]["text"].as_str() else {
                continue;
            };
            let path = normalize_path(PathBuf::from(path_text));
            let line_text = data["lines"]["text"]
                .as_str()
                .unwrap_or("")
                .trim_end_matches(['\r', '\n'])
                .to_string();
            let line = data["line_number"].as_u64().unwrap_or(1).saturating_sub(1) as usize;
            for submatch in data["submatches"].as_array().into_iter().flatten() {
                let start = submatch["start"].as_u64().unwrap_or(0) as usize;
                let end = submatch["end"].as_u64().unwrap_or(start as u64) as usize;
                let bytes = line_text.as_bytes();
                let start = start.min(bytes.len());
                let end = end.min(bytes.len()).max(start);
                let prefix = String::from_utf8_lossy(&bytes[..start]);
                let matched = String::from_utf8_lossy(&bytes[start..end]);
                results.push(Occurrence {
                    uri: uri_from_path(&path),
                    path: path.clone(),
                    line,
                    start: utf16_length(&prefix),
                    end: utf16_length(&format!("{prefix}{matched}")),
                    line_text: line_text.clone(),
                });
            }
        }
        Some(results)
    }

    fn search_with_builtin(&self, symbol: &str) -> Vec<Occurrence> {
        let mut files = Vec::new();
        collect_files(&self.root, &mut files);
        let mut results = Vec::new();
        for path in files {
            let relative = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .to_string_lossy();
            if !self.config.include.is_empty()
                && !self
                    .config
                    .include
                    .iter()
                    .any(|pattern| glob_matches(pattern, &relative))
            {
                continue;
            }
            if self
                .config
                .exclude
                .iter()
                .any(|pattern| glob_matches(pattern, &relative))
            {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            results.extend(self.search_text(&uri_from_path(&path), &path, &text, symbol));
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

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry
            .file_type()
            .map(|kind| kind.is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(normalize_path(path));
        }
    }
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    if let Some(stripped) = pattern.strip_prefix("**/") {
        if glob_matches(stripped, text) {
            return true;
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    fn matches(pattern: &[char], text: &[char], pattern_index: usize, text_index: usize) -> bool {
        if pattern_index == pattern.len() {
            return text_index == text.len();
        }
        match pattern[pattern_index] {
            '*' => {
                matches(pattern, text, pattern_index + 1, text_index)
                    || (text_index < text.len()
                        && matches(pattern, text, pattern_index, text_index + 1))
            }
            '?' => {
                text_index < text.len() && matches(pattern, text, pattern_index + 1, text_index + 1)
            }
            value => {
                text_index < text.len()
                    && value == text[text_index]
                    && matches(pattern, text, pattern_index + 1, text_index + 1)
            }
        }
    }
    matches(&pattern, &text, 0, 0)
}

fn rg_pattern(symbol: &str) -> String {
    let mut escaped = String::new();
    for character in symbol.chars() {
        if r#"\.^$*+?()[]{}|"#.contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    let prefix = if is_word_character(symbol.chars().next()) {
        r"(?<![\p{L}\p{N}_])"
    } else {
        ""
    };
    let suffix = if is_word_character(symbol.chars().last()) {
        r"(?![\p{L}\p{N}_])"
    } else {
        ""
    };
    format!("{prefix}{escaped}{suffix}")
}
