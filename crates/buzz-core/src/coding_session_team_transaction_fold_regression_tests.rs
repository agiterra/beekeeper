//! Regression suites for the dangling / wrong-type / invalid-correction rulings.
//!
//! A child of `coding_session_team_transaction_fold_tests`, split out only to
//! keep every file under 1,000 lines (REVIEW-B1b R6). No behaviour change:
//! `use super::*` brings in that module's fixture helpers and, through its
//! own glob, the fold under test.

use super::*;

// ── Dangling references are one record's defect (batch 2 2026-09-01, B1b) ────
//
// Live 2026-09-01 22:13: the runner's report `c737be4c` carried an
// `assignmentRef` naming an event that was not a team transaction of the
// session. The fold treated that as a structural failure of the whole supplied
// set, so `bee sessions operation get|list` failed closed for *every*
// operation until the lead published a repair — one seat's malformed record
// denying every seat the governance state. The record is excluded now, and so
// is every other record-local defect: a wrong-type pointer is that record's
// own `WrongTypeReference` and a bad correction its own `InvalidCorrection`
// (rounds 2 and 3). Only the caller's own filtering mistakes — **cross-context**
// events and reference **cycles** — remain hard errors, exactly as the module
// doc on `fold_coding_session_team_transactions` states.

#[test]
fn live_replay_a_dangling_assignment_ref_excludes_only_that_report() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    let verdict = signed(
        &disposition(
            &assignment_event.id.to_hex(),
            &good_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let missing = id("77");
    let bad_report = signed(&report(&missing, "Runner report c737be4c"), &actor, 4);

    let assignment_id = assignment_event.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let verdict_id = verdict.id.to_hex();
    let bad_report_id = bad_report.id.to_hex();
    let events = vec![assignment_event, good_report, verdict, bad_report];

    let fold = fold_coding_session_team_transactions(&events, &context).unwrap();

    assert_eq!(
        fold.included_event_ids,
        vec![
            assignment_id.clone(),
            good_report_id.clone(),
            verdict_id.clone()
        ]
    );
    assert_eq!(fold.excluded.len(), 1);
    assert_eq!(fold.excluded[0].event_id, bad_report_id);
    assert_eq!(
        exclusion_code_string(&fold, &bad_report_id),
        "DanglingReference"
    );
    assert!(exclusion_reason(&fold, &bad_report_id).contains(&missing));
    assert!(fold.conflicts.is_empty());
    assert!(fold.unseated_reports.is_empty());
    assert!(fold.canonical_terminal.is_none());
    assert_eq!(fold.assignments.len(), 1);
    assert_eq!(fold.assignments[0].assignment_event_id, assignment_id);
    // Lane 210: this set's approving disposition asks for nothing, so the
    // assignment settles here with no acknowledgement on the wire. It read
    // `!settled` before; the assertion is inverted deliberately and the rule
    // that settled it is asserted rather than inferred. What this test is
    // about — one dangling report excluded, everything else included, the
    // same answer in either direction — is untouched.
    assert!(fold.assignments[0].settled);
    assert_eq!(
        fold.assignments[0].settled_by,
        Some(CodingSessionTeamSettledBy::ApprovingDispositionWithoutAsk)
    );
    assert_eq!(fold.assignments[0].acknowledgement_event_id, None);

    let reversed: Vec<Event> = events.iter().cloned().rev().collect();
    let backward = fold_coding_session_team_transactions(&reversed, &context).unwrap();
    assert_eq!(fold, backward);
}

#[test]
fn a_record_depending_on_a_dangling_record_is_dependent_on_excluded() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let report_event = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    let bad_disposition = signed(
        &disposition(
            &assignment_event.id.to_hex(),
            &report_event.id.to_hex(),
            Some(id("77")),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let acknowledgement_event = signed(&acknowledgement(&bad_disposition.id.to_hex()), &actor, 4);

    let assignment_id = assignment_event.id.to_hex();
    let report_id = report_event.id.to_hex();
    let disposition_id = bad_disposition.id.to_hex();
    let acknowledgement_id = acknowledgement_event.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            report_event,
            bad_disposition,
            acknowledgement_event,
        ],
        &context,
    )
    .unwrap();

    assert_eq!(fold.included_event_ids, vec![assignment_id, report_id]);
    assert_eq!(
        exclusion_code_string(&fold, &disposition_id),
        "DanglingReference"
    );
    assert_eq!(
        exclusion_code_string(&fold, &acknowledgement_id),
        "DependentOnExcluded"
    );
    assert!(!fold.assignments[0].settled);
}

#[test]
fn a_dangling_supersedes_excludes_the_correction_and_keeps_the_original() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let original = signed(&assignment(&actor), &founder, 1);
    let missing = id("77");
    let mut correction_payload = assignment(&actor);
    correction_payload.supersedes = Some(missing.clone());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut correction_payload.body {
        body.objective = "Correction of an event nobody supplied".into();
    }
    let correction = signed(&correction_payload, &founder, 2);

    let original_id = original.id.to_hex();
    let correction_id = correction.id.to_hex();
    let fold = fold_coding_session_team_transactions(&[original, correction], &context).unwrap();

    assert_eq!(fold.included_event_ids, vec![original_id.clone()]);
    assert_eq!(
        exclusion_code_string(&fold, &correction_id),
        "DanglingReference"
    );
    assert!(exclusion_reason(&fold, &correction_id).contains(&missing));
    assert_eq!(fold.assignments[0].assignment_event_id, original_id);
}

#[test]
fn cross_context_stays_a_hard_error_beside_a_dangling_and_a_wrong_type_record() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "One"), &actor, 2);
    // Fix round 3: a report whose `assignmentRef` names a report is excluded,
    // not fatal.
    let wrong_type = signed(&report(&good_report.id.to_hex(), "Two"), &actor, 3);
    let dangling = signed(&report(&id("77"), "Dangling"), &actor, 4);
    let assignment_id = assignment_event.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let wrong_type_id = wrong_type.id.to_hex();
    let dangling_id = dangling.id.to_hex();
    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            good_report,
            wrong_type.clone(),
            dangling.clone(),
        ],
        &context,
    )
    .unwrap();
    assert_eq!(fold.included_event_ids, vec![assignment_id, good_report_id]);
    assert_eq!(
        exclusion_code_string(&fold, &wrong_type_id),
        "WrongTypeReference"
    );
    assert_eq!(
        exclusion_code_string(&fold, &dangling_id),
        "DanglingReference"
    );

    let cross_channel = signed_in_channel(&assignment(&actor), &founder, 5, OTHER_CHANNEL);
    assert!(fold_coding_session_team_transactions(
        &[cross_channel, wrong_type, dangling],
        &context
    )
    .unwrap_err()
    .contains("channel"));
}

// ── Fix round 1 (REVIEW-B1b F1) ──────────────────────────────────────────────
//
// The lane closed one of two doors onto the same denial of service. A verdict
// whose `assignmentRef` is dangling, or that governs a report whose
// `assignmentRef` is dangling, still hard-errored the whole fold — the live
// c737be4c shape one governance step further on, where the lead governs the
// malformed report. The reviewer's fuzz put 440 of 500 single random reference
// mistakes on that path. Cross-record pointer agreement is now compared only
// when both sides resolve.

#[test]
fn a_lead_verdict_over_a_dangling_report_is_dependent_on_excluded() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let missing = id("77");
    // The runner's `c737be4c`: a report naming an assignment nobody supplied.
    let bad_report = signed(&report(&missing, "Runner report c737be4c"), &actor, 2);
    // The lead governs it, naming the real assignment it issued — which is the
    // only assignment a lead actually knows.
    let verdict = signed(
        &disposition(
            &assignment_event.id.to_hex(),
            &bad_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let acknowledgement_event = signed(&acknowledgement(&verdict.id.to_hex()), &actor, 4);

    let assignment_id = assignment_event.id.to_hex();
    let bad_report_id = bad_report.id.to_hex();
    let verdict_id = verdict.id.to_hex();
    let acknowledgement_id = acknowledgement_event.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[assignment_event, bad_report, verdict, acknowledgement_event],
        &context,
    )
    .unwrap();

    assert_eq!(fold.included_event_ids, vec![assignment_id.clone()]);
    assert_eq!(
        exclusion_code_string(&fold, &bad_report_id),
        "DanglingReference"
    );
    assert!(exclusion_reason(&fold, &bad_report_id).contains(&missing));
    assert_eq!(
        exclusion_code_string(&fold, &verdict_id),
        "DependentOnExcluded"
    );
    assert_eq!(
        exclusion_code_string(&fold, &acknowledgement_id),
        "DependentOnExcluded"
    );
    assert_eq!(fold.assignments.len(), 1);
    assert_eq!(fold.assignments[0].assignment_event_id, assignment_id);
    assert!(!fold.assignments[0].settled);
    assert!(fold.canonical_terminal.is_none());
}

#[test]
fn a_mistyped_verdict_assignment_ref_excludes_the_verdict_not_the_session() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    let missing = id("77");
    // Every other record is flawless; the lead mistypes one pointer.
    let verdict = signed(
        &disposition(
            &missing,
            &good_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );

    let assignment_id = assignment_event.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let verdict_id = verdict.id.to_hex();

    let fold =
        fold_coding_session_team_transactions(&[assignment_event, good_report, verdict], &context)
            .unwrap();

    assert_eq!(
        fold.included_event_ids,
        vec![assignment_id.clone(), good_report_id]
    );
    assert_eq!(
        exclusion_code_string(&fold, &verdict_id),
        "DanglingReference"
    );
    assert!(exclusion_reason(&fold, &verdict_id).contains(&missing));
    assert_eq!(fold.assignments[0].assignment_event_id, assignment_id);
    assert!(!fold.assignments[0].settled);
}

#[test]
fn a_verdict_report_mismatch_between_two_supplied_assignments_excludes_the_verdict() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let other_actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let first_assignment = signed(&assignment(&actor), &founder, 1);
    let second_assignment = signed(&assignment(&other_actor), &founder, 2);
    let report_on_first = signed(&report(&first_assignment.id.to_hex(), "Done"), &actor, 3);
    // Both sides resolve and disagree, so the comparison still has canonical
    // meaning — but since fix round 3 it excludes the verdict rather than the
    // session.
    let verdict = signed(
        &disposition(
            &second_assignment.id.to_hex(),
            &report_on_first.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );

    let first_assignment_id = first_assignment.id.to_hex();
    let second_assignment_id = second_assignment.id.to_hex();
    let report_id = report_on_first.id.to_hex();
    let verdict_id = verdict.id.to_hex();
    let fold = fold_coding_session_team_transactions(
        &[
            first_assignment,
            second_assignment,
            report_on_first,
            verdict,
        ],
        &context,
    )
    .unwrap();

    assert_eq!(
        fold.included_event_ids,
        vec![first_assignment_id, second_assignment_id, report_id]
    );
    assert_eq!(fold.excluded.len(), 1);
    assert_eq!(
        exclusion_code_string(&fold, &verdict_id),
        "WrongTypeReference"
    );
    assert!(exclusion_reason(&fold, &verdict_id)
        .contains("verdict assignmentRef must match its report's assignmentRef"));
    assert!(fold.assignments.iter().all(|item| !item.settled));
}

// ── Fix round 2 (live wire, TeamRolesV1 2026-09-01 23:06) ───────────────────
//
// The runner published report 46b03d08 carrying `supersedes` c737be4c — the
// dangling report fix round 1 had just made survivable — but with a *different*
// `assignmentRef` (436c10ce, the real assignment, instead of the dangling
// f233c16b). Correcting the bad pointer changed the report's logical subject,
// so `bee sessions operation list` failed closed again with "a correction must
// preserve its logical subject": the same denial of service through the third
// rule. An invalid correction is now one record's defect too. Only
// cross-context events and cycles (and, still, wrong-type references) fail the
// whole fold.

#[test]
fn live_replay_a_report_correction_that_changes_its_assignment_ref_is_excluded_alone() {
    let founder = Keys::generate();
    let runner = Keys::generate();
    let context = context(&founder, vec![(&runner, "builder")]);
    // Field-for-field replay of the 23:06 shape; the real events could not be
    // fetched, because the command that lists them is the one that was failing.
    let assignment_event = signed(&assignment(&runner), &founder, 1); // 436c10ce…
    let missing = id("f2"); // f233c16b…, supplied by nobody
    let original = signed(&report(&missing, "Runner report"), &runner, 2); // c737be4c
    let mut correction_payload = report(
        &assignment_event.id.to_hex(),
        "Runner report, pointer corrected",
    );
    correction_payload.supersedes = Some(original.id.to_hex());
    let correction = signed(&correction_payload, &runner, 3); // 46b03d08

    let assignment_id = assignment_event.id.to_hex();
    let original_id = original.id.to_hex();
    let correction_id = correction.id.to_hex();

    let fold =
        fold_coding_session_team_transactions(&[assignment_event, original, correction], &context)
            .unwrap();

    // Every operation in the session is answered again.
    assert_eq!(fold.included_event_ids, vec![assignment_id.clone()]);
    assert_eq!(
        exclusion_code_string(&fold, &original_id),
        "DanglingReference"
    );
    assert!(exclusion_reason(&fold, &original_id).contains(&missing));
    assert_eq!(
        exclusion_code_string(&fold, &correction_id),
        "InvalidCorrection"
    );
    let reason = exclusion_reason(&fold, &correction_id);
    assert!(reason.contains(&original_id), "reason names its target");
    assert!(
        reason.contains("logical subject"),
        "reason names the defect"
    );
    assert_eq!(fold.excluded.len(), 2);
    assert_eq!(fold.assignments.len(), 1);
    assert_eq!(fold.assignments[0].assignment_event_id, assignment_id);
    assert!(!fold.assignments[0].settled);
}

#[test]
fn a_rejected_correction_leaves_its_target_untouched_and_only_its_dependants_fall_out() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let other_actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let other_assignment = signed(&assignment(&other_actor), &founder, 2);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 3);
    // A correction of a perfectly good, included report that moves it to a
    // different assignment — the subject change the live runner made.
    let mut correction_payload = report(&other_assignment.id.to_hex(), "Moved");
    correction_payload.supersedes = Some(good_report.id.to_hex());
    let correction = signed(&correction_payload, &actor, 4);
    // A verdict governing the rejected correction.
    let verdict = signed(
        &disposition(
            &other_assignment.id.to_hex(),
            &correction.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        5,
    );

    let assignment_id = assignment_event.id.to_hex();
    let other_assignment_id = other_assignment.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let correction_id = correction.id.to_hex();
    let verdict_id = verdict.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            other_assignment,
            good_report,
            correction,
            verdict,
        ],
        &context,
    )
    .unwrap();

    // The target keeps exactly the standing it already had: still included,
    // never superseded by a correction the fold rejected.
    assert_eq!(
        fold.included_event_ids,
        vec![assignment_id, other_assignment_id, good_report_id.clone()]
    );
    assert_eq!(
        exclusion_code_string(&fold, &correction_id),
        "InvalidCorrection"
    );
    assert_eq!(
        exclusion_code_string(&fold, &verdict_id),
        "DependentOnExcluded"
    );
    assert!(
        fold.conflicts.is_empty(),
        "a rejected correction is not a fork"
    );
    assert_eq!(fold.excluded.len(), 2);
    assert_eq!(fold.assignments.len(), 2);
    assert!(fold.assignments.iter().all(|item| !item.settled));
}

#[test]
fn cross_context_and_cycles_are_the_last_whole_set_failures() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);

    // Cross-context: still a hard error, beside a dangling record and an
    // invalid correction.
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let dangling = signed(&report(&id("77"), "Dangling"), &actor, 2);
    let mut bad_correction = assignment(&Keys::generate());
    bad_correction.supersedes = Some(assignment_event.id.to_hex());
    let bad_correction = signed(&bad_correction, &founder, 3);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "One"), &actor, 4);
    let wrong_type = signed(&report(&good_report.id.to_hex(), "Two"), &actor, 5);

    // All three record-local defects present at once, and the fold still
    // answers: only the two caller-level failures below are fatal.
    let survivable = vec![
        assignment_event.clone(),
        dangling.clone(),
        bad_correction.clone(),
        good_report.clone(),
        wrong_type.clone(),
    ];
    let fold = fold_coding_session_team_transactions(&survivable, &context).unwrap();
    assert_eq!(fold.included_event_ids.len(), 2);
    assert_eq!(fold.excluded.len(), 3);
    let mut codes: Vec<String> = fold
        .excluded
        .iter()
        .map(|item| format!("{:?}", item.code))
        .collect();
    codes.sort();
    assert_eq!(
        codes,
        vec![
            "DanglingReference",
            "InvalidCorrection",
            "WrongTypeReference"
        ]
    );

    let cross_channel = signed_in_channel(&assignment(&actor), &founder, 6, OTHER_CHANNEL);
    let mut with_cross_channel = survivable.clone();
    with_cross_channel.push(cross_channel);
    assert!(
        fold_coding_session_team_transactions(&with_cross_channel, &context)
            .unwrap_err()
            .contains("channel")
    );

    let mut cross_session_payload = assignment(&actor);
    cross_session_payload.session_ref = "6b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into();
    let cross_session = signed(&cross_session_payload, &founder, 7);
    let mut with_cross_session = survivable;
    with_cross_session.push(cross_session);
    assert!(
        fold_coding_session_team_transactions(&with_cross_session, &context)
            .unwrap_err()
            .contains("session")
    );
}

// ── Fix round 3 ─────────────────────────────────────────────────────────────
//
// The last record-local hard error. A reference that resolves to a supplied
// record of the wrong operation type — a report's `assignmentRef` naming a
// verdict, an acknowledgement naming an assignment, a disposition's
// `refutationRef` naming another disposition — was still ending the whole
// session's fold. Same shape as rounds 1 and 2, same ruling. After this,
// cross-context events and cycles are the only whole-set failures.

#[test]
fn a_report_whose_assignment_ref_names_a_verdict_is_excluded_alone() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let verifier = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder"), (&verifier, "verifier")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    let refutation_event = signed(
        &refutation(&assignment_event.id.to_hex(), &good_report.id.to_hex()),
        &verifier,
        3,
    );
    // The defect: a report answering a *verdict* instead of an assignment.
    let bad_report = signed(
        &report(&refutation_event.id.to_hex(), "Confused"),
        &actor,
        4,
    );

    let assignment_id = assignment_event.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let refutation_id = refutation_event.id.to_hex();
    let bad_report_id = bad_report.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[assignment_event, good_report, refutation_event, bad_report],
        &context,
    )
    .unwrap();

    assert_eq!(
        fold.included_event_ids,
        vec![assignment_id.clone(), good_report_id, refutation_id]
    );
    assert_eq!(fold.excluded.len(), 1);
    assert_eq!(
        exclusion_code_string(&fold, &bad_report_id),
        "WrongTypeReference"
    );
    assert!(exclusion_reason(&fold, &bad_report_id).contains("wrong-type"));
    assert_eq!(fold.assignments[0].assignment_event_id, assignment_id);
    assert!(!fold.assignments[0].settled);
}

#[test]
fn a_record_depending_on_a_wrong_type_record_is_dependent_on_excluded() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    // An acknowledgement naming an assignment instead of a disposition.
    let bad_ack = signed(&acknowledgement(&assignment_event.id.to_hex()), &actor, 3);
    // A correction of that bad acknowledgement: its own claim is sound, so it
    // falls out only because its parent did.
    let mut correction_payload = acknowledgement(&assignment_event.id.to_hex());
    correction_payload.supersedes = Some(bad_ack.id.to_hex());
    let correction = signed(&correction_payload, &actor, 4);

    let assignment_id = assignment_event.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let bad_ack_id = bad_ack.id.to_hex();
    let correction_id = correction.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[assignment_event, good_report, bad_ack, correction],
        &context,
    )
    .unwrap();

    assert_eq!(fold.included_event_ids, vec![assignment_id, good_report_id]);
    assert_eq!(
        exclusion_code_string(&fold, &bad_ack_id),
        "WrongTypeReference"
    );
    // The correction names the same wrong type, so it earns the same code on
    // its own account rather than inheriting one.
    assert_eq!(
        exclusion_code_string(&fold, &correction_id),
        "WrongTypeReference"
    );
}

#[test]
fn a_disposition_refutation_ref_naming_a_disposition_is_excluded_alone() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    let good_disposition = signed(
        &disposition(
            &assignment_event.id.to_hex(),
            &good_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    // `refutationRef` naming a disposition rather than a refutation.
    let bad_disposition = signed(
        &disposition(
            &assignment_event.id.to_hex(),
            &good_report.id.to_hex(),
            Some(good_disposition.id.to_hex()),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );
    let acknowledgement_event = signed(&acknowledgement(&good_disposition.id.to_hex()), &actor, 5);

    let good_disposition_id = good_disposition.id.to_hex();
    let bad_disposition_id = bad_disposition.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            good_report,
            good_disposition,
            bad_disposition,
            acknowledgement_event,
        ],
        &context,
    )
    .unwrap();

    assert_eq!(fold.included_event_ids.len(), 4);
    assert_eq!(fold.excluded.len(), 1);
    assert_eq!(
        exclusion_code_string(&fold, &bad_disposition_id),
        "WrongTypeReference"
    );
    // The sound governance chain still settles: one bad verdict does not stop
    // the assignment being approved.
    assert!(fold.assignments[0].settled);
    assert_eq!(
        fold.assignments[0].disposition_event_id,
        Some(good_disposition_id)
    );
    assert!(fold.conflicts.is_empty());
}

// ── Fix round 4 (REVIEW-B1b F5) ─────────────────────────────────────────────
//
// `is_authorized` follows an acknowledgement's pointer two hops — to the
// disposition, then to *its* assignment — to find the assigned actor. Nothing
// typed that second hop, so a lead pasting a report id where an assignment id
// belongs, plus a perfectly normal acknowledgement, returned
// Err("expected assignment record") for the whole set. The reviewer's ATTACK1.

#[test]
fn an_acknowledgement_whose_disposition_names_a_non_assignment_is_excluded_not_fatal() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let other_actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let first_assignment = signed(&assignment(&actor), &founder, 1);
    let first_report = signed(&report(&first_assignment.id.to_hex(), "One"), &actor, 2);
    let second_assignment = signed(&assignment(&other_actor), &founder, 3);
    let second_report = signed(
        &report(&second_assignment.id.to_hex(), "Two"),
        &other_actor,
        4,
    );
    // The lead pastes a *report* id where the assignment id belongs. Resolvable,
    // wrong type — the same class of mistake as the live c737be4c, with a real
    // id rather than an absent one.
    let bad_disposition = signed(
        &disposition(
            &second_report.id.to_hex(),
            &first_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        5,
    );
    // A completely normal acknowledgement of it.
    let acknowledgement_event = signed(&acknowledgement(&bad_disposition.id.to_hex()), &actor, 6);

    let first_assignment_id = first_assignment.id.to_hex();
    let first_report_id = first_report.id.to_hex();
    let second_assignment_id = second_assignment.id.to_hex();
    let second_report_id = second_report.id.to_hex();
    let bad_disposition_id = bad_disposition.id.to_hex();
    let acknowledgement_id = acknowledgement_event.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            first_assignment,
            first_report,
            second_assignment,
            second_report,
            bad_disposition,
            acknowledgement_event,
        ],
        &context,
    )
    .unwrap();

    assert_eq!(
        fold.included_event_ids,
        vec![
            first_assignment_id,
            first_report_id,
            second_assignment_id,
            second_report_id
        ]
    );
    assert_eq!(fold.excluded.len(), 2);
    assert_eq!(
        exclusion_code_string(&fold, &bad_disposition_id),
        "WrongTypeReference"
    );
    // The acknowledgement earns its own code from the second hop, rather than
    // detonating the fold or being mislabelled `Unauthorized`.
    assert_eq!(
        exclusion_code_string(&fold, &acknowledgement_id),
        "WrongTypeReference"
    );
    assert_eq!(
        exclusion_reason(&fold, &acknowledgement_id),
        "acknowledgement's disposition names a non-assignment"
    );
    assert!(fold.assignments.iter().all(|item| !item.settled));
}

#[test]
fn an_acknowledgement_of_a_dangling_disposition_still_falls_out_as_dependent() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let report_event = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    // The disposition's assignmentRef is absent rather than wrong-type, so the
    // second-hop type check must stay silent and let the dependency rule speak.
    let dangling_disposition = signed(
        &disposition(
            &id("77"),
            &report_event.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let acknowledgement_event = signed(
        &acknowledgement(&dangling_disposition.id.to_hex()),
        &actor,
        4,
    );

    let disposition_id = dangling_disposition.id.to_hex();
    let acknowledgement_id = acknowledgement_event.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            report_event,
            dangling_disposition,
            acknowledgement_event,
        ],
        &context,
    )
    .unwrap();

    assert_eq!(
        exclusion_code_string(&fold, &disposition_id),
        "DanglingReference"
    );
    assert_eq!(
        exclusion_code_string(&fold, &acknowledgement_id),
        "DependentOnExcluded"
    );
}

// The `note` and `decision.*` suites live in a sibling file for the same
// reason (REVIEW-B1b R6). It is a child of this module, so every fixture
// helper is in scope unchanged.
#[path = "coding_session_team_transaction_fold_decision_tests.rs"]
mod decisions;
