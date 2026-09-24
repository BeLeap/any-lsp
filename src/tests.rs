use crate::position::{symbol_at_position, uri_from_path};
use crate::search::{find_matches, ServerConfig, WorkspaceSearcher};
use crate::server::apply_change;
use crate::transport::{read_message, serve};
use crate::LspServer;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("any-lsp-rust-{id}"));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_file(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

#[test]
fn definition_prefers_code_declaration() {
    let directory = TempDir::new();
    let code = directory.0.join("code.py");
    write_file(
        &code,
        "def greet():\n    return message\n\nmessage = 'hello'\nprint(message)\n",
    );
    let uri = uri_from_path(&code);
    let mut server = LspServer::new(Some(directory.0.clone()));
    server
        .documents
        .insert(uri.clone(), fs::read_to_string(&code).unwrap());
    let result = server.handle(
        "textDocument/definition",
        &json!({"textDocument": {"uri": uri}, "position": {"line": 1, "character": 12}}),
    );
    assert_eq!(result[0]["range"]["start"]["line"], 3);
}

#[test]
fn definition_ignores_type_references_in_declaration_signatures() {
    let directory = TempDir::new();
    let declaration = directory.0.join("types.rs");
    let usage = directory.0.join("api.rs");
    let usage_text = "use crate::types::Widget;\npub fn build(widget: Widget) {}\n";
    write_file(&declaration, "pub struct Widget;\n");
    write_file(&usage, usage_text);

    let declaration_uri = uri_from_path(&declaration);
    let usage_uri = uri_from_path(&usage);
    let mut server = LspServer::new(Some(directory.0.clone()));
    server
        .documents
        .insert(usage_uri.clone(), usage_text.to_string());
    let character = usage_text.lines().nth(1).unwrap().find("Widget").unwrap() + 1;
    let result = server.handle(
        "textDocument/definition",
        &json!({"textDocument": {"uri": usage_uri}, "position": {"line": 1, "character": character}}),
    );

    assert_eq!(result[0]["uri"], declaration_uri);
    assert_eq!(result[0]["range"]["start"]["line"], 0);
}

#[test]
fn prose_definition_is_navigable() {
    let directory = TempDir::new();
    let notes = directory.0.join("notes.md");
    write_file(
        &notes,
        "# Message\n\nMessage is text shown to a reader.\n\nThe message is friendly.\n",
    );
    let uri = uri_from_path(&notes);
    let mut server = LspServer::new(Some(directory.0.clone()));
    server
        .documents
        .insert(uri.clone(), fs::read_to_string(&notes).unwrap());
    let result = server.handle(
        "textDocument/definition",
        &json!({"textDocument": {"uri": uri}, "position": {"line": 4, "character": 7}}),
    );
    assert_eq!(result[0]["range"]["start"]["line"], 2);
}

#[test]
fn references_exclude_declarations_by_default() {
    let directory = TempDir::new();
    let code = directory.0.join("code.py");
    let notes = directory.0.join("notes.md");
    write_file(&code, "message = 'hello'\nreturn message\nprint(message)\n");
    write_file(&notes, "The message is friendly.\n");
    let uri = uri_from_path(&code);
    let mut server = LspServer::new(Some(directory.0.clone()));
    server
        .documents
        .insert(uri.clone(), fs::read_to_string(&code).unwrap());
    let result = server.handle(
        "textDocument/references",
        &json!({"textDocument": {"uri": uri}, "position": {"line": 1, "character": 8}}),
    );
    let lines: HashSet<_> = result
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["range"]["start"]["line"].as_u64().unwrap())
        .collect();
    assert_eq!(lines, HashSet::from([0, 1, 2]));
}

#[test]
fn unsaved_buffer_replaces_disk_content() {
    let directory = TempDir::new();
    let code = directory.0.join("code.py");
    write_file(&code, "message = 'saved'\nold(message)\n");
    let uri = uri_from_path(&code);
    let mut server = LspServer::new(Some(directory.0.clone()));
    server.documents.insert(
        uri.clone(),
        "message = 'changed'\nuse(message)\n".to_string(),
    );
    let result = server.handle("textDocument/references", &json!({"textDocument": {"uri": uri}, "position": {"line": 1, "character": 5}, "context": {"includeDeclaration": true}}));
    let lines: Vec<_> = result
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["range"]["start"]["line"].as_u64().unwrap())
        .collect();
    assert_eq!(lines, vec![0, 1]);
}

#[test]
fn unicode_and_qualified_positions_work() {
    assert_eq!(
        symbol_at_position("🙂 message", &json!({"line": 0, "character": 3})),
        Some("message".to_string())
    );
    assert_eq!(
        symbol_at_position("object.message", &json!({"line": 0, "character": 9})),
        Some("message".to_string())
    );
}

#[test]
fn punctuation_symbols_are_searchable() {
    assert_eq!(find_matches("C++ uses C++", "C++", true).len(), 2);
}

#[test]
fn native_search_respects_ripgrep_filters() {
    let directory = TempDir::new();
    write_file(&directory.0.join("code.rs"), "needle\n");
    write_file(&directory.0.join("notes.txt"), "needle\n");
    write_file(&directory.0.join("excluded.rs"), "needle\n");
    write_file(&directory.0.join(".hidden.rs"), "needle\n");
    let mut config = ServerConfig::default();
    config.include = vec!["**/*.rs".to_string()];
    config.exclude = vec!["excluded.rs".to_string()];
    let results = WorkspaceSearcher {
        root: directory.0.clone(),
        config,
    }
    .search("needle", &HashMap::new());
    let paths: HashSet<_> = results
        .iter()
        .map(|result| result.path.file_name().unwrap().to_owned())
        .collect();

    assert_eq!(
        paths,
        HashSet::from(["code.rs".into(), ".hidden.rs".into(),])
    );
}

#[test]
fn incremental_changes_are_applied() {
    let updated = apply_change(
        "hello world",
        &json!({"range": {"start": {"line": 0, "character": 6}, "end": {"line": 0, "character": 11}}, "text": "Rust"}),
    );
    assert_eq!(updated, "hello Rust");
}

#[test]
fn stdio_round_trip_supports_initialize_and_definition() {
    let directory = TempDir::new();
    let source = directory.0.join("example.txt");
    write_file(&source, "Term: meaning\nUse Term here\n");
    let uri = uri_from_path(&source);
    let messages = vec![
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"rootUri": uri_from_path(&directory.0)}}),
        json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "text": "Term: meaning\nUse Term here\n"}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/definition", "params": {"textDocument": {"uri": uri}, "position": {"line": 1, "character": 5}}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "exit", "params": {}}),
    ];
    let mut input = Vec::new();
    for message in messages {
        let body = serde_json::to_vec(&message).unwrap();
        input.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        input.extend_from_slice(&body);
    }
    let mut reader = Cursor::new(input);
    let mut output = Vec::new();
    serve(
        &mut LspServer::new(Some(directory.0.clone())),
        &mut reader,
        &mut output,
    )
    .unwrap();
    let mut response_reader = Cursor::new(output);
    let mut responses = Vec::new();
    while let Some(response) = read_message(&mut response_reader).unwrap() {
        responses.push(response);
    }
    assert_eq!(
        responses
            .iter()
            .map(|response| response["id"].as_u64())
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3)]
    );
    assert_eq!(responses[1]["result"][0]["range"]["start"]["line"], 0);
}
