use crate::position::path_from_uri;
use crate::search::{find_matches, Occurrence};

fn starts_with_word(value: &str, words: &[&str]) -> bool {
    words.iter().any(|word| {
        value
            .get(..word.len())
            .map(|prefix| prefix.eq_ignore_ascii_case(word))
            .unwrap_or(false)
            && value
                .chars()
                .nth(word.chars().count())
                .map(|character| !character.is_alphanumeric())
                .unwrap_or(true)
    })
}

fn declaration_keyword(line: &str) -> bool {
    let mut rest = line.trim_start();
    let modifiers = [
        "export",
        "public",
        "private",
        "protected",
        "static",
        "async",
        "mut",
        "pub",
        "final",
        "readonly",
    ];
    while let Some(end) = rest.find(char::is_whitespace) {
        let word = &rest[..end];
        if !modifiers
            .iter()
            .any(|modifier| word.eq_ignore_ascii_case(modifier))
        {
            break;
        }
        rest = rest[end..].trim_start();
    }
    starts_with_word(
        rest,
        &[
            "def",
            "fn",
            "func",
            "function",
            "class",
            "struct",
            "enum",
            "interface",
            "trait",
            "type",
            "record",
            "module",
            "namespace",
            "macro",
            "concept",
            "template",
            "package",
        ],
    )
}

fn definition_score(occurrence: &Occurrence, symbol: &str) -> usize {
    let line = &occurrence.line_text;
    let trimmed = line.trim_start();
    let heading = trimmed.starts_with('#')
        && trimmed
            .chars()
            .take_while(|character| *character == '#')
            .count()
            <= 6
        && trimmed
            .chars()
            .nth(
                trimmed
                    .chars()
                    .take_while(|character| *character == '#')
                    .count(),
            )
            .map(|character| character.is_whitespace())
            .unwrap_or(false);
    let mut score = if heading { 2 } else { 0 };
    if declaration_keyword(line) {
        score = score.max(4);
    }

    let mut candidate = trimmed;
    if let Some(rest) = candidate
        .strip_prefix('-')
        .or_else(|| candidate.strip_prefix('*'))
    {
        candidate = rest.trim_start();
    }
    if let Some((_, end)) = find_matches(candidate, symbol, false)
        .into_iter()
        .find(|(start, _)| *start == 0)
    {
        let rest = candidate[end..].trim_start();
        if starts_with_word(
            rest,
            &["is", "means", "refers", "describes", "denotes", "stands"],
        ) || rest.starts_with(':')
            || rest.starts_with('=')
            || rest.starts_with('—')
            || rest.starts_with('-')
        {
            score = score.max(3);
        }
        if starts_with_word(rest, &["refers", "stands"]) {
            score = score.max(3);
        }
    }
    score
}

pub(crate) fn is_definition(occurrence: &Occurrence, symbol: &str) -> bool {
    definition_score(occurrence, symbol) > 0
}

pub(crate) fn same_document(left: &str, right: &str) -> bool {
    path_from_uri(left) == path_from_uri(right)
}

pub(crate) fn rank_definitions(
    occurrences: &[Occurrence],
    symbol: &str,
    current_uri: Option<&str>,
) -> Vec<Occurrence> {
    let scored: Vec<Occurrence> = occurrences
        .iter()
        .filter(|item| is_definition(item, symbol))
        .cloned()
        .collect();
    if scored.is_empty() {
        return occurrences.first().cloned().into_iter().collect();
    }
    let local: Vec<Occurrence> = current_uri
        .map(|uri| {
            scored
                .iter()
                .filter(|item| same_document(&item.uri, uri))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let candidates = if local.is_empty() { scored } else { local };
    let best_score = candidates
        .iter()
        .map(|item| definition_score(item, symbol))
        .max()
        .unwrap_or(0);
    candidates
        .into_iter()
        .filter(|item| definition_score(item, symbol) == best_score)
        .collect()
}
