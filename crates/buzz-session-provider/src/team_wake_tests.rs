use std::collections::BTreeMap;

use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_context::{
    CodingSessionContextHistoryItem, CodingSessionContextIdentity, CodingSessionContextInboxItem,
    CodingSessionContextPackage, CodingSessionContextProvenance, CodingSessionContextRole,
    CodingSessionContextRosterEntry, CodingSessionContextSeatStatus,
    CODING_SESSION_CONTEXT_PACKAGE_VERSION,
};
use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamTransactionBody,
};
use nostr::Keys;
use uuid::Uuid;

use super::*;

fn target(driver: &str, instance: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: driver.into(),
        instance_id: instance.into(),
        session_id: "lead-session".into(),
        generation,
    }
}

fn package(roster: Vec<CodingSessionContextRosterEntry>) -> CodingSessionContextPackage {
    CodingSessionContextPackage {
        v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
        session: CodingSessionContextIdentity {
            session_ref: Uuid::new_v4().to_string(),
            genesis_ref: "ab".repeat(32),
            channel_id: Uuid::new_v4(),
            name: None,
            goal: None,
            project_ref: None,
        },
        provenance: CodingSessionContextProvenance {
            generated_at: 1,
            complete_as_of: Some(1),
            complete: true,
            truncated: false,
            source_event_count: 0,
            source_event_breakdown: None,
            included_history_items: 0,
            omitted_history_items: 0,
            total_history_items: Some(0),
            notes: Vec::new(),
        },
        history: Vec::new(),
        roster,
        inbox: Vec::new(),
    }
}

fn roster(actor: &str, role: &str, target: CodingSessionTarget) -> CodingSessionContextRosterEntry {
    CodingSessionContextRosterEntry {
        target,
        actor: Some(actor.into()),
        role: Some(role.into()),
        status: CodingSessionContextSeatStatus::Active,
        last_signed_seq: Some(1),
        last_signed_at_ms: Some(1),
    }
}

fn report_source(id: &str) -> WakeSource {
    WakeSource::Report {
        operation_id: id.into(),
        operation_type: "report".into(),
        author_pubkey: "cd".repeat(32),
        created_at: 1,
    }
}

fn scope() -> WakeScope {
    WakeScope {
        channel_ref: Uuid::new_v4(),
        session_ref: Uuid::new_v4().to_string(),
        genesis_ref: "ab".repeat(32),
    }
}

#[test]
fn durable_store_recovers_pending_and_fences_completed_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    let id = "11".repeat(32);
    {
        let mut store = WakeIntentStore::open(dir.path()).expect("open");
        assert!(store.enqueue(scope(), report_source(&id)).expect("enqueue"));
    }
    let mut reopened = WakeIntentStore::open(dir.path()).expect("reopen");
    assert_eq!(reopened.pending().len(), 1);
    reopened.retire(0).expect("retire");
    drop(reopened);

    let mut completed = WakeIntentStore::open(dir.path()).expect("reopen completed");
    assert!(!completed
        .enqueue(scope(), report_source(&id))
        .expect("dedupe"));
    assert!(completed.pending().is_empty());
}

#[test]
fn a_pending_retry_yields_to_the_next_durable_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_id = "11".repeat(32);
    let second_id = "22".repeat(32);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .enqueue(scope(), report_source(&first_id))
        .expect("first");
    store
        .enqueue(scope(), report_source(&second_id))
        .expect("second");
    let mut first = store.pending()[0].clone();
    first.last_reason = Some("verified_snapshot_unavailable".into());
    store.defer_first(first).expect("defer");
    assert_eq!(store.pending()[0].source.event_id(), second_id);
    assert_eq!(
        store.pending()[1].last_reason.as_deref(),
        Some("verified_snapshot_unavailable")
    );
}

#[test]
fn recovery_recognizes_an_already_durable_terminal_for_the_same_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let child = target("claude", "child", 1);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .enqueue(
            scope(),
            WakeSource::Terminal {
                terminal_event_id: "11".repeat(32),
                actor_pubkey: "22".repeat(32),
                role: "builder".into(),
                caused_by_command_id: "assignment-turn".into(),
                source_target: child.clone(),
                prompt_at_ms: Some(1),
                terminal_at_ms: 2,
            },
        )
        .expect("enqueue");
    assert!(store.has_terminal_command("assignment-turn", &child));
    assert!(!store.has_terminal_command("hire-ready", &child));
}

#[test]
fn accepted_lead_resolves_across_provider_instances() {
    let actor = "22".repeat(32);
    let remote = target("codex-acp", "remote-codex", 7);
    let mut authority = CurrentAuthority::default();
    authority
        .seats
        .insert(actor.clone(), ("lead".into(), "33".repeat(32)));
    let context = package(vec![roster(&actor, "lead", remote.clone())]);

    assert_eq!(
        resolve_lead_target(&context, &authority).expect("remote lead"),
        remote
    );
}

#[test]
fn reporting_actor_and_remote_lead_resolve_to_distinct_exact_providers() {
    let child_actor = "11".repeat(32);
    let lead_actor = "22".repeat(32);
    let child = target("claude-agent-acp", "claude-primary", 4);
    let lead = target("codex-acp", "codex-primary", 7);
    let context = package(vec![
        roster(&child_actor, "builder", child.clone()),
        roster(&lead_actor, "lead", lead.clone()),
    ]);
    let mut authority = CurrentAuthority::default();
    authority
        .seats
        .insert(lead_actor, ("lead".into(), "33".repeat(32)));
    assert_eq!(
        resolve_actor_target(&context, &child_actor).expect("source provider"),
        child
    );
    assert_eq!(
        resolve_lead_target(&context, &authority).expect("remote lead provider"),
        lead
    );
}

#[test]
fn lead_resolution_fails_closed_for_zero_multiple_or_metadata_mismatch() {
    let actor = "22".repeat(32);
    let mut authority = CurrentAuthority::default();
    assert!(resolve_lead_target(&package(Vec::new()), &authority).is_err());

    authority
        .seats
        .insert(actor.clone(), ("lead".into(), "33".repeat(32)));
    assert!(resolve_lead_target(
        &package(vec![roster(
            &actor,
            "builder",
            target("codex", "remote", 1)
        )]),
        &authority
    )
    .is_err());

    let other = "44".repeat(32);
    authority
        .seats
        .insert(other.clone(), ("lead".into(), "55".repeat(32)));
    assert!(resolve_lead_target(
        &package(vec![
            roster(&actor, "lead", target("codex", "a", 1)),
            roster(&other, "lead", target("claude", "b", 1)),
        ]),
        &authority
    )
    .is_err());
}

#[test]
fn only_same_actor_report_inside_the_turn_window_suppresses_terminal() {
    let actor = "22".repeat(32);
    let assignment_ref = "aa".repeat(32);
    let reports = vec![IncludedReport {
        event_id: "11".repeat(32),
        assignment_ref: assignment_ref.clone(),
        author_pubkey: actor.clone(),
        created_at: 123,
    }];
    assert!(report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &actor,
        Some(122_000),
        124_000
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &"33".repeat(32),
        Some(122_000),
        124_000
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &actor,
        Some(124_000),
        125_000
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &"bb".repeat(32),
        &actor,
        Some(122_000),
        124_000
    ));
}

#[test]
fn generation_rollover_mints_a_distinct_bounded_command_id() {
    let source = "11".repeat(32);
    let first = command_id(&source, &target("codex", "remote", 1), 0);
    let next = command_id(&source, &target("codex", "remote", 2), 1);
    assert_ne!(first, next);
    assert!(first.len() <= 256);
    assert_eq!(first, command_id(&source, &target("codex", "remote", 1), 0));
}

#[test]
fn provider_wake_uses_pointer_only_report_content() {
    let source = report_source(&"11".repeat(32));
    let event = build_wake_event(
        &Keys::generate(),
        &scope(),
        &source,
        target("codex", "remote", 1),
        "wake-1".into(),
    )
    .expect("event");
    let payload: buzz_core::coding_session_command::CodingSessionCommandPayload =
        serde_json::from_str(&event.content).expect("payload");
    let buzz_core::coding_session_command::CodingSessionAction::ThreadTurnStart { text, .. } =
        payload.action
    else {
        panic!("turn start")
    };
    let pointer: serde_json::Value = serde_json::from_str(&text).expect("pointer");
    assert_eq!(
        pointer,
        serde_json::json!({"operationId": "11".repeat(32), "type": "report"})
    );
    assert_ne!(
        payload.command_id, "assignment-turn",
        "the target wake must not reuse the initiating assignment command id"
    );
}

#[test]
fn verified_provider_stage_or_exact_prompt_echo_settles_the_wake() {
    let exact_target = target("codex", "lead", 1);
    let expected_content = wake_text(&report_source(&"55".repeat(32))).expect("pointer");
    let mut context = package(Vec::new());
    context.inbox.push(CodingSessionContextInboxItem {
        event_id: "11".repeat(32),
        created_at: 10,
        command_id: "wake-1".into(),
        sender: "22".repeat(32),
        sender_role: None,
        target: exact_target.clone(),
        delivery: "boundary".into(),
        content: "pointer".into(),
        stage: Some(ReceiptStatus::TurnQueued),
        stage_at: Some(11),
        stage_code: None,
    });
    assert_eq!(
        command_outcome(&context, "wake-1", &exact_target),
        Some(ReceiptStatus::TurnQueued)
    );
    assert_eq!(
        command_outcome(&context, "wake-1", &target("codex", "other", 1)),
        None
    );
    context.inbox[0].stage = Some(ReceiptStatus::TurnDegraded);
    assert_eq!(
        command_outcome(&context, "wake-1", &exact_target),
        None,
        "degradation is not a settling provider outcome"
    );
    context.inbox[0].stage = Some(ReceiptStatus::InterruptDelivered);
    assert_eq!(
        command_outcome(&context, "wake-1", &exact_target),
        None,
        "interrupt delivery is not a settling provider outcome"
    );

    context.history.push(CodingSessionContextHistoryItem {
        event_id: "33".repeat(32),
        created_at: 12,
        author: "44".repeat(32),
        source_kind: buzz_core::kind::KIND_CODING_SESSION_TRANSCRIPT,
        target: exact_target.clone(),
        event_seq: 1,
        turn_id: Some("turn-1".into()),
        role: CodingSessionContextRole::User,
        item_kind: "user_prompt".into(),
        content: serde_json::json!({
            "kind":"user_prompt",
            "content": "a different operation pointer",
            "steered": false,
            "commandId":"wake-1"
        }),
    });
    assert!(!command_echoed(
        &context,
        "wake-1",
        &exact_target,
        &expected_content
    ));
    context.history[0].content["content"] = serde_json::json!(expected_content);
    assert!(command_echoed(
        &context,
        "wake-1",
        &exact_target,
        &expected_content
    ));
    assert!(!command_echoed(
        &context,
        "wake-2",
        &exact_target,
        &expected_content
    ));
}

#[test]
fn terminal_diagnostic_names_the_exact_initiating_command() {
    let source = WakeSource::Terminal {
        terminal_event_id: "11".repeat(32),
        actor_pubkey: "22".repeat(32),
        role: "builder".into(),
        caused_by_command_id: "assignment-turn".into(),
        source_target: target("claude", "child", 1),
        prompt_at_ms: Some(1),
        terminal_at_ms: 2,
    };
    let value: serde_json::Value =
        serde_json::from_str(&wake_text(&source).expect("diagnostic")).expect("json");
    assert_eq!(value["causedByCommandId"], "assignment-turn");
    assert_eq!(value["type"], "turn_ended_without_required_operation");
}

#[test]
fn signed_attempt_survives_restart_without_resigning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wake_scope = scope();
    let source = report_source(&"11".repeat(32));
    let target = target("codex", "remote", 1);
    let command_id = command_id(source.event_id(), &target, 0);
    let event = build_wake_event(
        &Keys::generate(),
        &wake_scope,
        &source,
        target.clone(),
        command_id.clone(),
    )
    .expect("event");
    {
        let mut store = WakeIntentStore::open(dir.path()).expect("open");
        store.enqueue(wake_scope, source).expect("enqueue");
        let mut intent = store.pending()[0].clone();
        intent.target = Some(target);
        intent.command_id = Some(command_id);
        intent.signed_event = Some(event.clone());
        store.replace(0, intent).expect("persist signed attempt");
    }
    let reopened = WakeIntentStore::open(dir.path()).expect("reopen");
    assert_eq!(reopened.pending()[0].signed_event.as_ref(), Some(&event));
}

#[test]
fn fold_context_never_promotes_a_seat_into_a_steering_grant() {
    let authority = CurrentAuthority {
        seats: BTreeMap::from([("22".repeat(32), ("lead".into(), "33".repeat(32)))]),
        ..CurrentAuthority::default()
    };
    let context = fold_context(&scope(), &"44".repeat(32), &authority);
    assert_eq!(context.active_seats.len(), 1);
    assert!(context.active_grants.is_empty());
}

#[test]
fn ready_or_ordinary_turn_without_canonical_assignment_never_requires_a_report() {
    let founder = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let child_target = target("claude", "child", 1);
    let wake_scope = scope();
    let mut authority = CurrentAuthority::default();
    authority
        .seats
        .insert(actor.clone(), ("builder".into(), "33".repeat(32)));
    let context = fold_context(&wake_scope, &founder.public_key().to_hex(), &authority);
    let mut ready = package(vec![roster(&actor, "builder", child_target.clone())]);
    ready.inbox.push(CodingSessionContextInboxItem {
        event_id: "44".repeat(32),
        created_at: 1,
        command_id: "hire-ready".into(),
        sender: founder.public_key().to_hex(),
        sender_role: None,
        target: child_target.clone(),
        delivery: "boundary".into(),
        content: "Reply READY when initialized.".into(),
        stage: None,
        stage_at: None,
        stage_code: None,
    });
    assert_eq!(
        turn_requires_report(
            &ready,
            &[],
            &context,
            "hire-ready",
            &child_target,
            &actor,
            "builder",
        ),
        TurnReportRequirement::NotRequired
    );
    ready.inbox.clear();
    assert_eq!(
        turn_requires_report(
            &ready,
            &[],
            &context,
            "hire-ready",
            &child_target,
            &actor,
            "builder",
        ),
        TurnReportRequirement::Unknown("initiating_command_not_query_visible"),
        "a temporarily absent initiating command is not evidence that the turn was READY"
    );
}

#[test]
fn only_exact_assignment_pointer_for_actor_and_command_requires_report() {
    let founder = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let child_target = target("claude", "child", 1);
    let wake_scope = scope();
    let mut authority = CurrentAuthority::default();
    authority
        .seats
        .insert(actor.clone(), ("builder".into(), "33".repeat(32)));
    let body = CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
        assignee_actor: actor.clone(),
        assignee_role: "builder".into(),
        objective: "Implement the bounded lane".into(),
        brief: "Implement and report evidence.".into(),
        branch: None,
        base_sha: None,
        file_ownership: vec!["crates/buzz-session-provider".into()],
        acceptance_steps: vec!["cargo test -p buzz-session-provider".into()],
    });
    let payload =
        buzz_sdk::coding_session_team_transaction::coding_session_team_transaction_payload(
            wake_scope.session_ref.clone(),
            wake_scope.genesis_ref.clone(),
            None,
            Some("assignment-turn".into()),
            body,
        );
    let assignment =
        buzz_sdk::coding_session_team_transaction::build_coding_session_team_transaction(
            &wake_scope.channel_ref.to_string(),
            payload,
        )
        .expect("builder")
        .sign_with_keys(&founder)
        .expect("sign");
    let mut context_package = package(vec![roster(&actor, "builder", child_target.clone())]);
    context_package.inbox.push(CodingSessionContextInboxItem {
        event_id: "44".repeat(32),
        created_at: 1,
        command_id: "assignment-turn".into(),
        sender: founder.public_key().to_hex(),
        sender_role: None,
        target: child_target.clone(),
        delivery: "boundary".into(),
        content: serde_json::json!({
            "operationId": assignment.id.to_hex(),
            "type": "assignment"
        })
        .to_string(),
        stage: None,
        stage_at: None,
        stage_code: None,
    });
    let context = fold_context(&wake_scope, &founder.public_key().to_hex(), &authority);
    assert_eq!(
        turn_requires_report(
            &context_package,
            std::slice::from_ref(&assignment),
            &context,
            "assignment-turn",
            &child_target,
            &actor,
            "builder",
        ),
        TurnReportRequirement::Required {
            assignment_ref: assignment.id.to_hex()
        }
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let terminal = WakeSource::Terminal {
        terminal_event_id: "55".repeat(32),
        actor_pubkey: actor.clone(),
        role: "builder".into(),
        caused_by_command_id: "assignment-turn".into(),
        source_target: child_target.clone(),
        prompt_at_ms: Some(1),
        terminal_at_ms: 2,
    };
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .enqueue(wake_scope.clone(), terminal)
        .expect("enqueue terminal");
    assert_eq!(
        turn_requires_report(
            &context_package,
            &[],
            &context,
            "assignment-turn",
            &child_target,
            &actor,
            "builder",
        ),
        TurnReportRequirement::Unknown("assignment_not_canonical")
    );
    let mut intent = store.pending()[0].clone();
    intent.last_reason = Some("assignment_not_canonical".into());
    store.replace(0, intent).expect("persist unknown");
    drop(store);

    let reopened = WakeIntentStore::open(dir.path()).expect("restart");
    assert_eq!(
        reopened.pending()[0].last_reason.as_deref(),
        Some("assignment_not_canonical")
    );
    assert_eq!(
        turn_requires_report(
            &context_package,
            std::slice::from_ref(&assignment),
            &context,
            "assignment-turn",
            &child_target,
            &actor,
            "builder",
        ),
        TurnReportRequirement::Required {
            assignment_ref: assignment.id.to_hex()
        },
        "the delayed signed assignment must make the durable terminal actionable"
    );
}
