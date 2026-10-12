//! Previously accepted responsibility scopes remain usable after all-terminal Stop.
use super::*;
use nostr::EventBuilder;

#[tokio::test]
async fn prior_accepted_channel_stages_its_workspace_without_rewriting_latest_enrollment() {
    let dir = tempfile::tempdir().expect("provider");
    let channel = Uuid::new_v4();
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let cwd = cwd.canonicalize().expect("canonical workspace");
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut host = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    let owner = crate::tests::test_operator_keys();
    let actor = Keys::generate();
    let definition = beekeeper_sdk::managed_agent::build_managed_agent_event(
        &actor.public_key().to_hex(),
        &beekeeper_core::managed_agent::ManagedAgentEventContent {
            name: "scope fixture".into(),
            system_prompt: Some("ordinary work".into()),
            parallelism: 1,
            respond_to: beekeeper_core::managed_agent::RespondTo::OwnerOnly,
            persona_id: None,
            model: None,
            provider: None,
            persona_source_version: None,
            respond_to_allowlist: vec![],
            home_role: None,
            project_digest: None,
            project_withdrawn: false,
        },
    )
    .expect("definition")
    .sign_with_keys(owner)
    .expect("signature");
    let (relay, _, server) =
        crate::tests::spawn_test_relay_with_events(&host.config.keys, vec![definition.clone()])
            .await;
    host.set_rest_client(relay.rest_client());
    host.set_relay_self(host.config.keys.public_key().to_hex());
    let seats = dir.path().join("actor-seats.json");
    write(&seats, &json!({"version":1,"pending":{"first":{"pubkey":actor.public_key().to_hex(),"nsec":actor.secret_key().to_secret_hex(),"authTag":null,"relayUrl":host.config.relay_url}}})).expect("seat");
    host.config.actor_seats_file = Some(seats.clone());
    let original = crate::tests::create_event(&host, channel, "first");
    let mut payload: Value = serde_json::from_str(&original.content).expect("create");
    payload["action"]["actor"] = json!(actor.public_key().to_hex());
    let original = EventBuilder::new(original.kind, payload.to_string())
        .tags(original.tags)
        .sign_with_keys(owner)
        .expect("signed create");
    host.handle_command_event(channel, &original)
        .await
        .expect("actual fake ACP accepted actor");
    let record = host.state.sessions().next().expect("accepted").clone();
    let target = host.target_for(&record);
    assert_eq!(
        host.sessions
            .shutdown_and_join(&record.session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await,
        crate::session::ActorQuiescence::Joined
    );
    host.record_quiesced(&target).expect("actual joined group");
    host.state
        .update_session(&record.session_id, |row| row.closed = true)
        .expect("stopped");
    let accepted = host
        .state
        .session(&record.session_id)
        .expect("stopped record")
        .clone();
    let actor_hex = actor.public_key().to_hex();
    let actor_dir = host.config.state_dir.join("actors").join(&actor_hex);
    atomic_write::create_dir_all_restricted(&actor_dir).expect("custody");
    crate::state::atomic_write(
        &actor_dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("key");
    let auth =
        beekeeper_sdk::nip_oa::compute_auth_tag(owner, &actor.public_key(), "").expect("auth");
    crate::state::atomic_write(&actor_dir.join("auth-tag.json"), auth.as_bytes())
        .expect("auth custody");
    write(
        &host.config.state_dir.join("conversation-routes.json"),
        &json!({"routes":[{"actorPubkey":actor_hex,"channelId":channel,"target":target}]}),
    )
    .expect("accepted route");
    let latest = Uuid::new_v4();
    let latest_cwd = dir.path().join("latest");
    std::fs::create_dir(&latest_cwd).expect("latest workspace");
    let enrollment_path = actor_dir.join("enrollment.json");
    write(&enrollment_path, &json!({"actorPubkey":actor_hex,"channelId":latest,"providerInstance":record.provider_instance_ref,"definition":definition,"cwd":latest_cwd,"projectRef":null,"replayFloorUnix":definition.created_at.as_secs()})).expect("latest enrollment");
    let bytes = std::fs::read(&enrollment_path).expect("original enrollment");
    payload["commandId"] = json!(Uuid::new_v4());
    let command = payload["commandId"].as_str().expect("command").to_owned();
    let content = payload.to_string();
    stage_with_context(
        &host.config,
        &host.state,
        Some(&relay.rest_client()),
        channel,
        &owner.public_key().to_hex(),
        &content,
        None,
    )
    .await
    .expect("prior accepted scoped start");
    assert_eq!(
        std::fs::read(&enrollment_path).expect("latest unchanged"),
        bytes
    );
    let hint = projects
        .parent()
        .expect("parent")
        .join(beekeeper_host_core::project::PENDING_HINTS_DIR)
        .join(format!("{command}.json"));
    let hint: Value =
        serde_json::from_slice(&std::fs::read(hint).expect("hint")).expect("hint JSON");
    assert_eq!(hint["path"], cwd.to_string_lossy().as_ref());
    assert_eq!(
        accepted_workspace(
            &host.config,
            &host.state,
            &actor_hex,
            channel,
            &record.provider_instance_ref,
            None,
            &owner.public_key().to_hex()
        )
        .expect("scope"),
        cwd
    );
    for variant in 0..8 {
        let mut wrong = accepted.clone();
        match variant {
            0 => wrong.created_by = Some(Keys::generate().public_key().to_hex()),
            1 => wrong.channel_id = latest,
            2 => wrong.provider_instance_ref = "other-runtime".into(),
            3 => wrong.project_ref = Some("different-project".into()),
            4 => wrong.process_witness = None,
            5 => wrong.authority_withdrawn = Some("revoked".into()),
            6 => wrong.closed = false,
            _ => {
                wrong.retired = Some(crate::state::Retirement {
                    deletion_event_id: "aa".repeat(32),
                    receipt_event_id: None,
                    at: 1,
                })
            }
        }
        host.state.insert_session(wrong).expect("negative source");
        assert!(
            accepted_workspace(
                &host.config,
                &host.state,
                &actor_hex,
                channel,
                &record.provider_instance_ref,
                None,
                &owner.public_key().to_hex()
            )
            .is_err(),
            "variant {variant}"
        );
    }
    host.state
        .insert_session(accepted.clone())
        .expect("restore accepted");
    // A second accepted scope with another workspace is never selected arbitrarily.
    let mut second = accepted.clone();
    second.session_id = Uuid::new_v4().to_string();
    second.cwd = latest_cwd;
    let second_target = second.target(&host.config.instance_id);
    second.process_witness.as_mut().expect("witness").target = second_target.clone();
    host.state
        .insert_session(second.clone())
        .expect("second exact stopped scope");
    write(
        &host.config.state_dir.join("conversation-routes.json"),
        &json!({"routes":[
        {"actorPubkey":actor_hex,"channelId":channel,"target":target},
        {"actorPubkey":actor_hex,"channelId":channel,"target":second_target}]}),
    )
    .expect("accepted routes");
    assert!(accepted_workspace(
        &host.config,
        &host.state,
        &actor_hex,
        channel,
        &record.provider_instance_ref,
        None,
        &owner.public_key().to_hex()
    )
    .is_err());
    second.channel_id = latest;
    host.state
        .insert_session(second)
        .expect("exclude second scope");
    let mut unknown_target = target.clone();
    unknown_target.session_id = Uuid::new_v4().to_string();
    let seats_before = std::fs::read(&seats).expect("accepted staged custody");
    host.state
        .stage_terminal_disposition(
            "uncertain",
            crate::state::TerminalDisposition {
                semantic_key: "uncertain".into(),
                event: original,
                lifecycle_start: Some(crate::state::LifecycleStartIntent {
                    phase: crate::state::LifecycleStartPhase::RuntimeStarting,
                    fingerprint: "retained".into(),
                    target: unknown_target,
                    actor: Some(actor_hex.clone()),
                    channel,
                    provider_ref: record.provider_instance_ref.clone(),
                    runtime: record.runtime,
                    initial_turn: false,
                }),
            },
        )
        .expect("reserved uncertainty");
    assert!(accepted_workspace(
        &host.config,
        &host.state,
        &actor_hex,
        channel,
        &record.provider_instance_ref,
        None,
        &owner.public_key().to_hex()
    )
    .is_err());
    assert!(stage_with_context(
        &host.config,
        &host.state,
        Some(&relay.rest_client()),
        channel,
        &owner.public_key().to_hex(),
        &content,
        None
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read(&seats).expect("custody untouched by unknown startup"),
        seats_before
    );
    assert_eq!(
        std::fs::read(&enrollment_path).expect("immutable enrollment"),
        bytes
    );
    server.abort();
}

#[path = "managed_enrollment_additional_tests.rs"]
mod additional;
