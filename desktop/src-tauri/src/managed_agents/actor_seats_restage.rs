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
//! substitute pack. Repository packs are re-staged at the generation's original
//! resolved commit. A moving project branch cannot silently replace its skills;
//! unavailable or unprovable original packs leave a named custody obstacle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use tauri::AppHandle;

use crate::app_state::AppState;
use crate::managed_agents::actor_seats::{
    actor_seats_path, mutate_actor_seats_file, plan_seat_pack, read_actor_seats,
    seat_entry_for_plan, stage_actor_seat, ActorSeatsFile, SeatPackPreview,
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

/// The sentence a fenced row is skipped with.
///
/// One line, and a named constant, on purpose. Written as a `\`-continued
/// multi-line literal it read fine in the source and shipped with eighteen
/// spaces in the middle of it — a wrapped literal is a formatting decision the
/// operator ends up reading. The test asserts this exact string.
pub(crate) const FENCED_SKIP_REASON: &str =
    "HANDOVER_FENCED: this session has been handed over, so its seat is not re-staged on this computer";

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
    /// Recovery must reproduce this exact coordinate or refuse. It does not
    /// follow a project branch that has advanced since the generation began.
    #[serde(default)]
    pub pack_ref: Option<packs_cache::PackRef>,
    /// Whether the provider's handover fence stops this generation acting.
    ///
    /// Set when the umbrella has been handed over to somebody else's execution
    /// body, or when a claim over it was voided (`docs/HANDOVER_IMPL.md` §3).
    /// A fenced generation still exists and is still this provider's, so the
    /// provider states the row rather than dropping it — but re-staging it
    /// would file a usable signing key on disk for a seat that cannot take a
    /// turn, so this host skips it and says so.
    ///
    /// A **retired** generation, by contrast, is absent from the file
    /// entirely: its umbrella was deleted and it is never coming back. Absent
    /// here means `false`, which is what every row a pre-fence provider wrote
    /// means.
    #[serde(default)]
    pub fenced: bool,
}

/// The whole `seat-requests.json`: a version tag plus the open rows.
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct SeatRequestsFile {
    /// Schema version the provider wrote. Not yet consulted — reserved so a
    /// future incompatible shape can be told apart from this one.
    #[serde(default)]
    #[allow(dead_code)]
    pub version: u32,
    /// Process that last recovered and published this request snapshot.
    #[serde(default, rename = "providerPid")]
    pub provider_pid: u32,
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
        // Ahead of every other check, including whether the actor is managed
        // here: a fenced generation cannot take a turn on this machine no
        // matter how well its custody could be reconstructed, so staging its
        // key would put a live credential on disk for work this body is not
        // allowed to do. Skipped and named, never silently dropped — the
        // report is where an operator finds out the fence is why nothing
        // restarted.
        if request.fenced {
            report
                .skipped
                .push((key.to_string(), FENCED_SKIP_REASON.to_string()));
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
        if plan.pack_ref != request.pack_ref || (request.pack_ref.is_none() && plan.pack_staged) {
            report.skipped.push((key.to_string(),
                "ACTOR_UNAVAILABLE: the original generation's pack cannot be verified; refusing a substitute".to_string()));
            continue;
        }
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

/// Reconstruct the original resolved repository pin, never its moving branch.
fn pinned_pack_source(
    request: &SeatRequest,
) -> Result<Option<packs_cache::ProjectPackSource>, String> {
    let Some(pack) = &request.pack_ref else {
        return Ok(None);
    };
    if pack.role != request.role {
        return Err("ACTOR_UNAVAILABLE: the original pack names a different role".into());
    }
    if !pack.repo.starts_with("30617:") {
        return Ok(None);
    }
    let suffix = format!("/{}", request.role);
    let path = pack
        .path
        .strip_suffix(&suffix)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            "ACTOR_UNAVAILABLE: the original pack path cannot be reconstructed".to_string()
        })?;
    Ok(Some(packs_cache::ProjectPackSource {
        repo: pack.repo.clone(),
        git_ref: None,
        sha: Some(pack.sha.clone()),
        path: path.to_string(),
    }))
}

fn ensure_restage_relay(current: &str, expected: &str) -> Result<(), String> {
    if current != expected {
        return Err("agent seat re-stage cancelled because the active community changed".into());
    }
    Ok(())
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
    expected_relay: &str,
    expected_pid: u32,
    stop: &AtomicBool,
) -> Result<RestageReport, String> {
    let requests_file = read_seat_requests(&seat_requests_path(state_dir))?;
    if requests_file.requests.is_empty() {
        return Ok(RestageReport::default());
    }
    let relay_url = relay_ws_url_with_override(state);
    ensure_restage_relay(&relay_url, expected_relay)?;
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
        // A fenced row is refused by the core below without consulting this
        // map, so resolving a pack for it would spend a relay read on a seat
        // that will not be staged.
        if key.is_empty() || request.fenced || existing.pending.contains_key(&key) {
            continue;
        }
        let Some(record) = records.iter().find(|record| record.pubkey == request.actor) else {
            continue;
        };
        let setup =
            crate::managed_agents::project_team_setup::actor::restage::is_setup_actor(record);
        if setup {
            let plan = crate::managed_agents::project_team_setup::actor::restage::restage_plan(
                app,
                state,
                record,
                request.project_ref.as_deref(),
                &request.role,
                request.pack_ref.as_ref(),
            );
            resolved_packs.insert(key, plan);
            continue;
        }
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
        ensure_restage_relay(&relay_ws_url_with_override(state), expected_relay)?;
        let pack_source = match pinned_pack_source(request) {
            Ok(Some(source)) => Some(source),
            Ok(None) => pack_source,
            Err(error) => {
                resolved_packs.insert(key, Err(error));
                continue;
            }
        };
        let plan = plan_seat_pack(
            app,
            state,
            &records,
            record,
            Some(request.role.as_str()),
            pack_source,
            None,
            // A restage re-resolves `main`'s definition: the custody file
            // does not record the seat's worktree, so the § 4.9 branch
            // override is not re-checked here. Disclosed in ledger 145.
            None,
        );
        resolved_packs.insert(key, Ok(plan));
    }

    ensure_restage_relay(&relay_ws_url_with_override(state), expected_relay)?;
    if stop.load(Ordering::Acquire) {
        return Err("agent seat re-stage cancelled because the supervisor stopped".into());
    }
    let report = mutate_actor_seats_file(&seats_path, |current| {
        let latest = read_seat_requests(&seat_requests_path(state_dir))?;
        if latest.provider_pid != expected_pid {
            return Err("agent seat re-stage cancelled because the provider changed".into());
        }
        let (updated, report) = restage_actor_seats_with(
            &latest.requests,
            current,
            &records,
            &relay_url,
            &resolved_packs,
        );
        *current = updated;
        Ok(report)
    })?;
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
