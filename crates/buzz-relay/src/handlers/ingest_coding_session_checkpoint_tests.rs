//! Relay ingest of NIP-CSCK turn checkpoints (kind 44231): structure only.

use buzz_core::coding_session_checkpoint::{
    validate_coding_session_checkpoint_event, CodingSessionCheckpointCoverage,
    CodingSessionCheckpointFile, CodingSessionCheckpointFileStatus, CodingSessionCheckpointGit,
    CodingSessionCheckpointPayload, CodingSessionCheckpointReason,
    CODING_SESSION_CHECKPOINT_SCHEMA, MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES,
};
use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_sdk::coding_session_checkpoint::build_coding_session_checkpoint;
use nostr::{EventBuilder, Kind, Tag};

use super::*;

fn checkpoint() -> CodingSessionCheckpointPayload {
    CodingSessionCheckpointPayload {
        schema: CODING_SESSION_CHECKPOINT_SCHEMA.to_owned(),
        session: CodingSessionTarget {
            driver: "provider-a".to_owned(),
            instance_id: "instance-1".to_owned(),
            session_id: "session-1".to_owned(),
            generation: 1,
        },
        turn_id: Some("turn-7".to_owned()),
        reason: CodingSessionCheckpointReason::Turn,
        coverage: CodingSessionCheckpointCoverage {
            from_seq: 41,
            through_seq: 58,
        },
        git: Some(CodingSessionCheckpointGit {
            head: None,
            branch: None,
            base_tree: None,
            tree: "a".repeat(40),
            commit: "b".repeat(40),
            outside_turn: None,
            complete: true,
            omitted: vec![],
            omitted_not_listed: 0,
        }),
        files: vec![CodingSessionCheckpointFile {
            path: "src/lib.rs".to_owned(),
            status: CodingSessionCheckpointFileStatus::Modified,
            from: None,
            additions: Some(2),
            deletions: Some(2),
        }],
        files_not_listed: 0,
        restorable: false,
        unavailable: None,
        summary: None,
    }
}

fn valid_event(channel: Uuid) -> nostr::Event {
    build_coding_session_checkpoint(channel, &checkpoint())
        .expect("checkpoint builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign checkpoint")
}

fn tags_of(event: &nostr::Event) -> Vec<Vec<String>> {
    event.tags.iter().map(|tag| tag.clone().to_vec()).collect()
}

fn resign(content: String, tags: Vec<Vec<String>>) -> nostr::Event {
    let tags = tags
        .into_iter()
        .map(|parts| Tag::parse(parts).expect("tag"))
        .collect::<Vec<_>>();
    EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_CHECKPOINT as u16), content)
        .tags(tags)
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign")
}

/// 44231 is a channel-scoped, strictly-gated coding-session write, and a
/// well-formed one passes the core validator the ingest arm calls. The signer
/// is a generated key: whether it was the generation's provider key is the
/// reader's question, not the relay's.
#[test]
fn checkpoint_uses_core_validator_and_strict_membership_admission() {
    let valid = valid_event(Uuid::new_v4());
    assert!(validate_coding_session_checkpoint_event(&valid).is_ok());
    assert_eq!(
        required_scope_for_kind(KIND_CODING_SESSION_CHECKPOINT, &valid).unwrap(),
        Scope::MessagesWrite
    );
    assert!(requires_h_channel_scope(KIND_CODING_SESSION_CHECKPOINT));
    assert!(is_coding_session_kind(KIND_CODING_SESSION_CHECKPOINT));
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_CHECKPOINT
    ));
}

#[test]
fn checkpoint_with_tags_out_of_order_is_refused() {
    let valid = valid_event(Uuid::new_v4());
    let mut tags = tags_of(&valid);
    tags.swap(1, 2);
    let event = resign(valid.content.clone(), tags);
    assert!(validate_coding_session_checkpoint_event(&event).is_err());
}

#[test]
fn checkpoint_with_a_wrong_semantic_key_is_refused() {
    let valid = valid_event(Uuid::new_v4());
    let mut tags = tags_of(&valid);
    assert_eq!(tags[4][0], "csck-key");
    tags[4][1].push('9');
    let event = resign(valid.content.clone(), tags);
    let error = validate_coding_session_checkpoint_event(&event).expect_err("wrong key");
    assert!(error.contains("csck-key"), "{error}");
}

#[test]
fn checkpoint_content_over_the_cap_is_refused() {
    let valid = valid_event(Uuid::new_v4());
    let padded = format!(
        "{}{}",
        valid.content,
        " ".repeat(MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES + 1 - valid.content.len())
    );
    assert!(padded.len() > MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES);
    let event = resign(padded, tags_of(&valid));
    assert!(validate_coding_session_checkpoint_event(&event).is_err());
}
