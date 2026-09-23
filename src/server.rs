use crate::navigation::{is_definition, rank_definitions, same_document};
use crate::position::{normalize_path, path_from_uri, symbol_at_position, utf16_to_byte_index};
use crate::search::{Occurrence, ServerConfig, WorkspaceSearcher};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

pub struct LspServer {
    root: PathBuf,
    config: ServerConfig,
    pub(crate) documents: HashMap<String, String>,
}

impl LspServer {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root: root
                .map(normalize_path)
                .unwrap_or_else(|| normalize_path(PathBuf::from("."))),
            config: ServerConfig::default(),
            documents: HashMap::new(),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        if let Some(uri) = params.get("rootUri").and_then(Value::as_str) {
            self.root = path_from_uri(uri);
        } else if let Some(path) = params.get("rootPath").and_then(Value::as_str) {
            self.root = normalize_path(PathBuf::from(path));
        } else if let Some(uri) = params["workspaceFolders"][0]["uri"].as_str() {
            self.root = path_from_uri(uri);
        }
        if let Some(options) = params
            .get("initializationOptions")
            .and_then(Value::as_object)
        {
            if let Some(max_results) = options.get("maxResults").and_then(Value::as_u64) {
                self.config.max_results = max_results.max(1) as usize;
            }
            if let Some(case_sensitive) = options.get("caseSensitive").and_then(Value::as_bool) {
                self.config.case_sensitive = case_sensitive;
            }
            if let Some(include) = options.get("include").and_then(Value::as_array) {
                self.config.include = include
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
            }
            if let Some(exclude) = options.get("exclude").and_then(Value::as_array) {
                self.config.exclude = exclude
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
            }
        }
        json!({
            "capabilities": {
                "textDocumentSync": {"openClose": true, "change": 1},
                "definitionProvider": true,
                "referencesProvider": true
            },
            "serverInfo": {"name": "any-lsp", "version": env!("CARGO_PKG_VERSION")}
        })
    }

    pub fn handle(&mut self, method: &str, params: &Value) -> Value {
        match method {
            "initialize" => self.initialize(params),
            "initialized" | "shutdown" | "exit" => Value::Null,
            "textDocument/didOpen" => {
                if let Some(document) = params.get("textDocument") {
                    if let (Some(uri), Some(text)) =
                        (document["uri"].as_str(), document["text"].as_str())
                    {
                        self.documents.insert(uri.to_string(), text.to_string());
                    }
                }
                Value::Null
            }
            "textDocument/didChange" => {
                self.did_change(params);
                Value::Null
            }
            "textDocument/didClose" => {
                if let Some(uri) = params["textDocument"]["uri"].as_str() {
                    self.documents.remove(uri);
                }
                Value::Null
            }
            "textDocument/definition" => self.definition(params),
            "textDocument/references" => self.references(params),
            _ => Value::Null,
        }
    }

    fn did_change(&mut self, params: &Value) {
        let Some(uri) = params["textDocument"]["uri"].as_str() else {
            return;
        };
        let mut current = self.documents.get(uri).cloned().unwrap_or_default();
        if let Some(changes) = params.get("contentChanges").and_then(Value::as_array) {
            for change in changes {
                if change.get("range").is_none() {
                    current = change["text"].as_str().unwrap_or("").to_string();
                } else {
                    current = apply_change(&current, change);
                }
            }
        }
        self.documents.insert(uri.to_string(), current);
    }

    fn document_and_symbol(&self, params: &Value) -> Option<(String, String)> {
        let uri = params["textDocument"]["uri"].as_str()?.to_string();
        let text = self
            .documents
            .get(&uri)
            .cloned()
            .or_else(|| fs::read_to_string(path_from_uri(&uri)).ok())?;
        let symbol = symbol_at_position(&text, params.get("position").unwrap_or(&Value::Null))?;
        Some((uri, symbol))
    }

    fn search(&self, symbol: &str, case_sensitive: Option<bool>) -> Vec<Occurrence> {
        let mut config = self.config.clone();
        if let Some(case_sensitive) = case_sensitive {
            config.case_sensitive = case_sensitive;
        }
        WorkspaceSearcher {
            root: self.root.clone(),
            config,
        }
        .search(symbol, &self.documents)
    }

    fn definition(&self, params: &Value) -> Value {
        let Some((uri, symbol)) = self.document_and_symbol(params) else {
            return json!([]);
        };
        let mut occurrences = self.search(&symbol, None);
        let mut definitions = rank_definitions(&occurrences, &symbol, Some(&uri));
        if self.config.case_sensitive
            && !occurrences
                .iter()
                .any(|item| same_document(&item.uri, &uri) && is_definition(item, &symbol))
        {
            let broader = self.search(&symbol, Some(false));
            if broader
                .iter()
                .any(|item| same_document(&item.uri, &uri) && is_definition(item, &symbol))
            {
                occurrences = broader;
                definitions = rank_definitions(&occurrences, &symbol, Some(&uri));
            }
        }
        Value::Array(
            definitions
                .into_iter()
                .map(|item| item.location())
                .collect(),
        )
    }

    fn references(&self, params: &Value) -> Value {
        let Some((_, symbol)) = self.document_and_symbol(params) else {
            return json!([]);
        };
        let mut occurrences = self.search(&symbol, None);
        let include_declaration = params["context"]["includeDeclaration"]
            .as_bool()
            .unwrap_or(false);
        if !include_declaration {
            let definitions: HashSet<_> = occurrences
                .iter()
                .filter(|item| is_definition(item, &symbol))
                .map(Occurrence::key)
                .collect();
            occurrences.retain(|item| !definitions.contains(&item.key()));
        }
        Value::Array(
            occurrences
                .into_iter()
                .map(|item| item.location())
                .collect(),
        )
    }
}

fn absolute_offset(text: &str, line_number: usize, character: usize) -> usize {
    let lines: Vec<&str> = text.split('\n').collect();
    let line_number = line_number.min(lines.len().saturating_sub(1));
    let before = lines[..line_number]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>();
    before + utf16_to_byte_index(lines[line_number], character)
}

pub(crate) fn apply_change(current: &str, change: &Value) -> String {
    let start = &change["range"]["start"];
    let end = &change["range"]["end"];
    let start_offset = absolute_offset(
        current,
        start["line"].as_u64().unwrap_or(0) as usize,
        start["character"].as_u64().unwrap_or(0) as usize,
    );
    let end_offset = absolute_offset(
        current,
        end["line"].as_u64().unwrap_or(0) as usize,
        end["character"].as_u64().unwrap_or(0) as usize,
    );
    format!(
        "{}{}{}",
        &current[..start_offset],
        change["text"].as_str().unwrap_or(""),
        &current[end_offset..]
    )
}
