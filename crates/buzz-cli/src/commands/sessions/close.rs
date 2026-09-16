//! `bee sessions close` — publish the same 44230 closure the desktop's
//! closure dialog publishes.
//!
//! # Why this exists
//!
//! Until now the only way to close a coding session was the desktop's closure
//! dialog. An operator at a terminal had exactly one whole-session verb,
//! `bee sessions delete`, and reached for it because it was there — and a
//! deletion is not a closure. Ledger 135(f): after `bee sessions delete` on
//! session `cc5cb114` both executions read `released`, but every seat
//! worktree still read "the session is not closed, so nothing is removed",
//! and the trees and the seat bundles had to be removed by hand.
//!
//! # One builder, not a second opinion
//!
//! The event is built by [`buzz_sdk::builders::build_coding_session_closure`]
//! from [`CodingSessionClosurePayload`] — the same builder and the same
//! validated payload the relay checks on ingest. Nothing here assembles tags
//! by hand, so a CLI close and a desktop close cannot drift into two shapes.
//!
//! # What it does not do
//!
//! It publishes a fact about the session and nothing else. It removes no
//! directory and no seat bundle: those live on whichever machine cut them,
//! and on that machine the host disposes of them from the closure. Run
//! `bee sessions worktree status --session <ref>` afterwards to see what
//! this machine's own trees became.

use serde_json::{json, Value};
use uuid::Uuid;

use buzz_core::coding_session_closure::{CodingSessionClosureAction, CodingSessionClosurePayload};
use buzz_core::kind::KIND_CODING_SESSION_GENESIS;
use buzz_sdk::builders::build_coding_session_closure;

use crate::client::BuzzClient;
use crate::commands::parse_write_response;
use crate::error::CliError;
use crate::validate::validate_uuid;

/// The actions this command accepts, spelled exactly as the payload spells
/// them. `open` is here because a reopen is the same event with the same
/// authority, and an operator who can close from a terminal and not undo it
/// is one typo from a trip to another machine.
pub fn parse_action(value: &str) -> Result<CodingSessionClosureAction, CliError> {
    match value {
        "closed" => Ok(CodingSessionClosureAction::Closed),
        "archived" => Ok(CodingSessionClosureAction::Archived),
        "open" => Ok(CodingSessionClosureAction::Open),
        other => Err(CliError::Usage(format!(
            "--action must be closed, archived or open (got {other:?})"
        ))),
    }
}

/// The genesis event id for one umbrella session in one channel.
///
/// Matched on the `sessionRef` in the genesis's **content**, never on its
/// `csg-session` tag: NIP-CSG is explicit that the tag exists for the relay's
/// uniqueness probe and that consumers must not select by it. Same rule
/// `session_owned_events` and the desktop's `findGenesis` follow.
pub fn find_genesis<'a>(events: &'a [Value], session_ref: &str) -> Option<&'a str> {
    events.iter().find_map(|event| {
        if event.get("kind").and_then(Value::as_u64) != Some(KIND_CODING_SESSION_GENESIS as u64) {
            return None;
        }
        let content = event.get("content")?.as_str()?;
        let parsed: Value = serde_json::from_str(content).ok()?;
        if parsed.get("sessionRef")?.as_str()? != session_ref {
            return None;
        }
        event.get("id")?.as_str()
    })
}

/// `bee sessions close` — settle (or reopen) one umbrella session.
pub async fn cmd_close(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: &str,
    action: CodingSessionClosureAction,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    validate_uuid(session_ref)?;
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;

    let events = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_GENESIS],
            "#h": [channel_id],
            "limit": 2000,
        }))
        .await?;
    let Some(genesis_ref) = find_genesis(&events, session_ref) else {
        // Without the genesis there is no authority root to name, and the
        // relay would refuse the closure for a reason the operator would have
        // to decode. Say which fact is missing instead.
        return Err(CliError::NotFound(format!(
            "no coding-session genesis for {session_ref:?} in channel {channel_id:?}, so no \
             closure can name its authority root"
        )));
    };

    let payload = CodingSessionClosurePayload::new(action, genesis_ref, session_ref);
    let builder = build_coding_session_closure(channel, &payload)
        .map_err(|error| CliError::Other(error.to_string()))?;
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    let mut response: Value = serde_json::from_str(&parse_write_response(
        &raw,
        "the session changed while it was being closed; retry",
    )?)
    .unwrap_or_else(|_| json!({}));
    if let Some(object) = response.as_object_mut() {
        object.insert("session_ref".into(), json!(session_ref));
        object.insert("genesis_ref".into(), json!(genesis_ref));
        object.insert(
            "action".into(),
            serde_json::to_value(action).unwrap_or(Value::Null),
        );
        // Said on every receipt because it is the one thing an operator is
        // likely to assume and be wrong about: a closure is a published fact,
        // and the directories are disposed of by the host that cut them.
        object.insert(
            "worktreeNote".into(),
            json!(
                "worktrees and seat bundles are disposed of by the host that cut them; run \
                 `bee sessions worktree status --session <ref>` on that machine"
            ),
        );
    }
    println!("{response}");
    Ok(())
}

#[cfg(test)]
#[path = "close_tests.rs"]
mod tests;
