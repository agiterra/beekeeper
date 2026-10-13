//! Explicit profile definition staging; the existing shared update owns execution effects.
use super::*;
use crate::managed_agents::{AgentDefinition, OwnedPersonaUpdateRequest};

pub(super) struct StagedPersonaUpdate {
    pub source: AgentDefinition,
    pub desired: AgentDefinition,
    pub original_pin: Option<Value>,
}
impl StagedPersonaUpdate {
    pub fn prepare(
        record: &ManagedAgentRecord,
        input: &OwnedPersonaUpdateRequest,
        personas: &[AgentDefinition],
        records: &[ManagedAgentRecord],
    ) -> Result<Self, String> {
        let id = record
            .persona_id
            .as_deref()
            .ok_or("owned definition reference absent")?;
        if id != input.definition.id {
            return Err("owned definition edit targets another persona".into());
        }
        let mut matches = personas.iter().filter(|p| p.id == id);
        let source = matches.next().ok_or("owned definition unavailable")?;
        if matches.next().is_some() {
            return Err("owned definition is ambiguous".into());
        }
        if source.is_builtin
            || source.shared
            || source.source_team.is_some()
            || source.catalog_source.is_some()
        {
            return Err("this edit requires an owned private standalone definition".into());
        }
        if records
            .iter()
            .filter(|r| r.persona_id.as_deref() == Some(id))
            .count()
            != 1
        {
            return Err(
                "this definition is linked to other agents; edit their configuration explicitly"
                    .into(),
            );
        }
        let desired =
            crate::commands::personas::update::prospective_persona_edit(source, &input.definition)?;
        // An accepted lost reply may arrive after the one atomic cache commit.
        // Only the exact already-pinned requested payload permits old timestamp replay.
        let desired_hash = crate::managed_agents::persona_events::persona_content_hash(
            &crate::managed_agents::persona_events::persona_event_content(&desired),
        );
        let same_payload = serde_json::to_value(source).map_err(|e| e.to_string())?
            == serde_json::to_value(&desired).map_err(|e| e.to_string())?;
        if source.updated_at != input.expected_updated_at
            && !(same_payload
                && record.persona_source_version.as_deref() == Some(desired_hash.as_str()))
        {
            return Err("owned definition changed; refresh before editing".into());
        }
        Ok(Self {
            source: source.clone(),
            desired,
            original_pin: None,
        })
    }
    pub fn apply_record(&self, record: &mut ManagedAgentRecord) {
        record.system_prompt = Some(self.desired.system_prompt.clone());
        record.persona_source_version =
            Some(crate::managed_agents::persona_events::persona_content_hash(
                &crate::managed_agents::persona_events::persona_event_content(&self.desired),
            ));
    }
    pub fn projected_personas(&self, personas: &[AgentDefinition]) -> Vec<AgentDefinition> {
        personas
            .iter()
            .map(|p| {
                if p.id == self.source.id {
                    self.desired.clone()
                } else {
                    p.clone()
                }
            })
            .collect()
    }
    pub fn assert_current(&self, personas: &[AgentDefinition]) -> Result<(), String> {
        let mut matches = personas.iter().filter(|p| p.id == self.source.id);
        let current = matches
            .next()
            .ok_or("owned definition disappeared before configuration")?;
        if matches.next().is_some()
            || serde_json::to_value(current).map_err(|e| e.to_string())?
                != serde_json::to_value(&self.source).map_err(|e| e.to_string())?
        {
            return Err("owned definition changed before configuration; refresh required".into());
        }
        Ok(())
    }
    pub fn commit_personas(&self, personas: &mut [AgentDefinition]) -> Result<(), String> {
        let current = personas
            .iter_mut()
            .find(|p| p.id == self.source.id)
            .ok_or("Configuration accepted; owned definition cache disappeared")?;
        if serde_json::to_value(&*current).map_err(|e| e.to_string())?
            != serde_json::to_value(&self.source).map_err(|e| e.to_string())?
        {
            return Err("Configuration accepted; owned definition changed before local commit; refresh required".into());
        }
        *current = self.desired.clone();
        current.updated_at = crate::util::now_iso();
        Ok(())
    }
}

#[cfg(test)]
#[path = "host_persona_update_tests.rs"]
mod tests;
