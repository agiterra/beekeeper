//! Who may **commission** an execution — the 2026-09-05 refuter's B1.
//!
//! Finding 90 closed one door and left another open. The provider set was
//! taken from an accepted lifecycle pair, and a pair is accepted when the
//! kind 44224 receipt is signed by the key the kind 44221 command names. That
//! checks the *receipt's* signer and nothing else, so a seat could publish a
//! `session.create` naming **itself** as `providerAuthorityPubkey` — both refs
//! are public — answer it with its own receipt, and become a provider of the
//! mission. Its own `observed` gate rows then survived the fold, and arm (B)
//! landed a push on rows the pusher had signed about itself. The refuter ran
//! exactly that end to end.
//!
//! The rule these cases hold to: a create or a resume commissions a provider
//! only when a key entitled to **steer** the mission signed it, or when it
//! answers an accepted `session.hire` — the host's door, since a host answering
//! a hire signs the create itself.

use super::*;

use crate::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA;
use crate::coding_session_observation::{
    fold_coding_session_observations, CodingSessionObservationFoldContext,
    CodingSessionObservationSource,
};
use crate::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND;
use nostr::{Event, Keys};
use serde_json::{json, Value};

use super::gate_fixture::{bound, default_green, signed_observation, TEST_REPOSITORY};
use super::hardening_tests::{
    create, query, receipt, signed, watched, Watched, CHANNEL, GENESIS, HEAD_SHA, SESSION,
};

/// One kind 44221 `session.hire` for the fixture mission, signed by `keys`.
fn hire(command_id: &str, keys: &Keys) -> Event {
    let content = json!({
        "schema": CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": command_id,
        "action": {
            "type": "session.hire",
            "sessionRef": SESSION,
            "genesisRef": GENESIS,
            "role": "builder",
            "providerInstanceRef": Value::Null,
            "model": Value::Null,
            "brief": "land the slice",
            "requestedBy": keys.public_key().to_hex(),
        },
    });
    signed(
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        content.to_string(),
        vec![
            ["h", CHANNEL],
            ["csl-v", "csl1-1"],
            ["csl-command", command_id],
        ],
        keys,
        90,
    )
}

/// A `session.create` naming `provider`, with `hireRef` set to `hire_id`.
fn create_answering(command_id: &str, hire_id: &str, provider: &Keys, keys: &Keys) -> Event {
    let content = json!({
        "schema": CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": command_id,
        "action": {
            "type": "session.create",
            "projectRef": Value::Null,
            "repoRef": Value::Null,
            "sessionRef": SESSION,
            "genesisRef": GENESIS,
            "providerInstanceRef": "claude-primary",
            "providerAuthorityPubkey": provider.public_key().to_hex(),
            "model": Value::Null,
            "title": Value::Null,
            "initialTurn": Value::Null,
            "hireRef": hire_id,
        },
    });
    signed(
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        content.to_string(),
        vec![
            ["h", CHANNEL],
            ["csl-v", "csl1-1"],
            ["csl-command", command_id],
        ],
        keys,
        100,
    )
}

/// Every default gate green on `HEAD_SHA`, signed by `signer` and folded with
/// `providers` as the mission's provider set.
fn rows_signed_by(
    signer: &Keys,
    providers: Vec<String>,
) -> Vec<crate::coding_session_observation::CodingSessionObservationGateEntry> {
    let event = signed_observation(
        signer,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    );
    fold_coding_session_observations(
        &[event],
        &CodingSessionObservationFoldContext {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            known_assignment_refs: Vec::new(),
            provider_pubkeys: Some(providers),
        },
    )
    .gates
}

/// The candidate `watched` builds, over rows `signer` published and a provider
/// set resolved from `commands`/`receipts` under `commissioners`.
fn candidate_over(
    watched: &Watched,
    signer: &Keys,
    commissioners: &[String],
    commands: &[Event],
    receipts: &[Event],
) -> (Vec<String>, VerdictAdmissionCandidate) {
    let providers = mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        commissioners,
        commands,
        receipts,
    );
    let candidate = VerdictAdmissionCandidate {
        observed_gates: rows_signed_by(signer, providers.clone()),
        ..watched.candidate(GatePolicyResolution::Absent, bound())
    };
    (providers, candidate)
}

/// The refuter's executed counterexample, at the rule: a seat signs its own
/// create naming **itself** as the provider, signs the receipt that answers
/// it, and signs its own green `observed` rows. It commissions nobody, the
/// rows fold to `declared`, and arm (B) refuses.
#[test]
fn b1_a_seat_cannot_commission_itself_as_its_own_missions_provider() {
    let watched = watched();
    let seat = &watched.builder;
    let commands = vec![create("create-1", GENESIS, seat, seat)];
    let receipts = vec![receipt("create-1", "created", "session-1", 1, seat)];
    let (providers, candidate) = candidate_over(
        &watched,
        seat,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    assert!(
        providers.is_empty(),
        "a create nobody entitled to steer signed names no provider, got {providers:?}"
    );
    let founders = watched.founders();
    let pusher = seat.public_key().to_hex();
    let outcome =
        evaluate_verdict_admission(&[candidate], &query(&pusher, &founders, TEST_REPOSITORY));
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::ObservedRowsAreDeclared { .. }) => {}
        other => panic!("expected the declared-not-observed refusal, got {other:?}"),
    }
}

/// The same commit, the same rows, the same seat — and the founder signed the
/// create. Now the seat *is* the provider, the rows are `observed`, and arm
/// (B) admits. The refusal above is about authority, not about the rows.
#[test]
fn b1_the_founder_signed_create_commissions_the_same_provider() {
    let watched = watched();
    let seat = &watched.builder;
    let commands = vec![create("create-1", GENESIS, seat, &watched.founder)];
    let receipts = vec![receipt("create-1", "created", "session-1", 1, seat)];
    let (providers, candidate) = candidate_over(
        &watched,
        seat,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    assert_eq!(providers, vec![seat.public_key().to_hex()]);
    let founders = watched.founders();
    let pusher = seat.public_key().to_hex();
    let outcome =
        evaluate_verdict_admission(&[candidate], &query(&pusher, &founders, TEST_REPOSITORY));
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::ObservedGates {
            session_ref, ..
        }) => assert_eq!(session_ref, SESSION),
        other => panic!("expected an observed-gate admission, got {other:?}"),
    }
}

/// The host's door: the founder asks for a seat with a `session.hire`, and the
/// **host** signs the create that answers it. The host is not a steering key,
/// so only the `hireRef` makes that create count — and a create answering a
/// hire nobody entitled to steer signed still counts for nothing.
#[test]
fn b1_a_create_answering_an_accepted_hire_commissions_its_provider() {
    let watched = watched();
    let host = Keys::generate();
    let asked = hire("hire-1", &watched.founder);
    let commands = vec![
        asked.clone(),
        create_answering("create-1", &asked.id.to_hex(), &host, &host),
    ];
    let receipts = vec![receipt("create-1", "created", "session-1", 1, &host)];
    let providers = mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    assert_eq!(providers, vec![host.public_key().to_hex()]);

    // The same create, answering a hire the *seat* signed: the hire authorizes
    // nothing, so neither does the create that names it.
    let self_asked = hire("hire-2", &watched.builder);
    let commands = vec![
        self_asked.clone(),
        create_answering("create-2", &self_asked.id.to_hex(), &host, &host),
    ];
    let receipts = vec![receipt("create-2", "created", "session-2", 1, &host)];
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    )
    .is_empty());

    // And a `hireRef` naming a hire that is not on this page resolves to
    // nothing rather than to "accepted".
    let commands = vec![create_answering(
        "create-3",
        &asked.id.to_hex(),
        &host,
        &host,
    )];
    let receipts = vec![receipt("create-3", "created", "session-3", 1, &host)];
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    )
    .is_empty());
}

/// An operator the mission's own authority chain granted may commission, and a
/// key nobody granted may not — the set is the caller's to resolve, and an
/// empty set commissions nothing at all.
#[test]
fn b1_an_operator_may_commission_and_an_empty_set_commissions_nothing() {
    let watched = watched();
    let operator = Keys::generate();
    let commands = vec![create("create-1", GENESIS, &watched.provider, &operator)];
    let receipts = vec![receipt(
        "create-1",
        "created",
        "session-1",
        1,
        &watched.provider,
    )];
    let steering = vec![
        watched.founder.public_key().to_hex(),
        operator.public_key().to_hex(),
    ];
    assert_eq!(
        mission_provider_pubkeys_from_lifecycle(SESSION, GENESIS, &steering, &commands, &receipts),
        vec![watched.provider.public_key().to_hex()]
    );
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts
    )
    .is_empty());
    assert!(
        mission_provider_pubkeys_from_lifecycle(SESSION, GENESIS, &[], &commands, &receipts)
            .is_empty(),
        "a caller that could not resolve who steers resolves no provider either"
    );
}
