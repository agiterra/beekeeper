//! A recorded group is observation evidence, never authority to signal a PID.
use beekeeper_core::coding_session_command::CodingSessionTarget;
use serde::{Deserialize, Serialize};

/// Private generation-bound child process observation retained across worker death.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProcessWitness {
    /// Exact responsibility for which this process was opened.
    pub target: CodingSessionTarget,
    /// Actual ACP child group captured at spawn. None fences an interrupted reopen.
    pub group_id: Option<u32>,
    /// Permanent exact-generation proof after a verified join or matching group absence.
    #[serde(default)]
    pub quiesced: bool,
}
impl ExecutionProcessWitness {
    /// Only an absent matching group proves quiescence without a live registry.
    /// Presence, permission errors, PID reuse and unsupported platforms stay unproved.
    pub(crate) fn proves_absent(&self, target: &CodingSessionTarget) -> bool {
        if self.target != *target {
            return false;
        }
        if self.quiesced {
            return true;
        }
        let Some(group) = self
            .group_id
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
}
impl crate::Provider {
    pub(crate) fn recovered_group_absent(&self, target: &CodingSessionTarget) -> bool {
        self.state
            .session(&target.session_id)
            .and_then(|record| record.process_witness.as_ref())
            .is_some_and(|witness| witness.proves_absent(target))
    }
    pub(crate) fn stage_reopening(
        &mut self,
        session_id: &str,
        target: &CodingSessionTarget,
    ) -> anyhow::Result<()> {
        self.state.update_session(session_id, |record| {
            record.process_witness = Some(ExecutionProcessWitness {
                target: target.clone(),
                group_id: None,
                quiesced: false,
            })
        })?;
        Ok(())
    }
    pub(crate) fn record_quiesced(&mut self, target: &CodingSessionTarget) -> anyhow::Result<()> {
        let record = self
            .state
            .session(&target.session_id)
            .ok_or_else(|| anyhow::anyhow!("quiesced record missing"))?;
        if record.target(&self.config.instance_id) != *target {
            return Err(anyhow::anyhow!("quiescence witness target changed"));
        }
        let group_id = record
            .process_witness
            .as_ref()
            .and_then(|witness| witness.group_id);
        self.state.update_session(&target.session_id, |record| {
            record.process_witness = Some(ExecutionProcessWitness {
                target: target.clone(),
                group_id,
                quiesced: true,
            })
        })?;
        Ok(())
    }
    /// The existing shutdown boundary for cleanup paths that owe no lifecycle receipt.
    pub(crate) async fn quiesce_cleanup(
        &mut self,
        target: &CodingSessionTarget,
        operation: &str,
    ) -> anyhow::Result<()> {
        let tracked = self.sessions.handle(&target.session_id).is_some();
        let absent = self.recovered_group_absent(target);
        let marker = crate::session_stop::begin_quiescence(
            &self.config,
            target,
            operation,
            tracked || absent,
        )
        .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?;
        if !tracked && !absent {
            return Err(anyhow::anyhow!(
                "SESSION_QUIESCENCE_UNCONFIRMED: recovered child group absence is unproved"
            ));
        }
        if self
            .sessions
            .shutdown_and_join(&target.session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await
            == crate::session::ActorQuiescence::Unproven
        {
            return Err(anyhow::anyhow!(
                "SESSION_QUIESCENCE_UNCONFIRMED: child group has not exited"
            ));
        }
        self.record_quiesced(target)?;
        std::fs::remove_file(marker)?;
        Ok(())
    }
    pub(crate) fn actor_has_unproved_stop(&self, actor: Option<&str>) -> bool {
        let Some(actor) = actor else {
            return false;
        };
        self.state.sessions().any(|record| {
            record.actor.as_deref() == Some(actor)
                && (self
                    .config
                    .state_dir
                    .join(format!("stop-quiescence-{}.json", record.session_id))
                    .exists()
                    || (record.closed && record.process_witness.is_some()))
                && !self.recovered_group_absent(&record.target(&self.config.instance_id))
        })
    }
}
#[cfg(all(test, unix))]
#[path = "process_witness_tests.rs"]
mod tests;
