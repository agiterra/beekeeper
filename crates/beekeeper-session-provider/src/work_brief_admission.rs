//! Assignment-only admission, before construction or submission of a turn.
use super::*;

impl Provider {
    /// Persist a bounded refusal using the existing retryable terminal outbox.
    /// Success means prepared, not submitted or confirmed by the adapter.
    pub(crate) fn admit_work_brief(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        prepared: Result<Option<String>, work_brief::WorkBriefRefusal>,
    ) -> anyhow::Result<Result<Option<String>, TurnDisposition>> {
        // Defense at the submission boundary, independent of the renderer.
        let prepared = prepared.and_then(|brief| {
            if brief
                .as_ref()
                .is_some_and(|text| text.len() > work_brief::WORK_BRIEF_BUDGET_BYTES)
            {
                Err(work_brief::WorkBriefRefusal::TooLarge)
            } else {
                Ok(brief)
            }
        });
        match prepared {
            Ok(brief) => Ok(Ok(brief)),
            Err(failure) => {
                let receipt = LifecycleReceipt::turn_refused(
                    command_id,
                    target,
                    failure.code(),
                    &failure.to_string(),
                );
                self.refuse_with_staged_answer(channel_id, command_id, &receipt, None)?;
                Ok(Err(TurnDisposition::Answered(failure.code().to_owned())))
            }
        }
    }

    /// A reconstructed native-steer fallback must not deliver an assignment
    /// pointer after losing its prepared frame. No retrieval is presumed.
    pub(crate) fn admit_fallback_work_brief(
        &mut self,
        attempt: &SteerAttemptRecord,
        framing: Option<&session::TurnFraming>,
    ) -> anyhow::Result<bool> {
        if verification_input::assignment_pointer(&attempt.text).is_none() {
            return Ok(true);
        }
        let brief = framing
            .and_then(|frame| frame.work_brief.clone())
            .ok_or(work_brief::WorkBriefRefusal::Unavailable)
            .map(Some);
        if self
            .admit_work_brief(
                attempt.channel_id,
                &attempt.command_id,
                &attempt.target,
                brief,
            )?
            .is_ok()
        {
            return Ok(true);
        }
        self.state
            .resolve_steer_attempt(&attempt.attempt_id, SteerDisposition::Prevented, None)?;
        self.in_flight.remove(&attempt.command_id);
        if let Some(handle) = self.sessions.handle(&attempt.session_id) {
            handle.acknowledge_turn_started(&attempt.command_id);
        }
        Ok(false)
    }

    pub(crate) async fn assignment_work_brief(
        &mut self,
        target: &CodingSessionTarget,
        command_id: &str,
        text: &str,
    ) -> Result<Option<String>, work_brief::WorkBriefRefusal> {
        let Some(operation_id) = verification_input::assignment_pointer(text) else {
            return Ok(None);
        };
        let unavailable = work_brief::WorkBriefRefusal::Unavailable;
        let record = self.state.session(&target.session_id).ok_or(unavailable)?;
        if self.target_for(record) != *target {
            return Err(unavailable);
        }
        // turn_framing requires the founder. Without it its None result
        // would discard a prepared brief while still sending the pointer.
        record.founder_pubkey.as_ref().ok_or(unavailable)?;
        let role = record.role.clone().ok_or(unavailable)?;
        let actor = record.actor.clone().ok_or(unavailable)?;
        let session_ref = record.session_ref.clone().ok_or(unavailable)?;
        let genesis_ref = record.genesis_ref.clone().ok_or(unavailable)?;
        let runtime = work_brief::RuntimeFacts {
            runtime: record.runtime.clone(),
            model: record.model.clone(),
            model_source: Self::model_source_words(record),
            compose_app_version: record
                .compose_ref
                .as_ref()
                .map(|compose| compose.app_version.clone()),
            compose_digest: record
                .compose_ref
                .as_ref()
                .map(|compose| compose.digest.clone()),
        };
        let scope = team_wake::WakeScope {
            channel_ref: record.channel_id,
            session_ref,
            genesis_ref,
        };
        let project_ref = record.project_ref.clone();
        let recorded_cwd = record.cwd.clone();
        let binding = record.execution_binding.clone();
        let rest = self.rest_client.clone().ok_or(unavailable)?;
        let relay_self = self.relay_self.clone().ok_or(unavailable)?;
        let cwd = gate_cwd::resolve(
            self.config.projects_file.as_deref(),
            &target.session_id,
            &recorded_cwd,
        )
        .present()
        .map(Path::to_path_buf);
        work_brief_collect::collect_work_brief(work_brief_collect::WorkBriefRequest {
            rest: &rest,
            relay_self: &relay_self,
            scope: &scope,
            operation_id: &operation_id,
            command_id,
            actor: &actor,
            role: &role,
            project_ref: project_ref.as_deref(),
            projects_file: self.config.projects_file.as_deref(),
            state_dir: &self.config.state_dir,
            session_id: &target.session_id,
            cwd,
            runtime,
            binding,
        })
        .await
    }
}
