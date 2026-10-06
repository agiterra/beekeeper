//! Tests for the kind:44249 ingest arm.
//!
//! These check the admission *decisions* the relay makes: which scope a work
//! record needs, that it is channel-scoped through `h` like its 4424x
//! siblings, that it is **not** project-`a`-scoped, and that a structurally
//! bad record is refused at ingest by a named reason.

use super::*;

use beekeeper_core::kind::{is_project_a_scoped_kind, KIND_PROJECT_WORK_RECORD};
use nostr::{EventBuilder, Keys, Kind, Tag};

use crate::handlers::ingest::{
    is_coding_session_kind, is_global_only_kind, requires_h_channel_scope,
    requires_strict_coding_session_membership,
};

const CHANNEL: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";

fn hex(prefix: &str) -> String {
    format!("{prefix}{}", "0".repeat(64 - prefix.len()))
}

fn record(record_type: &str, body: &str) -> Event {
    let genesis = hex("9e0e");
    let project = format!("30621:{}:kettle", hex("1ead"));
    let content = format!(
        "{{\"schema\":\"buzz-project-work/v1\",\"sessionRef\":\"{SESSION}\",\
         \"genesisRef\":\"{genesis}\",\"projectRef\":\"{project}\",\
         \"type\":\"{record_type}\",\"body\":{body}}}"
    );
    let tags = [
        ["h", CHANNEL],
        ["d", SESSION],
        ["a", project.as_str()],
        ["pwk-v", "buzz-project-work/v1"],
        ["pwk-genesis", genesis.as_str()],
        ["pwk-type", record_type],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).expect("tag"))
    .collect::<Vec<_>>();
    EventBuilder::new(Kind::Custom(KIND_PROJECT_WORK_RECORD as u16), content)
        .tags(tags)
        .sign_with_keys(&Keys::generate())
        .expect("sign")
}

fn declaration() -> Event {
    record(
        "work.declared",
        &format!(
            "{{\"workId\":\"9d0f0f0f-1111-4222-8333-444444444444\",\"goalRef\":\"{}\",\
             \"decisionRef\":null,\"responsibleActor\":\"{}\",\"planRef\":{{\"repository\":\"30617:{}:pivot-test-beekeeper-agents\",\
             \"commit\":\"{}\",\"path\":\"plans/kettle.md\"}},\"supersedes\":[]}}",
            hex("90a1"),
            hex("1ead"),
            hex("1ead"),
            "ab".repeat(20)
        ),
    )
}

#[test]
fn a_well_formed_work_record_is_admitted_structurally() {
    validate_project_work_record(&declaration()).expect("a conforming record is admitted");
}

#[test]
fn the_work_kind_is_channel_scoped_like_its_siblings() {
    // `required_scope_for_kind` is private to `ingest`; its 44249 arm
    // (`Scope::MessagesWrite`, the same as every 4424x sibling) is exercised
    // by that module's own scope tests. What is checked here is the shape of
    // the gate, which is what a work record's admission actually turns on.
    assert!(requires_h_channel_scope(KIND_PROJECT_WORK_RECORD));
    assert!(is_coding_session_kind(KIND_PROJECT_WORK_RECORD));
    assert!(requires_strict_coding_session_membership(
        KIND_PROJECT_WORK_RECORD
    ));
    assert!(!is_global_only_kind(KIND_PROJECT_WORK_RECORD));
}

#[test]
fn the_a_tag_is_a_selector_not_a_project_membership_gate() {
    // Saying otherwise would claim a relay gate that does not exist. 44240
    // and 44248 are gated by project membership *alone, with no channel*;
    // a work record is gated by channel membership through `h`.
    assert!(!is_project_a_scoped_kind(KIND_PROJECT_WORK_RECORD));
}

#[test]
fn an_unknown_record_type_is_refused_by_a_named_reason() {
    let event = record("work.retired", "{}");
    let refusal = validate_project_work_record(&event).expect_err("unknown pwk-type");
    assert!(
        refusal.starts_with("record-type:"),
        "an unknown type is refused as record-type, not as a bare invalid: {refusal}"
    );
}

#[test]
fn an_unknown_schema_version_is_refused_by_a_named_reason() {
    let mut event = declaration();
    let content = event.content.replace(
        "\"schema\":\"buzz-project-work/v1\"",
        "\"schema\":\"buzz-project-work/v2\"",
    );
    event = EventBuilder::new(event.kind, content)
        .tags(event.tags.to_vec())
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    let refusal = validate_project_work_record(&event).expect_err("v2");
    assert!(refusal.starts_with("schema:"), "{refusal}");
}

#[test]
fn a_tag_that_disagrees_with_content_is_refused_not_preferred() {
    let event = declaration();
    let mut tags = event.tags.to_vec();
    tags[1] = Tag::parse(["d", "99999999-2222-4333-8444-555555555555"]).expect("tag");
    let event = EventBuilder::new(event.kind, event.content.clone())
        .tags(tags)
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    let refusal = validate_project_work_record(&event).expect_err("parity");
    assert!(refusal.starts_with("tag-parity:"), "{refusal}");
}

#[test]
fn a_declaration_must_write_its_decision_ref_even_when_there_is_none() {
    let event = record(
        "work.declared",
        &format!(
            "{{\"workId\":\"9d0f0f0f-1111-4222-8333-444444444444\",\"goalRef\":\"{}\",\
             \"responsibleActor\":\"{}\",\"planRef\":{{\"repository\":\"30617:{}:pivot-test-beekeeper-agents\",\
             \"commit\":\"{}\",\"path\":\"plans/kettle.md\"}},\"supersedes\":[]}}",
            hex("90a1"),
            hex("1ead"),
            hex("1ead"),
            "ab".repeat(20)
        ),
    );
    let refusal = validate_project_work_record(&event).expect_err("absent decisionRef");
    assert!(refusal.starts_with("absent-key:"), "{refusal}");
}

#[test]
fn a_decision_id_in_goal_ref_is_not_the_relay_s_question() {
    // Both are 64-hex, so one event cannot tell a goal from a decision. The
    // relay admits it; the fold, which is the only layer holding the
    // session's goal set, refuses it with goal_ref_not_a_goal. Claiming that
    // check here would be claiming knowledge the relay does not have.
    let event = record(
        "work.declared",
        &format!(
            "{{\"workId\":\"9d0f0f0f-1111-4222-8333-444444444444\",\"goalRef\":\"{}\",\
             \"decisionRef\":null,\"responsibleActor\":\"{}\",\"planRef\":{{\"repository\":\"30617:{}:pivot-test-beekeeper-agents\",\
             \"commit\":\"{}\",\"path\":\"plans/kettle.md\"}},\"supersedes\":[]}}",
            hex("dec0de"),
            hex("1ead"),
            hex("1ead"),
            "ab".repeat(20)
        ),
    );
    assert!(validate_project_work_record(&event).is_ok());
}

#[test]
fn the_relay_does_not_adjudicate_authority_here() {
    // The record above is signed by a random key that holds no seat, no grant
    // and no founding genesis. The relay admits it on structure; whether it
    // counts is the fold's question, and the fold excludes it by name. This
    // is the same division 44244 draws, and this lane did not invent a
    // stronger gate for its sibling.
    let event = declaration();
    assert!(validate_project_work_record(&event).is_ok());
}

#[test]
fn an_event_of_another_kind_is_refused_as_the_wrong_kind() {
    let event = EventBuilder::new(Kind::Custom(44244), "{}")
        .tags(Vec::new())
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    let refusal = validate_project_work_record(&event).expect_err("wrong kind");
    assert!(refusal.starts_with("wrong-kind:"), "{refusal}");
}
