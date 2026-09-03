//! Native adapter for Project Pulse's mission rows.
//!
//! Desktop and `bee pulse digest` read **one** model. This boundary supplies
//! signed events and the verified authority projection, and delegates every
//! fold rule and every sentence to `buzz-core`'s
//! [`pulse_mission`](buzz_core_pkg::pulse_mission): the frontend receives
//! rendered strings and re-words nothing, so the two consumers cannot drift.
//!
//! Nothing here asks anyone to report. The rows are composed from the relay's
//! own kind 30618 ref state, the hooks' kind 44246 observations, and the signed
//! 44244/44245 records — see the module docs in `buzz-core`.

use std::collections::BTreeMap;

use buzz_core_pkg::coding_session_policy::CodingSessionPolicyGrant;
use buzz_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat, CodingSessionTeamFoldContext,
};
use buzz_core_pkg::pulse_mission::{
    fold_pulse_mission_row, open_rulings, pulse_mission_cap_disclosure, render_pulse_mission_lines,
    rulings_waiting_on_viewer, PulseMissionError, PulseMissionFacts, PulseMissionNames,
    PulseMissionRows, PulseMissionSources, PulseRefState, MAX_PULSE_MISSION_ROWS,
    PULSE_MISSION_ROWS_SCHEMA, PULSE_MISSION_SCOPE,
};
use buzz_core_pkg::pulse_overlap::{
    fold_pulse_overlaps, render_pulse_overlap_rows, PulseOverlapSide,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

/// Closed wire-schema identifier accepted by this adapter.
pub const PULSE_MISSION_REQUEST_SCHEMA: &str = "buzz-pulse-mission-rows-request/v1";

/// One active signed role seat in the verified authority projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseMissionSeatInput {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Canonical role slug.
    pub role: String,
}

/// One active signed operator grant in the verified authority projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseMissionGrantInput {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Signed grant event the active projection was derived from.
    pub grant_event_ref: String,
    /// Whether this grant confers steering authority.
    pub may_steer: bool,
    /// Unix seconds at which the relay accepted the transition.
    pub accepted_at: u64,
    /// Whether the transition granted rather than revoked steering.
    pub granted: bool,
}

/// One relay-signed ref, as the caller read it from kind 30618.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseMissionRefStateInput {
    /// Full ref name.
    pub ref_name: String,
    /// Commit the ref stands at.
    pub sha: String,
    /// Pubkey from the 30618 `p` tag — who moved it **last**.
    pub pusher_pubkey: String,
    /// The 30618 event's `created_at`, Unix seconds, or null.
    pub as_of: Option<i64>,
}

/// One umbrella's inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseMissionSessionInput {
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
    /// The session's name, or null.
    pub name: Option<String>,
    /// Newest durable observation time, or null.
    pub latest_observation_at: Option<i64>,
    /// Active receipt-backed role seats.
    pub active_seats: Vec<PulseMissionSeatInput>,
    /// Active receipt-backed operator grants.
    pub active_grants: Vec<PulseMissionGrantInput>,
    /// Seats the caller claims; each is kept only when the projection has it.
    pub claimed_seats: Vec<String>,
    /// Signed kind 44244 events, ascending.
    pub team_events: Vec<Event>,
    /// Signed kind 44245 events.
    pub policy_events: Vec<Event>,
    /// Signed kind 44246 events, ascending.
    pub observation_events: Vec<Event>,
    /// Relay-signed 30618 ref state for this umbrella's repo.
    pub ref_state: Vec<PulseMissionRefStateInput>,
    /// Paths the newest wip checkpoint named, for the overlap row.
    ///
    /// Empty until `checkpoint.files` lands (Lane L5), and an empty list
    /// produces no overlap row rather than a guessed one.
    pub overlap_files: Vec<String>,
    /// The commit that checkpoint named, or null.
    pub overlap_sha: Option<String>,
    /// That checkpoint's own time, or null.
    pub overlap_as_of: Option<i64>,
    /// The seat that made it, or null.
    pub overlap_author: Option<String>,
}

/// Everything the adapter needs for one digest's worth of mission rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseMissionRowsRequest {
    /// Must equal [`PULSE_MISSION_REQUEST_SCHEMA`].
    pub schema: String,
    /// The project coordinate this read is about, echoed for binding.
    #[serde(default)]
    pub project: String,
    /// The project's channel set — the floor this read was scoped to.
    #[serde(default)]
    pub channel_ids: Vec<String>,
    /// The wall-clock second the caller read its sources.
    #[serde(default)]
    pub now_unix: i64,
    /// The viewer's own pubkey, or null when this surface has no identity.
    #[serde(default)]
    pub viewer_pubkey: Option<String>,
    /// Display names by lowercase-hex pubkey.
    #[serde(default)]
    pub display_names: BTreeMap<String, String>,
    /// How many open sessions were in scope before any cap or unread session.
    #[serde(default)]
    pub open_session_count: usize,
    /// The sessions to fold, newest observation first, already capped.
    ///
    /// A caller that has not gathered a session's signed events yet supplies
    /// **none**, and [`unread_session_disclosure`] says so by name. Supplying a
    /// session with empty event lists would render an umbrella with no records
    /// as an umbrella whose records are empty, which are different facts.
    #[serde(default)]
    pub sessions: Vec<PulseMissionSessionInput>,
    /// Reads that failed before this adapter was called.
    #[serde(default)]
    pub read_errors: Vec<PulseMissionError>,
}

/// The disclosure a read owes when it folded fewer sessions than are in scope.
///
/// Distinct from the 8-row cap: the cap is a bound this surface chose, this is
/// a read that did not happen. Either way the number is stated rather than
/// rendered as a quiet project.
fn unread_session_disclosure(open_session_count: usize, read: usize) -> Option<PulseMissionError> {
    if read >= open_session_count.min(MAX_PULSE_MISSION_ROWS) {
        return None;
    }
    let unread = open_session_count.min(MAX_PULSE_MISSION_ROWS) - read;
    Some(PulseMissionError {
        scope: "missions".to_owned(),
        message: format!(
            "{open_session_count} open sessions in scope; {read} had their signed records read, so {unread} are not shown here"
        ),
    })
}

impl PulseMissionRowsRequest {
    fn validate(&self) -> Result<(), String> {
        if self.schema != PULSE_MISSION_REQUEST_SCHEMA {
            return Err(format!(
                "unsupported pulse-mission request schema: {}",
                self.schema
            ));
        }
        if self.sessions.len() > MAX_PULSE_MISSION_ROWS {
            return Err(format!(
                "pulse-mission request carries {} sessions; the cap is {MAX_PULSE_MISSION_ROWS}",
                self.sessions.len()
            ));
        }
        Ok(())
    }
}

fn context(session: &PulseMissionSessionInput) -> CodingSessionTeamFoldContext {
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
        // Pulse reads the session's 44245 records for the policy facts it
        // prints, but it does not hand this fold a `gates.verifierRequired`:
        // its digest is a read of many sessions at once and the flag is not in
        // the per-session input. `false` is what the field requires of a caller
        // that has not read the policy set — the fold then behaves exactly as
        // it did before the field existed — and a Pulse row must not be read as
        // saying no verifier is required. Threading it in is owed.
        verifier_required: false,
    }
}

fn policy_grants(session: &PulseMissionSessionInput) -> Vec<CodingSessionPolicyGrant> {
    use buzz_core_pkg::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
    session
        .active_grants
        .iter()
        .map(|grant| CodingSessionPolicyGrant {
            grantee: grant.actor_pubkey.clone(),
            accepted_at: grant.accepted_at,
            transition_type: if grant.granted && grant.may_steer {
                CodingSessionAuthorityTransitionType::GrantOperator
            } else {
                CodingSessionAuthorityTransitionType::Revoke
            },
        })
        .collect()
}

fn overlap_side(session: &PulseMissionSessionInput) -> Option<PulseOverlapSide> {
    // No paths, no row: an overlap asserted from files nobody published would
    // be exactly the guess this row exists to avoid.
    if session.overlap_files.is_empty() {
        return None;
    }
    Some(PulseOverlapSide {
        session_key: session.session_key.clone(),
        author_pubkey: session.overlap_author.clone()?,
        sha: session.overlap_sha.clone()?,
        as_of: session.overlap_as_of,
        files: session.overlap_files.clone(),
    })
}

/// Fold every supplied umbrella into the eight sibling keys Pulse renders.
fn mission_rows(request: PulseMissionRowsRequest) -> Result<PulseMissionRows, String> {
    request.validate()?;

    let names = PulseMissionNames {
        names: request.display_names.clone(),
        viewer: request.viewer_pubkey.clone(),
    };
    let founders: BTreeMap<String, String> = request
        .sessions
        .iter()
        .map(|session| (session.session_key.clone(), session.founder_pubkey.clone()))
        .collect();

    let mut facts: Vec<PulseMissionFacts> = Vec::with_capacity(request.sessions.len());
    let mut errors = request.read_errors.clone();
    let mut sides: Vec<PulseOverlapSide> = Vec::new();

    for session in &request.sessions {
        let context = context(session);
        let grants = policy_grants(session);
        let ref_state: Vec<PulseRefState> = session
            .ref_state
            .iter()
            .map(|state| PulseRefState {
                ref_name: state.ref_name.clone(),
                sha: state.sha.clone(),
                pusher_pubkey: state.pusher_pubkey.clone(),
                as_of: state.as_of,
            })
            .collect();
        let sources = PulseMissionSources {
            session_key: &session.session_key,
            channel_id: &session.channel_ref,
            name: session.name.as_deref(),
            latest_observation_at: session.latest_observation_at,
            context: &context,
            policy_grants: &grants,
            team_events: &session.team_events,
            policy_events: &session.policy_events,
            observation_events: &session.observation_events,
            ref_state: &ref_state,
            claimed_seats: &session.claimed_seats,
            gate_source: None,
        };
        let row = fold_pulse_mission_row(&sources, request.now_unix);
        // A failed read is one row's failure, disclosed by name, and the rest
        // of the digest is untouched.
        if let Some(reason) = &row.unreadable {
            errors.push(PulseMissionError {
                scope: format!("missions:{}", session.channel_ref),
                message: reason.clone(),
            });
        }
        if let Some(side) = overlap_side(session) {
            sides.push(side);
        }
        facts.push(row);
    }

    if let Some(disclosure) = pulse_mission_cap_disclosure(request.open_session_count) {
        errors.push(disclosure);
    }
    if let Some(disclosure) =
        unread_session_disclosure(request.open_session_count, request.sessions.len())
    {
        errors.push(disclosure);
    }

    let open = open_rulings(&facts);
    let founder_of = |session_key: &str| founders.get(session_key).cloned();
    let waiting = rulings_waiting_on_viewer(&open, request.viewer_pubkey.as_deref(), &founder_of);

    Ok(PulseMissionRows {
        missions_schema: PULSE_MISSION_ROWS_SCHEMA.to_owned(),
        mission_scope: PULSE_MISSION_SCOPE.to_owned(),
        missions: facts
            .iter()
            .map(|row| render_pulse_mission_lines(row, &names, request.now_unix))
            .collect(),
        mission_errors: errors,
        open_rulings: open,
        rulings_waiting_on_viewer: waiting,
        overlaps: render_pulse_overlap_rows(&fold_pulse_overlaps(&sides), &names, request.now_unix),
        viewer_pubkey: request.viewer_pubkey,
    })
}

/// Fold Project Pulse's mission rows for the sessions the caller supplies.
#[tauri::command]
pub async fn pulse_mission_rows(
    request: PulseMissionRowsRequest,
) -> Result<PulseMissionRows, String> {
    tauri::async_runtime::spawn_blocking(move || mission_rows(request))
        .await
        .map_err(|error| format!("pulse mission-row task failed: {error}"))?
}

#[cfg(test)]
#[path = "pulse_mission_tests.rs"]
mod tests;
