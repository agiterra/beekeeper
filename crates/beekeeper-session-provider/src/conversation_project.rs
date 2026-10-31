//! Owner-configured project context for the existing conversational participant.
//! This is routing information, never a grant or an alternate input consumer.
use nostr::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public coordinates installed by owner management, outside model custody.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectContext {
    /// Canonical project coordinate, matching the accepted responsibility.
    pub project: String,
    /// Existing ordinary conversation channel.
    pub channel: Uuid,
    /// Canonically installed project lead actor.
    pub actor: String,
    /// Exact owner-founded umbrella; no credentials or private context.
    pub genesis: Event,
    /// Exact public selection of the owner-operated hire listener, not a grant.
    #[serde(default)]
    pub hire_selection: Option<HireSelection>,
}
/// Public explicit identity/runtime selection; model strings include effort.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HireSelection {
    /// Worker identity selected by the owner listener.
    pub actor: String,
    /// Installed role accepted by the listener policy.
    pub role: String,
    /// Commissioned provider alias.
    pub provider_instance: String,
    /// Exact model selection, without normalization or fallback.
    pub model: String,
}

impl ProjectContext {
    /// Validate scope and render public existing CLI coordinates. The model
    /// must still possess accepted grants for every operation it chooses.
    pub fn instructions(
        &self,
        owner: &str,
        actor: &str,
        channel: Uuid,
        project: Option<&str>,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.actor == actor
                && self.channel == channel
                && project == Some(self.project.as_str()),
            "project conversation context differs from accepted responsibility"
        );
        anyhow::ensure!(
            self.genesis.verify().is_ok()
                && self.genesis.pubkey.to_hex() == owner
                && self.genesis.kind.as_u16() as u32
                    == beekeeper_core::kind::KIND_CODING_SESSION_GENESIS,
            "project conversation genesis is not owner-signed"
        );
        let payload = beekeeper_core::coding_session_genesis::decode_coding_session_genesis(
            &self.genesis.content,
        )
        .map_err(anyhow::Error::msg)?;
        let expected = beekeeper_sdk::build_coding_session_genesis(channel, &payload)?
            .build(self.genesis.pubkey);
        anyhow::ensure!(
            self.genesis.tags == expected.tags,
            "project conversation genesis channel differs"
        );
        let selection = match &self.hire_selection {
            Some(selection) => format!("Owner listener permitted selection: {}. Use these exact values including any model effort suffix, or omit --model to use this selected identity's model. Include --reply-to <source-event-id> on sessions hire using the exact source in this turn; this is the Host refusal return address. A catalog default does not override listener policy.", serde_json::to_string(selection)?),
            None => "HIRE_SELECTION_UNAVAILABLE: the owner listener has not supplied an explicit permitted selection. Do not guess a model or spend a turn polling for setup.".into(),
        };
        Ok(format!("{selection} Configured shared project {}. Existing delegation scope: --channel {} --session-ref {} --genesis {}. Use this scope for project work if delegation is useful; it is context, not authority. The owner service supervises bounded workers. Do not ask the person to operate session UI or repeat these coordinates. This conversational actor has no role-addressable execution: do not use sessions report --wake-to lead to return work here. Include the source request and thread from this turn, and this return address, in both the hire brief and assignment: after publishing the structured report without --wake-to, send one ordinary message with bee messages send --channel {} --mention {} --reply-to <source-thread-id> --content <report-and-assignment-event-IDs-and-evidence-summary>. Report acceptance is not notification delivery or assessment. Retain accepted IDs; on uncertainty inspect existing records and retry exact signed events, never recreate reports or notifications blindly. Read the referenced report and assess evidence before answering in this conversation. Explain in the hire brief that the canonical assignment wake follows: the worker should end its initial turn if it has no assignment yet, without requesting the reference or starting a clarification exchange. Publish the assignment and its wake through existing operations; do not poll with model turns.", self.project, channel, payload.session_ref, self.genesis.id, channel, actor))
    }
}

/// Resolve a requester-signed return reference against this Host's admitted
/// conversation ledger. The caller must separately verify project authority.
pub fn admitted_reply_origin(
    state_dir: &std::path::Path,
    actor: &str,
    channel: Uuid,
    source: &str,
    project: &str,
    genesis: &str,
) -> anyhow::Result<beekeeper_sdk::ThreadRef> {
    crate::conversation::admitted_reply_origin(state_dir, actor, channel, source, project, genesis)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_context_is_bound_to_owner_actor_channel_and_project() {
        let owner = nostr::Keys::generate();
        let actor = nostr::Keys::generate().public_key().to_hex();
        let channel = Uuid::new_v4();
        let payload = beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload::new(
            Uuid::new_v4().to_string(),
        );
        let genesis = beekeeper_sdk::build_coding_session_genesis(channel, &payload)
            .unwrap()
            .sign_with_keys(&owner)
            .unwrap();
        let context = ProjectContext {
            project: format!("30621:{}:fixture", owner.public_key()),
            channel,
            actor: actor.clone(),
            genesis,
            hire_selection: Some(HireSelection {
                actor: actor.clone(),
                role: "runner".into(),
                provider_instance: "fixture-primary".into(),
                model: "fixture-model[low]".into(),
            }),
        };
        let owner_hex = owner.public_key().to_hex();
        let text = context
            .instructions(&owner_hex, &actor, channel, Some(&context.project))
            .unwrap();
        assert!(text.contains(&context.genesis.id.to_hex()));
        assert!(text.contains(&payload.session_ref));
        assert!(text.contains("fixture-model[low]"));
        assert!(text.contains("fixture-primary"));
        assert!(text.contains("--reply-to <source-event-id>"));
        assert!(
            text.contains("--mention"),
            "delegation must carry an actor-addressed chat return path"
        );
        assert!(
            text.contains(&actor),
            "return address must name the configured actor"
        );
        assert!(
            text.contains("--reply-to"),
            "report notification must retain the source thread"
        );
        assert!(
            text.contains("without --wake-to"),
            "ordinary lead is not a role-addressable seat"
        );
        assert!(
            text.contains("assignment wake"),
            "hire briefing must not induce a clarification round trip"
        );
        assert!(context
            .instructions(&actor, &actor, channel, Some(&context.project))
            .is_err());
        assert!(context
            .instructions(&owner_hex, &owner_hex, channel, Some(&context.project))
            .is_err());
        assert!(context
            .instructions(&owner_hex, &actor, Uuid::new_v4(), Some(&context.project))
            .is_err());
        assert!(context
            .instructions(&owner_hex, &actor, channel, Some("other"))
            .is_err());
    }
}
