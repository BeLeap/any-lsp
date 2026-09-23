use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn normalize_path(path: PathBuf) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(&path) {
        return canonical;
    }
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

pub(crate) fn path_from_uri(uri: &str) -> PathBuf {
    let raw = if let Some(rest) = uri.strip_prefix("file://") {
        if let Some(rest) = rest.strip_prefix("localhost") {
            rest
        } else {
            rest
        }
    } else {
        uri
    };
    normalize_path(PathBuf::from(percent_decode(raw)))
}

fn percent_encode_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            result.push(*byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

pub(crate) fn uri_from_path(path: &Path) -> String {
    format!(
        "file://{}",
        percent_encode_path(&normalize_path(path.to_path_buf()).to_string_lossy())
    )
}

pub(crate) fn utf16_length(value: &str) -> usize {
    value.encode_utf16().count()
}

pub(crate) fn utf16_to_byte_index(value: &str, target: usize) -> usize {
    if target == 0 {
        return 0;
    }
    let mut units = 0;
    for (byte_index, character) in value.char_indices() {
        let next_units = units + character.len_utf16();
        if next_units > target {
            return byte_index;
        }
        units = next_units;
        if units == target {
            return byte_index + character.len_utf8();
        }
    }
    value.len()
}

fn utf16_to_char_index(value: &str, target: usize) -> usize {
    let mut units = 0;
    for (index, character) in value.chars().enumerate() {
        let next_units = units + character.len_utf16();
        if next_units > target {
            return index;
        }
        units = next_units;
        if units == target {
            return index + 1;
        }
    }
    value.chars().count()
}

fn position_line_and_index<'a>(text: &'a str, position: &Value) -> (&'a str, usize) {
    let line_number = position.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
    let character = position
        .get("character")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let lines: Vec<&str> = text.split('\n').collect();
    let line = lines
        .get(line_number.min(lines.len().saturating_sub(1)))
        .copied()
        .unwrap_or("");
    (line, utf16_to_char_index(line, character))
}

fn is_identifier_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '$')
}

fn generic_delimiter(character: char) -> bool {
    character.is_whitespace() || ",;:(){}[]<>\"'".contains(character)
}

fn token_at(line: &str, index: usize, identifier: bool) -> Option<String> {
    let characters: Vec<char> = line.chars().collect();
    if characters.is_empty() {
        return None;
    }
    let mut cursor = index.min(characters.len().saturating_sub(1));
    let predicate = |character: char| {
        if identifier {
            is_identifier_character(character)
        } else {
            !generic_delimiter(character)
        }
    };
    if !predicate(characters[cursor]) && cursor > 0 && predicate(characters[cursor - 1]) {
        cursor -= 1;
    }
    if !predicate(characters[cursor]) {
        return None;
    }
    let mut start = cursor;
    while start > 0 && predicate(characters[start - 1]) {
        start -= 1;
    }
    let mut end = cursor + 1;
    while end < characters.len() && predicate(characters[end]) {
        end += 1;
    }
    let token: String = characters[start..end].iter().collect();
    let trimmed = token.trim_matches(|character: char| ".,!?`~\"".contains(character));
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn symbol_at_position(text: &str, position: &Value) -> Option<String> {
    let (line, index) = position_line_and_index(text, position);
    token_at(line, index, true).or_else(|| token_at(line, index, false))
}
