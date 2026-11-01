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
            .any(|event| event.content.contains(status))
    };
    assert!(said(SESSION_RESTARTED_FRESH));
    assert!(!said(SESSION_RESTORED_NATIVE));
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
