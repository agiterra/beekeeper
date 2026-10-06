//! Native adapter for Project Pulse's declared work.
//!
//! A **separate** command from [`pulse_mission_rows`](super::pulse_mission),
//! deliberately: the mission response decoder refuses unknown top-level keys
//! and its fixture is pinned byte-for-byte from Rust, so the frozen mission
//! contract does not change to carry this (contract §3).
//!
//! This boundary supplies signed events and the verified authority projection,
//! and delegates every inclusion, settlement and terminal decision to
//! `buzz-core`'s
//! [`project_declared_work`](beekeeper_core_pkg::pulse_declared_work::project_declared_work),
//! which in turn re-implements no rule of the 44244 fold. Nothing here folds,
//! compares a path, or infers a repository.

use beekeeper_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat, CodingSessionTeamFoldContext,
};
use beekeeper_core_pkg::pulse_declared_work::{
    project_declared_work, PulseDeclaredWork, PulseDeclaredWorkError, PulseDeclaredWorkLifecycle,
    PulseDeclaredWorkSources, MAX_PULSE_DECLARED_WORK_SESSIONS, PULSE_DECLARED_WORK_SCHEMA,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

use super::pulse_mission::{PulseMissionGrantInput, PulseMissionSeatInput};

/// Closed wire-schema identifier accepted by this adapter.
pub const PULSE_DECLARED_WORK_REQUEST_SCHEMA: &str = "buzz-pulse-declared-work-request/v1";

/// One umbrella's inputs: its identity, its lifecycle, and its signed 44244 set.
///
/// `deny_unknown_fields`, and no field defaults: a caller that has not been
/// updated is refused by name rather than folded with a key silently missing.
/// The seat and grant shapes are the mission adapter's own types, so the two
/// reads cannot drift about what an active projection looks like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDeclaredWorkSessionInput {
    /// Umbrella key, as the Pulse digest keys sessions.
    pub session_key: String,
    /// Canonical channel UUID.
    pub channel_ref: String,
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Immutable genesis event id.
    pub genesis_ref: String,
    /// Pubkey of the genesis signer.
    pub founder_pubkey: String,
    /// The session's reported name, or null.
    pub name: Option<String>,
    /// Durable lifecycle, from the digest's session coordination fold.
    pub lifecycle: PulseDeclaredWorkLifecycle,
    /// Newest durable observation time, or null.
    pub latest_observation_at: Option<i64>,
    /// Active receipt-backed role seats.
    pub active_seats: Vec<PulseMissionSeatInput>,
    /// Active receipt-backed operator grants.
    pub active_grants: Vec<PulseMissionGrantInput>,
    /// Signed kind 44244 events, ascending by `(created_at, id)`.
    pub team_events: Vec<Event>,
}

/// Everything one page of declared work is read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDeclaredWorkRequest {
    /// Must equal [`PULSE_DECLARED_WORK_REQUEST_SCHEMA`].
    pub schema: String,
    /// The project coordinate this read is about, echoed for binding.
    pub project: String,
    /// The project's channel set — the floor this read was scoped to.
    pub channel_ids: Vec<String>,
    /// The wall-clock second the caller read its sources.
    ///
    /// Echoed for binding, and read by nothing here: this projection renders
    /// no age, so a clock skew cannot change a word it says.
    pub now_unix: i64,
    /// The viewer's own pubkey, or null when this surface has no identity.
    pub viewer_pubkey: Option<String>,
    /// The umbrellas to project, in the order the surface will show them.
    pub sessions: Vec<PulseDeclaredWorkSessionInput>,
    /// Reads that failed before this adapter was called.
    pub read_errors: Vec<PulseDeclaredWorkError>,
}

impl PulseDeclaredWorkRequest {
    fn validate(&self) -> Result<(), String> {
        if self.schema != PULSE_DECLARED_WORK_REQUEST_SCHEMA {
            return Err(format!(
                "unsupported pulse-declared-work request schema: {}",
                self.schema
            ));
        }
        if self.sessions.len() > MAX_PULSE_DECLARED_WORK_SESSIONS {
            return Err(format!(
                "pulse-declared-work request carries {} sessions; the cap is {MAX_PULSE_DECLARED_WORK_SESSIONS}",
                self.sessions.len()
            ));
        }
        Ok(())
    }
}

fn context(session: &PulseDeclaredWorkSessionInput) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: session.channel_ref.clone(),
        session_ref: session.session_ref.clone(),
        genesis_ref: session.genesis_ref.clone(),
        founder_pubkey: session.founder_pubkey.clone(),
        active_seats: session
            .active_seats
            .iter()
            .map(|seat| CodingSessionTeamActiveSeat {
                actor_pubkey: seat.actor_pubkey.clone(),
                role: seat.role.clone(),
            })
            .collect(),
        active_grants: session
            .active_grants
            .iter()
            .map(|grant| CodingSessionTeamActiveGrant {
                actor_pubkey: grant.actor_pubkey.clone(),
                grant_event_ref: grant.grant_event_ref.clone(),
                may_steer: grant.may_steer,
            })
            .collect(),
        // This read does not gather the umbrella's 44245 policy set, so it has
        // no `gates.verifierRequired` to hand the fold. `false` is what the
        // field requires of such a caller — the fold then behaves exactly as it
        // did before the field existed — and the declared-work response carries
        // no field that could render this `false` as "no verifier is required".
        verifier_required: false,
    }
}

/// Project every supplied umbrella's declared work, in request order.
fn declared_work(request: PulseDeclaredWorkRequest) -> Result<PulseDeclaredWork, String> {
    request.validate()?;

    // The caller's own failed reads come first: they happened before anything
    // here ran, and a surface that reordered them would misdate the failure.
    let mut errors = request.read_errors.clone();
    let mut sessions = Vec::with_capacity(request.sessions.len());

    for session in &request.sessions {
        let context = context(session);
        let projected = project_declared_work(&PulseDeclaredWorkSources {
            session_key: &session.session_key,
            channel_id: &session.channel_ref,
            session_ref: &session.session_ref,
            name: session.name.as_deref(),
            lifecycle: session.lifecycle,
            latest_observation_at: session.latest_observation_at,
            context: &context,
            team_events: &session.team_events,
        });
        // A failed fold is one session's failure, disclosed by name, and the
        // rest of the page is untouched. The session is still returned, so the
        // surface can render "records unreadable" rather than "no work".
        if let Some(reason) = &projected.unreadable {
            errors.push(PulseDeclaredWorkError {
                scope: format!("declared:{}", session.session_key),
                message: reason.clone(),
            });
        }
        sessions.push(projected);
    }

    Ok(PulseDeclaredWork {
        schema: PULSE_DECLARED_WORK_SCHEMA.to_owned(),
        viewer_pubkey: request.viewer_pubkey,
        sessions,
        errors,
    })
}

/// Project one page of Project Pulse's declared work.
#[tauri::command]
pub async fn pulse_declared_work(
    request: PulseDeclaredWorkRequest,
) -> Result<PulseDeclaredWork, String> {
    tauri::async_runtime::spawn_blocking(move || declared_work(request))
        .await
        .map_err(|error| format!("pulse declared-work task failed: {error}"))?
}

#[cfg(test)]
#[path = "pulse_declared_work_tests.rs"]
mod tests;
