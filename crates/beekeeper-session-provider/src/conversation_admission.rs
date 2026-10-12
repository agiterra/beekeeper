//! Ordinary author admission uses the existing ACP predicate and turn machinery.
use super::*;

impl Provider {
    pub(crate) async fn admit_conversation_event(
        &mut self,
        channel: Uuid,
        event: &Event,
    ) -> anyhow::Result<Option<String>> {
        event.verify()?;
        if event.kind != Kind::Custom(9)
            || (self.state.is_command_consumed(&event.id.to_hex())
                || self.state.is_command_refused(&event.id.to_hex()))
        {
            return Ok(None);
        }
        let declined_key = format!("conversation:{}:replay-declined", event.id);
        if self
            .outbox
            .contains(beekeeper_core::kind::KIND_STREAM_MESSAGE, &declined_key)
        {
            self.state
                .consume_command(&event.id.to_hex(), crate::now_secs())?;
            return Ok(None);
        }
        if !event.tags.iter().any(|tag| {
            tag.as_slice().first().is_some_and(|v| v == "h")
                && tag.as_slice().get(1) == Some(&channel.to_string())
        }) {
            return Ok(None);
        }
        let routes: ConversationRoutes =
            load(&self.config.state_dir.join("conversation-routes.json"))?;
        let origins = self.conversation_origins()?;
        if origins.legacy_answers.contains_key(&event.id.to_hex()) {
            self.state
                .consume_command(&event.id.to_hex(), crate::now_secs())?;
            return Ok(None);
        }
        let root = root(event);
        let actors: std::collections::BTreeSet<_> = routes
            .routes
            .iter()
            .filter(|route| {
                route.channel_id == channel
                    && event.pubkey.to_hex() != route.actor_pubkey
                    && (event.tags.iter().any(|tag| {
                        tag.as_slice().first().is_some_and(|v| v == "p")
                            && tag.as_slice().get(1) == Some(&route.actor_pubkey)
                    }) || origins
                        .bindings
                        .contains_key(&format!("{channel}:{}:{root}", route.actor_pubkey)))
            })
            .map(|route| route.actor_pubkey.clone())
            .collect();
        if actors.len() != 1 {
            return Ok(None);
        }
        let Some(actor) = actors.into_iter().next() else {
            return Ok(None);
        };
        let Some(route) = routes
            .routes
            .iter()
            .find(|route| route.channel_id == channel && route.actor_pubkey == actor)
        else {
            return Ok(None);
        };
        #[cfg(test)]
        let context = self
            .conversation_ownership_context
            .clone()
            .or(crate::host_ownership::context()
                .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?);
        #[cfg(not(test))]
        let context = crate::host_ownership::context()
            .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?;
        let ownership = context
            .as_ref()
            .map(|(home, instance)| {
                crate::host_ownership::conversation_ownership(
                    home,
                    *instance,
                    &actor,
                    &self.config.relay_url,
                )
            })
            .transpose()
            .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?;
        if ownership
            .as_ref()
            .is_some_and(|owner| owner.historical_setup)
        {
            self.state
                .consume_command(&event.id.to_hex(), crate::now_secs())?;
            return Ok(None);
        }
        // No pair lock crosses an async owner read. Synchronous routing reacquires
        // it before disposition and runtime admission has its own startup fence.
        drop(ownership);
        // Verify ordinary policy before disclosing any generic responsibility count.
        match self
            .conversation_author_allowed(&actor, channel, &event.pubkey.to_hex())
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                self.state
                    .consume_command(&event.id.to_hex(), crate::now_secs())?;
                return Ok(None);
            }
            Err(failure) => {
                let origin = Origin {
                    actor,
                    channel,
                    root,
                    event: event.id.to_hex(),
                    author: Some(event.pubkey.to_hex()),
                    target: route.target.clone(),
                    definition_event_id: None,
                    linked_definition_event_id: None,
                    private_definition_event_id: None,
                    instructions: None,
                };
                let command_id = event.id.to_hex();
                let mut origins = self.conversation_origins()?;
                origins.requests.insert(command_id.clone(), origin);
                self.save_conversation_origins(&origins)?;
                let receipt = crate::payload::LifecycleReceipt::turn_refused(
                    &command_id,
                    &route.target,
                    failure.code,
                    "This host could not verify ordinary conversation admission. This host has not executed this request.",
                );
                self.refuse_with_staged_answer(channel, &command_id, &receipt, None)?;
                return Ok(None);
            }
        }
        if let Some(floor) = route.replay_floor_unix {
            // An existing host origin already has its admission/replay owner.
            if !origins.requests.contains_key(&event.id.to_hex())
                && (event.created_at.as_secs() < floor
                    || crate::now_secs().saturating_sub(event.created_at.as_secs())
                        > self.config.command_horizon.as_secs())
            {
                let origin = Origin {
                    actor: actor.clone(),
                    channel,
                    root,
                    event: event.id.to_hex(),
                    author: Some(event.pubkey.to_hex()),
                    target: route.target.clone(),
                    definition_event_id: None,
                    linked_definition_event_id: None,
                    private_definition_event_id: None,
                    instructions: None,
                };
                let message = if event.created_at.as_secs() < floor {
                    "This request predates the responsibility replay boundary. Its earlier outcome is uncertain. This host has not replayed it; send a new request if work is still needed."
                } else {
                    "RESPONSIBILITY_REPLAY_TOO_OLD: This request exceeds the existing command admission horizon. This host has not replayed it; send a new request if work is still needed."
                };
                self.conversation_reply(
                    &origin,
                    &format!("{}:replay-declined", event.id),
                    message,
                )?;
                self.state
                    .consume_command(&event.id.to_hex(), crate::now_secs())?;
                return Ok(None);
            }
        }
        self.conversation_command_authorized_at(
            channel,
            event,
            context
                .as_ref()
                .map(|(home, instance)| (home.as_path(), *instance)),
            Some(&actor),
        )
    }
}
