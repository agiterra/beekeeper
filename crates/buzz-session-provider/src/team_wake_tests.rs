use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;

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
use tempfile::tempdir;
use uuid::Uuid;

use super::*;

fn target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "provider-a".into(),
        session_id: "lead-session".into(),
        generation,
    }
}

fn provider_target(driver: &str, instance_id: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: driver.into(),
        instance_id: instance_id.into(),
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
        policy: None,
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

fn scope(channel_ref: Uuid) -> WakeScope {
    WakeScope {
        channel_ref,
        session_ref: channel_ref.to_string(),
        genesis_ref: "ab".repeat(32),
    }
}

fn report(value: usize, created_at: u64) -> WakeSource {
    WakeSource::Report {
        operation_id: format!("{value:064x}"),
        operation_type: "assignment_report".into(),
        author_pubkey: format!("{:064x}", value.saturating_add(100_000)),
        created_at,
    }
}

fn terminal(channel: Uuid, value: usize) -> WakeSource {
    WakeSource::Terminal {
        terminal_event_id: format!("terminal-{channel}-{value}"),
        actor_pubkey: format!("{:064x}", value.saturating_add(200_000)),
        role: "builder".into(),
        caused_by_command_id: format!("assignment-{channel}-{value}"),
        source_target: CodingSessionTarget {
            driver: "claude-acp".into(),
            instance_id: "provider-b".into(),
            session_id: format!("builder-{channel}"),
            generation: 1,
        },
        prompt_at_ms: Some(1_000),
        terminal_at_ms: 2_000,
    }
}

fn next_pending(store: &mut WakeIntentStore, channels: &[Uuid]) -> Option<WakeIntent> {
    let channel = store
        .next_tick_channel(channels.iter().copied())
        .expect("select fair tick channel")?;
    store
        .pending_for_channel(channel)
        .expect("select channel work")
}

#[test]
fn unknown_store_schema_is_quarantined_without_bricking_startup() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("team-wake-intents.json"),
        br#"{"schema":"buzz-provider-team-wake-intents/v2"}"#,
    )
    .expect("write rejected schema");

    let mut store = WakeIntentStore::open(dir.path()).expect("quarantine and open v3");
    assert_eq!(
        store
            .capture_report(scope(Uuid::nil()), report(1, 1))
            .expect("fresh store works"),
        DiscoveryCapture::Admitted
    );
    let quarantined = fs::read_dir(dir.path())
        .expect("read state dir")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| name.starts_with("team-wake-intents.json.quarantined-"));
    assert!(
        quarantined,
        "the rejected store remains available for forensics"
    );
}

#[test]
fn corrupt_store_bytes_are_quarantined_without_bricking_startup() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("team-wake-intents.json"), b"\0not-json")
        .expect("write corrupt bytes");

    let mut store = WakeIntentStore::open(dir.path()).expect("quarantine and open v3");
    assert_eq!(
        store
            .capture_report(scope(Uuid::nil()), report(2, 2))
            .expect("fresh store works"),
        DiscoveryCapture::Admitted
    );
    assert!(fs::read_dir(dir.path())
        .expect("read state dir")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| name.starts_with("team-wake-intents.json.quarantined-")));
}

#[test]
fn oversized_or_control_filled_reason_detail_is_quarantined_on_reopen() {
    for detail in ["x".repeat(1_025), "control\nreason".into()] {
        let dir = tempdir().expect("tempdir");
        let channel = Uuid::new_v4();
        let mut store = WakeIntentStore::open(dir.path()).expect("open");
        store
            .capture_report(scope(channel), report(3, 3))
            .expect("capture");
        let mut intent = store
            .pending_for_channel(channel)
            .expect("promote")
            .expect("intent");
        intent.last_reason_detail = Some(detail);
        store
            .replace_in_flight(channel, intent)
            .expect("persist malformed fixture");
        drop(store);

        let reopened = WakeIntentStore::open(dir.path()).expect("quarantine and reopen");
        assert!(!reopened.has_work(channel));
        assert!(fs::read_dir(dir.path())
            .expect("read state dir")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .any(|name| name.starts_with("team-wake-intents.json.quarantined-")));
    }
}

#[test]
fn admitted_report_is_promoted_before_an_older_terminal_diagnostic() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let report = report(4, 4);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_terminal(scope(channel), terminal(channel, 4))
        .expect("terminal");
    store
        .capture_report(scope(channel), report.clone())
        .expect("report");
    let promoted = store
        .pending_for_channel(channel)
        .expect("promote")
        .expect("intent");
    assert_eq!(promoted.source.event_id(), report.event_id());
}

#[test]
fn already_in_flight_legacy_terminal_yields_to_a_later_report() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let report = report(5, 5);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_terminal(scope(channel), terminal(channel, 5))
        .expect("terminal");
    let mut stuck = store
        .pending_for_channel(channel)
        .expect("promote terminal")
        .expect("terminal intent");
    stuck.last_reason = Some("initiating_command_not_query_visible".into());
    store
        .defer_in_flight(channel, stuck.clone())
        .expect("persist old stuck state");
    store
        .capture_report(scope(channel), report.clone())
        .expect("later report");
    assert!(store
        .park_unattempted_terminal_behind_work(channel, stuck)
        .expect("park legacy terminal"));
    let promoted = store
        .pending_for_channel(channel)
        .expect("promote report")
        .expect("report intent");
    assert_eq!(promoted.source.event_id(), report.event_id());
}

#[test]
fn already_in_flight_legacy_terminal_yields_to_a_later_terminal() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let later = terminal(channel, 7);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_terminal(scope(channel), terminal(channel, 6))
        .expect("legacy terminal");
    let mut stuck = store
        .pending_for_channel(channel)
        .expect("promote legacy")
        .expect("legacy intent");
    stuck.last_reason = Some("initiating_command_not_query_visible".into());
    store
        .defer_in_flight(channel, stuck.clone())
        .expect("persist old stuck state");
    store
        .capture_terminal(scope(channel), later.clone())
        .expect("later terminal");
    assert!(store
        .park_unattempted_terminal_behind_work(channel, stuck)
        .expect("rotate legacy terminal"));
    let promoted = store
        .pending_for_channel(channel)
        .expect("promote later terminal")
        .expect("later intent");
    assert_eq!(promoted.source.event_id(), later.event_id());
}

/// v3.1 §1: a failed atomic write poisons this in-memory instance, while a
/// reopen recovers exactly the last good disk state.
#[test]
fn failed_write_poisons_until_reopen_and_recovers_last_good_state() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let saved = dir.path().join("saved-state");
    fs::create_dir(&state).expect("state dir");
    let channel = Uuid::new_v4();
    let first = report(10, 10);
    let second = report(11, 11);
    let mut store = WakeIntentStore::open(&state).expect("open");
    store
        .capture_report(scope(channel), first.clone())
        .expect("last good write");
    fs::rename(&state, &saved).expect("move durable state aside");
    fs::write(&state, b"not-a-directory").expect("block atomic write parent");
    assert!(store
        .capture_report(scope(channel), second.clone())
        .is_err());
    assert!(store
        .capture_report(scope(channel), report(12, 12))
        .is_err());
    fs::remove_file(&state).expect("remove blocker");
    fs::rename(&saved, &state).expect("restore last good state");
    let mut reopened = WakeIntentStore::open(&state).expect("reopen");
    assert_eq!(
        reopened
            .capture_report(scope(channel), first)
            .expect("known durable source"),
        DiscoveryCapture::Duplicate
    );
    assert_eq!(
        reopened
            .capture_report(scope(channel), second)
            .expect("failed source was not durable"),
        DiscoveryCapture::Admitted
    );
}

/// v3.1 §3.2–§3.4: structural refusal is durable and named, live report loss
/// is counted with its last id, and provider-local terminals park durably.
#[test]
fn refusal_counts_live_reports_and_parks_terminals_until_cleared() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let refused_report = report(20, 20);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    assert!(store
        .refuse_channel(channel, ChannelRefusalCode::PartitionSaturated)
        .expect("first refusal"));
    assert!(!store
        .refuse_channel(channel, ChannelRefusalCode::PartitionSaturated)
        .expect("reaffirm refusal"));
    assert_eq!(
        store
            .capture_live_report(scope(channel), refused_report.clone())
            .expect("count refused live report"),
        DiscoveryCapture::Refused(ChannelRefusalCode::PartitionSaturated)
    );
    assert!(store
        .capture_terminal(scope(channel), terminal(channel, 20))
        .expect("terminal evidence always admitted"));
    let refusal = store.refusal(channel).expect("durable refusal");
    assert_eq!(refusal.code, ChannelRefusalCode::PartitionSaturated);
    assert_eq!(refusal.refused_live_reports, 1);
    assert_eq!(
        refusal.last_refused_event_id.as_deref(),
        Some(refused_report.event_id())
    );
    assert!(store
        .refuse_channel(channel, ChannelRefusalCode::ResolvedLedgerFull)
        .expect("changed structural refusal"));
    let changed = store.refusal(channel).expect("changed refusal retained");
    assert_eq!(changed.code, ChannelRefusalCode::ResolvedLedgerFull);
    assert_eq!(changed.refused_live_reports, 1);
    assert_eq!(
        changed.last_refused_event_id.as_deref(),
        Some(refused_report.event_id())
    );
    assert!(store
        .pending_for_channel(channel)
        .expect("refused channel is parked")
        .is_none());
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("restart");
    assert_eq!(
        reopened
            .refusal(channel)
            .expect("refusal survives restart")
            .refused_live_reports,
        1
    );
    assert!(reopened
        .clear_refusal(channel)
        .expect("operator/reprobe clear"));
    let parked = reopened
        .pending_for_channel(channel)
        .expect("promote parked terminal")
        .expect("terminal retained");
    assert_eq!(parked.last_reason.as_deref(), Some("channel_refused"));
}

/// v3.1 §3.4: the inherited 32-page truth envelope covers the union of
/// resolved, admitted, and report in-flight ids, including on restart.
#[test]
fn oversized_combined_report_envelope_is_quarantined() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let resolved: Vec<String> = (0..32_000).map(|value| format!("{value:064x}")).collect();
    let snapshot = serde_json::json!({
        "schema": "buzz-provider-team-wake-intents/v3",
        "channels": [{
            "channelRef": channel,
            "resolved": resolved,
            "admitted": [{
                "scope": scope(channel),
                "source": report(32_001, 1)
            }],
            "inFlight": null,
            "terminals": []
        }]
    });
    fs::write(
        dir.path().join("team-wake-intents.json"),
        serde_json::to_vec(&snapshot).expect("serialize oversized snapshot"),
    )
    .expect("write oversized snapshot");

    let store = WakeIntentStore::open(dir.path()).expect("quarantine oversized snapshot");
    assert_eq!(store.channel_counts(channel), (0, 0, 0, 0));
    assert!(fs::read_dir(dir.path())
        .expect("read quarantine directory")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| name.starts_with("team-wake-intents.json.quarantined-")));
}

/// v3.1 §1: inspecting an existing in-flight item is read-only; only the
/// promotion transition writes.
#[test]
fn pending_for_existing_in_flight_performs_no_durable_write() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let saved = dir.path().join("saved-state");
    fs::create_dir(&state).expect("state dir");
    let channel = Uuid::new_v4();
    let mut store = WakeIntentStore::open(&state).expect("open");
    store
        .capture_report(scope(channel), report(30, 30))
        .expect("capture");
    store
        .pending_for_channel(channel)
        .expect("promotion writes")
        .expect("in flight");
    fs::rename(&state, &saved).expect("move durable state aside");
    fs::write(&state, b"not-a-directory").expect("block future writes");
    assert!(store
        .pending_for_channel(channel)
        .expect("existing in-flight read does not write")
        .is_some());
}

#[test]
fn live_capture_refuses_the_inherited_complete_partition_ledger_bound() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let resolved: Vec<String> = (0..32_000).map(|value| format!("{value:064x}")).collect();
    let snapshot = serde_json::json!({
        "schema": "buzz-provider-team-wake-intents/v3",
        "rrChannel": null,
        "channels": [{
            "channelRef": channel,
            "resolved": resolved,
            "admitted": [],
            "inFlight": null,
            "terminals": []
        }]
    });
    fs::write(
        dir.path().join("team-wake-intents.json"),
        serde_json::to_vec(&snapshot).expect("serialize snapshot"),
    )
    .expect("write bounded snapshot");
    let mut store = WakeIntentStore::open(dir.path()).expect("open bounded snapshot");
    assert_eq!(
        store
            .capture_live_report(scope(channel), report(32_001, 1))
            .expect("explicit ledger refusal"),
        DiscoveryCapture::ResolvedLedgerFull
    );
    let refusal = store.refusal(channel).expect("durable envelope refusal");
    assert_eq!(refusal.code, ChannelRefusalCode::ResolvedLedgerFull);
    assert_eq!(refusal.refused_live_reports, 1);
    assert_eq!(
        refusal.last_refused_event_id.as_deref(),
        Some(report(32_001, 1).event_id())
    );
}

#[test]
fn terminal_sanity_cap_is_an_explicit_refusal() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    for value in 0..256 {
        assert!(store
            .capture_terminal(scope(channel), terminal(channel, value))
            .expect("provider-controlled terminal is admitted"));
    }
    let error = store
        .capture_terminal(scope(channel), terminal(channel, 257))
        .expect_err("sanity cap refuses explicitly");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn channel_count_bound_is_an_explicit_refusal() {
    let dir = tempdir().expect("tempdir");
    let channels: Vec<Uuid> = (1..=1_024).map(Uuid::from_u128).collect();
    let snapshot = serde_json::json!({
        "schema": "buzz-provider-team-wake-intents/v3",
        "rrChannel": null,
        "channels": channels.iter().map(|channel| serde_json::json!({
            "channelRef": channel,
            "resolved": [],
            "admitted": [],
            "inFlight": null,
            "terminals": []
        })).collect::<Vec<_>>()
    });
    fs::write(
        dir.path().join("team-wake-intents.json"),
        serde_json::to_vec(&snapshot).expect("serialize channel bound snapshot"),
    )
    .expect("write channel bound snapshot");
    let mut store = WakeIntentStore::open(dir.path()).expect("open full channel snapshot");
    let error = store
        .capture_report(scope(Uuid::from_u128(1_025)), report(9_999, 1))
        .expect_err("channel bound refuses explicitly");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn report_resolution_is_permanent_and_timestamp_independent() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let past = report(1, 1);
    let future = report(2, u64::MAX);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    assert_eq!(
        store
            .capture_report(scope(channel), future.clone())
            .expect("future admission"),
        DiscoveryCapture::Admitted
    );
    assert_eq!(
        store
            .capture_report(scope(channel), past.clone())
            .expect("past admission"),
        DiscoveryCapture::Admitted
    );
    for expected in [future.event_id(), past.event_id()] {
        let intent = next_pending(&mut store, &[channel]).expect("pending");
        assert_eq!(intent.source.event_id(), expected);
        store.retire_in_flight(channel).expect("resolve");
    }
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("reopen");
    for source in [future, past] {
        assert_eq!(
            reopened
                .capture_report(scope(channel), source)
                .expect("replay"),
            DiscoveryCapture::Duplicate
        );
    }
    assert!(next_pending(&mut reopened, &[channel]).is_none());
}

#[test]
fn terminal_capture_is_total_under_report_saturation_and_dedupes_by_turn() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    for value in 0..64 {
        assert_eq!(
            store
                .capture_report(scope(channel), report(value, value as u64))
                .expect("admit report"),
            DiscoveryCapture::Admitted
        );
    }
    assert_eq!(
        store
            .capture_report(scope(channel), report(65, 65))
            .expect("saturated report"),
        DiscoveryCapture::Saturated
    );
    let terminal = terminal(channel, 1);
    assert!(store
        .capture_terminal(scope(channel), terminal.clone())
        .expect("terminal always admits"));
    assert!(!store
        .capture_terminal(scope(channel), terminal)
        .expect("terminal dedupes"));
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("reopen");
    for _ in 0..64 {
        let next = next_pending(&mut reopened, &[channel]).expect("report");
        assert!(matches!(next.source, WakeSource::Report { .. }));
        reopened.retire_in_flight(channel).expect("resolve report");
    }
    let next = next_pending(&mut reopened, &[channel]).expect("terminal");
    assert!(matches!(next.source, WakeSource::Terminal { .. }));
}

#[test]
fn signed_attempt_is_persisted_byte_exactly_before_publish() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let source = report(42, 42);
    let wake_scope = scope(channel);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_report(wake_scope.clone(), source.clone())
        .expect("capture");
    let mut intent = next_pending(&mut store, &[channel]).expect("intent");
    let lead = target(1);
    let command = command_id(source.event_id(), &lead, 0);
    let event = build_wake_event(
        &Keys::generate(),
        &wake_scope,
        &source,
        lead.clone(),
        command,
    )
    .expect("sign wake");
    intent.target = Some(lead);
    intent.signed_event = Some(event.clone());
    store
        .replace_in_flight(channel, intent)
        .expect("persist exact signed attempt");
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("restart");
    let recovered = next_pending(&mut reopened, &[channel]).expect("intent");
    assert_eq!(recovered.signed_event.as_ref(), Some(&event));
}

/// I7's initial `None -> Some(target)` transition is a durable pass of its
/// own: no signed bytes exist until the next pass has had a chance to inspect
/// an outcome for the newly derived deterministic command id.
#[test]
fn initial_target_bind_returns_before_the_driver_can_sign_or_send() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let source = report(77, 77);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_report(scope(channel), source.clone())
        .expect("capture");
    let mut intent = next_pending(&mut store, &[channel]).expect("select");
    let lead = target(3);
    intent.target = Some(lead.clone());
    intent.command_id = Some(command_id(source.event_id(), &lead, intent.attempt));
    intent.last_reason = Some("lead_target_bound_retry_pending".into());
    store
        .defer_in_flight(channel, intent)
        .expect("persist target bind");
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("restart");
    let bound = next_pending(&mut reopened, &[channel]).expect("recovered binding");
    assert_eq!(bound.target, Some(lead));
    assert_eq!(
        bound.signed_event, None,
        "binding pass did not sign or send"
    );
    assert_eq!(
        bound.command_id,
        Some(command_id(source.event_id(), &target(3), 0))
    );
}

/// I8's rollover guard consults the old exact target before deriving this new
/// command identity. A receipt for the old generation settles the source;
/// minting the distinct rollover id would otherwise wake the replacement.
#[test]
fn old_target_receipt_settles_before_rollover_redelivery() {
    let old_target = target(1);
    let replacement = target(2);
    let source = report(78, 78);
    let old_command = command_id(source.event_id(), &old_target, 0);
    let expected = wake_text(&source).expect("wake pointer");
    let mut context = package(Vec::new());
    context.inbox.push(CodingSessionContextInboxItem {
        event_id: "55".repeat(32),
        created_at: 1,
        command_id: old_command.clone(),
        sender: "66".repeat(32),
        sender_role: None,
        target: old_target.clone(),
        delivery: "boundary".into(),
        content: expected,
        stage: Some(ReceiptStatus::TurnQueued),
        stage_at: Some(1),
        stage_code: None,
    });
    assert!(command_outcome(&context, &old_command, &old_target).is_some());
    assert_ne!(
        old_command,
        command_id(source.event_id(), &replacement, 1),
        "a rollover would mint a different command id only if old settlement were absent"
    );
}

#[test]
fn one_round_robin_serves_each_channel_despite_a_blocked_neighbor() {
    let dir = tempdir().expect("tempdir");
    let channels = [
        Uuid::from_u128(10),
        Uuid::from_u128(20),
        Uuid::from_u128(30),
    ];
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    for (index, channel) in channels.into_iter().enumerate() {
        store
            .capture_report(scope(channel), report(index, index as u64))
            .expect("capture");
    }
    let mut seen = VecDeque::new();
    for _ in 0..3 {
        let mut intent = next_pending(&mut store, &channels).expect("intent");
        seen.push_back(intent.scope.channel_ref);
        if intent.scope.channel_ref == channels[0] {
            intent.last_reason = Some("blocked".into());
            store.defer_in_flight(channels[0], intent).expect("defer");
        } else {
            store
                .retire_in_flight(intent.scope.channel_ref)
                .expect("resolve");
        }
    }
    assert_eq!(seen.into_iter().collect::<HashSet<_>>().len(), 3);
}

/// v3.1 §2 single-mutator rule: the memory-only scheduler cursor has one
/// assignment site and never enters the durable JSON snapshot.
#[test]
fn scheduler_cursor_has_one_mutator_and_is_not_durable() {
    let source = include_str!("team_wake_store.rs");
    assert_eq!(
        source
            .matches("self.rr_channel = Some(channel_ref)")
            .count(),
        1
    );
    assert!(!source.contains("rr_channel: Option<Uuid>,\n    channels: &'a"));

    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    assert_eq!(
        store
            .next_tick_channel([channel])
            .expect("memory-only selection"),
        Some(channel)
    );
    store
        .capture_report(scope(channel), report(40, 40))
        .expect("force durable snapshot");
    let snapshot =
        fs::read_to_string(dir.path().join("team-wake-intents.json")).expect("read snapshot");
    assert!(!snapshot.contains("rrChannel"), "{snapshot}");
}

/// v3.1 R1/R3: horizon identity rebinding is durable and unsigned; the next
/// pass, not the mutating pass, may sign or publish it.
#[test]
fn horizon_rebump_returns_with_new_id_and_no_signed_event() {
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let source = report(41, 41);
    let lead = target(1);
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_report(scope(channel), source.clone())
        .expect("capture");
    let mut intent = next_pending(&mut store, &[channel]).expect("intent");
    intent.target = Some(lead.clone());
    intent.command_id = Some(command_id(source.event_id(), &lead, 0));
    intent.relay_accepted_at = Some(1);
    intent.attempt = 1;
    intent.command_id = Some(command_id(source.event_id(), &lead, 1));
    intent.signed_event = None;
    intent.relay_accepted_at = None;
    store
        .defer_in_flight(channel, intent)
        .expect("persist horizon rebump");
    drop(store);
    let mut reopened = WakeIntentStore::open(dir.path()).expect("restart");
    let rebound = next_pending(&mut reopened, &[channel]).expect("rebound");
    assert_eq!(rebound.attempt, 1);
    assert_eq!(
        rebound.command_id,
        Some(command_id(source.event_id(), &lead, 1))
    );
    assert!(rebound.signed_event.is_none());
}

#[test]
fn accepted_lead_resolves_across_provider_instances() {
    let actor = "22".repeat(32);
    let remote = provider_target("codex-acp", "remote-codex", 7);
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
    let child = provider_target("claude-agent-acp", "claude-primary", 4);
    let lead = provider_target("codex-acp", "codex-primary", 7);
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
            provider_target("codex", "remote", 1),
        )]),
        &authority,
    )
    .is_err());

    let other = "44".repeat(32);
    authority
        .seats
        .insert(other.clone(), ("lead".into(), "55".repeat(32)));
    assert!(resolve_lead_target(
        &package(vec![
            roster(&actor, "lead", provider_target("codex", "a", 1)),
            roster(&other, "lead", provider_target("claude", "b", 1)),
        ]),
        &authority,
    )
    .is_err());
}

#[test]
fn only_same_actor_report_wholly_inside_turn_window_suppresses_terminal() {
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
        124_000,
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &"bb".repeat(32),
        &actor,
        Some(122_000),
        124_000,
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &"33".repeat(32),
        Some(122_000),
        124_000,
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &actor,
        Some(123_001),
        124_000,
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &actor,
        Some(122_000),
        123_998,
    ));
    assert!(!report_suppresses_terminal(
        &reports,
        &assignment_ref,
        &actor,
        None,
        124_000,
    ));
}

#[test]
fn generation_rollover_mints_a_distinct_bounded_command_id() {
    let source = "11".repeat(32);
    let first = command_id(&source, &provider_target("codex", "remote", 1), 0);
    let next = command_id(&source, &provider_target("codex", "remote", 2), 1);
    assert_ne!(first, next);
    assert!(first.len() <= 256);
    assert_eq!(
        first,
        command_id(&source, &provider_target("codex", "remote", 1), 0)
    );
}

#[test]
fn provider_wake_uses_pointer_only_report_content() {
    let source = report(11, 1);
    let event = build_wake_event(
        &Keys::generate(),
        &scope(Uuid::new_v4()),
        &source,
        provider_target("codex", "remote", 1),
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
        serde_json::json!({"operationId": source.event_id(), "type": "assignment_report"})
    );
    assert_ne!(payload.command_id, "assignment-turn");
}

#[test]
fn verified_provider_stage_or_exact_prompt_echo_settles_the_wake() {
    let exact_target = provider_target("codex", "lead", 1);
    let expected_content = wake_text(&report(55, 1)).expect("pointer");
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
        command_outcome(&context, "wake-1", &provider_target("codex", "other", 1)),
        None
    );
    context.inbox[0].stage = Some(ReceiptStatus::TurnDegraded);
    assert_eq!(command_outcome(&context, "wake-1", &exact_target), None);

    context.inbox[0].command_id = "desktop-fallback-id".into();
    context.inbox[0].content = expected_content.clone();
    context.inbox[0].stage = Some(ReceiptStatus::TurnQueued);
    assert!(operation_wake_delivered(
        &context,
        &exact_target,
        &expected_content,
    ));
    context.inbox[0].content = "different pointer".into();
    assert!(!operation_wake_delivered(
        &context,
        &exact_target,
        &expected_content,
    ));
    context.inbox[0].content = expected_content.clone();
    for rejected in [ReceiptStatus::TurnDropped, ReceiptStatus::TurnRefused] {
        context.inbox[0].stage = Some(rejected);
        assert!(
            !operation_wake_delivered(&context, &exact_target, &expected_content),
            "a foreign producer's terminal refusal is not delivery"
        );
    }
    context.inbox[0].command_id = "wake-1".into();

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
        &expected_content,
    ));
    context.history[0].content["content"] = serde_json::json!(expected_content);
    assert!(command_echoed(
        &context,
        "wake-1",
        &exact_target,
        &expected_content,
    ));
    context.history[0].content["commandId"] = serde_json::json!("another-producer-id");
    assert!(operation_wake_delivered(
        &context,
        &exact_target,
        &expected_content,
    ));
}

#[test]
fn terminal_diagnostic_names_the_exact_initiating_command() {
    let source = WakeSource::Terminal {
        terminal_event_id: "11".repeat(32),
        actor_pubkey: "22".repeat(32),
        role: "builder".into(),
        caused_by_command_id: "assignment-turn".into(),
        source_target: provider_target("claude", "child", 1),
        prompt_at_ms: Some(1),
        terminal_at_ms: 2,
    };
    let value: serde_json::Value =
        serde_json::from_str(&wake_text(&source).expect("diagnostic")).expect("json");
    assert_eq!(value["causedByCommandId"], "assignment-turn");
    assert_eq!(value["type"], "turn_ended_without_required_operation");
}

#[test]
fn fold_context_never_promotes_a_seat_into_a_steering_grant() {
    let authority = CurrentAuthority {
        seats: BTreeMap::from([("22".repeat(32), ("lead".into(), "33".repeat(32)))]),
        ..CurrentAuthority::default()
    };
    let wake_scope = scope(Uuid::new_v4());
    let context = fold_context(&wake_scope, &"44".repeat(32), &authority);
    assert_eq!(context.active_seats.len(), 1);
    assert!(context.active_grants.is_empty());
}

#[test]
fn ready_or_ordinary_turn_without_canonical_assignment_never_requires_a_report() {
    let founder = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let child_target = provider_target("claude", "child", 1);
    let wake_scope = scope(Uuid::new_v4());
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
        TurnReportRequirement::Unknown("initiating_command_not_query_visible")
    );
}

#[test]
fn only_exact_assignment_pointer_for_actor_and_command_requires_report() {
    let founder = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let child_target = provider_target("claude", "child", 1);
    let wake_scope = scope(Uuid::new_v4());
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
    let wake_channel = wake_scope.channel_ref;
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_terminal(wake_scope, terminal)
        .expect("capture terminal");
    let mut intent = next_pending(&mut store, &[wake_channel]).expect("terminal");
    intent.last_reason = Some("assignment_not_canonical".into());
    store
        .defer_in_flight(wake_channel, intent)
        .expect("persist unknown");
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("restart");
    let recovered = next_pending(&mut reopened, &[wake_channel]).expect("terminal");
    assert_eq!(
        recovered.last_reason.as_deref(),
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
        }
    );
}

/// One inbox item for `command_id` on `target`, carrying `stage`/`code`.
fn inbox_item(
    command_id: &str,
    target: &CodingSessionTarget,
    content: &str,
    stage: ReceiptStatus,
    stage_code: Option<&str>,
) -> CodingSessionContextInboxItem {
    CodingSessionContextInboxItem {
        event_id: "1f".repeat(32),
        created_at: 10,
        command_id: command_id.to_owned(),
        sender: "2f".repeat(32),
        sender_role: None,
        target: target.clone(),
        delivery: "boundary".into(),
        content: content.to_owned(),
        stage: Some(stage),
        stage_at: Some(11),
        stage_code: stage_code.map(str::to_owned),
    }
}

/// P-T7 / I13 — the runner's `DUPLICATE_OPERATION` refusal *settles* the
/// sender's intent; it is never a delivery failure.
///
/// The refusal says the operation is already custodied by another command, so
/// something is delivering the wake — it is simply not this one. Retrying,
/// or re-arming a fallback against it, would spend the second lead turn the
/// fence exists to prevent. No behaviour change: `command_outcome` already
/// treats any `turn_refused` as terminal, and this test is what keeps it that
/// way.
#[test]
fn a_duplicate_operation_refusal_settles_the_wake_intent() {
    let exact_target = provider_target("codex", "lead", 1);
    let source = report(55, 1);
    let expected_content = wake_text(&source).expect("pointer");
    let mut context = package(Vec::new());
    context.inbox.push(inbox_item(
        "wake-1",
        &exact_target,
        &expected_content,
        ReceiptStatus::TurnRefused,
        Some(buzz_core::coding_session_payload::DUPLICATE_OPERATION),
    ));
    assert_eq!(
        command_outcome(&context, "wake-1", &exact_target),
        Some(ReceiptStatus::TurnRefused),
        "a refusal is a settling outcome whatever code it carries"
    );

    // ...and the settlement the provider then performs is permanent.
    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    assert_eq!(
        store
            .capture_report(scope(channel), source.clone())
            .expect("admit"),
        DiscoveryCapture::Admitted
    );
    let intent = next_pending(&mut store, &[channel]).expect("pending");
    assert_eq!(intent.source.event_id(), source.event_id());
    store.retire_in_flight(channel).expect("retire on refusal");
    assert_eq!(store.channel_counts(channel), (1, 0, 0, 0));
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("reopen");
    assert_eq!(
        reopened
            .capture_report(scope(channel), source)
            .expect("rediscovery"),
        DiscoveryCapture::Duplicate,
        "the report id is in the permanent resolved ledger"
    );
    assert!(next_pending(&mut reopened, &[channel]).is_none());
}

/// P-T8 / acceptance #5 — the Desktop fallback started during a provider
/// outage; the provider restarts and publishes nothing.
///
/// The provider's own command id has no receipt at all — it never got as far
/// as signing one. What settles the intent is *operation-level* evidence: a
/// foreign command id whose content is the exact pointer, addressed to the
/// exact lead target, that reached `turn_started`. Binding to the pointer and
/// the target rather than to the producer's command id is what lets either
/// producer prove delivery and stops the other spending a second lead turn.
#[test]
fn a_desktop_fallback_start_settles_the_provider_intent_across_a_restart() {
    let exact_target = provider_target("codex", "lead", 1);
    let source = report(77, 1);
    let expected_content = wake_text(&source).expect("pointer");
    let mut context = package(Vec::new());
    context.inbox.push(inbox_item(
        "team-wake-v1:795ce319:42dcf1b7",
        &exact_target,
        &expected_content,
        ReceiptStatus::TurnStarted,
        None,
    ));

    // The provider's own command is unanswered; the operation is not.
    assert_eq!(
        command_outcome(&context, "team-wake-provider-mint", &exact_target),
        None
    );
    assert!(operation_wake_delivered(
        &context,
        &exact_target,
        &expected_content
    ));
    // A different generation is a different execution, and never delivery.
    assert!(!operation_wake_delivered(
        &context,
        &provider_target("codex", "lead", 2),
        &expected_content
    ));

    let dir = tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let mut store = WakeIntentStore::open(dir.path()).expect("open");
    store
        .capture_report(scope(channel), source.clone())
        .expect("admit");
    let intent = next_pending(&mut store, &[channel]).expect("pending");
    assert!(
        intent.signed_event.is_none(),
        "the intent settles before anything is signed, so nothing is published"
    );
    store
        .retire_in_flight(channel)
        .expect("retire on operation evidence");
    drop(store);

    let mut reopened = WakeIntentStore::open(dir.path()).expect("restart");
    assert_eq!(
        reopened
            .capture_report(scope(channel), source)
            .expect("rediscovery after restart"),
        DiscoveryCapture::Duplicate,
        "a restart may not resurrect an operation another producer delivered"
    );
    assert!(next_pending(&mut reopened, &[channel]).is_none());
}

// ── Lane L9: there is no cross-umbrella wake ─────────────────────────────────

#[test]
fn a_foreign_umbrellas_pubkey_produces_no_wake_at_all() {
    // LANE-L9 §L9.5's red, at the producer rather than at the row: "a message
    // or note from a foreign umbrella's pubkey produces no 44220 wake (assert
    // on the published set)". REVIEW-L9 F3 found the shipped proof was a source
    // scan of `pulse_overlap.rs`, which shows the new module cannot send and
    // says nothing about this one.
    let founder = Keys::generate();
    let seated_lead = Keys::generate();
    let foreign_lead = Keys::generate();

    let authority = CurrentAuthority {
        seats: BTreeMap::from([(
            seated_lead.public_key().to_hex(),
            ("lead".to_string(), "33".repeat(32)),
        )]),
        ..CurrentAuthority::default()
    };
    let wake_scope = scope(Uuid::new_v4());
    let context = fold_context(&wake_scope, &founder.public_key().to_hex(), &authority);

    // The founder and this umbrella's own seated lead are not foreign.
    assert!(!wake_author_is_foreign(
        &context,
        &founder.public_key().to_hex()
    ));
    assert!(!wake_author_is_foreign(
        &context,
        &seated_lead.public_key().to_hex()
    ));

    // A real, seated, entirely legitimate lead of *another* umbrella is.
    // Membership of some umbrella is not membership of this one.
    assert!(wake_author_is_foreign(
        &context,
        &foreign_lead.public_key().to_hex()
    ));

    // And nothing this umbrella can be asked to wake resolves for that pubkey:
    // the published set for a foreign author is empty, because there is no
    // target to publish to.
    let package = package(vec![roster(
        &seated_lead.public_key().to_hex(),
        "lead",
        target(1),
    )]);
    let refused = resolve_actor_target(&package, &foreign_lead.public_key().to_hex())
        .expect_err("a foreign pubkey resolves to no provider generation");
    assert!(
        refused.contains("0 active receipt-backed provider generations"),
        "{refused}"
    );

    // The seated lead still resolves — the guard refuses foreigners, not
    // everyone, and a rule that broke ordinary wakes would be replaced within
    // the hour and the line lost with it.
    resolve_actor_target(&package, &seated_lead.public_key().to_hex())
        .expect("this umbrella's own lead still has a target");
}

#[test]
fn a_note_citing_another_umbrella_is_a_pointer_and_not_a_wake() {
    // A lead *may* publish a note citing the other umbrella's commit or
    // checkpoint — note refs are pointers and cross-umbrella pointers are
    // allowed. What must never follow is a wake, and `is_team_wake_pointer`
    // is what decides whether a turn's text is one.
    assert!(
        !is_team_wake_pointer("Their wip ref touches crates/buzz-core/src/pulse.rs too."),
        "prose citing another umbrella is prose"
    );
    assert!(
        !is_team_wake_pointer(&format!("see {}", "ab".repeat(32))),
        "an event id in prose is a pointer, not a wake"
    );
}

#[test]
fn every_wake_source_names_the_key_that_signed_it() {
    // The guard is only as good as its input: a source with no author would
    // slip past `wake_author_is_foreign` untested. Both variants carry one.
    let report = WakeSource::Report {
        operation_id: "aa".repeat(32),
        operation_type: "report".into(),
        author_pubkey: "bb".repeat(32),
        created_at: 1,
    };
    assert_eq!(report.author_pubkey(), Some("bb".repeat(32).as_str()));

    let terminal = WakeSource::Terminal {
        terminal_event_id: "cc".repeat(32),
        actor_pubkey: "dd".repeat(32),
        role: "builder".into(),
        caused_by_command_id: "cmd".into(),
        source_target: target(1),
        prompt_at_ms: None,
        terminal_at_ms: 2,
    };
    assert_eq!(terminal.author_pubkey(), Some("dd".repeat(32).as_str()));
}
