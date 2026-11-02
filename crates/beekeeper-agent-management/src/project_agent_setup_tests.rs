//! The chain's pure rules. The chain itself is exercised against a real relay,
//! Host and scripted runtime in `scripts/host-manage-check.py`.
use super::*;
use nostr::Keys;

#[test]
fn a_named_code_survives_classification_and_a_sentence_gets_one() {
    let (code, message, uncertain) = classify(&refused("CHANNEL_NOT_OWNED", "not yours"));
    assert_eq!(code, "CHANNEL_NOT_OWNED");
    assert_eq!(message, "not yours");
    assert!(!uncertain);
    assert!(classify(&Error::Other("connection reset".into())).2);
    let (code, _, uncertain) = classify(&Error::Unconfirmed("lost reply".into()));
    assert_eq!(code, "OPERATION_UNCONFIRMED");
    assert!(uncertain, "an unconfirmed operation may have taken effect");
    let (code, _, _) = classify(&Error::Conflict("Operation: used".into()));
    assert_eq!(
        code, "OPERATION_CONFLICT",
        "a sentence with a colon is not a code"
    );
}

#[test]
fn an_allowlist_must_be_real_keys_and_cannot_hold_the_owner() {
    let owner = "aa".repeat(32);
    let other = "bb".repeat(32);
    assert!(access_refusal(&SetupAccess::OwnerOnly, &owner).is_none());
    assert!(access_refusal(&SetupAccess::Allowlist(vec![other.clone()]), &owner).is_none());
    assert!(access_refusal(&SetupAccess::Allowlist(vec![]), &owner).is_some());
    assert!(access_refusal(&SetupAccess::Allowlist(vec!["BB".repeat(32)]), &owner).is_some());
    assert!(access_refusal(&SetupAccess::Allowlist(vec!["npub1x".into()]), &owner).is_some());
    assert!(access_refusal(&SetupAccess::Allowlist(vec![owner.clone()]), &owner).is_some());
}

#[test]
fn only_a_signed_refusal_reads_as_a_provider_refusal() {
    let error = Error::Refused(crate::management::tests_support_refusal(
        "op",
        "receipt",
        "MODEL_AUTH_UNAVAILABLE",
        "no login",
    ));
    let (receipt, signed) = crate::management::parse_provider_refusal(&error).unwrap();
    assert_eq!(receipt, "receipt");
    assert_eq!(signed.code, "MODEL_AUTH_UNAVAILABLE");
    for other in [
        Error::Refused("CHANNEL_NOT_OWNED: not yours".into()),
        Error::Refused("provider refused operation x in receipt y: not json".into()),
        Error::Unconfirmed("provider refused operation x in receipt y: {}".into()),
    ] {
        assert!(
            crate::management::parse_provider_refusal(&other).is_none(),
            "{other}"
        );
    }
}

#[test]
fn only_the_age_refusal_of_an_old_event_counts_as_expired() {
    use crate::management::relay_refused_for_age;
    let keys = Keys::generate();
    let sign = |at: u64| {
        nostr::EventBuilder::new(nostr::Kind::TextNote, "x")
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(&keys)
            .unwrap()
    };
    let now = nostr::Timestamp::now().as_secs();
    let (old, future) = (sign(now - 1000), sign(now + 1000));
    let too_far = || -> Result<Value, Error> {
        Err(Error::Relay {
            status: 400,
            body: "invalid: event timestamp too far from server time".into(),
        })
    };
    assert!(relay_refused_for_age(&too_far(), &old));
    assert!(
        !relay_refused_for_age(&too_far(), &future),
        "a clock problem"
    );
    let timeout: Result<Value, Error> = Err(Error::Other("timed out".into()));
    assert!(!relay_refused_for_age(&timeout, &old), "says nothing");
    let other: Result<Value, Error> = Err(Error::Relay {
        status: 400,
        body: "invalid: bad tags".into(),
    });
    assert!(!relay_refused_for_age(&other, &old));
    let server: Result<Value, Error> = Err(Error::Relay {
        status: 500,
        body: "timestamp too far".into(),
    });
    assert!(!relay_refused_for_age(&server, &old));
    let rejected: Result<Value, Error> = Ok(
        json!({"accepted": false, "message": "invalid: event timestamp too far from server time"}),
    );
    assert!(relay_refused_for_age(&rejected, &old));
    assert!(is_expired(&crate::management::expired_error(&old)));
    assert!(!is_expired(&Error::Unconfirmed(
        "JOURNAL_EVENT_EXPIRED: no".into()
    )));
}

#[test]
fn closing_an_attempt_traces_old_to_new_and_holds_the_new_id_unsent() {
    let spec = AgentSetupSpec {
        project: "30621:aa:slug".into(),
        channel: Uuid::from_u128(1),
        role: "lead".into(),
        display_name: "Lead".into(),
        provider_instance: "acme-primary".into(),
        model: "m".into(),
        parallelism: 1,
        access: SetupAccess::OwnerOnly,
    };
    let mut setup = AgentSetup {
        version: AGENT_SETUP_VERSION,
        setup_id: Uuid::from_u128(2),
        spec_digest: spec.digest().unwrap(),
        spec,
        owner: "aa".into(),
        relay_url: "ws://r".into(),
        created_at: String::new(),
        updated_at: String::new(),
        state: SetupState::NeedsAttention,
        install_operation: Uuid::from_u128(3),
        start_operation: Uuid::from_u128(4),
        confirmed_start: None,
        context_operation: Uuid::from_u128(5),
        session_ref: Uuid::from_u128(6),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: None,
        revision: None,
        receipt_event_id: None,
        steps: Vec::new(),
        last_refusal: None,
        readback: None,
    };
    let old = Uuid::from_u128(4);
    let fresh = close_attempt(&mut setup, "start", old, "CODE", "evidence".into());
    assert_ne!(fresh, old);
    assert_eq!(setup.attempts[0].operation_id, old);
    assert_eq!(setup.attempts[0].replaced_by, fresh);
    let held = setup.pending_action.as_ref().unwrap();
    assert_eq!(held.operation_id, fresh);
    assert!(!held.attempted);
    assert_eq!(current_start(&setup), fresh);
    let context = close_attempt(
        &mut setup,
        "context",
        Uuid::from_u128(5),
        "CODE",
        "e".into(),
    );
    assert_ne!(
        setup.pending_action.as_ref().unwrap().operation_id,
        context,
        "context holds no action"
    );
}

#[test]
fn a_confirmed_start_marks_only_its_own_attempt_landed() {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        home: dir.path().to_path_buf(),
        instance: beekeeper_host_core::layout::Instance::Production,
        native_home: None,
    };
    let spec = AgentSetupSpec {
        project: "30621:aa:slug".into(),
        channel: Uuid::from_u128(1),
        role: "lead".into(),
        display_name: "Lead".into(),
        provider_instance: "acme-primary".into(),
        model: "m".into(),
        parallelism: 1,
        access: SetupAccess::OwnerOnly,
    };
    let confirmed = Uuid::from_u128(4);
    let mut setup = AgentSetup {
        version: AGENT_SETUP_VERSION,
        setup_id: Uuid::from_u128(2),
        spec_digest: spec.digest().unwrap(),
        spec,
        owner: "aa".into(),
        relay_url: "ws://r".into(),
        created_at: String::new(),
        updated_at: String::new(),
        state: SetupState::Running,
        install_operation: Uuid::from_u128(3),
        start_operation: confirmed,
        confirmed_start: Some(agent_setup::ConfirmedSetupStart {
            operation_id: confirmed,
            target: beekeeper_core::coding_session_command::CodingSessionTarget {
                driver: "d".into(),
                instance_id: "i".into(),
                session_id: "s".into(),
                generation: 1,
            },
        }),
        context_operation: Uuid::from_u128(5),
        session_ref: Uuid::from_u128(6),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: None,
        revision: None,
        receipt_event_id: None,
        steps: Vec::new(),
        last_refusal: None,
        readback: None,
    };
    assert!(start_status(&context, &setup).unwrap() == StartStatus::Landed);
    // A later attempt is not landed by an older confirmation, however it ended.
    let fresh = Uuid::from_u128(9);
    setup.pending_action = Some(PendingAction {
        action: "start".into(),
        operation_id: fresh,
        attempted: true,
    });
    assert!(start_status(&context, &setup).unwrap() == StartStatus::NotSent);
    let path = local::operation_path(&context, fresh).unwrap();
    std::fs::write(
        &path,
        json!({"request":{},"actorPubkey":"aa","events":[]}).to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(start_status(&context, &setup).unwrap() == StartStatus::Unknown);
}

#[test]
fn a_lost_reply_reuses_the_action_id_and_a_different_action_gets_a_new_one() {
    let spec = AgentSetupSpec {
        project: "30621:aa:slug".into(),
        channel: Uuid::from_u128(1),
        role: "lead".into(),
        display_name: "Lead".into(),
        provider_instance: "acme-primary".into(),
        model: "m".into(),
        parallelism: 1,
        access: SetupAccess::OwnerOnly,
    };
    let mut setup = AgentSetup {
        version: AGENT_SETUP_VERSION,
        setup_id: Uuid::from_u128(2),
        spec_digest: spec.digest().unwrap(),
        spec,
        owner: "aa".into(),
        relay_url: "ws://r".into(),
        created_at: String::new(),
        updated_at: String::new(),
        state: SetupState::Running,
        install_operation: Uuid::from_u128(3),
        start_operation: Uuid::from_u128(4),
        confirmed_start: None,
        context_operation: Uuid::from_u128(5),
        session_ref: Uuid::from_u128(6),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: None,
        revision: None,
        receipt_event_id: None,
        steps: Vec::new(),
        last_refusal: None,
        readback: None,
    };
    let first = pending(&mut setup, "stop");
    assert_eq!(first, pending(&mut setup, "stop"));
    let second = pending(&mut setup, "start");
    assert_ne!(first, second);
    assert_eq!(setup.pending_action.as_ref().unwrap().action, "start");
}
