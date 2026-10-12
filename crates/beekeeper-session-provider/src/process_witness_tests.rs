use crate::*;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
#[tokio::test]
#[ignore = "isolated child process fixture, invoked only by the crash regression"]
async fn isolated_provider_child_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("BEEKEEPER_PROCESS_WITNESS_FIXTURE").expect("isolated invocation"),
    );
    let channel =
        Uuid::parse_str(&std::env::var("BEEKEEPER_PROCESS_WITNESS_CHANNEL").expect("channel"))
            .expect("uuid");
    let keys = nostr::Keys::parse(
        std::fs::read_to_string(root.join("fixture-key"))
            .expect("scoped fixture custody")
            .trim(),
    )
    .expect("fixture keys");
    let projects = root.join("projects.json");
    let template = crate::tests::provider(&root.join("state"), Some(&projects));
    let mut config = template.config.clone();
    drop(template);
    config.keys = keys;
    config.runtimes[0].agent_command = crate::session::testing::fake_agent(
        &root,
        "lingering-owned-agent",
        &crate::session::testing::lingering_agent(),
    );
    let mut provider = Provider::new(config).expect("isolated worker");
    let donor = crate::tests::create_event(&provider, channel, "owned-provider-crash");
    let operator = nostr::Keys::parse(
        std::fs::read_to_string(root.join("fixture-operator-key"))
            .expect("operator custody")
            .trim(),
    )
    .expect("fixture operator");
    let event = nostr::EventBuilder::new(donor.kind, donor.content)
        .tags(donor.tags.iter().cloned())
        .sign_with_keys(&operator)
        .expect("same operator command");
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("actual ACP launch");
    let target = provider
        .state
        .sessions()
        .next()
        .expect("record")
        .target(&provider.config.instance_id);
    let pid = provider
        .sessions
        .handle(&target.session_id)
        .expect("live actor")
        .child_group()
        .expect("captured group");
    // Exact predecessor crash window: the worker durably closes its record,
    // but has not staged a quiescence marker or stopped the actual child.
    let umbrella = Uuid::new_v4().to_string();
    let project = format!("30624:{}:closed-prefix", provider.config.keys.public_key());
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.closed = true;
            record.session_ref = Some(umbrella.clone());
            record.project_ref = Some(project.clone());
        })
        .expect("closed before marker");
    std::fs::write(
        root.join("ready.json"),
        serde_json::to_vec(
            &serde_json::json!({"target":target,"pid":pid,"umbrella":umbrella,"project":project}),
        )
        .expect("ready"),
    )
    .expect("ready write");
    std::future::pending::<()>().await;
}

#[cfg(unix)]
#[tokio::test]
async fn actual_worker_death_with_living_group_never_proves_stop_or_reopens_until_group_exit() {
    let dir = tempfile::tempdir().expect("fixture");
    let root = dir.path();
    let cwd = root.join("ordinary-work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(root, channel, &cwd);
    let template = crate::tests::provider(&root.join("state"), Some(&projects));
    let config = template.config.clone();
    drop(template);
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &root.join("fixture-key"),
        config.keys.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("private fixture custody");
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &root.join("fixture-operator-key"),
        crate::tests::test_operator_keys()
            .secret_key()
            .to_secret_hex()
            .as_bytes(),
    )
    .expect("private fixture operator custody");
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "process_witness::tests::isolated_provider_child_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("BEEKEEPER_PROCESS_WITNESS_FIXTURE", root)
        .env("BEEKEEPER_PROCESS_WITNESS_CHANNEL", channel.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("owned worker subprocess");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !root.join("ready.json").exists() {
        assert!(tokio::time::Instant::now() < deadline, "worker ready");
        assert!(
            child.try_wait().expect("child status").is_none(),
            "child fixture failed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let ready: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("ready.json")).expect("ready"))
            .expect("manifest");
    let target: beekeeper_core::coding_session_command::CodingSessionTarget =
        serde_json::from_value(ready["target"].clone()).expect("target");
    let pid = ready["pid"].as_u64().expect("captured fixture group") as u32;
    // Kill only the tracked test worker handle, not a PID read from durable state.
    child.kill().await.expect("abrupt owned worker death");
    child.wait().await.expect("worker exit");
    assert!(
        !crate::session::testing::group_is_gone(pid),
        "owned delayed grandchild survives worker death"
    );
    let mut provider = Provider::new(config.clone()).expect("restart");
    let record = provider
        .state
        .session(&target.session_id)
        .expect("durable record");
    assert_eq!(
        record
            .process_witness
            .as_ref()
            .expect("durable witness")
            .group_id,
        Some(pid)
    );
    let context = root.join("state/context-packages/retained");
    std::fs::create_dir_all(&context).expect("context");
    std::fs::write(context.join("0000000000.json"), b"{}").expect("private context");
    let mut git = crate::host_command::metadata_git_command(&cwd);
    let init = git
        .args(["init", "-q", "-b", "main"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("selected checkpoint fixture");
    assert!(init.status.success());
    let mut git = crate::host_command::metadata_git_command(&cwd);
    let committed = git
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("fixture commit");
    assert!(committed.status.success());
    let checkpoint = format!("refs/beekeeper/checkpoints/{}/1/base", target.session_id);
    let created = crate::host_command::metadata_git_command(&cwd)
        .args(["update-ref", &checkpoint, "HEAD"])
        .output()
        .expect("checkpoint ref");
    assert!(created.status.success());
    let umbrella = ready["umbrella"]
        .as_str()
        .expect("worker umbrella")
        .to_owned();
    let project = ready["project"]
        .as_str()
        .expect("worker project")
        .to_owned();
    assert!(
        provider
            .state
            .session(&target.session_id)
            .expect("record")
            .closed
    );
    let marker = root.join(format!("state/stop-quiescence-{}.json", target.session_id));
    assert!(!marker.exists());
    provider.recover().await.expect("actual recovery");
    assert!(context.exists());
    assert!(
        root.join("fixture-key").exists(),
        "custody retained before any stop marker"
    );
    assert!(crate::host_command::metadata_git_command(&cwd)
        .args(["show-ref", "--verify", &checkpoint])
        .output()
        .expect("checkpoint observation")
        .status
        .success());
    for _ in 0..2 {
        assert_eq!(provider.release_settled_umbrella(&umbrella).await, 0);
        assert!(
            marker.exists(),
            "settled reconciliation stages missing intent"
        );
        std::fs::remove_file(&marker).expect("reproduce predecessor missing marker");
        assert!(!provider.stop_project_executions(&project).await);
        assert!(marker.exists(), "cascade reconciles closed unproved record");
        std::fs::remove_file(&marker).expect("reproduce predecessor missing marker");
        assert!(provider
            .stop_for_withdrawn_authority(&target.session_id, "closed-prefix withdrawal")
            .await
            .is_err());
        assert!(
            marker.exists(),
            "withdrawal cannot skip an unproved witness"
        );
        assert!(context.exists());
        assert!(crate::host_command::metadata_git_command(&cwd)
            .args(["show-ref", "--verify", &checkpoint])
            .output()
            .expect("retained crash-prefix checkpoint")
            .status
            .success());
        std::fs::remove_file(&marker).expect("reproduce predecessor missing marker");
        provider
            .recover()
            .await
            .expect("repeat full crash-prefix recovery");
        assert!(context.exists());
    }
    assert!(matches!(
        provider
            .restore_generation(&target.session_id, None)
            .await
            .expect("restore outcome"),
        Err(native_restore::RestoreObstacle::ProviderUnavailable)
    ));
    assert_eq!(provider.sessions.live_count(), 0);
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        channel,
        "stop-recovered-live-group",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(channel, &stop)
        .await
        .expect("truthful pending");
    assert!(!provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|r| {
            r.command_id == "stop-recovered-live-group"
                && r.status == payload::ReceiptStatus::Stopped
        })
    }));
    assert!(context.exists());
    let umbrella = Uuid::new_v4().to_string();
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.session_ref = Some(umbrella.clone())
        })
        .expect("selected closure fixture");
    assert_eq!(
        provider.release_settled_umbrella(&umbrella).await,
        0,
        "closure cleanup cannot claim released child group"
    );
    assert!(context.exists());
    assert!(
        !provider.actor_has_unproved_stop(None),
        "actorless Solo contexts are not one shared identity"
    );
    let other =
        crate::tests::create_event(&provider, channel, "unrelated-solo-during-pending-stop");
    provider
        .handle_command_event(channel, &other)
        .await
        .expect("independent Solo admitted");
    let other_target = provider
        .state
        .sessions()
        .find(|record| record.command_id == "unrelated-solo-during-pending-stop")
        .expect("other accepted responsibility")
        .target(&provider.config.instance_id);
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "pending actorless context does not disable unrelated Solo work"
    );
    provider
        .sessions
        .shutdown_and_join(&other_target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;

    assert!(provider
        .stop_for_withdrawn_authority(&target.session_id, "fixture withdrawal")
        .await
        .is_err());
    assert!(context.exists());
    assert!(crate::host_command::metadata_git_command(&cwd)
        .args(["show-ref", "--verify", &checkpoint])
        .output()
        .expect("retained checkpoint observation")
        .status
        .success());
    // Test cleanup controls the freshly launched fixture group, never product
    // recovery. The production observation path performs no signal operation.
    nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .expect("owned fixture group cleanup");
    assert!(
        crate::session::testing::group_gone_within(pid, std::time::Duration::from_secs(5)).await
    );
    assert_eq!(
        provider.release_settled_umbrella(&umbrella).await,
        1,
        "matching group exit permits interrupted settled cleanup"
    );
    assert!(provider.recovered_group_absent(&target));
    assert!(
        provider.stop_project_executions(&project).await,
        "only proved exit permits project checkpoint retirement"
    );
    provider
        .handle_command_event(channel, &stop)
        .await
        .expect("same attempt retains immutable refusal");
    assert!(!provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|r| {
            r.command_id == "stop-recovered-live-group"
                && r.status == payload::ReceiptStatus::Stopped
        })
    }));
    // A new Stop delivery may establish terminal quiescence after the observed failure.
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        channel,
        "stop-after-proved-group-exit",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(channel, &stop)
        .await
        .expect("proved terminal");
    assert!(provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|r| {
            r.command_id == "stop-after-proved-group-exit"
                && r.status == payload::ReceiptStatus::Stopped
        })
    }));
    provider
        .recover()
        .await
        .expect("proved-exit recovery sweep");
    assert!(
        !context.exists(),
        "normal cleanup resumes after verified terminal quiescence"
    );
    assert!(
        root.join("fixture-key").exists(),
        "terminal Stop does not delete custody"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn proved_group_exit_allows_native_restore_and_interrupted_reopen_remains_fenced() {
    let dir = tempfile::tempdir().expect("fixture");
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    provider.config.runtimes[0].agent_command = session::testing::fake_agent(
        dir.path(),
        "native-resumable-agent",
        session::testing::RESUMABLE_AGENT,
    );
    let event = crate::tests::create_event(&provider, channel, "create-witness-native");
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("actual native-capable ACP");
    let target = provider
        .state
        .sessions()
        .next()
        .expect("record")
        .target(&provider.config.instance_id);
    provider
        .sessions
        .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
    assert!(provider.recovered_group_absent(&target));
    provider
        .restore_generation(&target.session_id, None)
        .await
        .expect("restore I/O")
        .expect("normal native restore");
    assert_eq!(provider.sessions.live_count(), 1);
    let witness = provider
        .state
        .session(&target.session_id)
        .expect("record")
        .process_witness
        .clone()
        .expect("new group fact");
    assert_eq!(witness.target, target);
    assert!(witness.group_id.is_some());
    provider
        .sessions
        .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
    let job = provider
        .prepare_restore(&target.session_id)
        .expect("staged reopen");
    drop(job);
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    assert!(
        matches!(provider.restore_generation(&target.session_id,None).await.expect("outcome"),Err(native_restore::RestoreObstacle::Prerequisite(failure)) if failure.code=="SESSION_QUIESCENCE_UNCONFIRMED")
    );
    assert_eq!(provider.sessions.live_count(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn restart_joins_delayed_grandchild_before_context_retirement_and_successor() {
    let dir = tempfile::tempdir().expect("fixture");
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    provider.config.runtimes[0].agent_command = session::testing::fake_agent(
        dir.path(),
        "delayed-restart-agent",
        &session::testing::lingering_agent(),
    );
    let create = crate::tests::create_event(&provider, channel, "create-delayed-restart");
    provider
        .handle_command_event(channel, &create)
        .await
        .expect("real ACP create");
    let target = provider
        .state
        .sessions()
        .next()
        .expect("record")
        .target(&provider.config.instance_id);
    let group = provider
        .sessions
        .handle(&target.session_id)
        .expect("live registry")
        .child_group()
        .expect("group");
    let package = Uuid::new_v4().to_string();
    let context = provider
        .config
        .state_dir
        .join("context-packages")
        .join(&package);
    std::fs::create_dir_all(&context).expect("context");
    std::fs::write(context.join("0000000000.json"), b"{}").expect("selected context");
    provider.context_refresh.insert(
        target.session_id.clone(),
        ContextRefreshState::opened(package),
    );
    let restart = crate::tests::lifecycle_target_event(
        &provider,
        channel,
        "restart-delayed-group",
        "session.restart",
        &target,
    );
    provider
        .handle_command_event(channel, &restart)
        .await
        .expect("restart");
    assert!(
        session::testing::group_is_gone(group),
        "successor never precedes old grandchild exit"
    );
    assert!(
        !context.exists(),
        "context retired only after verified group exit"
    );
    assert_eq!(
        provider
            .state
            .session(&target.session_id)
            .expect("successor")
            .generation,
        2
    );
    provider
        .sessions
        .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn durable_quiescence_survives_worker_restart_and_simulated_pgid_reuse_until_reopening() {
    let dir = tempfile::tempdir().expect("fixture");
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    let create = crate::tests::create_event(&provider, channel, "create-quiescence-fact");
    provider
        .handle_command_event(channel, &create)
        .await
        .expect("actual ACP create");
    let target = provider
        .state
        .sessions()
        .next()
        .expect("record")
        .target(&provider.config.instance_id);
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        channel,
        "stop-quiescence-fact",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(channel, &stop)
        .await
        .expect("verified stop");
    assert!(
        provider
            .state
            .session(&target.session_id)
            .expect("record")
            .process_witness
            .as_ref()
            .expect("witness")
            .quiesced
    );
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    // A controlled live group stands in for later OS reuse of the historical
    // PGID; the already-established generation proof must not signal or query it.
    let mut command = tokio::process::Command::new("/bin/sleep");
    command.arg("30").kill_on_drop(true);
    command.as_std_mut().process_group(0);
    let mut other = command.spawn().expect("owned unrelated group");
    let pid = other.id().expect("group");
    provider
        .state
        .update_session(&target.session_id, |record| {
            record
                .process_witness
                .as_mut()
                .expect("proved witness")
                .group_id = Some(pid)
        })
        .expect("simulated identifier reuse");
    assert!(!session::testing::group_is_gone(pid));
    assert!(
        provider.recovered_group_absent(&target),
        "durable exact-generation fact survives identifier reuse"
    );
    assert!(
        other.try_wait().expect("other status").is_none(),
        "product never signals recorded identifier"
    );
    provider
        .stage_reopening(&target.session_id, &target)
        .expect("new launch intent");
    assert!(
        !provider.recovered_group_absent(&target),
        "reopening replaces quiescence with uncertainty before any new child"
    );
    other.kill().await.expect("tracked fixture cleanup");
    other.wait().await.expect("fixture exit");
}
