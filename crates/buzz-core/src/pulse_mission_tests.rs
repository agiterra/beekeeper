//! What a mission row says, and what it refuses to say.
//!
//! Every fixture here is a **signed** event, because every rule under test
//! reads a signature: the 44244 fold verifies before it attributes, the
//! observation fold attributes gates by signer, and the wip lines are keyed on
//! the pubkey the relay recorded as the pusher.

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use super::*;
use crate::coding_session_observation::CODING_SESSION_OBSERVATION_SCHEMA;
use crate::coding_session_team_transaction::{
    CodingSessionTeamActiveSeat, CodingSessionTeamAssignment, CodingSessionTeamDecisionRequest,
    CodingSessionTeamDispositionDecision, CodingSessionTeamMissionCompleted,
    CodingSessionTeamReport, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::{KIND_CODING_SESSION_OBSERVATION, KIND_CODING_SESSION_TEAM_TRANSACTION};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const OTHER_SESSION: &str = "6c8f2d3b-a1e5-4c1f-b2a4-8d3e9f7b5c21";

fn id(byte: &str) -> String {
    byte.repeat(32)
}

fn genesis() -> String {
    id("ab")
}

fn context(founder: &Keys, seats: Vec<(&Keys, &str)>) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: seats
            .into_iter()
            .map(|(keys, role)| CodingSessionTeamActiveSeat {
                actor_pubkey: keys.public_key().to_hex(),
                role: role.into(),
            })
            .collect(),
        active_grants: Vec::new(),
        // This fixture reads no policy set, which is what the field requires of
        // such a caller: the fold then behaves as it did before it existed.
        verifier_required: false,
    }
}

fn payload(body: CodingSessionTeamTransactionBody) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        transaction_type: body.transaction_type(),
        supersedes: None,
        delivery_command_id: None,
        body,
    }
}

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: actor.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Build the protocol".into(),
            brief: "Implement the bounded assigned slice.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-core/src".into()],
            acceptance_steps: vec!["cargo test -p buzz-core".into()],
        },
    ))
}

fn report(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: "Done".into(),
            branch: None,
            base_sha: None,
            head_sha: None,
            files: Vec::new(),
            tests: Vec::new(),
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        },
    ))
}

fn disposition(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref: None,
            decision,
            summary: "Governed".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ))
}

fn decision_request(held_on: &str, question: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::DecisionRequest(
        CodingSessionTeamDecisionRequest {
            question: question.into(),
            options: Vec::new(),
            held_on: held_on.into(),
            blocks: Vec::new(),
            recommendation: None,
        },
    ))
}

fn completed(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: Vec::new(),
            summary: "Landed".into(),
            follow_ups: Vec::new(),
        },
    ))
}

fn signed(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", payload.session_ref.as_str()]).expect("d"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign")
}

fn observation(
    keys: &Keys,
    observation_type: &str,
    assignment_ref: Option<&str>,
    body: Value,
    created_at: u64,
) -> Event {
    let content = json!({
        "schema": CODING_SESSION_OBSERVATION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "type": observation_type,
        "assignmentRef": assignment_ref.map_or(Value::Null, |value| json!(value)),
        "body": body,
    })
    .to_string();
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        content,
    )
    .tags(vec![
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", SESSION]).expect("d"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("v"),
        Tag::parse(["csob-genesis", &genesis()]).expect("genesis"),
        Tag::parse(["csob-type", observation_type]).expect("type"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign")
}

fn gate_body(gate: &str, outcome: &str, command: &str) -> Value {
    json!({"rows": [{
        "gate": gate,
        "outcome": outcome,
        "command": command,
        "summary": Value::Null,
        "durationMs": Value::Null,
    }]})
}

fn checkpoint_body(phase: &str, written: u32, red: u32, green: u32) -> Value {
    json!({
        "phase": phase,
        "testsWritten": written,
        "testsRed": red,
        "testsGreen": green,
        "lastCommand": Value::Null,
        "lastSummary": Value::Null,
        "note": Value::Null,
    })
}

fn sources<'a>(
    context: &'a CodingSessionTeamFoldContext,
    team: &'a [Event],
    observations: &'a [Event],
    ref_state: &'a [PulseRefState],
) -> PulseMissionSources<'a> {
    PulseMissionSources {
        session_key: SESSION,
        channel_id: CHANNEL,
        name: Some("Route rail honesty"),
        latest_observation_at: Some(1_000),
        context,
        policy_grants: &[],
        team_events: team,
        policy_events: &[],
        observation_events: observations,
        ref_state,
        claimed_seats: &[],
        gate_source: None,
    }
}

fn names(entries: Vec<(&str, &str)>, viewer: Option<&str>) -> PulseMissionNames {
    PulseMissionNames {
        names: entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        viewer: viewer.map(str::to_owned),
    }
}

fn line<'a>(row: &'a PulseMissionRow, id: &str) -> Option<&'a PulseMissionLine> {
    row.lines.iter().find(|line| line.id == id)
}

fn seat_lines<'a>(row: &'a PulseMissionRow, pubkey: &str) -> Vec<&'a PulseMissionLine> {
    row.seats
        .iter()
        .filter(|seat| seat.pubkey == pubkey)
        .flat_map(|seat| seat.lines.iter())
        .collect()
}

// ── L9.7 the mission row ─────────────────────────────────────────────────────

#[test]
fn an_open_request_puts_the_waiting_line_before_the_state_word() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let request = signed(
        &decision_request("founder", "Do we widen the push gate for seats?"),
        &actor,
        2,
    );
    let events = vec![assignment, request];
    let facts = fold_pulse_mission_row(&sources(&context, &events, &[], &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);

    assert_eq!(row.lines[0].id, "waiting", "the waiting line comes first");
    assert!(
        row.lines[0]
            .text
            .starts_with("Waiting on the founder · asked by Bob · "),
        "waiting line was {:?}",
        row.lines[0].text
    );
    assert!(
        row.lines[0]
            .text
            .ends_with(": Do we widen the push gate for seats?"),
        "the question is carried verbatim: {:?}",
        row.lines[0].text
    );
    assert_eq!(row.lines[1].id, "state");
}

#[test]
fn an_unreadable_timestamp_renders_no_age_rather_than_zero_minutes() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let request = signed(&decision_request("founder", "Ship it?"), &actor, 9_000);
    let events = vec![request];
    // `now` is *older* than the request, so no age is readable at all.
    let facts = fold_pulse_mission_row(&sources(&context, &events, &[], &[]), 100);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 100);
    let waiting = line(&row, "waiting").expect("waiting line");
    assert!(
        !waiting.text.contains("0m"),
        "never `0m`: {:?}",
        waiting.text
    );
    assert!(waiting.text.ends_with(": Ship it?"), "{:?}", waiting.text);
}

#[test]
fn an_excluded_completion_renders_its_code_and_never_reads_completed() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex()), &actor, 2);
    // No disposition and no acknowledgement, so the completion cannot prove the
    // approval chain and the fold excludes it.
    let completion = signed(&completed(&assignment.id.to_hex()), &founder, 3);
    let events = vec![assignment, report, completion];
    let facts = fold_pulse_mission_row(&sources(&context, &events, &[], &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);

    let excluded = line(&row, "excluded-completion").expect("excluded line");
    assert!(
        excluded.text.contains("completionNotApproved"),
        "the code is named: {:?}",
        excluded.text
    );
    assert_eq!(
        line(&row, "state").expect("state").text,
        "Mission running",
        "an excluded completion never reads as completed"
    );
    assert_eq!(row.state, "running");
}

#[test]
fn the_newest_verdict_carries_its_own_token_verbatim() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex()), &actor, 2);
    let ruling = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::ApproveWithNotes,
        ),
        &founder,
        3,
    );
    let events = vec![assignment, report, ruling];
    let facts = fold_pulse_mission_row(&sources(&context, &events, &[], &[]), 10_000);
    let names = names(vec![(&founder.public_key().to_hex(), "Brian")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let verdict = line(&row, "verdict").expect("verdict");
    assert!(
        verdict
            .text
            .starts_with("Newest verdict: approve-with-notes by Brian · "),
        "{:?}",
        verdict.text
    );
}

#[test]
fn no_verdict_reads_as_no_verdict_rather_than_as_silence() {
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert_eq!(
        line(&row, "verdict").expect("verdict").text,
        "No verdict on the wire"
    );
}

// ── L9.6 bounds and one row's failure ────────────────────────────────────────

#[test]
fn a_failed_fold_is_one_rows_failure_and_names_the_reason() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    // The same event supplied twice is the fold's own hard error.
    let events = vec![assignment.clone(), assignment];
    let facts = fold_pulse_mission_row(&sources(&context, &events, &[], &[]), 10_000);
    assert_eq!(facts.state, PulseMissionState::Unreadable);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert_eq!(row.lines.len(), 1, "an unreadable row says one thing");
    assert!(
        row.lines[0]
            .text
            .starts_with("This session's records could not be read: duplicate supplied"),
        "{:?}",
        row.lines[0].text
    );
    assert!(row.seats.is_empty(), "nothing is claimed about seats");
}

#[test]
fn nine_open_sessions_disclose_the_cap_by_name() {
    assert!(pulse_mission_cap_disclosure(8).is_none());
    let disclosure = pulse_mission_cap_disclosure(9).expect("a ninth session is disclosed");
    assert_eq!(disclosure.scope, "missions");
    assert_eq!(
        disclosure.message,
        "9 open sessions in scope; the newest 8 by observation time were read"
    );
}

#[test]
fn no_open_session_discloses_nothing_at_all() {
    assert!(pulse_mission_cap_disclosure(0).is_none());
}

// ── L9.8 rulings waiting on you ──────────────────────────────────────────────

fn ruling(session: &str, held_on: &str, asker: &str) -> PulseMissionRuling {
    PulseMissionRuling {
        session_key: session.to_owned(),
        request_id: format!("{}{}", &held_on[..2.min(held_on.len())], id("cd")),
        held_on: held_on.to_owned(),
        asked_by: asker.to_owned(),
        asked_at: None,
        question: None,
    }
}

#[test]
fn a_founder_counts_two_and_the_named_seat_counts_one() {
    let founder = id("11");
    let seat = id("22");
    let open = vec![
        ruling(SESSION, "founder", &seat),
        ruling(OTHER_SESSION, &seat, &founder),
    ];
    let founder_of = |_: &str| Some(founder.clone());

    let for_founder = rulings_waiting_on_viewer(&open, Some(&founder), &founder_of);
    assert_eq!(for_founder.len(), 1, "the founder-held one");
    let for_seat = rulings_waiting_on_viewer(&open, Some(&seat), &founder_of);
    assert_eq!(for_seat.len(), 1, "the seat-held one");
    assert_eq!(for_seat[0].held_on, seat);
    assert_eq!(
        for_founder.len() + for_seat.len(),
        open.len(),
        "every open ruling is held on exactly one of them"
    );
}

#[test]
fn a_founder_held_request_elsewhere_counts_zero_and_still_lists() {
    let other_founder = id("33");
    let viewer = id("44");
    let open = vec![ruling(SESSION, "founder", &viewer)];
    let founder_of = |_: &str| Some(other_founder.clone());
    let waiting = rulings_waiting_on_viewer(&open, Some(&viewer), &founder_of);
    assert!(waiting.is_empty(), "not held on this viewer");
    assert_eq!(open.len(), 1, "and it is still listed as open");
}

#[test]
fn no_identity_means_an_empty_list_and_a_sentence_rather_than_zero() {
    let open = vec![ruling(SESSION, "founder", &id("55"))];
    let founder_of = |_: &str| Some(id("55"));
    assert!(rulings_waiting_on_viewer(&open, None, &founder_of).is_empty());
    assert_eq!(
        PULSE_NO_VIEWER_IDENTITY,
        "No identity on this surface, so nothing here can be held on you"
    );
}

// ── L9.9 gate truth per seat ─────────────────────────────────────────────────

#[test]
fn an_observed_row_beats_a_declared_row_and_says_which_source_it_shows() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_ref = assignment.id.to_hex();
    let declared = observation(
        &actor,
        "gate",
        Some(&assignment_ref),
        gate_body("cargo test", "passed", "cargo test -p buzz-core"),
        2,
    );
    let observed = observation(
        &provider,
        "gate",
        Some(&assignment_ref),
        gate_body("cargo test", "failed", "cargo test -p buzz-core"),
        3,
    );
    let team = vec![assignment];
    let observations = vec![declared, observed.clone()];

    // The `source` key is Lane L5's to land; until it does, every row on the
    // wire reads `declared`. The precedence rule is exercised through the same
    // adapter the wire will feed, so the day L5 lands it nothing here changes.
    let observed_id = observed.id.to_hex();
    let lookup = move |event: &Event, _gate: &str| -> Option<String> {
        (event.id.to_hex() == observed_id).then(|| "observed".to_owned())
    };
    let mut sources = sources(&context, &team, &observations, &[]);
    sources.gate_source = Some(&lookup);
    let facts = fold_pulse_mission_row(&sources, 10_000);
    let names = names(
        vec![
            (&actor.public_key().to_hex(), "Bob"),
            (&provider.public_key().to_hex(), "provider"),
        ],
        None,
    );
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let gate_lines: Vec<&str> = row
        .seats
        .iter()
        .flat_map(|seat| seat.lines.iter())
        .filter(|line| line.id == "gate")
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(
        gate_lines.len(),
        1,
        "the observed row replaces the declared one for the same (author, gate): {gate_lines:?}"
    );
    assert_eq!(
        gate_lines[0],
        "Bob · cargo test: failed (observed, over a declared row) · cargo test -p buzz-core",
        "an observed row wins, says so, and carries the command verbatim"
    );
}

#[test]
fn the_source_adapter_reads_the_wire_token_and_defaults_to_declared() {
    assert_eq!(
        PulseGateSource::from_wire_token(Some("observed")),
        PulseGateSource::Observed
    );
    assert_eq!(
        PulseGateSource::from_wire_token(Some("declared")),
        PulseGateSource::Declared
    );
    // Absent — every row on today's wire, since L5 has not landed the key.
    assert_eq!(
        PulseGateSource::from_wire_token(None),
        PulseGateSource::Declared
    );
    // An unknown token never buys the stronger claim.
    assert_eq!(
        PulseGateSource::from_wire_token(Some("machine")),
        PulseGateSource::Declared
    );
    assert!(PulseGateSource::Declared < PulseGateSource::Observed);
}

#[test]
fn a_not_run_gate_never_renders_as_a_pass_and_sorts_above_one() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let passed = observation(
        &actor,
        "gate",
        None,
        gate_body("clippy", "passed", "cargo clippy"),
        2,
    );
    let not_run = observation(
        &actor,
        "gate",
        None,
        gate_body("e2e", "not-run", "pnpm test:e2e"),
        3,
    );
    let observations = vec![passed, not_run];
    let facts = fold_pulse_mission_row(&sources(&context, &[], &observations, &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let gates: Vec<&str> = seat_lines(&row, &actor.public_key().to_hex())
        .into_iter()
        .filter(|line| line.id == "gate")
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(gates.len(), 2, "gates are never collapsed to one");
    assert!(gates[0].contains("e2e: not-run"), "{gates:?}");
    assert!(gates[1].contains("clippy: passed"), "{gates:?}");
    assert!(
        !gates.iter().any(|text| text.contains("not-run (passed")),
        "{gates:?}"
    );
}

#[test]
fn a_seat_with_no_gate_row_says_so_rather_than_reading_green() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    // The seat's prose "all green" lives in a turn nothing here reads. What the
    // wire holds is no gate row at all.
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let lines = seat_lines(&row, &actor.public_key().to_hex());
    let missing = lines
        .iter()
        .find(|line| line.id == "gate-missing")
        .expect("the missing-gate sentence");
    assert_eq!(
        missing.text,
        "No gate row on the wire for Bob — a claim in prose is not a gate row"
    );
    assert!(
        !lines.iter().any(|line| line.text.contains("no gate row\"")),
        "absence is stated once, never as a fake gate row"
    );
}

#[test]
fn more_than_four_gates_are_bounded_with_a_visible_count() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let observations: Vec<Event> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .enumerate()
        .map(|(index, gate)| {
            observation(
                &actor,
                "gate",
                None,
                gate_body(gate, "passed", "just ci"),
                2 + index as u64,
            )
        })
        .collect();
    let facts = fold_pulse_mission_row(&sources(&context, &[], &observations, &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let lines = seat_lines(&row, &actor.public_key().to_hex());
    assert_eq!(
        lines.iter().filter(|line| line.id == "gate").count(),
        MAX_PULSE_MISSION_GATE_LINES
    );
    assert_eq!(
        lines
            .iter()
            .find(|line| line.id == "gate-truncated")
            .expect("truncation is visible")
            .text,
        "2 more gates not shown"
    );
}

#[test]
fn the_wire_carries_no_exit_code_so_no_row_renders_one() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let observations = vec![observation(
        &actor,
        "gate",
        None,
        gate_body("cargo test", "failed", "cargo test -p buzz-core"),
        2,
    )];
    let facts = fold_pulse_mission_row(&sources(&context, &[], &observations, &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    for line in row.seats.iter().flat_map(|seat| seat.lines.iter()) {
        assert!(
            !line.text.contains("exit "),
            "no exit code is invented: {:?}",
            line.text
        );
    }
}

#[path = "pulse_mission_wire_tests.rs"]
mod wire_tests;

#[path = "pulse_mission_fixture_tests.rs"]
mod fixture_tests;

/// L22 / finding 31: Pulse reads a gate row signed before `headSha` existed,
/// and one signed after, and renders both.
///
/// Pulse's strict gate for kind 44246 is `buzz-core`'s own decoder — it holds
/// no second schema of its own (`pulse_mission.rs`'s `pulse_gate_source_token`
/// reads the raw content for one key and nothing else). So the read-optional
/// exemption is what keeps every row already on the wire renderable, and this
/// test is the claim rather than the coincidence: `gate_body` above writes the
/// five-key shape on purpose.
#[test]
fn a_gate_row_from_before_head_sha_and_one_after_both_reach_pulse() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_ref = assignment.id.to_hex();
    // The shape every 44246 on the wire carried before 2026-09-03.
    let old = observation(
        &actor,
        "gate",
        Some(&assignment_ref),
        gate_body("cargo fmt", "passed", "cargo fmt --check"),
        2,
    );
    // The shape this repository signs now.
    let mut new_body = gate_body("cargo clippy", "passed", "cargo clippy");
    new_body["rows"][0]["headSha"] = json!("07c470be007c470be007c470be007c470be007c4");
    new_body["rows"][0]["dirty"] = json!(false);
    let new = observation(&actor, "gate", Some(&assignment_ref), new_body, 3);
    let team = vec![assignment];
    let observations = vec![old, new];

    let facts = fold_pulse_mission_row(&sources(&context, &team, &observations, &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let gate_lines: Vec<&str> = row
        .seats
        .iter()
        .flat_map(|seat| seat.lines.iter())
        .filter(|line| line.id == "gate")
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(
        gate_lines.len(),
        2,
        "a widening that dropped the older row would be a reader losing history: {gate_lines:?}"
    );
    assert!(
        gate_lines.iter().any(|line| line.contains("cargo fmt")),
        "{gate_lines:?}"
    );
    assert!(
        gate_lines.iter().any(|line| line.contains("cargo clippy")),
        "{gate_lines:?}"
    );
}
