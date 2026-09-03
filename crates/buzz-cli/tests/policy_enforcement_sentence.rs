//! The one sentence every surface rendering a kind-44245 policy owes its
//! reader, asserted byte-identical wherever it is written down.
//!
//! It is not a comment: `bee sessions policy get|set` prints it, the Tauri
//! policy adapter returns it, and the Mission policy panel renders it. Four
//! copies exist because the crates that need it do not depend on one another
//! (`buzz-cli`, the Tauri crate, the Desktop bundle) and because POLICY.md is
//! the document a person reads before setting a policy at all.
//!
//! Batch 3, REVIEW-L7 F1 is why this guard exists. Item G made
//! `gates.verifierRequired` enforced at the 44244 fold's completion check, and
//! six places still told the reader that `budget.turns` was the only enforced
//! field. A founder who set `--verifier-required true` would have been told by
//! `policy get` that nothing counted it, and then refused by
//! `bee sessions complete`. A sentence a product prints about what it enforces
//! is a claim, and a claim that has drifted from the code is a lie the tests
//! must catch — the same argument `pack_rules.rs` makes about the seat packs.
//!
//! Files are read as text rather than linked, exactly as `pack_rules.rs` reads
//! the shipped packs: a rule that only holds for a fixture is a rule nothing
//! obeys.

use std::path::{Path, PathBuf};

/// The sentence, byte-for-byte. Batch `00-BATCH.md` §1k carries the same text.
const ENFORCEMENT_SENTENCE: &str = "Enforced: budget.turns at the provider's turn gate, gates.verifierRequired at the fold's completion check and at the relay's verdict-gated push, and gates.requiredGates at that push. Every other field is read and shown, never counted.";

/// Wordings that must never come back: each one was true before item G and is
/// false now, and each was printed to a user.
const RETIRED_PHRASINGS: &[&str] = &[
    // Retired by lane L22 (2026-09-03): the relay's verdict-gated push now
    // reads `gates.verifierRequired` *and* `gates.requiredGates`, so a
    // surface still saying the flag stops at the completion check — or that
    // the push gate reads no policy — is telling a founder the switch has no
    // landing effect immediately before it decides their landing.
    "gates.verifierRequired at the fold's completion check. Every other field",
    "reads no session policy at all",
    "arm (B) is not implemented",
    "only budget.turns is enforced",
    "Exactly one field is enforced",
    "Only `budget.turns` is enforced anywhere",
    "The one enforced field",
    "Only the turn ceiling binds anything",
    "exactly one is enforced",
    "read and shown, and nothing checks them",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/buzz-cli has a grandparent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Normalise the three ways this repository wraps one long string.
///
/// Rust continues a literal with a trailing `\` and swallows the indent;
/// TypeScript concatenates with `" +`; Markdown just wraps. Strip those, then
/// collapse whitespace, so the comparison is about the sentence rather than
/// about where each language happened to break the line.
fn collapsed(text: &str) -> String {
    text.chars()
        .filter(|character| !matches!(character, '"' | '\\' | '+'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `haystack` contains the sentence once whitespace is normalised.
fn carries_the_sentence(haystack: &str) -> bool {
    collapsed(haystack).contains(&collapsed(ENFORCEMENT_SENTENCE))
}

#[test]
fn every_surface_states_the_same_enforced_fields() {
    // (file, what prints it)
    let sites: &[(&str, &str)] = &[
        (
            "crates/buzz-cli/src/commands/sessions/policy.rs",
            "`bee sessions policy get|set`'s own `enforcement` field",
        ),
        (
            "desktop/src-tauri/src/commands/coding_session_policy.rs",
            "the Tauri policy-fold adapter's `enforcement` field",
        ),
        (
            "desktop/src/features/coding-sessions/lib/codingSessionPolicy.ts",
            "the Desktop fallback the launch form and the Mission panel render",
        ),
        (
            "docs/design/portable-team-loop/POLICY.md",
            "POLICY.md §4.2, the document a founder reads before setting a policy",
        ),
        (
            "desktop/src/features/coding-sessions/hooks/useCodingSessionSessionPolicy.test.mjs",
            "the Desktop hook suite that pins the sentence",
        ),
        (
            "desktop/src/features/coding-sessions/lib/codingSessionMissionPolicyView.test.mjs",
            "the Mission policy-view suite that pins the sentence",
        ),
    ];
    for (relative, who) in sites {
        let body = read(relative);
        assert!(
            carries_the_sentence(&body),
            "{relative} does not carry the enforcement sentence byte-for-byte, and it is \
             {who}:\n{ENFORCEMENT_SENTENCE}"
        );
    }
}

#[test]
fn no_surface_still_claims_one_field_is_enforced() {
    // Every file that says anything about what a policy enforces.
    let sites: &[&str] = &[
        "crates/buzz-cli/src/commands/sessions/policy.rs",
        "crates/buzz-cli/src/lib.rs",
        "desktop/src-tauri/src/commands/coding_session_policy.rs",
        "desktop/src-tauri/src/commands/coding_session_policy_tests.rs",
        "desktop/src/features/coding-sessions/lib/codingSessionPolicy.ts",
        "desktop/src/features/coding-sessions/lib/codingSessionPolicy.test.mjs",
        "desktop/src/features/coding-sessions/hooks/useCodingSessionSessionPolicy.test.mjs",
        "desktop/src/features/coding-sessions/lib/codingSessionMissionPolicyView.test.mjs",
        "docs/nips/NIP-CSP.md",
        "docs/design/portable-team-loop/POLICY.md",
    ];
    for relative in sites {
        let body = read(relative);
        for retired in RETIRED_PHRASINGS {
            assert!(
                !body.contains(retired),
                "{relative} still carries the retired phrasing {retired:?}. \
                 `gates.verifierRequired` has been enforced at the 44244 fold's completion \
                 check since 2026-09-02 (REVIEW-L7 F1); a surface that says otherwise tells a \
                 founder nothing counts the field it is about to refuse them for."
            );
        }
    }
}

/// The list of enforced field names and the sentence must agree.
///
/// Three names in the sentence, three names in the list the Mission panel
/// marks rows with. A name in one and not the other is the drift this pair of
/// tests exists to stop.
#[test]
fn the_enforced_field_list_names_exactly_what_the_sentence_names() {
    let body = read("desktop/src/features/coding-sessions/lib/codingSessionPolicy.ts");
    let start = body
        .find("export const CODING_SESSION_POLICY_ENFORCED_FIELDS")
        .expect("the enforced-field list exists");
    let end = body[start..]
        .find("];")
        .map(|offset| start + offset)
        .expect("the list is terminated");
    let list = &body[start..end];
    for field in [
        "budget.turns",
        "gates.verifierRequired",
        "gates.requiredGates",
    ] {
        assert!(
            list.contains(field),
            "the enforced-field list does not name {field}, which the enforcement sentence does"
        );
        assert!(
            ENFORCEMENT_SENTENCE.contains(field),
            "the enforcement sentence does not name {field}, which the enforced-field list does"
        );
    }
}
