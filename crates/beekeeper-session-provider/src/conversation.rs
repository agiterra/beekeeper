//! Conversation inputs reuse the provider's command admission, replay and runtime.
//! Routes and scoped actor credentials are host-local; no provider key is given
//! to the addressed participant. A binding pins one exact responsibility.
use std::collections::BTreeMap;
use std::fs;

use beekeeper_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery, CodingSessionTarget,
    CODING_SESSION_COMMAND_SCHEMA,
};
use nostr::{Event, Keys, Kind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{publish::Priority, Provider};

#[path = "conversation_admission.rs"]
mod admission;
#[path = "conversation_backfill.rs"]
pub(crate) mod backfill;
#[path = "conversation_definition.rs"]
mod definition;

/// Local routing materialization, installed by authorized host management.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationRoutes {
    /// Accepted participant responsibilities available in each transport.
    pub routes: Vec<ConversationRoute>,
}

/// An actor's existing accepted execution, never an arbitrary launch command.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationRoute {
    /// The participant identity; must match the accepted session actor.
    pub actor_pubkey: String,
    /// Authorized conversation transport; must match the session channel.
    pub channel_id: Uuid,
    /// Exact accepted execution and generation.
    pub target: CodingSessionTarget,
    /// Explicit lower bound for this responsibility’s replay.
    /// This is not evidence that an earlier request was consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_floor_unix: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Origin {
    actor: String,
    channel: Uuid,
    root: String,
    event: String,
    #[serde(default)]
    author: Option<String>,
    target: CodingSessionTarget,
    #[serde(default)]
    definition_event_id: Option<String>,
    #[serde(default)]
    linked_definition_event_id: Option<String>,
    #[serde(default)]
    private_definition_event_id: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Origins {
    #[serde(default)]
    legacy_answers: BTreeMap<String, String>,
    requests: BTreeMap<String, Origin>,
    bindings: BTreeMap<String, CodingSessionTarget>,
}

fn load<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> anyhow::Result<T> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

fn root(event: &Event) -> String {
    let marked = |marker: &str| {
        event.tags.iter().find_map(|tag| {
            let values = tag.as_slice();
            (values.first().is_some_and(|v| v == "e") && values.get(3).is_some_and(|v| v == marker))
                .then(|| values.get(1).cloned())
                .flatten()
        })
    };
    marked("root")
        .or_else(|| marked("reply"))
        .unwrap_or_else(|| event.id.to_hex())
}

impl Provider {
    /// Persistent custody is available only for explicitly routed ordinary
    /// participants. Legacy role seats keep their one-shot command handoff.
    pub(crate) fn conversation_actor_seat(
        &self,
        record: &crate::state::SessionRecord,
    ) -> anyhow::Result<Option<crate::actor_seats::ActorSeat>> {
        let Some(actor) = record.actor.as_deref() else {
            return Ok(None);
        };
        if record.role.is_some() || record.pack_ref.is_some() {
            return Ok(None);
        }
        let actor_key = nostr::PublicKey::from_hex(actor)?;
        let routes: ConversationRoutes =
            load(&self.config.state_dir.join("conversation-routes.json"))?;
        if !routes.routes.iter().any(|route| {
            route.actor_pubkey == actor
                && route.channel_id == record.channel_id
                && route.target == self.target_for(record)
        }) {
            return Ok(None);
        }
        let dir = self
            .config
            .state_dir
            .join("actors")
            .join(actor_key.to_hex());
        let key_path = dir.join("key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            anyhow::ensure!(
                fs::metadata(&key_path)?.permissions().mode() & 0o077 == 0,
                "conversation actor custody permissions are too broad"
            );
        }
        let nsec = fs::read_to_string(key_path)?;
        anyhow::ensure!(
            Keys::parse(nsec.trim())?.public_key() == actor_key,
            "conversation actor custody identity mismatch"
        );
        let auth_tag = match fs::read_to_string(dir.join("auth-tag.json")) {
            Ok(tag) => Some(tag),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Some(crate::actor_seats::ActorSeat {
            pubkey: actor.to_owned(),
            nsec: nsec.trim().to_owned(),
            auth_tag,
            relay_url: self.config.relay_url.clone(),
            display_name: None,
            pack_dir: None,
            persona_id: None,
            pack_ref: None,
            agents_checkout: None,
        }))
    }

    pub(crate) fn refresh_conversation_generation(
        &self,
        old: &CodingSessionTarget,
        new: &CodingSessionTarget,
    ) -> anyhow::Result<()> {
        let path = self.config.state_dir.join("conversation-routes.json");
        let mut routes: ConversationRoutes = load(&path)?;
        let mut changed = false;
        for route in &mut routes.routes {
            if route.target == *old {
                route.target = new.clone();
                changed = true;
            }
        }
        if changed {
            crate::state::atomic_write(&path, &serde_json::to_vec(&routes)?)?;
            let mut origins = self.conversation_origins()?;
            for target in origins.bindings.values_mut() {
                if *target == *old {
                    *target = new.clone();
                }
            }
            self.save_conversation_origins(&origins)?;
        }
        Ok(())
    }

    pub(crate) fn purge_unpublishable_conversations(&mut self) -> anyhow::Result<()> {
        let dir = self.config.state_dir.clone();
        let provider = self.config.keys.public_key();
        self.outbox.discard(|entry| {
            entry.event.pubkey != provider && !delegated_outbox_event(&dir, &entry.event)
        })?;
        Ok(())
    }

    fn conversation_origins(&self) -> anyhow::Result<Origins> {
        load(&self.config.state_dir.join("conversation-origins.json"))
    }

    fn save_conversation_origins(&self, origins: &Origins) -> anyhow::Result<()> {
        crate::state::atomic_write(
            &self.config.state_dir.join("conversation-origins.json"),
            &serde_json::to_vec(origins)?,
        )?;
        Ok(())
    }

    fn conversation_reply(&mut self, origin: &Origin, key: &str, text: &str) -> anyhow::Result<()> {
        let semantic_key = format!("conversation:{key}");
        if self
            .outbox
            .accepted_content(beekeeper_core::kind::KIND_STREAM_MESSAGE, &semantic_key)
            .is_some()
        {
            return Ok(());
        }
        // The identity is validated before it is used as a path component.
        let actor = nostr::PublicKey::from_hex(&origin.actor)?;
        let key_path = self
            .config
            .state_dir
            .join("actors")
            .join(actor.to_hex())
            .join("key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            anyhow::ensure!(
                fs::metadata(&key_path)?.permissions().mode() & 0o077 == 0,
                "conversation actor key must not be accessible to group or others"
            );
        }
        let keys = Keys::parse(fs::read_to_string(key_path)?.trim())?;
        anyhow::ensure!(
            keys.public_key() == actor,
            "conversation actor key does not match route"
        );
        let thread = beekeeper_sdk::ThreadRef {
            root_event_id: nostr::EventId::from_hex(&origin.root)?,
            parent_event_id: nostr::EventId::from_hex(&origin.event)?,
        };
        let event = beekeeper_sdk::builders::build_message(
            origin.channel,
            text,
            Some(&thread),
            &[],
            false,
            &[],
        )?
        .sign_with_keys(&keys)?;
        self.outbox.enqueue_conversation(
            &self.config.state_dir,
            &semantic_key,
            Priority::Live,
            event,
        )?;
        Ok(())
    }

    /// Translate a verified ordinary channel message into the existing turn protocol.
    #[cfg(test)]
    pub(crate) fn conversation_command(
        &mut self,
        channel: Uuid,
        event: &Event,
    ) -> anyhow::Result<Option<String>> {
        #[cfg(test)]
        let context = match self.conversation_ownership_context.clone() {
            Some(context) => Some(context),
            None => crate::host_ownership::context()
                .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?,
        };
        #[cfg(not(test))]
        let context = crate::host_ownership::context()
            .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?;
        self.conversation_command_at(
            channel,
            event,
            context
                .as_ref()
                .map(|(home, instance)| (home.as_path(), *instance)),
        )
    }

    /// Explicit context seam also keeps isolated ownership tests free of
    /// process-global environment mutation.
    #[cfg(test)]
    pub(crate) fn conversation_command_at(
        &mut self,
        channel: Uuid,
        event: &Event,
        context: Option<(&std::path::Path, beekeeper_host_core::layout::Instance)>,
    ) -> anyhow::Result<Option<String>> {
        self.conversation_command_authorized_at(channel, event, context, None)
    }

    fn conversation_command_authorized_at(
        &mut self,
        channel: Uuid,
        event: &Event,
        context: Option<(&std::path::Path, beekeeper_host_core::layout::Instance)>,
        permitted_actor: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        event.verify()?;
        let declined_key = format!("conversation:{}:ambiguous", event.id.to_hex());
        // A declined authorized request stays declined when routes change.
        // The durable signed outbox answer also closes the small write gap
        // between reply staging and the existing consumed-command ledger.
        if self.state.is_command_consumed(&event.id.to_hex())
            || self
                .outbox
                .contains(beekeeper_core::kind::KIND_STREAM_MESSAGE, &declined_key)
        {
            if !self.state.is_command_consumed(&event.id.to_hex()) {
                self.state
                    .consume_command(&event.id.to_hex(), crate::now_secs())?;
            }
            return Ok(None);
        }

        if crate::now_secs().saturating_sub(event.created_at.as_secs())
            > self.config.command_horizon.as_secs()
        {
            return Ok(None);
        }
        if !event.tags.iter().any(|tag| {
            let values = tag.as_slice();
            values.first().is_some_and(|v| v == "h")
                && values.get(1).is_some_and(|v| v == &channel.to_string())
        }) {
            return Ok(None);
        }
        let routes: ConversationRoutes =
            load(&self.config.state_dir.join("conversation-routes.json"))?;
        let mut origins = self.conversation_origins()?;
        let event_id = event.id.to_hex();
        let root = root(event);
        let actors: std::collections::BTreeSet<_> = routes
            .routes
            .iter()
            .filter_map(|route| {
                let mentioned = event.tags.iter().any(|tag| {
                    let values = tag.as_slice();
                    values.first().is_some_and(|v| v == "p")
                        && values.get(1) == Some(&route.actor_pubkey)
                });
                let already_bound = origins
                    .bindings
                    .contains_key(&format!("{channel}:{}:{root}", route.actor_pubkey));
                (route.channel_id == channel
                    && event.pubkey.to_hex() != route.actor_pubkey
                    && (mentioned || already_bound))
                    .then(|| route.actor_pubkey.clone())
            })
            .collect();
        if actors.len() != 1 {
            return Ok(None);
        }
        let Some(actor) = actors.into_iter().next() else {
            return Ok(None);
        };
        let binding_key = format!("{channel}:{actor}:{root}");
        let candidates: Vec<_> = routes
            .routes
            .iter()
            .filter(|route| {
                route.channel_id == channel
                    && route.actor_pubkey == actor
                    && self
                        .state
                        .session(&route.target.session_id)
                        .is_some_and(|record| {
                            record.actor.as_deref() == Some(&actor)
                                && record.channel_id == channel
                                && (permitted_actor == Some(actor.as_str())
                                    || crate::commands::operator_may_steer(
                                        record,
                                        &event.pubkey.to_hex(),
                                    ))
                        })
            })
            .collect();
        // No private target or project detail is disclosed to an ungranted sender.
        if candidates.is_empty() {
            return Ok(None);
        }
        let ownership = context
            .map(|(home, instance)| {
                crate::host_ownership::conversation_ownership(
                    home,
                    instance,
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
            // Unsupported historical setup cannot execute this input. Durable
            // consumption keeps a later retry from silently becoming fresh work.
            self.state.consume_command(&event_id, crate::now_secs())?;
            return Ok(None);
        }
        if ownership.as_ref().is_some_and(|owner| owner.busy) {
            let origin = Origin {
                actor,
                channel,
                root,
                event: event_id.clone(),
                author: Some(event.pubkey.to_hex()),
                target: candidates[0].target.clone(),
                definition_event_id: None,
                linked_definition_event_id: None,
                private_definition_event_id: None,
                instructions: None,
            };
            self.conversation_reply(&origin, &format!("{event_id}:ambiguous"),
                "This host cannot admit work while its ownership fence is busy or unavailable. This host has not executed this request; send a new request when it is available.")?;
            self.state.consume_command(&event_id, crate::now_secs())?;
            return Ok(None);
        }
        // Historical routes remain for bound-thread terminal replies. Only
        // current open generations can receive a new, unbound request.
        let active: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|route| {
                self.state
                    .session(&route.target.session_id)
                    .is_some_and(|record| {
                        !record.closed
                            && !record.is_retired()
                            && route.target == self.target_for(record)
                    })
            })
            .collect();
        let target = origins
            .requests
            .get(&event_id)
            .map(|origin| origin.target.clone())
            .or_else(|| origins.bindings.get(&binding_key).cloned())
            .or_else(|| (active.len() == 1).then(|| active[0].target.clone()));
        let Some(target) = target else {
            let origin = Origin {
                actor,
                channel,
                root,
                event: event_id.clone(),
                author: Some(event.pubkey.to_hex()),
                target: candidates[0].target.clone(),
                definition_event_id: None,
                linked_definition_event_id: None,
                private_definition_event_id: None,
                instructions: None,
            };
            let text = if active.is_empty() {
                "This participant has no active responsibility available. This request has not executed."
            } else {
                "This participant has multiple responsibilities. Reply in the thread for the responsibility you want; this request has not executed."
            };
            self.conversation_reply(&origin, &format!("{event_id}:ambiguous"), text)?;
            self.state.consume_command(&event_id, crate::now_secs())?;
            return Ok(None);
        };
        let origin = Origin {
            actor,
            channel,
            root,
            event: event_id.clone(),
            author: Some(event.pubkey.to_hex()),
            target: target.clone(),
            definition_event_id: None,
            linked_definition_event_id: None,
            private_definition_event_id: None,
            instructions: None,
        };
        // Persist origin and binding before mailbox admission; retries reconstruct
        // identical command bytes and use the provider's existing consumed ledger.
        origins.bindings.insert(binding_key, target.clone());
        let request = origins.requests.entry(event_id.clone()).or_insert(origin);
        if request.author.is_none() {
            request.author = Some(event.pubkey.to_hex());
        }
        self.save_conversation_origins(&origins)?;
        let payload = CodingSessionCommandPayload {
            schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
            command_id: event_id,
            target,
            action: CodingSessionAction::ThreadTurnStart {
                text: event.content.clone(),
                attachments: vec![],
                deliver: CodingSessionDelivery::Steer,
            },
        };
        if payload.validate().is_err() {
            if let Some(origin) = origins.requests.get(&payload.command_id) {
                self.conversation_reply(origin, &format!("{}:invalid", payload.command_id),
                    "Request refused: this message cannot be represented by the supported turn contract.")?;
            }
            return Ok(None);
        }
        Ok(Some(serde_json::to_string(&payload)?))
    }

    pub(crate) fn is_conversation_request(
        &self,
        command: &str,
        target: &CodingSessionTarget,
    ) -> anyhow::Result<bool> {
        Ok(self
            .conversation_origins()?
            .requests
            .get(command)
            .is_some_and(|origin| &origin.target == target))
    }

    pub(crate) fn conversation_receipt(
        &mut self,
        command_id: &str,
        receipt: &crate::payload::LifecycleReceipt,
    ) -> anyhow::Result<()> {
        let origins = self.conversation_origins()?;
        if let Some(origin) = origins.requests.get(command_id) {
            // Refusal details may include host paths. Publish only the public code.
            let status = serde_json::to_value(receipt.status)?;
            let text = match &receipt.error {
                Some(error) if status.as_str() == Some("turn_degraded") => format!("Request queued for the next turn boundary: {}. The runtime cannot apply this request as a native steer.", error.code),
                Some(error) => format!(
                    "Request {}: {}. No successful execution is claimed.",
                    status.as_str().unwrap_or("blocked"),
                    error.code
                ),
                None => format!("Request {}.", status.as_str().unwrap_or("accepted")),
            };
            self.conversation_reply(origin, &format!("{command_id}:{status}"), &text)?;
        }
        Ok(())
    }

    pub(crate) fn conversation_transcript(
        &mut self,
        target: &CodingSessionTarget,
        sequence: u64,
        item: &serde_json::Value,
    ) -> anyhow::Result<()> {
        if item["kind"] != "assistant_text" && item["kind"] != "result" {
            return Ok(());
        }
        let command = self
            .state
            .session(&target.session_id)
            .and_then(|r| r.open_turn.as_ref())
            .and_then(|t| t.command_id.clone());
        let Some(command) = command else {
            return Ok(());
        };
        let origins = self.conversation_origins()?;
        if let Some(origin) = origins.requests.get(&command) {
            let text = if item["kind"] == "result" {
                format!(
                    "Turn ended ({}): {}",
                    item["subtype"].as_str().unwrap_or("unknown"),
                    item["result"]
                        .as_str()
                        .unwrap_or("no result detail reported")
                )
            } else {
                item["text"].as_str().unwrap_or_default().to_owned()
            };
            self.conversation_reply(origin, &format!("{command}:text:{sequence}"), &text)?;
        }
        Ok(())
    }
}

/// Only conversation rows signed by a locally custodied, configured actor may
/// survive provider rotation. Other delegated signatures remain forbidden.
pub(crate) fn delegated_outbox_event(dir: &std::path::Path, event: &Event) -> bool {
    let validate = || -> anyhow::Result<bool> {
        if event.kind != Kind::Custom(9) || event.verify().is_err() {
            return Ok(false);
        }
        let actor = event.pubkey.to_hex();
        if !event.tags.iter().any(|tag| {
            let values = tag.as_slice();
            values.first().is_some_and(|v| v == "e")
                && values.get(3).is_some_and(|v| v == "root" || v == "reply")
        }) {
            return Ok(false);
        }
        let routes: ConversationRoutes = load(&dir.join("conversation-routes.json"))?;
        let channel = event.tags.iter().find_map(|tag| {
            let values = tag.as_slice();
            (values.first().is_some_and(|v| v == "h"))
                .then(|| values.get(1))
                .flatten()
        });
        if !routes.routes.iter().any(|route| {
            route.actor_pubkey == actor
                && channel.is_some_and(|c| c == &route.channel_id.to_string())
        }) {
            return Ok(false);
        }
        let key_path = dir.join("actors").join(&actor).join("key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(&key_path)?.permissions().mode() & 0o077 != 0 {
                return Ok(false);
            }
        }
        let keys = Keys::parse(fs::read_to_string(key_path)?.trim())?;
        Ok(keys.public_key() == event.pubkey)
    };
    validate().unwrap_or(false)
}

/// Actor projections authenticate separately from the infrastructure signer.
/// Uses the existing authenticated Nostr HTTP bridge, without another inbox.
pub(crate) struct ConversationPublisher {
    pub provider: beekeeper_acp::relay::RelayEventPublisher,
    pub rest: beekeeper_acp::relay::RestClient,
    pub state_dir: std::path::PathBuf,
}

impl crate::publish::EventSink for ConversationPublisher {
    async fn publish(&self, event: Event) -> Result<(), String> {
        if event.pubkey == self.rest.keys.public_key() {
            return self
                .provider
                .publish_event_acknowledged(event)
                .await
                .map_err(|e| e.to_string());
        }
        if !delegated_outbox_event(&self.state_dir, &event) {
            return Err("blocked: conversation actor custody or route was withdrawn".into());
        }
        let actor_dir = self.state_dir.join("actors").join(event.pubkey.to_hex());
        let key = fs::read_to_string(actor_dir.join("key")).map_err(|e| e.to_string())?;
        let mut rest = self.rest.clone();
        rest.keys = Keys::parse(key.trim()).map_err(|e| e.to_string())?;
        // The actor's own attestation, never the provider's: a renewing source
        // on the provider's client would otherwise win over it.
        rest.auth_tag_source = None;
        rest.auth_tag_json = match fs::read_to_string(actor_dir.join("auth-tag.json")) {
            Ok(tag) => Some(tag),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let response = rest.submit_event(&event).await.map_err(|e| e.to_string())?;
        if response
            .get("accepted")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            Ok(())
        } else {
            Err(response
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("actor event submission did not return acceptance")
                .to_owned())
        }
    }

    async fn publish_detailed(
        &self,
        event: Event,
    ) -> Result<(), beekeeper_acp::relay::AcknowledgedPublishError> {
        if event.pubkey == self.rest.keys.public_key() {
            self.provider
                .publish_event_acknowledged_detailed(event)
                .await
        } else {
            self.publish(event)
                .await
                .map_err(beekeeper_acp::relay::AcknowledgedPublishError::from)
        }
    }
}
