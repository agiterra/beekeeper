//! Effect boundaries inside the existing durable lifecycle disposition journal.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleStartPhase {
    BeforeEffects,
    EffectsStarted,
    RuntimeStarting,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleStartIntent {
    pub phase: LifecycleStartPhase,
    pub fingerprint: String,
    pub target: CodingSessionTarget,
    pub actor: Option<String>,
    pub channel: uuid::Uuid,
    pub provider_ref: String,
    pub runtime: String,
    pub initial_turn: bool,
}
pub(crate) fn lifecycle_fingerprint(channel: uuid::Uuid, operator: &str, content: &str) -> String {
    hex::encode(Sha256::digest(
        format!("{channel}\n{operator}\n{content}").as_bytes(),
    ))
}
impl StateStore {
    pub(crate) fn lifecycle_start(&self, command: &str) -> Option<LifecycleStartIntent> {
        self.snapshot
            .terminal_dispositions
            .get(command)
            .and_then(|value| value.lifecycle_start.clone())
    }
    pub(crate) fn retry_before_effects(&self, command: &str, fingerprint: &str) -> bool {
        self.lifecycle_start(command).is_some_and(|intent| {
            intent.phase == LifecycleStartPhase::BeforeEffects && intent.fingerprint == fingerprint
        })
    }
    pub(crate) fn unresolved_actor_start(&self, actor: Option<&str>) -> bool {
        self.snapshot.terminal_dispositions.values().any(|value| {
            value.lifecycle_start.as_ref().is_some_and(|intent| {
                intent.actor.as_deref() == actor
                    && intent.phase != LifecycleStartPhase::BeforeEffects
            })
        })
    }
    pub(crate) fn unresolved_start_target(&self, target: &CodingSessionTarget) -> bool {
        self.snapshot.terminal_dispositions.values().any(|value| {
            value.lifecycle_start.as_ref().is_some_and(|intent| {
                intent.target == *target && intent.phase != LifecycleStartPhase::BeforeEffects
            })
        })
    }
    pub(crate) fn advance_lifecycle_start(
        &mut self,
        command: &str,
        phase: LifecycleStartPhase,
    ) -> io::Result<()> {
        #[cfg(test)]
        if phase == LifecycleStartPhase::EffectsStarted
            && std::mem::take(&mut self.fault_plan.fail_next_lifecycle_effects_write)
        {
            return Err(io::Error::other(
                "injected lifecycle effect-boundary write failure",
            ));
        }
        let previous = self.snapshot.clone();
        let intent = self
            .snapshot
            .terminal_dispositions
            .get_mut(command)
            .and_then(|value| value.lifecycle_start.as_mut())
            .ok_or_else(|| io::Error::other("lifecycle start reservation missing"))?;
        intent.phase = phase;
        self.persist_or_restore(previous)
    }
    pub(crate) fn replace_lifecycle_outcome(
        &mut self,
        command: &str,
        outcome: TerminalDisposition,
    ) -> io::Result<()> {
        let previous = self.snapshot.clone();
        self.snapshot
            .terminal_dispositions
            .insert(command.to_owned(), outcome);
        self.persist_or_restore(previous)
    }
    pub(crate) fn insert_session_with_lifecycle_outcome(
        &mut self,
        record: SessionRecord,
        command: &str,
        outcome: TerminalDisposition,
    ) -> io::Result<()> {
        let previous = self.snapshot.clone();
        self.snapshot
            .sessions
            .insert(record.session_id.clone(), record);
        self.snapshot
            .terminal_dispositions
            .insert(command.to_owned(), outcome);
        self.persist_or_restore(previous)
    }
}
