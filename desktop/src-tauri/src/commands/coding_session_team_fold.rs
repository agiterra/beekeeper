//! Native adapter for the canonical NIP-CSTX team-transaction fold.
//!
//! The desktop supplies one already-verified authority projection plus raw
//! signed events. This boundary re-verifies every event and delegates all
//! graph, authority, settlement, and terminal semantics to `buzz-core`.

use buzz_core_pkg::coding_session_team_transaction::{
    fold_coding_session_team_transactions as fold_core_coding_session_team_transactions,
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat, CodingSessionTeamFoldConflict,
    CodingSessionTeamFoldContext, CodingSessionTeamFoldExclusion,
    CodingSessionTeamFoldExclusionCode,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

/// Closed wire-schema identifier accepted by this adapter.
pub const CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA: &str =
    "buzz-coding-session-team-fold-request/v1";
/// Closed wire-schema identifier for this native adapter.
pub const CODING_SESSION_TEAM_FOLD_ADAPTER_SCHEMA: &str =
    "buzz-coding-session-team-fold-adapter/v1";

/// One active signed role seat in the verified authority projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamActiveSeatInput {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Canonical role slug.
    pub role: String,
    /// Accepted grant-seat transition that produced this active seat.
    pub grant_event_ref: String,
}

/// One active signed operator grant in the verified authority projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamActiveGrantInput {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Signed grant event from which the active projection was derived.
    pub grant_event_ref: String,
    /// Whether this grant confers steering authority.
    pub may_steer: bool,
}

/// Exact session and authority snapshot against which the native fold runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamFoldAdapterContext {
    /// Canonical channel UUID expected in every event's `h` tag.
    pub channel_ref: String,
    /// Canonical umbrella UUID expected in every event and `d` tag.
    pub session_ref: String,
    /// Immutable session-genesis event id expected in every event.
    pub genesis_ref: String,
    /// Pubkey of the session-genesis signer.
    pub founder_pubkey: String,
    /// Receipt-backed accepted 44228 head event id, or null before the first link.
    pub authority_head_event_id: Option<String>,
    /// Accepted authority-chain sequence; zero exactly before the first link.
    pub authority_head_seq: u32,
    /// Active receipt-backed role seats at that authority head.
    pub active_seats: Vec<CodingSessionTeamActiveSeatInput>,
    /// Active receipt-backed operator grants at that authority head.
    pub active_grants: Vec<CodingSessionTeamActiveGrantInput>,
}

impl CodingSessionTeamFoldAdapterContext {
    fn validate_authority_binding(&self) -> Result<(), String> {
        for seat in &self.active_seats {
            validate_lower_hex_64("context.activeSeats.grantEventRef", &seat.grant_event_ref)?;
        }
        match (&self.authority_head_event_id, self.authority_head_seq) {
            (Some(event_id), seq) => {
                validate_lower_hex_64("context.authorityHeadEventId", event_id)?;
                if seq == 0 {
                    return Err("context.authorityHeadSeq must be at least 1".into());
                }
            }
            (None, 0) => {
                if !self.active_seats.is_empty() || !self.active_grants.is_empty() {
                    return Err(
                        "an active authority projection requires authority-head provenance".into(),
                    );
                }
            }
            (None, _) => {
                return Err("context.authorityHeadEventId must be present when authorityHeadSeq is positive".into());
            }
        }
        Ok(())
    }

    fn to_core(&self) -> CodingSessionTeamFoldContext {
        CodingSessionTeamFoldContext {
            channel_ref: self.channel_ref.clone(),
            session_ref: self.session_ref.clone(),
            genesis_ref: self.genesis_ref.clone(),
            founder_pubkey: self.founder_pubkey.clone(),
            active_seats: self
                .active_seats
                .iter()
                .map(|seat| CodingSessionTeamActiveSeat {
                    actor_pubkey: seat.actor_pubkey.clone(),
                    role: seat.role.clone(),
                })
                .collect(),
            active_grants: self
                .active_grants
                .iter()
                .map(|grant| CodingSessionTeamActiveGrant {
                    actor_pubkey: grant.actor_pubkey.clone(),
                    grant_event_ref: grant.grant_event_ref.clone(),
                    may_steer: grant.may_steer,
                })
                .collect(),
        }
    }
}

/// Caller-owned inputs accepted by the native fold boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamFoldAdapterRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// Exact verified authority/session projection for this invocation.
    pub context: CodingSessionTeamFoldAdapterContext,
    /// Caller-observed event ids, already sorted and checked against `events`.
    pub input_event_ids: Vec<String>,
    /// Raw signed kind-44244 Nostr events. No projected outputs are accepted.
    pub events: Vec<serde_json::Value>,
}

/// Exact authority/session binding echoed without the private projection sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterContextEcho {
    /// Canonical channel UUID used by the fold.
    pub channel_ref: String,
    /// Canonical umbrella UUID used by the fold.
    pub session_ref: String,
    /// Immutable session-genesis event id used by the fold.
    pub genesis_ref: String,
    /// Pubkey of the session-genesis signer used by the fold.
    pub founder_pubkey: String,
    /// Receipt-backed accepted 44228 head event id.
    pub authority_head_event_id: Option<String>,
    /// Accepted authority-chain sequence; zero exactly before the first link.
    pub authority_head_seq: u32,
}

impl From<&CodingSessionTeamFoldAdapterContext> for CodingSessionTeamFoldAdapterContextEcho {
    fn from(value: &CodingSessionTeamFoldAdapterContext) -> Self {
        Self {
            channel_ref: value.channel_ref.clone(),
            session_ref: value.session_ref.clone(),
            genesis_ref: value.genesis_ref.clone(),
            founder_pubkey: value.founder_pubkey.clone(),
            authority_head_event_id: value.authority_head_event_id.clone(),
            authority_head_seq: value.authority_head_seq,
        }
    }
}

/// Stable reason a signed event was excluded from the canonical projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingSessionTeamFoldAdapterExclusionCode {
    /// Signer lacked the required active authority.
    Unauthorized,
    /// A causal or `supersedes` reference named an event absent from the
    /// supplied set, so this one record could not be placed in the graph.
    DanglingReference,
    /// This record's claim to correct another was invalid, so it alone was
    /// rejected and the record it named was left untouched.
    InvalidCorrection,
    /// A reference resolved to a supplied record that cannot stand where this
    /// record put it — wrong operation type, wrong verdict subtype, or a
    /// pointer contradicting the record it names.
    WrongTypeReference,
    /// A required parent was unauthorized.
    DependentOnUnauthorized,
    /// A required parent was superseded or lost a correction conflict.
    DependentOnSuperseded,
    /// A required parent was excluded for another stable reason.
    DependentOnExcluded,
    /// A valid correction replaced this event.
    Superseded,
    /// Another correction head won deterministic ordering.
    CorrectionConflict,
    /// Mission completion lacked a settled approval chain.
    CompletionNotApproved,
    /// Another authorized terminal event won deterministic ordering.
    TerminalConflict,
}

/// One excluded signed event with stable code and diagnostic provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterExclusion {
    /// Excluded event id.
    pub event_id: String,
    /// Stable exclusion class.
    pub code: CodingSessionTeamFoldAdapterExclusionCode,
    /// Core-produced diagnostic.
    pub reason: String,
}

/// One deterministic choice among competing semantic heads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterConflict {
    /// Stable logical subject of the conflict.
    pub subject: String,
    /// Winner selected by core ordering.
    pub winner_event_id: String,
    /// All competing heads in deterministic order.
    pub contender_event_ids: Vec<String>,
}

/// Canonical approval state for one active assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterSettlement {
    /// Active assignment event id.
    pub assignment_event_id: String,
    /// Explicitly governed report, when the approval chain is complete.
    pub governed_report_event_id: Option<String>,
    /// Approving disposition acknowledged by the assigned actor.
    pub disposition_event_id: Option<String>,
    /// Assigned actor acknowledgement of that disposition.
    pub acknowledgement_event_id: Option<String>,
    /// Whether the full approval and acknowledgement chain is complete.
    pub settled: bool,
}

/// One canonical report whose author holds no active seat for the role its
/// assignment named.
///
/// The report is *included*: assignee equality is the inclusion rule, and this
/// says only that the included report carries no seat authority. Rendering it
/// as an exclusion would repeat the honesty bug this field exists to fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterUnseatedReport {
    /// Event id of the included report.
    pub event_id: String,
    /// Canonical lowercase-hex pubkey that signed the report.
    pub author_pubkey: String,
    /// Event id of the assignment the report answers.
    pub assignment_ref: String,
    /// Role slug that assignment named for its assignee.
    pub assignee_role: String,
}

/// Canonical newest authorized terminal record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterTerminal {
    /// Terminal event id.
    pub event_id: String,
    /// Exactly `mission.completed` or `mission.blocked`.
    #[serde(rename = "type")]
    pub transaction_type: String,
}

/// Closed native response derived only from verified events and core fold output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamFoldAdapterResponse {
    /// Exact adapter schema identifier.
    pub schema: String,
    /// Canonical implementation that produced every projected output.
    pub implementation: String,
    /// Exact input event ids, sorted lexicographically.
    pub input_event_ids: Vec<String>,
    /// Exact context and authority-head snapshot supplied for the fold.
    pub context: CodingSessionTeamFoldAdapterContextEcho,
    /// Canonical active facts after authority and correction projection.
    pub included_event_ids: Vec<String>,
    /// Rejected or displaced events with explicit provenance.
    pub excluded: Vec<CodingSessionTeamFoldAdapterExclusion>,
    /// Deterministically resolved correction, governance, and terminal conflicts.
    pub conflicts: Vec<CodingSessionTeamFoldAdapterConflict>,
    /// Canonical settlement state for every active assignment.
    pub assignments: Vec<CodingSessionTeamFoldAdapterSettlement>,
    /// Included reports whose author holds no active seat for the assignment's
    /// role, in included order. Always present; empty is a real answer.
    pub unseated_reports: Vec<CodingSessionTeamFoldAdapterUnseatedReport>,
    /// Canonical newest authorized terminal record, never inferred from silence.
    pub canonical_terminal: Option<CodingSessionTeamFoldAdapterTerminal>,
}

fn validate_lower_hex_64(field: &str, value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(format!("{field} must be a lowercase 64-hex event id"))
    }
}

fn exclusion_code(
    code: CodingSessionTeamFoldExclusionCode,
) -> CodingSessionTeamFoldAdapterExclusionCode {
    match code {
        CodingSessionTeamFoldExclusionCode::Unauthorized => {
            CodingSessionTeamFoldAdapterExclusionCode::Unauthorized
        }
        CodingSessionTeamFoldExclusionCode::DanglingReference => {
            CodingSessionTeamFoldAdapterExclusionCode::DanglingReference
        }
        CodingSessionTeamFoldExclusionCode::InvalidCorrection => {
            CodingSessionTeamFoldAdapterExclusionCode::InvalidCorrection
        }
        CodingSessionTeamFoldExclusionCode::WrongTypeReference => {
            CodingSessionTeamFoldAdapterExclusionCode::WrongTypeReference
        }
        CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized => {
            CodingSessionTeamFoldAdapterExclusionCode::DependentOnUnauthorized
        }
        CodingSessionTeamFoldExclusionCode::DependentOnSuperseded => {
            CodingSessionTeamFoldAdapterExclusionCode::DependentOnSuperseded
        }
        CodingSessionTeamFoldExclusionCode::DependentOnExcluded => {
            CodingSessionTeamFoldAdapterExclusionCode::DependentOnExcluded
        }
        CodingSessionTeamFoldExclusionCode::Superseded => {
            CodingSessionTeamFoldAdapterExclusionCode::Superseded
        }
        CodingSessionTeamFoldExclusionCode::CorrectionConflict => {
            CodingSessionTeamFoldAdapterExclusionCode::CorrectionConflict
        }
        CodingSessionTeamFoldExclusionCode::CompletionNotApproved => {
            CodingSessionTeamFoldAdapterExclusionCode::CompletionNotApproved
        }
        CodingSessionTeamFoldExclusionCode::TerminalConflict => {
            CodingSessionTeamFoldAdapterExclusionCode::TerminalConflict
        }
    }
}

fn map_exclusion(value: CodingSessionTeamFoldExclusion) -> CodingSessionTeamFoldAdapterExclusion {
    CodingSessionTeamFoldAdapterExclusion {
        event_id: value.event_id,
        code: exclusion_code(value.code),
        reason: value.reason,
    }
}

fn map_conflict(value: CodingSessionTeamFoldConflict) -> CodingSessionTeamFoldAdapterConflict {
    CodingSessionTeamFoldAdapterConflict {
        subject: value.subject,
        winner_event_id: value.winner_event_id,
        contender_event_ids: value.contender_event_ids,
    }
}

fn fold_adapter(
    request: CodingSessionTeamFoldAdapterRequest,
) -> Result<CodingSessionTeamFoldAdapterResponse, String> {
    if request.schema != CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA}"
        ));
    }
    request.context.validate_authority_binding()?;
    for event_id in &request.input_event_ids {
        validate_lower_hex_64("inputEventIds", event_id)?;
    }
    if !request
        .input_event_ids
        .windows(2)
        .all(|pair| pair[0] < pair[1])
    {
        return Err("inputEventIds must be strictly sorted without duplicates".into());
    }
    let mut events = request
        .events
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            serde_json::from_value::<Event>(value)
                .map_err(|error| format!("events[{index}] is not a signed Nostr event: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    events.sort_by_key(|event| event.id.to_hex());

    let input_event_ids = events
        .iter()
        .map(|event| event.id.to_hex())
        .collect::<Vec<_>>();
    if input_event_ids != request.input_event_ids {
        return Err("inputEventIds do not exactly match the supplied signed events".into());
    }
    let fold = fold_core_coding_session_team_transactions(&events, &request.context.to_core())?;
    let context = CodingSessionTeamFoldAdapterContextEcho::from(&request.context);

    Ok(CodingSessionTeamFoldAdapterResponse {
        schema: CODING_SESSION_TEAM_FOLD_ADAPTER_SCHEMA.into(),
        implementation: "buzz-core".into(),
        input_event_ids,
        context,
        included_event_ids: fold.included_event_ids,
        excluded: fold.excluded.into_iter().map(map_exclusion).collect(),
        conflicts: fold.conflicts.into_iter().map(map_conflict).collect(),
        assignments: fold
            .assignments
            .into_iter()
            .map(|value| CodingSessionTeamFoldAdapterSettlement {
                assignment_event_id: value.assignment_event_id,
                governed_report_event_id: value.governed_report_event_id,
                disposition_event_id: value.disposition_event_id,
                acknowledgement_event_id: value.acknowledgement_event_id,
                settled: value.settled,
            })
            .collect(),
        unseated_reports: fold
            .unseated_reports
            .into_iter()
            .map(|value| CodingSessionTeamFoldAdapterUnseatedReport {
                event_id: value.event_id,
                author_pubkey: value.author_pubkey,
                assignment_ref: value.assignment_ref,
                assignee_role: value.assignee_role,
            })
            .collect(),
        canonical_terminal: fold.canonical_terminal.map(|value| {
            CodingSessionTeamFoldAdapterTerminal {
                event_id: value.event_id,
                transaction_type: value.transaction_type.as_str().into(),
            }
        }),
    })
}

/// Re-verify and canonically fold raw signed team transactions.
#[tauri::command]
pub async fn fold_coding_session_team_transactions(
    request: CodingSessionTeamFoldAdapterRequest,
) -> Result<CodingSessionTeamFoldAdapterResponse, String> {
    tauri::async_runtime::spawn_blocking(move || fold_adapter(request))
        .await
        .map_err(|error| format!("team-transaction fold task failed: {error}"))?
}

#[cfg(test)]
#[path = "coding_session_team_fold_tests.rs"]
mod tests;
