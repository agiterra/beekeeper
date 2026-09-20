//! `CompletionNotVerified` — the exclusion, and the proof it subtracts nothing
//! when the policy did not ask for it.
//!
//! Every fixture here is one settled assignment: an assignment, the assigned
//! actor's report, an approving disposition and the actor's acknowledgement,
//! plus a `mission.completed` naming that assignment. That is the shape live
//! run 2 ended with (completion `f85635ad`, no verifier, no policy), and it is
//! the shape this rule must leave alone unless `verifier_required` is set.

use super::*;

use crate::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDecisionRequest,
    CodingSessionTeamDispositionDecision, CodingSessionTeamMissionCompleted,
    CodingSessionTeamReport, CODING_SESSION_TEAM_DECISION_FOUNDER,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

fn context(
    founder: &Keys,
    seats: Vec<(&Keys, &str)>,
    verifier_required: bool,
) -> CodingSessionTeamFoldContext {
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
        verifier_required,
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

fn signed(
    payload: &CodingSessionTeamTransactionPayload,
    keys: &Keys,
    created_at: u64,
) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("a payload serialises"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", payload.session_ref.as_str()]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("version tag"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("a test event signs")
}

/// One settled assignment plus its completion, in wire order.
struct SettledMission {
    events: Vec<nostr::Event>,
    assignment_id: String,
    report_id: String,
    completion_id: String,
}

/// Build the mission every test starts from: assignment → report →
/// disposition → acknowledgement → `mission.completed`.
///
/// `reporter` signs the report, and is the actor the assignment names, so the
/// report is canonical by assignee equality whatever seat the reporter holds.
fn settled_mission(founder: &Keys, reporter: &Keys) -> SettledMission {
    let assignment = signed(
        &payload(CodingSessionTeamTransactionBody::Assignment(
            CodingSessionTeamAssignment {
                assignee_actor: reporter.public_key().to_hex(),
                assignee_role: "builder".into(),
                objective: "Build the slice".into(),
                brief: "Implement the bounded assigned slice.".into(),
                branch: None,
                base_sha: None,
                file_ownership: vec!["crates/buzz-core/src".into()],
                acceptance_steps: vec!["cargo test -p buzz-core".into()],
            },
        )),
        founder,
        1,
    );
    let assignment_id = assignment.id.to_hex();
    let report = signed(
        &payload(CodingSessionTeamTransactionBody::Report(
            CodingSessionTeamReport {
                assignment_ref: assignment_id.clone(),
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
        )),
        reporter,
        2,
    );
    let report_id = report.id.to_hex();
    let disposition = signed(
        &payload(CodingSessionTeamTransactionBody::Verdict(
            CodingSessionTeamVerdict::Disposition {
                assignment_ref: assignment_id.clone(),
                report_ref: report_id.clone(),
                refutation_ref: None,
                decision: CodingSessionTeamDispositionDecision::Approve,
                summary: "Governed".into(),
                findings: Vec::new(),
                required_action: None,
            },
        )),
        founder,
        4,
    );
    let acknowledgement = signed(
        &payload(CodingSessionTeamTransactionBody::Acknowledgement(
            CodingSessionTeamAcknowledgement {
                acknowledged_event_ref: disposition.id.to_hex(),
                status: CodingSessionTeamAcknowledgementStatus::Received,
                note: None,
            },
        )),
        reporter,
        5,
    );
    let completion = signed(
        &payload(CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![assignment_id.clone()],
                landed_shas: vec!["1".repeat(40)],
                summary: "Complete".into(),
                follow_ups: Vec::new(),
            },
        )),
        founder,
        6,
    );
    let completion_id = completion.id.to_hex();
    SettledMission {
        events: vec![assignment, report, disposition, acknowledgement, completion],
        assignment_id,
        report_id,
        completion_id,
    }
}

/// A `refutation` of this exact report, with the decision the caller names.
fn refutation(
    assignment_id: &str,
    report_id: &str,
    decision: CodingSessionTeamRefutationDecision,
    author: &Keys,
) -> nostr::Event {
    signed(
        &payload(CodingSessionTeamTransactionBody::Verdict(
            CodingSessionTeamVerdict::Refutation {
                assignment_ref: assignment_id.into(),
                report_ref: report_id.into(),
                decision,
                summary: "Reproduced the lane's gate".into(),
                findings: Vec::new(),
                required_action: None,
            },
        )),
        author,
        3,
    )
}

fn exclusion<'a>(
    fold: &'a CodingSessionTeamFold,
    event_id: &str,
) -> Option<&'a CodingSessionTeamFoldExclusion> {
    fold.excluded.iter().find(|item| item.event_id == event_id)
}

#[test]
fn a_completion_without_a_verifier_is_excluded_when_the_policy_requires_one() {
    let founder = Keys::generate();
    let reporter = Keys::generate();
    let mission = settled_mission(&founder, &reporter);
    let context = context(&founder, vec![(&reporter, "builder")], true);

    let fold = fold_coding_session_team_transactions(&mission.events, &context)
        .expect("the set folds without a hard error");

    let excluded =
        exclusion(&fold, &mission.completion_id).expect("the completion is excluded, not admitted");
    assert_eq!(
        excluded.code,
        CodingSessionTeamFoldExclusionCode::CompletionNotVerified
    );
    assert_eq!(
        excluded.reason,
        format!(
            "mission.completed requires a verifier's ruling while the policy sets \
             gates.verifierRequired: assignment {} settled on report {}, and no active \
             verifier seat has ruled on that report",
            mission.assignment_id, mission.report_id
        )
    );
    // Excluded, never an Err, and never a terminal: the mission has no
    // canonical ending until a verifier rules.
    assert!(fold.canonical_terminal.is_none());
    // The settlement itself is untouched — the approval happened; the
    // verification did not.
    assert!(fold.assignments[0].settled);
}

#[test]
fn a_verifiers_not_refuted_refutation_admits_the_completion() {
    let founder = Keys::generate();
    let reporter = Keys::generate();
    let verifier = Keys::generate();
    let mission = settled_mission(&founder, &reporter);
    let context = context(
        &founder,
        vec![(&reporter, "builder"), (&verifier, "verifier")],
        true,
    );
    let mut events = mission.events.clone();
    events.push(refutation(
        &mission.assignment_id,
        &mission.report_id,
        CodingSessionTeamRefutationDecision::NotRefuted,
        &verifier,
    ));

    let fold = fold_coding_session_team_transactions(&events, &context).expect("the set folds");

    assert_eq!(
        fold.canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(mission.completion_id.as_str())
    );
    assert!(exclusion(&fold, &mission.completion_id).is_none());
}

#[test]
fn a_confirmed_or_blocked_refutation_still_refuses_the_completion() {
    for decision in [
        CodingSessionTeamRefutationDecision::Confirmed,
        CodingSessionTeamRefutationDecision::Blocked,
    ] {
        let founder = Keys::generate();
        let reporter = Keys::generate();
        let verifier = Keys::generate();
        let mission = settled_mission(&founder, &reporter);
        let context = context(
            &founder,
            vec![(&reporter, "builder"), (&verifier, "verifier")],
            true,
        );
        let mut events = mission.events.clone();
        events.push(refutation(
            &mission.assignment_id,
            &mission.report_id,
            decision,
            &verifier,
        ));

        let fold = fold_coding_session_team_transactions(&events, &context).expect("the set folds");

        let excluded = exclusion(&fold, &mission.completion_id)
            .unwrap_or_else(|| panic!("a {decision:?} refutation must not clear the completion"));
        assert_eq!(
            excluded.code,
            CodingSessionTeamFoldExclusionCode::CompletionNotVerified
        );
        assert!(fold.canonical_terminal.is_none());
    }
}

#[test]
fn a_not_refuted_refutation_signed_by_a_builder_refuses_the_completion() {
    let founder = Keys::generate();
    let reporter = Keys::generate();
    let other = Keys::generate();
    let mission = settled_mission(&founder, &reporter);
    // `other` holds a builder seat, so the refutation is unauthorized as well
    // as unverifying; the point of the test is that the completion is refused
    // rather than cleared by a non-verifier's signature.
    let context = context(
        &founder,
        vec![(&reporter, "builder"), (&other, "builder")],
        true,
    );
    let mut events = mission.events.clone();
    events.push(refutation(
        &mission.assignment_id,
        &mission.report_id,
        CodingSessionTeamRefutationDecision::NotRefuted,
        &other,
    ));

    let fold = fold_coding_session_team_transactions(&events, &context).expect("the set folds");

    assert_eq!(
        exclusion(&fold, &mission.completion_id).map(|item| item.code),
        Some(CodingSessionTeamFoldExclusionCode::CompletionNotVerified)
    );
    assert!(fold.canonical_terminal.is_none());
}

#[test]
fn a_report_signed_by_the_verifier_seat_is_itself_the_ruling() {
    // Live run 3's shape: the lead assigned the verification to Ira, whose
    // report *was* the verification. No separate `refutation` was published,
    // and refusing this would have blocked the completion with a verb nobody
    // was using (LANE-L7.md §L7.1, addendum ruling 1).
    let founder = Keys::generate();
    let verifier = Keys::generate();
    let mission = settled_mission(&founder, &verifier);
    let context = context(&founder, vec![(&verifier, "verifier")], true);

    let fold =
        fold_coding_session_team_transactions(&mission.events, &context).expect("the set folds");

    assert_eq!(
        fold.canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(mission.completion_id.as_str())
    );
    assert!(exclusion(&fold, &mission.completion_id).is_none());
}

#[test]
fn without_the_policy_flag_the_fold_is_byte_identical_to_today() {
    // Fixed keys, so the whole projection — every event id, every ordering —
    // is deterministic and can be compared against a checked-in golden.
    let founder = fixed_keys(0x11);
    let reporter = fixed_keys(0x22);
    let mission = settled_mission(&founder, &reporter);

    // `verifier_required: false` is what "the flag is false" and "no policy
    // record exists at all" both produce at the caller, so one fixture proves
    // both: this is live run 2's shape, which had neither.
    let today = context(&founder, vec![(&reporter, "builder")], false);
    let fold =
        fold_coding_session_team_transactions(&mission.events, &today).expect("the set folds");

    assert_eq!(
        fold.canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(mission.completion_id.as_str())
    );
    assert!(fold.excluded.is_empty());

    // **The whole projection, against a golden captured on base `8e8bdbb1a`.**
    //
    // REVIEW-L7 F6: this assertion used to compare the fold with a second fold
    // built from an identically-constructed context — `fold == fold` for a
    // deterministic function, which cannot fail and proved nothing.
    //
    // The golden below was produced by running this exact fixture through a
    // **base `8e8bdbb1a` build** — a detached throwaway worktree, before
    // `verifier_required` or this module existed — and is checked in verbatim
    // (1,436 bytes). It is therefore a real base-vs-lane comparison, not this
    // tree talking to itself. Any future change to the fold's output on a
    // no-policy session now has to edit that file on purpose.
    //
    // Edited on purpose once, by lane 183 (ledger 183): the golden gained
    // `awaiting: None` on the settled assignment and `pending_completion:
    // None` on the fold. Both are the new *absence* — this session settled and
    // finished, so there is no missing link and nothing is waiting — and no
    // pre-existing line of the capture changed.
    let golden = include_str!("../testdata/completion_verification_no_policy_fold.txt");
    assert_eq!(
        format!("{fold:#?}\n"),
        golden,
        "the fold's projection for a session with no policy changed; if that is intended, \
         re-capture crates/buzz-core/testdata/completion_verification_no_policy_fold.txt \
         and say why in the report"
    );
}

/// Deterministic keys, so a golden can exist at all.
///
/// `Keys::generate()` gives a different pubkey each run, which changes every
/// event id and every `(created_at, id)` ordering downstream.
fn fixed_keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("a fixed 32-byte secret key")
}

#[test]
fn the_flag_never_admits_a_completion_todays_rules_refuse() {
    // `CompletionNotApproved` still wins on an unsettled assignment: the new
    // pass only ever subtracts, and never reaches an assignment the fold did
    // not settle.
    let founder = Keys::generate();
    let reporter = Keys::generate();
    let verifier = Keys::generate();
    let mission = settled_mission(&founder, &reporter);
    let context = context(
        &founder,
        vec![(&reporter, "builder"), (&verifier, "verifier")],
        true,
    );
    // Drop the acknowledgement: the assignment is no longer settled.
    let events: Vec<nostr::Event> = mission
        .events
        .iter()
        .filter(|event| {
            !matches!(
                validate_coding_session_team_transaction_envelope(event)
                    .expect("a fixture decodes")
                    .body,
                CodingSessionTeamTransactionBody::Acknowledgement(_)
            )
        })
        .cloned()
        .collect();

    let fold = fold_coding_session_team_transactions(&events, &context).expect("the set folds");

    assert_eq!(
        exclusion(&fold, &mission.completion_id).map(|item| item.code),
        Some(CodingSessionTeamFoldExclusionCode::CompletionNotApproved)
    );
    assert!(fold.canonical_terminal.is_none());
}

/// **Fix round 1: a reader must never lose history.**
///
/// The proof at the *fold* level, not just the decoder's. An answer whose
/// signed body predates `condition` — three keys, exactly what live run 2's
/// `4847ff06…` carries — must still be canonical, must still answer its
/// request, and must therefore still let the completion that depends on it be
/// this mission's terminal.
///
/// The seven-key exact row this lane shipped first failed all three at once:
/// the answer was undecodable, so the request read as unanswered, so the
/// completion was excluded `CompletionBlockedByOpenDecision` and run 2's
/// `mission.completed f85635ad` vanished from the fold.
#[test]
fn an_answer_written_before_condition_existed_still_settles_its_mission() {
    let founder = Keys::generate();
    let reporter = Keys::generate();
    let mission = settled_mission(&founder, &reporter);
    let context = context(&founder, vec![(&reporter, "builder")], false);

    // A request that blocks the completion's assignment, and an answer written
    // in the pre-`condition` shape: the body's JSON simply has no such key.
    let request = signed(
        &payload(CodingSessionTeamTransactionBody::DecisionRequest(
            CodingSessionTeamDecisionRequest {
                question: "Push with --no-verify, or hold?".into(),
                options: vec!["push".into(), "hold".into()],
                held_on: CODING_SESSION_TEAM_DECISION_FOUNDER.into(),
                blocks: vec![mission.assignment_id.clone()],
                recommendation: None,
            },
        )),
        &reporter,
        2,
    );
    let answer = signed_body_json(
        &founder,
        3,
        "decision.answer",
        serde_json::json!({
            "requestRef": request.id.to_hex(),
            "choice": 0,
            "note": "C. Role always present, null outside a session."
        }),
    );
    let answer_id = answer.id.to_hex();

    let mut events = mission.events.clone();
    events.push(request);
    events.push(answer);

    let fold = fold_coding_session_team_transactions(&events, &context)
        .expect("the set folds without a hard error");

    // 1. The old-shape answer is canonical.
    assert!(fold.included_event_ids.contains(&answer_id));
    assert!(exclusion(&fold, &answer_id).is_none());
    // 2. It answers its request, so nothing is waiting.
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(
        fold.decisions[0].answer_event_id.as_deref(),
        Some(answer_id.as_str())
    );
    assert!(fold.waiting_on_decision.is_none());
    // 3. And the completion it unblocks is this mission's terminal — the exact
    //    fact the exact-key row erased from live run 2.
    assert_eq!(
        fold.canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(mission.completion_id.as_str())
    );
    assert!(fold.excluded.is_empty());
}

/// Sign a 44244 whose `body` is supplied as raw JSON.
///
/// The typed builders every other fixture uses cannot express a body that
/// omits a field of the struct, which is exactly the shape under test here.
fn signed_body_json(
    keys: &Keys,
    created_at: u64,
    transaction_type: &str,
    body: serde_json::Value,
) -> nostr::Event {
    let content = serde_json::json!({
        "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "type": transaction_type,
        "supersedes": serde_json::Value::Null,
        "deliveryCommandId": serde_json::Value::Null,
        "body": body,
    })
    .to_string();
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        content,
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("version tag"),
        Tag::parse(["cstx-genesis", genesis().as_str()]).expect("genesis tag"),
        Tag::parse(["cstx-type", transaction_type]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("a test event signs")
}
