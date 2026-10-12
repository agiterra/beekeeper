//! Responsibility replay uses the existing bounded reorder window and command ledger.
use super::*;
use crate::off_loop::OffLoopSlot;
use std::collections::BTreeSet;
use std::time::Duration;

pub(crate) struct Backfill {
    pub(crate) slot: OffLoopSlot<Fetched>,
    checked: BTreeSet<String>,
}
pub(crate) struct Fetched {
    key: String,
    channel: Uuid,
    result: Result<Vec<Event>, String>,
}
impl Default for Backfill {
    fn default() -> Self {
        Self {
            slot: OffLoopSlot::new(Duration::from_secs(15)),
            checked: BTreeSet::new(),
        }
    }
}
fn key(route: &ConversationRoute) -> anyhow::Result<String> {
    Ok(serde_json::to_string(route)?)
}
fn scoped(event: &Event, channel: Uuid, actor: &str) -> bool {
    event.verify().is_ok()
        && event.kind == Kind::Custom(9)
        && event.tags.iter().any(|tag| {
            tag.as_slice().first().is_some_and(|v| v == "h")
                && tag.as_slice().get(1) == Some(&channel.to_string())
        })
        && event.tags.iter().any(|tag| {
            tag.as_slice().first().is_some_and(|v| v == "p")
                && tag.as_slice().get(1).is_some_and(|v| v == actor)
        })
}
impl Provider {
    fn route_query_since(&self, route: &ConversationRoute) -> anyhow::Result<u64> {
        Ok(route
            .replay_floor_unix
            .ok_or_else(|| anyhow::anyhow!("responsibility replay boundary unavailable"))?
            .saturating_sub(crate::REPLAY_GRACE_SECS))
    }
    pub(crate) fn conversation_replay_since(&self, channel: Uuid) -> Option<u64> {
        let routes: ConversationRoutes =
            load(&self.config.state_dir.join("conversation-routes.json")).ok()?;
        routes
            .routes
            .iter()
            .filter(|route| route.channel_id == channel)
            .filter(|route| route.replay_floor_unix.is_some())
            .filter_map(|route| self.route_query_since(route).ok())
            .min()
    }
    pub(crate) fn start_conversation_backfill(&mut self) -> anyhow::Result<()> {
        if self.conversation_backfill.slot.busy() {
            return Ok(());
        }
        let Some(rest) = self.rest_client.clone() else {
            return Ok(());
        };
        let routes: ConversationRoutes =
            load(&self.config.state_dir.join("conversation-routes.json"))?;
        let mut selected = None;
        for route in routes.routes {
            if route.replay_floor_unix.is_none()
                || !self
                    .state
                    .session(&route.target.session_id)
                    .is_some_and(|record| {
                        !record.closed
                            && record.retired.is_none()
                            && self.target_for(record) == route.target
                    })
            {
                continue;
            }
            let since = self.route_query_since(&route)?;
            let key = format!("{}:{since}", key(&route)?);
            if !self.conversation_backfill.checked.contains(&key) {
                selected = Some((route, key, since));
                break;
            }
        }
        let Some((route, key, since)) = selected else {
            return Ok(());
        };
        let channel = route.channel_id;
        let until = crate::now_secs();
        // Protect ordering against live inputs while the scoped snapshot is read.
        self.replay.open_until.insert(
            channel,
            std::time::Instant::now()
                + self.conversation_backfill.slot.timeout
                + crate::REPLAY_REORDER_WINDOW,
        );
        let timeout = self.conversation_backfill.slot.timeout;
        self.conversation_backfill.slot.spawn(async move {
            let result = tokio::time::timeout(timeout, async {
                let filter: nostr::Filter = serde_json::from_value(serde_json::json!({"kinds":[9],"#h":[channel.to_string()],"#p":[route.actor_pubkey],"since":since,"until":until,"limit":1000})).map_err(|e|e.to_string())?;
                let rows = rest.query(&[filter]).await.map_err(|e|e.to_string())?;
                let rows = rows.as_array().ok_or("responsibility replay response malformed")?;
                if rows.len()>=1000 {return Err("RESPONSIBILITY_REPLAY_CAPACITY: bounded history is incomplete; explicit recovery required".into());}
                let mut events = Vec::new();
                for row in rows {
                    let event:Event=serde_json::from_value(row.clone()).map_err(|_|"responsibility replay event malformed")?;
                    if scoped(&event,channel,&route.actor_pubkey) { events.push(event); }
                }
                events.sort_by(|a,b|a.created_at.cmp(&b.created_at).then_with(||a.id.cmp(&b.id)));
                Ok(events)
            }).await.unwrap_or_else(|_|Err("RESPONSIBILITY_REPLAY_TIMEOUT: bounded relay read timed out".into()));
            Fetched {key,channel,result}
        });
        Ok(())
    }
    pub(crate) async fn finish_conversation_backfill(
        &mut self,
        fetched: Fetched,
    ) -> anyhow::Result<()> {
        self.conversation_backfill.slot.settle();
        let events = match fetched.result {
            Ok(events) => events,
            Err(error) => {
                self.open_replay_window(fetched.channel);
                return Err(anyhow::anyhow!(error));
            }
        };
        // Only mark checked after every event has entered existing replay/admission.
        // A failed apply retries the same bounded snapshot; durable ledgers dedupe.
        for event in events {
            self.handle_command_event(fetched.channel, &event).await?;
        }
        self.conversation_backfill.checked.insert(fetched.key);
        self.open_replay_window(fetched.channel);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_route_wire_shape_defaults_to_no_explicit_replay_boundary() {
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "primary".into(),
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
        let route:ConversationRoute=serde_json::from_value(serde_json::json!({"actorPubkey":"ab".repeat(32),"channelId":Uuid::new_v4(),"target":target})).unwrap();
        assert_eq!(route.replay_floor_unix, None);
        assert!(serde_json::to_value(route)
            .unwrap()
            .get("replayFloorUnix")
            .is_none());
    }
}
