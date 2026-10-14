//! Real rejected ACP startup, process-group observation and durable Stop retry.
use super::*;
use crate::session::testing::{fake_agent, load_rejecting_agent};
use std::process::Stdio;
use tokio::process::Command;

async fn fixture(root: &std::path::Path) -> (crate::Provider, SessionRecord) {
    let cwd = root.join("work");
    std::fs::create_dir(&cwd).unwrap();
    let channel = uuid::Uuid::new_v4();
    let projects = crate::tests::write_projects(root, channel, &cwd);
    let mut provider = crate::tests::provider(&root.join("state"), Some(&projects));
    let create = crate::tests::create_event(&provider, channel, "original-native");
    provider
        .handle_command_event(channel, &create)
        .await
        .unwrap();
    let record = provider.state.sessions().next().unwrap().clone();
    let target = record.target(&provider.config.instance_id);
    assert_ne!(
        provider
            .sessions
            .shutdown_and_join(&record.session_id, session::ACTOR_RETIRE_GRACE)
            .await,
        session::ActorQuiescence::Unproven
    );
    provider.record_quiesced(&target).unwrap();
    let record = provider.state.session(&record.session_id).unwrap().clone();
    (provider, record)
}
fn stopped(provider: &crate::Provider, command: &str) -> bool {
    provider.outbox.pending_events().any(|event| {
        crate::payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(
            |receipt| {
                receipt.command_id == command
                    && receipt.status == crate::payload::ReceiptStatus::Stopped
            },
        )
    })
}
#[tokio::test]
async fn rejected_native_restore_records_actual_quiescence_and_allows_stop_without_fresh_fallback()
{
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let log = root.path().join("work/methods");
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "reject",
        &load_rejecting_agent(log.to_str().unwrap()),
    );
    let refused = provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code(), NATIVE_RESTORE_REJECTED);
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation);
    assert_eq!(current.resume_cursor, prior.resume_cursor);
    let witness = current.process_witness.as_ref().unwrap();
    assert!(witness.group_id.is_some());
    assert!(witness.quiesced);
    assert!(witness.attempted_target.is_none());
    assert!(provider.recovered_group_absent(&prior.target(&provider.config.instance_id)));
    let methods = std::fs::read_to_string(log).unwrap();
    assert!(methods.contains("session/load"));
    assert!(!methods.contains("session/new"));
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "stop-rejected",
        "session.stop",
        &prior.target(&provider.config.instance_id),
    );
    provider
        .handle_command_event(prior.channel_id, &stop)
        .await
        .unwrap();
    assert!(stopped(&provider, "stop-rejected"));
    // A distinct, explicit Create chooses fresh context only after proved Stop;
    // the refused generation and its native cursor remain durable history.
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "explicit-fresh",
        crate::session::testing::GOOD_AGENT,
    );
    let fresh = crate::tests::create_event(&provider, prior.channel_id, "explicit-fresh-context");
    provider
        .handle_command_event(prior.channel_id, &fresh)
        .await
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert!(current.closed);
    assert_eq!(current.resume_cursor, prior.resume_cursor);
    assert!(provider
        .state
        .sessions()
        .any(|record| record.session_id != prior.session_id && !record.closed));
}
#[tokio::test]
async fn rejected_native_restore_with_unreaped_group_survives_recovery_and_stop_waits_for_positive_exit(
) {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let group_file = root.path().join("work/group");
    // Reproduce the observable file-created/write-not-complete startup cut.
    std::fs::write(&group_file, b"").unwrap();
    let gate = root.path().join("work/gate");
    let log = root.path().join("work/methods");
    let script = format!(
        "printf '%s' \"$$\" > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.01; done\n{}",
        group_file.display(),
        gate.display(),
        load_rejecting_agent(log.to_str().unwrap())
    );
    provider.config.runtimes[0].agent_command = fake_agent(root.path(), "delayed-reject", &script);
    let target = prior.target(&provider.config.instance_id);
    let job = provider.prepare_restore(&prior.session_id).unwrap();
    let restore = run_restore_job(job);
    tokio::pin!(restore);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let group = loop {
        // Creation precedes printf's write. Wait for the actual positive PID,
        // not merely the directory entry, before joining that process group.
        if let Some(group) = std::fs::read_to_string(&group_file)
            .ok()
            .and_then(|raw| raw.trim().parse::<i32>().ok())
            .filter(|group| *group > 0)
        {
            break group;
        }
        tokio::select! {
            _ = &mut restore => panic!("restore completed before gate"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        assert!(tokio::time::Instant::now() < deadline);
    };
    // This independently parented group member is killed by ACP shutdown, but
    // its exact zombie remains observable until this test joins it. Group
    // absence therefore cannot be inferred from the direct adapter's join.
    let mut delayed = Command::new("sleep")
        .arg("600")
        .process_group(group)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "stop-delayed-restore",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &stop)
        .await
        .unwrap();
    assert!(!stopped(&provider, "stop-delayed-restore"));
    assert!(provider.state.session(&prior.session_id).unwrap().closed);
    std::fs::write(&gate, b"continue").unwrap();
    let run = restore.await;
    assert_eq!(
        provider
            .finish_restore(run)
            .await
            .unwrap()
            .unwrap_err()
            .code(),
        NATIVE_RESTORE_REJECTED
    );
    let witness = provider
        .state
        .session(&prior.session_id)
        .unwrap()
        .process_witness
        .as_ref()
        .unwrap();
    assert_eq!(witness.target, target);
    assert_eq!(witness.group_id, Some(u32::try_from(group).unwrap()));
    assert!(!witness.quiesced);
    assert_eq!(
        provider
            .state
            .session(&prior.session_id)
            .unwrap()
            .resume_cursor,
        prior.resume_cursor
    );
    let config = provider.config.clone();
    let retained_package = provider
        .config
        .state_dir
        .join("context-packages")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&retained_package).unwrap();
    let protected_history = retained_package.join("history.json");
    std::fs::write(&protected_history, b"retained private recovery context").unwrap();
    drop(provider);
    let mut recovered = crate::Provider::new(config).unwrap();
    recovered.recover().await.unwrap();
    assert!(
        protected_history.exists(),
        "pending quiescence retains physical context cache"
    );
    assert!(!recovered.recovered_group_absent(&target));
    recovered
        .handle_command_event(prior.channel_id, &stop)
        .await
        .unwrap();
    assert!(!stopped(&recovered, "stop-delayed-restore"));
    delayed.wait().await.unwrap();
    assert!(recovered.recovered_group_absent(&target));
    recovered
        .handle_command_event(prior.channel_id, &stop)
        .await
        .unwrap();
    assert!(
        !stopped(&recovered, "stop-delayed-restore"),
        "refused wire operation never re-executes"
    );
    let retry = crate::tests::lifecycle_target_event(
        &recovered,
        prior.channel_id,
        "stop-proved-exit",
        "session.stop",
        &target,
    );
    recovered
        .handle_command_event(prior.channel_id, &retry)
        .await
        .unwrap();
    assert!(stopped(&recovered, "stop-proved-exit"));
    assert_eq!(
        recovered
            .state
            .session(&prior.session_id)
            .unwrap()
            .resume_cursor,
        prior.resume_cursor
    );
    assert!(!std::fs::read_to_string(log)
        .unwrap()
        .contains("session/new"));
}

#[tokio::test]
async fn failed_resume_retains_uncommitted_attempt_target_and_proved_cleanup_without_generation_or_cursor_change(
) {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let log = root.path().join("work/initialize");
    let script = format!(
        r#"read -r line
printf '%s' "$line" > '{}'
id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
printf '{{"jsonrpc":"2.0","id":%s,"error":{{"code":-32000,"message":"initialize refused"}}}}\n' "$id"
exec sleep 600
"#,
        log.display()
    );
    provider.config.runtimes[0].agent_command = fake_agent(root.path(), "init-refuses", &script);
    let target = prior.target(&provider.config.instance_id);
    let resume = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "failed-resume-attempt",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &resume)
        .await
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation);
    assert_eq!(current.resume_cursor, prior.resume_cursor);
    let witness = current.process_witness.as_ref().unwrap();
    assert_eq!(witness.target, target);
    let attempt = witness
        .attempted_target
        .as_ref()
        .expect("failed successor provenance");
    assert_eq!(attempt.generation, target.generation + 1);
    assert_eq!(attempt.session_id, target.session_id);
    assert!(witness.quiesced);
    assert!(witness.group_id.is_some());
    assert!(witness.proves_absent(&target));
    let mut wrong = witness.clone();
    wrong.attempted_target.as_mut().unwrap().generation += 1;
    assert!(!wrong.proves_absent(&target));
    assert!(!std::fs::read_to_string(log)
        .unwrap()
        .contains("session/new"));
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "stop-failed-successor",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &stop)
        .await
        .unwrap();
    assert!(stopped(&provider, "stop-failed-successor"));
}

#[tokio::test]
async fn failed_startup_proof_cannot_replace_changed_cursor_binding_or_newer_witness() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let target = prior.target(&provider.config.instance_id);
    let proof = crate::process_witness::ExecutionProcessWitness {
        target: target.clone(),
        attempted_target: None,
        group_id: None,
        quiesced: true,
    };
    provider
        .stage_reopening(&prior.session_id, &target)
        .unwrap();
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.resume_cursor = Some("changed-cursor".into())
        })
        .unwrap();
    assert!(!provider.record_failed_startup(&prior, &proof).unwrap());
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.resume_cursor = prior.resume_cursor.clone()
        })
        .unwrap();
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.generation_command_id = Some("another-attempt".into())
        })
        .unwrap();
    assert!(!provider.record_failed_startup(&prior, &proof).unwrap());
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.generation_command_id = prior.generation_command_id.clone()
        })
        .unwrap();
    // Only a platform with an execution backend binds a scope; a Legacy plan
    // stores none (`execution_scope.rs` `prepare`, `session_create.rs`). A
    // differing binding is built either way so the comparison is exercised.
    assert_eq!(prior.execution_binding.is_some(), cfg!(target_os = "macos"));
    let mut changed =
        prior
            .execution_binding
            .clone()
            .unwrap_or(crate::execution_scope::ExecutionBinding {
                scope_digest: String::new(),
                project_ref: None,
                actor: None,
                tree: None,
                agents: None,
                branch: None,
                agents_writable: false,
            });
    changed.scope_digest = "changed-scope".into();
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.execution_binding = Some(changed.clone());
        })
        .unwrap();
    assert!(!provider.record_failed_startup(&prior, &proof).unwrap());
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.execution_binding = prior.execution_binding.clone()
        })
        .unwrap();
    let mut newer = proof.clone();
    newer.group_id = Some(u32::MAX);
    newer.quiesced = false;
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.process_witness = Some(newer.clone())
        })
        .unwrap();
    assert!(!provider.record_failed_startup(&prior, &proof).unwrap());
    assert_eq!(
        provider
            .state
            .session(&prior.session_id)
            .unwrap()
            .process_witness
            .as_ref(),
        Some(&newer)
    );
    provider
        .stage_reopening(&prior.session_id, &target)
        .unwrap();
    provider
        .state
        .update_session(&prior.session_id, |record| record.generation += 1)
        .unwrap();
    assert!(!provider.record_failed_startup(&prior, &proof).unwrap());
}

/// The pre-spawn working-directory refusal exists only where an execution
/// backend exists: off macOS `prepare` returns `ExecutionPlan::Legacy` before
/// any scope validation (`execution_scope.rs:645`), by design, so a renamed
/// cwd is only observed at spawn there and never as `Unplanned`.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn refused_before_spawn_records_proven_absence() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let job = provider.prepare_restore(&prior.session_id).unwrap();
    let saved = root.path().join("retained-work");
    std::fs::rename(&prior.cwd, &saved).unwrap();
    let run = run_restore_job(job).await;
    assert!(matches!(run.outcome, RestoreOutcome::Unplanned(_)));
    std::fs::rename(saved, &prior.cwd).unwrap();
    provider.finish_restore(run).await.unwrap().unwrap_err();
    assert!(provider.recovered_group_absent(&prior.target(&provider.config.instance_id)));
}

#[tokio::test]
async fn cancelled_restore_does_not_record_absence() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let spawned = root.path().join("work/cancelled-group");
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "cancelled-start",
        &format!(
            "printf '%s' \"$$\" > '{}'\nexec sleep 600\n",
            spawned.display()
        ),
    );
    let job = provider.prepare_restore(&prior.session_id).unwrap();
    let meta = job.meta();
    let mut running = Box::pin(run_restore_job(job));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !spawned.exists() {
        tokio::select! {
            _ = &mut running => panic!("startup unexpectedly completed"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        assert!(tokio::time::Instant::now() < deadline);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut running)
            .await
            .is_err()
    );
    drop(running);
    // An off-loop deadline cannot assert a child was never started merely
    // because it has no returned observation. The durable staged fence remains.
    provider
        .finish_restore(RestoreRun::timed_out(meta, Duration::from_secs(1)))
        .await
        .unwrap()
        .unwrap_err();
    assert!(!provider.recovered_group_absent(&prior.target(&provider.config.instance_id)));
    let record = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(record.resume_cursor, prior.resume_cursor);
    assert_eq!(record.generation, prior.generation);
    assert_eq!(record.process_witness.as_ref().unwrap().group_id, None);
}

#[tokio::test]
async fn opened_restore_racing_closed_record_discards_then_retains_proved_group() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let group_file = root.path().join("work/opening-group");
    let gate = root.path().join("work/opening-gate");
    let log = root.path().join("work/opening-methods");
    let opens = load_rejecting_agent(log.to_str().unwrap()).replace(
        "\"error\":{\"code\":-32000,\"message\":\"load rejected\"}",
        "\"result\":{\"sessionId\":\"acp-session-1\"}",
    );
    assert!(!opens.contains("load rejected"));
    let script = format!(
        "printf '%s' \"$$\" > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.01; done\n{}",
        group_file.display(),
        gate.display(),
        opens
    );
    provider.config.runtimes[0].agent_command = fake_agent(root.path(), "delayed-open", &script);
    let target = prior.target(&provider.config.instance_id);
    let job = provider.prepare_restore(&prior.session_id).unwrap();
    let mut running = Box::pin(run_restore_job(job));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !group_file.exists() {
        tokio::select! { _ = &mut running => panic!("restore ended before gate"), _ = tokio::time::sleep(Duration::from_millis(10)) => {} }
        assert!(tokio::time::Instant::now() < deadline);
    }
    let stop = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "stop-before-open",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &stop)
        .await
        .unwrap();
    assert!(!stopped(&provider, "stop-before-open"));
    std::fs::write(gate, b"continue").unwrap();
    let run = running.await;
    assert!(matches!(&run.outcome, RestoreOutcome::Started {started, ..} if started.is_ok()));
    provider.finish_restore(run).await.unwrap().unwrap_err();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert!(current.closed);
    assert_eq!(current.resume_cursor, prior.resume_cursor);
    assert!(current.process_witness.as_ref().unwrap().group_id.is_some());
    assert!(provider.recovered_group_absent(&target));
    assert!(provider.sessions.handle(&prior.session_id).is_none());
    let retry = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "stop-after-discard",
        "session.stop",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &retry)
        .await
        .unwrap();
    assert!(stopped(&provider, "stop-after-discard"));
}

#[tokio::test]
async fn absent_failed_adapter_observation_never_becomes_no_spawn_proof() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let log = root.path().join("work/missing-evidence-methods");
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "reject-without-returned-observation",
        &load_rejecting_agent(log.to_str().unwrap()),
    );
    let job = provider.prepare_restore(&prior.session_id).unwrap();
    let mut run = run_restore_job(job).await;
    assert!(matches!(&run.outcome, RestoreOutcome::Started {started, ..} if started.is_err()));
    run.startup_witness = None;
    provider.finish_restore(run).await.unwrap().unwrap_err();
    assert!(!provider.recovered_group_absent(&prior.target(&provider.config.instance_id)));
    assert_eq!(
        provider
            .state
            .session(&prior.session_id)
            .unwrap()
            .resume_cursor,
        prior.resume_cursor
    );
}

#[tokio::test]
async fn successful_open_cannot_attach_over_same_generation_changed_cursor() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = fixture(root.path()).await;
    let log = root.path().join("work/stale-open-methods");
    let opens = load_rejecting_agent(log.to_str().unwrap()).replace(
        "\"error\":{\"code\":-32000,\"message\":\"load rejected\"}",
        "\"result\":{\"sessionId\":\"acp-session-1\"}",
    );
    provider.config.runtimes[0].agent_command = fake_agent(root.path(), "stale-open", &opens);
    let job = provider.prepare_restore(&prior.session_id).unwrap();
    let run = run_restore_job(job).await;
    assert!(matches!(&run.outcome, RestoreOutcome::Started {started, ..} if started.is_ok()));
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.resume_cursor = Some("later-cursor".into())
        })
        .unwrap();
    provider.finish_restore(run).await.unwrap().unwrap_err();
    assert!(provider.sessions.handle(&prior.session_id).is_none());
    assert_eq!(
        provider
            .state
            .session(&prior.session_id)
            .unwrap()
            .resume_cursor
            .as_deref(),
        Some("later-cursor")
    );
    assert!(
        !provider.recovered_group_absent(&prior.target(&provider.config.instance_id)),
        "stale cleanup cannot overwrite newer record fence"
    );
}
