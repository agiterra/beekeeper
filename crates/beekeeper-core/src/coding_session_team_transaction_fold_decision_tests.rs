//! Regression suites for `note` and `decision.*` (batch 2 2026-09-01, B1c).
//!
//! A child of `coding_session_team_transaction_fold_regression_tests`, split
//! out only to keep every file under 1,000 lines (REVIEW-B1b R6). `use
//! super::*` brings in the fixture helpers and the fold under test.

use super::*;

#[test]
fn a_note_is_listed_and_changes_no_state_at_all() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    // The note points at the assignment AND at an event nobody supplied: a
    // pointer is not a causal reference, so neither costs the note its place.
    let seat_note = signed(
        &note(
            "Rebasing; nothing is blocked.",
            vec![assignment_event.id.to_hex(), id("77")],
        ),
        &actor,
        2,
    );
    let founder_note = signed(&note("Acknowledged.", Vec::new()), &founder, 3);

    let assignment_id = assignment_event.id.to_hex();
    let seat_note_id = seat_note.id.to_hex();
    let founder_note_id = founder_note.id.to_hex();
    let events = vec![assignment_event, seat_note, founder_note];
    let fold = fold_coding_session_team_transactions(&events, &context).unwrap();

    assert_eq!(
        fold.included_event_ids,
        vec![
            assignment_id.clone(),
            seat_note_id.clone(),
            founder_note_id.clone()
        ]
    );
    assert!(fold.excluded.is_empty());
    assert_eq!(fold.notes.len(), 2);
    assert_eq!(fold.notes[0].event_id, seat_note_id);
    assert_eq!(fold.notes[0].author_pubkey, actor.public_key().to_hex());
    assert_eq!(fold.notes[0].refs, vec![assignment_id.clone(), id("77")]);
    assert_eq!(fold.notes[1].event_id, founder_note_id);
    assert!(fold.notes[1].refs.is_empty());

    // Nothing a note touches is state: no terminal, no settlement, no waiting.
    assert!(fold.canonical_terminal.is_none());
    assert!(fold.waiting_on_decision.is_none());
    assert!(fold.decisions.is_empty());
    assert_eq!(fold.assignments.len(), 1);
    assert!(!fold.assignments[0].settled);
    assert!(fold.conflicts.is_empty());

    let reversed: Vec<Event> = events.iter().cloned().rev().collect();
    assert_eq!(
        fold_coding_session_team_transactions(&reversed, &context).unwrap(),
        fold
    );
}

#[test]
fn a_note_from_an_unseated_stranger_is_unauthorized() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let stranger_note = signed(&note("I was never hired.", Vec::new()), &stranger, 1);
    let stranger_note_id = stranger_note.id.to_hex();

    let fold = fold_coding_session_team_transactions(&[stranger_note], &context).unwrap();
    assert!(fold.included_event_ids.is_empty());
    assert!(fold.notes.is_empty());
    assert_eq!(
        exclusion_code_string(&fold, &stranger_note_id),
        "Unauthorized"
    );
}

#[test]
fn an_open_request_blocking_active_work_waits_on_a_person_without_a_terminal() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let request = signed(
        &decision_request(
            "Ship the CLI fix now, or after the app rebuild?",
            "founder",
            vec![assignment_id.clone()],
        ),
        &actor,
        2,
    );
    let request_id = request.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[assignment_event.clone(), request.clone()],
        &context,
    )
    .unwrap();
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].request_event_id, request_id);
    assert_eq!(fold.decisions[0].held_on, "founder");
    assert_eq!(fold.decisions[0].blocks, vec![assignment_id.clone()]);
    assert_eq!(fold.decisions[0].answered_by, None);
    assert_eq!(fold.decisions[0].answer_event_id, None);
    let waiting = fold.waiting_on_decision.clone().unwrap();
    assert_eq!(waiting.request_event_id, request_id);
    assert_eq!(waiting.held_on, "founder");
    // The whole point: waiting on a person is NOT a terminal.
    assert!(fold.canonical_terminal.is_none());
    assert!(fold.excluded.is_empty());

    // The founder answers; the waiting state clears with no terminal either.
    let answer = signed(&decision_answer(&request_id, 1), &founder, 3);
    let answer_id = answer.id.to_hex();
    let answered =
        fold_coding_session_team_transactions(&[assignment_event, request, answer], &context)
            .unwrap();
    assert_eq!(answered.decisions.len(), 1);
    assert_eq!(
        answered.decisions[0].answered_by,
        Some(founder.public_key().to_hex())
    );
    assert_eq!(answered.decisions[0].answer_event_id, Some(answer_id));
    assert!(answered.waiting_on_decision.is_none());
    assert!(answered.canonical_terminal.is_none());
}

/// **Finding 16, live run 2 (10:38–10:40).** The first decision ever put on
/// this wire — `2099cdb3`, held on the founder, three options — carried
/// `blocks: []` because no assignment existed yet: the lead was asking
/// *before* assigning. The lead then ended its turn "held on you: the ruling"
/// and the fold answered `waitingOnDecision: null`, so the rail could not say
/// "waiting on the founder" for exactly the case the verb was built for.
///
/// The rule this asserts: the oldest canonical unanswered `decision.request`
/// **is** the waiting state, whatever `blocks` holds. `blocks` stays in
/// `decisions[].blocks` and answers *which assignments it holds up*, never
/// *whether anyone is waiting*.
#[test]
fn a_founder_held_question_with_no_blocks_is_the_waiting_state() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    // The live shape: no `blocks` at all, and no assignment anywhere.
    let request = signed(
        &decision_request("Which relay?", "founder", Vec::new()),
        &actor,
        1,
    );
    let request_id = request.id.to_hex();

    let fold =
        fold_coding_session_team_transactions(std::slice::from_ref(&request), &context).unwrap();
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].request_event_id, request_id);
    assert!(fold.decisions[0].blocks.is_empty());
    let waiting = fold
        .waiting_on_decision
        .clone()
        .expect("a question nobody answered is a mission waiting on a person");
    assert_eq!(waiting.request_event_id, request_id);
    assert_eq!(waiting.held_on, "founder");
    // Nothing else moved: the waiting state is not a terminal and excludes
    // nothing.
    assert!(fold.canonical_terminal.is_none());
    assert!(fold.excluded.is_empty());

    // Answered, the waiting state clears — the only thing that clears it.
    let answer = signed(&decision_answer(&request_id, 0), &founder, 2);
    let answered = fold_coding_session_team_transactions(&[request, answer], &context).unwrap();
    assert!(answered.waiting_on_decision.is_none());
    assert!(answered.decisions[0].answer_event_id.is_some());
}

/// Two open questions are two real facts; the rail can only point at one, so
/// it points at the one that has been waiting longest.
#[test]
fn the_oldest_unanswered_question_is_the_one_being_waited_on() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let first = signed(
        &decision_request("Which relay?", "founder", Vec::new()),
        &actor,
        1,
    );
    let first_id = first.id.to_hex();
    let second = signed(
        &decision_request("Rebase or merge?", &actor.public_key().to_hex(), Vec::new()),
        &founder,
        2,
    );
    let second_id = second.id.to_hex();

    let events = vec![first, second];
    let fold = fold_coding_session_team_transactions(&events, &context).unwrap();
    assert_eq!(fold.decisions.len(), 2);
    assert_eq!(
        fold.waiting_on_decision.clone().unwrap().request_event_id,
        first_id
    );
    assert_eq!(fold.waiting_on_decision.clone().unwrap().held_on, "founder");

    // Supplied in the other order the answer is identical: the fold sorts its
    // own records, so nothing here depends on how a relay handed them over.
    let reversed: Vec<Event> = events.iter().cloned().rev().collect();
    assert_eq!(
        fold_coding_session_team_transactions(&reversed, &context).unwrap(),
        fold
    );

    // Answer the older one and the younger one takes its place, held on the
    // seat it names rather than on the founder.
    let mut answered = events;
    answered.push(signed(&decision_answer(&first_id, 0), &founder, 3));
    let fold = fold_coding_session_team_transactions(&answered, &context).unwrap();
    let waiting = fold.waiting_on_decision.clone().unwrap();
    assert_eq!(waiting.request_event_id, second_id);
    assert_eq!(waiting.held_on, actor.public_key().to_hex());
}

#[test]
fn only_the_named_party_or_the_founder_may_answer() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder"), (&stranger, "verifier")]);
    let held_on_actor = signed(
        &decision_request("Rebase or merge?", &actor.public_key().to_hex(), Vec::new()),
        &founder,
        1,
    );
    let request_id = held_on_actor.id.to_hex();

    let by_actor = signed(&decision_answer(&request_id, 0), &actor, 2);
    let by_actor_id = by_actor.id.to_hex();
    let fold = fold_coding_session_team_transactions(&[held_on_actor.clone(), by_actor], &context)
        .unwrap();
    assert_eq!(fold.decisions[0].answer_event_id, Some(by_actor_id));

    // The founder may always rule, whoever the request named.
    let by_founder = signed(&decision_answer(&request_id, 0), &founder, 3);
    let by_founder_id = by_founder.id.to_hex();
    let fold =
        fold_coding_session_team_transactions(&[held_on_actor.clone(), by_founder], &context)
            .unwrap();
    assert_eq!(fold.decisions[0].answer_event_id, Some(by_founder_id));

    // A seated verifier the request did not name has no standing here.
    let by_stranger = signed(&decision_answer(&request_id, 0), &stranger, 4);
    let by_stranger_id = by_stranger.id.to_hex();
    let fold =
        fold_coding_session_team_transactions(&[held_on_actor, by_stranger], &context).unwrap();
    assert_eq!(
        exclusion_code_string(&fold, &by_stranger_id),
        "Unauthorized"
    );
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].answer_event_id, None);
    assert_eq!(fold.decisions[0].answered_by, None);
}

#[test]
fn a_founder_held_request_is_not_answerable_by_a_seat() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let request = signed(
        &decision_request("Ship it?", "founder", Vec::new()),
        &actor,
        1,
    );
    let request_id = request.id.to_hex();
    let self_answer = signed(&decision_answer(&request_id, 0), &actor, 2);
    let self_answer_id = self_answer.id.to_hex();

    let fold = fold_coding_session_team_transactions(&[request, self_answer], &context).unwrap();
    assert_eq!(
        exclusion_code_string(&fold, &self_answer_id),
        "Unauthorized"
    );
    assert_eq!(fold.decisions[0].answer_event_id, None);
}

#[test]
fn two_answers_to_one_request_pick_a_deterministic_winner_and_disclose_it() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let request = signed(
        &decision_request("Ship it?", "founder", Vec::new()),
        &actor,
        1,
    );
    let request_id = request.id.to_hex();
    let first = signed(&decision_answer(&request_id, 0), &founder, 2);
    let second = signed(&decision_answer(&request_id, 1), &founder, 3);
    let first_id = first.id.to_hex();
    let second_id = second.id.to_hex();

    let fold =
        fold_coding_session_team_transactions(&[request, first.clone(), second], &context).unwrap();
    assert_eq!(fold.decisions[0].answer_event_id, Some(second_id.clone()));
    let conflict = fold
        .conflicts
        .iter()
        .find(|conflict| conflict.subject == format!("decision:{request_id}"))
        .unwrap();
    assert_eq!(conflict.winner_event_id, second_id);
    assert_eq!(conflict.contender_event_ids, vec![first_id, second_id]);
}

#[test]
fn a_request_naming_an_unresolvable_assignment_stays_a_real_open_question() {
    // REVIEW-B1c F3: `blocks` are pointers, not causal references. A question
    // must outlive the work it is about — including work the caller did not
    // supply — so the request is canonical and simply produces no waiting
    // state, rather than being deleted along with its pointer.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let missing = id("77");
    let request = signed(
        &decision_request("Ship it?", "founder", vec![missing.clone()]),
        &actor,
        2,
    );
    let request_id = request.id.to_hex();
    let answer = signed(&decision_answer(&request_id, 0), &founder, 3);
    let answer_id = answer.id.to_hex();

    let fold =
        fold_coding_session_team_transactions(&[assignment_event, request, answer], &context)
            .unwrap();
    assert_eq!(
        fold.included_event_ids,
        vec![assignment_id, request_id.clone(), answer_id.clone()]
    );
    assert!(fold.excluded.is_empty());
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].request_event_id, request_id);
    assert_eq!(fold.decisions[0].blocks, vec![missing]);
    assert_eq!(fold.decisions[0].answer_event_id, Some(answer_id));
    // The question was answered, so nothing is waiting. An unresolvable
    // `blocks` pointer never decided that either way (finding 16).
    assert!(fold.waiting_on_decision.is_none());
}

#[test]
fn correcting_an_assignment_never_deletes_the_question_it_blocks() {
    // REVIEW-B1c F3, the live shape: the lead fixes a typo in an assignment
    // brief and the founder's open ruling must not vanish from the fold.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let first = signed(&assignment(&actor), &founder, 1);
    let first_id = first.id.to_hex();
    let mut corrected = assignment(&actor);
    corrected.supersedes = Some(first_id.clone());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut corrected.body {
        body.brief = "Implement the bounded assigned slice. (typo fixed)".into();
    }
    let second = signed(&corrected, &founder, 2);
    let second_id = second.id.to_hex();
    let request = signed(
        &decision_request("Ship it?", "founder", vec![first_id.clone()]),
        &actor,
        3,
    );
    let request_id = request.id.to_hex();

    let fold = fold_coding_session_team_transactions(&[first, second, request], &context).unwrap();
    assert_eq!(
        fold.included_event_ids,
        vec![second_id, request_id.clone()],
        "the corrected assignment wins and the question survives"
    );
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].request_event_id, request_id);
    assert_eq!(fold.decisions[0].blocks, vec![first_id]);
    // The blocked id is now superseded, so the *completion* rule no longer
    // holds anything up — but the question is still open and nobody answered
    // it, so the mission is still waiting on a person (finding 16).
    assert_eq!(
        fold.waiting_on_decision.clone().unwrap().request_event_id,
        request_id
    );
    assert_eq!(
        exclusion_code_string(&fold, &fold.excluded[0].event_id),
        "Superseded"
    );
    assert_eq!(fold.excluded.len(), 1);
}

#[test]
fn a_wrong_type_answer_pointer_excludes_that_record_alone() {
    // The rule B1b round 3 established, for the one new verb that still has a
    // causal reference: `decision.answer.requestRef`. A request's `blocks` is
    // a pointer (F3) and is deliberately not type-checked.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let report_event = signed(&report(&assignment_id, "Done"), &actor, 2);
    let report_id = report_event.id.to_hex();
    // `blocks` naming a report is a pointer at the wrong thing, not a defect.
    let request = signed(
        &decision_request("Ship it?", "founder", vec![report_id.clone()]),
        &actor,
        3,
    );
    let request_id = request.id.to_hex();
    // `requestRef` naming an assignment is a wrong-type causal reference.
    let bad_answer = signed(&decision_answer(&assignment_id, 0), &founder, 4);
    let bad_answer_id = bad_answer.id.to_hex();
    let good_note = signed(&note("Both pointers are odd.", Vec::new()), &actor, 5);
    let good_note_id = good_note.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            report_event,
            request,
            bad_answer,
            good_note,
        ],
        &context,
    )
    .expect("a wrong-type pointer must not fail the whole fold");

    assert_eq!(
        fold.included_event_ids,
        vec![assignment_id, report_id, request_id.clone(), good_note_id]
    );
    assert_eq!(
        exclusion_code_string(&fold, &bad_answer_id),
        "WrongTypeReference"
    );
    assert_eq!(fold.excluded.len(), 1);
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].answer_event_id, None);
    // The only answer was excluded, so the question stands unanswered and the
    // mission is waiting — a wrong-type pointer costs that record its place,
    // never the question its waiting state.
    assert_eq!(
        fold.waiting_on_decision.clone().unwrap().request_event_id,
        request_id
    );
    assert_eq!(fold.notes.len(), 1);
}

#[test]
fn a_completion_cannot_outrun_an_open_ruling_it_asked_for() {
    // REVIEW-B1c F1: before this arm the fold answered `canonical_terminal =
    // MissionCompleted` and `waiting_on_decision = founder` at the same time,
    // with nothing excluded — two contradictory signed answers and no rule for
    // which wins. Same class of product lie as the four `mission.blocked`
    // records this lane exists to fix.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let report_event = signed(&report(&assignment_id, "Done"), &actor, 2);
    let disposition_event = signed(
        &disposition(
            &assignment_id,
            &report_event.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let acknowledgement_event = signed(&acknowledgement(&disposition_event.id.to_hex()), &actor, 4);
    let request = signed(
        &decision_request("Ship it?", "founder", vec![assignment_id.clone()]),
        &actor,
        5,
    );
    let request_id = request.id.to_hex();
    let completed_event = signed(&completed(&assignment_id, "Done"), &founder, 6);
    let completed_id = completed_event.id.to_hex();
    let events = vec![
        assignment_event,
        report_event,
        disposition_event,
        acknowledgement_event,
        request,
        completed_event,
    ];

    let fold = fold_coding_session_team_transactions(&events, &context).unwrap();
    assert!(
        fold.canonical_terminal.is_none(),
        "a mission waiting on a person is not complete"
    );
    assert_eq!(
        exclusion_code_string(&fold, &completed_id),
        "CompletionBlockedByOpenDecision"
    );
    assert!(exclusion_reason(&fold, &completed_id).contains(&assignment_id));
    let waiting = fold.waiting_on_decision.clone().unwrap();
    assert_eq!(waiting.request_event_id, request_id);
    assert_eq!(waiting.held_on, "founder");
    // The approval chain itself is untouched: this is not `CompletionNotApproved`.
    assert!(fold.assignments[0].settled);

    // Answer the request and the very same completion folds.
    let answer = signed(&decision_answer(&request_id, 0), &founder, 7);
    let mut answered = events.clone();
    answered.push(answer);
    let fold = fold_coding_session_team_transactions(&answered, &context).unwrap();
    assert_eq!(
        fold.canonical_terminal.unwrap().event_id,
        completed_id,
        "answering the ruling unblocks the completion"
    );
    assert!(fold.waiting_on_decision.is_none());
}

#[test]
fn a_completion_is_not_blocked_by_a_request_about_other_work() {
    // Finding 16's third consequence, stated as a test: widening the waiting
    // state must widen **no exclusion**. `completion_blocked_by_open_decision`
    // keys on `open_blocks`, so a request naming nothing this completion names
    // still lets the completion fold — while the fold now also says, honestly,
    // that a person is being waited on.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let report_event = signed(&report(&assignment_id, "Done"), &actor, 2);
    let disposition_event = signed(
        &disposition(
            &assignment_id,
            &report_event.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let acknowledgement_event = signed(&acknowledgement(&disposition_event.id.to_hex()), &actor, 4);
    // An open question about nothing this completion names.
    let request = signed(
        &decision_request("Which relay?", "founder", Vec::new()),
        &actor,
        5,
    );
    let completed_event = signed(&completed(&assignment_id, "Done"), &founder, 6);
    let completed_id = completed_event.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[
            assignment_event,
            report_event,
            disposition_event,
            acknowledgement_event,
            request,
            completed_event,
        ],
        &context,
    )
    .unwrap();
    assert_eq!(fold.canonical_terminal.unwrap().event_id, completed_id);
    assert!(
        fold.excluded.is_empty(),
        "a question about other work excludes nothing"
    );
    assert_eq!(
        fold.waiting_on_decision.unwrap().held_on,
        "founder",
        "the completion folds AND the open question is still waiting on a person"
    );
}

#[test]
fn a_seat_cannot_take_a_founder_held_ruling_back_and_answer_it_itself() {
    // REVIEW-B1c F2: `heldOn` is part of the request's logical subject, so a
    // correction that moves the ruling onto its own asker is an invalid
    // correction. A queue a seat can unilaterally empty is not a queue.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let request = signed(
        &decision_request("Ship it?", "founder", vec![assignment_id.clone()]),
        &actor,
        2,
    );
    let request_id = request.id.to_hex();
    let mut rewritten = decision_request(
        "Ship it?",
        &actor.public_key().to_hex(),
        vec![assignment_id],
    );
    rewritten.supersedes = Some(request_id.clone());
    let rewritten_event = signed(&rewritten, &actor, 3);
    let rewritten_id = rewritten_event.id.to_hex();
    let self_answer = signed(&decision_answer(&rewritten_id, 0), &actor, 4);
    let self_answer_id = self_answer.id.to_hex();

    let fold = fold_coding_session_team_transactions(
        &[assignment_event, request, rewritten_event, self_answer],
        &context,
    )
    .unwrap();

    assert_eq!(
        exclusion_code_string(&fold, &rewritten_id),
        "InvalidCorrection"
    );
    assert!(exclusion_reason(&fold, &rewritten_id).contains("logical subject"));
    assert_eq!(
        exclusion_code_string(&fold, &self_answer_id),
        "DependentOnExcluded"
    );
    // The founder's question is still canonical, still open, still waiting.
    assert_eq!(fold.decisions.len(), 1);
    assert_eq!(fold.decisions[0].request_event_id, request_id);
    assert_eq!(fold.decisions[0].held_on, "founder");
    assert_eq!(fold.decisions[0].answer_event_id, None);
    assert_eq!(
        fold.waiting_on_decision.clone().unwrap().request_event_id,
        request_id
    );
}

#[test]
fn a_prose_only_correction_of_a_blocked_terminal_is_an_invalid_correction() {
    // REVIEW-B1c F5, stronger reading: editing a terminal's sentence without
    // changing what is blocking is what a `note` is for.
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let first = signed(&blocked("Signing is held"), &founder, 1);
    let first_id = first.id.to_hex();

    let mut prose_only = blocked("Signing is still held, per the 23:06 sync");
    prose_only.supersedes = Some(first_id.clone());
    let prose_only_event = signed(&prose_only, &founder, 2);
    let prose_only_id = prose_only_event.id.to_hex();

    let fold = fold_coding_session_team_transactions(&[first.clone(), prose_only_event], &context)
        .unwrap();
    assert_eq!(
        exclusion_code_string(&fold, &prose_only_id),
        "InvalidCorrection"
    );
    assert!(exclusion_reason(&fold, &prose_only_id).contains("must change its blockers"));
    assert_eq!(fold.canonical_terminal.unwrap().event_id, first_id);

    // Reordering the same blockers is prose too.
    let mut two = blocked("Two blockers");
    if let CodingSessionTeamTransactionBody::MissionBlocked(body) = &mut two.body {
        body.blockers = vec!["keychain".into(), "relay".into()];
    }
    let two_event = signed(&two, &founder, 3);
    let mut reordered = blocked("Two blockers, reordered");
    if let CodingSessionTeamTransactionBody::MissionBlocked(body) = &mut reordered.body {
        body.blockers = vec!["relay".into(), "keychain".into()];
    }
    reordered.supersedes = Some(two_event.id.to_hex());
    let reordered_event = signed(&reordered, &founder, 4);
    let reordered_id = reordered_event.id.to_hex();
    let fold =
        fold_coding_session_team_transactions(&[two_event, reordered_event], &context).unwrap();
    assert_eq!(
        exclusion_code_string(&fold, &reordered_id),
        "InvalidCorrection"
    );

    // A correction that actually changes the blockers is still a correction.
    let mut real = blocked("The relay is down too");
    if let CodingSessionTeamTransactionBody::MissionBlocked(body) = &mut real.body {
        body.blockers = vec!["External dependency".into(), "the relay is down".into()];
    }
    real.supersedes = Some(first_id.clone());
    let real_event = signed(&real, &founder, 5);
    let real_id = real_event.id.to_hex();
    let fold = fold_coding_session_team_transactions(&[first, real_event], &context).unwrap();
    assert_eq!(exclusion_code_string(&fold, &first_id), "Superseded");
    assert_eq!(fold.canonical_terminal.unwrap().event_id, real_id);
}
