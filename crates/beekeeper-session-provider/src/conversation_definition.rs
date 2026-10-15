//! Authorized public behavior, applied through the existing turn preface.
use super::*;

fn pinned_definition_matches(
    request: &Origin,
    definition_id: &str,
    linked_id: Option<&str>,
    private_id: Option<&str>,
) -> bool {
    request.definition_event_id.as_deref() == Some(definition_id)
        && request.linked_definition_event_id.as_deref() == linked_id
        && request.private_definition_event_id.as_deref() == private_id
}

impl Provider {
    pub(super) async fn conversation_definition_head(
        &self,
        actor_hex: &str,
    ) -> Result<
        (
            nostr::PublicKey,
            Event,
            beekeeper_core::managed_agent::ManagedAgentEventContent,
        ),
        crate::session::CreateFailure,
    > {
        let fail = |code| {
            crate::execution_scope::refuse(code, "the participant's authorized definition is unavailable; no conversation turn was admitted")
        };
        let actor = nostr::PublicKey::from_hex(actor_hex)
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
        Ok((owner, event.clone(), definition))
    }

    /// `authored_at` is the signed input's `created_at` at admission, so a
    /// sibling's NIP-OA time conditions are judged at the input's own time;
    /// re-checks of an already admitted input pass `None` rather than invent
    /// a later timestamp.
    pub(crate) async fn conversation_author_allowed(
        &self,
        actor_hex: &str,
        channel: Uuid,
        author: &str,
        authored_at: Option<u64>,
    ) -> Result<bool, crate::session::CreateFailure> {
        let (owner, _, definition) = self.conversation_definition_head(actor_hex).await?;
        let settings = crate::managed_configuration::Settings::load(&self.config, Some(actor_hex))?;
        let enforced_owner_only = match settings
            .as_ref()
            .and_then(|settings| settings.record.get("enforced_owner_only"))
        {
            None => false,
            Some(serde_json::Value::Bool(value)) => *value,
            _ => {
                return Err(crate::execution_scope::refuse(
                    "AGENT_PRIVATE_CONFIG_UNAVAILABLE",
                    "private author policy is malformed; no conversation turn was admitted",
                ))
            }
        };
        let rest = self.rest_client.as_ref().ok_or_else(|| {
            crate::execution_scope::refuse(
                "AGENT_DEFINITION_UNAVAILABLE",
                "definition transport unavailable",
            )
        })?;
        match beekeeper_acp::ordinary_author_decision(
            beekeeper_acp::OrdinaryAuthorPolicy {
                respond_to: definition.respond_to,
                allowlist: &definition.respond_to_allowlist,
                author,
                owner: &owner.to_hex(),
                enforced_owner_only,
                authored_at,
            },
            channel,
            rest,
        )
        .await
        {
            beekeeper_acp::AuthorDecision::Allowed => Ok(true),
            beekeeper_acp::AuthorDecision::Denied => Ok(false),
            // A sibling whose owner attestation has expired is answered, not
            // dropped: "not allowed" must never read as "not running".
            beekeeper_acp::AuthorDecision::AttestationNotInForce => {
                Err(crate::execution_scope::refuse(
                    crate::payload::OWNER_ATTESTATION_EXPIRED,
                    format!(
                        "author {author}'s owner attestation is not in force at this input's time; no turn was admitted"
                    ),
                ))
            }
        }
    }

    pub(crate) async fn conversation_capability(
        &mut self,
        channel: Uuid,
        event_id: &str,
        operator: &str,
        content: &str,
    ) -> Result<Option<crate::commands::ConversationCapability>, crate::session::CreateFailure>
    {
        let failure = || {
            crate::execution_scope::refuse(
                "UNAUTHORIZED_OPERATOR",
                "ordinary author capability does not match its signed origin",
            )
        };
        let Ok(command) = crate::commands::decode_turn_command(content) else {
            return Ok(None);
        };
        if self.state.is_command_consumed(&command.command_id)
            || self.state.is_command_refused(&command.command_id)
        {
            return Ok(None);
        }
        let origins = self.conversation_origins().map_err(|_| failure())?;
        let Some(origin) = origins.requests.get(&command.command_id) else {
            return Ok(None);
        };
        if origin.channel != channel
            || origin.event != event_id
            || origin.event != command.command_id
            || origin.target != command.target
            || origin.author.as_deref() != Some(operator)
        {
            return Ok(None);
        }
        if !self
            .conversation_author_allowed(&origin.actor, channel, operator, None)
            .await?
        {
            return Err(failure());
        }
        Ok(Some(crate::commands::ConversationCapability {
            command_id: command.command_id,
            event_id: event_id.into(),
            target: command.target,
            author: operator.into(),
            channel,
        }))
    }

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
        let (owner, event, definition) = self.conversation_definition_head(&origin.actor).await?;
        if !self
            .conversation_author_allowed(&origin.actor, origin.channel, operator, None)
            .await?
        {
            return Err(fail("UNAUTHORIZED_OPERATOR"));
        }
        let rest = self
            .rest_client
            .as_ref()
            .ok_or_else(|| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
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
        let mut private_id = None;
        let mut prompt = definition.system_prompt.clone().unwrap_or_default();
        let mut desired_model = definition.model.as_deref();
        let linked;
        if let Some(persona_id) = definition.persona_id.as_deref() {
            linked = if let Some((pin, content)) =
                crate::managed_configuration::private_persona::load(
                    &self.config,
                    &origin.actor,
                    &event,
                    owner,
                    persona_id,
                )? {
                private_id = Some(pin.id.to_hex());
                content
            } else {
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
                            && candidate.kind
                                == Kind::Custom(beekeeper_core::kind::KIND_PERSONA as u16)
                            && candidate.verify().is_ok()
                            && candidate.tags.iter().any(|tag| {
                                let parts = tag.as_slice();
                                parts.first().is_some_and(|v| v == "d")
                                    && parts.get(1).is_some_and(|v| v == persona_id)
                            })
                    })
                    .max_by_key(|candidate| (candidate.created_at, std::cmp::Reverse(candidate.id)))
                    .ok_or_else(|| fail("AGENT_DEFINITION_PACK_UNAVAILABLE"))?;
                // Public linked heads retain their existing sharing requirement.
                if !head
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["shared", "true"])
                {
                    return Err(fail("AGENT_DEFINITION_PACK_UNAVAILABLE"));
                }
                let linked = serde_json::from_str::<beekeeper_core::persona::PersonaEventContent>(
                    &head.content,
                )
                .map_err(|_| fail("AGENT_DEFINITION_PACK_UNAVAILABLE"))?;
                linked_id = Some(head.id.to_hex());
                linked
            };
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
                !pinned_definition_matches(
                    request,
                    &event.id.to_hex(),
                    linked_id.as_deref(),
                    private_id.as_deref(),
                )
            }) {
                return Err(fail("AGENT_DEFINITION_UPDATE_PENDING"));
            }
        }
        let instructions = format!(
            "Participant definition {} (linked {}, private pin {}, owner {}, actor {}):\n{}",
            event.id.to_hex(),
            linked_id.as_deref().unwrap_or("none"),
            private_id.as_deref().unwrap_or("none"),
            owner.to_hex(),
            actor.to_hex(),
            prompt
        );
        if let Some(request) = origins.requests.get_mut(command_id) {
            request.definition_event_id = Some(event.id.to_hex());
            request.linked_definition_event_id = linked_id;
            request.private_definition_event_id = private_id;
            request.instructions = Some(instructions.clone());
        }
        self.save_conversation_origins(&origins)
            .map_err(|_| fail("AGENT_DEFINITION_UNAVAILABLE"))?;
        Ok(Some(instructions))
    }
}

#[cfg(test)]
mod private_pin_boundary_tests {
    use super::*;
    fn persisted(private: Option<&str>, linked: Option<&str>) -> Origin {
        serde_json::from_value(serde_json::json!({
            "actor":"fixture-actor","channel":Uuid::nil(),"root":"root","event":"request",
            "target":{"driver":"csp","instanceId":"fixture","sessionId":"session","generation":1},
            "definitionEventId":"accepted30177","linkedDefinitionEventId":linked,
            "privateDefinitionEventId":private,"instructions":"pinned private instructions"
        }))
        .unwrap()
    }
    #[test]
    fn changing_private_pin_cannot_enter_an_open_turn_with_unchanged_public_revision() {
        let original = persisted(Some("private-a"), None);
        let bytes = serde_json::to_vec(&original).unwrap();
        let recovered: Origin = serde_json::from_slice(&bytes).unwrap();
        assert!(pinned_definition_matches(
            &recovered,
            "accepted30177",
            None,
            Some("private-a")
        ));
        assert!(!pinned_definition_matches(
            &recovered,
            "accepted30177",
            None,
            Some("private-b")
        ));
        assert!(!pinned_definition_matches(
            &recovered,
            "accepted30177",
            Some("private-a"),
            None
        ));
        assert_eq!(
            recovered.instructions.as_deref(),
            Some("pinned private instructions")
        );
        assert!(recovered.linked_definition_event_id.is_none());
    }
    #[test]
    fn legacy_public_pin_replay_remains_public_without_a_private_pin_requirement() {
        let original = persisted(None, Some("public30175"));
        let mut value = serde_json::to_value(original).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("privateDefinitionEventId");
        let recovered: Origin = serde_json::from_value(value).unwrap();
        assert!(pinned_definition_matches(
            &recovered,
            "accepted30177",
            Some("public30175"),
            None
        ));
        assert!(!pinned_definition_matches(
            &recovered,
            "accepted30177",
            Some("new-public30175"),
            None
        ));
        assert!(recovered.private_definition_event_id.is_none());
    }
}
