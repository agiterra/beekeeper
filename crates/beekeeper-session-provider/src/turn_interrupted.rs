//! A turn the provider's restart could neither continue nor prove ended
//! (HEADLESS_PLUGGABLE_SLICE.md § Step 2, "interrupted then terminal").
//!
//! Boot recovery used to synthesize a failed `result` for every turn that was
//! open when the previous process died. When the recorded process group is
//! still present that result is a guess: the adapter may still be working
//! in the worktree. So the receipt says `turn_interrupted` with
//! `quiescence: "unconfirmed"`, a marker file remembers the turn, and the
//! runtime tick asks the existing witness (`process_witness.rs`) until the
//! group is proved absent — then one terminal `failed` receipt under the same
//! `commandId`, the synthesized `result`, and the marker is gone. No reaper
//! and no signal: `killpg(group, 0)` is the only probe, as before.

use std::path::PathBuf;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_payload::LifecycleReceipt;
use beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use beekeeper_sdk::builders::build_coding_session_lifecycle_receipt;
use beekeeper_sdk::coding_session::{
    build_coding_session_turn_interrupted_receipt, coding_session_lifecycle_receipt_semantic_key,
    coding_session_turn_receipt_semantic_key_str, TURN_INTERRUPTED_BY_RESTART,
    TURN_INTERRUPTED_STATUS,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::publish::Priority;
use crate::state::{OpenTurn, SessionRecord};
use crate::Provider;

/// Marker schema, so an older provider never misreads the file.
const MARKER_SCHEMA: &str = "beekeeper-turn-interrupted/v1";

/// What the terminal needs once the group is proved absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InterruptedTurn {
    /// Always [`MARKER_SCHEMA`].
    pub(crate) schema: String,
    /// The generation whose group was still present at recovery.
    pub(crate) target: CodingSessionTarget,
    /// Channel the receipt and result publish into.
    pub(crate) channel_id: Uuid,
    /// The turn as it was recorded open.
    pub(crate) open_turn: OpenTurn,
    /// The process group the witness recorded at recovery, probed by the
    /// marker itself from then on: a resume of this session rewrites the
    /// witness for the new generation (`stage_reopening`), and the
    /// interrupted turn must still be able to prove its own group gone.
    #[serde(default)]
    pub(crate) group_id: Option<u32>,
}

/// Whether `group` is proved absent right now: `killpg(group, 0)` answers
/// `ESRCH`. `None` is unprovable, never absent.
fn group_proved_absent(group: Option<u32>) -> bool {
    let Some(group) = group
        .and_then(|id| i32::try_from(id).ok())
        .filter(|id| *id > 0)
    else {
        return false;
    };
    #[cfg(unix)]
    {
        matches!(
            nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), None),
            Err(nix::errno::Errno::ESRCH)
        )
    }
    #[cfg(not(unix))]
    {
        let _ = group;
        false
    }
}

fn marker_path(state_dir: &std::path::Path, session_id: &str) -> PathBuf {
    state_dir.join(format!("turn-interrupted-{session_id}.json"))
}

impl Provider {
    /// Whether the witness recorded a group for `target` and that group is
    /// present right now. Distinct from "unproved": a record with no witness
    /// or no group id answers `false` here, so boot recovery synthesizes its
    /// terminal at once exactly as before.
    pub(crate) fn recorded_group_present(&self, target: &CodingSessionTarget) -> bool {
        let Some(witness) = self
            .state
            .session(&target.session_id)
            .and_then(|record| record.process_witness.clone())
        else {
            return false;
        };
        if witness.target != *target || witness.quiesced {
            return false;
        }
        let Some(group) = witness
            .group_id
            .and_then(|id| i32::try_from(id).ok())
            .filter(|id| *id > 0)
        else {
            return false;
        };
        #[cfg(unix)]
        {
            !matches!(
                nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), None),
                Err(nix::errno::Errno::ESRCH)
            )
        }
        #[cfg(not(unix))]
        {
            let _ = group;
            false
        }
    }

    /// Record the interrupted turn durably and queue its `turn_interrupted`
    /// receipt. The marker is written before the receipt is queued, so a
    /// crash between the two leaves a turn to settle, never a receipt that
    /// promises a terminal nobody will send.
    pub(crate) fn stage_interrupted_turn(
        &mut self,
        record: &SessionRecord,
        target: &CodingSessionTarget,
        open_turn: &OpenTurn,
    ) -> anyhow::Result<()> {
        let Some(command_id) = open_turn.command_id.as_deref() else {
            return Ok(());
        };
        let marker = InterruptedTurn {
            schema: MARKER_SCHEMA.to_owned(),
            target: target.clone(),
            channel_id: record.channel_id,
            open_turn: open_turn.clone(),
            group_id: record
                .process_witness
                .as_ref()
                .filter(|witness| witness.target == *target)
                .and_then(|witness| witness.group_id),
        };
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &marker_path(&self.config.state_dir, &target.session_id),
            &serde_json::to_vec(&marker)?,
        )
        .map_err(anyhow::Error::msg)?;
        let event =
            build_coding_session_turn_interrupted_receipt(record.channel_id, command_id, target)?
                .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            &coding_session_turn_receipt_semantic_key_str(command_id, TURN_INTERRUPTED_STATUS),
            Priority::High,
            event,
        )?;
        Ok(())
    }

    /// Every interrupted turn still waiting on its group.
    pub(crate) fn interrupted_turns(&self) -> Vec<InterruptedTurn> {
        let Ok(entries) = std::fs::read_dir(&self.config.state_dir) else {
            return Vec::new();
        };
        let mut turns: Vec<InterruptedTurn> = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("turn-interrupted-") && name.ends_with(".json")
                })
            })
            .filter_map(|entry| std::fs::read(entry.path()).ok())
            .filter_map(|bytes| serde_json::from_slice::<InterruptedTurn>(&bytes).ok())
            .filter(|turn| turn.schema == MARKER_SCHEMA)
            .collect();
        turns.sort_by(|a, b| a.target.session_id.cmp(&b.target.session_id));
        turns
    }

    /// Whether `turn`'s group is proved absent: by the marker's own group id
    /// first, else by the session's witness (which a resume may since have
    /// rewritten for a newer generation — the marker's id is what survives
    /// that), else because the record itself is gone.
    fn interrupted_turn_settleable(&self, turn: &InterruptedTurn) -> bool {
        group_proved_absent(turn.group_id)
            || self.recovered_group_absent(&turn.target)
            || self.state.session(&turn.target.session_id).is_none()
    }

    /// Settle the interrupted turn of `session_id`, if one is waiting and its
    /// group is proved absent. Called before a resume rewrites the witness
    /// (`stage_reopening`), so the terminal `failed` is published ahead of
    /// the new generation's own receipts rather than never: the old witness
    /// is the last record of the old group unless the marker kept it.
    pub(crate) fn settle_interrupted_turn_before_reopening(
        &mut self,
        session_id: &str,
    ) -> anyhow::Result<usize> {
        let waiting: Vec<InterruptedTurn> = self
            .interrupted_turns()
            .into_iter()
            .filter(|turn| turn.target.session_id == session_id)
            .collect();
        let mut settled = 0usize;
        for turn in waiting {
            if self.interrupted_turn_settleable(&turn) {
                self.settle_interrupted_turn(&turn)?;
                settled += 1;
            } else {
                tracing::warn!(
                    target: "csp::recovery",
                    session_id,
                    group = ?turn.group_id,
                    "reopening a session whose interrupted turn's process group is still present; the marker keeps the group id and settles once it exits"
                );
            }
        }
        Ok(settled)
    }

    /// For each interrupted turn whose group is now proved absent: one
    /// terminal `failed` receipt under the turn's `commandId`, the
    /// synthesized `result`, and the marker removed. Turns whose group is
    /// still present, or unprovable, wait for the next tick.
    pub(crate) fn settle_interrupted_turns(&mut self) -> anyhow::Result<usize> {
        let mut settled = 0usize;
        for turn in self.interrupted_turns() {
            if !self.interrupted_turn_settleable(&turn) {
                continue;
            }
            self.settle_interrupted_turn(&turn)?;
            settled += 1;
        }
        Ok(settled)
    }

    /// One interrupted turn's terminal: the `failed` receipt, the synthesized
    /// `result`, and the marker removed. A marker whose session record is
    /// gone is only removed; there is nothing left to answer into.
    fn settle_interrupted_turn(&mut self, turn: &InterruptedTurn) -> anyhow::Result<()> {
        {
            let Some(command_id) = turn.open_turn.command_id.as_deref() else {
                let _ = std::fs::remove_file(marker_path(
                    &self.config.state_dir,
                    &turn.target.session_id,
                ));
                return Ok(());
            };
            let Some(record) = self.state.session(&turn.target.session_id).cloned() else {
                let _ = std::fs::remove_file(marker_path(
                    &self.config.state_dir,
                    &turn.target.session_id,
                ));
                return Ok(());
            };
            let receipt = LifecycleReceipt::failed(
                command_id,
                TURN_INTERRUPTED_BY_RESTART,
                "The provider restarted while this turn was running and its process has now \
                 exited; the turn did not finish. Send it again.",
            );
            let event = build_coding_session_lifecycle_receipt(
                turn.channel_id,
                command_id,
                &serde_json::to_string(&receipt)?,
            )?
            .sign_with_keys(&self.config.keys)?;
            self.outbox.enqueue(
                KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
                &coding_session_lifecycle_receipt_semantic_key(command_id),
                Priority::High,
                event,
            )?;
            self.synthesize_interrupted_terminal(
                &record,
                &turn.target,
                &turn.open_turn,
                Some(TURN_INTERRUPTED_BY_RESTART),
            )?;
            std::fs::remove_file(marker_path(&self.config.state_dir, &turn.target.session_id))?;
            tracing::info!(
                target: "csp::recovery",
                session_id = %turn.target.session_id,
                command_id,
                "interrupted turn settled: process group proved absent"
            );
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::process_witness::ExecutionProcessWitness;
    use crate::session::testing::fake_agent;
    use crate::tests::{config_of, write_projects, CollectingSink};
    use beekeeper_core::kind::KIND_CODING_SESSION_TRANSCRIPT;
    use nostr::Keys;

    /// A child in its own process group that exits on its own; the test
    /// never signals it (`kill_on_drop` is off), it only waits for it.
    fn own_group_child(secs: &str) -> std::process::Child {
        use std::os::unix::process::CommandExt;
        std::process::Command::new("sleep")
            .arg(secs)
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("sleep child")
    }

    fn receipts(sink: &CollectingSink, command_id: &str) -> Vec<serde_json::Value> {
        sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .filter(|content| content["commandId"] == command_id)
            .collect()
    }

    #[tokio::test]
    async fn a_present_group_at_recovery_is_interrupted_then_failed_once_it_exits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel, &cwd);
        let keys = Keys::generate();
        let agent = fake_agent(
            dir.path(),
            "good-agent",
            crate::session::testing::GOOD_AGENT,
        );
        // The group the previous process recorded: a child of this test
        // that exits by itself. The provider only probes it.
        let mut lingering = own_group_child("4");
        let group = lingering.id();

        let (target, record_command) = {
            let mut first = Provider::new(config_of(
                keys.clone(),
                &state_dir,
                Some(&projects),
                agent.clone(),
            ))
            .expect("provider");
            let create = crate::tests::create_event(&first, channel, "create-1");
            first
                .handle_command_event(channel, &create)
                .await
                .expect("create");
            let target = first
                .state
                .sessions()
                .next()
                .expect("session")
                .target(&first.config.instance_id);
            // The previous process: a verified turn open, and the witness
            // naming the lingering group.
            first
                .state
                .update_session(&target.session_id, |record| {
                    record.open_turn = Some(OpenTurn {
                        turn_id: "turn-7".into(),
                        command_id: Some("turn-cmd-7".into()),
                        team_wake_eligible: false,
                        started_at_ms: now_ms_for_test(),
                        operator_pubkey: None,
                    });
                    record.process_witness = Some(ExecutionProcessWitness {
                        target: target.clone(),
                        attempted_target: None,
                        group_id: Some(group),
                        quiesced: false,
                    });
                })
                .expect("open turn recorded");
            first
                .sessions
                .shutdown_and_join(&target.session_id, crate::session::ACTOR_RETIRE_GRACE)
                .await;
            first.flush(&CollectingSink::new()).await.expect("flush");
            (target, "turn-cmd-7")
        };

        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        assert!(
            restarted.recorded_group_present(&target),
            "fixture group is alive"
        );
        restarted.recover().await.expect("recover");
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");

        let interrupted = receipts(&sink, record_command);
        assert_eq!(interrupted.len(), 1, "{interrupted:?}");
        assert_eq!(interrupted[0]["status"], TURN_INTERRUPTED_STATUS);
        assert_eq!(interrupted[0]["quiescence"], "unconfirmed");
        assert_eq!(interrupted[0]["session"]["sessionId"], target.session_id);
        assert!(
            sink.contents_of(KIND_CODING_SESSION_TRANSCRIPT)
                .iter()
                .all(|item| item["item"]["kind"] != "result"),
            "no synthesized result while the group is present"
        );
        assert_eq!(restarted.interrupted_turns().len(), 1);
        assert_eq!(
            restarted.interrupted_turns()[0].group_id,
            Some(group),
            "the marker keeps the group it must outlive the witness to probe"
        );
        assert!(restarted
            .state
            .session(&target.session_id)
            .expect("record")
            .open_turn
            .is_none());
        // Still present: the tick settles nothing.
        assert_eq!(restarted.settle_interrupted_turns().expect("tick"), 0);

        // The group exits on its own; the test only waits for it.
        lingering.wait().expect("sleep exits");
        assert!(crate::session::testing::group_is_gone(group));
        assert_eq!(restarted.settle_interrupted_turns().expect("tick"), 1);
        assert_eq!(restarted.settle_interrupted_turns().expect("idempotent"), 0);
        assert!(restarted.interrupted_turns().is_empty());
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        let terminal = receipts(&sink, record_command);
        assert_eq!(terminal.len(), 1, "{terminal:?}");
        assert_eq!(terminal[0]["status"], "failed");
        assert_eq!(terminal[0]["error"]["code"], TURN_INTERRUPTED_BY_RESTART);
        assert!(
            terminal[0]["session"].is_null(),
            "a failed receipt names no session"
        );
        let results: Vec<serde_json::Value> = sink
            .contents_of(KIND_CODING_SESSION_TRANSCRIPT)
            .into_iter()
            .filter(|item| item["item"]["kind"] == "result")
            .collect();
        assert_eq!(results.len(), 1, "{results:?}");
        assert_eq!(results[0]["item"]["result"], "provider terminated mid-turn");
        assert_eq!(results[0]["item"]["errorCode"], TURN_INTERRUPTED_BY_RESTART);
        assert_eq!(results[0]["turnId"], "turn-7");
    }

    /// A resume of the same session in the tick the group exits: reopening
    /// rewrites the witness for the new generation, so the interrupted turn
    /// is settled first — one terminal `failed`, the marker gone — rather
    /// than left with a witness that can no longer prove its group absent.
    #[tokio::test]
    async fn reopening_settles_an_interrupted_turn_whose_group_has_exited() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel, &cwd);
        let keys = Keys::generate();
        let agent = fake_agent(
            dir.path(),
            "good-agent",
            crate::session::testing::GOOD_AGENT,
        );
        let mut lingering = own_group_child("2");
        let group = lingering.id();
        let target = {
            let mut first = Provider::new(config_of(
                keys.clone(),
                &state_dir,
                Some(&projects),
                agent.clone(),
            ))
            .expect("provider");
            let create = crate::tests::create_event(&first, channel, "create-3");
            first
                .handle_command_event(channel, &create)
                .await
                .expect("create");
            let target = first
                .state
                .sessions()
                .next()
                .expect("session")
                .target(&first.config.instance_id);
            first
                .state
                .update_session(&target.session_id, |record| {
                    record.open_turn = Some(OpenTurn {
                        turn_id: "turn-9".into(),
                        command_id: Some("turn-cmd-9".into()),
                        team_wake_eligible: false,
                        started_at_ms: now_ms_for_test(),
                        operator_pubkey: None,
                    });
                    record.process_witness = Some(ExecutionProcessWitness {
                        target: target.clone(),
                        attempted_target: None,
                        group_id: Some(group),
                        quiesced: false,
                    });
                })
                .expect("open turn recorded");
            first
                .sessions
                .shutdown_and_join(&target.session_id, crate::session::ACTOR_RETIRE_GRACE)
                .await;
            first.flush(&CollectingSink::new()).await.expect("flush");
            target
        };
        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        restarted.recover().await.expect("recover");
        restarted
            .flush(&CollectingSink::new())
            .await
            .expect("flush");
        assert_eq!(restarted.interrupted_turns().len(), 1);

        // The group exits; in the same tick the session is reopened for its
        // next generation, which rewrites the witness.
        lingering.wait().expect("sleep exits");
        let next = CodingSessionTarget {
            generation: target.generation + 1,
            ..target.clone()
        };
        restarted
            .stage_reopening(&target.session_id, &next)
            .expect("reopen");
        let witness = restarted
            .state
            .session(&target.session_id)
            .expect("record")
            .process_witness
            .clone()
            .expect("witness");
        assert_eq!(
            witness.target, next,
            "the witness now names the new generation"
        );
        assert_eq!(witness.group_id, None);
        assert!(
            restarted.interrupted_turns().is_empty(),
            "settled before the witness was rewritten"
        );
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        let terminal = receipts(&sink, "turn-cmd-9");
        assert_eq!(terminal.len(), 1, "{terminal:?}");
        assert_eq!(terminal[0]["status"], "failed");
        assert_eq!(terminal[0]["error"]["code"], TURN_INTERRUPTED_BY_RESTART);
        // Nothing left for the tick, and nothing published twice.
        assert_eq!(restarted.settle_interrupted_turns().expect("tick"), 0);
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        assert!(receipts(&sink, "turn-cmd-9").is_empty());
    }

    #[tokio::test]
    async fn no_recorded_group_keeps_the_immediate_synthesis() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel, &cwd);
        let keys = Keys::generate();
        let agent = fake_agent(
            dir.path(),
            "good-agent",
            crate::session::testing::GOOD_AGENT,
        );
        let target = {
            let mut first = Provider::new(config_of(
                keys.clone(),
                &state_dir,
                Some(&projects),
                agent.clone(),
            ))
            .expect("provider");
            let create = crate::tests::create_event(&first, channel, "create-2");
            first
                .handle_command_event(channel, &create)
                .await
                .expect("create");
            let target = first
                .state
                .sessions()
                .next()
                .expect("session")
                .target(&first.config.instance_id);
            first
                .state
                .update_session(&target.session_id, |record| {
                    record.open_turn = Some(OpenTurn {
                        turn_id: "turn-8".into(),
                        command_id: Some("turn-cmd-8".into()),
                        team_wake_eligible: false,
                        started_at_ms: now_ms_for_test(),
                        operator_pubkey: None,
                    });
                    record.process_witness = None;
                })
                .expect("open turn recorded");
            first
                .sessions
                .shutdown_and_join(&target.session_id, crate::session::ACTOR_RETIRE_GRACE)
                .await;
            first.flush(&CollectingSink::new()).await.expect("flush");
            target
        };
        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        assert!(!restarted.recorded_group_present(&target));
        restarted.recover().await.expect("recover");
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        assert!(
            receipts(&sink, "turn-cmd-8").is_empty(),
            "no interrupted receipt"
        );
        assert!(restarted.interrupted_turns().is_empty());
        let results: Vec<serde_json::Value> = sink
            .contents_of(KIND_CODING_SESSION_TRANSCRIPT)
            .into_iter()
            .filter(|item| item["item"]["kind"] == "result")
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["item"]["result"], "provider terminated mid-turn");
        assert!(results[0]["item"].get("errorCode").is_none());
    }

    fn now_ms_for_test() -> i64 {
        crate::state::now_ms()
    }
}
