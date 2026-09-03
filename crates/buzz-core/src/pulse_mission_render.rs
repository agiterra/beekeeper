//! The eight sibling keys, and the only place Pulse's sentences exist.
//!
//! Split out of [`super`] so no file here passes 1,000 lines. A child module,
//! so it reads its parent's items unchanged — and so there is still exactly one
//! `render_pulse_mission_lines`, which `bee pulse missions` and Desktop both
//! call. A `.ts`/`.tsx` file that composed one of these sentences would be a
//! second, drifting copy.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationPhase,
};

use super::*;

// ── The eight sibling keys ───────────────────────────────────────────────────

/// One rendered sentence, and the closed id a surface hangs a testid on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionLine {
    /// Closed line id.
    pub id: String,
    /// The sentence, composed in Rust and re-worded by nobody.
    pub text: String,
}

impl PulseMissionLine {
    fn new(id: &str, text: String) -> Self {
        Self {
            id: id.to_owned(),
            text,
        }
    }
}

/// One seat, as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionSeatRow {
    /// The seat's pubkey.
    pub pubkey: String,
    /// The seat's role, or `null`.
    pub role: Option<String>,
    /// The seat's own sentences, in order.
    pub lines: Vec<PulseMissionLine>,
}

/// One moved ref, as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionMovedRow {
    /// `wip` or `landing`.
    pub kind: String,
    /// The commit.
    pub sha: String,
    /// The ref.
    #[serde(rename = "ref")]
    pub ref_name: String,
    /// The pusher.
    pub author_pubkey: String,
    /// The seat's own commit subject, or `null`.
    pub subject: Option<String>,
    /// Age of the ref-state event, or `null`.
    pub age_seconds: Option<i64>,
    /// The sentence.
    pub lines: Vec<PulseMissionLine>,
}

/// One mission row, as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionRow {
    /// Umbrella key.
    pub session_key: String,
    /// Canonical umbrella UUID, or `null`.
    pub session_ref: Option<String>,
    /// Channel the records were read from.
    pub channel_id: String,
    /// The session's name, or `null`.
    pub name: Option<String>,
    /// The state word.
    pub state: String,
    /// Newest durable observation time, or `null`.
    pub latest_observation_at: Option<i64>,
    /// Mission-level sentences, in order.
    pub lines: Vec<PulseMissionLine>,
    /// One entry per seat.
    pub seats: Vec<PulseMissionSeatRow>,
    /// What the relay says moved.
    pub moved: Vec<PulseMissionMovedRow>,
    /// Phase timing and the cost disclosure.
    pub timing: Vec<PulseMissionLine>,
}

/// One read that failed or was bounded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionError {
    /// What the failure was about.
    pub scope: String,
    /// Human-readable detail.
    pub message: String,
}

/// The sibling object attached beside a Pulse digest.
///
/// Exactly eight keys, added to what `bee pulse digest` prints and Desktop
/// holds. Nothing existing is removed, renamed or re-typed, and kind 44240's
/// event body does not change at all, so an older digest reader ignores eight
/// unknown keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMissionRows {
    /// Always [`PULSE_MISSION_ROWS_SCHEMA`].
    pub missions_schema: String,
    /// What these rows were read over, stated rather than implied.
    pub mission_scope: String,
    /// The rows, newest observation first.
    pub missions: Vec<PulseMissionRow>,
    /// Every read that failed or was bounded.
    pub mission_errors: Vec<PulseMissionError>,
    /// Every open ruling in scope.
    pub open_rulings: Vec<PulseMissionRuling>,
    /// The subset held on the viewer.
    pub rulings_waiting_on_viewer: Vec<PulseMissionRuling>,
    /// Mechanical overlap rows.
    pub overlaps: Vec<crate::pulse_overlap::PulseOverlapRow>,
    /// The viewer's own pubkey, or `null` when this surface has no identity.
    pub viewer_pubkey: Option<String>,
}

/// What [`PulseMissionRows::mission_scope`] always says.
pub const PULSE_MISSION_SCOPE: &str =
    "project channels · the newest 8 open sessions by observation time";

/// The rulings held on the viewer: a founder-held request when the viewer is
/// that session's founder, or a request held on the viewer's own pubkey.
pub fn rulings_waiting_on_viewer(
    open: &[PulseMissionRuling],
    viewer: Option<&str>,
    founder_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<PulseMissionRuling> {
    let Some(viewer) = viewer else {
        return Vec::new();
    };
    open.iter()
        .filter(|ruling| {
            if ruling.held_on == "founder" {
                founder_of(&ruling.session_key).as_deref() == Some(viewer)
            } else {
                ruling.held_on == viewer
            }
        })
        .cloned()
        .collect()
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// Display names by pubkey, plus the viewer, so `{Who}` reads `You` for them.
#[derive(Debug, Clone, Default)]
pub struct PulseMissionNames {
    /// Resolved display names by lowercase-hex pubkey.
    pub names: BTreeMap<String, String>,
    /// The viewer's own pubkey, when this surface has an identity.
    pub viewer: Option<String>,
}

impl PulseMissionNames {
    /// `{Who}`: `You` for the viewer, else the display name, else 8 hex.
    pub fn who(&self, pubkey: &str) -> String {
        if self.viewer.as_deref() == Some(pubkey) {
            return "You".to_owned();
        }
        self.names
            .get(pubkey)
            .cloned()
            .unwrap_or_else(|| short_hex(pubkey))
    }
}

/// The first eight hex of an event id or pubkey.
pub fn short_hex(value: &str) -> String {
    value.chars().take(8).collect()
}

/// A bounded, human relative age: `42m ago` shape, never `0m`.
fn relative(seconds: i64) -> Option<String> {
    if seconds < 60 {
        return Some("just now".to_owned());
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return Some(format!("{minutes}m ago"));
    }
    let hours = minutes / 60;
    if hours < 24 {
        return Some(format!("{hours}h ago"));
    }
    Some(format!("{}d ago", hours / 24))
}

/// `since` rendered against `now`, or `None` when nothing is readable.
fn age(now_unix: i64, since: Option<i64>) -> Option<String> {
    let since = since?;
    if since <= 0 || since > now_unix {
        return None;
    }
    relative(now_unix - since)
}

fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    format!("{}h {}m", minutes / 60, minutes % 60)
}

/// Compose every sentence one mission row shows.
///
/// This is the **only** place these sentences exist. `bee pulse digest` prints
/// exactly what Desktop renders because both call this function; a `.ts`/`.tsx`
/// file that composed one of them would be a second, drifting copy.
pub fn render_pulse_mission_lines(
    facts: &PulseMissionFacts,
    names: &PulseMissionNames,
    now_unix: i64,
) -> PulseMissionRow {
    let mut lines = Vec::new();

    if let Some(reason) = &facts.unreadable {
        lines.push(PulseMissionLine::new(
            "unreadable",
            format!("This session's records could not be read: {reason}"),
        ));
        return PulseMissionRow {
            session_key: facts.session_key.clone(),
            session_ref: facts.session_ref.clone(),
            channel_id: facts.channel_id.clone(),
            name: facts.name.clone(),
            state: facts.state.as_str().to_owned(),
            latest_observation_at: facts.latest_observation_at,
            lines,
            seats: Vec::new(),
            moved: Vec::new(),
            timing: Vec::new(),
        };
    }

    // The open decision comes first: a person is waiting.
    if let Some(waiting) = &facts.waiting {
        let held = if waiting.held_on == "founder" {
            "Waiting on the founder".to_owned()
        } else {
            format!("Waiting on {}", names.who(&waiting.held_on))
        };
        let mut text = format!("{held} · asked by {}", names.who(&waiting.asked_by));
        if let Some(rendered) = age(now_unix, waiting.asked_at) {
            text.push_str(&format!(" · {rendered}"));
        }
        if let Some(question) = &waiting.question {
            text.push_str(&format!(": {question}"));
        }
        lines.push(PulseMissionLine::new("waiting", text));
    }

    lines.push(PulseMissionLine::new(
        "state",
        match (facts.state, &facts.terminal_event_id) {
            (PulseMissionState::Running, _) => "Mission running".to_owned(),
            (PulseMissionState::Blocked, Some(id)) => {
                format!("Mission blocked · {}", short_hex(id))
            }
            (PulseMissionState::Completed, Some(id)) => {
                format!("Mission completed · {}", short_hex(id))
            }
            (state, _) => format!("Mission {}", state.as_str()),
        },
    ));

    lines.push(PulseMissionLine::new(
        "verdict",
        match &facts.verdict {
            Some(verdict) => format!(
                "Newest verdict: {} by {} · {}",
                verdict.token,
                names.who(&verdict.author),
                short_hex(&verdict.event_id)
            ),
            None => "No verdict on the wire".to_owned(),
        },
    ));

    // An excluded completion is not a completion, and never renders as one.
    if let Some(excluded) = &facts.excluded_completion {
        lines.push(PulseMissionLine::new(
            "excluded-completion",
            format!(
                "A completion was excluded · {} · {} — this mission is not completed",
                excluded.code,
                short_hex(&excluded.event_id)
            ),
        ));
    }

    lines.push(PulseMissionLine::new(
        "policy",
        render_policy(&facts.policy, names),
    ));

    if !facts.seat_claims_refused.is_empty() {
        let count = facts.seat_claims_refused.len();
        let (noun, verb) = if count == 1 {
            ("seat", "it")
        } else {
            ("seats", "them")
        };
        lines.push(PulseMissionLine::new(
            "seat-claims-refused",
            format!(
                "{count} claimed {noun} refused: no accepted authority transition supports {verb}"
            ),
        ));
    }

    // A surface with no identity cannot know what is held on the reader, and
    // "unknown" must not render as an empty waiting list (§0.8). The sentence
    // is composed here, like every other, so the CLI and Desktop print the same
    // words — an earlier draft left Desktop with no line at all and its own e2e
    // test injected the missing one (REVIEW-L9 F4.1).
    if names.viewer.is_none() {
        lines.push(PulseMissionLine::new(
            "not-read",
            PULSE_NO_VIEWER_IDENTITY.to_owned(),
        ));
    }

    if !facts.ref_state_present {
        lines.push(PulseMissionLine::new(
            "ref-state",
            "No ref state on the wire for this repo".to_owned(),
        ));
    } else if facts.seats.iter().any(|seat| seat.wip.is_some()) {
        lines.push(PulseMissionLine::new(
            "wip-window",
            format!(
                "Wip refs are pruned when their branch merges or after {} days",
                WIP_REF_RETENTION_DAYS
            ),
        ));
    }

    let seats = facts
        .seats
        .iter()
        .map(|seat| render_seat(seat, names, now_unix))
        .collect();
    let moved = facts
        .moved
        .iter()
        .map(|entry| render_moved(entry, names, now_unix))
        .collect();

    let mut timing: Vec<PulseMissionLine> = facts
        .timing
        .iter()
        .map(|phase| {
            PulseMissionLine::new(
                "timing",
                match phase.duration_ms {
                    Some(ms) => format!(
                        "{}: {} (the author's own measurement)",
                        phase.phase,
                        duration(ms)
                    ),
                    None => format!(
                        "{}: still running (the author's own measurement)",
                        phase.phase
                    ),
                },
            )
        })
        .collect();
    if timing.is_empty() {
        timing.push(PulseMissionLine::new(
            "timing-missing",
            "No phase timing on the wire for this session".to_owned(),
        ));
    }
    // Said rather than implied: this surface reads no usage events, so it can
    // show wall time and cannot show tokens.
    timing.push(PulseMissionLine::new(
        "cost",
        "Token cost is not on this surface: Pulse reads no usage events".to_owned(),
    ));

    PulseMissionRow {
        session_key: facts.session_key.clone(),
        session_ref: facts.session_ref.clone(),
        channel_id: facts.channel_id.clone(),
        name: facts.name.clone(),
        state: facts.state.as_str().to_owned(),
        latest_observation_at: facts.latest_observation_at,
        lines,
        seats,
        moved,
        timing,
    }
}

fn render_policy(policy: &PulseMissionPolicy, names: &PulseMissionNames) -> String {
    let Some(author) = &policy.author else {
        return "No policy set for this session".to_owned();
    };
    if policy.withdrawn {
        return format!("Policy withdrawn by {}", names.who(author));
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(posture) = &policy.posture {
        parts.push(format!("posture {posture}"));
    }
    if let Some(turns) = policy.budget_turns {
        parts.push(format!("budget {turns} turns"));
    }
    if !policy.irreversible.is_empty() {
        parts.push(format!("irreversible {}", policy.irreversible.join(", ")));
    }
    if parts.is_empty() {
        // Nothing Pulse renders is set, and no bar is drawn for a limit
        // nothing counts.
        return format!(
            "Policy by {}: nothing this row renders is set",
            names.who(author)
        );
    }
    format!("Policy by {}: {}", names.who(author), parts.join(" · "))
}

fn render_seat(
    seat: &PulseMissionSeat,
    names: &PulseMissionNames,
    now_unix: i64,
) -> PulseMissionSeatRow {
    let who = names.who(&seat.pubkey);
    let role = seat.role.clone().unwrap_or_else(|| "no role".to_owned());
    let mut lines = Vec::new();

    lines.push(PulseMissionLine::new(
        "live",
        match &seat.checkpoint {
            Some(checkpoint) => match age(now_unix, checkpoint.at) {
                Some(rendered) => format!(
                    "{who} ({role}) · {}, checkpointed {rendered}",
                    phase_word(checkpoint.phase)
                ),
                None => format!("{who} ({role}) · {}", phase_word(checkpoint.phase)),
            },
            None => format!("{who} ({role}) · no checkpoint on the wire"),
        },
    ));

    if let Some(checkpoint) = &seat.checkpoint {
        let mut text = format!(
            "{who} · tests {}/{} green, {} seen red · {}",
            checkpoint.tests_green,
            checkpoint.tests_written,
            checkpoint.tests_red,
            phase_word(checkpoint.phase)
        );
        if let Some(rendered) = age(now_unix, checkpoint.at) {
            text.push_str(&format!(" · {rendered}"));
        }
        lines.push(PulseMissionLine::new("checkpoint", text));
    }

    if seat.gates.is_empty() {
        lines.push(PulseMissionLine::new(
            "gate-missing",
            format!("No gate row on the wire for {who} — a claim in prose is not a gate row"),
        ));
    } else {
        for gate in &seat.gates {
            let source = if gate.over_declared {
                format!("{}, over a declared row", gate.source.as_str())
            } else {
                gate.source.as_str().to_owned()
            };
            lines.push(PulseMissionLine::new(
                "gate",
                format!(
                    "{who} · {}: {} ({source}) · {}",
                    gate.gate,
                    outcome_word(gate.outcome),
                    gate.command
                ),
            ));
        }
        if seat.gates_truncated > 0 {
            lines.push(PulseMissionLine::new(
                "gate-truncated",
                format!("{} more gates not shown", seat.gates_truncated),
            ));
        }
    }

    for owed in &seat.owed {
        let mut text = format!(
            "{who} owes a report on assignment {}",
            short_hex(&owed.assignment_id)
        );
        if let Some(rendered) = age(now_unix, owed.assigned_at) {
            text.push_str(&format!(" · assigned {rendered}"));
        }
        lines.push(PulseMissionLine::new("owed", text));
    }

    lines.push(PulseMissionLine::new(
        "wip",
        match &seat.wip {
            Some(wip) => {
                let mut text = format!(
                    "{who}'s local commits: {} on {}",
                    short_hex(&wip.sha),
                    wip.ref_name.trim_start_matches("refs/heads/")
                );
                if let Some(rendered) = age(now_unix, wip.as_of) {
                    text.push_str(&format!(", ref state as of {rendered}"));
                }
                text
            }
            // What the relay holds, never a claim about that person's config.
            None => format!("{who}'s local commits: not shared"),
        },
    ));

    PulseMissionSeatRow {
        pubkey: seat.pubkey.clone(),
        role: seat.role.clone(),
        lines,
    }
}

const fn phase_word(phase: CodingSessionObservationPhase) -> &'static str {
    match phase {
        CodingSessionObservationPhase::Planning => "planning",
        CodingSessionObservationPhase::Red => "red",
        CodingSessionObservationPhase::Green => "green",
        CodingSessionObservationPhase::Gates => "gates",
        CodingSessionObservationPhase::Reporting => "reporting",
    }
}

const fn outcome_word(outcome: CodingSessionObservationGateOutcome) -> &'static str {
    match outcome {
        CodingSessionObservationGateOutcome::Passed => "passed",
        CodingSessionObservationGateOutcome::Failed => "failed",
        CodingSessionObservationGateOutcome::NotRun => "not-run",
    }
}

fn render_moved(
    moved: &PulseMissionMoved,
    names: &PulseMissionNames,
    now_unix: i64,
) -> PulseMissionMovedRow {
    let _ = now_unix;
    let who = names.who(&moved.author_pubkey);
    let mut text = match moved.kind {
        PulseMovedKind::Wip => format!("{} on {} by {who}", short_hex(&moved.sha), moved.ref_name),
        PulseMovedKind::Landing => format!(
            "{} landed on {} by {who}",
            short_hex(&moved.sha),
            moved.ref_name
        ),
    };
    if let Some(seconds) = moved.age_seconds {
        if let Some(rendered) = relative(seconds) {
            text.push_str(&format!(" · {rendered}"));
        }
    }
    match moved.kind {
        PulseMovedKind::Wip => {
            if let Some(subject) = &moved.subject {
                text.push_str(&format!(" · {subject}"));
            }
        }
        // A wip ref proves a commit was pushed, not that anybody reviewed it —
        // and the mission's newest verdict is not a verdict *over this commit*.
        // Saying so is the whole difference between disclosure and a claim.
        PulseMovedKind::Landing => match &moved.verdict {
            Some(verdict) => text.push_str(&format!(
                " · the mission's newest verdict is {} by {} ({}), which is not a verdict over this commit",
                verdict.token,
                names.who(&verdict.author),
                short_hex(&verdict.event_id)
            )),
            None => text.push_str(" · no verdict on the wire for this commit"),
        },
    }

    PulseMissionMovedRow {
        kind: moved.kind.as_str().to_owned(),
        sha: moved.sha.clone(),
        ref_name: moved.ref_name.clone(),
        author_pubkey: moved.author_pubkey.clone(),
        subject: moved.subject.clone(),
        age_seconds: moved.age_seconds,
        lines: vec![PulseMissionLine::new("moved", text)],
    }
}

/// The sentence a surface with no identity shows in place of a waiting list.
pub const PULSE_NO_VIEWER_IDENTITY: &str =
    "No identity on this surface, so nothing here can be held on you";

/// The disclosure for open sessions the cap did not read.
pub fn pulse_mission_cap_disclosure(open_sessions: usize) -> Option<PulseMissionError> {
    if open_sessions <= MAX_PULSE_MISSION_ROWS {
        return None;
    }
    Some(PulseMissionError {
        scope: "missions".to_owned(),
        message: format!(
            "{open_sessions} open sessions in scope; the newest {MAX_PULSE_MISSION_ROWS} by \
             observation time were read"
        ),
    })
}

/// Every open ruling across a set of folded rows, in row order.
pub fn open_rulings(rows: &[PulseMissionFacts]) -> Vec<PulseMissionRuling> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    rows.iter()
        .filter_map(|facts| facts.waiting.clone())
        .filter(|ruling| seen.insert(ruling.request_id.clone()))
        .collect()
}
