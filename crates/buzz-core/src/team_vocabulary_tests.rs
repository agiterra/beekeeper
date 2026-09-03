//! Guards on the compiled-in team-fold vocabulary.
//!
//! The one that matters is [`every_fold_exclusion_code_has_a_word`]: it is the
//! reason a new exclusion code cannot reach a live run with nobody able to say
//! what it means. Delete an entry from `vocabulary/team-fold.toml` and it names
//! the missing code.

use super::*;

/// The number of `== label ==` blocks in the frozen-sentence snapshot.
///
/// Frozen so that adding or removing a quote is a deliberate act with a diff on
/// this line, rather than something a reviewer has to notice.
const FROZEN_QUOTE_BLOCKS: usize = 6;

#[test]
fn the_vocabulary_file_parses() {
    let entries = team_vocabulary().expect("the compiled-in vocabulary parses");
    assert!(
        entries.len() >= 15,
        "expected at least the four concept words plus every exclusion code, got {}",
        entries.len()
    );
    for entry in entries {
        assert!(!entry.word.is_empty(), "an entry has no word");
        assert!(
            !entry.meaning.is_empty(),
            "{}: a word with no meaning is a word a seat still has to grep for",
            entry.word
        );
        assert!(!entry.cause.is_empty(), "{}: no cause", entry.word);
        assert!(
            entry.command.starts_with("bee "),
            "{}: `command` must be a runnable bee line, got {:?}",
            entry.word,
            entry.command
        );
    }
}

/// No spelling reaches two entries.
///
/// Within one entry a spelling may repeat case-insensitively — `superseded`
/// is both the word a reader meets and, capitalised, the Rust `Debug` spelling
/// of the code, and both must resolve. Across two entries it never may, or
/// `lookup_word` would answer with whichever came first in the file.
#[test]
fn no_spelling_reaches_two_entries() {
    let entries = team_vocabulary().expect("the compiled-in vocabulary parses");
    let mut seen: Vec<(String, String)> = Vec::new();
    for entry in entries {
        let mut mine: Vec<String> = Vec::new();
        for spelling in std::iter::once(&entry.word).chain(entry.aliases.iter()) {
            let lowered = spelling.to_ascii_lowercase();
            if let Some((owner, _)) = seen.iter().find(|(_, other)| other == &lowered) {
                panic!(
                    "{spelling:?} resolves to both {owner:?} and {:?}; a word means one thing",
                    entry.word
                );
            }
            if !mine.contains(&lowered) {
                mine.push(lowered);
            }
        }
        for lowered in mine {
            seen.push((entry.word.clone(), lowered));
        }
    }
}

/// The guard the lane exists for.
///
/// `fold_exclusion_wire_code` matches exhaustively, so a new
/// `CodingSessionTeamFoldExclusionCode` variant already breaks the build one
/// file from here. This is the second half: the code must also have a word.
#[test]
fn every_fold_exclusion_code_has_a_word() {
    for code in FOLD_EXCLUSION_CODES {
        let wire = fold_exclusion_wire_code(*code);
        let entry = lookup_word(wire)
            .expect("the compiled-in vocabulary parses")
            .unwrap_or_else(|| {
                panic!(
                    "the fold can emit exclusion code {wire:?} and \
                     crates/buzz-core/src/vocabulary/team-fold.toml defines no word for it: \
                     a seat meeting it has nowhere to look but the source"
                )
            });
        assert!(
            !entry.meaning.is_empty(),
            "{wire}: the entry exists but says nothing"
        );
    }
}

#[test]
fn the_exclusion_code_list_covers_every_variant_the_match_knows() {
    let mut wire: Vec<&str> = FOLD_EXCLUSION_CODES
        .iter()
        .map(|code| fold_exclusion_wire_code(*code))
        .collect();
    let before = wire.len();
    wire.sort_unstable();
    wire.dedup();
    assert_eq!(
        wire.len(),
        before,
        "FOLD_EXCLUSION_CODES lists a code twice: {wire:?}"
    );
    assert_eq!(
        before, 13,
        "the fold's exclusion codes changed. Add the variant to \
         FOLD_EXCLUSION_CODES, give it an arm in fold_exclusion_wire_code, and \
         write its word in vocabulary/team-fold.toml — in that order."
    );
}

#[test]
fn the_four_words_the_live_run_asked_for_are_defined() {
    for word in ["unseated", "dangling", "waiting", "superseded"] {
        assert!(
            lookup_word(word)
                .expect("the compiled-in vocabulary parses")
                .is_some(),
            "{word:?} is a word the fold prints and this file must define"
        );
    }
}

#[test]
fn aliases_resolve_and_are_case_insensitive() {
    let by_alias = lookup_word("DanglingReference")
        .expect("the compiled-in vocabulary parses")
        .expect("the Rust Debug spelling resolves");
    assert_eq!(by_alias.word, "dangling");
    let by_camel = lookup_word("unseatedReports")
        .expect("the compiled-in vocabulary parses")
        .expect("the JSON key resolves");
    assert_eq!(by_camel.word, "unseated");
}

#[test]
fn a_near_miss_gets_the_closest_word() {
    assert_eq!(
        closest_word("unseted").expect("the compiled-in vocabulary parses"),
        Some("unseated")
    );
    assert_eq!(
        closest_word("dangeling").expect("the compiled-in vocabulary parses"),
        Some("dangling")
    );
}

#[test]
fn a_wild_miss_suggests_nothing_rather_than_guessing() {
    assert_eq!(
        closest_word("postgres").expect("the compiled-in vocabulary parses"),
        None
    );
}

/// Every quoted sentence is byte-identical to the checked-in copy.
#[test]
fn every_frozen_sentence_matches_the_snapshot_byte_for_byte() {
    let entries = team_vocabulary().expect("the compiled-in vocabulary parses");
    let mut quoted = 0;
    for entry in entries {
        assert_eq!(
            entry.frozen_source.is_empty(),
            entry.frozen.is_empty(),
            "{}: `frozen` and `frozen_source` are written together or not at all",
            entry.word
        );
        if entry.frozen_source.is_empty() {
            continue;
        }
        quoted += 1;
        let snapshot = frozen_sentence(&entry.frozen_source)
            .expect("the snapshot parses")
            .unwrap_or_else(|| {
                panic!(
                    "{}: frozen_source {:?} names no block in \
                     crates/buzz-core/src/vocabulary/team-fold.quotes.txt",
                    entry.word, entry.frozen_source
                )
            });
        assert_eq!(
            entry.frozen, snapshot,
            "{}: the quoted sentence has drifted from the checked-in copy",
            entry.word
        );
    }
    assert!(
        quoted >= 3,
        "expected the specification's frozen sentences to be quoted, found {quoted}"
    );
}

#[test]
fn the_snapshot_is_well_formed_and_its_size_is_frozen() {
    let blocks = frozen_sentences().expect("the snapshot parses");
    assert_eq!(
        blocks.len(),
        FROZEN_QUOTE_BLOCKS,
        "the frozen-sentence snapshot changed size; update FROZEN_QUOTE_BLOCKS \
         in the same commit so the change is visible in the diff"
    );
    let mut labels: Vec<&str> = blocks.iter().map(|(label, _)| label.as_str()).collect();
    let before = labels.len();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), before, "two snapshot blocks share a label");
    for (label, sentence) in blocks {
        assert!(
            !sentence.trim().is_empty(),
            "snapshot block {label:?} has an empty sentence"
        );
    }
}

// ── The reader itself ────────────────────────────────────────────────────────

#[test]
fn the_reader_refuses_a_line_outside_its_subset() {
    let error = parse_vocabulary("[[entry]]\nword = 3\n").expect_err("a bare number is refused");
    assert!(
        error.contains("basic string"),
        "the diagnostic must name the shape it wanted, got {error:?}"
    );
}

#[test]
fn the_reader_refuses_a_field_before_any_entry() {
    let error = parse_vocabulary("word = \"unseated\"\n").expect_err("a stray field is refused");
    assert!(error.contains("before any [[entry]]"), "got {error:?}");
}

#[test]
fn the_reader_refuses_an_unknown_field() {
    let source = concat!(
        "[[entry]]\n",
        "word = \"x\"\naliases = []\nmeaning = \"m\"\ncause = \"c\"\n",
        "command = \"bee sessions explain x\"\nfrozen_source = \"\"\nfrozen = \"\"\n",
        "colour = \"blue\"\n"
    );
    let error = parse_vocabulary(source).expect_err("an unknown field is refused");
    assert!(error.contains("unknown field"), "got {error:?}");
}

/// REVIEW-L13 F3. Real TOML refuses a duplicate key; this reader used to take
/// the first and drop the second without a word. The consequence was exactly
/// the quiet drift this file exists to prevent: paste a corrected `frozen =`
/// under the old one and the binary keeps quoting the old sentence while the
/// file reads as the new one, with every test still green.
#[test]
fn the_reader_refuses_a_duplicate_key_naming_the_line() {
    let source = concat!(
        "[[entry]]\n",
        "word = \"first\"\n",
        "word = \"second\"\n",
        "aliases = []\nmeaning = \"m\"\ncause = \"c\"\n",
        "command = \"bee sessions explain first\"\nfrozen_source = \"\"\nfrozen = \"\"\n"
    );
    let error = parse_vocabulary(source).expect_err("a duplicate key is refused");
    assert!(
        error.contains("line 3") && error.contains("appears twice"),
        "the diagnostic must name the line and the key, got {error:?}"
    );
}

#[test]
fn the_reader_refuses_a_missing_field() {
    let error = parse_vocabulary("[[entry]]\nword = \"x\"\n")
        .expect_err("an entry missing every other field is refused");
    assert!(error.contains("is missing"), "got {error:?}");
}

#[test]
fn the_reader_keeps_escaped_quotes() {
    let source = concat!(
        "[[entry]]\n",
        "word = \"x\"\naliases = [\"y\", \"z\"]\nmeaning = \"a \\\"quoted\\\" word\"\n",
        "cause = \"c\"\ncommand = \"bee sessions explain x\"\n",
        "frozen_source = \"\"\nfrozen = \"\"\n"
    );
    let entries = parse_vocabulary(source).expect("the subset parses");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].meaning, "a \"quoted\" word");
    assert_eq!(entries[0].aliases, vec!["y".to_string(), "z".to_string()]);
}

#[test]
fn edit_distance_is_the_usual_one() {
    assert_eq!(edit_distance("", ""), 0);
    assert_eq!(edit_distance("unseated", "unseated"), 0);
    assert_eq!(edit_distance("unseted", "unseated"), 1);
    assert_eq!(edit_distance("kitten", "sitting"), 3);
}
