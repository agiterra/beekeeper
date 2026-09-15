//! Carrying a published project association forward so a computer that does
//! not hold it never withdraws it (lane-owned; see PROJECT_AGENT_HIRING_IMPL.md).
//!
//! An agent's kind:30177 is one replaceable event per `(owner, agent)`, and
//! every computer holding the agent publishes it. Before this module, a second
//! computer whose record had no `project_ref` published a projection with no
//! `project_digest`, superseding the associated computer's event and removing
//! the agent from its project for every reader (review 2026-09-14, finding 3).
//!
//! Three pieces close that:
//!
//! - [`published_project_digest`], the projection rule: a verified public
//!   project publishes its digest, a verified private project publishes none
//!   (withdrawing any carried digest on purpose), and anything unverified or
//!   unassociated republishes only what it carries.
//! - [`carry_inbound_project_digest`]: an inbound same-owner kind:30177 with a
//!   digest is carried onto a record that cannot itself decide the answer.
//! - [`guard_managed_agent_withdrawal`], used by the flush loop: a digest-less
//!   kind:30177 is not published while the relay head at that address carries a
//!   digest this computer has no authority to withdraw. The digest is carried
//!   instead and the corrected row publishes in its place.
//!
//! A carried digest is never membership: hiring reads only `project_ref`.
//! Withdrawal of an association needs an explicit dissociation, which does not
//! exist yet, so a carried digest is never cleared by a digest-less event.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use buzz_core_pkg::kind::KIND_MANAGED_AGENT;
use buzz_core_pkg::project_agent_association::project_agent_digest;

use super::retention::{get_retained_event, open_retention_db, RetainedEvent};
use super::ManagedAgentRecord;
use crate::app_state::AppState;

/// Whether `value` has the shape [`project_agent_digest`] produces: 64
/// lowercase hex characters. Anything else is never carried or honored.
pub(crate) fn is_project_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn recorded_project(record: &ManagedAgentRecord) -> Option<&str> {
    record
        .project_ref
        .as_deref()
        .map(buzz_core_pkg::project_agent_association::trim_ascii_whitespace)
        .filter(|project| !project.is_empty())
}

fn carried_digest(record: &ManagedAgentRecord) -> Option<String> {
    record
        .carried_project_digest
        .as_deref()
        .filter(|digest| is_project_digest(digest))
        .map(str::to_owned)
}

/// Whether the record's own project is known, from a signed head, to be
/// private. Only this state may withdraw a published digest.
fn project_known_private(record: &ManagedAgentRecord) -> bool {
    recorded_project(record).is_some() && record.project_public == Some(false)
}

/// The `project_digest` a record's kind:30177 publishes.
///
/// | `project_ref` | `project_public` | published                         |
/// | ------------- | ---------------- | --------------------------------- |
/// | set           | `Some(true)`     | digest of `project_ref`           |
/// | set           | `Some(false)`    | none — a private project is never announced |
/// | set           | `None`           | the carried digest, if any        |
/// | unset         | any              | the carried digest, if any        |
///
/// A public `project_ref` that is not a well-formed coordinate has no digest
/// of its own, so it falls back to the carried one rather than withdrawing.
pub(crate) fn published_project_digest(record: &ManagedAgentRecord) -> Option<String> {
    match (recorded_project(record), record.project_public) {
        (Some(_), Some(false)) => None,
        (Some(project), Some(true)) => {
            project_agent_digest(project).or_else(|| carried_digest(record))
        }
        (Some(_), None) | (None, _) => carried_digest(record),
    }
}

/// Carry the `project_digest` of an inbound kind:30177 **authored by this
/// computer's owner** onto `record`. Returns whether the record changed.
///
/// Carries only onto a record that cannot decide the answer itself: no
/// `project_ref`, or a `project_ref` whose visibility is not yet verified. A
/// record whose project is verified (public or private) keeps its own answer.
/// A missing or malformed inbound digest never clears a carried one. Never
/// touches `project_ref`, `project_public` or `home_role`.
pub(crate) fn carry_inbound_project_digest(
    record: &mut ManagedAgentRecord,
    inbound_digest: Option<&str>,
) -> bool {
    let Some(digest) = inbound_digest.filter(|digest| is_project_digest(digest)) else {
        return false;
    };
    if recorded_project(record).is_some() && record.project_public.is_some() {
        return false;
    }
    if record.carried_project_digest.as_deref() == Some(digest) {
        return false;
    }
    record.carried_project_digest = Some(digest.to_owned());
    true
}

/// What the flush loop does with a pending digest-less kind:30177 whose relay
/// head carries `relay_digest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WithdrawalGuard {
    /// Publish the row: nothing is withdrawn, or this computer knows the
    /// agent's project is private and withdraws on purpose.
    Publish,
    /// Do not publish; carry this digest onto the record and republish it.
    Carry(String),
    /// Do not publish and change nothing: this computer holds no record that
    /// could decide, so it must not withdraw blind.
    Withhold,
}

/// Decide a pending digest-less kind:30177 against its relay head's digest.
///
/// | local record                | relay head digest | decision   |
/// | --------------------------- | ----------------- | ---------- |
/// | any                         | none or malformed | `Publish`  |
/// | project known private       | valid             | `Publish`  |
/// | present, not known private  | valid             | `Carry`    |
/// | missing                     | valid             | `Withhold` |
pub(crate) fn withheld_withdrawal(
    local: Option<&ManagedAgentRecord>,
    relay_digest: Option<&str>,
) -> WithdrawalGuard {
    let Some(digest) = relay_digest.filter(|digest| is_project_digest(digest)) else {
        return WithdrawalGuard::Publish;
    };
    match local {
        None => WithdrawalGuard::Withhold,
        Some(record) if project_known_private(record) => WithdrawalGuard::Publish,
        Some(_) => WithdrawalGuard::Carry(digest.to_owned()),
    }
}

/// What a carry hook did for one agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CarryHookOutcome {
    /// Publish the pending row as it is.
    Publish,
    /// The record now carries the digest and a corrected row was retained;
    /// the flush publishes the retained head in place of the old row.
    Carried,
    /// Leave the row pending and publish nothing.
    Withhold,
}

/// Called by the flush loop with `(retention db, owner keys, agent pubkey,
/// relay digest)`. It runs synchronously, never across an `.await`, and takes
/// the managed-agent store lock itself.
pub(crate) type CarryHook<'a> =
    &'a (dyn Fn(&Path, &nostr::Keys, &str, &str) -> Result<CarryHookOutcome, String> + Send + Sync);

/// Apply [`withheld_withdrawal`] to the agent `agent` in `agents` for
/// `relay_digest`, carrying the digest when it says so. Returns the decision
/// and whether a record changed (the caller saves only then).
pub(crate) fn apply_relay_digest(
    agents: &mut [ManagedAgentRecord],
    agent: &str,
    relay_digest: &str,
) -> (WithdrawalGuard, bool) {
    let record = agents.iter_mut().find(|record| record.pubkey == agent);
    let decision = withheld_withdrawal(record.as_deref(), Some(relay_digest));
    let changed = match (&decision, record) {
        (WithdrawalGuard::Carry(digest), Some(record))
            if record.carried_project_digest.as_deref() != Some(digest.as_str()) =>
        {
            record.carried_project_digest = Some(digest.clone());
            true
        }
        _ => false,
    };
    (decision, changed)
}

/// Re-retain `record` after a carry and report what the flush should do: a
/// projection that now publishes a digest is [`CarryHookOutcome::Carried`];
/// one that still publishes none (nothing carried could apply) is withheld.
pub(crate) fn retain_after_carry(
    conn: &rusqlite::Connection,
    owner_keys: &nostr::Keys,
    record: &ManagedAgentRecord,
) -> Result<CarryHookOutcome, String> {
    super::reconcile::retain_agent_record(conn, owner_keys, record)?;
    Ok(if published_project_digest(record).is_some() {
        CarryHookOutcome::Carried
    } else {
        CarryHookOutcome::Withhold
    })
}

/// The production carry hook: under the store lock, load this computer's
/// records, decide, save a carried digest, and re-retain the agent's row.
pub(crate) fn app_carry_hook(
    app: tauri::AppHandle,
) -> impl Fn(&Path, &nostr::Keys, &str, &str) -> Result<CarryHookOutcome, String> + Send + Sync {
    move |db_path, owner_keys, agent, relay_digest| {
        use tauri::Manager;
        let state = app
            .try_state::<AppState>()
            .ok_or_else(|| "app state is unavailable".to_string())?;
        let _guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut agents = super::load_managed_agents(&app)?;
        let (decision, changed) = apply_relay_digest(&mut agents, agent, relay_digest);
        match decision {
            WithdrawalGuard::Publish => Ok(CarryHookOutcome::Publish),
            WithdrawalGuard::Withhold => Ok(CarryHookOutcome::Withhold),
            WithdrawalGuard::Carry(_) => {
                if changed {
                    super::save_managed_agents(&app, &agents)?;
                }
                let record = agents
                    .iter()
                    .find(|record| record.pubkey == agent)
                    .ok_or_else(|| format!("agent {agent} disappeared during carry"))?;
                let conn = open_retention_db(db_path)?;
                retain_after_carry(&conn, owner_keys, record)
            }
        }
    }
}

/// What the flush loop does with one pending row after the guard.
#[derive(Debug)]
pub(crate) enum FlushGuard {
    /// Publish the row it was given.
    Proceed,
    /// Leave it pending this sweep.
    Skip,
    /// Publish this retained head instead (the carried correction).
    Replaced(RetainedEvent),
}

/// Agents whose relay read failed and has already been logged; cleared on the
/// next successful read so a recurring failure logs once, not every sweep.
static READ_FAILURE_LOGGED: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn log_read_failure_once(agent: &str, error: &str) {
    let Ok(mut logged) = READ_FAILURE_LOGGED.lock() else {
        return;
    };
    if logged.insert(agent.to_owned()) {
        tracing::warn!(
            agent,
            error,
            "withholding a kind:30177 without a project digest: the relay head could not be read, so publishing could withdraw another computer's project association"
        );
    }
}

fn clear_read_failure(agent: &str) {
    if let Ok(mut logged) = READ_FAILURE_LOGGED.lock() {
        logged.remove(agent);
    }
}

fn content_digest(content: &str) -> Option<String> {
    serde_json::from_str::<super::agent_events::ManagedAgentEventContent>(content)
        .ok()?
        .project_digest
        .filter(|digest| is_project_digest(digest))
}

fn d_tag(event: &nostr::Event) -> Option<&str> {
    event.tags.iter().find_map(|tag| {
        let values = tag.as_slice();
        (values.first().map(String::as_str) == Some("d"))
            .then(|| values.get(1).map(String::as_str))
            .flatten()
    })
}

/// The `project_digest` of the newest signed kind:30177 by `owner` at `agent`
/// among `events`. Events with a bad signature, another author, kind or d tag
/// are ignored; ties on `created_at` go to the lowest id, as NIP-01 keeps.
pub(crate) fn relay_head_digest(
    events: &[nostr::Event],
    owner: &nostr::PublicKey,
    agent: &str,
) -> Option<String> {
    events
        .iter()
        .filter(|event| {
            event.pubkey == *owner
                && u32::from(event.kind.as_u16()) == KIND_MANAGED_AGENT
                && d_tag(event) == Some(agent)
                && event.verify().is_ok()
        })
        .min_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        })
        .and_then(|head| content_digest(&head.content))
}

/// The flush loop's publish-time guard for one pending row.
///
/// Anything but a kind:30177 whose content has no valid `project_digest`
/// proceeds untouched. For such a row the relay head at
/// `{kinds:[30177], authors:[owner], #d:[agent]}` is read first:
/// - read failure: skip (fail closed), logged once per agent;
/// - no head, or a head without a digest: proceed;
/// - a head with a digest: `hook` decides; a carry publishes the corrected
///   retained head in place of this row, provided it now carries a digest.
pub(crate) async fn guard_managed_agent_withdrawal(
    current: &RetainedEvent,
    state: &AppState,
    relay_api_base: &str,
    db_path: &Path,
    owner_keys: &nostr::Keys,
    hook: CarryHook<'_>,
) -> Result<FlushGuard, String> {
    if current.kind != KIND_MANAGED_AGENT || content_digest(&current.content).is_some() {
        return Ok(FlushGuard::Proceed);
    }
    let owner = owner_keys.public_key();
    let filter = serde_json::json!({
        "kinds": [KIND_MANAGED_AGENT],
        "authors": [owner.to_hex()],
        "#d": [current.d_tag],
    });
    let events = match crate::relay::query_relay_at_with_keys(
        state,
        relay_api_base,
        &[filter],
        owner_keys,
        None,
    )
    .await
    {
        Ok(events) => events,
        Err(error) => {
            log_read_failure_once(&current.d_tag, &error);
            return Ok(FlushGuard::Skip);
        }
    };
    clear_read_failure(&current.d_tag);
    let Some(relay_digest) = relay_head_digest(&events, &owner, &current.d_tag) else {
        return Ok(FlushGuard::Proceed);
    };
    match hook(db_path, owner_keys, &current.d_tag, &relay_digest) {
        Ok(CarryHookOutcome::Publish) => Ok(FlushGuard::Proceed),
        Ok(CarryHookOutcome::Withhold) => Ok(FlushGuard::Skip),
        Ok(CarryHookOutcome::Carried) => {
            let conn = open_retention_db(db_path)?;
            let head = get_retained_event(&conn, current.kind, &current.pubkey, &current.d_tag)?;
            Ok(match head {
                Some(head) if head.pending_sync && content_digest(&head.content).is_some() => {
                    FlushGuard::Replaced(head)
                }
                _ => FlushGuard::Skip,
            })
        }
        Err(error) => {
            tracing::warn!(
                agent = %current.d_tag,
                %error,
                "withholding a kind:30177 without a project digest: carrying the published association failed"
            );
            Ok(FlushGuard::Skip)
        }
    }
}

#[cfg(test)]
#[path = "project_association_carry_tests.rs"]
mod tests;
