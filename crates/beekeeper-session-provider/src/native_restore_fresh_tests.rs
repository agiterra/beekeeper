//! An unprompted generation has no adapter transcript to reattach to: its
//! strict restore opens `session/new` in the same generation and says Fresh,
//! while a generation that ever had a prompt still refuses.
use super::*;
use crate::session::testing::{fake_agent, load_rejecting_agent};

async fn unprompted(root: &std::path::Path) -> (crate::Provider, SessionRecord) {
    let cwd = root.join("work");
    std::fs::create_dir(&cwd).unwrap();
    let channel = uuid::Uuid::new_v4();
    let projects = crate::tests::write_projects(root, channel, &cwd);
    let mut provider = crate::tests::provider(&root.join("state"), Some(&projects));
    let create = crate::tests::create_event(&provider, channel, "unprompted");
    provider
        .handle_command_event(channel, &create)
        .await
        .unwrap();
    let record = provider.state.sessions().next().unwrap().clone();
    assert!(!record.native_history_started, "a create with no turn");
    let target = record.target(&provider.config.instance_id);
    provider
        .sessions
        .shutdown_and_join(&record.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
    provider.record_quiesced(&target).unwrap();
    let log = root.join("work/methods");
    provider.config.runtimes[0].agent_command =
        fake_agent(root, "reject", &load_rejecting_agent(log.to_str().unwrap()));
    let record = provider.state.session(&record.session_id).unwrap().clone();
    (provider, record)
}

fn methods(root: &std::path::Path) -> String {
    std::fs::read_to_string(root.join("work/methods")).unwrap_or_default()
}

/// Acceptance 1 and R4: fresh in the same generation, new cursor, said so.
#[tokio::test]
async fn unprompted_generation_reopens_fresh_in_the_same_generation() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    let prior_fresh = provider
        .outbox
        .pending_events()
        .filter(|event| event.content.contains("session_fresh"))
        .count();
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation);
    assert_eq!(
        current.resume_cursor.as_deref(),
        Some("a-brand-new-conversation")
    );
    assert!(!current.native_history_started);
    assert!(methods(root.path()).contains("session/new"));
    let said = |status: &str| {
        provider
            .outbox
            .pending_events()
            .filter(|event| event.content.contains(status))
            .count()
    };
    // The create's own row, then one for this open: `prior_fresh` is read before it.
    assert_eq!(said("session_fresh"), prior_fresh + 1);
    assert_eq!(said(SESSION_RESTORED_NATIVE), 0);
    // Acceptance 6 / R5: a replay finds the generation live and opens nothing.
    let again = provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(again.code(), RestoreObstacle::AlreadyLive.code());
    assert_eq!(methods(root.path()).matches("session/new").count(), 1);
}

/// Acceptance 2 and R1.
#[tokio::test]
async fn prompted_generation_still_refuses_and_never_calls_session_new() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .mark_native_history_started(&prior.session_id)
        .unwrap();
    let refused = provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code(), NATIVE_RESTORE_REJECTED);
    assert!(!methods(root.path()).contains("session/new"));
    assert_eq!(
        provider
            .state
            .session(&prior.session_id)
            .unwrap()
            .resume_cursor,
        prior.resume_cursor
    );
}

/// Acceptance 3 and R2: the fact is written before the prompt is delivered,
/// so a crash after recording and before sending refuses, never goes fresh.
#[tokio::test]
async fn crash_after_recording_before_prompt_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .mark_native_history_started(&prior.session_id)
        .unwrap();
    // The provider restarts: the fact is read back from disk.
    let state_dir = root.path().join("state");
    drop(provider);
    let mut restarted = crate::tests::provider(&state_dir, None);
    restarted.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "reject2",
        &load_rejecting_agent(root.path().join("work/methods").to_str().unwrap()),
    );
    assert!(
        restarted
            .state
            .session(&prior.session_id)
            .unwrap()
            .native_history_started
    );
    let refused = restarted
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code(), NATIVE_RESTORE_REJECTED);
    assert!(!methods(root.path()).contains("session/new"));
}

/// The new conversation becomes the one R1 protects: once prompted, the next
/// restore reattaches the replacement cursor and says native.
#[tokio::test]
async fn a_fresh_open_that_is_later_prompted_restores_its_new_cursor_natively() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    provider
        .mark_native_history_started(&prior.session_id)
        .unwrap();
    let target = prior.target(&provider.config.instance_id);
    provider
        .sessions
        .shutdown_and_join(&prior.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
    provider.record_quiesced(&target).unwrap();
    let log = root.path().join("work/second-methods");
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "restore",
        &crate::session::testing::restorable_agent(log.to_str().unwrap()),
    );
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    let methods = std::fs::read_to_string(&log).unwrap();
    assert!(methods.contains("session/load"), "{methods}");
    assert!(!methods.contains("session/new"), "{methods}");
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(
        current.resume_cursor.as_deref(),
        Some("a-brand-new-conversation")
    );
    assert!(provider
        .outbox
        .pending_events()
        .any(|event| event.content.contains(SESSION_RESTORED_NATIVE)));
}

/// An adapter-started turn is native history nobody prompted, so it counts.
#[tokio::test]
async fn an_autonomous_turn_marks_the_generation_as_having_history() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .handle_session_event(session::SessionEvent::AutonomousTurnStarted {
            session_id: prior.session_id.clone(),
            turn_id: "autonomous-1".into(),
        })
        .unwrap();
    assert!(
        provider
            .state
            .session(&prior.session_id)
            .unwrap()
            .native_history_started
    );
}

/// The same forever-refusal by another road: a prompted generation whose
/// `session.resume` fell back to a new conversation holds nothing to restore
/// until it is prompted again, so it must not keep the old fact.
#[tokio::test]
async fn a_resume_that_falls_back_to_session_new_clears_the_fact() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .mark_native_history_started(&prior.session_id)
        .unwrap();
    let target = prior.target(&provider.config.instance_id);
    let resume = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "resume-after-history",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &resume)
        .await
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation + 1);
    assert_eq!(
        current.resume_cursor.as_deref(),
        Some("a-brand-new-conversation")
    );
    assert!(!current.native_history_started);
}

/// A resume that really reattaches keeps the fact: the history is still there.
#[tokio::test]
async fn a_resume_that_reattaches_keeps_the_fact() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .mark_native_history_started(&prior.session_id)
        .unwrap();
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "reattach",
        &crate::session::testing::restorable_agent(root.path().join("work/kept").to_str().unwrap()),
    );
    let target = prior.target(&provider.config.instance_id);
    let resume = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "resume-keeps-history",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &resume)
        .await
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation + 1);
    assert!(current.native_history_started);
}

/// A runtime that keeps its own context is never published as fresh; the row
/// that says it may remember more is the honest one.
#[tokio::test]
async fn a_runtime_keeping_its_own_context_is_not_called_fresh_on_restore() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider.config.runtimes[0].context =
        Some(beekeeper_core::coding_session_runtime::RuntimeContext::Runtime);
    let rows = |provider: &crate::Provider, status: &str| {
        provider
            .outbox
            .pending_events()
            .filter(|event| event.content.contains(status))
            .count()
    };
    let before = rows(&provider, "\"session_fresh\"");
    let held_before = rows(&provider, crate::execution_scope::STATUS_RUNTIME_CONTEXT);
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rows(&provider, "\"session_fresh\""), before);
    assert_eq!(
        rows(&provider, crate::execution_scope::STATUS_RUNTIME_CONTEXT),
        held_before + 1
    );
}

/// R4 as emitted: a fresh restore queues continuity statuses only, no receipt,
/// and never says it reattached or leaks the new cursor.
#[tokio::test]
async fn a_fresh_restore_queues_only_what_it_can_truthfully_say() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    let seen: std::collections::HashSet<String> = provider
        .outbox
        .pending_events()
        .map(|event| event.id.to_hex())
        .collect();
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    let queued: Vec<_> = provider
        .outbox
        .pending_events()
        .filter(|event| !seen.contains(&event.id.to_hex()))
        .collect();
    let mut statuses = Vec::new();
    for event in &queued {
        assert_ne!(
            u32::from(event.kind.as_u16()),
            beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            "no command was issued, so no receipt: {}",
            event.content
        );
        for word in [
            SESSION_RESTORED_NATIVE,
            "a-brand-new-conversation",
            "reattach",
        ] {
            assert!(!event.content.contains(word), "{word}: {}", event.content);
        }
        let value: serde_json::Value = serde_json::from_str(&event.content).unwrap();
        if let Some(status) = value["item"]["status"].as_str() {
            statuses.push(status.to_owned());
        }
    }
    assert!(
        statuses.iter().any(|status| status == "session_fresh"),
        "{statuses:?}"
    );
    assert!(
        statuses
            .iter()
            .all(|status| status == "session_fresh" || status.starts_with("execution_boundary")),
        "{statuses:?}"
    );
}

/// R4: the generation's metadata does not change on a fresh restore and has no
/// continuity field that could disagree with the transcript.
#[tokio::test]
async fn the_metadata_says_nothing_about_continuity_on_a_fresh_restore() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    let target = prior.target(&provider.config.instance_id);
    let before = serde_json::to_value(provider.metadata_for(&target, SessionStatus::Idle)).unwrap();
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    let after = serde_json::to_value(provider.metadata_for(&target, SessionStatus::Idle)).unwrap();
    assert_eq!(before["session"], after["session"]);
    let text = after.to_string().to_lowercase();
    for word in [
        "contin",
        "restor",
        "resum",
        "rehydrat",
        "native",
        "loaded",
        "reattach",
        "a-brand-new",
        "cursor",
    ] {
        assert!(!text.contains(word), "{word}: {text}");
    }
}

/// F3: a non-native resume records what that open delivered, so an earlier
/// package cannot be reported as lost for a conversation that never had one.
/// (A real rehydrated resume needs a relay and an umbrella chain, and no test
/// in this crate reaches it; the actor-level tests cover `Some` at open.)
#[tokio::test]
async fn a_resume_that_opens_new_records_that_opens_bootstrap_transport() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.bootstrap_transport = Some(session::BootstrapTransport::SystemPrompt);
        })
        .unwrap();
    let target = prior.target(&provider.config.instance_id);
    let resume = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "resume-new-conversation",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &resume)
        .await
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation + 1);
    assert_eq!(current.bootstrap_transport, None);
}

/// F3: a reattachment did not change what the conversation was given.
#[tokio::test]
async fn a_resume_that_reattaches_keeps_the_bootstrap_transport() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.bootstrap_transport = Some(session::BootstrapTransport::SystemPrompt);
        })
        .unwrap();
    provider.config.runtimes[0].agent_command = fake_agent(
        root.path(),
        "reattach-transport",
        &crate::session::testing::restorable_agent(root.path().join("work/t").to_str().unwrap()),
    );
    let target = prior.target(&provider.config.instance_id);
    let resume = crate::tests::lifecycle_target_event(
        &provider,
        prior.channel_id,
        "resume-reattached",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(prior.channel_id, &resume)
        .await
        .unwrap();
    let current = provider.state.session(&prior.session_id).unwrap();
    assert_eq!(current.generation, prior.generation + 1);
    assert_eq!(
        current.bootstrap_transport,
        Some(session::BootstrapTransport::SystemPrompt)
    );
}

#[tokio::test]
async fn fresh_restore_discloses_unattached_verified_package() {
    let root = tempfile::tempdir().unwrap();
    let (mut provider, prior) = unprompted(root.path()).await;
    provider
        .state
        .update_session(&prior.session_id, |record| {
            record.bootstrap_transport = Some(session::BootstrapTransport::SystemPrompt);
        })
        .unwrap();
    provider
        .restore_generation(&prior.session_id, None)
        .await
        .unwrap()
        .unwrap();
    assert!(
        provider.outbox.pending_events().any(|event| {
            let value: serde_json::Value = serde_json::from_str(&event.content).unwrap_or_default();
            value["item"]["status"] == "session_fresh"
                && value["item"]["reason"] == "context_not_reattached"
        }),
        "fresh restore must disclose that the prior package was not reattached"
    );
}
