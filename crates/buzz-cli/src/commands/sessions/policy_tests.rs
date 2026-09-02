//! `bee sessions policy` — flag translation, the withdrawal record, and the
//! disclosure `get` owes its reader.

use buzz_core::coding_session_policy::{
    CodingSessionAttention, CodingSessionContextTier, CodingSessionIrreversibleAct,
    CodingSessionPosture, CODING_SESSION_POLICY_SCHEMA,
};
use buzz_sdk::coding_session_policy::build_coding_session_policy;
use nostr::Keys;
use serde_json::json;

use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "12".repeat(32)
}

fn args() -> SessionPolicySetArgs {
    SessionPolicySetArgs {
        channel: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis: genesis(),
        posture: None,
        budget_turns: None,
        tokens_per_seat: None,
        tokens_per_session: None,
        cost_usd: None,
        context_tier: None,
        attention: None,
        red_first: None,
        review_every_lane: None,
        required_gate: Vec::new(),
        verifier_required: None,
        bench_identity: Vec::new(),
        bench_provider: Vec::new(),
        challenger_sample_rate: None,
        irreversible: Vec::new(),
        time_box_secs: None,
        on_milestone: None,
    }
}

fn usage(error: CliError) -> String {
    match error {
        CliError::Usage(message) => message,
        other => panic!("expected a usage refusal, got {other:?}"),
    }
}

#[test]
fn every_flag_lands_on_the_field_the_nip_names() {
    let mut set = args();
    set.posture = Some("overnight".into());
    set.budget_turns = Some(240);
    set.tokens_per_seat = Some(4_000_000);
    set.tokens_per_session = Some(40_000_000);
    set.cost_usd = Some(120.5);
    set.context_tier = Some("long".into());
    set.attention = Some("decisions".into());
    set.red_first = Some(true);
    set.review_every_lane = Some(true);
    set.required_gate = vec!["just ci".into(), "just test".into()];
    set.verifier_required = Some(false);
    set.bench_identity = vec!["ab".repeat(32), "cd".repeat(32)];
    set.bench_provider = vec!["claude-primary".into(), "codex-primary".into()];
    set.challenger_sample_rate = Some(0.25);
    set.irreversible = vec!["push".into(), "deploy".into(), "external-message".into()];
    set.time_box_secs = Some(28_800);
    set.on_milestone = Some("the lane lands and CI is green".into());

    let payload = payload_from_args(&set).expect("every flag is valid");
    assert_eq!(payload.schema, CODING_SESSION_POLICY_SCHEMA);
    assert_eq!(payload.posture, Some(CodingSessionPosture::Overnight));
    assert_eq!(payload.attention, Some(CodingSessionAttention::Decisions));
    let budget = payload.budget.clone().expect("budget");
    assert_eq!(budget.turns, Some(240));
    assert_eq!(budget.tokens_per_seat, Some(4_000_000));
    assert_eq!(budget.tokens_per_session, Some(40_000_000));
    assert_eq!(budget.cost_usd_per_session, Some(120.5));
    assert_eq!(budget.context_tier, Some(CodingSessionContextTier::Long));
    let gates = payload.gates.clone().expect("gates");
    assert_eq!(gates.red_first, Some(true));
    assert_eq!(gates.review_every_lane, Some(true));
    assert_eq!(
        gates.required_gates.as_deref(),
        Some(["just ci".to_owned(), "just test".to_owned()].as_slice())
    );
    assert_eq!(gates.verifier_required, Some(false));
    let bench = payload.bench.clone().expect("bench");
    assert_eq!(bench.challenger_sample_rate, Some(0.25));
    assert_eq!(
        payload.irreversible.as_deref(),
        Some(
            [
                CodingSessionIrreversibleAct::Push,
                CodingSessionIrreversibleAct::Deploy,
                CodingSessionIrreversibleAct::ExternalMessage,
            ]
            .as_slice()
        )
    );
    let stop = payload.stop.clone().expect("stop");
    assert_eq!(stop.time_box_secs, Some(28_800));

    // And the whole thing builds into the exact four-tag envelope, so nothing
    // this command can express reaches a signer as bytes the relay refuses.
    let event = build_coding_session_policy(CHANNEL, payload)
        .expect("builder")
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    assert!(
        !event.content.contains("null"),
        "unset keys are omitted, never null: {}",
        event.content
    );
}

/// POLICY.md §2.3: a sub-object that sets nothing is refused by the record, so
/// the CLI must omit it rather than emit `"budget": {}`.
#[test]
fn a_subobject_nobody_set_is_omitted_not_emitted_empty() {
    let mut set = args();
    set.posture = Some("ship".into());
    let payload = payload_from_args(&set).expect("posture alone is a policy");
    assert!(payload.budget.is_none());
    assert!(payload.gates.is_none());
    assert!(payload.bench.is_none());
    assert!(payload.stop.is_none());
    assert!(payload.irreversible.is_none());
    assert!(payload.validate().is_ok());
}

/// `set` with no policy flag would publish the withdrawal record. Withdrawing
/// a policy is a decision somebody made, and it has its own verb.
#[test]
fn a_set_that_sets_nothing_is_refused_and_names_clear() {
    let message = usage(payload_from_args(&args()).unwrap_err());
    assert!(
        message.contains("bee sessions policy clear"),
        "the refusal must name the verb that does mean this: {message}"
    );
}

#[test]
fn a_word_outside_the_closed_vocabulary_is_refused_by_name() {
    let mut set = args();
    set.posture = Some("yolo".into());
    let message = usage(payload_from_args(&set).unwrap_err());
    assert!(
        message.contains("--posture must be one of: spike, ship, investigate, overnight"),
        "{message}"
    );

    let mut bad_act = args();
    bad_act.irreversible = vec!["rm-rf".into()];
    let message = usage(payload_from_args(&bad_act).unwrap_err());
    assert!(
        message.contains("--irreversible must be one of"),
        "{message}"
    );
}

#[test]
fn a_zero_budget_is_refused_before_signing() {
    let mut set = args();
    set.budget_turns = Some(0);
    let message = usage(payload_from_args(&set).unwrap_err());
    assert!(
        !message.is_empty(),
        "a zero ceiling must be named: {message}"
    );
}

// ── `get` folds authority, exactly as the provider does (REVIEW-B2 F1) ──────

fn grant_operator(grantee: &str, accepted_at: u64) -> CodingSessionPolicyGrant {
    CodingSessionPolicyGrant {
        grantee: grantee.to_owned(),
        accepted_at,
        transition_type:
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType::GrantOperator,
    }
}

fn revoke(grantee: &str, accepted_at: u64) -> CodingSessionPolicyGrant {
    CodingSessionPolicyGrant {
        grantee: grantee.to_owned(),
        accepted_at,
        transition_type:
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType::Revoke,
    }
}

fn policy_event(keys: &Keys, turns: Option<u32>, created_at: u64) -> nostr::Event {
    let mut payload = CodingSessionPolicyPayload::empty(SESSION, genesis());
    if let Some(turns) = turns {
        payload.budget = Some(
            buzz_core::coding_session_policy::CodingSessionPolicyBudget {
                turns: Some(turns),
                tokens_per_seat: None,
                tokens_per_session: None,
                cost_usd_per_session: None,
                context_tier: None,
            },
        );
    }
    build_coding_session_policy(CHANNEL, payload)
        .expect("builder")
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign")
}

/// **REVIEW-B2 F1, the reviewer's own case.** A founder record with
/// `turns: 3`, and a *stranger*-signed record with `turns: 9999` published
/// later into the same channel — which the relay stores, by design, because it
/// validates structure and leaves standing to the consumer.
///
/// Before this fix `get` printed the stranger's record, with an author and an
/// event id, as "the newest accepted policy", while the provider — reading the
/// identical two events — folded the founder's. The two surfaces this lane
/// shipped disagreed, and the one a person reads was the wrong one.
#[test]
fn a_strangers_record_is_excluded_and_the_founders_wins() {
    let founder = Keys::parse(&"11".repeat(32)).expect("founder key");
    let stranger = Keys::generate();
    let founder_hex = founder.public_key().to_hex();

    let good = policy_event(&founder, Some(3), 100);
    let bad = policy_event(&stranger, Some(9999), 200);
    let bad_id = bad.id.to_hex();
    let good_id = good.id.to_hex();

    let fold = fold_policies(&[good, bad], SESSION, &genesis(), &founder_hex, &[]);
    let value = policy_json(&fold);

    assert_eq!(value["policy"]["eventId"], Value::String(good_id));
    assert_eq!(value["policy"]["policy"]["budget"]["turns"], json!(3));
    assert_eq!(value["policy"]["authorIsFounder"], Value::Bool(true));
    assert_eq!(value["excluded"].as_array().expect("array").len(), 1);
    assert_eq!(value["excluded"][0]["eventId"], Value::String(bad_id));
    assert_eq!(value["excluded"][0]["code"], json!("unauthorized"));
    assert!(
        value["excluded"][0]["reason"]
            .as_str()
            .expect("reason")
            .contains("could not steer this umbrella"),
        "{}",
        value["excluded"][0]["reason"]
    );
}

/// The exact shape the reviewer reproduced: a stranger's record **alone**.
/// `policy` is `null` and the stranger is disclosed, never promoted and never
/// silently dropped.
#[test]
fn a_strangers_record_alone_prints_null_and_one_excluded_row() {
    let founder = Keys::parse(&"11".repeat(32)).expect("founder key");
    let stranger = Keys::generate();
    let bad = policy_event(&stranger, Some(9999), 200);
    let bad_id = bad.id.to_hex();

    let fold = fold_policies(
        &[bad],
        SESSION,
        &genesis(),
        &founder.public_key().to_hex(),
        &[],
    );
    let value = policy_json(&fold);
    assert_eq!(value["policy"], Value::Null);
    assert_eq!(value["excluded"].as_array().expect("array").len(), 1);
    assert_eq!(value["excluded"][0]["eventId"], Value::String(bad_id));
    assert_eq!(value["excluded"][0]["code"], json!("unauthorized"));
}

/// A granted operator's policy is folded — and the grant is judged **at the
/// record's own time**, so a policy signed before the grant was accepted is
/// not retroactively blessed by it, and one signed while the grant stood is not
/// retroactively invalidated by a later revoke.
#[test]
fn an_operator_grant_is_judged_at_the_records_own_time() {
    let founder = Keys::parse(&"11".repeat(32)).expect("founder key");
    let operator = Keys::generate();
    let founder_hex = founder.public_key().to_hex();
    let operator_hex = operator.public_key().to_hex();
    let grants = vec![
        grant_operator(&operator_hex, 150),
        revoke(&operator_hex, 250),
    ];

    let too_early = policy_event(&operator, Some(7), 100);
    let too_early_id = too_early.id.to_hex();
    let fold = fold_policies(&[too_early], SESSION, &genesis(), &founder_hex, &grants);
    assert!(fold.selected.is_none(), "a policy signed before the grant");
    assert_eq!(fold.excluded[0].event_id, too_early_id);

    let in_window = policy_event(&operator, Some(8), 200);
    let in_window_id = in_window.id.to_hex();
    let fold = fold_policies(&[in_window], SESSION, &genesis(), &founder_hex, &grants);
    assert_eq!(
        fold.selected.as_ref().map(|item| item.event_id.as_str()),
        Some(in_window_id.as_str())
    );
    assert!(!fold.selected.expect("selected").author_is_founder);

    let after_revoke = policy_event(&operator, Some(9), 300);
    let fold = fold_policies(&[after_revoke], SESSION, &genesis(), &founder_hex, &grants);
    assert!(fold.selected.is_none(), "a policy signed after the revoke");
}

/// `get` prints `null` for an umbrella nobody set a policy for — never an empty
/// object, which a reader could take for "a policy that sets nothing".
#[test]
fn get_prints_null_when_nobody_set_a_policy() {
    let founder = Keys::parse(&"11".repeat(32)).expect("founder key");
    let value = policy_json(&fold_policies(
        &[],
        SESSION,
        &genesis(),
        &founder.public_key().to_hex(),
        &[],
    ));
    assert_eq!(value["policy"], Value::Null);
    assert_eq!(value["excluded"], Value::Array(Vec::new()));
    assert_eq!(value["enforcement"], POLICY_ENFORCEMENT_DISCLOSURE);
}

#[test]
fn get_returns_the_newest_record_and_calls_a_withdrawal_a_record() {
    let keys = Keys::parse(&"11".repeat(32)).expect("founder key");
    let mut first = CodingSessionPolicyPayload::empty(SESSION, genesis());
    first.posture = Some(CodingSessionPosture::Ship);
    let first_event = build_coding_session_policy(CHANNEL, first)
        .expect("builder")
        .custom_created_at(nostr::Timestamp::from_secs(1_000))
        .sign_with_keys(&keys)
        .expect("sign");
    let withdrawal = CodingSessionPolicyPayload::empty(SESSION, genesis());
    let withdrawal_event = build_coding_session_policy(CHANNEL, withdrawal)
        .expect("builder")
        .custom_created_at(nostr::Timestamp::from_secs(2_000))
        .sign_with_keys(&keys)
        .expect("sign");
    let withdrawal_id = withdrawal_event.id.to_hex();

    let value = policy_json(&fold_policies(
        &[first_event, withdrawal_event],
        SESSION,
        &genesis(),
        &keys.public_key().to_hex(),
        &[],
    ));
    assert_eq!(value["policy"]["eventId"], Value::String(withdrawal_id));
    assert_eq!(value["policy"]["setsAnyPolicy"], Value::Bool(false));
    assert_eq!(value["excluded"], Value::Array(Vec::new()));
}

/// A record the reader cannot decode is a fact the author needs. Dropping it
/// would let a malformed policy masquerade as no policy at all.
#[test]
fn an_unreadable_record_is_listed_rather_than_dropped() {
    let keys = Keys::parse(&"11".repeat(32)).expect("founder key");
    let event = nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_POLICY as u16),
        "{\"schema\":\"buzz-coding-session-policy/v1\"}",
    )
    .sign_with_keys(&keys)
    .expect("sign");
    let event_id = event.id.to_hex();

    let value = policy_json(&fold_policies(
        &[event],
        SESSION,
        &genesis(),
        &keys.public_key().to_hex(),
        &[],
    ));
    assert_eq!(value["policy"], Value::Null);
    assert_eq!(value["excluded"][0]["eventId"], Value::String(event_id));
    assert_eq!(value["excluded"][0]["code"], json!("undecodable"));
}

// ── REVIEW-B2 F2: `set`/`clear` refuse, they do not publish-and-warn ─────────

/// The rule, and only the rule. An earlier cut of this command signed anyway
/// for an active `lead` seat holding no operator grant and disclosed
/// `willNotBind` in the answer — writing a permanent record onto a public relay
/// that no consumer in the repository would act on, with the warning living
/// only in one operator's terminal.
#[test]
fn a_lead_without_an_operator_grant_is_refused_before_signing() {
    let founder = "11".repeat(32);
    let lead = "22".repeat(32);

    let error = refuse_without_policy_standing(&lead, 500, &founder, &[]).unwrap_err();
    let message = usage(error);
    assert!(
        message.contains(&lead),
        "the refusal names the key: {message}"
    );
    assert!(
        message.contains("nor a seat holding an operator grant"),
        "the refusal names the provable rule: {message}"
    );
    assert!(
        message.contains("binding nothing"),
        "the refusal says why it is not merely a warning: {message}"
    );

    // The founder always may.
    assert!(refuse_without_policy_standing(&founder, 500, &founder, &[]).is_ok());
    // So does the same lead once an operator grant is accepted.
    assert!(
        refuse_without_policy_standing(&lead, 500, &founder, &[grant_operator(&lead, 400)]).is_ok()
    );
    // And not before it was accepted.
    assert!(
        refuse_without_policy_standing(&lead, 300, &founder, &[grant_operator(&lead, 400)])
            .is_err()
    );
    // Nor after it is revoked.
    assert!(refuse_without_policy_standing(
        &lead,
        600,
        &founder,
        &[grant_operator(&lead, 400), revoke(&lead, 550)]
    )
    .is_err());
}
