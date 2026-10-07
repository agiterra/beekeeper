//! Tests for `bee sessions checkpoints` / `bee sessions diff`. Every git
//! fixture is a throwaway repository under a tempdir; nothing here runs git in
//! this checkout.

use std::path::Path;

use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

use beekeeper_core::coding_session_checkpoint::{
    CodingSessionCheckpointCoverage, CodingSessionCheckpointFile,
    CodingSessionCheckpointFileStatus, CodingSessionCheckpointGit, CodingSessionCheckpointPayload,
    CodingSessionCheckpointReason, CodingSessionCheckpointUnavailable,
    CodingSessionCheckpointUnavailableCode, CODING_SESSION_CHECKPOINT_SCHEMA,
};
use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_payload::TranscriptEnvelope;
use beekeeper_sdk::coding_session_checkpoint::build_coding_session_checkpoint;
use beekeeper_sdk::kind::KIND_CODING_SESSION_TRANSCRIPT;

use super::*;
use crate::commands::sessions::worktree::git_command;

const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

fn target(session: &str) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".to_owned(),
        instance_id: "claude-primary".to_owned(),
        session_id: session.to_owned(),
        generation: 1,
    }
}

fn git_payload(
    session: &str,
    turn: &str,
    from: u64,
    through: u64,
    head: Option<&str>,
    base: Option<&str>,
    tree: &str,
) -> CodingSessionCheckpointPayload {
    CodingSessionCheckpointPayload {
        schema: CODING_SESSION_CHECKPOINT_SCHEMA.to_owned(),
        session: target(session),
        turn_id: Some(turn.to_owned()),
        reason: CodingSessionCheckpointReason::Turn,
        coverage: CodingSessionCheckpointCoverage {
            from_seq: from,
            through_seq: through,
        },
        git: Some(CodingSessionCheckpointGit {
            head: head.map(str::to_owned),
            branch: Some("main".to_owned()),
            base_tree: base.map(str::to_owned),
            tree: tree.to_owned(),
            commit: "c".repeat(40),
            outside_turn: Some(false),
            complete: true,
            omitted: vec![],
            omitted_not_listed: 0,
        }),
        files: if base.is_some() {
            vec![CodingSessionCheckpointFile {
                path: "a.txt".to_owned(),
                status: CodingSessionCheckpointFileStatus::Modified,
                from: None,
                additions: Some(1),
                deletions: Some(1),
            }]
        } else {
            vec![]
        },
        files_not_listed: 0,
        restorable: false,
        unavailable: None,
        summary: None,
    }
}

fn unavailable_payload(session: &str, turn: &str, through: u64) -> CodingSessionCheckpointPayload {
    let mut payload = git_payload(session, turn, through, through, None, None, &"a".repeat(40));
    payload.git = None;
    payload.files = vec![];
    payload.unavailable = Some(CodingSessionCheckpointUnavailable {
        code: CodingSessionCheckpointUnavailableCode::NotARepository,
        sentence: "The session's directory is not a git repository.".to_owned(),
    });
    payload
}

fn signed(keys: &Keys, payload: &CodingSessionCheckpointPayload) -> Value {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let event = build_coding_session_checkpoint(channel, payload)
        .expect("builder")
        .sign_with_keys(keys)
        .expect("sign");
    serde_json::to_value(event).expect("event json")
}

fn transcript(keys: &Keys, session: &str, seq: u64) -> Value {
    let target = target(session);
    let envelope = TranscriptEnvelope::new(
        &target,
        seq,
        1_000,
        Some("turn-1"),
        json!({ "kind": "user_prompt", "text": "hi" }),
    );
    json!({
        "id": format!("{seq:064x}"),
        "pubkey": keys.public_key().to_hex(),
        "kind": KIND_CODING_SESSION_TRANSCRIPT,
        "created_at": 1,
        "sig": "0".repeat(128),
        "tags": [
            ["h", CHANNEL],
            ["cst-v", "cst1-1"],
            ["cs-target", coding_session_target_key(&target)],
            ["cst-seq", seq.to_string()],
        ],
        "content": serde_json::to_string(&envelope).expect("envelope"),
    })
}

fn key(session: &str) -> String {
    coding_session_target_key(&target(session))
}

#[test]
fn checkpoints_admit_a_valid_checkpoint_from_the_transcript_signer() {
    let provider = Keys::generate();
    let payload = git_payload(
        "s1",
        "turn-1",
        1,
        5,
        Some(&"d".repeat(40)),
        Some(&"a".repeat(40)),
        &"b".repeat(40),
    );
    let events = vec![transcript(&provider, "s1", 1), signed(&provider, &payload)];
    let report = checkpoints_report(&events, Some(&key("s1")), None);
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    assert_eq!(report.rows.len(), 1);
    let row = &report.rows[0];
    assert_eq!(row["turnId"], "turn-1");
    assert_eq!(row["reason"], "turn");
    assert_eq!(row["fromSeq"], 1);
    assert_eq!(row["throughSeq"], 5);
    assert_eq!(row["baseTree"], "a".repeat(40));
    assert_eq!(row["tree"], "b".repeat(40));
    assert_eq!(row["files"][0]["path"], "a.txt");
    assert_eq!(row["files"][0]["additions"], 1);
    assert_eq!(row["complete"], true);
    assert_eq!(row["outsideTurn"], false);
    assert_eq!(row["restorable"], false);
    assert_eq!(row["unavailable"], Value::Null);
    assert!(row.get("sig").is_none());
}

#[test]
fn checkpoints_refuse_a_foreign_signer_with_the_reason() {
    let provider = Keys::generate();
    let stranger = Keys::generate();
    let payload = git_payload("s1", "turn-1", 1, 5, None, None, &"b".repeat(40));
    let events = vec![transcript(&provider, "s1", 1), signed(&stranger, &payload)];
    let report = checkpoints_report(&events, Some(&key("s1")), None);
    assert!(report.rows.is_empty());
    assert_eq!(report.refused.len(), 1);
    assert!(
        report.refused[0]
            .reason
            .contains("not by the key that signs"),
        "{}",
        report.refused[0].reason
    );
    assert_eq!(
        report.refused[0].signer.as_deref(),
        Some(stranger.public_key().to_hex().as_str())
    );
}

#[test]
fn checkpoints_refuse_when_no_transcript_names_the_generation() {
    let provider = Keys::generate();
    let payload = git_payload("s1", "turn-1", 1, 5, None, None, &"b".repeat(40));
    let report = checkpoints_report(&[signed(&provider, &payload)], None, Some("ddddddd"));
    assert!(report.rows.is_empty());
    assert!(report.refused[0].reason.contains("signer is unknown"));
}

#[test]
fn checkpoints_refuse_malformed_and_tampered_events() {
    let provider = Keys::generate();
    let payload = git_payload("s1", "turn-1", 1, 5, None, None, &"b".repeat(40));
    let mut tampered = signed(&provider, &payload);
    tampered["content"] = json!(tampered["content"]
        .as_str()
        .expect("content")
        .replace("turn-1", "turn-2"));
    let not_an_event = json!({
        "id": "x",
        "kind": KIND_CODING_SESSION_CHECKPOINT,
        "tags": [["cs-target", key("s1")]],
        "content": "{}",
    });
    let events = vec![transcript(&provider, "s1", 1), tampered, not_an_event];
    let report = checkpoints_report(&events, Some(&key("s1")), None);
    assert!(report.rows.is_empty());
    assert_eq!(report.refused.len(), 2, "{:?}", report.refused);
    assert!(report
        .refused
        .iter()
        .any(|refusal| refusal.reason.contains("signature does not verify")));
    assert!(report
        .refused
        .iter()
        .any(|refusal| refusal.reason.contains("not a well-formed signed event")));
}

#[test]
fn checkpoints_refuse_content_the_core_decoder_rejects() {
    let provider = Keys::generate();
    // A validly signed 44231 whose content carries an unknown key.
    let event = nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_CHECKPOINT as u16),
        r#"{"schema":"buzz-coding-session-checkpoint/v1","extra":1}"#,
    )
    .sign_with_keys(&provider)
    .expect("sign");
    let events = vec![
        transcript(&provider, "s1", 1),
        serde_json::to_value(event).expect("json"),
    ];
    let report = checkpoints_report(&events, None, None);
    assert!(report.rows.is_empty());
    assert_eq!(report.refused.len(), 1);
    assert!(!report.refused[0].reason.is_empty());
}

#[test]
fn checkpoints_fold_keeps_the_highest_seq_per_turn_and_counts_duplicates() {
    let provider = Keys::generate();
    let early = git_payload("s1", "turn-1", 1, 5, None, None, &"b".repeat(40));
    let late = git_payload("s1", "turn-1", 1, 9, None, None, &"e".repeat(40));
    let mut rewind = git_payload("s1", "turn-1", 9, 9, None, None, &"e".repeat(40));
    rewind.reason = CodingSessionCheckpointReason::PreRewind;
    rewind.turn_id = None;
    let events = vec![
        transcript(&provider, "s1", 1),
        signed(&provider, &early),
        signed(&provider, &late),
        signed(&provider, &late),
        signed(&provider, &rewind),
    ];
    let report = checkpoints_report(&events, Some(&key("s1")), None);
    let reasons: Vec<(&str, u64)> = report
        .rows
        .iter()
        .map(|row| {
            (
                row["reason"].as_str().unwrap_or(""),
                row["throughSeq"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(reasons, vec![("pre_rewind", 9), ("turn", 9)]);
    assert_eq!(report.superseded, 1);
    assert!(report.duplicates >= 1);
}

#[test]
fn checkpoints_commit_matches_head_prefix_across_targets() {
    let provider = Keys::generate();
    let head = format!("abcdef1{}", "0".repeat(33));
    let hit = git_payload("s1", "turn-1", 1, 5, Some(&head), None, &"b".repeat(40));
    let other = git_payload(
        "s2",
        "turn-1",
        1,
        5,
        Some(&"9".repeat(40)),
        None,
        &"b".repeat(40),
    );
    let events = vec![
        transcript(&provider, "s1", 1),
        transcript(&provider, "s2", 1),
        signed(&provider, &hit),
        signed(&provider, &other),
    ];
    let prefix = normalize_commit_prefix("ABCDEF1").expect("prefix");
    let report = checkpoints_report(&events, None, Some(&prefix));
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0]["sessionId"], "s1");
    let miss = checkpoints_report(&events, None, Some("1234567"));
    assert!(miss.rows.is_empty());
    assert!(normalize_commit_prefix("abc12").is_err());
    assert!(normalize_commit_prefix("zzzzzzz").is_err());
}

#[test]
fn checkpoints_unavailable_row_and_diff_say_why() {
    let provider = Keys::generate();
    let payload = unavailable_payload("s1", "turn-1", 4);
    let events = vec![transcript(&provider, "s1", 1), signed(&provider, &payload)];
    let report = checkpoints_report(&events, Some(&key("s1")), None);
    let row = &report.rows[0];
    assert_eq!(row["unavailable"]["code"], "NOT_A_REPOSITORY");
    assert_eq!(row["tree"], Value::Null);
    let (fold, _) = read_checkpoints(&events);
    let plan = plan_diff(&fold.checkpoints, None, None, DiffScope::Turn).expect("plan");
    let answer = diff_answer(CHANNEL, &key("s1"), &plan, Ok(None));
    assert_eq!(answer["diffUnavailable"]["code"], "CHECKPOINT_UNAVAILABLE");
    assert_eq!(answer["patch"], Value::Null);
    assert!(answer["diffUnavailable"]["sentence"]
        .as_str()
        .unwrap_or("")
        .contains("NOT_A_REPOSITORY"));
}

#[test]
fn checkpoints_baseline_missing_uses_the_previous_tree_and_says_so() {
    let provider = Keys::generate();
    let first = git_payload(
        "s1",
        "turn-1",
        1,
        5,
        None,
        Some(&"a".repeat(40)),
        &"b".repeat(40),
    );
    let second = git_payload("s1", "turn-2", 6, 9, None, None, &"e".repeat(40));
    let events = vec![
        transcript(&provider, "s1", 1),
        signed(&provider, &first),
        signed(&provider, &second),
    ];
    let (fold, _) = read_checkpoints(&events);
    let plan = plan_diff(&fold.checkpoints, Some("turn-2"), None, DiffScope::Turn).expect("plan");
    assert_eq!(plan.from_tree.as_deref(), Some("b".repeat(40).as_str()));
    assert_eq!(
        plan.baseline.as_ref().expect("baseline")["source"],
        "previous_checkpoint"
    );
    let alone = plan_diff(&fold.checkpoints[1..], None, None, DiffScope::Turn).expect("plan");
    assert_eq!(
        alone.unavailable.as_ref().map(|u| u.code),
        Some("BASELINE_MISSING")
    );
    let session = plan_diff(&fold.checkpoints, None, None, DiffScope::Session).expect("plan");
    assert_eq!(session.from_tree.as_deref(), Some("a".repeat(40).as_str()));
    assert_eq!(session.to_tree.as_deref(), Some("e".repeat(40).as_str()));
    assert!(plan_diff(&fold.checkpoints, Some("turn-1"), None, DiffScope::Session).is_err());
    assert!(matches!(
        plan_diff(&fold.checkpoints, Some("turn-9"), None, DiffScope::Turn),
        Err(CliError::NotFound(_))
    ));
}

#[test]
fn checkpoints_session_outside_turn_is_null_unless_every_checkpoint_says() {
    let provider = Keys::generate();
    let with_outside = |turn: &str, from: u64, through: u64, outside: Option<bool>| {
        let mut payload = git_payload(
            "s1",
            turn,
            from,
            through,
            None,
            Some(&"a".repeat(40)),
            &"b".repeat(40),
        );
        if let Some(git) = payload.git.as_mut() {
            git.outside_turn = outside;
        }
        payload
    };
    let session_outside = |values: [Option<bool>; 2]| {
        let events = vec![
            transcript(&provider, "s1", 1),
            signed(&provider, &with_outside("turn-1", 1, 5, values[0])),
            signed(&provider, &with_outside("turn-2", 6, 9, values[1])),
        ];
        let (fold, _) = read_checkpoints(&events);
        let plan = plan_diff(&fold.checkpoints, None, None, DiffScope::Session).expect("plan");
        plan_files(&plan)["outsideTurn"].clone()
    };
    // Unknown everywhere is unknown, never a measured "no".
    assert_eq!(session_outside([None, None]), Value::Null);
    // One unknown leaves "no" unproven.
    assert_eq!(session_outside([Some(false), None]), Value::Null);
    assert_eq!(session_outside([Some(false), Some(false)]), json!(false));
    // Any measured "yes" is a yes.
    assert_eq!(session_outside([None, Some(true)]), json!(true));
}

// ── Git fixtures (tempdir repositories only) ────────────────────────────────

fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_command(dir)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository with two trees: `a.txt` = "one" and then "two".
fn repo_with_two_trees(dir: &Path) -> (String, String) {
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "one\n").expect("write");
    git(dir, &["add", "a.txt"]);
    let base = git(dir, &["write-tree"]);
    std::fs::write(dir.join("a.txt"), "two\n").expect("write");
    git(dir, &["add", "a.txt"]);
    let tree = git(dir, &["write-tree"]);
    (base, tree)
}

#[test]
fn checkpoints_diff_reads_the_patch_from_a_tempdir_repo() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (base, tree) = repo_with_two_trees(temp.path());
    let patch = diff_trees_in(temp.path(), &base, &tree).expect("patch");
    assert!(patch.patch.contains("-one"), "{}", patch.patch);
    assert!(patch.patch.contains("+two"), "{}", patch.patch);
    assert_eq!(patch.truncated_bytes, 0);

    let provider = Keys::generate();
    let payload = git_payload("s1", "turn-1", 1, 5, None, Some(&base), &tree);
    let events = vec![transcript(&provider, "s1", 1), signed(&provider, &payload)];
    let (fold, _) = read_checkpoints(&events);
    let plan = plan_diff(&fold.checkpoints, None, None, DiffScope::Turn).expect("plan");
    let checkout = Checkout {
        dir: temp.path().to_path_buf(),
        source: "explicit",
    };
    let answer = diff_answer(CHANNEL, &key("s1"), &plan, Ok(Some(checkout)));
    assert_eq!(answer["diffUnavailable"], Value::Null);
    assert_eq!(answer["checkout"], "explicit");
    assert_eq!(answer["files"][0]["path"], "a.txt");
    assert!(answer["patch"].as_str().unwrap_or("").contains("+two"));
}

#[test]
fn checkpoints_diff_missing_objects_and_no_checkout_keep_the_file_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (base, _) = repo_with_two_trees(temp.path());
    let absent = "1".repeat(40);
    let missing = diff_trees_in(temp.path(), &base, &absent).expect_err("missing");
    assert_eq!(missing.code, "OBJECTS_MISSING");
    assert_eq!(missing.missing, vec![absent.clone()]);

    let provider = Keys::generate();
    let payload = git_payload("s1", "turn-1", 1, 5, None, Some(&base), &absent);
    let events = vec![transcript(&provider, "s1", 1), signed(&provider, &payload)];
    let (fold, _) = read_checkpoints(&events);
    let plan = plan_diff(&fold.checkpoints, None, None, DiffScope::Turn).expect("plan");
    let answer = diff_answer(
        CHANNEL,
        &key("s1"),
        &plan,
        Ok(Some(Checkout {
            dir: temp.path().to_path_buf(),
            source: "seat_worktree",
        })),
    );
    assert_eq!(answer["diffUnavailable"]["code"], "OBJECTS_MISSING");
    assert_eq!(answer["files"][0]["path"], "a.txt");
    assert_eq!(answer["patch"], Value::Null);

    let none = diff_answer(CHANNEL, &key("s1"), &plan, Ok(None));
    assert_eq!(none["diffUnavailable"]["code"], "NO_CHECKOUT");
    assert_eq!(none["files"][0]["path"], "a.txt");

    let blob = git(temp.path(), &["hash-object", "a.txt"]);
    let not_tree = diff_trees_in(temp.path(), &base, &blob).expect_err("not a tree");
    assert_eq!(not_tree.code, "NOT_A_TREE");
}

#[test]
fn checkpoints_diff_leaves_the_repository_index_and_head_alone() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (base, tree) = repo_with_two_trees(temp.path());
    let marker = temp.path().join("hook-fired");
    let hook = temp.path().join(".git/hooks/post-checkout");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", marker.display())).expect("hook");
    let index = temp.path().join(".git/index");
    let before = std::fs::metadata(&index)
        .and_then(|meta| meta.modified())
        .expect("mtime");
    let head_before = std::fs::read_to_string(temp.path().join(".git/HEAD")).expect("HEAD");
    diff_trees_in(temp.path(), &base, &tree).expect("patch");
    let after = std::fs::metadata(&index)
        .and_then(|meta| meta.modified())
        .expect("mtime");
    assert_eq!(before, after);
    assert_eq!(
        head_before,
        std::fs::read_to_string(temp.path().join(".git/HEAD")).expect("HEAD")
    );
    assert!(!marker.exists());
}

#[test]
fn checkpoints_resolve_checkout_from_the_host_record() {
    let temp = tempfile::tempdir().expect("tempdir");
    let seat = temp.path().join("seat");
    let channel_dir = temp.path().join("channel");
    std::fs::create_dir_all(&seat).expect("seat");
    std::fs::create_dir_all(&channel_dir).expect("channel");
    let store = temp.path().join("coding-session-workdirs.json");
    std::fs::write(
        &store,
        json!({
            "version": 2,
            "worktrees": {
                "ref-1/lead": {
                    "path": seat, "branch": "b", "repoRoot": temp.path(),
                    "createdAt": "2026-10-07T00:00:00Z", "sessionId": "s1"
                }
            },
            "byChannel": { CHANNEL: { "path": channel_dir, "updatedAt": "x" } }
        })
        .to_string(),
    )
    .expect("store");
    let found = resolve_checkout(&store, "s1", Some("ref-1"), CHANNEL).expect("read");
    assert_eq!(found.map(|c| c.source), Some("seat_worktree"));
    let wrong_ref = resolve_checkout(&store, "s1", Some("ref-2"), CHANNEL).expect("read");
    assert_eq!(wrong_ref.map(|c| c.source), Some("channel_checkout"));
    let other_channel =
        resolve_checkout(&store, "s9", None, "00000000-0000-0000-0000-000000000000").expect("read");
    assert!(other_channel.is_none());
    let absent = resolve_checkout(&temp.path().join("nope.json"), "s1", None, CHANNEL);
    assert_eq!(absent, Ok(None));
    std::fs::write(&store, r#"{"version": 99}"#).expect("store");
    assert!(resolve_checkout(&store, "s1", None, CHANNEL).is_err());
}
