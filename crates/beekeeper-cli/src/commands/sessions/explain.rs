//! `bee sessions explain <word>` — the fold's vocabulary, from the binary.
//!
//! Every word the typed team fold prints — `unseated`, `dangling`, `waiting`,
//! `superseded`, and every exclusion code — is defined in
//! `beekeeper_core::team_vocabulary`, compiled in from a data file. This command is
//! the way a seat reads it.
//!
//! It reaches no relay and needs no key: a seat that has to ask what a word
//! means should not have to be authenticated, on a network, or in a checkout to
//! find out. See `crates/beekeeper-cli/TESTING.md` for the runbook entry.

use serde_json::{json, Value};

use beekeeper_core::team_vocabulary::{
    closest_word, lookup_word, team_vocabulary, TeamVocabularyEntry,
};

use crate::error::CliError;
use crate::OutputFormat;

/// `bee sessions explain [word]`.
///
/// With a word, prints that entry. With none, lists every word once. An unknown
/// word exits 1 naming the closest match.
///
/// # Errors
/// [`CliError::Usage`] (exit 1) for an unknown word, and for the authoring bug
/// where the compiled-in data file does not parse.
pub fn cmd_explain(word: Option<&str>, format: &OutputFormat) -> Result<(), CliError> {
    let compact = matches!(format, OutputFormat::Compact);
    match word {
        None => {
            let entries = team_vocabulary().map_err(data_error)?;
            let rows: Vec<Value> = entries
                .iter()
                .map(|entry| entry_json(entry, compact))
                .collect();
            println!("{}", Value::Array(rows));
        }
        Some(word) => {
            let found = lookup_word(word).map_err(data_error)?;
            let entry = match found {
                Some(entry) => entry,
                None => return Err(unknown_word(word)?),
            };
            println!("{}", entry_json(entry, compact));
        }
    }
    Ok(())
}

/// The refusal for a word nothing defines, with the closest spelling when one
/// is close enough to be a typo rather than a guess.
fn unknown_word(word: &str) -> Result<CliError, CliError> {
    let suggestion = closest_word(word).map_err(data_error)?;
    let message = match suggestion {
        Some(closest) => format!(
            "unknown word {word:?}: did you mean {closest:?}? \
             Run `bee sessions explain` with no argument to list every word."
        ),
        None => format!(
            "unknown word {word:?}. \
             Run `bee sessions explain` with no argument to list every word."
        ),
    };
    Ok(CliError::Usage(message))
}

/// One entry as JSON.
///
/// `--format compact` drops the two disclosure fields and keeps the four a
/// reader asked for: the word, what it means, what causes it, and the command
/// that shows it.
fn entry_json(entry: &TeamVocabularyEntry, compact: bool) -> Value {
    if compact {
        return json!({
            "word": entry.word,
            "meaning": entry.meaning,
            "cause": entry.cause,
            "command": entry.command,
        });
    }
    json!({
        "word": entry.word,
        "aliases": entry.aliases,
        "meaning": entry.meaning,
        "cause": entry.cause,
        "command": entry.command,
        // Present and null when the specification freezes no sentence about
        // this word — "nobody froze one" and "one exists" stay different
        // answers, and neither is invented here.
        "frozenSource": non_empty(&entry.frozen_source),
        "frozen": non_empty(&entry.frozen),
    })
}

/// `None` for an empty field, so the JSON says `null` rather than `""`.
fn non_empty(value: &str) -> Option<&str> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// The compiled-in data file failed to parse — an authoring bug, reported as
/// one rather than panicked on.
fn data_error(message: &'static str) -> CliError {
    CliError::Usage(format!(
        "the compiled-in team vocabulary could not be read: {message}"
    ))
}

#[cfg(test)]
#[path = "explain_tests.rs"]
mod tests;
