//! Typed builder and parser for NIP-CSCK turn checkpoints (kind 44231).

use buzz_core::coding_session_checkpoint::{
    coding_session_checkpoint_tags, encode_coding_session_checkpoint,
    validate_coding_session_checkpoint_event, CodingSessionCheckpointPayload,
};
use buzz_core::kind::KIND_CODING_SESSION_CHECKPOINT;
use nostr::{Event, EventBuilder, Kind, Tag};
use uuid::Uuid;

use crate::SdkError;

/// Build a structurally validated kind 44231 checkpoint.
///
/// Emits the exact five-tag envelope NIP-CSCK requires — `h`, `csck-v`,
/// `cs-target`, `csck-seq`, `csck-key`, in that order — and leaves signing and
/// publishing to the caller. The caller must sign with the key that signs the
/// same generation's 44225 transcript items; a reader rejects any other
/// signer, and this builder cannot know which key that is.
///
/// Everything the parser refuses, the builder refuses first — including every
/// path that is not repo-relative — so a provider never signs bytes the relay
/// and every reader would reject.
pub fn build_coding_session_checkpoint(
    channel_id: Uuid,
    payload: &CodingSessionCheckpointPayload,
) -> Result<EventBuilder, SdkError> {
    let content = encode_coding_session_checkpoint(payload).map_err(SdkError::InvalidInput)?;
    let tags = coding_session_checkpoint_tags(&channel_id, payload)
        .into_iter()
        .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidTag(error.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_CHECKPOINT as u16), content).tags(tags))
}

/// Parse and validate a signed kind 44231 event's exact envelope. Structure
/// only: the signer is the caller's check.
pub fn parse_coding_session_checkpoint(
    event: &Event,
) -> Result<CodingSessionCheckpointPayload, SdkError> {
    validate_coding_session_checkpoint_event(event).map_err(SdkError::InvalidInput)
}

#[cfg(test)]
mod tests {
    use buzz_core::coding_session_checkpoint::{
        CodingSessionCheckpointCoverage, CodingSessionCheckpointFile,
        CodingSessionCheckpointFileStatus, CodingSessionCheckpointGit,
        CodingSessionCheckpointReason, CODING_SESSION_CHECKPOINT_SCHEMA,
    };
    use buzz_core::coding_session_command::CodingSessionTarget;
    use nostr::Keys;

    use super::*;

    const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

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

    #[test]
    fn builder_emits_the_exact_envelope_and_round_trips() {
        let channel = Uuid::parse_str(CHANNEL).expect("uuid");
        let event = build_coding_session_checkpoint(channel, &checkpoint())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        assert_eq!(event.kind.as_u16(), 44231);
        let tags: Vec<Vec<String>> = event.tags.iter().map(|tag| tag.clone().to_vec()).collect();
        assert_eq!(
            tags,
            vec![
                vec!["h".to_owned(), CHANNEL.to_owned()],
                vec!["csck-v".to_owned(), "csck1-1".to_owned()],
                vec![
                    "cs-target".to_owned(),
                    "coding-session/v1|10:provider-a10:instance-19:session-11:1".to_owned(),
                ],
                vec!["csck-seq".to_owned(), "58".to_owned()],
                vec![
                    "csck-key".to_owned(),
                    "coding-session-checkpoint/v1|10:provider-a10:instance-19:session-11:12:584:turn"
                        .to_owned(),
                ],
            ]
        );
        assert_eq!(
            tags[2][1],
            crate::coding_session::coding_session_target_key(&checkpoint().session)
        );
        assert_eq!(
            parse_coding_session_checkpoint(&event).expect("parse"),
            checkpoint()
        );
    }

    #[test]
    fn builder_refuses_what_the_parser_refuses() {
        let channel = Uuid::parse_str(CHANNEL).expect("uuid");
        let mut absolute = checkpoint();
        absolute.files[0].path = "/Users/someone/repo/src/lib.rs".to_owned();
        assert!(build_coding_session_checkpoint(channel, &absolute).is_err());

        let mut turnless = checkpoint();
        turnless.turn_id = None;
        assert!(build_coding_session_checkpoint(channel, &turnless).is_err());

        let mut neither = checkpoint();
        neither.git = None;
        neither.files.clear();
        assert!(build_coding_session_checkpoint(channel, &neither).is_err());
    }
}
