//! Authorized public behavior, applied through the existing turn preface.
use super::*;

impl Provider {
    /// Reconcile the authorized public behavior at turn admission, then retain
    /// that exact revision with this request for deterministic recovery.
    pub(crate) async fn conversation_instructions(
        &mut self,
        command_id: &str,
        operator: &str,
    ) -> Result<Option<String>, crate::session::CreateFailure> {
        let fail = |code| {
            crate::execution_scope::refuse(code,
            "this participant's authorized definition is unavailable or cannot be applied safely; no conversation turn was admitted")
        };
        let mut origins = self
            .conversation_origins()
            .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let Some(origin) = origins.requests.get(command_id).cloned() else {
            return Ok(None);
        };
        if origin.definition_event_id.is_some() && self.state.is_command_consumed(command_id) {
            return Ok(origin.instructions);
        }
        let actor = nostr::PublicKey::from_hex(&origin.actor)
            .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let tag = fs::read_to_string(
            self.config
                .state_dir
                .join("actors")
                .join(actor.to_hex())
                .join("auth-tag.json"),
        )
        .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let owner = beekeeper_sdk::nip_oa::verify_auth_tag(&tag, &actor)
            .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let rest = self
            .rest_client
            .as_ref()
            .ok_or_else(|| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let rows = rest
            .query(&[nostr::Filter::new()
                .kind(Kind::Custom(
                    beekeeper_core::kind::KIND_MANAGED_AGENT as u16,
                ))
                .author(owner)
                .identifier(actor.to_hex())])
            .await
            .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let events = rows
            .as_array()
            .ok_or_else(|| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let event = events
            .iter()
            .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
            .filter(|event| {
                event.pubkey == owner
                    && event.kind == Kind::Custom(beekeeper_core::kind::KIND_MANAGED_AGENT as u16)
                    && event.verify().is_ok()
                    && event.tags.iter().any(|tag| {
                        let parts = tag.as_slice();
                        parts.first().is_some_and(|v| v == "d")
                            && parts.get(1).is_some_and(|v| v == &actor.to_hex())
                    })
            })
            .max_by_key(|event| (event.created_at, std::cmp::Reverse(event.id)))
            .ok_or_else(|| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        let definition: beekeeper_core::managed_agent::ManagedAgentEventContent =
            serde_json::from_str(&event.content)
                .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        beekeeper_core::managed_agent::validate_managed_agent_definition_text(
            &definition.name,
            definition.persona_id.as_deref(),
            definition.system_prompt.as_deref(),
        )
        .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        use beekeeper_core::managed_agent::RespondTo;
        let permitted = match definition.respond_to {
            RespondTo::OwnerOnly => operator == owner.to_hex(),
            RespondTo::Allowlist => definition
                .respond_to_allowlist
                .iter()
                .any(|p| p == operator),
            RespondTo::Anyone => true,
        };
        if !permitted {
            return Err(fail("UNAUTHORIZED_OPERATOR"));
        }
        let record = self
            .state
            .session(&origin.target.session_id)
            .ok_or_else(|| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        if definition.persona_id.is_none()
            && definition
                .provider
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty())
        {
            return Err(fail("AGENT_PROVIDER_UPDATE_PENDING"));
        }
        let mut linked_id = None;
        let mut prompt = definition.system_prompt.clone().unwrap_or_default();
        let mut desired_model = definition.model.as_deref();
        let linked;
        if let Some(persona_id) = definition.persona_id.as_deref() {
            let rows = rest
                .query(&[nostr::Filter::new()
                    .kind(Kind::Custom(beekeeper_core::kind::KIND_PERSONA as u16))
                    .author(owner)
                    .identifier(persona_id)])
                .await
                .map_err(|_| fail("AGENT_DEFINITION_PACK_UNAVAILABLE"))?;
            let head = rows
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
                .filter(|candidate| {
                    candidate.pubkey == owner
                        && candidate.kind == Kind::Custom(beekeeper_core::kind::KIND_PERSONA as u16)
                        && candidate.verify().is_ok()
                        && candidate.tags.iter().any(|tag| {
                            let parts = tag.as_slice();
                            parts.first().is_some_and(|v| v == "d")
                                && parts.get(1).is_some_and(|v| v == persona_id)
                        })
                })
                .max_by_key(|candidate| (candidate.created_at, std::cmp::Reverse(candidate.id)))
                .ok_or_else(|| fail("AGENT_DEFINITION_PACK_UNAVAILABLE"))?;
            // Only the existing shared projection is supported. A protected
            // owner-only pack is never read using an owner's credential.
            if !head
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["shared", "true"])
            {
                return Err(fail("AGENT_DEFINITION_PACK_UNAVAILABLE"));
            }
            linked =
                serde_json::from_str::<beekeeper_core::persona::PersonaEventContent>(&head.content)
                    .map_err(|_| fail("AGENT_DEFINITION_PACK_UNAVAILABLE"))?;
            if linked
                .runtime
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .is_some_and(|runtime| runtime != record.runtime)
            {
                return Err(fail("AGENT_RUNTIME_UPDATE_PENDING"));
            }
            if linked
                .provider
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty())
            {
                return Err(fail("AGENT_PROVIDER_UPDATE_PENDING"));
            }
            // Runtime/provider custody remains the accepted session's host
            // binding. A pack cannot switch account or inject environment.
            prompt = linked.system_prompt.clone().unwrap_or_default();
            desired_model = linked.model.as_deref().filter(|v| !v.trim().is_empty());
            linked_id = Some(head.id.to_hex());
        }
        if desired_model.is_some_and(|desired| {
            record
                .model_requested
                .as_deref()
                .or(record.model.as_deref())
                != Some(desired)
        }) {
            return Err(fail("AGENT_MODEL_UPDATE_PENDING"));
        }
        // Applying newly changed instructions inside a live turn would change
        // responsibility mid-flight; the caller must retry at its safe boundary.
        if let Some(open) = record.open_turn.as_ref() {
            let running_origin = open
                .command_id
                .as_deref()
                .and_then(|id| origins.requests.get(id));
            if running_origin.is_none_or(|request| {
                request.definition_event_id.as_deref() != Some(event.id.to_hex().as_str())
                    || request.linked_definition_event_id != linked_id
            }) {
                return Err(fail("AGENT_DEFINITION_UPDATE_PENDING"));
            }
        }
        let instructions = format!(
            "Participant definition {} (linked {}, owner {}, actor {}):\n{}",
            event.id.to_hex(),
            linked_id.as_deref().unwrap_or("none"),
            owner.to_hex(),
            actor.to_hex(),
            prompt
        );
        if let Some(request) = origins.requests.get_mut(command_id) {
            request.definition_event_id = Some(event.id.to_hex());
            request.linked_definition_event_id = linked_id;
            request.instructions = Some(instructions.clone());
        }
        self.save_conversation_origins(&origins)
            .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        Ok(Some(instructions))
    }
}
