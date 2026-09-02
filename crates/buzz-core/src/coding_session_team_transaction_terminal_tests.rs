//! The terminal-correction rule (B2.7, live finding 14).
//!
//! A child of `coding_session_team_transaction_tests`, split out only to keep
//! every file under 1,000 lines. `use super::*` gives it that module's fixture
//! helpers unchanged.

use super::*;

/// **Live run TeamRolesV1, finding 14 (2026-09-01 02:35).** The lead published
/// `mission.blocked` `1a5dcc8c` at 21:57, worked for four and a half hours, and
/// published `mission.completed` `98476799` at 02:35. Nothing in the vocabulary
/// could retract the first, so the fold recorded a `terminal` **conflict** and
/// a person reading the Mission rail saw Blocked in red over a mission that was
/// working, then Completed wearing a conflict badge.
///
/// A completion may correct a blocked. The reverse never: a mission that has
/// completed is not reopened by a later record claiming it never did.
#[test]
fn a_completion_may_correct_a_blocked_and_never_the_reverse() {
    let lead = Keys::generate();
    let blocked = |supersedes: Option<String>| CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::MissionBlocked,
        supersedes,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::MissionBlocked(CodingSessionTeamMissionBlocked {
            assignment_refs: Vec::new(),
            summary: "Waiting on the runner's acknowledgement".into(),
            blockers: vec!["the runner has not acknowledged".into()],
            held_on: None,
            required_action: "Nudge the runner".into(),
        }),
    };
    let completed = |supersedes: Option<String>| CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::MissionCompleted,
        supersedes,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![id("cd")],
                landed_shas: Vec::new(),
                summary: "TeamRolesV1 is complete".into(),
                follow_ups: Vec::new(),
            },
        ),
    };

    // 1a5dcc8c → 98476799, the live pair, with the correction the shape needed.
    let blocked_event = event_with_keys(&blocked(None), &lead);
    let completion = event_with_keys(&completed(Some(blocked_event.id.to_hex())), &lead);
    assert!(
        validate_coding_session_team_transaction_supersession(&completion, &blocked_event).is_ok(),
        "a completion may correct the blocked it supersedes"
    );

    // The reverse is refused by name.
    let completed_first = event_with_keys(&completed(None), &lead);
    let reopening = event_with_keys(&blocked(Some(completed_first.id.to_hex())), &lead);
    assert_eq!(
        validate_coding_session_team_transaction_supersession(&reopening, &completed_first)
            .unwrap_err(),
        TERMINAL_COMPLETION_IS_NOT_REOPENED
    );
    assert_eq!(
        TERMINAL_COMPLETION_IS_NOT_REOPENED,
        "a completion may correct a blocked, never the reverse: publish a new mission.blocked, \
         or a note, rather than correcting a completion"
    );

    // The crossing is the *only* one: every other pair of types still refuses.
    let other_author = Keys::generate();
    let foreign = event_with_keys(&blocked(None), &other_author);
    let across_authors = event_with_keys(&completed(Some(foreign.id.to_hex())), &lead);
    assert_eq!(
        validate_coding_session_team_transaction_supersession(&across_authors, &foreign)
            .unwrap_err(),
        "a correction must have the same signed author"
    );
}

/// The two terminals share one correction subject — "how this mission ended" —
/// so a completion superseding a blocked is a same-subject correction rather
/// than an `InvalidCorrection`. Direction is the type rule's job, not this
/// string's.
#[test]
fn both_terminals_share_one_correction_subject() {
    let blocked = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::MissionBlocked,
        supersedes: None,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::MissionBlocked(CodingSessionTeamMissionBlocked {
            assignment_refs: Vec::new(),
            summary: "Blocked".into(),
            blockers: vec!["one".into()],
            held_on: None,
            required_action: "Fix it".into(),
        }),
    };
    let completed = CodingSessionTeamTransactionPayload {
        transaction_type: CodingSessionTeamTransactionType::MissionCompleted,
        body: CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![id("cd")],
                landed_shas: Vec::new(),
                summary: "Done".into(),
                follow_ups: Vec::new(),
            },
        ),
        ..blocked.clone()
    };
    assert_eq!(logical_subject(&blocked), "mission.terminal");
    assert_eq!(logical_subject(&completed), "mission.terminal");
}
