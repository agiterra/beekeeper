//! Accepted-revision cache acknowledgment, serialized with the existing inbound store lock.
use crate::managed_agents::retention::{
    get_retained_event, open_retention_db, retain_inbound_event, InboundOutcome, RetainedEvent,
};
use nostr::Event;
use std::{path::Path, sync::Mutex};

pub(super) fn retain_with_store_lock(
    lock: &Mutex<()>,
    db: &Path,
    event: &RetainedEvent,
) -> Result<(), String> {
    // No network await occurs under this lock. The inlet uses the same lock
    // for its retention preflight, public projection save and final retain.
    let _guard = lock.lock().map_err(|e| e.to_string())?;
    let conn = open_retention_db(db)?;
    if retain_inbound_event(&conn, event)? == InboundOutcome::Skipped {
        let exact = get_retained_event(&conn, event.kind, &event.pubkey, &event.d_tag)?
            .is_some_and(|row| !row.pending_sync && row.raw_event == event.raw_event);
        if !exact {
            return Err("Configuration accepted; a newer client publication is pending; refresh and reconcile".into());
        }
    }
    Ok(())
}

/// Caller holds the same store lock until the source-checked local commit ends.
pub(super) fn require_exact_head(
    db: &Path,
    owner: &str,
    actor: &str,
    revision: &str,
) -> Result<(), String> {
    let conn = open_retention_db(db)?;
    let exact = get_retained_event(&conn, 30177, owner, actor)?.is_some_and(|row| {
        if row.pending_sync {
            return false;
        }
        serde_json::from_str::<Event>(&row.raw_event).is_ok_and(|event| {
            event.id.to_hex() == revision
                && event.pubkey.to_hex() == owner
                && event.kind.as_u16() == 30177
                && event.tags.iter().any(|tag| tag.as_slice() == ["d", actor])
                && event.verify().is_ok()
        })
    });
    if !exact {
        return Err("Configuration accepted; its exact acknowledged revision was superseded before local commit; refresh and reconcile".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "host_configuration_ack_tests.rs"]
mod tests;
