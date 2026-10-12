use super::*;
use nostr::Keys;

#[tokio::test]
async fn managed_slot_without_protected_configuration_refuses_before_spawning_real_adapter() {
    let dir = tempfile::tempdir().expect("fixture");
    let cwd = dir.path().join("ordinary-work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    // Use the executable fake ACP adapter, not the unavailable sentinel: without
    // the missing-custody guard this would successfully launch a real child.
    provider.config.runtimes[0].instance_ref = "managed-actor".into();
    let original = crate::tests::create_event(&provider, channel, "managed-missing-custody");
    let mut content: serde_json::Value = serde_json::from_str(&original.content).expect("create");
    content["action"]["providerInstanceRef"] = serde_json::json!("managed-actor");
    let event = nostr::EventBuilder::new(
        original.kind,
        serde_json::to_string(&content).expect("body"),
    )
    .tags(original.tags.iter().cloned())
    .sign_with_keys(crate::tests::test_operator_keys())
    .expect("signed command");
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("named refusal");
    assert_eq!(provider.sessions.live_count(), 0);
    assert_eq!(provider.state.sessions().count(), 0);
    assert!(provider
        .outbox
        .pending_events()
        .filter_map(|event| payload::decode_coding_session_lifecycle_receipt(&event.content).ok())
        .any(|receipt| receipt.command_id == "managed-missing-custody"
            && receipt
                .error
                .as_ref()
                .is_some_and(|error| error.code == "AGENT_PRIVATE_CONFIG_UNAVAILABLE")));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn protected_generic_actor_launches_actual_acp_inside_existing_private_boundary() {
    generic_actor_fixture(false).await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn protected_catalog_mcp_reaches_actual_acp_and_runs_inside_same_actor_boundary() {
    generic_actor_fixture(true).await;
}

#[cfg(target_os = "macos")]
async fn generic_actor_fixture(with_mcp: bool) {
    let root = tempfile::tempdir().expect("fixture");
    let cwd = root.path().join("ordinary-work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(root.path(), channel, &cwd);
    let mut provider = crate::tests::provider(&root.path().join("state"), Some(&projects));
    let actor = Keys::generate();
    let owner = crate::tests::test_operator_keys();
    let auth = beekeeper_sdk::nip_oa::compute_auth_tag(owner, &actor.public_key(), "")
        .expect("attestation");
    let mut original_runtime = provider.config.runtimes[0].clone();
    let mcp_command = cwd.join("selected-mcp");
    let wire = cwd.join("private-acp-requests.jsonl");
    if with_mcp {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&mcp_command, "#!/bin/sh\nprintf 'ran' > mcp-executed\n")
            .expect("MCP fixture");
        std::fs::set_permissions(&mcp_command, std::fs::Permissions::from_mode(0o700))
            .expect("executable MCP");
        let script =
            std::fs::read_to_string(&original_runtime.agent_args[0]).expect("adapter script");
        let script = script.replace("while IFS= read -r line; do", "umask 077\nwhile IFS= read -r line; do\n  printf '%s\\n' \"$line\" >> private-acp-requests.jsonl").replace("*'\"method\":\"session/new\"'*)", "*'\"method\":\"session/new\"'*)\n      mcp=$(printf '%s' \"$line\" | sed -n 's/.*\"command\":\"\\([^\"]*\\)\".*/\\1/p')\n      \"$mcp\" || exit 91");
        let adapter = cwd.join("mcp-capable-adapter.sh");
        std::fs::write(&adapter, script).expect("MCP-capable client");
        original_runtime.agent_args = vec![adapter.to_string_lossy().into_owned()];
    }
    provider.config.runtimes[0].instance_ref = "managed-actor".into();
    provider.config.runtimes[0].driver = "managed-acp".into();
    provider.config.runtimes[0].runtime = "custom-runtime".into();
    let original = crate::tests::create_event(&provider, channel, "generic-protected-launch");
    let mut body: serde_json::Value = serde_json::from_str(&original.content).expect("create");
    body["action"]["providerInstanceRef"] = serde_json::json!("managed-actor");
    body["action"]["actor"] = serde_json::json!(actor.public_key().to_hex());
    let event =
        nostr::EventBuilder::new(original.kind, serde_json::to_string(&body).expect("body"))
            .tags(original.tags.iter().cloned())
            .sign_with_keys(owner)
            .expect("owner create");
    let actor_dir = provider
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex());
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&actor_dir).expect("custody dir");
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &actor_dir.join("auth-tag.json"),
        auth.as_bytes(),
    )
    .expect("actor attestation");
    let definition = nostr::EventBuilder::new(nostr::Kind::Custom(30177), r#"{"name":"Generic","systemPrompt":"Ordinary work","parallelism":1,"respondTo":"owner-only"}"#)
        .tags([nostr::Tag::parse(["d", &actor.public_key().to_hex()]).expect("d")])
        .sign_with_keys(owner)
        .expect("signed definition");
    let mut snapshot = serde_json::json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor.public_key().to_hex(),"relayUrl":provider.config.relay_url,"definition":definition,"record":{"runtime":"custom-runtime","effective_command":original_runtime.agent_command,"effective_args":original_runtime.agent_args,"effective_env":{"CUSTOM_ADAPTER_OPTION":"explicit"}}});
    if with_mcp {
        snapshot["record"]["effective_mcp_command"] = serde_json::json!(mcp_command);
    }
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &actor_dir.join("configuration.json"),
        &serde_json::to_vec(&snapshot).expect("snapshot"),
    )
    .expect("protected configuration");
    let seats = serde_json::json!({"version":1,"pending":{"generic-protected-launch":{"pubkey":actor.public_key().to_hex(),"nsec":actor.secret_key().to_secret_hex(),"authTag":auth,"relayUrl":provider.config.relay_url}}});
    let seats_path = provider
        .config
        .actor_seats_file
        .as_ref()
        .expect("seat path");
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        seats_path,
        &serde_json::to_vec(&seats).expect("seats"),
    )
    .expect("staged scoped seat");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("isolated HTTP");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/query",
                axum::routing::post(|| async { axum::Json(serde_json::json!([])) }),
            ),
        )
        .await
        .expect("fixture serve")
    });
    provider.rest_client = Some(beekeeper_acp::relay::RestClient {
        http: reqwest::Client::new(),
        base_url: format!("http://{address}"),
        keys: provider.config.keys.clone(),
        auth_tag_json: None,
    });
    provider.relay_self = Some(Keys::generate().public_key().to_hex());
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("generic lifecycle");
    let receipts: Vec<_> = provider
        .outbox
        .pending_events()
        .filter_map(|event| payload::decode_coding_session_lifecycle_receipt(&event.content).ok())
        .filter(|receipt| receipt.command_id == "generic-protected-launch")
        .collect();
    assert!(
        receipts
            .iter()
            .any(|receipt| receipt.status == payload::ReceiptStatus::Created),
        "actual generic launch refused: {receipts:?}"
    );
    if with_mcp {
        let received: serde_json::Value = std::fs::read_to_string(&wire)
            .expect("private ACP wire")
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|request| request["method"] == "session/new")
            .expect("actual session/new");
        let servers = received["params"]["mcpServers"]
            .as_array()
            .expect("MCP servers");
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0]["command"].as_str(),
            mcp_command.canonicalize().expect("installed MCP").to_str()
        );
        assert_eq!(servers[0]["args"], serde_json::json!([]));
        let environment: std::collections::BTreeMap<_, _> = servers[0]["env"]
            .as_array()
            .expect("scoped MCP environment")
            .iter()
            .map(|value| {
                (
                    value["name"].as_str().expect("name"),
                    value["value"].as_str().expect("value"),
                )
            })
            .collect();
        let key = Keys::parse(environment.get("BEEKEEPER_PRIVATE_KEY").expect("actor key"))
            .expect("valid scoped key");
        assert!(
            key.public_key() == actor.public_key(),
            "MCP must receive actor custody, never provider or owner"
        );
        assert!(environment.get("BEEKEEPER_AUTH_TAG").is_some_and(|tag| {
            beekeeper_sdk::nip_oa::verify_auth_tag(tag, &actor.public_key())
                .is_ok_and(|key| key == owner.public_key())
        }));
        assert!(!environment.contains_key("BEEKEEPER_CSP_PRIVATE_KEY"));
        assert!(!environment.contains_key("NOSTR_PRIVATE_KEY"));
        assert_eq!(
            std::fs::read_to_string(cwd.join("mcp-executed"))
                .expect("MCP descendant actually executed in boundary"),
            "ran"
        );
        std::fs::remove_file(&wire).expect("remove private wire fixture");
    }
    let record = provider.state.sessions().next().expect("accepted session");
    assert_eq!(record.runtime, "custom-runtime");
    assert_eq!(record.driver, "managed-acp");
    assert!(
        record.execution_binding.is_some(),
        "must use actual prepared Mac boundary, not test override"
    );
    let session = record.session_id.clone();
    assert!(provider
        .sessions
        .handle(&session)
        .expect("live actual child")
        .child_pid_for_tests()
        .is_some());
    assert_ne!(
        provider
            .sessions
            .shutdown_and_join(&session, session::ACTOR_RETIRE_GRACE)
            .await,
        session::ActorQuiescence::Unproven
    );
    server.abort();
}
