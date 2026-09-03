//! Arm **(B)**: provider-observed gate rows, green on the pushed commit,
//! admit a seat's push with no second seat.
//!
//! The arm L21 could not build. Its whole thesis is the `headSha` a gate row
//! now carries: *these* gates were green *on this commit*, measured by a
//! mechanism the seat cannot sign for. Every case here is written against
//! folded 44246 events rather than hand-built entries, so the provenance
//! check that downgrades a self-asserted `observed` row to `declared`
//! (`coding_session_observation_fold.rs`) is exercised rather than assumed.
//!
//! A sibling file of [`super::arms_tests`] for the same reason that one is a
//! sibling of [`super::tests`]: the repository ceiling is 1,000 lines.

use super::*;

use crate::coding_session_observation::{
    fold_coding_session_observations, CodingSessionObservationBody,
    CodingSessionObservationFoldContext, CodingSessionObservationGate,
    CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
    CodingSessionObservationPayload, CodingSessionObservationSource, CodingSessionObservationType,
    CODING_SESSION_OBSERVATION_SCHEMA,
};
use crate::coding_session_team_transaction::CodingSessionTeamActiveSeat;
use crate::kind::KIND_CODING_SESSION_OBSERVATION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "c0066ddd-8214-4baf-81d2-3046fead0d32";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79";
const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";
const OTHER_SHA: &str = "1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b";

/// One gate row, as the provider would sign it.
struct Row<'a> {
    gate: &'a str,
    outcome: CodingSessionObservationGateOutcome,
    head_sha: Option<&'a str>,
    dirty: Option<bool>,
}

fn row<'a>(gate: &'a str, head_sha: Option<&'a str>) -> Row<'a> {
    Row {
        gate,
        outcome: CodingSessionObservationGateOutcome::Passed,
        head_sha,
        dirty: head_sha.map(|_| false),
    }
}

/// Sign one kind 44246 gate observation carrying `rows`.
fn observation(keys: &Keys, source: CodingSessionObservationSource, rows: &[Row<'_>]) -> Event {
    let payload = CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        observation_type: CodingSessionObservationType::Gate,
        source,
        assignment_ref: None,
        body: CodingSessionObservationBody::Gate(CodingSessionObservationGate {
            rows: rows
                .iter()
                .map(|row| CodingSessionObservationGateRow {
                    gate: row.gate.to_owned(),
                    outcome: row.outcome,
                    command: format!("{} --locked", row.gate),
                    summary: None,
                    duration_ms: Some(1_000),
                    head_sha: row.head_sha.map(str::to_owned),
                    dirty: row.dirty,
                })
                .collect(),
        }),
    };
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        serde_json::to_string(&payload).expect("payload serializes"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("version tag"),
        Tag::parse(["csob-genesis", GENESIS]).expect("genesis tag"),
        Tag::parse(["csob-type", "gate"]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(400))
    .sign_with_keys(keys)
    .expect("event signs")
}

/// A mission whose only records are observations: no assignment, no report and
/// no verdict at all, which is exactly what arm (B) is for.
struct Watched {
    founder: Keys,
    provider: Keys,
    builder: Keys,
    seats: Vec<CodingSessionTeamActiveSeat>,
}

fn watched() -> Watched {
    let founder = Keys::generate();
    let provider = Keys::generate();
    let builder = Keys::generate();
    let seats = vec![CodingSessionTeamActiveSeat {
        actor_pubkey: builder.public_key().to_hex(),
        role: "builder".into(),
    }];
    Watched {
        founder,
        provider,
        builder,
        seats,
    }
}

/// Fold `events` the way the relay does, with the provider set supplied so a
/// misclaimed `observed` is folded down to `declared`.
fn candidate(
    watched: &Watched,
    events: &[Event],
    gate_policy: Option<VerdictAdmissionGatePolicy>,
) -> VerdictAdmissionCandidate {
    let fold = fold_coding_session_observations(
        events,
        &CodingSessionObservationFoldContext {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            known_assignment_refs: Vec::new(),
            provider_pubkeys: Some(vec![watched.provider.public_key().to_hex()]),
        },
    );
    VerdictAdmissionCandidate {
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: watched.founder.public_key().to_hex(),
        canonical: Vec::new(),
        active_seats: watched.seats.clone(),
        observed_gates: fold.gates,
        gate_policy,
    }
}

fn query<'a>(pusher: &'a str, founders: &'a [String]) -> VerdictAdmissionQuery<'a> {
    VerdictAdmissionQuery {
        ref_name: "refs/heads/whoami/cli",
        new_oid: HEAD_SHA,
        pusher_pubkey: pusher,
        repo_founders: founders,
    }
}

/// Every default gate, observed green on the pushed commit by the mission's
/// provider, over a clean worktree — the velocity arm, end to end.
#[test]
fn b_the_default_gates_observed_green_on_the_pushed_sha_admit_a_seat_push() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, Some(HEAD_SHA)))
        .collect();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::ObservedGates {
            session_ref,
            head_sha,
            gates,
            ..
        }) => {
            assert_eq!(session_ref, SESSION);
            assert_eq!(head_sha, HEAD_SHA);
            assert_eq!(gates, DEFAULT_REQUIRED_GATES.to_vec());
        }
        other => panic!("expected an observed-gate admission, got {other:?}"),
    }
}

/// Green rows that name **another** commit admit nothing.
///
/// This is the whole reason the key exists: without it these rows would read
/// as "this mission has green gates", and an earlier commit's green would
/// land a later one — the shape of finding 27.
#[test]
fn b_green_rows_naming_another_commit_do_not_admit_this_one() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, Some(OTHER_SHA)))
        .collect();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Refused(refusal @ VerdictAdmissionRefusal::NoApprovingVerdict { .. }) => {
            let reason = refusal.reason();
            assert!(
                reason.contains(&format!("No observed gate row names {HEAD_SHA}")),
                "{reason}"
            );
        }
        other => panic!("expected the nothing-names-it refusal, got {other:?}"),
    }
}

/// A row a **seat** signed is a claim, not a measurement — even when it says
/// `observed`, which the fold downgrades because the signer is no provider of
/// this mission.
#[test]
fn b_rows_the_seat_signed_are_declared_and_admit_nothing() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, Some(HEAD_SHA)))
        .collect();
    let events = vec![observation(
        // The seat itself, claiming `observed`.
        &watched.builder,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Refused(
            refusal @ VerdictAdmissionRefusal::ObservedRowsAreDeclared { .. },
        ) => {
            assert!(refusal.reason().contains(HEAD_SHA), "{}", refusal.reason());
        }
        other => panic!("expected the declared-not-observed refusal, got {other:?}"),
    }
}

/// One red gate is named, by name, on the commit it was red on.
#[test]
fn b_a_gate_observed_red_on_the_pushed_sha_is_named_in_the_refusal() {
    let watched = watched();
    let mut rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, Some(HEAD_SHA)))
        .collect();
    rows[2].outcome = CodingSessionObservationGateOutcome::Failed;
    let red_gate = rows[2].gate.to_owned();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Refused(refusal @ VerdictAdmissionRefusal::ObservedGateRed { .. }) => {
            let reason = refusal.reason();
            assert!(reason.contains(&red_gate), "{reason}");
            assert!(reason.contains(HEAD_SHA), "{reason}");
        }
        other => panic!("expected the observed-red refusal, got {other:?}"),
    }
}

/// A green run over a worktree that did not match the commit is not evidence
/// about that commit.
#[test]
fn b_a_dirty_worktree_admits_nothing_however_green_the_rows() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| Row {
            gate,
            outcome: CodingSessionObservationGateOutcome::Passed,
            head_sha: Some(HEAD_SHA),
            dirty: Some(true),
        })
        .collect();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Refused(refusal @ VerdictAdmissionRefusal::ObservedDirty { .. }) => {
            assert!(refusal.reason().contains(HEAD_SHA), "{}", refusal.reason());
        }
        other => panic!("expected the observed-dirty refusal, got {other:?}"),
    }
}

/// A required gate nobody ran is named, and so is the whole required list.
#[test]
fn b_a_required_gate_that_was_never_observed_is_named_with_the_list() {
    let watched = watched();
    let rows = vec![row(DEFAULT_REQUIRED_GATES[0], Some(HEAD_SHA))];
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Refused(
            refusal @ VerdictAdmissionRefusal::RequiredGateNotObserved { .. },
        ) => {
            let reason = refusal.reason();
            assert!(reason.contains(DEFAULT_REQUIRED_GATES[1]), "{reason}");
            for gate in DEFAULT_REQUIRED_GATES {
                assert!(
                    reason.contains(gate),
                    "the required list is named: {reason}"
                );
            }
        }
        other => panic!("expected the required-gate-missing refusal, got {other:?}"),
    }
}

/// `gates.verifierRequired: true` turns arm (B) off. The founder asked for a
/// second seat, and green gates are not one.
#[test]
fn b_is_off_when_the_mission_policy_requires_a_verifier() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, Some(HEAD_SHA)))
        .collect();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let policy = Some(VerdictAdmissionGatePolicy {
        verifier_required: Some(true),
        required_gates: None,
    });
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, policy)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    assert!(
        !outcome.is_admitted(),
        "verifierRequired must not be landable by gate rows, got {outcome:?}"
    );
}

/// The policy's own gate list replaces the default, and is what the refusal
/// names.
#[test]
fn b_the_policys_own_gate_list_is_the_required_one() {
    let watched = watched();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &[row("just ci", Some(HEAD_SHA))],
    )];
    let policy = Some(VerdictAdmissionGatePolicy {
        verifier_required: Some(false),
        required_gates: Some(vec!["just ci".into()]),
    });
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, policy)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::ObservedGates { gates, .. }) => {
            assert_eq!(gates, vec!["just ci".to_owned()]);
        }
        other => panic!("expected an observed-gate admission, got {other:?}"),
    }
}

/// A stranger holding the same patch is not a seat of this mission, and green
/// gates do not seat them.
#[test]
fn b_admits_a_seat_and_not_a_stranger() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, Some(HEAD_SHA)))
        .collect();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let stranger = Keys::generate().public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&stranger, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::PushNotSeated { .. }) => {}
        other => panic!("expected the not-seated refusal, got {other:?}"),
    }
}

/// A row signed before `headSha` existed names no commit, and admits nothing
/// — it is not read as "the commit being pushed".
///
/// Finding 31's rule has two halves: the old row still **decodes**, and it
/// still **admits nothing**. A reader that treated absent as "whatever is
/// being pushed" would land every commit on the strength of a row that
/// predates the question.
#[test]
fn b_a_row_signed_before_head_sha_existed_admits_nothing() {
    let watched = watched();
    let rows: Vec<Row<'_>> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| row(gate, None))
        .collect();
    let events = vec![observation(
        &watched.provider,
        CodingSessionObservationSource::Observed,
        &rows,
    )];
    let founder = watched.founder.public_key().to_hex();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&watched, &events, None)],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    assert!(
        !outcome.is_admitted(),
        "a row naming no commit admits nothing, got {outcome:?}"
    );
}
