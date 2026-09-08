//! Re-stage custody for seats the coding-session provider already owns.
//!
//! [`crate::managed_agents::actor_seats`] stages a seat's key material
//! *before* the create that names it is published — a one-shot, consumed at
//! spawn. That channel has no answer for a provider that restarts: an open
//! seated generation the provider is trying to restore natively
//! (`docs/CI_CONTINUATION_RECOVERY_SPEC.md` §2) needs the same custody again,
//! but no create is being published this time, so nothing would ever call
//! `stage_coding_session_actor_seat`.
//!
//! The provider closes that gap on its side: it maintains `seat-requests.json`
//! in its state dir, one row per open seated generation, rewritten on create,
//! resume, stop/close, and recovery (`crates/buzz-session-provider`, module
//! `seat_requests`). It carries no secret — an `actor` pubkey, a `role`, an
//! optional project reference, and identifiers, nothing this desktop did not
//! already know. This module is the other side: after every provider (re)start
//! the supervisor calls [`restage_actor_seats_for_provider`], which reads that
//! file and, for every row with no seat already staged under its `commandId`,
//! re-derives the same custody `stage_coding_session_actor_seat` would have
//! filed — same keyring hydration, same pack-selection rule
//! ([`crate::managed_agents::actor_seats::plan_seat_pack`]), same refusals —
//! and files it under that row's `commandId` so the provider's restore path
//! finds it waiting.
//!
//! An actor this computer does not manage, or whose secret the keyring cannot
//! produce this boot, is skipped with the same sentence the create-time path
//! would have refused it with — never a substitute identity, never a
//! substitute pack. A project's pack source is always re-read from the relay
//! rather than trusted from the row's own `packRef`: a pack can move between
//! restarts, and the row's `packRef` is what was staged *before*, not a
//! promise of what to stage now.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tauri::AppHandle;

use crate::app_state::AppState;
use crate::managed_agents::actor_seats::{
    actor_seats_path, plan_seat_pack, read_actor_seats, seat_entry_for_plan, stage_actor_seat,
    write_actor_seats, ActorSeatsFile, SeatPackPreview,
};
use crate::managed_agents::packs_cache;
use crate::managed_agents::role_packs_view::fetch_project_pack_source;
use crate::managed_agents::storage::load_managed_agents;
use crate::managed_agents::types::ManagedAgentRecord;
use crate::relay::relay_ws_url_with_override;

/// Filename of the provider's seat-request ledger, a sibling of
/// [`crate::managed_agents::actor_seats::ACTOR_SEATS_FILE_NAME`] inside the
/// provider's state dir.
pub(crate) const SEAT_REQUESTS_FILE_NAME: &str = "seat-requests.json";

/// One row of `seat-requests.json`: an open seated generation whose custody
/// may need to be re-staged. The provider writes this file; the host only
/// reads it (see the module docs). Field names are the wire contract with the
/// provider's writer — camelCase on disk like every other host-local file in
/// this channel.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeatRequest {
    /// The generation's command id — the exact key [`stage_actor_seat`] would
    /// file custody under, and the key the provider's restore path reads
    /// custody back from.
    pub command_id: String,
    /// The seat's public key. Must name a managed agent on this computer or
    /// the row is skipped, never substituted.
    pub actor: String,
    /// The seat's role, exactly as it was given at create time. Picks the
    /// pack ([`plan_seat_pack`]) the same way it did then.
    pub role: String,
    /// `30621:<owner>:<id>`, when the generation is seated within a project.
    /// `None` takes the local rungs, exactly as an unseated-by-project create
    /// does today.
    #[serde(default)]
    pub project_ref: Option<String>,
    /// The session this generation belongs to. Not consulted by the staging
    /// decision; parsed for full fidelity with the provider's wire shape and
    /// available to a future log line, never read yet — `#[allow(dead_code)]`
    /// says exactly that rather than leaving an unexplained warning.
    #[serde(default)]
    #[allow(dead_code)]
    pub session_id: String,
    /// The generation number. Not consulted here — re-staging never bumps a
    /// generation, it only re-supplies the same seat's key material for the
    /// generation that is already open. Parsed for schema fidelity; unread
    /// today.
    #[serde(default)]
    #[allow(dead_code)]
    pub generation: u64,
    /// The pack the provider staged this seat with before the restart.
    /// Deliberately unused for the restaging decision (see the module docs):
    /// the pack is always re-resolved, never trusted from this field. Parsed
    /// so the row round-trips faithfully; unread today.
    #[serde(default)]
    #[allow(dead_code)]
    pub pack_ref: Option<packs_cache::PackRef>,
}

/// The whole `seat-requests.json`: a version tag plus the open rows.
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct SeatRequestsFile {
    /// Schema version the provider wrote. Not yet consulted — reserved so a
    /// future incompatible shape can be told apart from this one.
    #[serde(default)]
    #[allow(dead_code)]
    pub version: u32,
    /// The open seated generations, one row each.
    #[serde(default)]
    pub requests: Vec<SeatRequest>,
}

/// Path of the seat-request file inside a provider state dir.
pub(crate) fn seat_requests_path(state_dir: &Path) -> PathBuf {
    state_dir.join(SEAT_REQUESTS_FILE_NAME)
}

/// Read the provider's seat-request ledger.
///
/// A missing file is an empty one: the provider has not written it yet, or
/// every open generation has already been re-staged and its row cleared —
/// either way there is nothing to re-stage. A malformed file is an error, the
/// same rule [`crate::managed_agents::actor_seats::read_actor_seats`] uses for
/// its own file: silently treating malformed content as empty would leave a
/// real request unserved rather than surfacing the parse failure.
pub(crate) fn read_seat_requests(path: &Path) -> Result<SeatRequestsFile, String> {
    if !path.exists() {
        return Ok(SeatRequestsFile::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(SeatRequestsFile::default());
    }
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

/// What one [`restage_actor_seats_for_provider`] call did.
///
/// A caller has to be honest about it: `staged` is what actually changed on
/// disk, `already_present` is what needed nothing, and `skipped` names every
/// row this call could not honour and why — never silently dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RestageReport {
    /// Rows read from `seat-requests.json`.
    pub requested: usize,
    /// Seats newly staged by this call.
    pub staged: usize,
    /// Rows whose seat was already staged under that `commandId` — left
    /// untouched.
    pub already_present: usize,
    /// `(commandId, reason)` for every row this call did not stage.
    pub skipped: Vec<(String, String)>,
}

/// The decision core: given the requests, the seats file as it stood, the
/// managed-agent records (already keyring-hydrated), and a pack resolution
/// already computed for every row that needs one, decide what to stage.
///
/// Split out from [`restage_actor_seats_for_provider`] so the staging and
/// skip logic is testable without a live `AppHandle` or a relay — this
/// crate's tests never build one. `resolved_packs` is keyed by trimmed
/// `commandId` and carries either the plan [`plan_seat_pack`] produced or the
/// reason a project's pack source could not be read; a row that needed no
/// resolution (already staged, or its actor is not managed here) is decided
/// before this map is ever consulted.
pub(crate) fn restage_actor_seats_with(
    requests: &[SeatRequest],
    existing: &ActorSeatsFile,
    records: &[ManagedAgentRecord],
    relay_url: &str,
    resolved_packs: &BTreeMap<String, Result<SeatPackPreview, String>>,
) -> (ActorSeatsFile, RestageReport) {
    let mut file = existing.clone();
    let mut report = RestageReport {
        requested: requests.len(),
        ..RestageReport::default()
    };
    for request in requests {
        let key = request.command_id.trim();
        if key.is_empty() {
            report.skipped.push((
                request.command_id.clone(),
                "an agent seat needs the create's commandId".to_string(),
            ));
            continue;
        }
        if file.pending.contains_key(key) {
            report.already_present += 1;
            continue;
        }
        let Some(record) = records.iter().find(|record| record.pubkey == request.actor) else {
            report.skipped.push((
                key.to_string(),
                format!(
                    "agent {} is not a managed agent on this computer",
                    request.actor
                ),
            ));
            continue;
        };
        let plan = match resolved_packs.get(key) {
            Some(Ok(plan)) => plan.clone(),
            Some(Err(reason)) => {
                report.skipped.push((key.to_string(), reason.clone()));
                continue;
            }
            None => {
                report.skipped.push((
                    key.to_string(),
                    "seat re-stage: no pack resolution recorded for this request".to_string(),
                ));
                continue;
            }
        };
        let entry = match seat_entry_for_plan(record, relay_url, plan) {
            Ok(entry) => entry,
            Err(reason) => {
                report.skipped.push((key.to_string(), reason));
                continue;
            }
        };
        if let Err(reason) = stage_actor_seat(&mut file, key, entry) {
            report.skipped.push((key.to_string(), reason));
            continue;
        }
        report.staged += 1;
    }
    (file, report)
}

/// Re-stage custody for every open seated generation the provider's
/// `seat-requests.json` names but has no seat filed for yet.
///
/// Called once after the provider's initial spawn and again after every
/// successful respawn ([`crate::session_provider::supervisor`]) — the
/// supervisor already owns the restart event, so no polling and no new IPC
/// are needed. Idempotent: a row whose seat is already staged is left alone
/// and counted `already_present`, so calling this after every restart is
/// exactly as safe as calling it once.
///
/// Holds [`AppState::managed_agents_store_lock`] for the record read, the same
/// lock [`crate::managed_agents::actor_seats::stage_coding_session_actor_seat`]
/// holds, so a re-stage never races a save of the same store.
///
/// Never stages a substitute: an actor not managed on this computer, a
/// managed actor whose secret the keyring cannot produce this boot, or a
/// project pack source the relay would not read back are each skipped with
/// the same sentence the create-time path would have refused them with — see
/// [`restage_actor_seats_with`] for exactly which sentence.
pub(crate) async fn restage_actor_seats_for_provider(
    app: &AppHandle,
    state: &AppState,
    state_dir: &Path,
) -> Result<RestageReport, String> {
    let requests_file = read_seat_requests(&seat_requests_path(state_dir))?;
    if requests_file.requests.is_empty() {
        return Ok(RestageReport::default());
    }
    let relay_url = relay_ws_url_with_override(state);
    let seats_path = actor_seats_path(state_dir);
    let (records, existing) = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = load_managed_agents(app)?;
        let existing = read_actor_seats(&seats_path)?;
        (records, existing)
    };

    // Resolve a pack only for rows that will actually need one: already-staged
    // rows and rows for an unmanaged actor are decided by the core without
    // ever consulting this map, so there is no reason to read the relay for
    // them.
    let mut resolved_packs: BTreeMap<String, Result<SeatPackPreview, String>> = BTreeMap::new();
    for request in &requests_file.requests {
        let key = request.command_id.trim().to_string();
        if key.is_empty() || existing.pending.contains_key(&key) {
            continue;
        }
        let Some(record) = records.iter().find(|record| record.pubkey == request.actor) else {
            continue;
        };
        let pack_source = match request.project_ref.as_deref() {
            Some(project_ref) => match fetch_project_pack_source(state, project_ref).await {
                Ok(source) => source,
                Err(error) => {
                    resolved_packs.insert(key, Err(error));
                    continue;
                }
            },
            None => None,
        };
        let plan = plan_seat_pack(
            app,
            state,
            &records,
            record,
            Some(request.role.as_str()),
            pack_source,
            None,
        );
        resolved_packs.insert(key, Ok(plan));
    }

    let (updated, report) = restage_actor_seats_with(
        &requests_file.requests,
        &existing,
        &records,
        &relay_url,
        &resolved_packs,
    );
    if report.staged > 0 {
        write_actor_seats(&seats_path, &updated)?;
    }
    for (command_id, reason) in &report.skipped {
        tracing::debug!(command_id = %command_id, %reason, "seat re-stage: skipped a row");
    }
    tracing::info!(
        requested = report.requested,
        staged = report.staged,
        already_present = report.already_present,
        skipped = report.skipped.len(),
        "seat re-stage after coding-session provider (re)start"
    );
    Ok(report)
}

#[cfg(test)]
#[path = "actor_seats_restage_tests.rs"]
mod tests;
