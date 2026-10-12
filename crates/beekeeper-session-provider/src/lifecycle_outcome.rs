//! Existing lifecycle disposition journal: reserved starts and exact accepted answers.
use super::*;
use payload::CREATE_START_OUTCOME_UNCONFIRMED;
use state::{LifecycleStartIntent, LifecycleStartPhase};
impl Provider {
    pub(crate) fn lifecycle_event(
        &self,
        channel: Uuid,
        receipt: &LifecycleReceipt,
    ) -> anyhow::Result<Event> {
        Ok(build_coding_session_lifecycle_receipt(
            channel,
            &receipt.command_id,
            &serde_json::to_string(receipt)?,
        )?
        .sign_with_keys(&self.config.keys)?)
    }
    pub(crate) fn reserve_create_start(
        &mut self,
        plan: &CreatePlan,
    ) -> anyhow::Result<CodingSessionTarget> {
        if let Some(intent) = self.state.lifecycle_start(&plan.command_id) {
            if intent.fingerprint != plan.request_fingerprint
                || intent.phase != LifecycleStartPhase::BeforeEffects
            {
                return Err(anyhow::anyhow!(
                    "create operation reservation conflicts or has uncertain effects"
                ));
            }
            return Ok(intent.target);
        }
        let descriptor = self
            .config
            .runtime(&plan.runtime_instance_ref)
            .ok_or_else(|| anyhow::anyhow!("create runtime missing before reservation"))?;
        let target = CodingSessionTarget {
            driver: descriptor.driver.clone(),
            instance_id: self.config.instance_id.clone(),
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
        let intent = LifecycleStartIntent {
            phase: LifecycleStartPhase::BeforeEffects,
            fingerprint: plan.request_fingerprint.clone(),
            target: target.clone(),
            actor: plan.actor.clone(),
            channel: plan.channel_id,
            provider_ref: plan.runtime_instance_ref.clone(),
            runtime: descriptor.runtime.clone(),
            initial_turn: plan.initial_turn.is_some(),
        };
        let mut receipt=LifecycleReceipt::failed(&plan.command_id,CREATE_START_OUTCOME_UNCONFIRMED,"startup completion was not established after interruption; inspect the reserved responsibility and repair custody before retrying");
        receipt.session = Some(target.clone());
        let event = self.lifecycle_event(plan.channel_id, &receipt)?;
        self.state.stage_terminal_disposition(
            &plan.command_id,
            state::TerminalDisposition {
                semantic_key: coding_session_lifecycle_receipt_semantic_key(&plan.command_id),
                event,
                lifecycle_start: Some(intent),
            },
        )?;
        Ok(target)
    }
    pub(crate) fn accepted_create_disposition(
        &self,
        plan: &CreatePlan,
        target: &CodingSessionTarget,
    ) -> anyhow::Result<state::TerminalDisposition> {
        let mut intent = self
            .state
            .lifecycle_start(&plan.command_id)
            .ok_or_else(|| anyhow::anyhow!("accepted create reservation missing"))?;
        if plan.initial_turn.is_none() {
            return Ok(state::TerminalDisposition {
                semantic_key: coding_session_lifecycle_receipt_semantic_key(&plan.command_id),
                event: self.lifecycle_event(
                    plan.channel_id,
                    &LifecycleReceipt::created(&plan.command_id, target),
                )?,
                lifecycle_start: None,
            });
        }
        intent.phase = LifecycleStartPhase::RuntimeStarting;
        let mut receipt=LifecycleReceipt::failed(&plan.command_id,CREATE_START_OUTCOME_UNCONFIRMED,"the accepted execution exists but initial-turn delivery was interrupted; no duplicate turn or context is started by recovery");
        receipt.session = Some(target.clone());
        Ok(state::TerminalDisposition {
            semantic_key: coding_session_lifecycle_receipt_semantic_key(&plan.command_id),
            event: self.lifecycle_event(plan.channel_id, &receipt)?,
            lifecycle_start: Some(intent),
        })
    }
    pub(crate) fn project_lifecycle_outcome(
        &mut self,
        channel: Uuid,
        command: &str,
        receipt: &LifecycleReceipt,
    ) -> anyhow::Result<()> {
        let old = self
            .state
            .terminal_dispositions()
            .into_iter()
            .find(|(id, _)| id == command)
            .map(|(_, value)| value);
        let mut final_receipt = receipt.clone();
        let mut intent = None;
        if let Some(start) = old.as_ref().and_then(|old| old.lifecycle_start.as_ref()) {
            if start.phase == LifecycleStartPhase::RuntimeStarting
                && receipt.status == payload::ReceiptStatus::Failed
            {
                final_receipt = payload::decode_coding_session_lifecycle_receipt(
                    &old.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("start outcome missing"))?
                        .event
                        .content,
                )
                .map_err(anyhow::Error::msg)?;
                if let Some(cause) = receipt
                    .error
                    .as_ref()
                    .filter(|cause| cause.code != CREATE_START_OUTCOME_UNCONFIRMED)
                {
                    if let Some(error) = final_receipt.error.as_mut() {
                        error.message = format!(
                            "startup failed ({}): {}; physical startup outcome remains unconfirmed; inspect the reserved responsibility before repair",
                            cause.code, cause.message,
                        );
                    }
                }
                intent = Some(start.clone());
            }
        }
        let content = serde_json::to_string(&final_receipt)?;
        let event = if let Some(old) = old.as_ref().filter(|old| old.event.content == content) {
            old.event.clone()
        } else {
            self.lifecycle_event(channel, &final_receipt)?
        };
        self.state.replace_lifecycle_outcome(
            command,
            state::TerminalDisposition {
                semantic_key: coding_session_lifecycle_receipt_semantic_key(command),
                event,
                lifecycle_start: intent,
            },
        )?;
        self.flush_terminal_dispositions()
    }
    pub(crate) fn publish_uncertain_start(
        &mut self,
        intent: &LifecycleStartIntent,
    ) -> anyhow::Result<()> {
        let mut metadata = self.metadata_for(&intent.target, SessionStatus::Disconnected);
        metadata.agent_ref = intent.actor.clone();
        metadata.project_ref = None;
        metadata.repo_ref = None;
        metadata.role = None;
        metadata.provider = Some(
            ProviderInstanceAlias::from_wire(intent.provider_ref.clone())
                .map_err(anyhow::Error::msg)?,
        );
        metadata.runtime =
            Some(RuntimeWord::from_wire(intent.runtime.clone()).map_err(anyhow::Error::msg)?);
        let event = build_coding_session_metadata(
            intent.channel,
            &intent.target,
            &serde_json::to_string(&metadata)?,
        )?
        .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue_latest(
            KIND_CODING_SESSION_METADATA,
            &coding_session_metadata_semantic_key(&intent.target),
            Priority::Live,
            event,
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "lifecycle_crash_tests.rs"]
mod tests;
