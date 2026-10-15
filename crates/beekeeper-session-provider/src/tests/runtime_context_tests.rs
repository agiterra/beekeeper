//! A runtime that keeps its own conversation (descriptor `context:
//! "runtime"`) is never published as fresh or as restarted without context:
//! Beekeeper restored nothing, but the runtime may still hold its own.

use super::*;
use beekeeper_core::coding_session_runtime::RuntimeContext;

fn receipt_for(sink: &CollectingSink, command_id: &str) -> serde_json::Value {
    sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == command_id)
        .expect("answered")
}

/// A provider with no context sidecar (so every open names a loss reason),
/// whose runtimes declare `context` as given.
fn context_provider(dir: &Path, channel_id: Uuid, context: Option<RuntimeContext>) -> Provider {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let projects = write_projects(dir, channel_id, &cwd);
    let mut provider = provider_with_sidecar_path(&dir.join("state"), Some(&projects), None);
    for runtime in &mut provider.config.runtimes {
        runtime.context = context;
    }
    provider
}

#[tokio::test]
async fn create_and_resume_of_a_runtime_that_keeps_its_context_never_claim_a_fresh_start() {
    for context in [None, Some(RuntimeContext::Runtime)] {
        let keeps = context.is_some();
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let mut provider = context_provider(dir.path(), channel_id, context);
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("create");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let created = status_items(&sink);
        let fresh = created
            .iter()
            .find(|item| item["status"] == "session_fresh");
        let held = created
            .iter()
            .find(|item| item["status"] == crate::execution_scope::STATUS_RUNTIME_CONTEXT);
        assert_eq!(fresh.is_some(), !keeps, "{created:?}");
        assert_eq!(held.is_some(), keeps, "{created:?}");
        if let Some(held) = held {
            // S1: the withheld row's reason travels on the runtime row.
            assert!(held["reason"].is_string(), "{held}");
        }

        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        provider
            .sessions
            .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        let resume =
            lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let resumed = status_items(&sink);
        let restarted = resumed
            .iter()
            .find(|item| item["status"] == "session_restarted_without_context");
        let held = resumed
            .iter()
            .find(|item| item["status"] == crate::execution_scope::STATUS_RUNTIME_CONTEXT);
        assert_eq!(restarted.is_some(), !keeps, "{resumed:?}");
        assert_eq!(held.is_some(), keeps, "{resumed:?}");
        let receipt = receipt_for(&sink, "resume-1");
        assert_eq!(receipt["status"], "resumed_without_context", "{receipt}");
        let message = receipt["error"]["message"].as_str().expect("message");
        assert_eq!(
            message.contains("keeps its own conversation"),
            keeps,
            "{message}"
        );
        if let Some(held) = held {
            assert_eq!(held["reason"], "context_sidecar_unavailable", "{held}");
        }
    }
}
