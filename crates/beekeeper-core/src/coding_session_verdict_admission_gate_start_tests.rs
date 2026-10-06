//! SV-41: a provider-signed gate start admits nothing.
//!
//! A start is an observed `gate:<gate>` **phase**, so the fold routes it to
//! `gate_starts` and never to `gates`, the one collection admission reads.
//! These cases pin that from the admission side: an open start with no gate
//! rows is a gate nobody observed, and a start beside green rows changes
//! nothing about the admission they earn.

use super::*;

use crate::coding_session_observation::{
    fold_coding_session_observations, CodingSessionObservationBody,
    CodingSessionObservationFoldContext, CodingSessionObservationPayload,
    CodingSessionObservationPhaseTiming, CodingSessionObservationSource,
    CodingSessionObservationType, CODING_SESSION_OBSERVATION_SCHEMA,
};
use crate::coding_session_team_transaction::CodingSessionTeamActiveSeat;
use crate::kind::KIND_CODING_SESSION_OBSERVATION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "c0066ddd-8214-4baf-81d2-3046fead0d32";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79";
const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";

/// An open observed start for `gate`, as the provider signs it.
fn start(provider: &Keys, gate: &str) -> Event {
    let payload = CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        observation_type: CodingSessionObservationType::Phase,
        source: CodingSessionObservationSource::Observed,
        assignment_ref: None,
        body: CodingSessionObservationBody::Phase(CodingSessionObservationPhaseTiming {
            phase: format!("gate:{gate}"),
            started_at_ms: 1_759_572_120_000,
            ended_at_ms: None,
            duration_ms: None,
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
        Tag::parse(["csob-type", "phase"]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(401))
    .sign_with_keys(provider)
    .expect("event signs")
}

struct Mission {
    founder: Keys,
    provider: Keys,
    builder: Keys,
}

fn mission() -> Mission {
    Mission {
        founder: Keys::generate(),
        provider: Keys::generate(),
        builder: Keys::generate(),
    }
}

fn candidate(mission: &Mission, events: &[Event]) -> VerdictAdmissionCandidate {
    let fold = fold_coding_session_observations(
        events,
        &CodingSessionObservationFoldContext {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            known_assignment_refs: Vec::new(),
            provider_pubkeys: Some(vec![mission.provider.public_key().to_hex()]),
        },
    );
    VerdictAdmissionCandidate {
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: mission.founder.public_key().to_hex(),
        canonical: Vec::new(),
        active_seats: vec![CodingSessionTeamActiveSeat {
            actor_pubkey: mission.builder.public_key().to_hex(),
            role: "builder".into(),
        }],
        observed_gates: fold.gates,
        gate_policy: super::gate_fixture::resolved(None),
        bound_repositories: super::gate_fixture::bound(),
        excluded_unauthorized_policies: 0,
    }
}

fn admit(mission: &Mission, events: &[Event]) -> VerdictAdmission {
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    evaluate_verdict_admission(
        &[candidate(mission, events)],
        &VerdictAdmissionQuery {
            ref_name: "refs/heads/whoami/cli",
            new_oid: HEAD_SHA,
            pusher_pubkey: &pusher,
            repo_founders: std::slice::from_ref(&founder),
            candidate_source: &super::VERDICT_ADMISSION_BOUND_CHANNEL,
            repository: super::gate_fixture::TEST_REPOSITORY,
        },
    )
}

/// One gate green on the head, the other only *started*: the started gate is
/// still a gate nobody observed, and the refusal names it as missing — never
/// as running, never as passed.
#[test]
fn a_gate_start_never_admits_a_verdict() {
    let mission = mission();
    let green_first = super::gate_fixture::signed_observation(
        &mission.provider,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionObservationSource::Observed,
        vec![super::gate_fixture::green_row(
            DEFAULT_REQUIRED_GATES[0],
            HEAD_SHA,
        )],
    );
    let started_rest: Vec<Event> = DEFAULT_REQUIRED_GATES[1..]
        .iter()
        .map(|gate| start(&mission.provider, gate))
        .collect();
    let mut events = vec![green_first];
    events.extend(started_rest);
    match admit(&mission, &events) {
        VerdictAdmission::Refused(
            refusal @ VerdictAdmissionRefusal::RequiredGateNotObserved { .. },
        ) => {
            assert!(
                refusal.reason().contains(DEFAULT_REQUIRED_GATES[1]),
                "{}",
                refusal.reason()
            );
        }
        other => panic!("a start must not count as an observed gate, got {other:?}"),
    }

    // Starts alone are no gate rows at all.
    let only_starts: Vec<Event> = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| start(&mission.provider, gate))
        .collect();
    assert!(
        candidate(&mission, &only_starts).observed_gates.is_empty(),
        "a start never enters the collection admission reads"
    );
    assert!(
        matches!(admit(&mission, &only_starts), VerdictAdmission::Refused(_)),
        "starts with no rows admit nothing"
    );
}

/// The same clean green rows admit with or without a start beside them, and
/// the evidence is identical: the start adds nothing and takes nothing away.
#[test]
fn a_start_beside_green_rows_changes_nothing() {
    let mission = mission();
    let green = super::gate_fixture::signed_observation(
        &mission.provider,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionObservationSource::Observed,
        super::gate_fixture::default_green(HEAD_SHA),
    );
    let without = admit(&mission, std::slice::from_ref(&green));
    let with = admit(
        &mission,
        &[green, start(&mission.provider, DEFAULT_REQUIRED_GATES[0])],
    );
    assert!(
        matches!(without, VerdictAdmission::Admitted(_)),
        "{without:?}"
    );
    assert_eq!(with, without);
}
