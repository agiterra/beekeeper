//! Findings 89, 90 and 91 — the 2026-09-05 admission audit, at the rule.
//!
//! Three defects, each a way a partial or self-authored record acquired
//! authority: a policy the page missed (or a withdrawal read past) opened
//! arm (B); a key that published metadata became a provider; a seat and
//! green rows on one repository admitted a push to another of the same
//! founder. Every case here is written against signed events through the
//! real decoders, so the resolution and the provider rule are exercised
//! rather than assumed.
//!
//! A sibling of [`super::observed_tests`] for the same reason that one is a
//! sibling of [`super::tests`]: the repository ceiling is 1,000 lines.

use super::*;

use crate::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA;
use crate::coding_session_observation::{
    fold_coding_session_observations, CodingSessionObservationFoldContext,
    CodingSessionObservationSource,
};
use crate::coding_session_payload::{
    Capabilities, CONTEXT_NOT_RECOVERED, LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA,
};
use crate::coding_session_policy::CODING_SESSION_POLICY_SCHEMA;
use crate::coding_session_team_transaction::CodingSessionTeamActiveSeat;
use crate::kind::{
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_POLICY,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use super::gate_fixture::{bound, default_green, folded, signed_observation, TEST_REPOSITORY};

pub(super) const CHANNEL: &str = "c0066ddd-8214-4baf-81d2-3046fead0d32";
pub(super) const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
pub(super) const GENESIS: &str = "c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79";
const OTHER_GENESIS: &str = "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";
pub(super) const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";
const OTHER_REPOSITORY: &str =
    "30617:f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0:other-repo";

// ── event builders ──────────────────────────────────────────────────────

pub(super) fn signed(
    kind: u32,
    content: String,
    tags: Vec<[&str; 2]>,
    keys: &Keys,
    at: u64,
) -> Event {
    EventBuilder::new(Kind::Custom(kind as u16), content)
        .tags(
            tags.into_iter()
                .map(|parts| Tag::parse(parts).expect("tag parses")),
        )
        .custom_created_at(Timestamp::from_secs(at))
        .sign_with_keys(keys)
        .expect("event signs")
}

/// One kind 44245 policy record for the fixture mission.
fn policy(body: Value, keys: &Keys, at: u64) -> Event {
    let mut content = json!({
        "schema": CODING_SESSION_POLICY_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": GENESIS,
    });
    if let (Some(target), Some(extra)) = (content.as_object_mut(), body.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    signed(
        KIND_CODING_SESSION_POLICY,
        content.to_string(),
        vec![
            ["h", CHANNEL],
            ["d", SESSION],
            ["csp-v", CODING_SESSION_POLICY_SCHEMA],
            ["csp-genesis", GENESIS],
        ],
        keys,
        at,
    )
}

fn verifier_required() -> Value {
    json!({ "gates": { "verifierRequired": true } })
}

fn target(session_id: &str, generation: u64) -> Value {
    json!({
        "driver": "claude-acp",
        "instanceId": "instance-1",
        "sessionId": session_id,
        "generation": generation,
    })
}

/// One kind 44221 `session.create` naming `provider` for `genesis`.
pub(super) fn create(command_id: &str, genesis: &str, provider: &Keys, keys: &Keys) -> Event {
    let content = json!({
        "schema": CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": command_id,
        "action": {
            "type": "session.create",
            "projectRef": Value::Null,
            "repoRef": Value::Null,
            "sessionRef": SESSION,
            "genesisRef": genesis,
            "providerInstanceRef": "claude-primary",
            "providerAuthorityPubkey": provider.public_key().to_hex(),
            "model": Value::Null,
            "title": Value::Null,
            "initialTurn": Value::Null,
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

/// One kind 44221 `session.resume` of `session_id` at `generation`, naming
/// `provider`.
fn resume(
    command_id: &str,
    session_id: &str,
    generation: u64,
    provider: &Keys,
    keys: &Keys,
) -> Event {
    next_generation(
        "session.resume",
        command_id,
        session_id,
        generation,
        provider,
        keys,
    )
}

/// A resume-shaped command of `action_type` (`session.resume` or
/// `session.restart`, which mint the next generation alike).
fn next_generation(
    action_type: &str,
    command_id: &str,
    session_id: &str,
    generation: u64,
    provider: &Keys,
    keys: &Keys,
) -> Event {
    let content = json!({
        "schema": CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": command_id,
        "action": {
            "type": action_type,
            "session": target(session_id, generation),
            "providerAuthorityPubkey": provider.public_key().to_hex(),
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
        110,
    )
}

/// One kind 44224 receipt answering `command_id` with `status`, signed by
/// `signer`, in the exact shape the decoder requires of that status: a
/// `failed` names no session and carries an error, a
/// `resumed_without_context` names its session and carries
/// `CONTEXT_NOT_RECOVERED`, and the clean outcomes carry no error.
pub(super) fn receipt(
    command_id: &str,
    status: &str,
    session_id: &str,
    generation: u64,
    signer: &Keys,
) -> Event {
    let (session, error) = match status {
        "failed" => (
            Value::Null,
            json!({ "code": "PROVIDER_DOWN", "message": "the provider refused" }),
        ),
        "resumed_without_context" => (
            target(session_id, generation),
            json!({ "code": CONTEXT_NOT_RECOVERED, "message": "fresh context" }),
        ),
        _ => (target(session_id, generation), Value::Null),
    };
    let content = json!({
        "schema": LIFECYCLE_RECEIPT_SCHEMA,
        "commandId": command_id,
        "status": status,
        "session": session,
        "error": error,
    });
    let key = format!(
        "coding-session-lifecycle-receipt/v1|{}:{}",
        command_id.len(),
        command_id
    );
    signed(
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        content.to_string(),
        vec![
            ["h", CHANNEL],
            ["cslr-v", "cslr1-1"],
            ["csl-command", command_id],
            ["csl-key", key.as_str()],
        ],
        signer,
        120,
    )
}

/// One kind 44223 metadata event naming the fixture mission, as any channel
/// member could publish it.
fn metadata(signer: &Keys) -> Event {
    let content = json!({
        "schema": METADATA_SCHEMA,
        "session": target("session-1", 1),
        "projectRef": Value::Null,
        "repoRef": Value::Null,
        "title": Value::Null,
        "agentRef": Value::Null,
        "provider": "claude-primary",
        "runtime": "claude",
        "model": Value::Null,
        "status": "idle",
        "branch": Value::Null,
        "capabilities": Capabilities::v1_claude(),
        "sessionRef": SESSION,
    });
    signed(
        KIND_CODING_SESSION_METADATA,
        content.to_string(),
        vec![["h", CHANNEL], ["d", SESSION]],
        signer,
        130,
    )
}

// ── the mission under test ──────────────────────────────────────────────

pub(super) struct Watched {
    pub(super) founder: Keys,
    pub(super) provider: Keys,
    pub(super) builder: Keys,
}

pub(super) fn watched() -> Watched {
    Watched {
        founder: Keys::generate(),
        provider: Keys::generate(),
        builder: Keys::generate(),
    }
}

impl Watched {
    fn seats(&self) -> Vec<CodingSessionTeamActiveSeat> {
        vec![CodingSessionTeamActiveSeat {
            actor_pubkey: self.builder.public_key().to_hex(),
            role: "builder".into(),
        }]
    }

    /// Every default gate green on `HEAD_SHA`, signed by the provider and
    /// folded with the provider set supplied.
    pub(super) fn green_rows(
        &self,
    ) -> Vec<crate::coding_session_observation::CodingSessionObservationGateEntry> {
        let event = signed_observation(
            &self.provider,
            CHANNEL,
            SESSION,
            GENESIS,
            CodingSessionObservationSource::Observed,
            default_green(HEAD_SHA),
        );
        folded(&self.provider, SESSION, GENESIS, &[event])
    }

    pub(super) fn candidate(
        &self,
        gate_policy: GatePolicyResolution,
        bound_repositories: Vec<String>,
    ) -> VerdictAdmissionCandidate {
        VerdictAdmissionCandidate {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            founder_pubkey: self.founder.public_key().to_hex(),
            canonical: Vec::new(),
            active_seats: self.seats(),
            observed_gates: self.green_rows(),
            gate_policy,
            bound_repositories,
            excluded_unauthorized_policies: 0,
        }
    }

    pub(super) fn founders(&self) -> Vec<String> {
        vec![self.founder.public_key().to_hex()]
    }

    /// Who may commission an execution of this mission: the founder alone,
    /// this fixture granting nobody `operator` (2026-09-05 refuter, B1).
    pub(super) fn commissioners(&self) -> Vec<String> {
        vec![self.founder.public_key().to_hex()]
    }
}

pub(super) fn query<'a>(
    pusher: &'a str,
    founders: &'a [String],
    repository: &'a str,
) -> VerdictAdmissionQuery<'a> {
    VerdictAdmissionQuery {
        ref_name: "refs/heads/whoami/cli",
        new_oid: HEAD_SHA,
        pusher_pubkey: pusher,
        repo_founders: founders,
        candidate_source: &super::VERDICT_ADMISSION_BOUND_CHANNEL,
        repository,
    }
}

fn observed_gates_policy(outcome: VerdictAdmission) -> VerdictAdmissionPolicyEvidence {
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::ObservedGates { policy, .. }) => {
            policy
        }
        other => panic!("expected an observed-gate admission, got {other:?}"),
    }
}

// ── finding 89: policy resolution ───────────────────────────────────────

/// No record names the mission: `Absent`, and the admission says so rather
/// than implying the founder chose the defaults.
#[test]
fn f89_no_policy_resolves_absent_and_the_admission_discloses_it() {
    let watched = watched();
    let resolution = resolve_mission_gate_policy(SESSION, GENESIS, &[]);
    assert_eq!(resolution, GatePolicyResolution::Absent);
    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(resolution, bound())],
        &query(&pusher, &founders, TEST_REPOSITORY),
    );
    let policy = observed_gates_policy(outcome);
    assert_eq!(policy.event_id, None);
    assert_eq!(policy.resolution, VerdictAdmissionPolicyResolution::Absent);
    assert_eq!(policy.resolution.as_str(), "absent");
}

/// A record filed under another genesis is another mission's, whatever its
/// `d` says — the both-tags rule, and the page-omission case at the rule:
/// a page that carries no record for *this* mission is `Absent`, never a
/// neighbour's policy.
#[test]
fn f89_a_policy_under_another_genesis_is_not_this_missions() {
    let founder = Keys::generate();
    let mut foreign = policy(verifier_required(), &founder, 500);
    foreign = EventBuilder::new(foreign.kind, foreign.content.clone())
        .tags([
            Tag::parse(["h", CHANNEL]).expect("tag"),
            Tag::parse(["d", SESSION]).expect("tag"),
            Tag::parse(["csp-v", CODING_SESSION_POLICY_SCHEMA]).expect("tag"),
            Tag::parse(["csp-genesis", OTHER_GENESIS]).expect("tag"),
        ])
        .sign_with_keys(&founder)
        .expect("signs");
    assert_eq!(
        resolve_mission_gate_policy(SESSION, GENESIS, &[foreign]),
        GatePolicyResolution::Absent
    );
}

/// The audit's "authorized withdrawal": an older restrictive policy followed
/// by a valid empty withdrawal. The withdrawal is the policy; the old gates
/// are not resurrected, and the admission names the withdrawal it stood on.
#[test]
fn f89_a_withdrawal_does_not_resurrect_the_older_restrictive_policy() {
    let watched = watched();
    let restrictive = policy(verifier_required(), &watched.founder, 500);
    let withdrawal = policy(json!({}), &watched.founder, 600);
    // Page order is the caller's; newest-first is what the relay hands over.
    let resolution =
        resolve_mission_gate_policy(SESSION, GENESIS, &[withdrawal.clone(), restrictive.clone()]);
    assert_eq!(
        resolution,
        GatePolicyResolution::Withdrawn {
            event_id: withdrawal.id.to_hex()
        }
    );
    assert!(!resolution.requires_a_verifier());
    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(resolution, bound())],
        &query(&pusher, &founders, TEST_REPOSITORY),
    );
    let evidence = observed_gates_policy(outcome);
    assert_eq!(evidence.event_id, Some(withdrawal.id.to_hex()));
    assert_eq!(
        evidence.resolution,
        VerdictAdmissionPolicyResolution::Withdrawn
    );

    // And the other way round: the restrictive policy is the newest, the
    // withdrawal is history, and arm (B) is closed.
    let restrictive_again = policy(verifier_required(), &watched.founder, 700);
    let resolution =
        resolve_mission_gate_policy(SESSION, GENESIS, &[restrictive_again.clone(), withdrawal]);
    assert!(resolution.requires_a_verifier(), "{resolution:?}");
    assert_eq!(
        resolution.evidence(0).map(|it| it.event_id),
        Some(Some(restrictive_again.id.to_hex()))
    );
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(resolution, bound())],
        &query(&pusher, &founders, TEST_REPOSITORY),
    );
    assert!(!outcome.is_admitted(), "{outcome:?}");
}

/// The audit's "unreadable authoritative policy": the newest record carries
/// a key this build does not know. The answer is `Unreadable` — not the
/// older, weaker policy — and the push is refused by name.
#[test]
fn f89_an_unreadable_newest_record_refuses_rather_than_reading_the_older_one() {
    let watched = watched();
    let permissive = policy(
        json!({ "gates": { "verifierRequired": false } }),
        &watched.founder,
        500,
    );
    let unreadable = policy(
        json!({ "noPushWithoutReview": true }),
        &watched.founder,
        600,
    );
    let resolution =
        resolve_mission_gate_policy(SESSION, GENESIS, &[permissive, unreadable.clone()]);
    let (event_id, reason) = resolution.unreadable().expect("unreadable");
    assert_eq!(event_id, unreadable.id.to_hex());
    assert!(reason.contains("noPushWithoutReview"), "{reason}");
    assert_eq!(resolution.evidence(0), None);

    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(resolution.clone(), bound())],
        &query(&pusher, &founders, TEST_REPOSITORY),
    );
    match outcome {
        VerdictAdmission::Refused(refusal @ VerdictAdmissionRefusal::PolicyUnreadable { .. }) => {
            let sentence = refusal.reason();
            assert!(sentence.contains(&unreadable.id.to_hex()), "{sentence}");
            assert!(sentence.contains(SESSION), "{sentence}");
            assert!(sentence.contains("noPushWithoutReview"), "{sentence}");
        }
        other => panic!("expected the policy-unreadable refusal, got {other:?}"),
    }

    // Arm (C)'s half of the same rule: the rows are judged against the
    // policy's gate list, and a policy nobody read has none to judge by.
    let candidate = watched.candidate(resolution, bound());
    match gate_rows_status(&candidate, HEAD_SHA) {
        VerdictAdmissionGateRows::Short(short) => {
            assert!(matches!(
                *short,
                VerdictAdmissionRefusal::PolicyUnreadable { .. }
            ));
        }
        VerdictAdmissionGateRows::Green => panic!("green rows under an unreadable policy"),
    }
    assert!(matches!(
        gates_observed_green(&candidate, HEAD_SHA, &pusher),
        Err(VerdictAdmissionRefusal::PolicyUnreadable { .. })
    ));
}

/// Ties on `created_at` go to the larger event id, as the desktop fold's
/// `isNewer` breaks them — so two readers of one page choose one record.
#[test]
fn f89_the_newest_record_is_chosen_by_created_at_then_by_id() {
    let founder = Keys::generate();
    let one = policy(verifier_required(), &founder, 500);
    let two = policy(json!({}), &founder, 500);
    let winner = if one.id.to_hex() > two.id.to_hex() {
        &one
    } else {
        &two
    };
    let resolution = resolve_mission_gate_policy(SESSION, GENESIS, &[one.clone(), two.clone()]);
    let chosen = match &resolution {
        GatePolicyResolution::Present { event_id, .. }
        | GatePolicyResolution::Withdrawn { event_id } => event_id.clone(),
        other => panic!("expected a readable record, got {other:?}"),
    };
    assert_eq!(chosen, winner.id.to_hex());
    let later = policy(json!({}), &founder, 501);
    let resolution = resolve_mission_gate_policy(SESSION, GENESIS, &[one, later.clone(), two]);
    assert_eq!(
        resolution,
        GatePolicyResolution::Withdrawn {
            event_id: later.id.to_hex()
        }
    );
}

/// A record that sets a budget and no gate half is a present policy whose
/// gate half is unset: defaults, disclosed as `present` under its own id.
#[test]
fn f89_a_policy_with_no_gate_half_is_present_with_the_defaults() {
    let founder = Keys::generate();
    let budget_only = policy(json!({ "budget": { "turns": 3 } }), &founder, 500);
    let resolution =
        resolve_mission_gate_policy(SESSION, GENESIS, std::slice::from_ref(&budget_only));
    assert_eq!(
        resolution,
        GatePolicyResolution::Present {
            policy: VerdictAdmissionGatePolicy::default(),
            event_id: budget_only.id.to_hex(),
        }
    );
    assert_eq!(resolution.required_gates(), DEFAULT_REQUIRED_GATES.to_vec());
}

// ── finding 90: providers are proven by the lifecycle ───────────────────

/// The accepted create names the provider, and its receipt — signed by that
/// provider — confirms it. A key that merely published metadata is not in
/// the set, though it is in the deprecated metadata set.
#[test]
fn f90_the_lifecycle_names_the_provider_and_metadata_does_not() {
    let watched = watched();
    let stranger = Keys::generate();
    let commands = vec![create(
        "create-1",
        GENESIS,
        &watched.provider,
        &watched.founder,
    )];
    let receipts = vec![receipt(
        "create-1",
        "created",
        "session-1",
        1,
        &watched.provider,
    )];
    let providers = mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    assert_eq!(providers, vec![watched.provider.public_key().to_hex()]);

    #[allow(deprecated)]
    let signers = metadata_signers(SESSION, &[metadata(&stranger), metadata(&watched.provider)]);
    assert!(signers.contains(&stranger.public_key().to_hex()));
    assert!(!providers.contains(&stranger.public_key().to_hex()));
}

/// A receipt signed by anyone but the named provider confirms nothing; nor
/// does a `failed` receipt; nor does a create for another genesis.
#[test]
fn f90_a_receipt_by_another_key_a_failure_or_another_genesis_proves_nothing() {
    let watched = watched();
    let impostor = Keys::generate();
    let commands = vec![
        create("create-1", GENESIS, &watched.provider, &watched.founder),
        create("create-2", GENESIS, &watched.provider, &watched.founder),
        create(
            "create-3",
            OTHER_GENESIS,
            &watched.provider,
            &watched.founder,
        ),
    ];
    let receipts = vec![
        receipt("create-1", "created", "session-1", 1, &impostor),
        receipt("create-2", "failed", "session-2", 1, &watched.provider),
        receipt("create-3", "created", "session-3", 1, &watched.provider),
    ];
    for event in &receipts {
        crate::coding_session_payload::decode_coding_session_lifecycle_receipt(&event.content)
            .expect("every fixture receipt decodes, so each case fails for its own reason");
    }
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    )
    .is_empty());
    // And with no receipts at all, an unanswered create names nobody.
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &[]
    )
    .is_empty());
}

/// Two receipts for one command, or two commands under one id, are ambiguous
/// and accepted as nothing — the desktop fold's rule.
#[test]
fn f90_an_ambiguous_pairing_proves_nothing() {
    let watched = watched();
    let commands = vec![create(
        "create-1",
        GENESIS,
        &watched.provider,
        &watched.founder,
    )];
    let receipts = vec![
        receipt("create-1", "created", "session-1", 1, &watched.provider),
        receipt("create-1", "created", "session-1", 1, &watched.provider),
    ];
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    )
    .is_empty());
    let commands = vec![
        create("create-1", GENESIS, &watched.provider, &watched.founder),
        create("create-1", GENESIS, &Keys::generate(), &watched.founder),
    ];
    let receipts = vec![receipt(
        "create-1",
        "created",
        "session-1",
        1,
        &watched.provider,
    )];
    assert!(mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    )
    .is_empty());
}

/// A resume extends the set only along the accepted chain: from the created
/// execution at its accepted generation to the next one, confirmed by the
/// resume's own named provider. A resume of an execution nobody created for
/// this mission adds nobody.
#[test]
fn f90_a_resume_extends_the_provider_set_only_along_the_accepted_chain() {
    let watched = watched();
    let second = Keys::generate();
    let outsider = Keys::generate();
    let commands = vec![
        create("create-1", GENESIS, &watched.provider, &watched.founder),
        resume("resume-1", "session-1", 1, &second, &watched.founder),
        resume("resume-2", "session-1", 2, &second, &watched.founder),
        // An execution this mission never created.
        resume("resume-x", "session-x", 1, &outsider, &watched.founder),
        // The right execution at the wrong generation.
        resume("resume-y", "session-1", 7, &outsider, &watched.founder),
    ];
    let receipts = vec![
        receipt("create-1", "created", "session-1", 1, &watched.provider),
        // Page order is deliberately not chain order.
        receipt("resume-2", "resumed", "session-1", 3, &second),
        receipt(
            "resume-1",
            "resumed_without_context",
            "session-1",
            2,
            &second,
        ),
        receipt("resume-x", "resumed", "session-x", 2, &outsider),
        receipt("resume-y", "resumed", "session-1", 8, &outsider),
    ];
    let providers = mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    assert_eq!(
        providers,
        vec![
            watched.provider.public_key().to_hex(),
            second.public_key().to_hex()
        ]
    );
}

/// A `session.restart` mints the next generation exactly as a resume does,
/// so the chain walks through it: a resume of the restarted generation 2
/// still extends the set. A restart from a key that may not steer adds
/// nobody, the same commissioning rule a resume follows.
#[test]
fn a_restart_extends_the_provider_set_like_a_resume() {
    let watched = watched();
    let second = Keys::generate();
    let third = Keys::generate();
    let stranger = Keys::generate();
    let commands = vec![
        create("create-1", GENESIS, &watched.provider, &watched.founder),
        next_generation(
            "session.restart",
            "restart-1",
            "session-1",
            1,
            &second,
            &watched.founder,
        ),
        resume("resume-2", "session-1", 2, &third, &watched.founder),
        next_generation(
            "session.restart",
            "restart-x",
            "session-1",
            3,
            &stranger,
            &stranger,
        ),
    ];
    let receipts = vec![
        receipt("create-1", "created", "session-1", 1, &watched.provider),
        receipt("restart-1", "resumed", "session-1", 2, &second),
        receipt("resume-2", "resumed", "session-1", 3, &third),
        receipt("restart-x", "resumed", "session-1", 4, &stranger),
    ];
    let providers = mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    assert_eq!(
        providers,
        vec![
            watched.provider.public_key().to_hex(),
            second.public_key().to_hex(),
            third.public_key().to_hex(),
        ]
    );
}

/// The audit's "provider self-certification", end to end: an ordinary
/// channel writer publishes metadata naming the mission and self-signed
/// `observed` green rows. With the provider set taken from the lifecycle,
/// the fold keeps those rows `declared` and arm (B) refuses.
#[test]
fn f90_a_metadata_author_gains_no_observer_authority() {
    let watched = watched();
    let commands = vec![create(
        "create-1",
        GENESIS,
        &watched.provider,
        &watched.founder,
    )];
    let receipts = vec![receipt(
        "create-1",
        "created",
        "session-1",
        1,
        &watched.provider,
    )];
    let providers = mission_provider_pubkeys_from_lifecycle(
        SESSION,
        GENESIS,
        &watched.commissioners(),
        &commands,
        &receipts,
    );
    // The builder publishes metadata for M, then "observes" its own gates.
    let _claim = metadata(&watched.builder);
    let rows = signed_observation(
        &watched.builder,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    );
    let fold = fold_coding_session_observations(
        &[rows],
        &CodingSessionObservationFoldContext {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            known_assignment_refs: Vec::new(),
            provider_pubkeys: Some(providers),
        },
    );
    let candidate = VerdictAdmissionCandidate {
        observed_gates: fold.gates,
        ..watched.candidate(GatePolicyResolution::Absent, bound())
    };
    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let outcome =
        evaluate_verdict_admission(&[candidate], &query(&pusher, &founders, TEST_REPOSITORY));
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::ObservedRowsAreDeclared { .. }) => {}
        other => panic!("expected the declared-not-observed refusal, got {other:?}"),
    }
}

// ── finding 91: the repository binding ──────────────────────────────────

/// The audit's "cross-project proof": one founder, two repositories. A seat
/// and green rows on the mission bound to R1 do not admit the same commit to
/// R2, and the refusal names the mission and the repository.
#[test]
fn f91_a_mission_bound_to_another_repository_of_the_same_founder_does_not_admit() {
    let watched = watched();
    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(GatePolicyResolution::Absent, bound())],
        &query(&pusher, &founders, OTHER_REPOSITORY),
    );
    match outcome {
        VerdictAdmission::Refused(
            refusal @ VerdictAdmissionRefusal::MissionNotBoundToRepository { .. },
        ) => {
            let sentence = refusal.reason();
            assert!(sentence.contains(SESSION), "{sentence}");
            assert!(sentence.contains(OTHER_REPOSITORY), "{sentence}");
        }
        other => panic!("expected the not-bound refusal, got {other:?}"),
    }
    // Bound to both, the same push admits.
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(
            GatePolicyResolution::Absent,
            vec![TEST_REPOSITORY.to_owned(), OTHER_REPOSITORY.to_owned()],
        )],
        &query(&pusher, &founders, OTHER_REPOSITORY),
    );
    assert!(outcome.is_admitted(), "{outcome:?}");
    // Bound to nothing, it admits nowhere.
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(GatePolicyResolution::Absent, Vec::new())],
        &query(&pusher, &founders, TEST_REPOSITORY),
    );
    assert!(
        matches!(
            outcome,
            VerdictAdmission::Refused(VerdictAdmissionRefusal::MissionNotBoundToRepository { .. })
        ),
        "{outcome:?}"
    );
}

/// The binding is checked before either arm: an unbound mission whose policy
/// is unreadable is refused for the binding, and a bound one for the policy.
#[test]
fn f91_the_binding_is_checked_before_the_policy_and_before_both_arms() {
    let watched = watched();
    let unreadable = GatePolicyResolution::Unreadable {
        event_id: "ab".repeat(32),
        reason: "carries unsupported field".into(),
    };
    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(unreadable.clone(), bound())],
        &query(&pusher, &founders, OTHER_REPOSITORY),
    );
    assert!(
        matches!(
            outcome,
            VerdictAdmission::Refused(VerdictAdmissionRefusal::MissionNotBoundToRepository { .. })
        ),
        "{outcome:?}"
    );
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(unreadable, bound())],
        &query(&pusher, &founders, TEST_REPOSITORY),
    );
    assert!(
        matches!(
            outcome,
            VerdictAdmission::Refused(VerdictAdmissionRefusal::PolicyUnreadable { .. })
        ),
        "{outcome:?}"
    );
}

/// The owner half of a coordinate is hex and case-folds; the `d` half is a
/// name and does not.
#[test]
fn f91_coordinates_fold_the_owner_hex_and_not_the_name() {
    let watched = watched();
    let founders = watched.founders();
    let pusher = watched.builder.public_key().to_hex();
    let upper_owner = TEST_REPOSITORY.replace("f0f0", "F0F0");
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(GatePolicyResolution::Absent, bound())],
        &query(&pusher, &founders, &upper_owner),
    );
    assert!(outcome.is_admitted(), "{outcome:?}");
    let upper_name = TEST_REPOSITORY.replace("whoami-cli", "WHOAMI-CLI");
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(GatePolicyResolution::Absent, bound())],
        &query(&pusher, &founders, &upper_name),
    );
    assert!(!outcome.is_admitted(), "{outcome:?}");
}

// ── arm (A): the founder's receipt ──────────────────────────────────────

/// The audit's "founder audit": a founder's landing records the exception it
/// is — no policy evaluated, `founder_exception` — and can never be read as
/// verifier-approved, whatever the mission's binding or policy.
#[test]
fn founder_push_evidence_discloses_that_no_policy_was_evaluated() {
    let watched = watched();
    let founder = watched.founder.public_key().to_hex();
    let founders = watched.founders();
    let unreadable = GatePolicyResolution::Unreadable {
        event_id: "ab".repeat(32),
        reason: "unreadable".into(),
    };
    let outcome = evaluate_verdict_admission(
        &[watched.candidate(unreadable, Vec::new())],
        &query(&founder, &founders, OTHER_REPOSITORY),
    );
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::FounderPush {
            pusher_pubkey,
            policy_not_evaluated,
        }) => {
            assert_eq!(pusher_pubkey, founder);
            assert_eq!(
                policy_not_evaluated,
                VerdictAdmissionPolicyNotEvaluated::FounderException
            );
            assert_eq!(policy_not_evaluated.as_str(), "founder_exception");
        }
        other => panic!("expected a founder push admission, got {other:?}"),
    }
}

/// Every new sentence names its inputs and never the ref.
#[test]
fn the_new_refusal_sentences_name_their_inputs() {
    let unreadable = VerdictAdmissionRefusal::PolicyUnreadable {
        session_ref: SESSION.into(),
        event_id: "ab".repeat(32),
        reason: "carries unsupported field \"noPushWithoutReview\"".into(),
    }
    .reason();
    for input in [SESSION, &"ab".repeat(32), "noPushWithoutReview"] {
        assert!(unreadable.contains(input), "{unreadable}");
    }
    let unbound = VerdictAdmissionRefusal::MissionNotBoundToRepository {
        session_ref: SESSION.into(),
        repository: OTHER_REPOSITORY.into(),
    }
    .reason();
    for input in [SESSION, OTHER_REPOSITORY] {
        assert!(unbound.contains(input), "{unbound}");
    }
    for sentence in [&unreadable, &unbound] {
        assert!(!sentence.contains("refs/heads"), "{sentence}");
    }
}
