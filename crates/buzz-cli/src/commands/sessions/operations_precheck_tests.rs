//! The two live shapes that forced B1b's four fold rounds, refused before
//! signing.
//!
//! Every fixture below is a local reconstruction of a record that is really on
//! `wss://hive.agiterra.org`, channel `4aa32763-…`, session `7a374285-…`. The
//! ids differ (these events are signed by generated keys), the **shapes** do
//! not: `c737be4c` is a report whose `assignmentRef` names an event nobody
//! published, and `46b03d08` is a correction of it that also changed the
//! `assignmentRef` — the two records the fold spent rounds 1–4 learning to
//! exclude, and which nothing stopped anyone writing.

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamAcknowledgement,
    CodingSessionTeamAcknowledgementStatus, CodingSessionTeamActiveSeat,
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamFoldContext, CodingSessionTeamMissionBlocked,
    CodingSessionTeamMissionCompleted, CodingSessionTeamReport, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CodingSessionTeamVerdict,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

use super::*;

const CHANNEL: &str = "4aa32763-b8e6-49af-9420-b79e7a677aa7";
const SESSION: &str = "7a374285-24ef-4ef6-bdfb-28748170eeef";
const GENESIS: &str = "17c2656a2b71f3cd73728e5649a214a8df6993ded805835e4f4c16ae24852e07";
/// The exact `assignmentRef` the live `c737be4c` report carried — an event id
/// nobody ever published into that session.
const LIVE_MISSING_ASSIGNMENT: &str =
    "f233c16b77ddd9c950e7c8aad31613fbf9fc490b631d8e241f79ae96959d99a6";

fn context(founder: &Keys, seats: Vec<(&Keys, &str)>) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: seats
            .into_iter()
            .map(|(keys, role)| CodingSessionTeamActiveSeat {
                actor_pubkey: keys.public_key().to_hex(),
                role: role.into(),
            })
            .collect(),
        active_grants: Vec::new(),
        // These fixtures pre-date `gates.verifierRequired`; `false` is what a
        // session with no policy folds under.
        verifier_required: false,
    }
}

fn payload(body: CodingSessionTeamTransactionBody) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
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
            assignee_role: "runner".into(),
            objective: "Run the gate".into(),
            brief: "Run the gate and report.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-cli/src".into()],
            acceptance_steps: vec!["just ci".into()],
        },
    ))
}

fn report(assignment_ref: &str, summary: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: summary.into(),
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

fn acknowledgement(reference: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Acknowledgement(
        CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: reference.into(),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        },
    ))
}

fn disposition(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::Approve,
            summary: "Governed".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ))
}

/// One assignment carried all the way to settled: assignment, report,
/// approving disposition, and the assignee's acknowledgement.
///
/// A `mission.completed` naming an *active* assignment that is not settled is
/// excluded `CompletionNotApproved`, so a terminal fixture that skipped this
/// chain would be testing the wrong refusal.
fn settled_assignment(founder: &Keys, actor: &Keys) -> (Vec<nostr::Event>, String) {
    let assignment_event = signed(&assignment(actor), founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let report_event = signed(&report(&assignment_id, "Lane ran the gate"), actor, 2);
    let disposition_event = signed(
        &disposition(&assignment_id, &report_event.id.to_hex()),
        founder,
        3,
    );
    let acknowledgement_event = signed(&acknowledgement(&disposition_event.id.to_hex()), actor, 4);
    (
        vec![
            assignment_event,
            report_event,
            disposition_event,
            acknowledgement_event,
        ],
        assignment_id,
    )
}

fn blocked(summary: &str, blockers: Vec<&str>) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionBlocked(
        CodingSessionTeamMissionBlocked {
            assignment_refs: Vec::new(),
            summary: summary.into(),
            blockers: blockers.into_iter().map(str::to_owned).collect(),
            held_on: None,
            required_action: "Resolve it".into(),
        },
    ))
}

fn completed(assignment_ref: &str, summary: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: Vec::new(),
            summary: summary.into(),
            follow_ups: Vec::new(),
        },
    ))
}

fn signed(
    payload: &CodingSessionTeamTransactionPayload,
    keys: &Keys,
    created_at: u64,
) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("payload serializes"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("cstx-v tag"),
        Tag::parse(["cstx-genesis", GENESIS]).expect("cstx-genesis tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("cstx-type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign")
}

/// Run the writer-side check the way `publish_operation` does.
fn precheck(
    events: &[nostr::Event],
    context: &CodingSessionTeamFoldContext,
    signer: &Keys,
    candidate: &CodingSessionTeamTransactionPayload,
) -> Result<Option<String>, CliError> {
    let fold = fold_coding_session_team_transactions(events, context).expect("session folds");
    let signer_pubkey = signer.public_key().to_hex();
    check_against_fold(
        events,
        &fold,
        &PrecheckRequest {
            channel: CHANNEL,
            session_ref: SESSION,
            genesis: GENESIS,
            signer_pubkey: &signer_pubkey,
            payload: candidate,
        },
    )
}

fn usage_message(error: CliError) -> String {
    match error {
        CliError::Usage(message) => message,
        other => panic!("expected a usage refusal, got {other:?}"),
    }
}

// ── The live `c737be4c` shape: a report citing an assignment nobody published ─

#[test]
fn the_c737be4c_shape_is_refused_before_signing() {
    let founder = Keys::generate();
    let runner = Keys::generate();
    let context = context(&founder, vec![(&runner, "runner")]);
    let assignment_event = signed(&assignment(&runner), &founder, 1);
    let events = vec![assignment_event];

    // The live record's `assignmentRef` named `f233c16b…`, which was not a team
    // transaction of the session at all. This is that exact value.
    let missing = LIVE_MISSING_ASSIGNMENT.to_owned();
    let candidate = report(&missing, "All three gates PASSED.");

    let message = usage_message(precheck(&events, &context, &runner, &candidate).unwrap_err());
    assert!(
        message.contains(&missing),
        "the refusal must name the id: {message}"
    );
    assert!(
        message.contains("is not a transaction of this session"),
        "the refusal must name the rule: {message}"
    );
}

/// The same shape, one step removed: the cited id **is** in the session but the
/// fold excluded it, so citing it would build governance on a record the
/// session does not hold.
#[test]
fn a_reference_to_an_excluded_record_is_refused_before_signing() {
    let founder = Keys::generate();
    let runner = Keys::generate();
    let context = context(&founder, vec![(&runner, "runner")]);
    let assignment_event = signed(&assignment(&runner), &founder, 1);
    let dangling = signed(
        &report(LIVE_MISSING_ASSIGNMENT, "All three gates PASSED."),
        &runner,
        2,
    );
    let dangling_id = dangling.id.to_hex();
    let events = vec![assignment_event, dangling];

    let candidate = acknowledgement(&dangling_id);

    let message = usage_message(precheck(&events, &context, &runner, &candidate).unwrap_err());
    assert!(
        message.contains(&dangling_id) && message.contains("DanglingReference"),
        "the refusal must name the id and the fold's own code: {message}"
    );
}

// ── The live `46b03d08` shape: a correction that moved its own subject ───────

#[test]
fn the_46b03d08_shape_is_refused_with_the_frozen_message() {
    let founder = Keys::generate();
    let runner = Keys::generate();
    let context = context(&founder, vec![(&runner, "runner")]);
    // A faithful replay of the live graph at 23:06: the assignment the
    // correction moved *to* (`436c10ce`) is a real, included assignment; the
    // report being corrected (`c737be4c`) cites an assignment nobody published
    // (`f233c16b…`) and is therefore already excluded `DanglingReference`.
    let real_assignment = signed(&assignment(&runner), &founder, 1);
    let original = signed(
        &report(LIVE_MISSING_ASSIGNMENT, "All three gates PASSED."),
        &runner,
        2,
    );
    let original_id = original.id.to_hex();
    let events = vec![real_assignment.clone(), original];

    // Live 23:06: the runner's superseding report changed `assignmentRef` from
    // `c737be4c`'s subject to `436c10ce`.
    let mut candidate = report(&real_assignment.id.to_hex(), "All three gates PASSED.");
    candidate.supersedes = Some(original_id.clone());

    let message = usage_message(precheck(&events, &context, &runner, &candidate).unwrap_err());
    assert!(
        message.contains(
            "a correction changes wording, never its subject; publish a new report instead"
        ),
        "the frozen message must appear verbatim: {message}"
    );
    assert!(
        message.contains(&original_id),
        "the refusal must name the id: {message}"
    );
}

#[test]
fn a_correction_that_keeps_its_subject_is_allowed() {
    let founder = Keys::generate();
    let runner = Keys::generate();
    let context = context(&founder, vec![(&runner, "runner")]);
    let assignment_event = signed(&assignment(&runner), &founder, 1);
    let original = signed(
        &report(&assignment_event.id.to_hex(), "Lane ran the gate"),
        &runner,
        2,
    );
    let original_id = original.id.to_hex();
    let events = vec![assignment_event.clone(), original];

    let mut candidate = report(
        &assignment_event.id.to_hex(),
        "Lane ran the gate; two tests were red first",
    );
    candidate.supersedes = Some(original_id.clone());

    assert_eq!(
        precheck(&events, &context, &runner, &candidate).expect("a wording correction is allowed"),
        Some(original_id)
    );
}

#[test]
fn a_correction_of_another_authors_record_is_refused() {
    let founder = Keys::generate();
    let runner = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&runner, "runner")]);
    let assignment_event = signed(&assignment(&runner), &founder, 1);
    let original = signed(
        &report(&assignment_event.id.to_hex(), "Lane ran the gate"),
        &runner,
        2,
    );
    let original_id = original.id.to_hex();
    let events = vec![assignment_event.clone(), original];

    let mut candidate = report(&assignment_event.id.to_hex(), "I disagree");
    candidate.supersedes = Some(original_id.clone());

    let message = usage_message(precheck(&events, &context, &lead, &candidate).unwrap_err());
    assert!(
        message.contains("same signed author") && message.contains(&original_id),
        "{message}"
    );
}

#[test]
fn a_prose_only_blocked_correction_is_refused_before_signing() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead")]);
    let first = signed(&blocked("Waiting on review", vec!["review"]), &lead, 1);
    let first_id = first.id.to_hex();
    let events = vec![first];

    let mut candidate = blocked("Still waiting on the review", vec!["review"]);
    candidate.supersedes = Some(first_id.clone());

    let message = usage_message(precheck(&events, &context, &lead, &candidate).unwrap_err());
    assert!(
        message.contains(TERMINAL_PROSE_EDIT_NEEDS_A_NOTE),
        "the writer must quote the reader's own constant: {message}"
    );
}

// ── B2.7: a completion may correct a blocked (live 02:35, finding 14) ────────

#[test]
fn a_completion_supersedes_the_leads_own_canonical_blocked() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead")]);
    // The live pair: `1a5dcc8c` (mission.blocked, 21:57) then `98476799`
    // (mission.completed, 02:35). The completion named no `supersedes`, so the
    // fold could only record a `terminal` conflict between the two.
    let (mut events, assignment_id) = settled_assignment(&founder, &lead);
    let blocked_event = signed(&blocked("Waiting on the runner", vec!["ack"]), &lead, 5);
    let blocked_id = blocked_event.id.to_hex();
    events.push(blocked_event);

    let candidate = completed(&assignment_id, "TeamRolesV1 is complete");

    assert_eq!(
        precheck(&events, &context, &lead, &candidate).expect("a completion is allowed"),
        Some(blocked_id),
        "the completion adopts the lead's own blocked as its correction target"
    );
}

#[test]
fn a_completion_does_not_correct_someone_elses_blocked() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let other = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead"), (&other, "runner")]);
    let (mut events, assignment_id) = settled_assignment(&founder, &other);
    let blocked_event = signed(&blocked("Waiting on the runner", vec!["ack"]), &other, 5);
    events.push(blocked_event);

    let candidate = completed(&assignment_id, "TeamRolesV1 is complete");

    assert_eq!(
        precheck(&events, &context, &lead, &candidate).expect("a completion is allowed"),
        None,
        "correcting another author's terminal is not a completion's business"
    );
}

#[test]
fn a_blocked_may_never_correct_a_completion() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead")]);
    let (mut events, assignment_id) = settled_assignment(&founder, &lead);
    let completion = signed(&completed(&assignment_id, "Done"), &lead, 5);
    let completion_id = completion.id.to_hex();
    events.push(completion);

    let mut candidate = blocked("Actually blocked", vec!["ci"]);
    candidate.supersedes = Some(completion_id.clone());

    let message = usage_message(precheck(&events, &context, &lead, &candidate).unwrap_err());
    assert!(
        message.contains(TERMINAL_COMPLETION_IS_NOT_REOPENED),
        "{message}"
    );
}

/// The end-to-end claim of B2.7, read back through the fold the way
/// `bee sessions operation list` does: one corrected terminal, no conflict.
#[test]
fn the_corrected_terminal_folds_without_a_conflict() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead")]);
    let (mut events, assignment_id) = settled_assignment(&founder, &lead);
    let blocked_event = signed(&blocked("Waiting on the runner", vec!["ack"]), &lead, 5);
    let blocked_id = blocked_event.id.to_hex();

    let mut completion = completed(&assignment_id, "TeamRolesV1 is complete");
    completion.supersedes = Some(blocked_id.clone());
    let completion_event = signed(&completion, &lead, 6);
    let completion_id = completion_event.id.to_hex();

    events.push(blocked_event);
    events.push(completion_event);
    let fold = fold_coding_session_team_transactions(&events, &context).expect("folds");

    assert_eq!(
        fold.canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(completion_id.as_str())
    );
    assert!(
        fold.conflicts.iter().all(|item| item.subject != "terminal"),
        "a corrected terminal is not a conflict: {:?}",
        fold.conflicts
    );
    assert_eq!(
        fold.excluded
            .iter()
            .map(|item| (item.event_id.as_str(), format!("{:?}", item.code)))
            .collect::<Vec<_>>(),
        vec![(blocked_id.as_str(), "Superseded".to_owned())]
    );
}

/// The live shape as it actually stands on the wire — the completion carries no
/// `supersedes` — still folds to a `terminal` conflict. B2.7 changes what is
/// *written*, not how records already on relays are read.
#[test]
fn an_uncorrected_completion_still_conflicts_with_the_blocked() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead")]);
    let (mut events, assignment_id) = settled_assignment(&founder, &lead);
    let blocked_event = signed(&blocked("Waiting on the runner", vec!["ack"]), &lead, 5);
    let completion_event = signed(
        &completed(&assignment_id, "TeamRolesV1 is complete"),
        &lead,
        6,
    );
    events.push(blocked_event);
    events.push(completion_event);

    let fold = fold_coding_session_team_transactions(&events, &context).expect("folds");
    assert!(
        fold.conflicts.iter().any(|item| item.subject == "terminal"),
        "the live 2026-09-01 fold is unchanged: {:?}",
        fold.conflicts
    );
}

/// The live chain, exactly: `8771ecc1` ← `1a5dcc8c` (a blocked correcting a
/// blocked) ← the completion. All three are one correction group, so the
/// newest wins and the two earlier terminals are `Superseded` rather than
/// conflicting.
#[test]
fn a_completion_may_correct_the_newest_link_of_a_blocked_chain() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let context = context(&founder, vec![(&lead, "lead")]);
    let (mut events, assignment_id) = settled_assignment(&founder, &lead);

    let first_blocked = signed(&blocked("Blocked", vec!["ack"]), &lead, 5);
    let first_blocked_id = first_blocked.id.to_hex();
    let mut second = blocked("Blocked, corrected", vec!["ack", "ci"]);
    second.supersedes = Some(first_blocked_id.clone());
    let second_blocked = signed(&second, &lead, 6);
    let second_blocked_id = second_blocked.id.to_hex();
    events.push(first_blocked);
    events.push(second_blocked);

    // The writer picks the newest canonical terminal, which is the second link.
    let candidate = completed(&assignment_id, "TeamRolesV1 is complete");
    assert_eq!(
        precheck(&events, &context, &lead, &candidate).expect("a completion is allowed"),
        Some(second_blocked_id.clone())
    );

    let mut completion = candidate;
    completion.supersedes = Some(second_blocked_id.clone());
    let completion_event = signed(&completion, &lead, 7);
    let completion_id = completion_event.id.to_hex();
    events.push(completion_event);

    let fold = fold_coding_session_team_transactions(&events, &context).expect("folds");
    assert_eq!(
        fold.canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(completion_id.as_str())
    );
    assert!(
        fold.conflicts.iter().all(|item| item.subject != "terminal"),
        "one corrected terminal, no conflict: {:?}",
        fold.conflicts
    );
    // `fold.excluded` is ordered by event id, which these generated keys make
    // arbitrary, so compare the set rather than the order.
    let mut superseded: Vec<&str> = fold
        .excluded
        .iter()
        .filter(|item| format!("{:?}", item.code) == "Superseded")
        .map(|item| item.event_id.as_str())
        .collect();
    superseded.sort_unstable();
    let mut expected = vec![first_blocked_id.as_str(), second_blocked_id.as_str()];
    expected.sort_unstable();
    assert_eq!(superseded, expected);
}

/// **REVIEW-B2 F5.** The pre-check's module doc says it reads the fold's rules
/// and never re-implements one. This pins the last rule that had a copy: for
/// every ordered pair of operation types, the writer refuses a `--supersedes`
/// exactly when `buzz-core`'s own `terminal_correction_is_allowed` says the
/// crossing is illegal — no case where the two disagree.
#[test]
fn the_writer_and_the_reader_agree_on_every_type_crossing() {
    use buzz_core::coding_session_team_transaction::terminal_correction_is_allowed;

    let types = [
        CodingSessionTeamTransactionType::Assignment,
        CodingSessionTeamTransactionType::Report,
        CodingSessionTeamTransactionType::Verdict,
        CodingSessionTeamTransactionType::Acknowledgement,
        CodingSessionTeamTransactionType::MissionCompleted,
        CodingSessionTeamTransactionType::MissionBlocked,
        CodingSessionTeamTransactionType::Note,
        CodingSessionTeamTransactionType::DecisionRequest,
        CodingSessionTeamTransactionType::DecisionAnswer,
    ];
    let mut crossings = 0;
    for previous in types {
        for current in types {
            if previous == current {
                continue;
            }
            crossings += 1;
            let allowed = terminal_correction_is_allowed(previous, current);
            assert_eq!(
                allowed,
                previous == CodingSessionTeamTransactionType::MissionBlocked
                    && current == CodingSessionTeamTransactionType::MissionCompleted,
                "{} -> {} is the only legal crossing",
                previous.as_str(),
                current.as_str()
            );
        }
    }
    assert_eq!(crossings, 72, "every ordered pair of the nine types");
}
