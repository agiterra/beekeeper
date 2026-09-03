use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptStatus, SessionMetadata, SessionStatus,
    LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA,
};
use buzz_sdk::builders::{
    build_coding_session_genesis, build_coding_session_lifecycle_command,
    build_coding_session_lifecycle_receipt, build_coding_session_metadata,
};
use nostr::Keys;
use uuid::Uuid;

use super::crew::{HiredSeat, SeatReceipt};
use super::hire_evidence::{create_receipt_binding, verify_hire_evidence, HireEvidenceRequest};

fn request<'a>(
    channel: &'a str,
    session_ref: &'a str,
    genesis: &'a str,
    role: &'a str,
    provider_instance: Option<&'a str>,
) -> HireEvidenceRequest<'a> {
    HireEvidenceRequest {
        channel,
        session_ref,
        genesis,
        role,
        provider_instance,
    }
}

fn signed_hire() -> (
    Vec<serde_json::Value>,
    HiredSeat,
    SeatReceipt,
    String,
    String,
    Keys,
) {
    let channel = Uuid::new_v4();
    let session = Uuid::new_v4().to_string();
    let host = Keys::generate();
    let provider = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let genesis_event =
        build_coding_session_genesis(channel, &CodingSessionGenesisPayload::new(session.clone()))
            .expect("genesis builder")
            .sign_with_keys(&host)
            .expect("sign genesis");
    let genesis = genesis_event.id.to_hex();
    let command_id = "create-hired";
    // Live shape (cleantest, 2026-09-01): the receipt's target carries the
    // provider's cryptographic short id, never the human alias the create and
    // metadata name (`provider-1` here, `claude-primary` on the wire).
    let target = CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: "session-1".into(),
        generation: 1,
    };
    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: command_id.into(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(session.clone()),
            genesis_ref: Some(genesis.clone()),
            provider_instance_ref: "provider-1".try_into().expect("alias"),
            provider_authority_pubkey: provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: Some("build it".into()),
            actor: Some(actor.clone()),
            role: Some("builder".into()),
            hire_ref: None,
            routing: None,
        },
    };
    let create = build_coding_session_lifecycle_command(channel, &create_payload)
        .expect("create builder")
        .sign_with_keys(&host)
        .expect("sign create");
    let receipt_payload = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: command_id.into(),
        status: ReceiptStatus::Created,
        session: Some(target.clone()),
        error: None,
        turn_id: None,
    };
    let receipt = build_coding_session_lifecycle_receipt(
        channel,
        command_id,
        &serde_json::to_string(&receipt_payload).expect("serialize receipt"),
    )
    .expect("receipt builder")
    .sign_with_keys(&provider)
    .expect("sign receipt");
    let metadata_payload = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: Some(actor.clone()),
        role: Some("builder".into()),
        provider: Some("provider-1".try_into().expect("alias")),
        // Live shape: the runtime word, not the driver slug.
        runtime: Some("claude".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: Some(session.clone()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
    };
    let metadata = build_coding_session_metadata(
        channel,
        &target,
        &serde_json::to_string(&metadata_payload).expect("serialize metadata"),
    )
    .expect("metadata builder")
    .sign_with_keys(&provider)
    .expect("sign metadata");
    let seat = HiredSeat {
        event_id: create.id.to_hex(),
        create_signer: host.public_key().to_hex(),
        command_id: command_id.into(),
        actor,
        role: "builder".into(),
        session_ref: session.clone(),
        genesis_ref: genesis.clone(),
        provider_authority_pubkey: provider.public_key().to_hex(),
        provider_instance_ref: "provider-1".try_into().expect("alias"),
        model: None,
        at: create.created_at.as_secs() as i64,
        raw: serde_json::to_value(&create).expect("create JSON"),
    };
    let seat_receipt = SeatReceipt {
        event_id: receipt.id.to_hex(),
        signer: provider.public_key().to_hex(),
        status: ReceiptStatus::Created,
        target_key: Some(coding_session_target_key(&target)),
        error_code: None,
        error_message: None,
        at: receipt.created_at.as_secs() as i64,
        raw: serde_json::to_value(&receipt).expect("receipt JSON"),
    };
    (
        vec![
            serde_json::to_value(genesis_event).expect("genesis JSON"),
            serde_json::to_value(create).expect("create JSON"),
            serde_json::to_value(receipt).expect("receipt JSON"),
            serde_json::to_value(metadata).expect("metadata JSON"),
        ],
        seat,
        seat_receipt,
        channel.to_string(),
        genesis,
        provider,
    )
}

#[test]
fn signed_create_receipt_and_provider_metadata_prove_exact_hire() {
    let (events, seat, receipt, channel, genesis, _provider) = signed_hire();
    verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &receipt,
    )
    .expect("verified hire");
}

#[test]
fn actor_role_mismatch_and_forged_or_unreceipted_create_never_authorize() {
    let (events, seat, receipt, channel, genesis, _provider) = signed_hire();
    assert!(verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "verifier",
            Some("provider-1"),
        ),
        &seat,
        &receipt,
    )
    .is_err());

    let mut forged = seat.clone();
    forged.raw["sig"] = serde_json::json!("0".repeat(128));
    assert!(verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &forged,
        &receipt,
    )
    .is_err());

    let mut unreceipted = receipt.clone();
    unreceipted.raw["sig"] = serde_json::json!("0".repeat(128));
    assert!(verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &unreceipted,
    )
    .is_err());
}

#[test]
fn member_controlled_matching_create_receipt_and_metadata_triplet_is_inert() {
    let (mut events, mut seat, receipt, channel, genesis, _provider) = signed_hire();
    let attacker = Keys::generate();
    let original: nostr::Event =
        serde_json::from_value(seat.raw.clone()).expect("original create event");
    let malicious = nostr::EventBuilder::new(original.kind, original.content.clone())
        .tags(original.tags.clone())
        .sign_with_keys(&attacker)
        .expect("malicious create signature");
    seat.event_id = malicious.id.to_hex();
    seat.create_signer = attacker.public_key().to_hex();
    seat.raw = serde_json::to_value(&malicious).expect("malicious create JSON");
    events.push(serde_json::to_value(malicious).expect("malicious create JSON"));

    let error = verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &receipt,
    )
    .expect_err("a matching self-described triplet cannot replace founder authority");
    assert!(error.to_string().contains("genesis") || error.to_string().contains("match"));
}

#[test]
fn provider_instance_must_match_request_create_receipt_and_metadata() {
    let (events, seat, receipt, channel, genesis, provider) = signed_hire();
    assert!(verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-2"),
        ),
        &seat,
        &receipt,
    )
    .is_err());

    let receipt_event: nostr::Event =
        serde_json::from_value(receipt.raw.clone()).expect("receipt event");
    let receipt_payload: LifecycleReceipt =
        serde_json::from_str(&receipt_event.content).expect("receipt payload");
    let mut wrong_receipt_payload = receipt_payload.clone();
    let wrong_receipt_target = CodingSessionTarget {
        driver: "acp".into(),
        instance_id: "provider-2".into(),
        session_id: "session-1".into(),
        generation: 1,
    };
    wrong_receipt_payload.session = Some(wrong_receipt_target.clone());
    let wrong_receipt_event = build_coding_session_lifecycle_receipt(
        Uuid::parse_str(&channel).expect("channel UUID"),
        &seat.command_id,
        &serde_json::to_string(&wrong_receipt_payload).expect("wrong receipt payload"),
    )
    .expect("wrong receipt builder")
    .sign_with_keys(&provider)
    .expect("wrong receipt signature");
    let wrong_receipt = SeatReceipt {
        event_id: wrong_receipt_event.id.to_hex(),
        signer: provider.public_key().to_hex(),
        status: receipt.status,
        target_key: Some(coding_session_target_key(&wrong_receipt_target)),
        error_code: None,
        error_message: None,
        at: wrong_receipt_event.created_at.as_secs() as i64,
        raw: serde_json::to_value(wrong_receipt_event).expect("wrong receipt JSON"),
    };
    assert!(verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &wrong_receipt,
    )
    .is_err());

    let mut wrong_metadata_events = events.clone();
    wrong_metadata_events.retain(|value| value["kind"] != serde_json::json!(44223));
    let target = receipt_payload.session.expect("receipt target");
    let wrong_metadata_payload = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: Some(seat.actor.clone()),
        role: Some("builder".into()),
        provider: Some("provider-2".try_into().expect("alias")),
        runtime: Some("wrong-runtime".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: Some(seat.session_ref.clone()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
    };
    let wrong_metadata = build_coding_session_metadata(
        Uuid::parse_str(&channel).expect("channel UUID"),
        &target,
        &serde_json::to_string(&wrong_metadata_payload).expect("wrong metadata payload"),
    )
    .expect("wrong metadata builder")
    .sign_with_keys(&provider)
    .expect("wrong metadata signature");
    wrong_metadata_events
        .push(serde_json::to_value(wrong_metadata).expect("wrong provider metadata JSON"));
    assert!(verify_hire_evidence(
        &wrong_metadata_events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &receipt,
    )
    .is_err());

    let mut wrong_target = receipt.clone();
    let target = CodingSessionTarget {
        driver: "acp".into(),
        instance_id: "provider-2".into(),
        session_id: "session-1".into(),
        generation: 1,
    };
    wrong_target.target_key = Some(coding_session_target_key(&target));
    assert!(verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &wrong_target,
    )
    .is_err());
}

/// Regression for the first live `seat-repair` (2026-09-01): the real
/// `created` receipt `3e53c993…` was refused as "unbound" because the binding
/// compared the target's cryptographic `instanceId` (`1958c6c448e05eed`) with
/// the create's human alias (`claude-primary`). They are different namespaces;
/// the alias is only comparable against the provider's own metadata.
#[test]
fn receipt_instance_id_is_the_providers_short_id_not_the_create_alias() {
    let (events, seat, receipt, channel, genesis, _provider) = signed_hire();
    assert_ne!(seat.provider_instance_ref.as_str(), "1958c6c448e05eed");
    let bound = create_receipt_binding(&channel, &seat, &receipt).expect("receipt binds");
    assert_eq!(
        bound.map(|target| target.instance_id),
        Some("1958c6c448e05eed".to_owned())
    );
    verify_hire_evidence(
        &events,
        &request(
            &channel,
            &seat.session_ref,
            &genesis,
            "builder",
            Some("provider-1"),
        ),
        &seat,
        &receipt,
    )
    .expect("live-shape evidence verifies");
}
