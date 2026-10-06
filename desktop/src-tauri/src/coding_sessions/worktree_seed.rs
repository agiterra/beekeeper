//! Seeding a freshly cut worktree's build state, as the project declares it.
//!
//! This runs between the checkout being proved and the tree being recorded, so
//! the record carries what happened and nothing has run in the tree before it
//! is seeded. It lives in its own module because `worktree.rs` is within a few
//! lines of the file-size ceiling, and because the launcher's job here is small:
//! the declaration is parsed and carried out by `buzz-core`, and the sequence by
//! `beekeeper_session_provider::sandbox_seed_host`, which the action-step path shares.
//!
//! What is genuinely this layer's to decide is three things:
//!
//! * **Which checkout is the source.** The host resolves it — a manifest cannot
//!   name one — and it is the project's recorded checkout, the warm tree
//!   somebody actually builds in.
//! * **Where a shared pool lives.** Inside the directory the *seat's* provider
//!   already grants its executions, which is not the desktop's own host-Git
//!   directory: those are different roots with different caches, and a pool
//!   under the wrong one would be a directory the seat never reads. With no
//!   provider commissioned there is no such root, and every `share` is carried
//!   out as a clone and says so rather than calling one tree's own directory
//!   shared.
//! * **That none of it may fail the create.** An unseeded sandbox is *cold*,
//!   which is slow; a sandbox that is not the commit it claims is *wrong*. And
//!   `sandbox.yml` is a tracked file an agent may edit, so letting it fail a
//!   hire would hand whoever can edit it a lever to fail every hire.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use beekeeper_session_provider_pkg::execution_scope::project_scope_pool_dir;
use beekeeper_session_provider_pkg::sandbox_seed_host::{seed_tree, summarize};

use crate::coding_sessions::host_git;

/// Seed a freshly cut worktree off the async executor, and answer with the
/// receipt to disclose.
///
/// The orchestration lives here rather than at the call site because that file
/// sits against the file-size ceiling, and because a panicking seeder must not
/// cost a hire the tree it just cut — which is a property of seeding, not of
/// worktree creation.
pub(crate) async fn seed_for_created(
    app: &AppHandle,
    relay_url: String,
    source: String,
    repo_root: String,
    tree: String,
    project_ref: Option<String>,
) -> Option<serde_json::Value> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        seed_created_worktree(
            &app,
            &relay_url,
            Path::new(&source),
            Path::new(&repo_root),
            Path::new(&tree),
            project_ref.as_deref(),
        )
    })
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(
            target: "beekeeper::sandbox",
            %error,
            "the sandbox seeder task failed; this sandbox starts cold"
        );
        None
    })
}

/// Seed a freshly cut worktree, and answer with the receipt to disclose.
///
/// `None` when the project declares no `sandbox.yml`: nothing was asked for, so
/// there is nothing to say. Any other outcome — including one where nothing
/// landed — answers `Some`, because a cold sandbox a person was not told about
/// is what this disclosure exists to prevent.
pub(crate) fn seed_created_worktree(
    app: &AppHandle,
    relay_url: &str,
    source: &Path,
    repo_root: &Path,
    tree: &Path,
    project_ref: Option<&str>,
) -> Option<serde_json::Value> {
    let pool_root = provider_state_dir_for(app, relay_url)
        .map(|dir| project_scope_pool_dir(&dir, project_ref, tree));
    // One prepared boundary, for the git questions the seeder asks and for the
    // project's own setup recipes: preparing it twice would mean two answers to
    // the question of what this tree may touch.
    let boundary = match host_git::prepare(&host_git::Workspace {
        tree,
        repo_root: Some(repo_root),
        name: "sandbox-seed",
        host_branch: None,
        host_read: &[],
    }) {
        Ok(boundary) => boundary,
        Err(error) => {
            tracing::warn!(
                target: "beekeeper::sandbox",
                %error,
                "the project boundary could not be prepared for seeding"
            );
            return Some(serde_json::json!({
                "declares": true,
                "complete": false,
                "detail": format!(
                    "the project boundary could not be prepared, so nothing was seeded and this \
                     sandbox starts cold: {error}"
                ),
            }));
        }
    };

    let receipt = match seed_tree(&boundary, source, tree, pool_root.as_deref()) {
        // The project declares nothing to seed.
        Ok(None) => return None,
        Ok(Some(receipt)) => receipt,
        Err(refusal) => {
            // Present and wrong. Disclosed, and the create carries on cold.
            tracing::warn!(
                target: "beekeeper::sandbox",
                code = refusal.code,
                message = %refusal.message,
                "the project's sandbox.yml was refused; this sandbox starts cold"
            );
            return Some(serde_json::json!({
                "declares": true,
                "complete": false,
                "refusal": refusal,
                "detail": "the project's sandbox.yml could not be read, so nothing was seeded \
                           and this sandbox starts cold",
            }));
        }
    };

    let summary = summarize(&receipt);
    if receipt.complete {
        tracing::info!(target: "beekeeper::sandbox", summary = %summary, "seeded a seat worktree");
    } else {
        tracing::warn!(target: "beekeeper::sandbox", summary = %summary, "a seat worktree is cold");
        for outcome in receipt.unsatisfied() {
            tracing::warn!(
                target: "beekeeper::sandbox",
                entry = %outcome.id,
                code = outcome.disposition.code().unwrap_or("incomplete"),
                detail = %outcome.detail,
                "a sandbox entry did not land"
            );
        }
    }
    let mut value = serde_json::to_value(&receipt).ok()?;
    if let Some(object) = value.as_object_mut() {
        object.insert("declares".into(), serde_json::Value::Bool(true));
        object.insert("summary".into(), serde_json::Value::String(summary));
    }
    Some(value)
}

/// The state directory the executions that follow will use.
///
/// Not the desktop's own host-Git directory: a seat's provider keeps its state
/// under its own identity, with its own dependency caches, so a pool seeded
/// under the wrong root would be a directory the seat never reads. With no
/// provider commissioned there is no such root, and every `share` downgrades to
/// a clone — disclosed, never called shared.
fn provider_state_dir_for(app: &AppHandle, relay_url: &str) -> Option<PathBuf> {
    let store = crate::session_provider::store::load_provider_store(app).ok()?;
    let record = super::workdir_store::projects_view_provider_for_relay(&store, relay_url)?;
    crate::session_provider::provider_state_dir(app, &record.provider_pubkey).ok()
}
