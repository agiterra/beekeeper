//! What one mission row is composed from, before any of it is a sentence.
//!
//! Split out of [`super`] so no file here passes 1,000 lines. A child module,
//! so it reads its parent's items unchanged.

use serde::{Deserialize, Serialize};

use super::PulseGateSource;
use crate::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationPhase,
};

// ── Facts ────────────────────────────────────────────────────────────────────

/// The mission's own state word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PulseMissionState {
    /// No canonical terminal.
    Running,
    /// A canonical `mission.blocked`.
    Blocked,
    /// A canonical `mission.completed`.
    Completed,
    /// The 44244 fold returned `Err` for this session.
    Unreadable,
}

impl PulseMissionState {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Unreadable => "unreadable",
        }
    }
}

/// An open `decision.request` and everything a reader needs to answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionRuling {
    /// Umbrella the request belongs to.
    pub session_key: String,
    /// Event id of the open request.
    pub request_id: String,
    /// Exactly `founder`, or the lowercase 64-hex actor holding it.
    pub held_on: String,
    /// Pubkey that signed the request.
    pub asked_by: String,
    /// The request's own `created_at`, Unix seconds — **display only**.
    ///
    /// Never used for discovery, dedupe or ordering (I4). `None` when the
    /// request event itself was not supplied, in which case no age is rendered
    /// at all rather than `0m`.
    pub asked_at: Option<i64>,
    /// The question, verbatim, when it could be read.
    pub question: Option<String>,
}

/// The newest included verdict — a `disposition` or a `refutation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionVerdict {
    /// Event id of the verdict.
    pub event_id: String,
    /// Pubkey that signed it.
    pub author: String,
    /// The record's own token, verbatim.
    pub token: String,
}

/// A `mission.completed` the fold refused, and the code it refused it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionExcludedCompletion {
    /// Event id of the refused completion.
    pub event_id: String,
    /// The exclusion code's own wire token.
    pub code: String,
}

/// What the policy fold selected, reduced to the three fields Pulse renders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PulseMissionPolicy {
    /// Author of the selected record.
    pub author: Option<String>,
    /// `true` when the selected record sets nothing — the explicit withdrawal.
    pub withdrawn: bool,
    /// The posture word, when set.
    pub posture: Option<String>,
    /// `budget.turns`, when set.
    pub budget_turns: Option<u32>,
    /// `irreversible`, when set.
    pub irreversible: Vec<String>,
}

/// One gate row after provenance is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionGate {
    /// The gate's name.
    pub gate: String,
    /// What it said.
    pub outcome: CodingSessionObservationGateOutcome,
    /// The command that said it, verbatim.
    pub command: String,
    /// Where the row came from.
    pub source: PulseGateSource,
    /// Whether an observed row displaced a declared row for this same gate.
    pub over_declared: bool,
    /// Event id of the observation the row was read from.
    pub event_id: String,
}

/// The newest checkpoint one seat's hook published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionCheckpoint {
    /// Loop phase.
    pub phase: CodingSessionObservationPhase,
    /// Tests written.
    pub tests_written: u32,
    /// Tests seen red.
    pub tests_red: u32,
    /// Tests now green.
    pub tests_green: u32,
    /// The observation's own `created_at`, Unix seconds — display only.
    pub at: Option<i64>,
}

/// An assignment with no settling report yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionOwed {
    /// Event id of the assignment.
    pub assignment_id: String,
    /// The assignment's own `created_at`, Unix seconds — display only.
    pub assigned_at: Option<i64>,
}

/// One seat's newest wip ref, as the **relay** signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionWip {
    /// Full ref name under `refs/heads/wip/`.
    pub ref_name: String,
    /// Commit the ref stands at.
    pub sha: String,
    /// The 30618 event's own `created_at`, Unix seconds — display only.
    pub as_of: Option<i64>,
}

/// Everything Pulse knows about one seat, all of it produced by mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionSeat {
    /// The seat's pubkey.
    pub pubkey: String,
    /// The seat's role, when a verified 44228 transition names one.
    pub role: Option<String>,
    /// Newest checkpoint, when one is on the wire.
    pub checkpoint: Option<PulseMissionCheckpoint>,
    /// Gate rows, ordered `failed`, `not-run`, `passed`, then by name.
    pub gates: Vec<PulseMissionGate>,
    /// Gate rows beyond [`MAX_PULSE_MISSION_GATE_LINES`].
    pub gates_truncated: usize,
    /// Assignments this seat has not reported on.
    pub owed: Vec<PulseMissionOwed>,
    /// The seat's newest wip ref, when the relay has one.
    pub wip: Option<PulseMissionWip>,
}

/// What the relay's ref state says moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionMoved {
    /// `wip` for a `refs/heads/wip/**` ref, `landing` for any other branch.
    pub kind: PulseMovedKind,
    /// The commit the ref stands at.
    pub sha: String,
    /// The full ref name.
    pub ref_name: String,
    /// The pusher, from the 30618 `p` tag.
    pub author_pubkey: String,
    /// The commit subject, when a checkpoint naming this SHA carried one.
    ///
    /// **Shown as the seat's own words and parsed by nothing** (I5).
    pub subject: Option<String>,
    /// Age in seconds of the ref-state event — display only.
    pub age_seconds: Option<i64>,
    /// For a landing: the newest verdict, when one is readable.
    pub verdict: Option<PulseMissionVerdict>,
}

/// Whether a moved ref is a shared wip ref or a landing on an ordinary branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PulseMovedKind {
    /// A ref under `refs/heads/wip/`.
    Wip,
    /// Any other branch.
    Landing,
}

impl PulseMovedKind {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wip => "wip",
            Self::Landing => "landing",
        }
    }
}

/// One phase the author measured itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionPhase {
    /// The phase name, in the author's own words.
    pub phase: String,
    /// The author's own measured duration, milliseconds.
    pub duration_ms: Option<u64>,
}

/// One relay-signed ref, as [`fold_pulse_mission_row`] wants it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseRefState {
    /// Full ref name, e.g. `refs/heads/wip/builder/1f2e3d4c`.
    pub ref_name: String,
    /// Commit the ref stands at.
    pub sha: String,
    /// Pubkey from the 30618 `p` tag — who moved it **last**.
    pub pusher_pubkey: String,
    /// The 30618 event's `created_at`, Unix seconds.
    pub as_of: Option<i64>,
}

/// Everything one mission row is composed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseMissionFacts {
    /// Umbrella key, as the Pulse digest keys sessions.
    pub session_key: String,
    /// Canonical umbrella UUID, when there is one.
    pub session_ref: Option<String>,
    /// Channel the records were read from.
    pub channel_id: String,
    /// The session's name, when one is on the wire.
    pub name: Option<String>,
    /// Newest durable observation time — display only.
    pub latest_observation_at: Option<i64>,
    /// The mission's state word.
    pub state: PulseMissionState,
    /// Why the records could not be read, when `state` is `unreadable`.
    pub unreadable: Option<String>,
    /// The open decision, when the fold names one.
    pub waiting: Option<PulseMissionRuling>,
    /// The canonical terminal's event id, when there is one.
    pub terminal_event_id: Option<String>,
    /// Newest included verdict.
    pub verdict: Option<PulseMissionVerdict>,
    /// A refused completion, when the fold excluded one.
    pub excluded_completion: Option<PulseMissionExcludedCompletion>,
    /// The policy in force, withdrawn, or absent.
    pub policy: PulseMissionPolicy,
    /// One entry per seat with something to show.
    pub seats: Vec<PulseMissionSeat>,
    /// Refs the relay says moved.
    pub moved: Vec<PulseMissionMoved>,
    /// Phase timings, the authors' own measurements.
    pub timing: Vec<PulseMissionPhase>,
    /// Seats a caller claimed that no verified 44228 transition supports.
    pub seat_claims_refused: Vec<String>,
    /// Whether any 30618 ref state at all was supplied for this repo.
    pub ref_state_present: bool,
    /// Whether the observation fold was handed a provider set and so could
    /// check who signed each `observed` and `measured` row.
    ///
    /// `false` is the fold's own `provenance_checked: false`, carried rather
    /// than hidden: every gate row's source word is then the row's own word
    /// for itself, and the rendered line says `unverified` beside it (§8 I9).
    pub gate_provenance_checked: bool,
}
