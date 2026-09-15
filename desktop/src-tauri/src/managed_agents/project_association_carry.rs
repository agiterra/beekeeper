//! Carrying a published project association forward so a computer that does
//! not hold it never withdraws it, and withdrawing it everywhere once its
//! project is private (lane-owned; see PROJECT_AGENT_HIRING_IMPL.md).
//!
//! An agent's kind:30177 is one replaceable event per `(owner, agent)`, and
//! every computer holding the agent publishes it. A second computer whose
//! record had no `project_ref` once published a projection with no
//! `project_digest`, superseding the associated computer's event (review
//! 2026-09-14, finding 3). Carrying fixed that, but a digest-less event cannot
//! tell a privacy withdrawal from that stale overwrite, so a carrying computer
//! could resurrect a withdrawn digest (review 2026-09-15). A withdrawal is
//! therefore explicit: the owner-signed `project_withdrawn` marker, remembered
//! as the sticky `project_publication_withdrawn` record flag.
//!
//! - [`published_association`], the projection: a verified public project
//!   publishes its digest; a withdrawn or verified private one publishes the
//!   marker; anything else republishes only what it carries.
//! - [`note_verified_visibility`]: the signed head's verdict sets or clears
//!   the flag. Only this computer's own verified public project clears it.
//! - [`carry_inbound_project_digest`]: a same-owner inbound marker withdraws;
//!   a digest is carried onto a record that cannot decide and is not
//!   withdrawn; an event with neither changes nothing.
//! - [`guard_managed_agent_withdrawal`], used by the flush loop: a pending
//!   kind:30177 not backed by this computer's own verified public project is
//!   checked against the relay head first. A marker there withdraws here; a
//!   digest there is carried or, when nothing here can decide, withheld.
//!
//! A carried digest is never membership: hiring reads only `project_ref`.

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

/// What a kind:30177 says about the agent's project association.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Announced {
    /// Neither a valid digest nor the marker (or no event at all).
    #[default]
    Nothing,
    /// A well-formed `project_digest`.
    Digest(String),
    /// The `project_withdrawn` marker. Wins over a digest in the same event.
    Withdrawn,
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

/// The digest of the record's own project when a signed head read it public.
fn own_public_digest(record: &ManagedAgentRecord) -> Option<String> {
    match (recorded_project(record), record.project_public) {
        (Some(project), Some(true)) => project_agent_digest(project),
        _ => None,
    }
}

/// Whether the record's own project is known, from a signed head, to be
/// private: this computer is then the withdrawer.
fn project_known_private(record: &ManagedAgentRecord) -> bool {
    recorded_project(record).is_some() && record.project_public == Some(false)
}

/// Whether another computer's digest may be carried onto `record`: it is not
/// withdrawn, and its own project's visibility is not verified either way.
fn may_carry(record: &ManagedAgentRecord) -> bool {
    let verified = recorded_project(record).is_some() && record.project_public.is_some();
    !(record.project_publication_withdrawn || verified)
}

/// The association a record's kind:30177 publishes, first match wins:
///
/// | record                                   | published             |
/// | ---------------------------------------- | --------------------- |
/// | `project_ref` verified public            | its digest, no marker |
/// | `project_publication_withdrawn`          | marker, no digest     |
/// | `project_ref` verified private           | marker, no digest     |
/// | carries a well-formed digest             | the carried digest    |
/// | otherwise                                | nothing               |
///
/// A public `project_ref` that is not a well-formed coordinate has no digest
/// of its own, so it falls through rather than publishing a guess.
pub(crate) fn published_association(record: &ManagedAgentRecord) -> Announced {
    if let Some(digest) = own_public_digest(record) {
        return Announced::Digest(digest);
    }
    if record.project_publication_withdrawn || project_known_private(record) {
        return Announced::Withdrawn;
    }
    carried_digest(record).map_or(Announced::Nothing, Announced::Digest)
}

/// The `project_digest` a record's kind:30177 publishes
/// ([`published_association`]).
pub(crate) fn published_project_digest(record: &ManagedAgentRecord) -> Option<String> {
    match published_association(record) {
        Announced::Digest(digest) => Some(digest),
        Announced::Nothing | Announced::Withdrawn => None,
    }
}

/// Whether a record's kind:30177 publishes the `project_withdrawn` marker
/// ([`published_association`]); never together with a digest.
pub(crate) fn published_project_withdrawn(record: &ManagedAgentRecord) -> bool {
    published_association(record) == Announced::Withdrawn
}

/// Withdraw the record's publication: set the sticky flag and drop any carried
/// digest. Returns whether the record changed.
pub(crate) fn mark_publication_withdrawn(record: &mut ManagedAgentRecord) -> bool {
    let changed = !record.project_publication_withdrawn || record.carried_project_digest.is_some();
    record.project_publication_withdrawn = true;
    record.carried_project_digest = None;
    changed
}

/// Record the newest verified signed head's verdict on the record's own
/// `project_ref`, replacing a known one. Private withdraws (flag set, carry
/// dropped); public is the only thing that clears the flag, and lets the own
/// digest publish again. Returns whether the record changed.
pub(crate) fn note_verified_visibility(record: &mut ManagedAgentRecord, public: bool) -> bool {
    let mut changed = record.project_public != Some(public);
    record.project_public = Some(public);
    if !public {
        changed |= mark_publication_withdrawn(record);
    } else if record.project_publication_withdrawn {
        record.project_publication_withdrawn = false;
        changed = true;
    }
    changed
}

/// Apply an inbound kind:30177 **authored by this computer's owner** to
/// `record`. Returns whether the record changed.
///
/// | inbound                    | record                          | effect                 |
/// | -------------------------- | ------------------------------- | ---------------------- |
/// | marker                     | any                             | withdraw, drop carry   |
/// | valid digest, no marker    | [`may_carry`]                   | carry the digest       |
/// | valid digest, no marker    | withdrawn or project verified   | unchanged              |
/// | neither (or malformed)     | any                             | unchanged              |
///
/// A digest-less event never clears a carried digest or the flag: that is the
/// stale-host case. Never touches `project_ref`, `project_public` or
/// `home_role`.
pub(crate) fn carry_inbound_project_digest(
    record: &mut ManagedAgentRecord,
    inbound_digest: Option<&str>,
    inbound_withdrawn: bool,
) -> bool {
    if inbound_withdrawn {
        return mark_publication_withdrawn(record);
    }
    let Some(digest) = inbound_digest.filter(|digest| is_project_digest(digest)) else {
        return false;
    };
    if !may_carry(record) || record.carried_project_digest.as_deref() == Some(digest) {
        return false;
    }
    record.carried_project_digest = Some(digest.to_owned());
    true
}

/// What the flush loop does with a pending kind:30177 after its relay head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CarryHookOutcome {
    /// Publish the pending row as it is.
    Publish,
    /// The record changed or the row was stale, and a corrected row was
    /// retained; the flush publishes the retained head in its place.
    Corrected,
    /// Leave the row pending and publish nothing.
    Withhold,
}

/// Decide a pending row announcing `row` against the relay head announcing
/// `head`, updating `agent`'s record in `agents`. Returns the decision and
/// whether a record changed (the caller saves only then).
///
/// | local record | relay head | record update             | decision                      |
/// | ------------ | ---------- | ------------------------- | ----------------------------- |
/// | missing      | nothing    | —                         | `Publish`                     |
/// | missing      | digest / marker | —                    | `Withhold`                    |
/// | present      | marker     | withdraw, drop carry      | then by projection            |
/// | present      | digest     | carry it if [`may_carry`] | then by projection            |
/// | present      | nothing    | —                         | then by projection            |
///
/// "By projection": `Withhold` when the head has a digest and the record
/// still projects nothing (never withdraw blind), `Publish` when the row
/// already equals the projection, otherwise `Corrected`. A withdrawn record
/// projects the marker, so a carried digest is never published over one.
pub(crate) fn apply_relay_head(
    agents: &mut [ManagedAgentRecord],
    agent: &str,
    row: &Announced,
    head: &Announced,
) -> (CarryHookOutcome, bool) {
    let Some(record) = agents.iter_mut().find(|record| record.pubkey == agent) else {
        let decision = match head {
            Announced::Nothing => CarryHookOutcome::Publish,
            Announced::Digest(_) | Announced::Withdrawn => CarryHookOutcome::Withhold,
        };
        return (decision, false);
    };
    let changed = match head {
        Announced::Withdrawn => mark_publication_withdrawn(record),
        Announced::Digest(digest)
            if may_carry(record)
                && record.carried_project_digest.as_deref() != Some(digest.as_str()) =>
        {
            record.carried_project_digest = Some(digest.clone());
            true
        }
        Announced::Digest(_) | Announced::Nothing => false,
    };
    let projected = published_association(record);
    let decision = if matches!(head, Announced::Digest(_)) && projected == Announced::Nothing {
        CarryHookOutcome::Withhold
    } else if projected == *row {
        CarryHookOutcome::Publish
    } else {
        CarryHookOutcome::Corrected
    };
    (decision, changed)
}

/// Whether a pending row announcing `row` is backed by `agent`'s own verified
/// public project here, so it publishes without reading the relay head.
pub(crate) fn backed_by_own_public(
    agents: &[ManagedAgentRecord],
    agent: &str,
    row: &Announced,
) -> bool {
    let Announced::Digest(digest) = row else {
        return false;
    };
    agents
        .iter()
        .find(|record| record.pubkey == agent)
        .and_then(own_public_digest)
        .is_some_and(|own| own == *digest)
}

/// The flush loop's view of this computer's managed-agent store. Both methods
/// run synchronously, never across an `.await`, and take the store lock.
pub(crate) trait CarryHook: Send + Sync {
    /// [`backed_by_own_public`] over this computer's records.
    fn backed(&self, agent: &str, row: &Announced) -> Result<bool, String>;
    /// [`apply_relay_head`] over this computer's records, saving a changed
    /// record and re-retaining the agent's row on `Corrected`.
    fn reconcile(
        &self,
        db_path: &Path,
        owner_keys: &nostr::Keys,
        agent: &str,
        row: &Announced,
        head: &Announced,
    ) -> Result<CarryHookOutcome, String>;
}

/// The production carry hook over the app's managed-agent store.
pub(crate) struct AppCarryHook(pub(crate) tauri::AppHandle);

impl AppCarryHook {
    fn with_records<T>(
        &self,
        apply: impl FnOnce(&mut Vec<ManagedAgentRecord>) -> Result<T, String>,
    ) -> Result<T, String> {
        use tauri::Manager;
        let state = self
            .0
            .try_state::<AppState>()
            .ok_or_else(|| "app state is unavailable".to_string())?;
        let _guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut agents = super::load_managed_agents(&self.0)?;
        apply(&mut agents)
    }
}

impl CarryHook for AppCarryHook {
    fn backed(&self, agent: &str, row: &Announced) -> Result<bool, String> {
        self.with_records(|agents| Ok(backed_by_own_public(agents, agent, row)))
    }

    fn reconcile(
        &self,
        db_path: &Path,
        owner_keys: &nostr::Keys,
        agent: &str,
        row: &Announced,
        head: &Announced,
    ) -> Result<CarryHookOutcome, String> {
        self.with_records(|agents| {
            let (decision, changed) = apply_relay_head(agents, agent, row, head);
            if changed {
                super::save_managed_agents(&self.0, agents)?;
            }
            if decision == CarryHookOutcome::Corrected {
                let record = agents
                    .iter()
                    .find(|record| record.pubkey == agent)
                    .ok_or_else(|| format!("agent {agent} disappeared during carry"))?;
                super::reconcile::retain_agent_record(
                    &open_retention_db(db_path)?,
                    owner_keys,
                    record,
                )?;
            }
            Ok(decision)
        })
    }
}

/// What the flush loop does with one pending row after the guard.
#[derive(Debug)]
pub(crate) enum FlushGuard {
    /// Publish the row it was given.
    Proceed,
    /// Leave it pending this sweep.
    Skip,
    /// Publish this retained head instead (the corrected row).
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
            "withholding a kind:30177: the relay head could not be read, so publishing could withdraw another computer's project association or undo a withdrawal"
        );
    }
}

fn clear_read_failure(agent: &str) {
    if let Ok(mut logged) = READ_FAILURE_LOGGED.lock() {
        logged.remove(agent);
    }
}

/// What a kind:30177 content announces; unparseable content announces nothing.
pub(crate) fn content_announced(content: &str) -> Announced {
    let Ok(content) =
        serde_json::from_str::<super::agent_events::ManagedAgentEventContent>(content)
    else {
        return Announced::Nothing;
    };
    if content.project_withdrawn {
        return Announced::Withdrawn;
    }
    content
        .project_digest
        .filter(|digest| is_project_digest(digest))
        .map_or(Announced::Nothing, Announced::Digest)
}

fn d_tag(event: &nostr::Event) -> Option<&str> {
    event.tags.iter().find_map(|tag| {
        let values = tag.as_slice();
        (values.first().map(String::as_str) == Some("d"))
            .then(|| values.get(1).map(String::as_str))
            .flatten()
    })
}

/// What the newest signed kind:30177 by `owner` at `agent` among `events`
/// announces. Events with a bad signature, another author, kind or d tag are
/// ignored; ties on `created_at` go to the lowest id, as NIP-01 keeps.
pub(crate) fn relay_head(
    events: &[nostr::Event],
    owner: &nostr::PublicKey,
    agent: &str,
) -> Announced {
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
        .map_or(Announced::Nothing, |head| content_announced(&head.content))
}

/// The flush loop's publish-time guard for one pending row.
///
/// Anything but a kind:30177, and a kind:30177 whose digest is this
/// computer's own verified public project's, proceeds untouched. Every other
/// kind:30177 (digest-less, marked, or carrying another computer's digest)
/// reads the relay head at `{kinds:[30177], authors:[owner], #d:[agent]}`:
/// - read failure: skip (fail closed), logged once per agent;
/// - otherwise `hook` reconciles ([`apply_relay_head`]); a correction
///   publishes the retained head in place of this row, and a digest is never
///   published over a relay marker unless it is this computer's own.
pub(crate) async fn guard_managed_agent_withdrawal(
    current: &RetainedEvent,
    state: &AppState,
    relay_api_base: &str,
    db_path: &Path,
    owner_keys: &nostr::Keys,
    hook: &dyn CarryHook,
) -> Result<FlushGuard, String> {
    if current.kind != KIND_MANAGED_AGENT {
        return Ok(FlushGuard::Proceed);
    }
    let agent = current.d_tag.as_str();
    let row = content_announced(&current.content);
    match hook.backed(agent, &row) {
        Ok(true) => return Ok(FlushGuard::Proceed),
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(agent, %error, "withholding a kind:30177: the local association could not be read");
            return Ok(FlushGuard::Skip);
        }
    }
    let owner = owner_keys.public_key();
    let filter = serde_json::json!({
        "kinds": [KIND_MANAGED_AGENT],
        "authors": [owner.to_hex()],
        "#d": [agent],
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
            log_read_failure_once(agent, &error);
            return Ok(FlushGuard::Skip);
        }
    };
    clear_read_failure(agent);
    let head = relay_head(&events, &owner, agent);
    match hook.reconcile(db_path, owner_keys, agent, &row, &head) {
        Ok(CarryHookOutcome::Publish) => Ok(FlushGuard::Proceed),
        Ok(CarryHookOutcome::Withhold) => Ok(FlushGuard::Skip),
        Ok(CarryHookOutcome::Corrected) => {
            let conn = open_retention_db(db_path)?;
            let Some(corrected) = get_retained_event(&conn, current.kind, &current.pubkey, agent)?
                .filter(|corrected| corrected.pending_sync)
            else {
                return Ok(FlushGuard::Skip);
            };
            let announced = content_announced(&corrected.content);
            let safe = match (&head, &announced) {
                (Announced::Digest(_), Announced::Nothing) => false,
                (Announced::Withdrawn, Announced::Digest(_)) => {
                    hook.backed(agent, &announced).unwrap_or(false)
                }
                _ => true,
            };
            Ok(if safe {
                FlushGuard::Replaced(corrected)
            } else {
                FlushGuard::Skip
            })
        }
        Err(error) => {
            tracing::warn!(
                agent,
                %error,
                "withholding a kind:30177: reconciling the published association failed"
            );
            Ok(FlushGuard::Skip)
        }
    }
}

#[cfg(test)]
#[path = "project_association_carry_tests.rs"]
mod tests;
