//! Typed builders and parser for NIP-PW work records (kind 44249).
//!
//! One builder per record type, each returning the exact six-tag envelope the
//! contract fixes — `h`, `d`, `a`, `pwk-v`, `pwk-genesis`, `pwk-type`, in that
//! order — and leaving signing and publishing to the caller.
//!
//! The builders do **not** check that the caller holds `may_lead`: the
//! signature is the author, and whether that author held standing is the
//! consuming fold's question against the accepted NIP-CSAT chain, exactly as
//! for kinds 44244 through 44247. What they do check is everything the
//! parser and the relay will check, so no author ever signs bytes both will
//! reject.

use buzz_core::kind::KIND_PROJECT_WORK_RECORD;
use buzz_core::project_work::{
    validate_project_work_envelope, validate_project_work_payload, ProjectWorkAssignmentBound,
    ProjectWorkBody, ProjectWorkDeclared, ProjectWorkEvent, ProjectWorkEvidenceBound,
    ProjectWorkPayload, ProjectWorkRecordType, PROJECT_WORK_SCHEMA,
};
use nostr::{Event, EventBuilder, Kind, Tag};

use crate::SdkError;

/// The envelope fields every work record shares.
///
/// One struct rather than three repeated parameter lists, so a builder cannot
/// be given a session and a genesis that a sibling builder would have named
/// differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWorkEnvelope {
    /// The session's channel uuid — the membership gate, as for 44244.
    pub channel_ref: String,
    /// The session uuid; becomes both the `d` tag and `content.sessionRef`.
    pub session_ref: String,
    /// The session genesis event id.
    pub genesis_ref: String,
    /// The canonical `30621:<64-hex>:<d>` project coordinate.
    pub project_ref: String,
}

/// Build a `work.declared`: a lead adopts a committed plan as this work's
/// contract.
///
/// # Errors
///
/// Returns [`SdkError::InvalidInput`] when the record would be refused by the
/// relay or the fold, and [`SdkError::InvalidTag`] when a tag cannot be built.
pub fn build_project_work_declared(
    envelope: &ProjectWorkEnvelope,
    body: ProjectWorkDeclared,
) -> Result<EventBuilder, SdkError> {
    build(envelope, ProjectWorkBody::Declared(body))
}

/// Build a `work.assignment_bound`: which criteria a 44244 assignment owes.
///
/// # Errors
///
/// As [`build_project_work_declared`].
pub fn build_project_work_assignment_bound(
    envelope: &ProjectWorkEnvelope,
    body: ProjectWorkAssignmentBound,
) -> Result<EventBuilder, SdkError> {
    build(envelope, ProjectWorkBody::AssignmentBound(body))
}

/// Build a `work.evidence_bound`: which evidence answers which criteria, at
/// which artifact commit.
///
/// # Errors
///
/// As [`build_project_work_declared`].
pub fn build_project_work_evidence_bound(
    envelope: &ProjectWorkEnvelope,
    body: ProjectWorkEvidenceBound,
) -> Result<EventBuilder, SdkError> {
    build(envelope, ProjectWorkBody::EvidenceBound(body))
}

/// Assemble the payload from an envelope and a body.
///
/// Public because a caller that has already built a payload — the CLI's
/// `--example` path, which serializes a constructed value rather than a
/// hand-typed string — needs the same composition the builders use.
#[must_use]
pub fn project_work_payload(
    envelope: &ProjectWorkEnvelope,
    body: ProjectWorkBody,
) -> ProjectWorkPayload {
    ProjectWorkPayload {
        schema: PROJECT_WORK_SCHEMA.to_owned(),
        session_ref: envelope.session_ref.clone(),
        genesis_ref: envelope.genesis_ref.clone(),
        project_ref: envelope.project_ref.clone(),
        record_type: body.record_type(),
        body,
    }
}

fn build(envelope: &ProjectWorkEnvelope, body: ProjectWorkBody) -> Result<EventBuilder, SdkError> {
    validate_channel_ref(&envelope.channel_ref)?;
    let payload = project_work_payload(envelope, body);
    validate_project_work_payload(&payload)
        .map_err(|refusal| SdkError::InvalidInput(refusal.to_string()))?;
    let content = payload
        .canonical_content()
        .map_err(|refusal| SdkError::InvalidInput(refusal.to_string()))?;
    let tags = payload
        .canonical_tags(&envelope.channel_ref)
        .into_iter()
        .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidTag(error.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBuilder::new(Kind::Custom(KIND_PROJECT_WORK_RECORD as u16), content).tags(tags))
}

/// Check `channel_ref` against the exact rule the envelope validator applies
/// to the `h` tag: a canonical lowercase hyphenated uuid.
fn validate_channel_ref(channel_ref: &str) -> Result<(), SdkError> {
    let parsed = uuid::Uuid::parse_str(channel_ref)
        .map_err(|_| SdkError::InvalidInput("h must be a UUID".to_owned()))?;
    if parsed.to_string() != channel_ref {
        return Err(SdkError::InvalidInput(
            "h must be a lowercase canonical UUID".to_owned(),
        ));
    }
    Ok(())
}

/// Parse and validate a signed kind 44249 event's exact envelope.
///
/// # Errors
///
/// Returns [`SdkError::InvalidInput`] carrying the refusal's stable code and
/// message.
pub fn parse_project_work_record(event: &Event) -> Result<ProjectWorkPayload, SdkError> {
    validate_project_work_envelope(&ProjectWorkEvent::from(event))
        .map_err(|refusal| SdkError::InvalidInput(refusal.to_string()))
}

/// The record type a signed event declares in its `pwk-type` tag, without
/// decoding its content.
///
/// The cheap read a router wants before it commits to a full decode.
#[must_use]
pub fn project_work_record_type(event: &Event) -> Option<ProjectWorkRecordType> {
    event
        .tags
        .iter()
        .map(|tag| tag.as_slice())
        .find(|parts| parts.first().map(String::as_str) == Some("pwk-type"))
        .and_then(|parts| parts.get(1))
        .and_then(|value| ProjectWorkRecordType::from_wire(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::project_work::{
        ProjectWorkEvidenceKind, ProjectWorkEvidenceRef, ProjectWorkPlanRef,
    };
    use nostr::Keys;

    const CHANNEL: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    const SESSION: &str = "11111111-2222-4333-8444-555555555555";

    fn envelope() -> ProjectWorkEnvelope {
        ProjectWorkEnvelope {
            channel_ref: CHANNEL.to_owned(),
            session_ref: SESSION.to_owned(),
            genesis_ref: "9e0e".to_owned() + &"0".repeat(60),
            project_ref: format!("30621:{}:kettle", "1ead".to_owned() + &"0".repeat(60)),
        }
    }

    fn declared() -> ProjectWorkDeclared {
        ProjectWorkDeclared {
            work_id: "9d0f0f0f-1111-4222-8333-444444444444".to_owned(),
            goal_ref: "90a1".to_owned() + &"0".repeat(60),
            responsible_actor: "1ead".to_owned() + &"0".repeat(60),
            plan_ref: ProjectWorkPlanRef {
                repository: format!(
                    "30617:{}:pivot-test-beekeeper-agents",
                    "1ead".to_owned() + &"0".repeat(60)
                ),
                commit: "ab".repeat(20),
                path: "plans/kettle.md".to_owned(),
            },
            supersedes: Vec::new(),
        }
    }

    #[test]
    fn the_declaration_builder_emits_the_exact_envelope_and_round_trips() {
        let event = build_project_work_declared(&envelope(), declared())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");

        let payload = parse_project_work_record(&event).expect("parse");
        assert_eq!(payload.record_type, ProjectWorkRecordType::Declared);
        assert_eq!(
            project_work_record_type(&event),
            Some(ProjectWorkRecordType::Declared)
        );

        let tags: Vec<Vec<String>> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        assert_eq!(tags.len(), 6);
        assert_eq!(tags[0], ["h", CHANNEL]);
        assert_eq!(tags[1], ["d", SESSION]);
        assert_eq!(tags[2][0], "a");
        assert_eq!(tags[3], ["pwk-v", PROJECT_WORK_SCHEMA]);
        assert_eq!(tags[4][0], "pwk-genesis");
        assert_eq!(tags[5], ["pwk-type", "work.declared"]);
        // Absent is not null: an unset optional is on the wire as JSON null.
        assert!(
            event.content.contains("\"supersedes\":[]"),
            "{}",
            event.content
        );
    }

    #[test]
    fn the_binding_builders_write_their_nullable_keys_as_null() {
        let event = build_project_work_assignment_bound(
            &envelope(),
            ProjectWorkAssignmentBound {
                declaration_ref: "dec1".to_owned() + &"0".repeat(60),
                criterion_ids: vec!["cli-behaviour".to_owned()],
                assignment_ref: "a551".to_owned() + &"0".repeat(60),
                replaces_binding: None,
            },
        )
        .expect("builder")
        .sign_with_keys(&Keys::generate())
        .expect("sign");
        assert!(
            event.content.contains("\"replacesBinding\":null"),
            "{}",
            event.content
        );
        assert_eq!(
            project_work_record_type(&event),
            Some(ProjectWorkRecordType::AssignmentBound)
        );

        let event = build_project_work_evidence_bound(
            &envelope(),
            ProjectWorkEvidenceBound {
                declaration_ref: "dec1".to_owned() + &"0".repeat(60),
                criterion_ids: vec!["cli-behaviour".to_owned()],
                artifact_commit: "e7".repeat(20),
                evidence_refs: vec![ProjectWorkEvidenceRef {
                    kind: ProjectWorkEvidenceKind::Verdict,
                    event_id: "bed1".to_owned() + &"0".repeat(60),
                }],
                completion_ref: None,
            },
        )
        .expect("builder")
        .sign_with_keys(&Keys::generate())
        .expect("sign");
        assert!(
            event.content.contains("\"completionRef\":null"),
            "{}",
            event.content
        );
        assert!(parse_project_work_record(&event).is_ok());
    }

    #[test]
    fn the_builder_refuses_a_channel_its_own_parser_would_reject() {
        for bad in [
            "not-a-uuid",
            "",
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
            "aaaaaaaabbbb4ccc8dddeeeeeeeeeeee",
        ] {
            let mut envelope = envelope();
            envelope.channel_ref = bad.to_owned();
            assert!(
                build_project_work_declared(&envelope, declared()).is_err(),
                "{bad:?} must never reach a signer"
            );
        }
    }

    #[test]
    fn an_invalid_record_never_reaches_a_signer() {
        // A bare repo id where the full 30617 coordinate belongs.
        let mut body = declared();
        body.plan_ref.repository = "pivot-test-beekeeper-agents".to_owned();
        assert!(build_project_work_declared(&envelope(), body).is_err());

        // A plan path that escapes `plans/`.
        let mut body = declared();
        body.plan_ref.path = "../actions.yml".to_owned();
        assert!(build_project_work_declared(&envelope(), body).is_err());

        // An abbreviated agents commit.
        let mut body = declared();
        body.plan_ref.commit = "ab12345".to_owned();
        assert!(build_project_work_declared(&envelope(), body).is_err());

        // A binding that names no criterion proves nothing.
        assert!(build_project_work_assignment_bound(
            &envelope(),
            ProjectWorkAssignmentBound {
                declaration_ref: "dec1".to_owned() + &"0".repeat(60),
                criterion_ids: Vec::new(),
                assignment_ref: "a551".to_owned() + &"0".repeat(60),
                replaces_binding: None,
            },
        )
        .is_err());
    }
}
