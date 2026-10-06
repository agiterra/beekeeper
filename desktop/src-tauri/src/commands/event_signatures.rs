//! Native, batched Nostr event signature verification for the webview.
//!
//! The desktop verifies every provider-authored event before it will present
//! it as such. Doing that in JavaScript (`nostr-tools` `verifyEvent`, BigInt
//! Schnorr) costs ~1.5 ms per event on the webview's main thread — 8.7 s for
//! the 5,843 events of one real session channel (SV-117, ledger 347). The same
//! check in libsecp256k1 is two orders of magnitude cheaper and runs here, off
//! the UI thread, across cores.
//!
//! The command answers one fact per event, in input order, and never more:
//!
//! - `valid` — the id is the NIP-01 hash of exactly the fields received, and
//!   the signature verifies over that id under the event's pubkey.
//! - `invalid-signature` — the id is the hash of the fields received, but the
//!   signature does not verify. The id fixes the signed bytes, so this is a
//!   stable verdict about `(id, sig)`.
//! - `unchecked` — anything else: the id does not match what was received, or
//!   the event could not be read. This is deliberately *not* a verdict about
//!   `(id, sig)`, because an attacker can pair a real id and signature with any
//!   content; the caller must treat it as "not verified here" and decide on
//!   its own (the webview falls back to its JavaScript check, which rejects it).
//!
//! Each event is parsed on its own, so one malformed entry cannot fail the
//! batch.

use std::str::FromStr;

use nostr::secp256k1::{schnorr::Signature, Message, XOnlyPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(test)]
#[path = "event_signatures_tests.rs"]
mod tests;

/// Batches smaller than this are verified on one thread; spawning workers
/// costs more than it saves.
const PARALLEL_THRESHOLD: usize = 64;

/// Upper bound on events per call, so a runaway caller cannot pin every core
/// on one IPC message. The webview chunks larger batches.
pub const MAX_EVENT_SIGNATURE_BATCH: usize = 20_000;

/// One event's verification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventSignatureVerdict {
    /// The id hashes the received fields and the signature verifies over it.
    Valid,
    /// The id hashes the received fields; the signature does not verify.
    InvalidSignature,
    /// Not verified: id mismatch, or the event could not be read.
    Unchecked,
}

/// The seven signed NIP-01 fields, exactly as the webview projected them.
#[derive(Debug, Deserialize)]
struct WireEvent {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u64,
    tags: Vec<Vec<String>>,
    content: String,
    sig: String,
}

/// The NIP-01 event id over the received fields: lowercase hex of
/// `sha256(JSON([0, pubkey, created_at, kind, tags, content]))`.
///
/// `serde_json`'s string escaping matches `JSON.stringify` for every
/// well-formed string (named escapes for `\b \f \n \r \t`, lowercase `\u00xx`
/// for other control characters, nothing else escaped), which is what
/// `nostr-tools` hashes. Any disagreement could only turn a match into a
/// mismatch, which reads as `unchecked`, never as `valid`.
fn compute_event_id(event: &WireEvent) -> Option<[u8; 32]> {
    let canonical = serde_json::to_string(&(
        0u8,
        &event.pubkey,
        event.created_at,
        event.kind,
        &event.tags,
        &event.content,
    ))
    .ok()?;
    Some(Sha256::digest(canonical.as_bytes()).into())
}

fn verify_wire_event(event: &WireEvent) -> EventSignatureVerdict {
    let Some(digest) = compute_event_id(event) else {
        return EventSignatureVerdict::Unchecked;
    };
    // Exact string comparison, as the webview's check does: an upper-case or
    // padded id is not the canonical id.
    if hex::encode(digest) != event.id {
        return EventSignatureVerdict::Unchecked;
    }
    let (Ok(pubkey), Ok(sig)) = (
        XOnlyPublicKey::from_str(&event.pubkey),
        Signature::from_str(&event.sig),
    ) else {
        return EventSignatureVerdict::InvalidSignature;
    };
    let message = Message::from_digest(digest);
    if nostr::SECP256K1
        .verify_schnorr(&sig, &message, &pubkey)
        .is_ok()
    {
        EventSignatureVerdict::Valid
    } else {
        EventSignatureVerdict::InvalidSignature
    }
}

/// Verify one event given as untyped JSON. Never panics; anything unreadable
/// is `unchecked`.
pub fn verify_event_value(value: serde_json::Value) -> EventSignatureVerdict {
    match serde_json::from_value::<WireEvent>(value) {
        Ok(event) => verify_wire_event(&event),
        Err(_) => EventSignatureVerdict::Unchecked,
    }
}

/// Verify a batch, preserving input order, spreading large batches across the
/// machine's cores.
pub fn verify_event_values(events: Vec<serde_json::Value>) -> Vec<EventSignatureVerdict> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(8);
    if events.len() < PARALLEL_THRESHOLD || workers < 2 {
        return events.into_iter().map(verify_event_value).collect();
    }
    let chunk_size = events.len().div_ceil(workers);
    let mut chunks: Vec<Vec<serde_json::Value>> = Vec::with_capacity(workers);
    let mut rest = events.into_iter();
    loop {
        let chunk: Vec<serde_json::Value> = rest.by_ref().take(chunk_size).collect();
        if chunk.is_empty() {
            break;
        }
        chunks.push(chunk);
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .into_iter()
                        .map(verify_event_value)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                // A worker cannot panic (every path returns a verdict). If one
                // did, its chunk comes back empty and the caller's count check
                // rejects the whole batch rather than misalign verdicts.
                handle.join().unwrap_or_default()
            })
            .collect()
    })
}

/// Verify a batch of signed Nostr events natively; one verdict per event, in
/// input order. See the module docs for what each verdict does and does not
/// claim.
#[tauri::command]
pub async fn verify_event_signatures(
    events: Vec<serde_json::Value>,
) -> Result<Vec<EventSignatureVerdict>, String> {
    if events.len() > MAX_EVENT_SIGNATURE_BATCH {
        return Err(format!(
            "verify_event_signatures: batch of {} exceeds the {} event limit",
            events.len(),
            MAX_EVENT_SIGNATURE_BATCH
        ));
    }
    let expected = events.len();
    let verdicts = tauri::async_runtime::spawn_blocking(move || verify_event_values(events))
        .await
        .map_err(|error| format!("verify_event_signatures: worker failed: {error}"))?;
    if verdicts.len() != expected {
        return Err("verify_event_signatures: verdict count mismatch".to_string());
    }
    Ok(verdicts)
}
