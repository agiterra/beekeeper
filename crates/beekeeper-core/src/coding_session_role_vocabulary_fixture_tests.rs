//! The `captain` / `qa` fixture vector (headless pluggable round 2, Lane E).
//!
//! One project names its lead `captain` and its judging role `qa`. Every
//! authority rule that used to compare against the literals `"lead"` and
//! `"verifier"` must pass for that project exactly as it passes for the
//! default — and, the mirror, a seat holding the *foundation's* word must
//! hold nothing in that project, or the manifest would be decoration.
//!
//! Each test runs the same records under both vocabularies, so the default
//! fixture is proven unchanged in the same breath.

use super::*;

use crate::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamMissionCompleted, CodingSessionTeamRefutationDecision,
    CodingSessionTeamReport, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use crate::role_vocabulary::RoleVocabulary;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

fn captain_qa() -> RoleVocabulary {
    RoleVocabulary::from_team(Some("captain"), Some(&["qa".to_owned()])).expect("a vocabulary")
}

fn context(
    founder: &Keys,
    seats: Vec<(&Keys, &str)>,
    vocabulary: RoleVocabulary,
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
        vocabulary,
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

fn signed(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("a payload serialises"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("version tag"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("a test event signs")
}

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: actor.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Build the slice".into(),
            brief: "Implement the bounded assigned slice.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/beekeeper-core/src".into()],
            acceptance_steps: vec!["cargo test -p beekeeper-core".into()],
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

fn refutation(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Refutation {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            decision: CodingSessionTeamRefutationDecision::NotRefuted,
            summary: "Could not break it".into(),
            findings: Vec::new(),
            required_action: None,
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

fn acknowledgement(disposition_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Acknowledgement(
        CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: disposition_ref.into(),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        },
    ))
}

fn completed(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: vec!["1".repeat(40)],
            summary: "Complete".into(),
            follow_ups: Vec::new(),
        },
    ))
}

fn code_of(
    fold: &CodingSessionTeamFold,
    event_id: &str,
) -> Option<CodingSessionTeamFoldExclusionCode> {
    fold.excluded
        .iter()
        .find(|item| item.event_id == event_id)
        .map(|item| item.code)
}

/// `may_lead`: the seat holding the project's lead role assigns, disposes and
/// completes; a seat holding the foundation's word does not. Mirrored under
/// the default vocabulary with the roles swapped.
#[test]
fn authority_follows_the_projects_lead_role() {
    let founder = Keys::generate();
    let captain = Keys::generate();
    let foundation_lead = Keys::generate();
    let builder = Keys::generate();
    let seats = |vocabulary: RoleVocabulary| {
        context(
            &founder,
            vec![(&captain, "captain"), (&foundation_lead, "lead")],
            vocabulary,
            false,
        )
    };

    let by_captain = signed(&assignment(&builder), &captain, 1);
    let by_foundation_lead = signed(&assignment(&builder), &foundation_lead, 2);
    let records = [by_captain.clone(), by_foundation_lead.clone()];

    let under_captain =
        fold_coding_session_team_transactions(&records, &seats(captain_qa())).expect("folds");
    assert_eq!(code_of(&under_captain, &by_captain.id.to_hex()), None);
    assert_eq!(
        code_of(&under_captain, &by_foundation_lead.id.to_hex()),
        Some(CodingSessionTeamFoldExclusionCode::Unauthorized),
        "the foundation's `lead` holds no standing in a captain project"
    );
    assert!(seats(captain_qa()).may_lead(&captain.public_key().to_hex()));
    assert!(!seats(captain_qa()).may_lead(&foundation_lead.public_key().to_hex()));

    let under_default =
        fold_coding_session_team_transactions(&records, &seats(RoleVocabulary::default()))
            .expect("folds");
    assert_eq!(
        code_of(&under_default, &by_captain.id.to_hex()),
        Some(CodingSessionTeamFoldExclusionCode::Unauthorized),
        "under the default vocabulary a captain seat is nobody"
    );
    assert_eq!(
        code_of(&under_default, &by_foundation_lead.id.to_hex()),
        None
    );
}

/// Refutation signing: the project's first verification role signs
/// refutations; the foundation's `verifier` does not in a `qa` project.
#[test]
fn refutations_are_signed_by_the_projects_judging_role() {
    let founder = Keys::generate();
    let qa = Keys::generate();
    let verifier = Keys::generate();
    let builder = Keys::generate();
    let seats = |vocabulary: RoleVocabulary| {
        context(
            &founder,
            vec![(&qa, "qa"), (&verifier, "verifier")],
            vocabulary,
            false,
        )
    };
    let assignment = signed(&assignment(&builder), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex()), &builder, 2);
    let by_qa = signed(
        &refutation(&assignment.id.to_hex(), &report.id.to_hex()),
        &qa,
        3,
    );
    let by_verifier = signed(
        &refutation(&assignment.id.to_hex(), &report.id.to_hex()),
        &verifier,
        4,
    );
    let records = [assignment, report, by_qa.clone(), by_verifier.clone()];

    let under_qa =
        fold_coding_session_team_transactions(&records, &seats(captain_qa())).expect("folds");
    assert_eq!(code_of(&under_qa, &by_qa.id.to_hex()), None);
    assert_eq!(
        code_of(&under_qa, &by_verifier.id.to_hex()),
        Some(CodingSessionTeamFoldExclusionCode::Unauthorized)
    );

    let under_default =
        fold_coding_session_team_transactions(&records, &seats(RoleVocabulary::default()))
            .expect("folds");
    assert_eq!(
        code_of(&under_default, &by_qa.id.to_hex()),
        Some(CodingSessionTeamFoldExclusionCode::Unauthorized)
    );
    assert_eq!(code_of(&under_default, &by_verifier.id.to_hex()), None);
}

/// Completion verification under `gates.verifierRequired`: a `qa` seat's
/// `not-refuted` clears the completion in a `qa` project and a `verifier`
/// seat's does not; the settlement names the project's lead role as the
/// party owing a disposition.
#[test]
fn completion_verification_and_settlement_follow_the_vocabulary() {
    let founder = Keys::generate();
    let qa = Keys::generate();
    let verifier = Keys::generate();
    let builder = Keys::generate();
    let seats = |vocabulary: RoleVocabulary| {
        context(
            &founder,
            vec![(&qa, "qa"), (&verifier, "verifier")],
            vocabulary,
            true,
        )
    };
    let assignment = signed(&assignment(&builder), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex()), &builder, 2);
    let disposition = signed(
        &disposition(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
        3,
    );
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &builder, 4);
    let completion = signed(&completed(&assignment.id.to_hex()), &founder, 6);
    let cleared_by = |judge: &Keys| {
        signed(
            &refutation(&assignment.id.to_hex(), &report.id.to_hex()),
            judge,
            5,
        )
    };

    for (vocabulary, judge, other) in [
        (captain_qa(), &qa, &verifier),
        (RoleVocabulary::default(), &verifier, &qa),
    ] {
        let with_judge = [
            assignment.clone(),
            report.clone(),
            disposition.clone(),
            acknowledgement.clone(),
            cleared_by(judge),
            completion.clone(),
        ];
        let fold = fold_coding_session_team_transactions(&with_judge, &seats(vocabulary.clone()))
            .expect("folds");
        assert_eq!(
            fold.canonical_terminal.as_ref().map(|t| t.event_id.clone()),
            Some(completion.id.to_hex()),
            "{}'s not-refuted clears the completion under its own vocabulary",
            vocabulary.verdict_role()
        );

        let with_other = [
            assignment.clone(),
            report.clone(),
            disposition.clone(),
            acknowledgement.clone(),
            cleared_by(other),
            completion.clone(),
        ];
        let fold = fold_coding_session_team_transactions(&with_other, &seats(vocabulary.clone()))
            .expect("folds");
        assert_eq!(fold.canonical_terminal, None);
        assert_eq!(
            code_of(&fold, &completion.id.to_hex()),
            Some(CodingSessionTeamFoldExclusionCode::CompletionNotVerified),
            "a seat holding another project's word clears nothing"
        );
    }

    // Settlement: with a report and no disposition, the party owed is the
    // project's own lead role.
    let awaiting = [assignment.clone(), report.clone()];
    let fold =
        fold_coding_session_team_transactions(&awaiting, &seats(captain_qa())).expect("folds");
    assert_eq!(
        fold.assignments[0]
            .awaiting
            .as_ref()
            .map(|a| a.owed_by_role.as_str()),
        Some("captain")
    );
    let fold = fold_coding_session_team_transactions(&awaiting, &seats(RoleVocabulary::default()))
        .expect("folds");
    assert_eq!(
        fold.assignments[0]
            .awaiting
            .as_ref()
            .map(|a| a.owed_by_role.as_str()),
        Some("lead")
    );
}
