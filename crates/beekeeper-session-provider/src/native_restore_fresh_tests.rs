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
