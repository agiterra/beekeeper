//! A recorded group is observation evidence, never authority to signal a PID.
use beekeeper_core::coding_session_command::CodingSessionTarget;
use serde::{Deserialize, Serialize};

/// Private generation-bound child process observation retained across worker death.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProcessWitness {
    /// Exact responsibility for which this process was opened.
    pub target: CodingSessionTarget,
    /// Failed successor attempt whose generation never committed. The owning
    /// incumbent was proved absent before this attempt; its actual group is
    /// still required to be absent before either responsibility can retire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempted_target: Option<CodingSessionTarget>,
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
        if self.attempted_target.as_ref().is_some_and(|attempt| {
            attempt.driver != target.driver
                || attempt.instance_id != target.instance_id
                || attempt.session_id != target.session_id
                || target.generation.checked_add(1) != Some(attempt.generation)
        }) {
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
                attempted_target: None,
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
        let attempted_target = record
            .process_witness
            .as_ref()
            .and_then(|w| w.attempted_target.clone());
        let group_id = record
            .process_witness
            .as_ref()
            .and_then(|witness| witness.group_id);
        self.state.update_session(&target.session_id, |record| {
            record.process_witness = Some(ExecutionProcessWitness {
                target: target.clone(),
                attempted_target,
                group_id,
                quiesced: true,
            })
        })?;
        Ok(())
    }
    /// Fold a failed attempt only into its still-staged exact incumbent.
    /// A successful successor, newer process witness or generation is never
    /// replaced. A successor attempt retains its separate target provenance.
    pub(crate) fn record_failed_startup(
        &mut self,
        prior: &crate::state::SessionRecord,
        observed: &ExecutionProcessWitness,
    ) -> anyhow::Result<bool> {
        let owner = prior.target(&self.config.instance_id);
        let same_execution = owner.driver == observed.target.driver
            && owner.instance_id == observed.target.instance_id
            && owner.session_id == observed.target.session_id
            && (owner.generation == observed.target.generation
                || owner.generation.checked_add(1) == Some(observed.target.generation));
        if !same_execution
            || !prior
                .process_witness
                .as_ref()
                .is_some_and(|w| w.proves_absent(&owner))
        {
            return Err(anyhow::anyhow!(
                "failed startup lacks exact incumbent absence provenance"
            ));
        }
        if !self.startup_still_current(prior, &observed.target) {
            return Ok(false);
        }
        let witness = ExecutionProcessWitness {
            target: owner,
            attempted_target: (observed.target.generation != prior.generation)
                .then(|| observed.target.clone()),
            group_id: observed.group_id,
            quiesced: observed.quiesced,
        };
        self.state.update_session(&prior.session_id, |record| {
            record.process_witness = Some(witness)
        })?;
        Ok(true)
    }
    /// Exact staging fence shared by failed cleanup and successful attachment.
    /// Closing the incumbent is allowed here only so late cleanup can record
    /// proof; the attachment caller separately refuses closed records.
    pub(crate) fn startup_still_current(
        &self,
        prior: &crate::state::SessionRecord,
        attempted: &CodingSessionTarget,
    ) -> bool {
        let owner = prior.target(&self.config.instance_id);
        let Some(current) = self.state.session(&owner.session_id) else {
            return false;
        };
        current.target(&self.config.instance_id) == owner
            && current.resume_cursor == prior.resume_cursor
            && current.generation_command_id == prior.generation_command_id
            && current.execution_binding == prior.execution_binding
            && current.provider_instance_ref == prior.provider_instance_ref
            && current.actor == prior.actor
            && current.project_ref == prior.project_ref
            && current.channel_id == prior.channel_id
            && self.sessions.handle(&owner.session_id).is_none()
            && current.process_witness.as_ref().is_some_and(|w| {
                w.target == *attempted
                    && w.group_id.is_none()
                    && !w.quiesced
                    && w.attempted_target.is_none()
            })
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
