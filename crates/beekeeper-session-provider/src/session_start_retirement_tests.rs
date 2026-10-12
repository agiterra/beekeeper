use super::*;

fn package(provider: &crate::Provider, id: &str) -> std::path::PathBuf {
    let path = provider.config.state_dir.join("context-packages").join(id);
    std::fs::create_dir_all(&path).expect("package");
    std::fs::write(path.join("1.json"), b"protected context fixture").expect("context");
    path
}

#[cfg(unix)]
#[tokio::test]
async fn unattached_actor_retirement_joins_delayed_grandchild_before_context_cleanup() {
    let dir = tempfile::tempdir().expect("fixture");
    let provider = crate::tests::provider(&dir.path().join("state"), None);
    let id = uuid::Uuid::new_v4().to_string();
    let path = package(&provider, &id);
    let marker = dir.path().join("grandchild-writes");
    let mut request = tests::request(
        format!(
            "(while true; do printf x >> '{}'; sleep 0.02; done) &\n{}",
            marker.display(),
            testing::GOOD_AGENT
        ),
        dir.path(),
    );
    request.agent_args.insert(0, "-c".into());
    let (events, _receiver) = tokio::sync::mpsc::channel(100);
    let started = SessionManager::start(request, events)
        .await
        .expect("actual startup");
    let pid = started.handle.child_group().expect("captured group");
    for _ in 0..100 {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        marker.exists(),
        "grandchild actually writes before retirement"
    );
    assert!(path.exists());
    let proof = provider
        .retire_unattached_context(Some(started), Some(&id))
        .await;
    assert_ne!(proof, ActorQuiescence::Unproven);
    assert!(
        testing::group_is_gone(pid),
        "whole group is gone before package removal"
    );
    assert!(!path.exists());
    let before = std::fs::read(&marker).expect("writes");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(std::fs::read(marker).expect("no later writes"), before);
}

#[tokio::test]
async fn actual_initialize_failure_retains_context_without_exit_witness_through_recovery() {
    let dir = tempfile::tempdir().expect("fixture");
    let mut provider = crate::tests::provider(&dir.path().join("state"), None);
    let id = uuid::Uuid::new_v4().to_string();
    let path = package(&provider, &id);
    let mut request = tests::request(
        "sleep 600 </dev/null >/dev/null 2>&1 &\nread -r request\nexit 1\n".into(),
        dir.path(),
    );
    let target = request.target.clone();
    let channel = request.channel_id;
    request.agent_args.insert(0, "-c".into());
    let (events, _receiver) = tokio::sync::mpsc::channel(100);
    let failure = match SessionManager::start(request, events).await {
        Ok(started) => {
            started.discard().await;
            panic!("fixture must fail initialize");
        }
        Err(failure) => failure,
    };
    assert!(failure.message.contains("initialize"));
    // Persist the same existing RuntimeStarting uncertainty used by create.
    let command = "actual-failed-start-retention";
    let mut receipt = crate::payload::LifecycleReceipt::failed(
        command,
        crate::payload::CREATE_START_OUTCOME_UNCONFIRMED,
        &failure.message,
    );
    receipt.session = Some(target.clone());
    let event = provider
        .lifecycle_event(channel, &receipt)
        .expect("receipt");
    provider
        .state
        .stage_terminal_disposition(
            command,
            crate::state::TerminalDisposition {
                semantic_key: crate::coding_session_lifecycle_receipt_semantic_key(command),
                event,
                lifecycle_start: Some(crate::state::LifecycleStartIntent {
                    phase: crate::state::LifecycleStartPhase::RuntimeStarting,
                    fingerprint: "actual startup fixture".into(),
                    target: target.clone(),
                    actor: None,
                    channel,
                    provider_ref: "claude-primary".into(),
                    runtime: "claude".into(),
                    initial_turn: false,
                }),
            },
        )
        .expect("retained intent");
    assert_eq!(
        provider.retire_unattached_context(None, Some(&id)).await,
        ActorQuiescence::Unproven
    );
    assert!(
        path.exists(),
        "absence of a returned live handle is not exit proof"
    );
    let config = provider.config.clone();
    drop(provider);
    let mut recovered = crate::Provider::new(config).expect("restart");
    recovered.recover().await.expect("full recovery");
    assert!(
        path.exists(),
        "unresolved startup retains package through recovery"
    );
    assert!(recovered.state.unresolved_start_target(&target));
    assert_eq!(recovered.state.sessions().count(), 0);
    assert_eq!(recovered.sessions.live_count(), 0);
}
