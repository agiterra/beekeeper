//! `bee sessions explain` — the entry shape, and the two refusals.

use super::*;

use crate::error::exit_code;

/// The full-JSON entry for one word, as the command would print it.
fn entry(word: &str) -> Value {
    let found = lookup_word(word)
        .expect("the compiled-in vocabulary parses")
        .expect("the word is defined");
    entry_json(found, false)
}

#[test]
fn an_entry_carries_the_four_things_a_reader_asked_for() {
    let value = entry("unseated");
    assert_eq!(value["word"], "unseated");
    assert!(
        value["meaning"]
            .as_str()
            .is_some_and(|text| text.contains("no active seat")),
        "meaning: {}",
        value["meaning"]
    );
    assert!(value["cause"].as_str().is_some_and(|text| !text.is_empty()));
    assert!(
        value["command"]
            .as_str()
            .is_some_and(|text| text.starts_with("bee ")),
        "command: {}",
        value["command"]
    );
}

#[test]
fn a_frozen_sentence_is_disclosed_and_its_absence_is_null_not_empty() {
    let quoted = entry("unseated");
    assert!(quoted["frozen"].is_string(), "unseated quotes a sentence");
    assert!(quoted["frozenSource"].is_string());

    let unquoted = entry("dangling");
    assert!(
        unquoted["frozen"].is_null(),
        "no sentence was frozen for `dangling`, and null says so; an empty string would not"
    );
    assert!(unquoted["frozenSource"].is_null());
}

#[test]
fn compact_drops_the_disclosure_fields_and_keeps_the_four() {
    let found = lookup_word("waiting")
        .expect("the compiled-in vocabulary parses")
        .expect("the word is defined");
    let value = entry_json(found, true);
    let object = value.as_object().expect("an object");
    assert_eq!(object.len(), 4, "compact prints four keys, got {object:?}");
    for key in ["word", "meaning", "cause", "command"] {
        assert!(object.contains_key(key), "compact dropped {key}");
    }
}

#[test]
fn a_wire_code_and_a_debug_spelling_reach_the_same_entry() {
    assert_eq!(entry("dangling_reference")["word"], "dangling");
    assert_eq!(entry("DanglingReference")["word"], "dangling");
}

#[test]
fn an_unknown_word_exits_one_naming_the_closest_match() {
    let error = unknown_word("unseted").expect("the suggestion is computed");
    let message = error.to_string();
    assert!(
        message.contains("did you mean \"unseated\""),
        "the refusal must name the closest word, got {message:?}"
    );
    assert_eq!(exit_code(&error), 1);
}

#[test]
fn a_word_nothing_is_close_to_gets_no_guess() {
    let error = unknown_word("postgres").expect("the suggestion is computed");
    let message = error.to_string();
    assert!(
        !message.contains("did you mean"),
        "a wild miss must not be answered with a confident guess, got {message:?}"
    );
    assert!(message.contains("list every word"), "got {message:?}");
    assert_eq!(exit_code(&error), 1);
}

#[test]
fn listing_prints_every_word_once() {
    let entries = team_vocabulary().expect("the compiled-in vocabulary parses");
    let mut words: Vec<&str> = entries.iter().map(|entry| entry.word.as_str()).collect();
    let before = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), before, "a word is listed twice");
    assert!(
        words.contains(&"unseated") && words.contains(&"waiting"),
        "the listing must carry the words the live run had to grep for"
    );
}

#[test]
fn explain_needs_no_relay_and_no_key() {
    // The signature is the proof: `cmd_explain` takes no client and is not
    // async. If a future edit makes it reach the relay, this stops compiling.
    let run: fn(Option<&str>, &OutputFormat) -> Result<(), CliError> = cmd_explain;
    assert!(run(Some("unseated"), &OutputFormat::Compact).is_ok());
    assert!(run(None, &OutputFormat::Json).is_ok());
}
