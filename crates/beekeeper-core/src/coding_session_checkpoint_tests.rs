use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};

use super::*;

const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const TREE: &str = "1111111111111111111111111111111111111111";
const BASE: &str = "2222222222222222222222222222222222222222";
const COMMIT: &str = "3333333333333333333333333333333333333333";
const HEAD: &str = "4444444444444444444444444444444444444444";

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: "f543d7bd-6059-44e3-a50b-4275d8440dd5".into(),
        generation: 2,
    }
}

fn payload() -> CodingSessionCheckpointPayload {
    CodingSessionCheckpointPayload {
        schema: CODING_SESSION_CHECKPOINT_SCHEMA.into(),
        session: target(),
        turn_id: Some("bc918cc7-7f95-4600-ad73-9492f743270c".into()),
        reason: CodingSessionCheckpointReason::Turn,
        coverage: CodingSessionCheckpointCoverage {
            from_seq: 41,
            through_seq: 58,
        },
        git: Some(CodingSessionCheckpointGit {
            head: Some(HEAD.into()),
            branch: Some("work/parity".into()),
            base_tree: Some(BASE.into()),
            tree: TREE.into(),
            commit: COMMIT.into(),
            outside_turn: Some(false),
            complete: true,
            omitted: vec![],
            omitted_not_listed: 0,
        }),
        files: vec![
            CodingSessionCheckpointFile {
                path: "src/main.rs".into(),
                status: CodingSessionCheckpointFileStatus::Modified,
                from: None,
                additions: Some(3),
                deletions: Some(1),
            },
            CodingSessionCheckpointFile {
                path: "assets/logo.png".into(),
                status: CodingSessionCheckpointFileStatus::Added,
                from: None,
                additions: None,
                deletions: None,
            },
            CodingSessionCheckpointFile {
                path: "docs/new.md".into(),
                status: CodingSessionCheckpointFileStatus::Renamed,
                from: Some("docs/old.md".into()),
                additions: Some(0),
                deletions: Some(0),
            },
        ],
        files_not_listed: 0,
        restorable: false,
        unavailable: None,
        summary: None,
    }
}

fn unavailable_payload() -> CodingSessionCheckpointPayload {
    CodingSessionCheckpointPayload {
        git: None,
        files: vec![],
        unavailable: Some(CodingSessionCheckpointUnavailable {
            code: CodingSessionCheckpointUnavailableCode::NotARepository,
            sentence: "The session's folder is not a git repository.".into(),
        }),
        ..payload()
    }
}

fn value() -> Value {
    serde_json::to_value(payload()).expect("serialize")
}

fn decode_value(value: &Value) -> Result<CodingSessionCheckpointPayload, String> {
    decode_coding_session_checkpoint(&value.to_string())
}

fn refused(value: &Value, needle: &str) {
    let error = decode_value(value).expect_err("must be refused");
    assert!(error.contains(needle), "{error:?} lacks {needle:?}");
}

fn sign(content: &str, tags: Vec<Vec<String>>) -> Event {
    let tags = tags
        .into_iter()
        .map(|parts| Tag::parse(parts).expect("tag"))
        .collect::<Vec<_>>();
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_CHECKPOINT as u16),
        content.to_owned(),
    )
    .tags(tags)
    .sign_with_keys(&Keys::generate())
    .expect("sign")
}

fn tags_for(payload: &CodingSessionCheckpointPayload) -> Vec<Vec<String>> {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    coding_session_checkpoint_tags(&channel, payload)
        .into_iter()
        .map(|pair| pair.to_vec())
        .collect()
}

// ── Kind ────────────────────────────────────────────────────────────────────

#[test]
fn the_checkpoint_kind_claims_its_reservation_and_is_regular() {
    assert_eq!(KIND_CODING_SESSION_CHECKPOINT, 44231);
    assert!(crate::kind::ALL_KINDS.contains(&KIND_CODING_SESSION_CHECKPOINT));
    assert!(!crate::kind::is_replaceable(KIND_CODING_SESSION_CHECKPOINT));
    assert!(!crate::kind::is_parameterized_replaceable(
        KIND_CODING_SESSION_CHECKPOINT
    ));
    assert!(!crate::kind::is_ephemeral(KIND_CODING_SESSION_CHECKPOINT));
}

// ── Round trip ──────────────────────────────────────────────────────────────

#[test]
fn a_full_checkpoint_round_trips_key_for_key() {
    let content = encode_coding_session_checkpoint(&payload()).expect("encode");
    let decoded = decode_coding_session_checkpoint(&content).expect("decode");
    assert_eq!(decoded, payload());
    let reparsed: Value = serde_json::from_str(&content).expect("json");
    assert_eq!(
        reparsed,
        json!({
            "schema": "buzz-coding-session-checkpoint/v1",
            "session": {
                "driver": "claude-agent-acp",
                "instanceId": "1958c6c448e05eed",
                "sessionId": "f543d7bd-6059-44e3-a50b-4275d8440dd5",
                "generation": 2
            },
            "turnId": "bc918cc7-7f95-4600-ad73-9492f743270c",
            "reason": "turn",
            "coverage": {"fromSeq": 41, "throughSeq": 58},
            "git": {
                "head": HEAD, "branch": "work/parity", "baseTree": BASE, "tree": TREE,
                "commit": COMMIT, "outsideTurn": false, "complete": true, "omitted": [],
                "omittedNotListed": 0
            },
            "files": [
                {"path": "src/main.rs", "status": "modified", "from": null,
                 "additions": 3, "deletions": 1},
                {"path": "assets/logo.png", "status": "added", "from": null,
                 "additions": null, "deletions": null},
                {"path": "docs/new.md", "status": "renamed", "from": "docs/old.md",
                 "additions": 0, "deletions": 0}
            ],
            "filesNotListed": 0,
            "restorable": false,
            "unavailable": null,
            "summary": null
        })
    );
}

#[test]
fn an_unavailable_checkpoint_round_trips() {
    let content = encode_coding_session_checkpoint(&unavailable_payload()).expect("encode");
    assert_eq!(
        decode_coding_session_checkpoint(&content).expect("decode"),
        unavailable_payload()
    );
}

#[test]
fn every_unavailable_code_and_every_status_decodes() {
    for code in [
        CodingSessionCheckpointUnavailableCode::NotARepository,
        CodingSessionCheckpointUnavailableCode::BoundaryUnprepared,
        CodingSessionCheckpointUnavailableCode::TimedOut,
        CodingSessionCheckpointUnavailableCode::GitFailed,
    ] {
        let mut checkpoint = unavailable_payload();
        checkpoint.unavailable.as_mut().expect("set").code = code;
        let content = encode_coding_session_checkpoint(&checkpoint).expect("encode");
        assert!(content.contains(code.as_str()));
    }
    let mut checkpoint = payload();
    checkpoint.files = vec![CodingSessionCheckpointFile {
        path: "gone.txt".into(),
        status: CodingSessionCheckpointFileStatus::Deleted,
        from: None,
        additions: Some(0),
        deletions: Some(9),
    }];
    encode_coding_session_checkpoint(&checkpoint).expect("a deletion encodes");
}

#[test]
fn sha256_object_ids_are_accepted_when_consistent() {
    let mut checkpoint = payload();
    let git = checkpoint.git.as_mut().expect("git");
    git.head = Some("a".repeat(64));
    git.base_tree = Some("b".repeat(64));
    git.tree = "c".repeat(64);
    git.commit = "d".repeat(64);
    encode_coding_session_checkpoint(&checkpoint).expect("sha-256 repo");

    checkpoint.git.as_mut().expect("git").head = Some(HEAD.into());
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("mixed widths");
    assert!(
        error.contains("must all be SHA-1 or all SHA-256"),
        "{error}"
    );
}

// ── Bounds ──────────────────────────────────────────────────────────────────

fn file(index: usize) -> CodingSessionCheckpointFile {
    CodingSessionCheckpointFile {
        path: format!("f/{index}"),
        status: CodingSessionCheckpointFileStatus::Added,
        from: None,
        additions: Some(1),
        deletions: Some(0),
    }
}

#[test]
fn files_are_capped_at_256() {
    let mut checkpoint = payload();
    checkpoint.files = (0..MAX_CHECKPOINT_FILES).map(file).collect();
    encode_coding_session_checkpoint(&checkpoint).expect("256 files fit");

    checkpoint.files.push(file(MAX_CHECKPOINT_FILES));
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("257 files");
    assert!(error.contains("files exceeds 256"), "{error}");

    let mut raw = value();
    raw["files"] = Value::Array(
        (0..=MAX_CHECKPOINT_FILES)
            .map(|index| serde_json::to_value(file(index)).expect("file"))
            .collect(),
    );
    refused(&raw, "files exceeds 256");
}

#[test]
fn omitted_is_capped_at_32() {
    let omission = |index: usize| CodingSessionCheckpointOmission {
        path: format!("big/{index}.bin"),
        reason: CodingSessionCheckpointOmissionReason::TooLarge,
    };
    let mut checkpoint = payload();
    let git = checkpoint.git.as_mut().expect("git");
    git.complete = false;
    git.omitted = (0..MAX_CHECKPOINT_OMITTED).map(omission).collect();
    encode_coding_session_checkpoint(&checkpoint).expect("32 omissions fit");

    checkpoint
        .git
        .as_mut()
        .expect("git")
        .omitted
        .push(omission(MAX_CHECKPOINT_OMITTED));
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("33 omissions");
    assert!(error.contains("git.omitted exceeds 32"), "{error}");

    let content = serde_json::to_string(&checkpoint).expect("serialize");
    let error = decode_coding_session_checkpoint(&content).expect_err("33 on decode");
    assert!(error.contains("git.omitted exceeds 32"), "{error}");
}

#[test]
fn omissions_beyond_the_cap_are_counted_not_dropped() {
    // A turn that left 50 large artefacts names 32 and counts the other 18,
    // so a reader can say "50 files not captured" rather than a false 32.
    let mut checkpoint = payload();
    let git = checkpoint.git.as_mut().expect("git");
    git.complete = false;
    git.omitted = (0..MAX_CHECKPOINT_OMITTED)
        .map(|index| CodingSessionCheckpointOmission {
            path: format!("big/{index}.bin"),
            reason: CodingSessionCheckpointOmissionReason::TooLarge,
        })
        .collect();
    git.omitted_not_listed = 18;
    let content = encode_coding_session_checkpoint(&checkpoint).expect("counted overflow");
    let decoded = decode_coding_session_checkpoint(&content).expect("decode");
    let git = decoded.git.expect("git");
    assert_eq!(git.omitted.len() as u64 + git.omitted_not_listed, 50);

    // A count alone still makes the capture incomplete.
    let mut checkpoint = payload();
    checkpoint.git.as_mut().expect("git").omitted_not_listed = 1;
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("complete + count");
    assert!(error.contains("omittedNotListed"), "{error}");
    let mut raw = value();
    raw["git"]["omittedNotListed"] = json!(1);
    refused(&raw, "omittedNotListed");

    // The count is a non-negative safe integer.
    let mut raw = value();
    raw["git"]["complete"] = json!(false);
    raw["git"]["omittedNotListed"] = json!(-1);
    assert!(decode_value(&raw).is_err());
    raw["git"]["omittedNotListed"] = json!(MAX_CHECKPOINT_SAFE_INTEGER + 1);
    refused(&raw, "omittedNotListed must be a safe integer");
}

#[test]
fn content_is_capped_at_32_kib() {
    let raw = value();
    let base = raw.to_string().len();
    // Pad with whitespace, which JSON permits and the cap still counts.
    let exact = format!(
        "{}{}",
        " ".repeat(MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES - base),
        raw
    );
    assert_eq!(exact.len(), MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES);
    decode_coding_session_checkpoint(&exact).expect("32 KiB exactly decodes");
    let over = format!(" {exact}");
    let error = decode_coding_session_checkpoint(&over).expect_err("32 KiB + 1");
    assert!(error.contains("exceeds 32768 bytes"), "{error}");

    // The encoder refuses a payload whose own serialization is too large.
    let mut checkpoint = payload();
    checkpoint.files = (0..MAX_CHECKPOINT_FILES)
        .map(|index| CodingSessionCheckpointFile {
            path: format!("{}/{index}", "p".repeat(200)),
            ..file(index)
        })
        .collect();
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("too large");
    assert!(error.contains("exceeds 32768 bytes"), "{error}");
}

#[test]
fn the_unavailable_sentence_is_capped_at_512_bytes() {
    let mut checkpoint = unavailable_payload();
    checkpoint.unavailable.as_mut().expect("set").sentence = "s".repeat(512);
    encode_coding_session_checkpoint(&checkpoint).expect("512 B fits");
    checkpoint.unavailable.as_mut().expect("set").sentence = "s".repeat(513);
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("513 B");
    assert!(error.contains("exceeds 512 bytes"), "{error}");
    checkpoint.unavailable.as_mut().expect("set").sentence = "two\nlines".into();
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

#[test]
fn identifiers_are_capped_at_512_bytes() {
    let mut checkpoint = payload();
    checkpoint.turn_id = Some("t".repeat(512));
    encode_coding_session_checkpoint(&checkpoint).expect("512 B fits");
    checkpoint.turn_id = Some("t".repeat(513));
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
    let mut checkpoint = payload();
    checkpoint.session.session_id = "s".repeat(513);
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
    checkpoint.session.session_id = " ".into();
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

#[test]
fn coverage_must_be_ordered_positive_safe_integers() {
    for (from, through) in [(0, 5), (6, 5), (1, MAX_CHECKPOINT_SAFE_INTEGER + 1)] {
        let mut checkpoint = payload();
        checkpoint.coverage = CodingSessionCheckpointCoverage {
            from_seq: from,
            through_seq: through,
        };
        assert!(
            encode_coding_session_checkpoint(&checkpoint).is_err(),
            "{from}..{through} must be refused"
        );
    }
    let mut checkpoint = payload();
    checkpoint.coverage = CodingSessionCheckpointCoverage {
        from_seq: 7,
        through_seq: 7,
    };
    encode_coding_session_checkpoint(&checkpoint).expect("a one-item range is fine");
    let mut raw = value();
    raw["coverage"]["fromSeq"] = json!(-1);
    assert!(decode_value(&raw).is_err());
    let mut checkpoint = payload();
    checkpoint.session.generation = 0;
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

// ── Exact keys, at every level ──────────────────────────────────────────────

/// Selects one nested object of a payload `Value`.
type Pointer = fn(&mut Value) -> &mut Value;

#[test]
fn an_unknown_key_is_refused_at_every_nesting_level() {
    let pointers: [(&str, Pointer); 7] = [
        ("payload", |raw| raw),
        ("session", |raw| &mut raw["session"]),
        ("coverage", |raw| &mut raw["coverage"]),
        ("git", |raw| &mut raw["git"]),
        ("files entry", |raw| &mut raw["files"][0]),
        ("git.omitted entry", |raw| &mut raw["git"]["omitted"][0]),
        ("unavailable", |raw| &mut raw["unavailable"]),
    ];
    for (what, at) in pointers {
        let mut raw = match what {
            "unavailable" => serde_json::to_value(unavailable_payload()).expect("value"),
            _ => value(),
        };
        if what == "git.omitted entry" {
            raw["git"]["complete"] = json!(false);
            raw["git"]["omitted"] = json!([{"path": "big.bin", "reason": "too_large"}]);
            decode_value(&raw).expect("the base for this level decodes");
        }
        at(&mut raw)
            .as_object_mut()
            .expect("object")
            .insert("extra".into(), json!(1));
        refused(&raw, "unsupported field \"extra\"");
    }
}

#[test]
fn absent_is_not_null() {
    // A nullable key may be null but never absent.
    for key in ["turnId", "git", "unavailable", "summary"] {
        let mut raw = value();
        raw.as_object_mut().expect("object").remove(key);
        refused(&raw, &format!("missing {key:?}"));
    }
    for key in ["head", "branch", "baseTree", "outsideTurn"] {
        let mut raw = value();
        raw["git"].as_object_mut().expect("git").remove(key);
        refused(&raw, &format!("missing {key:?}"));
        let mut raw = value();
        raw["git"][key] = Value::Null;
        decode_value(&raw).expect("null is a value for a nullable key");
    }
    for key in ["from", "additions", "deletions"] {
        let mut raw = value();
        raw["files"][0].as_object_mut().expect("file").remove(key);
        refused(&raw, &format!("missing {key:?}"));
    }
    // A required key may be neither null nor absent.
    for key in [
        "schema",
        "session",
        "reason",
        "coverage",
        "files",
        "filesNotListed",
        "restorable",
    ] {
        let mut raw = value();
        raw[key] = Value::Null;
        refused(&raw, "requires a value");
        let mut raw = value();
        raw.as_object_mut().expect("object").remove(key);
        refused(&raw, &format!("missing {key:?}"));
    }
    for key in ["tree", "commit", "complete", "omitted", "omittedNotListed"] {
        let mut raw = value();
        raw["git"][key] = Value::Null;
        refused(&raw, "requires a value");
    }
}

#[test]
fn closed_vocabularies_refuse_unknown_tokens_by_name() {
    let mut raw = value();
    raw["reason"] = json!("checkpoint");
    refused(&raw, "unsupported token \"checkpoint\"");
    let mut raw = value();
    raw["files"][0]["status"] = json!("copied");
    refused(&raw, "unsupported token \"copied\"");
    let mut raw = serde_json::to_value(unavailable_payload()).expect("value");
    raw["unavailable"]["code"] = json!("NO_GIT");
    refused(&raw, "unsupported token \"NO_GIT\"");
    let mut raw = value();
    raw["git"]["complete"] = json!(false);
    raw["git"]["omitted"] = json!([{"path": "a", "reason": "secret"}]);
    refused(&raw, "unsupported token \"secret\"");
    let mut raw = value();
    raw["schema"] = json!("buzz-coding-session-checkpoint/v2");
    refused(&raw, "unsupported coding-session checkpoint schema");
}

#[test]
fn a_duplicate_key_is_refused() {
    let content = value().to_string().replacen(
        "\"restorable\":false",
        "\"restorable\":false,\"restorable\":true",
        1,
    );
    assert!(decode_coding_session_checkpoint(&content).is_err());
}

#[test]
fn summary_must_be_null_in_v1() {
    let mut raw = value();
    raw["summary"] = json!("context summary");
    refused(&raw, "summary must be null in v1");
    let mut checkpoint = payload();
    checkpoint.summary = Some("x".into());
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

// ── Cross-field rules ───────────────────────────────────────────────────────

#[test]
fn exactly_one_of_git_and_unavailable_is_set() {
    let mut both = payload();
    both.unavailable = unavailable_payload().unavailable;
    let error = encode_coding_session_checkpoint(&both).expect_err("both");
    assert!(error.contains("both set"), "{error}");

    let mut neither = payload();
    neither.git = None;
    neither.files = vec![];
    let error = encode_coding_session_checkpoint(&neither).expect_err("neither");
    assert!(error.contains("both null"), "{error}");

    let mut raw = value();
    raw["unavailable"] = json!({"code": "TIMED_OUT", "sentence": "Capture timed out."});
    refused(&raw, "both set");
}

#[test]
fn an_unavailable_checkpoint_lists_no_files() {
    let mut checkpoint = unavailable_payload();
    checkpoint.files = vec![file(0)];
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
    let mut checkpoint = unavailable_payload();
    checkpoint.files_not_listed = 3;
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

#[test]
fn turn_id_is_null_only_for_a_pre_rewind_capture() {
    let mut pre_rewind = payload();
    pre_rewind.reason = CodingSessionCheckpointReason::PreRewind;
    pre_rewind.turn_id = None;
    let content = encode_coding_session_checkpoint(&pre_rewind).expect("pre_rewind, no turn");
    assert!(content.contains("\"reason\":\"pre_rewind\""));
    assert!(content.contains("\"turnId\":null"));

    let mut turn = payload();
    turn.turn_id = None;
    let error = encode_coding_session_checkpoint(&turn).expect_err("turn without turnId");
    assert!(error.contains("turnId must be present"), "{error}");
    let mut raw = value();
    raw["turnId"] = Value::Null;
    refused(&raw, "turnId must be present");
}

#[test]
fn a_rename_and_only_a_rename_names_from() {
    let mut checkpoint = payload();
    checkpoint.files[2].from = None;
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
    let mut checkpoint = payload();
    checkpoint.files[0].from = Some("src/old.rs".into());
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
    let mut checkpoint = payload();
    checkpoint.files[2].from = Some(checkpoint.files[2].path.clone());
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

#[test]
fn a_path_is_listed_once_and_omissions_make_a_capture_incomplete() {
    let mut checkpoint = payload();
    checkpoint.files[1].path = checkpoint.files[0].path.clone();
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());

    let mut checkpoint = payload();
    checkpoint.git.as_mut().expect("git").omitted = vec![CodingSessionCheckpointOmission {
        path: "big.bin".into(),
        reason: CodingSessionCheckpointOmissionReason::TooLarge,
    }];
    let error = encode_coding_session_checkpoint(&checkpoint).expect_err("complete + omitted");
    assert!(error.contains("complete must be false"), "{error}");
}

#[test]
fn object_ids_must_be_lowercase_full_hex() {
    for bad in [
        "ABCDEF".repeat(7)[..40].to_owned(),
        "a".repeat(39),
        "g".repeat(40),
    ] {
        let mut checkpoint = payload();
        checkpoint.git.as_mut().expect("git").tree = bad.clone();
        assert!(
            encode_coding_session_checkpoint(&checkpoint).is_err(),
            "{bad} must be refused"
        );
    }
    let mut checkpoint = payload();
    checkpoint.git.as_mut().expect("git").branch = Some("refs/heads/main".into());
    assert!(encode_coding_session_checkpoint(&checkpoint).is_err());
}

// ── Paths ───────────────────────────────────────────────────────────────────

const REFUSED_PATHS: &[&str] = &[
    "/Users/brian/Projects/x/src/main.rs",
    "\\server\\share\\a.txt",
    "C:\\work\\a.txt",
    "C:/work/a.txt",
    "../outside.txt",
    "src/../../etc/passwd",
    "src/..",
    "a\0b",
    "a\nb",
    "refs/beekeeper/checkpoints/s/1/58",
    "refs/heads/main",
    "secrets/[elided private context: 24 bytes, sha256:d68b]",
    "keys/ghp_••••••••abcd",
    "[redacted 148 B · #1]",
    "",
    "src//main.rs",
    "./src/main.rs",
    "src/",
];

#[test]
fn every_unpublishable_path_is_refused_when_encoding_and_when_decoding() {
    for path in REFUSED_PATHS {
        assert!(
            !is_publishable_checkpoint_path(path),
            "{path:?} must not be publishable"
        );
        // As a listed file.
        let mut checkpoint = payload();
        checkpoint.files[0].path = (*path).to_owned();
        assert!(
            encode_coding_session_checkpoint(&checkpoint).is_err(),
            "encode must refuse files path {path:?}"
        );
        let mut raw = value();
        raw["files"][0]["path"] = json!(path);
        assert!(
            decode_value(&raw).is_err(),
            "decode must refuse files path {path:?}"
        );
        // As a rename source.
        let mut raw = value();
        raw["files"][2]["from"] = json!(path);
        assert!(
            decode_value(&raw).is_err(),
            "decode must refuse from {path:?}"
        );
        // As an omission.
        let mut raw = value();
        raw["git"]["complete"] = json!(false);
        raw["git"]["omitted"] = json!([{"path": path, "reason": "unreadable"}]);
        assert!(
            decode_value(&raw).is_err(),
            "decode must refuse omitted path {path:?}"
        );
    }
}

#[test]
fn ordinary_repo_relative_paths_are_publishable() {
    for path in [
        "README.md",
        "src/main.rs",
        ".github/workflows/ci.yml",
        "a b/c d.txt",
        "docs/ünïcode.md",
        "..hidden/file",
        "x/...",
        "refsx/a",
    ] {
        assert!(is_publishable_checkpoint_path(path), "{path:?}");
    }
}

// ── Event envelope ──────────────────────────────────────────────────────────

#[test]
fn the_tags_are_exact_and_in_order() {
    let tags = tags_for(&payload());
    assert_eq!(
        tags,
        vec![
            vec!["h".to_owned(), CHANNEL.to_owned()],
            vec!["csck-v".to_owned(), "csck1-1".to_owned()],
            vec![
                "cs-target".to_owned(),
                "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed36:\
                 f543d7bd-6059-44e3-a50b-4275d8440dd51:2"
                    .to_owned()
            ],
            vec!["csck-seq".to_owned(), "58".to_owned()],
            vec![
                "csck-key".to_owned(),
                "coding-session-checkpoint/v1|16:claude-agent-acp16:1958c6c448e05eed36:\
                 f543d7bd-6059-44e3-a50b-4275d8440dd51:22:584:turn"
                    .to_owned()
            ],
        ]
    );
    assert_eq!(tags[2][1], coding_session_target_key(&target()));
}

#[test]
fn a_pre_rewind_and_a_turn_at_the_same_seq_have_different_identities() {
    // Turn 3 ends at seq 58; a rewind's pre_rewind capture covers through the
    // same seq. If the two shared a key, every reader would drop the
    // pre_rewind as a duplicate and the rewind could not be undone.
    let turn = payload();
    let pre_rewind = CodingSessionCheckpointPayload {
        turn_id: None,
        reason: CodingSessionCheckpointReason::PreRewind,
        ..payload()
    };
    assert_eq!(turn.coverage.through_seq, pre_rewind.coverage.through_seq);
    let turn_tags = tags_for(&turn);
    let pre_rewind_tags = tags_for(&pre_rewind);
    assert_eq!(turn_tags[3], pre_rewind_tags[3], "csck-seq is the same");
    assert_ne!(turn_tags[4][1], pre_rewind_tags[4][1], "csck-key differs");
    assert!(pre_rewind_tags[4][1].ends_with("2:5810:pre_rewind"));
    assert_eq!(
        pre_rewind_tags[4][1],
        coding_session_checkpoint_semantic_key(
            &target(),
            CodingSessionCheckpointReason::PreRewind,
            58
        )
    );

    // The key is bound to the content's reason: a turn's key on a pre_rewind
    // payload is refused.
    let content = encode_coding_session_checkpoint(&pre_rewind).expect("encode");
    validate_coding_session_checkpoint_event(&sign(&content, pre_rewind_tags.clone()))
        .expect("pre_rewind validates with its own key");
    let mut borrowed = pre_rewind_tags;
    borrowed[4][1] = turn_tags[4][1].clone();
    let error = validate_coding_session_checkpoint_event(&sign(&content, borrowed))
        .expect_err("a turn key on a pre_rewind");
    assert!(error.contains("csck-key"), "{error}");
}

#[test]
fn a_well_formed_event_validates_and_decodes() {
    let content = encode_coding_session_checkpoint(&payload()).expect("encode");
    let event = sign(&content, tags_for(&payload()));
    assert_eq!(
        validate_coding_session_checkpoint_event(&event).expect("valid"),
        payload()
    );
    assert_eq!(
        decode_coding_session_checkpoint_event(&event).expect("decodes"),
        payload()
    );
}

#[test]
fn a_bad_envelope_is_refused() {
    let content = encode_coding_session_checkpoint(&payload()).expect("encode");
    let good = tags_for(&payload());

    let mut reordered = good.clone();
    reordered.swap(3, 4);
    assert!(validate_coding_session_checkpoint_event(&sign(&content, reordered)).is_err());

    let mut missing = good.clone();
    missing.pop();
    assert!(validate_coding_session_checkpoint_event(&sign(&content, missing)).is_err());

    let mut extra = good.clone();
    extra.push(vec!["t".into(), "x".into()]);
    assert!(validate_coding_session_checkpoint_event(&sign(&content, extra)).is_err());

    for (index, wrong) in [
        (0, "D3E440EA-89F8-4AEE-8A02-17EDC3E7272E"),
        (1, "csck1-2"),
        (2, "coding-session/v1|wrong"),
        (3, "57"),
        (4, "coding-session-checkpoint/v1|wrong"),
    ] {
        let mut tags = good.clone();
        tags[index][1] = wrong.into();
        assert!(
            validate_coding_session_checkpoint_event(&sign(&content, tags)).is_err(),
            "tag {index} = {wrong:?} must be refused"
        );
    }

    let mut three_fields = good.clone();
    three_fields[1].push("extra".into());
    assert!(validate_coding_session_checkpoint_event(&sign(&content, three_fields)).is_err());

    let wrong_kind = EventBuilder::new(Kind::Custom(44225), content.clone())
        .tags(
            good.iter()
                .map(|parts| Tag::parse(parts.clone()).expect("tag"))
                .collect::<Vec<_>>(),
        )
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    assert!(validate_coding_session_checkpoint_event(&wrong_kind).is_err());

    let bad_content = sign("{}", good);
    assert!(validate_coding_session_checkpoint_event(&bad_content).is_err());
}
