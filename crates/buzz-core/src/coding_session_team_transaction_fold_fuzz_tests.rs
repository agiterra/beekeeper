//! Seeded fuzz: one reference mistake per set must never fail the fold.
//!
//! A child of `coding_session_team_transaction_fold_tests`, split out only to
//! keep every file under 1,000 lines (REVIEW-B1b R6). No behaviour change:
//! `use super::*` brings in that module's fixture helpers and, through its
//! own glob, the fold under test.

use super::*;

/// Seeded xorshift — the fuzz below must be reproducible from its own source,
/// never from wall-clock or thread entropy.
struct FoldFuzzRng(u64);

impl FoldFuzzRng {
    fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }

    fn hex64(&mut self) -> String {
        (0..4).fold(String::with_capacity(64), |mut text, _| {
            text.push_str(&format!("{:016x}", self.next_u64()));
            text
        })
    }
}

#[test]
fn five_hundred_single_reference_mistakes_all_fold_without_a_hard_error() {
    let mut rng = FoldFuzzRng(0x5eed_1234_9abc_def0);
    let mut folded = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for iteration in 0..500usize {
        // Fifteen mistake sites, one per set. Sites 0-8 replace a single
        // reference with an id nobody supplied; the chain is built in causal
        // order with the mistake applied before signing, so every other pointer
        // names a real supplied id. Sites 9-11 leave the chain flawless and
        // append one invalid correction (changed subject, changed author,
        // changed operation type) — fix round 2's shapes. Sites 12-14 append
        // one wrong-type reference — fix round 3's.
        let site = iteration % 15;
        let missing = rng.hex64();
        let founder = Keys::generate();
        let actor = Keys::generate();
        let verifier = Keys::generate();
        let context = context(&founder, vec![(&actor, "builder"), (&verifier, "verifier")]);

        let mut assignment_payload = assignment(&actor);
        if site == 8 {
            assignment_payload.supersedes = Some(missing.clone());
        }
        let assignment_event = signed(&assignment_payload, &founder, 1);
        let assignment_id = assignment_event.id.to_hex();

        let report_event = signed(
            &report(if site == 0 { &missing } else { &assignment_id }, "Done"),
            &actor,
            2,
        );
        let report_id = report_event.id.to_hex();

        let refutation_event = signed(
            &refutation(
                if site == 1 { &missing } else { &assignment_id },
                if site == 2 { &missing } else { &report_id },
            ),
            &verifier,
            3,
        );
        let refutation_id = refutation_event.id.to_hex();

        let disposition_event = signed(
            &disposition(
                if site == 3 { &missing } else { &assignment_id },
                if site == 4 { &missing } else { &report_id },
                Some(if site == 5 {
                    missing.clone()
                } else {
                    refutation_id.clone()
                }),
                CodingSessionTeamDispositionDecision::Approve,
            ),
            &founder,
            4,
        );
        let disposition_id = disposition_event.id.to_hex();

        let acknowledgement_event = signed(
            &acknowledgement(if site == 6 { &missing } else { &disposition_id }),
            &actor,
            5,
        );
        let completed_event = signed(
            &completed(if site == 7 { &missing } else { &assignment_id }, "Done"),
            &founder,
            6,
        );

        let mut events = vec![
            assignment_event,
            report_event,
            refutation_event,
            disposition_event,
            acknowledgement_event,
            completed_event,
        ];
        // Sites 12-14: one wrong-type reference appended to a flawless chain.
        let wrong_type_id = if site >= 12 {
            let payload = match site {
                // A report answering a report.
                12 => report(&report_id, "Confused"),
                // An acknowledgement naming an assignment.
                13 => acknowledgement(&assignment_id),
                // A disposition whose `refutationRef` names a disposition.
                _ => disposition(
                    &assignment_id,
                    &report_id,
                    Some(disposition_id.clone()),
                    CodingSessionTeamDispositionDecision::Approve,
                ),
            };
            let signer = if site == 14 { &founder } else { &actor };
            let event = signed(&payload, signer, 7);
            let event_id = event.id.to_hex();
            events.push(event);
            Some(event_id)
        } else {
            None
        };

        // Sites 9-11: one invalid correction appended to a flawless chain.
        let correction_id = if (9..12).contains(&site) {
            let (payload, signer) = match site {
                // Subject change: corrects the assignment to name a different
                // assignee, which is a different logical subject.
                9 => {
                    let mut payload = assignment(&verifier);
                    payload.supersedes = Some(assignment_id.clone());
                    (payload, &founder)
                }
                // Author change: the same body signed by someone else.
                10 => {
                    let mut payload = assignment(&actor);
                    payload.supersedes = Some(assignment_id.clone());
                    (payload, &verifier)
                }
                // Operation-type change: an assignment claiming to correct a
                // report. Signed by the report's own author, so the type check
                // is what rejects it rather than the author check.
                _ => {
                    let mut payload = assignment(&actor);
                    payload.supersedes = Some(report_id.clone());
                    (payload, &actor)
                }
            };
            let correction = signed(&payload, signer, 7);
            let correction_id = correction.id.to_hex();
            events.push(correction);
            Some(correction_id)
        } else {
            None
        };

        match fold_coding_session_team_transactions(&events, &context) {
            Ok(fold) => {
                folded += 1;
                // Whatever the mistake, the assignment-bearing prefix of the
                // session is still answered: a fold that "succeeds" by
                // excluding everything would be no better than the hard error.
                assert!(
                    !fold.included_event_ids.is_empty() || site == 8,
                    "site {site} excluded the entire session"
                );
                // Sites 9-14 leave a flawless chain plus exactly one bad
                // record, so the whole session must survive with exactly one
                // exclusion carrying the right code.
                for (event_id, expected) in [
                    (correction_id, "InvalidCorrection"),
                    (wrong_type_id, "WrongTypeReference"),
                ] {
                    let Some(event_id) = event_id else { continue };
                    assert_eq!(
                        fold.excluded.len(),
                        1,
                        "site {site} excluded more than the one bad record"
                    );
                    assert_eq!(
                        exclusion_code_string(&fold, &event_id),
                        expected,
                        "site {site}"
                    );
                    assert!(fold.canonical_terminal.is_some(), "site {site}");
                }
            }
            Err(error) => errors.push(format!("site {site}: {error}")),
        }
    }

    assert!(
        errors.is_empty(),
        "a single dangling reference must never fail the fold; {} of 500 did, first five: {:?}",
        errors.len(),
        &errors[..errors.len().min(5)]
    );
    assert_eq!(folded, 500);
}

// B1c extends the same guarantee to the two new verbs. B1b's fuzz above is
// carried verbatim so the finalizer's diff lines up; this one adds the new
// reference sites rather than editing it.

#[test]
fn five_hundred_single_reference_mistakes_across_the_new_verbs_all_fold_without_a_hard_error() {
    let mut rng = FoldFuzzRng(0x5eed_1234_9abc_def0);
    let mut folded = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for iteration in 0..500usize {
        // Six reference sites across `note` and `decision.*`, one mistake per
        // set. Site 0 is the control: a note's `refs` are pointers, not causal
        // references, so a dangling one must not exclude the note at all.
        let site = iteration % 6;
        let missing = rng.hex64();
        let founder = Keys::generate();
        let actor = Keys::generate();
        let context = context(&founder, vec![(&actor, "builder")]);

        let assignment_event = signed(&assignment(&actor), &founder, 1);
        let assignment_id = assignment_event.id.to_hex();

        let note_event = signed(
            &note(
                "Nothing is blocked; the lane is rebasing.",
                vec![if site == 0 {
                    missing.clone()
                } else {
                    assignment_id.clone()
                }],
            ),
            &actor,
            2,
        );
        let note_id = note_event.id.to_hex();

        let request_event = signed(
            &decision_request(
                "Ship now, or after the rebuild?",
                if site == 1 {
                    &missing
                } else {
                    CODING_SESSION_TEAM_DECISION_FOUNDER
                },
                vec![if site == 2 {
                    missing.clone()
                } else {
                    assignment_id.clone()
                }],
            ),
            &actor,
            3,
        );
        let request_id = request_event.id.to_hex();

        let answer_event = signed(
            &decision_answer(if site == 3 { &missing } else { &request_id }, 0),
            &founder,
            4,
        );

        let mut correction = decision_request("Ship now, or later?", "founder", Vec::new());
        correction.supersedes = Some(if site == 4 {
            missing.clone()
        } else {
            request_id.clone()
        });
        let correction_event = signed(&correction, &actor, 5);

        let mut blocked_payload = blocked("Signing is held");
        blocked_payload.supersedes = if site == 5 {
            Some(missing.clone())
        } else {
            None
        };
        let blocked_event = signed(&blocked_payload, &founder, 6);

        match fold_coding_session_team_transactions(
            &[
                assignment_event,
                note_event,
                request_event,
                answer_event,
                correction_event,
                blocked_event,
            ],
            &context,
        ) {
            Ok(fold) => {
                folded += 1;
                assert!(
                    fold.included_event_ids.contains(&assignment_id),
                    "site {site} lost the assignment"
                );
                // A note is never collateral damage: it carries no causal
                // reference, so no mistake anywhere can exclude it.
                assert!(
                    fold.included_event_ids.contains(&note_id),
                    "site {site} excluded a note"
                );
                assert_eq!(fold.notes.len(), 1, "site {site} lost the note listing");
            }
            Err(error) => errors.push(format!("site {site}: {error}")),
        }
    }

    assert!(
        errors.is_empty(),
        "a single dangling reference must never fail the fold; {} of 500 did, first five: {:?}",
        errors.len(),
        &errors[..errors.len().min(5)]
    );
    assert_eq!(folded, 500);
}
