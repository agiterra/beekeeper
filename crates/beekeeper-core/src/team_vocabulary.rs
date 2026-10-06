//! The team-fold vocabulary, carried in the binary.
//!
//! Every word the typed team fold puts in front of a seat — `unseated`,
//! `waiting`, `dangling`, `superseded`, and every exclusion code — is defined
//! in `vocabulary/team-fold.toml`, which is `include_str!`-ed here and parsed
//! once. A reader with no checkout gets the same answer as a reader with one:
//! `bee sessions explain <word>` reads this, never a source tree.
//!
//! The reason is a measured one. On 2026-09-01 a lead spent its own context
//! grepping the fold's Rust source for the string `"unseated"`, because that
//! was the only place the word its own tool had just printed was defined.
//!
//! Two guards keep this honest, both in `team_vocabulary_tests.rs`:
//!
//! 1. [`fold_exclusion_wire_code`] matches on
//!    [`CodingSessionTeamFoldExclusionCode`] **exhaustively**, so a new
//!    exclusion code cannot compile `buzz-core` until someone visits this file;
//!    the test then asserts every code in [`FOLD_EXCLUSION_CODES`] has an entry.
//! 2. Every `frozen` sentence in the TOML is compared byte-for-byte against
//!    `vocabulary/team-fold.quotes.txt`, the checked-in copy of the batch
//!    specification's frozen vocabulary.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::coding_session_team_transaction::CodingSessionTeamFoldExclusionCode;

/// The vocabulary data file, compiled into the binary.
const VOCABULARY_TOML: &str = include_str!("vocabulary/team-fold.toml");

/// The checked-in copy of the frozen sentences the TOML quotes.
const FROZEN_QUOTES: &str = include_str!("vocabulary/team-fold.quotes.txt");

/// One vocabulary entry: a word, what it means, what causes it, and the one
/// command that shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamVocabularyEntry {
    /// The canonical word, as a reader meets it.
    pub word: String,
    /// Other spellings that resolve to this entry — wire codes, the Rust
    /// `Debug` spelling, the camelCase JSON key.
    pub aliases: Vec<String>,
    /// One sentence: what the word means.
    pub meaning: String,
    /// What makes the word appear.
    pub cause: String,
    /// The one command that shows it.
    pub command: String,
    /// Label of the frozen sentence this entry quotes, or empty when the
    /// specification freezes none.
    pub frozen_source: String,
    /// That sentence, byte-for-byte, or empty.
    pub frozen: String,
}

/// Every exclusion code the fold can emit, in the order a reader meets them.
///
/// Kept beside [`fold_exclusion_wire_code`] deliberately: that function's match
/// is exhaustive, so adding a variant to
/// [`CodingSessionTeamFoldExclusionCode`] breaks this crate's build until the
/// author lands here, and `team_vocabulary_tests.rs` then fails until the word
/// is written too.
pub const FOLD_EXCLUSION_CODES: &[CodingSessionTeamFoldExclusionCode] = &[
    CodingSessionTeamFoldExclusionCode::Unauthorized,
    CodingSessionTeamFoldExclusionCode::DanglingReference,
    CodingSessionTeamFoldExclusionCode::WrongTypeReference,
    CodingSessionTeamFoldExclusionCode::InvalidCorrection,
    CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized,
    CodingSessionTeamFoldExclusionCode::DependentOnSuperseded,
    CodingSessionTeamFoldExclusionCode::DependentOnExcluded,
    CodingSessionTeamFoldExclusionCode::Superseded,
    CodingSessionTeamFoldExclusionCode::CorrectionConflict,
    CodingSessionTeamFoldExclusionCode::CompletionNotApproved,
    CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision,
    CodingSessionTeamFoldExclusionCode::CompletionNotVerified,
    CodingSessionTeamFoldExclusionCode::TerminalConflict,
];

/// The snake_case wire spelling of one exclusion code.
///
/// This match is exhaustive on purpose. A new exclusion code stops the build
/// here, one file away from the word that has to be written for it.
pub fn fold_exclusion_wire_code(code: CodingSessionTeamFoldExclusionCode) -> &'static str {
    match code {
        CodingSessionTeamFoldExclusionCode::Unauthorized => "unauthorized",
        CodingSessionTeamFoldExclusionCode::DanglingReference => "dangling_reference",
        CodingSessionTeamFoldExclusionCode::WrongTypeReference => "wrong_type_reference",
        CodingSessionTeamFoldExclusionCode::InvalidCorrection => "invalid_correction",
        CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized => "dependent_on_unauthorized",
        CodingSessionTeamFoldExclusionCode::DependentOnSuperseded => "dependent_on_superseded",
        CodingSessionTeamFoldExclusionCode::DependentOnExcluded => "dependent_on_excluded",
        CodingSessionTeamFoldExclusionCode::Superseded => "superseded",
        CodingSessionTeamFoldExclusionCode::CorrectionConflict => "correction_conflict",
        CodingSessionTeamFoldExclusionCode::CompletionNotApproved => "completion_not_approved",
        CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision => {
            "completion_blocked_by_open_decision"
        }
        CodingSessionTeamFoldExclusionCode::CompletionNotVerified => "completion_not_verified",
        CodingSessionTeamFoldExclusionCode::TerminalConflict => "terminal_conflict",
    }
}

/// The parsed vocabulary, or the parse error that stopped it.
fn vocabulary_once() -> &'static Result<Vec<TeamVocabularyEntry>, String> {
    static VOCABULARY: OnceLock<Result<Vec<TeamVocabularyEntry>, String>> = OnceLock::new();
    VOCABULARY.get_or_init(|| parse_vocabulary(VOCABULARY_TOML))
}

/// Every vocabulary entry, in file order.
///
/// # Errors
/// Returns the parse diagnostic when the compiled-in data file is not the
/// strict subset [`parse_vocabulary`] accepts. That is an authoring bug the
/// test suite catches; callers surface it rather than panicking on it.
pub fn team_vocabulary() -> Result<&'static [TeamVocabularyEntry], &'static str> {
    match vocabulary_once() {
        Ok(entries) => Ok(entries.as_slice()),
        Err(message) => Err(message.as_str()),
    }
}

/// Look one word up by its canonical spelling or any of its aliases.
///
/// Matching is case-insensitive so `explain DanglingReference` and
/// `explain danglingreference` both land.
///
/// # Errors
/// Propagates a data-file parse failure from [`team_vocabulary`].
pub fn lookup_word(word: &str) -> Result<Option<&'static TeamVocabularyEntry>, &'static str> {
    let needle = word.trim().to_ascii_lowercase();
    for entry in team_vocabulary()? {
        if entry.word.to_ascii_lowercase() == needle
            || entry
                .aliases
                .iter()
                .any(|alias| alias.to_ascii_lowercase() == needle)
        {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// The closest known spelling to `word`, for a "did you mean" line.
///
/// Returns `None` when nothing is within an edit distance of a third of the
/// word's length — a wild miss gets the full list rather than a bad guess.
///
/// # Errors
/// Propagates a data-file parse failure from [`team_vocabulary`].
pub fn closest_word(word: &str) -> Result<Option<&'static str>, &'static str> {
    let needle = word.trim().to_ascii_lowercase();
    let budget = std::cmp::max(2, needle.chars().count() / 3);
    let mut best: Option<(usize, &'static str)> = None;
    for entry in team_vocabulary()? {
        for candidate in std::iter::once(&entry.word).chain(entry.aliases.iter()) {
            let distance = edit_distance(&needle, &candidate.to_ascii_lowercase());
            if distance <= budget && best.is_none_or(|(seen, _)| distance < seen) {
                best = Some((distance, entry.word.as_str()));
            }
        }
    }
    Ok(best.map(|(_, word)| word))
}

/// Every frozen sentence in the checked-in snapshot, as `(label, sentence)`,
/// in file order.
///
/// # Errors
/// Returns the diagnostic when the snapshot file is malformed.
pub fn frozen_sentences() -> Result<&'static [(String, String)], &'static str> {
    static QUOTES: OnceLock<Result<Vec<(String, String)>, String>> = OnceLock::new();
    match QUOTES.get_or_init(|| parse_frozen_quotes(FROZEN_QUOTES)) {
        Ok(blocks) => Ok(blocks.as_slice()),
        Err(message) => Err(message.as_str()),
    }
}

/// One frozen sentence from the checked-in snapshot, by its label.
///
/// The single source for a sentence the specification froze: the CLI's `--help`
/// rules and this crate's vocabulary both read it here, so the two can never
/// drift into different wordings of the same rule.
///
/// # Errors
/// Returns the diagnostic when the snapshot file is malformed.
pub fn frozen_sentence(label: &str) -> Result<Option<&'static str>, &'static str> {
    Ok(frozen_sentences()?
        .iter()
        .find(|(name, _)| name == label)
        .map(|(_, sentence)| sentence.as_str()))
}

// ── The strict reader ────────────────────────────────────────────────────────

/// Parse the `[[entry]]` subset of TOML this data file uses.
///
/// Deliberately not a TOML library: `buzz-core` carries no I/O and no parser
/// dependencies, and the file is one this repository writes and this test suite
/// checks. Anything outside the subset — an inline table, a multi-line string,
/// a bare key, a number — is an error naming the line, never a silent skip.
fn parse_vocabulary(source: &str) -> Result<Vec<TeamVocabularyEntry>, String> {
    let mut entries: Vec<TeamVocabularyEntry> = Vec::new();
    let mut current: Option<Vec<(String, Vec<String>)>> = None;

    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        let number = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[entry]]" {
            if let Some(fields) = current.take() {
                entries.push(build_entry(fields, number)?);
            }
            current = Some(Vec::new());
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            return Err(format!(
                "line {number}: expected `key = value`, got {line:?}"
            ));
        };
        let Some(fields) = current.as_mut() else {
            return Err(format!(
                "line {number}: `{key}` appears before any [[entry]]"
            ));
        };
        let key = key.trim();
        // Real TOML refuses a duplicate key, and so does this: `build_entry`
        // below takes the *first* match, so a second `frozen = ...` pasted under
        // the first would leave the binary quoting the old sentence while the
        // file reads as the new one, with every test still green (REVIEW-L13 F3).
        if fields.iter().any(|(seen, _)| seen == key) {
            return Err(format!(
                "line {number}: `{key}` appears twice in one entry; a duplicate key would \
                 be dropped silently and the first value kept"
            ));
        }
        let values = parse_value(value.trim(), number)?;
        fields.push((key.to_string(), values));
    }
    if let Some(fields) = current.take() {
        entries.push(build_entry(fields, source.lines().count())?);
    }
    if entries.is_empty() {
        return Err("the vocabulary file declares no [[entry]]".to_string());
    }
    Ok(entries)
}

/// A basic string, or a single-line array of basic strings.
fn parse_value(value: &str, line: usize) -> Result<Vec<String>, String> {
    if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        let inner = inner.trim();
        if inner.is_empty() {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        for item in inner.split(',') {
            items.push(parse_basic_string(item.trim(), line)?);
        }
        return Ok(items);
    }
    Ok(vec![parse_basic_string(value, line)?])
}

/// One TOML basic string, with the four escapes this file is allowed to use.
fn parse_basic_string(value: &str, line: usize) -> Result<String, String> {
    let Some(body) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return Err(format!(
            "line {line}: expected a double-quoted basic string, got {value:?}"
        ));
    };
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            other => {
                return Err(format!(
                    "line {line}: unsupported escape \\{} in a vocabulary string",
                    other.unwrap_or(' ')
                ))
            }
        }
    }
    Ok(out)
}

/// Turn one entry's collected fields into a [`TeamVocabularyEntry`].
fn build_entry(
    fields: Vec<(String, Vec<String>)>,
    line: usize,
) -> Result<TeamVocabularyEntry, String> {
    let single = |name: &str| -> Result<String, String> {
        let found = fields.iter().find(|(key, _)| key == name);
        match found {
            Some((_, values)) if values.len() == 1 => Ok(values[0].clone()),
            Some((_, _)) => Err(format!(
                "entry ending at line {line}: `{name}` must be one string"
            )),
            None => Err(format!("entry ending at line {line}: `{name}` is missing")),
        }
    };
    let aliases = fields
        .iter()
        .find(|(key, _)| key == "aliases")
        .map(|(_, values)| values.clone())
        .ok_or_else(|| format!("entry ending at line {line}: `aliases` is missing"))?;

    let known: BTreeSet<&str> = [
        "word",
        "aliases",
        "meaning",
        "cause",
        "command",
        "frozen_source",
        "frozen",
    ]
    .into_iter()
    .collect();
    for (key, _) in &fields {
        if !known.contains(key.as_str()) {
            return Err(format!(
                "entry ending at line {line}: unknown field `{key}`; the shape is frozen"
            ));
        }
    }

    Ok(TeamVocabularyEntry {
        word: single("word")?,
        aliases,
        meaning: single("meaning")?,
        cause: single("cause")?,
        command: single("command")?,
        frozen_source: single("frozen_source")?,
        frozen: single("frozen")?,
    })
}

/// Parse the `== label ==` blocks of the frozen-sentence snapshot.
fn parse_frozen_quotes(source: &str) -> Result<Vec<(String, String)>, String> {
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut pending: Option<String> = None;
    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim_end();
        let number = index + 1;
        if let Some(label) = line
            .strip_prefix("== ")
            .and_then(|rest| rest.strip_suffix(" =="))
        {
            if pending.is_some() {
                return Err(format!(
                    "line {number}: label {label:?} follows an empty block"
                ));
            }
            pending = Some(label.to_string());
            continue;
        }
        if let Some(label) = pending.take() {
            if line.trim().is_empty() {
                return Err(format!("line {number}: block {label:?} has no sentence"));
            }
            blocks.push((label, line.to_string()));
        }
    }
    if let Some(label) = pending {
        return Err(format!("block {label:?} has no sentence"));
    }
    if blocks.is_empty() {
        return Err("the frozen-sentence snapshot declares no blocks".to_string());
    }
    Ok(blocks)
}

/// Levenshtein distance, iterative and allocation-light.
fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current: Vec<usize> = vec![0; right.len() + 1];
    for (i, l) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, r) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(l != r);
            let insertion = current[j] + 1;
            let deletion = previous[j + 1] + 1;
            current[j + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
#[path = "team_vocabulary_tests.rs"]
mod tests;
