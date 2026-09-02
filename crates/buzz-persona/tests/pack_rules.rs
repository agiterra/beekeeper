//! Rules the shipped packs under `personas/roles/` must satisfy.
//!
//! Unlike `integration.rs`, which builds synthetic packs in a temp directory,
//! this suite reads the packs this repository actually ships. A rule that only
//! holds for a fixture is a rule no seat ever obeys.
//!
//! Batch 3 L1.4: a seat that pushes with the full pre-push gate attached hits
//! the 60 s tool timeout (live run 2, Bob 11:01) and, worse, the NIP-98
//! credential git minted at ref discovery expires inside a long hook window
//! (`docs/INTEGRATION.md` § "Landing a batch"). The hook budget is deliberately
//! **not** raised — the hooks are what make a green SHA green — so every seat
//! pack has to carry the rule humans already follow.

use std::path::{Path, PathBuf};

use buzz_persona::pack;

/// The verbatim sentence every seat pack's `push-your-lane` skill must carry.
///
/// Byte-for-byte from `review-2026-09-01/batch3/LANE-L1.md` §L1.4. It is
/// asserted rather than merely documented because four packs drifting apart by
/// one word is exactly how "the rule is in the pack" becomes untrue.
const PUSH_RULE: &str = "Run the floor first, on the exact SHA you are about to push: cargo fmt --check, cargo clippy --all-targets -- -D warnings and the unit tests for every crate you touched, plus pnpm typecheck && pnpm test if you touched desktop, plus anything else your brief names. Only then push with the hooks skipped, on that identical SHA: git push --no-verify origin <branch>. Never --no-verify on a SHA no gate has run against, and never push to GitHub directly: origin is the relay and the bridge mirrors it.";

/// Phrasings the rule must NOT carry any more.
///
/// REVIEW-L1 F7: "the one your brief names" let a brief that named
/// `cargo test -p buzz-core` stand in for the whole pre-push gate, so a seat
/// could push, with `--no-verify`, a SHA no clippy, typecheck or file-size
/// gate had ever seen. The floor is now the repo's fast gate; a brief adds to
/// it and can never shrink it.
const RETIRED_PHRASINGS: &[&str] = &[
    "Run your own gate first",
    "the one your brief names",
    "the tests and gates your brief names",
];

/// The roles that take a brief, do work, and push a lane branch.
///
/// The lead is deliberately absent: it already carries the rule in
/// `personas/roles/lead/skills/beekeeper-project/SKILL.md`, and a seat loads
/// only its own pack.
const SEAT_ROLES: &[&str] = &["builder", "runner", "verifier", "designer"];

/// Repository root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/buzz-persona has a grandparent")
        .to_path_buf()
}

#[test]
fn every_seat_pack_carries_the_push_rule_verbatim() {
    let roles_dir = repo_root().join("personas").join("roles");
    for role in SEAT_ROLES {
        let skill = roles_dir
            .join(role)
            .join("skills")
            .join("push-your-lane")
            .join("SKILL.md");
        assert!(
            skill.is_file(),
            "role {role} carries no push-your-lane skill at {}: a seat loads only its own pack, \
             so the rule has to be in each one",
            skill.display()
        );
        let body = std::fs::read_to_string(&skill)
            .unwrap_or_else(|error| panic!("reading {}: {error}", skill.display()));
        assert!(
            body.contains(PUSH_RULE),
            "role {role}'s push-your-lane skill does not carry the rule byte-for-byte:\n{body}"
        );
        for retired in RETIRED_PHRASINGS {
            assert!(
                !body.contains(retired),
                "role {role}'s push-your-lane skill still carries the retired phrasing \
                 {retired:?}, which let a narrow brief stand in for the whole gate \
                 (REVIEW-L1 F7)"
            );
        }
        assert!(
            body.contains("docs/INTEGRATION.md"),
            "role {role}'s push-your-lane skill must cite docs/INTEGRATION.md for why the \
             credential expires inside a long hook window"
        );
    }
}

#[test]
fn every_shipped_role_pack_still_loads() {
    let roles_dir = repo_root().join("personas").join("roles");
    let mut loaded = 0;
    for entry in std::fs::read_dir(&roles_dir).expect("personas/roles is readable") {
        let entry = entry.expect("a readable directory entry");
        if !entry.path().is_dir() {
            continue;
        }
        let pack = pack::load_pack(&entry.path()).unwrap_or_else(|error| {
            panic!("pack {} failed to load: {error}", entry.path().display())
        });
        assert!(
            !pack.personas.is_empty(),
            "pack {} declares no persona",
            entry.path().display()
        );
        loaded += 1;
    }
    assert!(
        loaded >= SEAT_ROLES.len(),
        "expected at least the four seat packs under personas/roles, found {loaded}"
    );
}
