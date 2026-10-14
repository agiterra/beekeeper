//! `checkpointOnStop`: one worktree checkpoint before a stop's quiescence
//! (HEADLESS_PLUGGABLE_SLICE.md § Step 2, "checkpoint before stop").
//!
//! Off by default. When `BEEKEEPER_CSP_CHECKPOINT_ON_STOP` is on, a
//! `session.stop` first asks whether the execution's worktree differs from
//! the tree its last kind-44231 checkpoint recorded; if it does, the existing
//! end-of-turn capture runs once for the turn the edits belong to, the stop
//! waits for it to publish, and only then does quiescence begin. The host
//! sets the variable where it sets the other `BEEKEEPER_CSP_*` ones
//! (`docs/agent-host.md`); the host.json spelling is `checkpointOnStop`.

use std::time::Duration;

use crate::commands::StopPlan;
use crate::session::SessionEvent;
use crate::turn_checkpoint_git::{capture_tree, CapturePhase, RefLeaf};
use crate::Provider;

/// Environment variable the provider reads; `1`, `true`, `on` or `yes` enable.
pub const CHECKPOINT_ON_STOP_ENV: &str = "BEEKEEPER_CSP_CHECKPOINT_ON_STOP";

/// Longest a stop waits for its checkpoint to publish: the end capture's own
/// ceiling plus the host-Git preparation it runs behind.
const CHECKPOINT_BEFORE_STOP_WAIT: Duration = Duration::from_secs(90);

/// Read the switch from the process environment.
pub fn checkpoint_on_stop_from_env() -> bool {
    checkpoint_on_stop_from(|name| std::env::var(name).ok())
}

/// [`checkpoint_on_stop_from_env`] against an injected lookup, for tests.
pub fn checkpoint_on_stop_from(lookup: impl Fn(&str) -> Option<String>) -> bool {
    lookup(CHECKPOINT_ON_STOP_ENV).is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        )
    })
}

/// Why no checkpoint was taken before this stop; each is logged, none blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopCheckpointSkipped {
    /// The record is gone or already closed.
    NoOpenRecord,
    /// The worktree's tree equals the last published checkpoint's tree.
    Unchanged,
    /// The worktree changed but no turn is known to attribute the edits to;
    /// a `turn` checkpoint must name a turn, so none is published.
    NoTurnToAttribute,
    /// The tree could not be measured; the sentence is the capture's own.
    NotMeasured(String),
}

impl Provider {
    /// The turn a stop checkpoint belongs to: the open turn, else the turn
    /// whose coverage the checkpoint chain still holds open.
    fn stop_checkpoint_turn_id(&self, session_id: &str, generation: u64) -> Option<String> {
        let record = self.state.session(session_id)?;
        if let Some(open) = &record.open_turn {
            return Some(open.turn_id.clone());
        }
        self.state
            .checkpoint_chain(session_id, generation)
            .and_then(|chain| chain.open.as_ref())
            .map(|turn| turn.turn_id.clone())
    }

    /// Decide and, when due, run and await the checkpoint before `plan`'s
    /// stop. Returns what was skipped, or `None` when a checkpoint published.
    pub(crate) async fn checkpoint_before_stop(
        &mut self,
        plan: &StopPlan,
    ) -> anyhow::Result<Option<StopCheckpointSkipped>> {
        let session_id = plan.target.session_id.clone();
        let Some(record) = self
            .state
            .session(&session_id)
            .filter(|record| !record.closed)
            .cloned()
        else {
            return Ok(Some(StopCheckpointSkipped::NoOpenRecord));
        };
        let generation = record.generation;
        let through_seq = record.next_seq.saturating_sub(1);
        let host_git = self.host_git_request(&record, &record.cwd);
        let scope = host_git.prepare().await;
        let measured = capture_tree(
            &record.cwd,
            scope.as_ref(),
            &session_id,
            generation,
            RefLeaf::Through(through_seq),
            None,
            CapturePhase::TurnEnd,
        )
        .await;
        let tree = match measured {
            Ok(captured) => captured.tree,
            Err(failure) => {
                let sentence = failure.sentence.clone();
                tracing::info!(target: "csp::checkpoint", %session_id, "no checkpoint before stop: {sentence}");
                return Ok(Some(StopCheckpointSkipped::NotMeasured(sentence)));
            }
        };
        let last_tree = self
            .state
            .checkpoint_chain(&session_id, generation)
            .and_then(|chain| chain.recorded.values().next_back().cloned())
            .flatten();
        if last_tree.as_deref() == Some(tree.as_str()) {
            tracing::info!(target: "csp::checkpoint", %session_id, "no checkpoint before stop: worktree unchanged since the last checkpoint");
            return Ok(Some(StopCheckpointSkipped::Unchanged));
        }
        let Some(turn_id) = self.stop_checkpoint_turn_id(&session_id, generation) else {
            tracing::info!(target: "csp::checkpoint", %session_id, "no checkpoint before stop: the worktree changed but no turn is open to attribute it to");
            return Ok(Some(StopCheckpointSkipped::NoTurnToAttribute));
        };
        self.start_turn_checkpoint(
            &session_id,
            plan.channel_id,
            &plan.target,
            &turn_id,
            through_seq,
        );
        // Wait for this capture to report, handling whatever else reports
        // meanwhile exactly as the loop would.
        let deadline = tokio::time::Instant::now() + CHECKPOINT_BEFORE_STOP_WAIT;
        loop {
            let event = tokio::select! {
                event = self.session_events.recv() => event,
                () = tokio::time::sleep_until(deadline) => {
                    anyhow::bail!("checkpoint before stop did not report within {}s", CHECKPOINT_BEFORE_STOP_WAIT.as_secs());
                }
            };
            let Some(event) = event else {
                anyhow::bail!("session event channel closed before the checkpoint reported");
            };
            let ours = matches!(
                &event,
                SessionEvent::CheckpointCaptured { session_id: reported, .. } if *reported == session_id
            );
            self.handle_session_event(event)?;
            if ours {
                tracing::info!(target: "csp::checkpoint", %session_id, turn_id, "checkpoint before stop published");
                return Ok(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway repository in the test's own temporary directory, with one
    /// commit so the capture has a `HEAD` to parent on. Hermetic git: no
    /// inherited repository selection, no global or system configuration.
    fn git_init_with_commit(cwd: &std::path::Path) {
        let git = |args: &[&str]| {
            let mut command = std::process::Command::new("git");
            for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
                command.env_remove(var);
            }
            let output = command
                .arg("-C")
                .arg(cwd)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(cwd.join("README"), b"fixture\n").expect("seed file");
        git(&["add", "README"]);
        git(&["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
    }

    #[test]
    fn the_switch_is_off_unless_spelled_on() {
        assert!(!checkpoint_on_stop_from(|_| None));
        assert!(!checkpoint_on_stop_from(|_| Some("0".into())));
        assert!(!checkpoint_on_stop_from(|_| Some("off".into())));
        for on in ["1", "true", "ON", " yes "] {
            assert!(
                checkpoint_on_stop_from(|name| {
                    assert_eq!(name, CHECKPOINT_ON_STOP_ENV);
                    Some(on.into())
                }),
                "{on}"
            );
        }
    }

    #[tokio::test]
    async fn a_stop_without_the_switch_takes_no_checkpoint() {
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("ordinary-work");
        std::fs::create_dir(&cwd).expect("workspace");
        let channel = uuid::Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        assert!(!provider.checkpoint_on_stop, "off by default");
        let event = crate::tests::create_event(&provider, channel, "create-no-ckpt");
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        let stop = crate::tests::lifecycle_target_event(
            &provider,
            channel,
            "stop-no-ckpt",
            "session.stop",
            &target,
        );
        provider
            .handle_command_event(channel, &stop)
            .await
            .expect("stop");
        let sink = crate::tests::CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert!(sink
            .contents_of(beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT)
            .is_empty());
        assert!(sink
            .contents_of(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .iter()
            .any(|r| r["commandId"] == "stop-no-ckpt" && r["status"] == "stopped"));
    }

    /// With the switch on and the worktree edited after the create, the
    /// stop publishes one checkpoint for the open turn before `stopped`.
    #[tokio::test]
    async fn a_stop_with_the_switch_checkpoints_a_changed_worktree_once() {
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("ordinary-work");
        std::fs::create_dir(&cwd).expect("workspace");
        git_init_with_commit(&cwd);
        let channel = uuid::Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        provider.checkpoint_on_stop = true;
        let event = crate::tests::create_event(&provider, channel, "create-ckpt");
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        // The turn whose edits the stop checkpoint belongs to.
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.open_turn = Some(crate::state::OpenTurn {
                    turn_id: "turn-stop".into(),
                    command_id: Some("turn-cmd-stop".into()),
                    team_wake_eligible: false,
                    started_at_ms: crate::state::now_ms(),
                    operator_pubkey: None,
                });
            })
            .expect("open turn");
        std::fs::write(cwd.join("edited.txt"), b"changed before stop\n").expect("edit");
        let stop = crate::tests::lifecycle_target_event(
            &provider,
            channel,
            "stop-ckpt",
            "session.stop",
            &target,
        );
        provider
            .handle_command_event(channel, &stop)
            .await
            .expect("stop");
        let sink = crate::tests::CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let checkpoints = sink.contents_of(beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT);
        assert_eq!(checkpoints.len(), 1, "{checkpoints:?}");
        assert_eq!(checkpoints[0]["reason"], "turn");
        assert_eq!(checkpoints[0]["turnId"], "turn-stop");
        assert_eq!(checkpoints[0]["session"]["sessionId"], target.session_id);
        assert!(
            checkpoints[0]["git"]["tree"].is_string(),
            "{:?}",
            checkpoints[0]
        );
        assert!(sink
            .contents_of(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .iter()
            .any(|r| r["commandId"] == "stop-ckpt" && r["status"] == "stopped"));
        // A second look at the same tree is unchanged: nothing more is taken.
        let plan = StopPlan {
            command_id: "stop-again".into(),
            channel_id: channel,
            target: target.clone(),
        };
        assert_eq!(
            provider
                .checkpoint_before_stop(&plan)
                .await
                .expect("decide"),
            Some(StopCheckpointSkipped::NoOpenRecord),
            "the record is closed after the stop"
        );
    }
}
