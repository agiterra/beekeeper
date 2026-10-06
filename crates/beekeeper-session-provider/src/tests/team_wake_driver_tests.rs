use super::*;

use nostr::{EventBuilder, Timestamp};

use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
use beekeeper_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery,
    CODING_SESSION_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{Capabilities, LifecycleReceipt, SessionMetadata};
use beekeeper_core::coding_session_routing::RoutingRecord;
use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamReport, CodingSessionTeamTransactionBody,
};
use beekeeper_sdk::builders::{
    build_coding_session_authority_transition, build_coding_session_genesis,
    build_coding_session_lifecycle_command, build_coding_session_lifecycle_receipt,
    build_coding_session_metadata, build_coding_session_turn_receipt,
};
use beekeeper_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};

pub(super) const BUILDER_ROLE: &str = "builder";
pub(super) const LEAD_ROLE: &str = "lead";

fn routing_record(role: &str, provider: &str) -> RoutingRecord {
    serde_json::from_value(serde_json::json!({
        "class": role,
        "tier": "standard",
        "risk": {"impact": 3, "uncertainty": 3, "irreversibility": 2, "score": 18},
        "profile": null,
        "chosen": {"provider": provider, "model": "default", "effort": "medium"},
        "runnerUp": null,
        "reason": "fixture route",
        "reviewRequired": false,
        "reviewReasons": [],
        "challengerSample": false,
        "override": null,
        "registryVersion": 1,
        "catalogRevision": 1
    }))
    .expect("routing fixture")
}

pub(super) struct DriverChannelFixture {
    pub(super) channel: Uuid,
    pub(super) scope: team_wake::WakeScope,
    pub(super) builder: Keys,
    pub(super) lead: Keys,
    pub(super) builder_target: CodingSessionTarget,
    pub(super) lead_target: CodingSessionTarget,
    pub(super) events: Vec<Event>,
    pub(super) assignments: Vec<Event>,
    pub(super) reports: Vec<Event>,
    pub(super) builder_record: SessionRecord,
}

fn seat_transition(
    founder: &Keys,
    channel: Uuid,
    genesis_ref: &str,
    previous: Option<String>,
    sequence: u32,
    actor: &Keys,
    role: &str,
) -> Event {
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_seat(
        genesis_ref,
        previous,
        sequence,
        actor.public_key().to_hex(),
        role,
    );
    build_coding_session_authority_transition(channel, &payload)
        .expect("seat transition builder")
        .sign_with_keys(founder)
        .expect("sign seat transition")
}

fn seat_receipt(
    relay: &Keys,
    channel: Uuid,
    genesis_ref: &str,
    transition: &Event,
    sequence: u32,
    actor: &Keys,
    role: &str,
) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({
            "type": authority::ACCEPTANCE_RECEIPT_TYPE,
            "genesisRef": genesis_ref,
            "acceptedEventId": transition.id.to_hex(),
            "seq": sequence,
            "transitionType": "grant-seat",
            "granteePubkey": actor.public_key().to_hex(),
            "role": role,
        })
        .to_string(),
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel.to_string()]).expect("channel tag")
    ])
    .sign_with_keys(relay)
    .expect("sign seat receipt")
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execution_events(
    founder: &Keys,
    provider_authority: &Keys,
    channel: Uuid,
    session_ref: &str,
    genesis_ref: &str,
    command_id: &str,
    provider_instance_ref: &str,
    actor: &Keys,
    role: &str,
    target: &CodingSessionTarget,
) -> Vec<Event> {
    let routing = routing_record(role, provider_instance_ref);
    let command = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: command_id.into(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(session_ref.into()),
            genesis_ref: Some(genesis_ref.into()),
            provider_instance_ref: provider_instance_ref.try_into().expect("alias"),
            provider_authority_pubkey: provider_authority.public_key().to_hex(),
            model: None,
            title: Some(format!("{role} fixture")),
            initial_turn: None,
            actor: Some(actor.public_key().to_hex()),
            role: Some(role.into()),
            hire_ref: None,
            routing: Some(routing.clone()),
        },
    };
    let command_event = build_coding_session_lifecycle_command(channel, &command)
        .expect("lifecycle command builder")
        .sign_with_keys(founder)
        .expect("sign lifecycle command");
    let receipt = LifecycleReceipt::created(command_id, target);
    let receipt_event = build_coding_session_lifecycle_receipt(
        channel,
        command_id,
        &serde_json::to_string(&receipt).expect("receipt json"),
    )
    .expect("lifecycle receipt builder")
    .sign_with_keys(provider_authority)
    .expect("sign lifecycle receipt");
    let metadata = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: Some(format!("{role} fixture")),
        agent_ref: Some(actor.public_key().to_hex()),
        role: Some(role.into()),
        provider: Some(provider_instance_ref.try_into().expect("alias")),
        runtime: Some("fixture".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_baseline(),
        session_ref: Some(session_ref.into()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: Some(routing),
        bee_stamp: None,
        pack_ref: None,
        compose_ref: None,
        handover: None,
    };
    let metadata_event = build_coding_session_metadata(
        channel,
        target,
        &serde_json::to_string(&metadata).expect("metadata json"),
    )
    .expect("metadata builder")
    .sign_with_keys(provider_authority)
    .expect("sign metadata");
    vec![command_event, receipt_event, metadata_event]
}

#[allow(clippy::too_many_arguments)]
fn resume_execution_events(
    founder: &Keys,
    provider_authority: &Keys,
    channel: Uuid,
    session_ref: &str,
    command_id: &str,
    provider_instance_ref: &str,
    actor: &Keys,
    role: &str,
    old_target: &CodingSessionTarget,
) -> (CodingSessionTarget, Vec<Event>) {
    let target = CodingSessionTarget {
        generation: old_target.generation + 1,
        ..old_target.clone()
    };
    let command = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: command_id.into(),
        action: CodingSessionLifecycleAction::SessionResume {
            session: old_target.clone(),
            provider_authority_pubkey: provider_authority.public_key().to_hex(),
        },
    };
    let command_event = build_coding_session_lifecycle_command(channel, &command)
        .expect("resume command builder")
        .sign_with_keys(founder)
        .expect("sign resume command");
    let receipt = LifecycleReceipt::resumed(command_id, &target);
    let receipt_event = build_coding_session_lifecycle_receipt(
        channel,
        command_id,
        &serde_json::to_string(&receipt).expect("resume receipt json"),
    )
    .expect("resume receipt builder")
    .sign_with_keys(provider_authority)
    .expect("sign resume receipt");
    let metadata = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: Some(format!("{role} fixture")),
        agent_ref: Some(actor.public_key().to_hex()),
        role: Some(role.into()),
        provider: Some(provider_instance_ref.try_into().expect("alias")),
        runtime: Some("fixture".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_baseline(),
        session_ref: Some(session_ref.into()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        compose_ref: None,
        handover: None,
    };
    let metadata_event = build_coding_session_metadata(
        channel,
        &target,
        &serde_json::to_string(&metadata).expect("resume metadata json"),
    )
    .expect("resume metadata builder")
    .sign_with_keys(provider_authority)
    .expect("sign resume metadata");
    (target, vec![command_event, receipt_event, metadata_event])
}

fn assignment_and_report(
    fixture: &DriverChannelFixture,
    index: usize,
    report_created_at: u64,
) -> (Event, Event) {
    let assignment = build_coding_session_team_transaction(
        &fixture.channel.to_string(),
        coding_session_team_transaction_payload(
            fixture.scope.session_ref.clone(),
            fixture.scope.genesis_ref.clone(),
            None,
            None,
            CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                assignee_actor: fixture.builder.public_key().to_hex(),
                assignee_role: BUILDER_ROLE.into(),
                objective: format!("objective {index}"),
                brief: "Produce one signed report.".into(),
                branch: None,
                base_sha: None,
                file_ownership: vec![format!("fixture/{index}")],
                acceptance_steps: vec!["report".into()],
            }),
        ),
    )
    .expect("assignment builder")
    .sign_with_keys(&fixture.lead)
    .expect("sign assignment");
    let report = build_coding_session_team_transaction(
        &fixture.channel.to_string(),
        coding_session_team_transaction_payload(
            fixture.scope.session_ref.clone(),
            fixture.scope.genesis_ref.clone(),
            None,
            None,
            CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                assignment_ref: assignment.id.to_hex(),
                summary: format!("report {index}"),
                branch: None,
                base_sha: None,
                head_sha: None,
                files: Vec::new(),
                tests: Vec::new(),
                red_before_green: None,
                deviations: Vec::new(),
                residuals: Vec::new(),
                anomalies: Vec::new(),
            }),
        ),
    )
    .expect("report builder")
    .custom_created_at(Timestamp::from(report_created_at))
    .sign_with_keys(&fixture.builder)
    .expect("sign report");
    (assignment, report)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn driver_channel_fixture(
    provider: &Provider,
    relay: &Keys,
    cwd: &Path,
    channel: Uuid,
    role_with_lead: bool,
    report_count: usize,
    skew: impl Fn(usize) -> u64,
) -> DriverChannelFixture {
    let founder = &provider.config.keys;
    let session_ref = channel.to_string();
    let genesis = build_coding_session_genesis(
        channel,
        &beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload::new(&session_ref),
    )
    .expect("genesis builder")
    .sign_with_keys(founder)
    .expect("sign genesis");
    let genesis_ref = genesis.id.to_hex();
    let scope = team_wake::WakeScope {
        channel_ref: channel,
        session_ref: session_ref.clone(),
        genesis_ref: genesis_ref.clone(),
    };
    let builder = Keys::generate();
    let lead = Keys::generate();
    let builder_target = CodingSessionTarget {
        driver: "fixture-acp".into(),
        instance_id: provider.config.instance_id.clone(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let lead_target = CodingSessionTarget {
        driver: "fixture-acp".into(),
        instance_id: "remote-lead-host".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let first = seat_transition(
        founder,
        channel,
        &genesis_ref,
        None,
        1,
        &builder,
        BUILDER_ROLE,
    );
    let first_receipt = seat_receipt(
        relay,
        channel,
        &genesis_ref,
        &first,
        1,
        &builder,
        BUILDER_ROLE,
    );
    let mut events = vec![genesis, first.clone(), first_receipt];
    let second = seat_transition(
        founder,
        channel,
        &genesis_ref,
        Some(first.id.to_hex()),
        2,
        &lead,
        LEAD_ROLE,
    );
    let second_receipt = seat_receipt(relay, channel, &genesis_ref, &second, 2, &lead, LEAD_ROLE);
    events.extend([second, second_receipt]);
    events.extend(execution_events(
        founder,
        founder,
        channel,
        &session_ref,
        &genesis_ref,
        "create-builder",
        "fixture-provider",
        &builder,
        BUILDER_ROLE,
        &builder_target,
    ));
    if role_with_lead {
        events.extend(execution_events(
            founder,
            founder,
            channel,
            &session_ref,
            &genesis_ref,
            "create-lead",
            "fixture-provider",
            &lead,
            LEAD_ROLE,
            &lead_target,
        ));
    }
    let mut builder_record = governed_record(channel, cwd, &genesis_ref);
    builder_record
        .session_id
        .clone_from(&builder_target.session_id);
    builder_record.driver.clone_from(&builder_target.driver);
    builder_record.provider_instance_ref = "fixture-provider".into();
    builder_record.session_ref = Some(session_ref);
    builder_record.genesis_ref = Some(genesis_ref);
    builder_record.actor = Some(builder.public_key().to_hex());
    builder_record.role = Some(BUILDER_ROLE.into());
    let mut fixture = DriverChannelFixture {
        channel,
        scope,
        builder,
        lead,
        builder_target,
        lead_target,
        events,
        assignments: Vec::new(),
        reports: Vec::new(),
        builder_record,
    };
    for index in 0..report_count {
        let (assignment, report) = assignment_and_report(&fixture, index, skew(index));
        fixture.events.extend([assignment.clone(), report.clone()]);
        fixture.assignments.push(assignment);
        fixture.reports.push(report);
    }
    fixture
}

pub(super) fn published_wakes(control: &RecordingTestRelay) -> Vec<Event> {
    control
        .published
        .lock()
        .expect("published lock")
        .iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_COMMAND)
        .cloned()
        .collect()
}

fn query_count_for(control: &RecordingTestRelay, channel: Uuid) -> usize {
    let channel = channel.to_string();
    control
        .queries
        .lock()
        .expect("queries lock")
        .iter()
        .filter(|query| query.to_string().contains(&channel))
        .count()
}

fn add_turn_outcome(control: &RecordingTestRelay, provider_authority: &Keys, event: &Event) {
    let payload: CodingSessionCommandPayload =
        serde_json::from_str(&event.content).expect("published wake payload");
    let channel = event
        .tags
        .iter()
        .find_map(|tag| {
            let tag = tag.as_slice();
            (tag.first().map(String::as_str) == Some("h"))
                .then(|| Uuid::parse_str(&tag[1]).expect("wake channel"))
        })
        .expect("wake h tag");
    let receipt = LifecycleReceipt::turn_queued(&payload.command_id, &payload.target);
    let receipt = build_coding_session_turn_receipt(
        channel,
        &payload.command_id,
        beekeeper_core::coding_session_payload::ReceiptStatus::TurnQueued,
        &serde_json::to_string(&receipt).expect("turn receipt json"),
    )
    .expect("turn receipt builder")
    .sign_with_keys(provider_authority)
    .expect("sign turn receipt");
    control.events.lock().expect("events lock").push(receipt);
}

pub(super) fn expire_backoffs(provider: &mut Provider) {
    provider.team_wake_backoff.clear();
    provider.team_wake_discovery_backoff.clear();
}

/// v3.1 §6 T0: the production scheduler and real RelayEventPublisher seam
/// jointly prove structural parking, rotation fairness, timestamp-independent
/// discovery, terminal admission under report pressure, R1/R2 publication
/// fences, restart recovery, and replay idempotence. Publication is counted
/// only from kind-44220 EVENT frames received by the fake relay.
#[tokio::test]
async fn team_wake_t0_driver_publisher_survives_pressure_restart_and_rollover() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("checkout");
    let founder = test_operator_keys().clone();
    let relay_keys = Keys::generate();
    let channels = [
        Uuid::from_u128(0xa),
        Uuid::from_u128(0xb),
        Uuid::from_u128(0xc),
        Uuid::from_u128(0xd),
    ];
    let mut provider = Provider::new(config_of(
        founder.clone(),
        &dir.path().join("state"),
        None,
        "missing-agent".into(),
    ))
    .expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());

    let a = driver_channel_fixture(&provider, &relay_keys, &cwd, channels[0], true, 1, |_| 10);
    let b = driver_channel_fixture(&provider, &relay_keys, &cwd, channels[1], false, 1, |_| 20);
    let c = driver_channel_fixture(
        &provider,
        &relay_keys,
        &cwd,
        channels[2],
        true,
        64,
        |index| {
            if index == 0 {
                1
            } else {
                100 + index as u64
            }
        },
    );
    let d = driver_channel_fixture(
        &provider,
        &relay_keys,
        &cwd,
        channels[3],
        true,
        2,
        |index| {
            if index == 0 {
                4_000_000_000
            } else {
                300
            }
        },
    );

    provider
        .state
        .insert_session(b.builder_record.clone())
        .expect("insert B builder");
    provider
        .state
        .insert_session(c.builder_record.clone())
        .expect("insert C builder");
    provider
        .state
        .insert_session(d.builder_record.clone())
        .expect("insert D builder");
    for report in &c.reports {
        provider
            .on_team_transaction(channels[2], report)
            .expect("live-admit C report");
    }
    assert_eq!(provider.team_wakes.channel_counts(channels[2]).1, 64);
    let terminal_command_id = "terminal-assignment-command";
    let terminal_assignment = build_coding_session_team_transaction(
        &channels[2].to_string(),
        coding_session_team_transaction_payload(
            c.scope.session_ref.clone(),
            c.scope.genesis_ref.clone(),
            None,
            Some(terminal_command_id.into()),
            CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                assignee_actor: c.builder.public_key().to_hex(),
                assignee_role: BUILDER_ROLE.into(),
                objective: "terminal diagnostic assignment".into(),
                brief: "This assignment intentionally ends without a report.".into(),
                branch: None,
                base_sha: None,
                file_ownership: vec!["fixture/terminal".into()],
                acceptance_steps: vec!["terminal diagnostic".into()],
            }),
        ),
    )
    .expect("terminal assignment builder")
    .sign_with_keys(&c.lead)
    .expect("sign terminal assignment");
    let terminal_pointer = serde_json::json!({
        "operationId": terminal_assignment.id.to_hex(),
        "type": "assignment"
    })
    .to_string();
    let terminal_command = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.into(),
        command_id: terminal_command_id.into(),
        target: c.builder_target.clone(),
        action: CodingSessionAction::ThreadTurnStart {
            text: terminal_pointer,
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        },
    };
    let terminal_command =
        beekeeper_sdk::builders::build_coding_session_command(channels[2], &terminal_command)
            .expect("terminal command builder")
            .sign_with_keys(&founder)
            .expect("sign terminal command");
    assert!(provider
        .team_wakes
        .capture_terminal(
            c.scope.clone(),
            team_wake::WakeSource::Terminal {
                terminal_event_id: "ee".repeat(32),
                actor_pubkey: c.builder.public_key().to_hex(),
                role: BUILDER_ROLE.into(),
                caused_by_command_id: terminal_command_id.into(),
                source_target: c.builder_target.clone(),
                prompt_at_ms: Some(now_ms().saturating_sub(10_000)),
                terminal_at_ms: now_ms().saturating_sub(5_000),
            },
        )
        .expect("terminal under report saturation"));

    // Restart after live admission but before the complete scan. Report refs
    // and the provider-local terminal must survive without any test-only
    // cursor or reconstructed progress.
    drop(provider);
    let mut provider = Provider::new(config_of(
        founder.clone(),
        &dir.path().join("state"),
        None,
        "missing-agent".into(),
    ))
    .expect("restart before discovery");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    assert_eq!(
        provider.team_wakes.channel_counts(channels[2]),
        (0, 64, 0, 1)
    );
    provider.subscribed.extend(channels);

    let mut relay_events = Vec::new();
    relay_events.extend(vec![a.reports[0].clone(); 1_000]);
    relay_events.extend(
        a.events
            .iter()
            .filter(|event| event.id != a.reports[0].id)
            .cloned(),
    );
    relay_events.extend(b.events.clone());
    relay_events.extend(c.events.clone());
    relay_events.push(terminal_assignment);
    relay_events.push(terminal_command);
    relay_events.extend(d.events.clone());
    let (relay, control, server) = spawn_recording_test_relay(&founder, relay_events).await;
    provider.set_rest_client(relay.rest_client());
    let publisher = relay.event_publisher();

    // A reaches the complete-partition bound once and is then removed from
    // both discovery and processing candidate sets.
    provider
        .run_one_team_wake_tick(&publisher)
        .await
        .expect("A refusal tick");
    assert_eq!(
        provider
            .team_wakes
            .refusal(channels[0])
            .map(|item| item.code),
        Some(team_wake::ChannelRefusalCode::PartitionSaturated)
    );
    let a_queries = query_count_for(&control, channels[0]);

    // B, C, and D each receive one scheduler visit in the next rotation. B
    // blocks on the missing lead; C and D bind identity but R1 forbids a send
    // in the same pass.
    for _ in 0..3 {
        expire_backoffs(&mut provider);
        let before = published_wakes(&control).len();
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("rotation tick");
        assert_eq!(
            published_wakes(&control).len(),
            before,
            "an identity-mutating pass must not publish"
        );
    }
    assert_eq!(query_count_for(&control, channels[0]), a_queries);
    assert_eq!(
        provider
            .team_wakes
            .pending_for_channel(channels[1])
            .expect("B pending")
            .and_then(|intent| intent.last_reason),
        Some("lead_target_not_exact".into())
    );
    for channel in [channels[2], channels[3]] {
        let bound = provider
            .team_wakes
            .pending_for_channel(channel)
            .expect("healthy pending")
            .expect("healthy channel visited once");
        assert!(bound.target.is_some());
        assert!(bound.command_id.is_some());
        assert!(bound.signed_event.is_none());
    }

    // Drive C/D by production ticks. Each first visit binds, each second sends
    // to the fake relay, and a provider-signed outcome on the next visit
    // retires it. B gets one blocked visit whenever its slot rotates around.
    let mut settled = HashSet::new();
    let expected_reports: HashSet<String> = c
        .reports
        .iter()
        .chain(d.reports.iter())
        .map(|event| event.id.to_hex())
        .collect();
    let mut guard = 0usize;
    while settled.len() < expected_reports.len() + 1
        || provider.team_wakes.has_work(channels[2])
        || provider.team_wakes.has_work(channels[3])
    {
        guard += 1;
        assert!(
            guard < 2_000,
            "C/D/terminal driver loop stalled: settled={}/{} published={} B={:?} C={:?} D={:?}",
            settled.len(),
            expected_reports.len() + 1,
            published_wakes(&control).len(),
            provider.team_wakes.channel_counts(channels[1]),
            provider.team_wakes.channel_counts(channels[2]),
            provider.team_wakes.channel_counts(channels[3]),
        );
        expire_backoffs(&mut provider);
        let before = published_wakes(&control);
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("drain tick");
        let after = published_wakes(&control);
        for wake in after.iter().skip(before.len()) {
            let payload: CodingSessionCommandPayload =
                serde_json::from_str(&wake.content).expect("wake payload");
            let pointer = match payload.action {
                CodingSessionAction::ThreadTurnStart { text, .. } => text,
                _ => panic!("wake must be a turn start"),
            };
            let pointer: serde_json::Value = serde_json::from_str(&pointer).expect("wake pointer");
            let source = pointer
                .get("operationId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("terminal")
                .to_owned();
            assert!(settled.insert(source), "source published twice");
            add_turn_outcome(&control, &founder, wake);
        }
    }
    assert_eq!(
        provider.team_wakes.channel_counts(channels[2]),
        (64, 0, 0, 0)
    );
    assert_eq!(
        provider.team_wakes.channel_counts(channels[3]),
        (2, 0, 0, 0)
    );
    assert_eq!(query_count_for(&control, channels[0]), a_queries);

    // One more D report publishes to generation one. Its signed outcome is
    // made visible together with a generation-two lead before restart. The
    // reopened provider must settle the old target and publish nothing to the
    // new generation (R2).
    let (rollover_assignment, rollover_report) = assignment_and_report(&d, 99, 500);
    control
        .events
        .lock()
        .expect("events lock")
        .extend([rollover_assignment, rollover_report.clone()]);
    provider
        .on_team_transaction(channels[3], &rollover_report)
        .expect("admit rollover report");
    expire_backoffs(&mut provider);
    let before_rollover = published_wakes(&control).len();
    provider
        .process_team_wake_for(channels[3], &publisher)
        .await
        .expect("bind rollover wake");
    assert_eq!(published_wakes(&control).len(), before_rollover);
    expire_backoffs(&mut provider);
    control.reject_next_publish.store(true, Ordering::SeqCst);
    provider
        .process_team_wake_for(channels[3], &publisher)
        .await
        .expect("relay rejection leaves exact signed attempt durable");
    let after_rejection = published_wakes(&control);
    assert_eq!(after_rejection.len(), before_rollover + 1);
    let rejected_wake = after_rejection
        .last()
        .expect("rejected old-target wake")
        .clone();
    let durable = provider
        .team_wakes
        .pending_for_channel(channels[3])
        .expect("read rejected attempt")
        .expect("rejected attempt remains pending");
    assert_eq!(durable.signed_event.as_ref(), Some(&rejected_wake));
    assert!(durable.relay_accepted_at.is_none());

    // The fake endpoint received EVENT but rejected it before the driver could
    // persist accepted_at. Restart must resend byte-exact signed bytes through
    // RelayEventPublisher, not reconstruct an equivalent event.
    drop(provider);
    let mut provider = Provider::new(config_of(
        founder.clone(),
        &dir.path().join("state"),
        None,
        "missing-agent".into(),
    ))
    .expect("restart after endpoint rejection");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    provider.set_rest_client(relay.rest_client());
    provider.subscribed.extend(channels);
    provider
        .process_team_wake_for(channels[3], &publisher)
        .await
        .expect("resend rejected signed attempt");
    let after_old_publish = published_wakes(&control);
    assert_eq!(after_old_publish.len(), before_rollover + 2);
    let old_wake = after_old_publish.last().expect("accepted retry").clone();
    assert_eq!(old_wake, rejected_wake);
    let old_payload: CodingSessionCommandPayload =
        serde_json::from_str(&old_wake.content).expect("old wake payload");
    assert_eq!(old_payload.target, d.lead_target);
    add_turn_outcome(&control, &founder, &old_wake);
    let (new_lead_target, resume_events) = resume_execution_events(
        &founder,
        &founder,
        channels[3],
        &d.scope.session_ref,
        "resume-lead",
        "fixture-provider",
        &d.lead,
        LEAD_ROLE,
        &d.lead_target,
    );
    assert_ne!(new_lead_target, d.lead_target);
    control
        .events
        .lock()
        .expect("events lock")
        .extend(resume_events);

    // This second boundary is after proof visibility but before retirement.
    // The reopened provider must settle the old generation without sending to
    // the new lead generation.
    drop(provider);
    let mut provider = Provider::new(config_of(
        founder.clone(),
        &dir.path().join("state"),
        None,
        "missing-agent".into(),
    ))
    .expect("restart provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    provider.set_rest_client(relay.rest_client());
    provider.subscribed.extend(channels);
    provider
        .process_team_wake_for(channels[3], &publisher)
        .await
        .expect("settle proven old target after restart");
    assert_eq!(published_wakes(&control).len(), before_rollover + 2);
    assert_eq!(provider.team_wakes.channel_counts(channels[3]).0, 3);

    // A restart reconstructs every durable debt but no scheduler cursor. Its
    // one allowed re-probe reaffirms the refusal, then it consumes zero ticks.
    let before_restart_ticks = published_wakes(&control).len();
    for _ in 0..8 {
        expire_backoffs(&mut provider);
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("restart tick");
    }
    assert_eq!(query_count_for(&control, channels[0]), a_queries * 2);
    assert_eq!(published_wakes(&control).len(), before_restart_ticks);

    // Both live admission and a complete rescan rediscover only permanent ids;
    // neither may reach the publisher.
    for report in c.reports.iter().chain(d.reports.iter()) {
        provider
            .on_team_transaction(
                report
                    .tags
                    .iter()
                    .find_map(|tag| {
                        let tag = tag.as_slice();
                        (tag.first().map(String::as_str) == Some("h"))
                            .then(|| Uuid::parse_str(&tag[1]).expect("report channel"))
                    })
                    .expect("h tag"),
                report,
            )
            .expect("live replay");
    }
    provider
        .on_team_transaction(channels[3], &rollover_report)
        .expect("live rollover replay");
    provider.team_wake_scanned_channels.remove(&channels[2]);
    provider.team_wake_scanned_channels.remove(&channels[3]);
    provider.discover_team_wake_partition_for(channels[2]).await;
    provider.discover_team_wake_partition_for(channels[3]).await;
    for _ in 0..8 {
        expire_backoffs(&mut provider);
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("replay tick");
    }
    assert_eq!(published_wakes(&control).len(), before_restart_ticks);

    relay.shutdown().await;
    server.abort();
}
