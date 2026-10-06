use super::*;

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: config::DRIVER.into(),
        instance_id: "instance-1".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    }
}

#[tokio::test]
async fn accepted_metadata_survives_restart_and_pending_truth_wins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let keys = Keys::generate();
    let agent = fake_agent(dir.path(), "agent", GOOD_AGENT);
    let config = config_of(keys, &state_dir, None, agent);
    let channel = Uuid::new_v4();
    let target = target();
    let mut first = Provider::new(config.clone()).expect("provider");
    first
        .publish_metadata(channel, &target, SessionStatus::Disconnected)
        .expect("metadata");
    first.flush(&CollectingSink::new()).await.expect("accepted");
    drop(first);
    let mut restarted = Provider::new(config).expect("restart");
    restarted
        .publish_metadata(channel, &target, SessionStatus::Disconnected)
        .expect("unchanged");
    assert_eq!(restarted.pending_publishes(), 0);
    restarted
        .publish_metadata(channel, &target, SessionStatus::Running)
        .expect("running");
    restarted
        .publish_metadata(channel, &target, SessionStatus::Disconnected)
        .expect("replace running");
    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let events = sink.contents_of(KIND_CODING_SESSION_METADATA);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["status"], "disconnected");
    let mut next = target;
    next.generation += 1;
    restarted
        .publish_metadata(channel, &next, SessionStatus::Disconnected)
        .expect("new generation");
    assert_eq!(
        restarted.pending_publishes(),
        1,
        "new generation is a new fact"
    );
}

#[tokio::test]
async fn recovery_skips_left_channels_and_unchanged_accepted_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel, &cwd);
    let config = config_of(
        Keys::generate(),
        &state_dir,
        Some(&projects),
        fake_agent(dir.path(), "agent", GOOD_AGENT),
    );
    let mut first = Provider::new(config.clone()).expect("provider");
    first
        .handle_command_event(channel, &create_event(&first, channel, "create-recovery"))
        .await
        .expect("create");
    first.flush(&CollectingSink::new()).await.expect("flush");
    drop(first);
    let mut recovered = Provider::new(config.clone()).expect("restart");
    recovered.membership_known = true;
    recovered.subscribed.insert(channel);
    recovered.recover().await.expect("recover");
    let sink = CollectingSink::new();
    recovered.flush(&sink).await.expect("flush");
    assert_eq!(sink.contents_of(KIND_CODING_SESSION_METADATA).len(), 1);
    drop(recovered);
    let mut unchanged = Provider::new(config.clone()).expect("restart again");
    unchanged.membership_known = true;
    unchanged.subscribed.insert(channel);
    unchanged.recover().await.expect("recover");
    assert_eq!(unchanged.pending_publishes(), 0);
    let target = unchanged
        .state
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");
    unchanged
        .publish_metadata(channel, &target, SessionStatus::Running)
        .expect("queued before departure");
    drop(unchanged);
    let mut departed = Provider::new(config).expect("restart after departure");
    departed.membership_known = true;
    departed.recover().await.expect("recover");
    let sink = CollectingSink::new();
    departed.flush(&sink).await.expect("purge");
    assert!(sink.all().is_empty());
    assert_eq!(
        departed.state.sessions().count(),
        1,
        "membership loss preserves local history"
    );
}

#[tokio::test]
async fn newest_catalog_replaces_only_its_own_channel_and_receipt_passes_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let channel = Uuid::new_v4();
    let other = Uuid::new_v4();
    provider.subscribed.extend([channel, other]);
    provider.refresh_catalog(true).expect("v1");
    provider.config.runtimes[0]
        .allowed_models
        .push("new-model".into());
    provider.refresh_catalog(true).expect("v2");
    assert_eq!(
        provider.pending_publishes(),
        2,
        "one latest catalog per channel"
    );
    let target = target();
    provider
        .enqueue_receipt(
            channel,
            "fresh-create",
            &LifecycleReceipt::created("fresh-create", &target),
        )
        .expect("receipt");
    let sink = CollectingSink::new();
    provider.flush_one(&sink).await.expect("one send");
    assert_eq!(
        u32::from(sink.all()[0].kind.as_u16()),
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT
    );
    provider.flush(&sink).await.expect("catalogs");
    let catalogs = catalog_events(&sink);
    assert_eq!(catalogs.len(), 2);
    for event in catalogs {
        let body: serde_json::Value = serde_json::from_str(&event.content).expect("json");
        assert_eq!(body["revision"], 2);
        let channel = tag_value(&event, "h").expect("h");
        assert_eq!(
            tag_value(&event, "cspc-key"),
            Some(coding_session_provider_catalog_semantic_key(
                &channel,
                2,
                &event.content
            ))
        );
    }
}

#[test]
fn catalog_digest_detects_discovery_fallback_and_adapter_rank_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let projects = ProjectsFile::load(None);
    let fallback = catalog::body_digest(&catalog::build(&provider.config, &projects, 1));
    let runtime = provider.config.runtimes[0].instance_ref.clone();
    let model = provider.config.runtimes[0].default_model.clone();
    provider.config.runtimes[0]
        .allowed_models
        .push("discovered-model".into());
    provider
        .config
        .model_details
        .entry(runtime.clone())
        .or_default()
        .insert(
            model.clone(),
            config::ModelDetail {
                rank: Some(0),
                ..Default::default()
            },
        );
    let discovered = catalog::body_digest(&catalog::build(&provider.config, &projects, 1));
    assert_ne!(
        fallback, discovered,
        "timeout fallback changes the advertised offer"
    );
    provider
        .config
        .model_details
        .get_mut(&runtime)
        .expect("details")
        .get_mut(&model)
        .expect("model")
        .rank = Some(1);
    let reordered = catalog::body_digest(&catalog::build(&provider.config, &projects, 1));
    assert_ne!(
        discovered, reordered,
        "adapter rank is signed catalog content"
    );
    assert_eq!(
        reordered,
        catalog::body_digest(&catalog::build(&provider.config, &projects, 99)),
        "revision is excluded from digest"
    );
}

#[tokio::test]
async fn leaving_before_catalog_ack_and_rejoining_requeues_current_catalog() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let channel = Uuid::new_v4();
    provider.membership_known = true;
    provider.subscribed.insert(channel);
    provider.refresh_catalog(true).expect("catalog");
    assert_eq!(provider.pending_publishes(), 1);
    provider.subscribed.remove(&channel);
    provider.purge_outbox_for_left_channels().expect("leave");
    assert_eq!(provider.pending_publishes(), 0);
    provider.subscribed.insert(channel);
    provider.refresh_catalog(false).expect("rejoin");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(catalog_events(&sink).len(), 1);
    assert_eq!(provider.state.catalog().revision, 1);
}
