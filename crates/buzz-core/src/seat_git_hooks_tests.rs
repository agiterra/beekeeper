//! Tests for the seat git-hook plan.
//!
//! The property under test throughout is the one Brian ruled on: **nothing in
//! Pulse's local-commit path may depend on anyone being asked to report**. The
//! ref a commit lands on is *derived* from the seat's role and its assignment
//! id — both of which the hire host already knows — and never read out of
//! anything an agent wrote. These tests are where that stops being a comment.

use super::*;

const SEAT: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const ASSIGNMENT: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

fn request() -> SeatGitHookRequest {
    SeatGitHookRequest {
        seat_role: "Refuter".to_string(),
        seat_pubkey: SEAT.to_string(),
        keyfile_path: Some("/tmp/seat/key".to_string()),
        signer_program: "git-sign-nostr".to_string(),
        assignment_id: Some(ASSIGNMENT.to_string()),
        session_ref: Some("session-1".to_string()),
        genesis_ref: Some("genesis-1".to_string()),
        channel_id: Some("c0ffee".to_string()),
        branch: Some("lane/refuter".to_string()),
    }
}

fn config_value<'a>(plan: &'a SeatGitHookPlan, key: &str) -> Option<&'a str> {
    plan.config
        .iter()
        .find(|line| line.key == key)
        .map(|line| line.value.as_str())
}

fn config_keys(plan: &SeatGitHookPlan) -> Vec<&str> {
    plan.config.iter().map(|line| line.key.as_str()).collect()
}

// ---------------------------------------------------------------- ref naming

#[test]
fn ref_name_is_derived_and_sanitised() {
    assert_eq!(
        wip_ref_name("Refuter", "0f1e2d3c").expect("ref"),
        "refs/heads/wip/refuter/0f1e2d3c"
    );
}

#[test]
fn ref_name_collapses_and_bounds_hostile_segments() {
    // Anything an agent could type reaches the ref name only through this.
    let name = wip_ref_name("../../refs/heads/main", "a b/c;rm -rf ~").expect("ref");
    assert_eq!(name, "refs/heads/wip/refs-heads-main/a-b-c-rm-rf");
    assert!(is_wip_ref(&name));
}

#[test]
fn ref_name_bounds_each_segment_to_forty_bytes() {
    let name = wip_ref_name(&"r".repeat(80), &"s".repeat(80)).expect("ref");
    assert_eq!(
        name,
        format!("refs/heads/wip/{}/{}", "r".repeat(40), "s".repeat(40))
    );
}

#[test]
fn ref_name_refuses_a_segment_that_sanitises_to_nothing() {
    assert_eq!(
        wip_ref_name("!!!", "0f1e2d3c").unwrap_err(),
        "a wip ref role is empty once sanitised"
    );
    assert_eq!(
        wip_ref_name("refuter", "///").unwrap_err(),
        "a wip ref assignment slug is empty once sanitised"
    );
}

#[test]
fn assignment_slug_prefers_the_assignment_id() {
    assert_eq!(
        wip_assignment_slug(Some(ASSIGNMENT), Some("lane/refuter")).expect("slug"),
        "0f1e2d3c"
    );
}

#[test]
fn assignment_slug_falls_back_to_the_branch() {
    assert_eq!(
        wip_assignment_slug(None, Some("lane/Refuter")).expect("slug"),
        "lane-refuter"
    );
    // A malformed id is not an id; the branch still names the ref.
    assert_eq!(
        wip_assignment_slug(Some("not-hex"), Some("lane/refuter")).expect("slug"),
        "lane-refuter"
    );
}

#[test]
fn assignment_slug_refuses_when_there_is_nothing_to_derive_from() {
    assert_eq!(
        wip_assignment_slug(None, None).unwrap_err(),
        "no assignment id and no branch to derive a wip ref from"
    );
}

// -------------------------------------------------------------------- plan

#[test]
fn plan_writes_the_signing_config_in_order() {
    let plan = plan_seat_git_hooks(&request()).expect("plan");
    assert_eq!(
        config_keys(&plan),
        vec![
            "gpg.format",
            "gpg.x509.program",
            "commit.gpgsign",
            "user.signingkey",
            "nostr.keyfile",
            "buzz.wipShare",
            "buzz.seatRole",
            "buzz.wipRef",
            "buzz.assignmentId",
            "buzz.sessionRef",
            "buzz.genesisRef",
            "buzz.channel",
        ]
    );
    assert_eq!(config_value(&plan, "gpg.format"), Some("x509"));
    assert_eq!(config_value(&plan, "user.signingkey"), Some(SEAT));
    assert_eq!(config_value(&plan, "buzz.seatRole"), Some("refuter"));
    assert_eq!(
        config_value(&plan, "buzz.wipRef"),
        Some("refs/heads/wip/refuter/0f1e2d3c")
    );
}

#[test]
fn plan_never_names_a_remote() {
    // CLAUDE.md: never hard-code a remote name in tooling. Two pre-push guards
    // did and both broke silently the day the remote names moved.
    let plan = plan_seat_git_hooks(&request()).expect("plan");
    for line in &plan.config {
        assert!(
            !line.key.starts_with("remote."),
            "config line {} names a remote",
            line.key
        );
        assert!(
            !line.key.contains("Remote") && !line.key.contains("remote"),
            "config line {} names a remote",
            line.key
        );
    }
}

#[test]
fn plan_without_an_assignment_writes_no_assignment_line() {
    // So the prepare-commit-msg hook invents no trailer.
    let mut request = request();
    request.assignment_id = None;
    let plan = plan_seat_git_hooks(&request).expect("plan");
    assert_eq!(config_value(&plan, "buzz.assignmentId"), None);
    assert_eq!(
        config_value(&plan, "buzz.wipRef"),
        Some("refs/heads/wip/refuter/lane-refuter")
    );
}

#[test]
fn plan_omits_unset_optional_lines() {
    let mut request = request();
    request.session_ref = None;
    request.genesis_ref = None;
    request.channel_id = Some(String::new());
    let plan = plan_seat_git_hooks(&request).expect("plan");
    assert_eq!(config_value(&plan, "buzz.sessionRef"), None);
    assert_eq!(config_value(&plan, "buzz.genesisRef"), None);
    assert_eq!(config_value(&plan, "buzz.channel"), None);
}

#[test]
fn plan_refuses_a_seat_pubkey_that_is_not_lowercase_hex() {
    let mut request = request();
    request.seat_pubkey = SEAT.to_uppercase();
    assert_eq!(
        plan_seat_git_hooks(&request).unwrap_err(),
        "seat pubkey must be 64 lowercase hex characters"
    );
}

#[test]
fn plan_refuses_an_assignment_id_that_is_not_lowercase_hex() {
    let mut request = request();
    request.assignment_id = Some("deadbeef".to_string());
    assert_eq!(
        plan_seat_git_hooks(&request).unwrap_err(),
        "assignment id must be 64 lowercase hex characters"
    );
}

#[test]
fn a_seat_with_no_keyfile_shares_unsigned_rather_than_failing_every_commit() {
    // REVIEW-L9 F2: the Desktop hire host has no stable key file to point at —
    // the seat's secret is injected as `$NOSTR_PRIVATE_KEY`, which both the
    // signer and the credential helper read first. Signing is therefore
    // all-or-nothing: five lines together, or none of them. Pointing
    // `nostr.keyfile` at a path that does not exist while `commit.gpgsign` is
    // true fails **every** commit the seat makes.
    let mut request = request();
    request.keyfile_path = None;
    let plan = plan_seat_git_hooks(&request).expect("a seat still shares without a key file");
    let keys: Vec<&str> = plan.config.iter().map(|line| line.key.as_str()).collect();
    for signing in [
        "gpg.format",
        "gpg.x509.program",
        "commit.gpgsign",
        "user.signingkey",
        "nostr.keyfile",
    ] {
        assert!(
            !keys.contains(&signing),
            "{signing} must be absent: {keys:?}"
        );
    }
    // The sharing half is untouched: the seat still pushes to its own ref.
    assert!(keys.contains(&"buzz.wipShare"));
    assert!(keys.contains(&"buzz.wipRef"));
    assert!(keys.contains(&"buzz.seatRole"));

    // A blank string is the same fact as no path, not a path named "  ".
    let mut request = self::request();
    request.keyfile_path = Some("  ".to_string());
    let plan = plan_seat_git_hooks(&request).expect("blank is none");
    assert!(!plan.config.iter().any(|line| line.key == "nostr.keyfile"));

    // A signer that is asked to sign and given no program is still refused.
    let mut request = self::request();
    request.signer_program = String::new();
    assert_eq!(
        plan_seat_git_hooks(&request).unwrap_err(),
        "a seat needs a signer program to sign with"
    );
}

#[test]
fn plan_carries_both_hooks_executable() {
    let plan = plan_seat_git_hooks(&request()).expect("plan");
    let names: Vec<&str> = plan.hooks.iter().map(|hook| hook.name).collect();
    assert_eq!(names, vec!["post-commit", "prepare-commit-msg"]);
    for hook in &plan.hooks {
        assert_eq!(hook.mode, 0o755);
        assert!(hook.contents.starts_with("#!/usr/bin/env bash"));
    }
}

// ------------------------------------------------------------- script bytes

#[test]
fn the_post_commit_constant_is_the_script_on_disk() {
    assert_eq!(
        WIP_POST_COMMIT_HOOK,
        include_str!("../../../scripts/wip-post-commit.sh"),
        "the seat installer and lefthook must write the same bytes"
    );
}

#[test]
fn the_prepare_commit_msg_constant_is_the_script_on_disk() {
    assert_eq!(
        WIP_PREPARE_COMMIT_MSG_HOOK,
        include_str!("../../../scripts/wip-prepare-commit-msg.sh"),
        "the seat installer and lefthook must write the same bytes"
    );
}

#[test]
fn the_post_commit_hook_never_reaches_for_uncommitted_work() {
    for forbidden in ["git status", "git add", "git stash", "eval "] {
        assert!(
            !WIP_POST_COMMIT_HOOK.contains(forbidden),
            "the post-commit hook must never run `{forbidden}`"
        );
    }
    // `set -e` would let a failed push fail somebody's commit.
    assert!(WIP_POST_COMMIT_HOOK.contains("set -uo pipefail"));
    assert!(!WIP_POST_COMMIT_HOOK.contains("set -euo"));
}

#[test]
fn is_wip_ref_needs_a_name_after_the_prefix() {
    // Re-exported from `pulse_mission` so the renderer and the pruner cannot
    // disagree about what a wip ref is.
    assert!(is_wip_ref("refs/heads/wip/human/main"));
    assert!(!is_wip_ref("refs/heads/wip/"));
    assert!(!is_wip_ref("refs/heads/wip"));
    assert!(!is_wip_ref("feature/wip/x"));
}
