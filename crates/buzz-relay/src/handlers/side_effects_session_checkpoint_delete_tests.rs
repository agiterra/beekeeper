//! A whole-session delete reaches the session's turn checkpoints (kind 44231).
//!
//! A checkpoint carries repo-relative paths, branch names and head SHAs of the
//! session's working tree, and it is signed by the provider rather than by the
//! person deleting the session. Without an admission here, "delete this
//! session" would either be refused as "must be event author" the moment the
//! client named a checkpoint, or — had the client not named them — leave every
//! checkpoint readable after the person was told the session was gone.
//!
//! The admission is by identity, not by kind: a checkpoint belongs to the
//! session only through a `cs-target` that this deletion's own metadata
//! attributed to it, exactly as a 44225 transcript item does.

use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;

const TARGET_A: &str =
    "coding-session/v1|16:claude-agent-acp16:b051835ed3dc579836:ea97f1ab-dd38-47f5-bd86-a293bc4153ba1:1";
const TARGET_B: &str =
    "coding-session/v1|16:claude-agent-acp16:b051835ed3dc579836:99999999-dd38-47f5-bd86-a293bc4153ba1:1";

fn session(channel_id: Uuid) -> AuthorizedSessionDeletion {
    AuthorizedSessionDeletion {
        session_ref: "d9c338f4-85ec-4d61-aac6-9ffbe5b1bf42".to_string(),
        channel_id,
        genesis_id: vec![0; 32],
        execution_targets: [TARGET_A.to_string()].into_iter().collect(),
    }
}

/// A checkpoint as a provider signs it: the NIP-CSCK five-tag envelope. The
/// content is not decoded by the admission, so it stays minimal.
fn checkpoint(channel_id: Uuid, target: &str) -> Event {
    let keys = Keys::generate();
    let channel = channel_id.to_string();
    EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_CODING_SESSION_CHECKPOINT as u16),
        "{}",
    )
    .tags([
        Tag::parse(["h", channel.as_str()]).expect("h tag"),
        Tag::parse(["csck-v", "csck1-1"]).expect("version tag"),
        Tag::parse(["cs-target", target]).expect("target tag"),
        Tag::parse(["csck-seq", "4"]).expect("seq tag"),
        Tag::parse(["csck-key", "coding-session-checkpoint/v1|x"]).expect("key tag"),
    ])
    .sign_with_keys(&keys)
    .expect("sign checkpoint")
}

#[test]
fn a_session_delete_reaches_its_own_checkpoints() {
    let channel_id = Uuid::new_v4();
    assert!(
        session_deletion_admits(&checkpoint(channel_id, TARGET_A), &session(channel_id)),
        "a checkpoint of an execution this deletion's metadata attributed to the session \
         must be deletable with it, or its file list outlives the session"
    );
}

#[test]
fn a_checkpoint_of_another_execution_is_not_reached() {
    let channel_id = Uuid::new_v4();
    assert!(
        !session_deletion_admits(&checkpoint(channel_id, TARGET_B), &session(channel_id)),
        "a checkpoint whose cs-target no named metadata attributed here belongs to someone else"
    );
}

#[test]
fn a_checkpoint_in_another_channel_is_not_reached() {
    let channel_id = Uuid::new_v4();
    assert!(
        !session_deletion_admits(&checkpoint(Uuid::new_v4(), TARGET_A), &session(channel_id)),
        "the channel is checked first: a matching cs-target elsewhere is not this session's"
    );
}

#[test]
fn a_foreign_checkpoint_gets_the_session_specific_refusal() {
    // A checkpoint named alongside a genesis but not admitted must be refused
    // with the sentence that says it belongs to another session, not with the
    // generic authorship one.
    assert!(coding_session_scoped_kind(
        buzz_core::kind::KIND_CODING_SESSION_CHECKPOINT
    ));
}

#[test]
fn checkpoints_keep_their_ordinary_deletion_rules() {
    // A checkpoint is provider content, not a permanent identity anchor: the
    // provider may still delete its own outside a whole-session delete.
    assert!(
        refuse_permanent_identity_deletion(buzz_core::kind::KIND_CODING_SESSION_CHECKPOINT).is_ok()
    );
}
