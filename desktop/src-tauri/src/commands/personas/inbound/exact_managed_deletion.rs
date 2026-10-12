//! Exact managed-agent deletion guard; other NIP-09 coordinates keep their existing behavior.
use super::*;
use crate::managed_agents::retention::{
    get_retained_event, retain_inbound_event, tombstone_retention_d_tag, InboundOutcome,
    RetainedEvent,
};
use beekeeper_core_pkg::kind::{KIND_DELETION, KIND_MANAGED_AGENT};
use nostr::JsonUtil;

pub(super) fn needs_head(event: &nostr::Event, kind: u32) -> bool {
    kind == KIND_MANAGED_AGENT
        && event
            .tags
            .iter()
            .any(|tag| tag.as_slice().first().map(String::as_str) == Some("e"))
}

pub(super) fn retain_tombstone(
    conn: &rusqlite::Connection,
    event: &nostr::Event,
    kind: u32,
    target: &str,
    agents: &[ManagedAgentRecord],
) -> Result<InboundOutcome, String> {
    if needs_head(event, kind) {
        let Some(row) = get_retained_event(conn, kind, &event.pubkey.to_hex(), target)? else {
            return Err("exact agent deletion has no retained current signed head; cache and custody preserved".into());
        };
        if row.pending_sync {
            return Ok(InboundOutcome::Skipped);
        }
        let head = parse_verified_inbound_event(&row.raw_event)?;
        if head.kind.as_u16() as u32 != kind
            || head.pubkey != event.pubkey
            || head.content != row.content
            || head.created_at.as_secs() as i64 != row.created_at
            || head
                .tags
                .iter()
                .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
                .count()
                != 1
            || !head.tags.iter().any(|tag| tag.as_slice() == ["d", target])
        {
            return Err(
                "exact agent deletion retained head scope is invalid; cache preserved".into(),
            );
        }
        if !event.tags.iter().any(|tag| {
            let values = tag.as_slice();
            values.first().map(String::as_str) == Some("e")
                && values.get(1) == Some(&head.id.to_hex())
        }) {
            return Ok(InboundOutcome::Skipped);
        }
        let Some(agent) = agents.iter().find(|agent| agent.pubkey == target) else {
            return Ok(InboundOutcome::Skipped);
        };
        let content: ManagedAgentEventContent = serde_json::from_str(&head.content)
            .map_err(|_| "exact agent deletion retained projection is malformed")?;
        if crate::managed_agents::agent_events::agent_event_content(agent)
            != content.public_projection()
        {
            return Ok(InboundOutcome::Skipped);
        }
    }
    retain_inbound_event(
        conn,
        &RetainedEvent {
            kind: KIND_DELETION,
            pubkey: event.pubkey.to_hex(),
            d_tag: tombstone_retention_d_tag(kind, target),
            content: event.content.clone(),
            created_at: event.created_at.as_secs() as i64,
            raw_event: event.as_json(),
            pending_sync: false,
        },
    )
}

/// Resolve e-only managed-agent deletion exclusively from the bounded signed retention cache.
pub(super) fn coordinate(
    conn: &rusqlite::Connection,
    event: &nostr::Event,
) -> Result<Option<(u32, String)>, String> {
    if let Some(coordinate) = parse_deletion_coordinate(event) {
        return Ok(Some(coordinate));
    }
    // Do not reinterpret a malformed/foreign a coordinate as an e-only locator.
    if event
        .tags
        .iter()
        .any(|tag| tag.as_slice().first().map(String::as_str) == Some("a"))
    {
        return Ok(None);
    }
    let mut ids = event.tags.iter().filter_map(|tag| {
        let values = tag.as_slice();
        (values.first().map(String::as_str) == Some("e"))
            .then(|| values.get(1))
            .flatten()
    });
    let Some(id) = ids.next() else {
        return Ok(None);
    };
    if ids.next().is_some() || nostr::EventId::from_hex(id).is_err() {
        return Ok(None);
    }
    let mut query = conn.prepare("SELECT d_tag, raw_event, pending_sync FROM persona_events WHERE kind = ?1 AND pubkey = ?2 ORDER BY d_tag LIMIT 1001")
        .map_err(|_|"exact agent deletion cache query unavailable")?;
    let rows = query
        .query_map(
            rusqlite::params![KIND_MANAGED_AGENT, event.pubkey.to_hex()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .map_err(|_| "exact agent deletion cache read unavailable")?;
    let mut target = None;
    let mut bytes = 0usize;
    for (index, row) in rows.enumerate() {
        let (d, raw, pending) = row.map_err(|_| "exact agent deletion cache row unavailable")?;
        bytes = bytes.saturating_add(raw.len());
        if index >= 1000 || raw.len() > 1024 * 1024 || bytes > 8 * 1024 * 1024 {
            return Err(
                "exact agent deletion cache exceeds bounded proof budget; custody retained".into(),
            );
        }
        if pending {
            continue;
        }
        let Ok(head) = parse_verified_inbound_event(&raw) else {
            continue;
        };
        if head.id.to_hex() != *id {
            continue;
        }
        if head.kind.as_u16() as u32 != KIND_MANAGED_AGENT
            || head.pubkey != event.pubkey
            || head
                .tags
                .iter()
                .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
                .count()
                != 1
            || !head
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["d", d.as_str()])
            || d.len() != 64
            || !d.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("exact agent deletion signed locator is invalid; custody retained".into());
        }
        if target.is_some() {
            return Err(
                "exact agent deletion signed locator is ambiguous; custody retained".into(),
            );
        }
        target = Some((KIND_MANAGED_AGENT, d));
    }
    Ok(target)
}
