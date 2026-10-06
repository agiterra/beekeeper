//! Findings 89, 90 and 91 — the 2026-09-05 admission audit, **in production
//! composition**, against Postgres.
//!
//! `beekeeper_core::coding_session_verdict_admission`'s own hardening tests prove
//! the rule. These prove the half the rule cannot: which events this module
//! actually fetches, from where, and with which filter. Every defect the audit
//! found lived in that half —
//!
//! * a mission's policy read as one page across a whole channel, so a record
//!   outside it resolved to "nobody set a policy" and the default gates ran
//!   ([`a_policy_outside_the_old_page_size_is_still_found`]);
//! * a valid withdrawal skipped for the restrictive record it withdrew
//!   ([`an_authorized_withdrawal_is_effective_and_the_old_gates_do_not_return`]);
//! * an unreadable record read past to a weaker one
//!   ([`an_unreadable_newest_policy_refuses_rather_than_reading_an_older_one`]);
//! * the signer of a kind 44223 promoted into the mission's provider set
//!   ([`a_metadata_only_provider_gains_no_authority`]).
//!
//! The fourth, a mission on another repository admitting a push
//! (finding 91), is refused one step earlier — by the narrowed scope — and
//! lives in [`super::lookup_tests`]. What this file proves about it is that
//! the binding the rule checks is *populated from a statement somebody with
//! standing made*: [`a_strangers_metadata_cannot_widen_a_missions_binding`].

use super::*;

use beekeeper_core::coding_session_observation::CodingSessionObservationSource;
use beekeeper_core::coding_session_policy::{
    CodingSessionPolicyGates, CODING_SESSION_POLICY_SCHEMA,
};
use beekeeper_core::coding_session_verdict_admission::VERDICT_ADMISSION_MAX_POLICIES;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use uuid::Uuid;

use super::observed_tests::{default_green, watched, Watched, HEAD_SHA};

/// Publish one genuine hire for this mission. The admission resolver reads the
/// same stored kind-44221 page production uses, so the regression proves that
/// possessing this public event id cannot authorize a different create signer.
async fn publish_hire(w: &Watched, signer: &Keys) -> nostr::Event {
    let command_id = format!("hire-{}", Uuid::new_v4().simple());
    let content = serde_json::json!({
        "schema": beekeeper_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": command_id,
        "action": {
            "type": "session.hire",
            "sessionRef": w.session_ref,
            "genesisRef": w.genesis_ref,
            "role": "builder",
            "providerInstanceRef": serde_json::Value::Null,
            "model": serde_json::Value::Null,
            "brief": "run the gate",
            "requestedBy": signer.public_key().to_hex(),
        },
    })
    .to_string();
    beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
        &content,
    )
    .expect("hire fixture decodes");
    let event = EventBuilder::new(
        Kind::Custom(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16),
        content,
    )
    .tags([
        Tag::parse(["h", &w.channel_id.to_string()]).expect("h"),
        Tag::parse(["csl-v", "csl1-1"]).expect("v"),
        Tag::parse(["csl-command", &command_id]).expect("command"),
    ])
    .sign_with_keys(signer)
    .expect("sign hire");
    w.state
        .db
        .insert_event(w.community, &event, Some(w.channel_id))
        .await
        .expect("insert hire");
    event
}

/// Publish one kind 44245 record for this mission with a chosen body, signer
/// and time.
///
/// Deliberately not `Watched::set_gate_policy`: these cases are about records
/// that are **not** a well-formed founder-signed gate policy — a withdrawal, a
/// body this build cannot read, a stranger's competing ceiling.
async fn publish_policy(w: &Watched, body: serde_json::Value, signer: &Keys, at: u64) {
    let mut content = serde_json::json!({
        "schema": CODING_SESSION_POLICY_SCHEMA,
        "sessionRef": w.session_ref,
        "genesisRef": w.genesis_ref,
    });
    if let (Some(target), Some(extra)) = (content.as_object_mut(), body.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    let event = EventBuilder::new(
        Kind::Custom(beekeeper_core::kind::KIND_CODING_SESSION_POLICY as u16),
        content.to_string(),
    )
    .tags([
        Tag::parse(["h", &w.channel_id.to_string()]).expect("h"),
        Tag::parse(["d", &w.session_ref]).expect("d"),
        Tag::parse(["csp-v", CODING_SESSION_POLICY_SCHEMA]).expect("v"),
        Tag::parse(["csp-genesis", &w.genesis_ref]).expect("genesis"),
    ])
    .custom_created_at(Timestamp::from_secs(at))
    .sign_with_keys(signer)
    .expect("sign policy");
    w.state
        .db
        .insert_event(w.community, &event, Some(w.channel_id))
        .await
        .expect("insert policy");
}

/// Fill the mission's channel with `count` kind 44245 records belonging to
/// **other** missions, newer than anything this mission published.
///
/// This is the page-omission shape: before finding 89 the gate read one page
/// of 44245 across the channel, so a mission whose own record sat below the
/// bound resolved to `Absent` and was judged with the defaults.
async fn crowd_the_policy_page(w: &Watched, count: usize, at: u64) {
    for index in 0..count {
        let other_session = Uuid::new_v4().to_string();
        let content = serde_json::json!({
            "schema": CODING_SESSION_POLICY_SCHEMA,
            "sessionRef": other_session,
            "genesisRef": w.genesis_ref,
            "gates": { "verifierRequired": false },
        })
        .to_string();
        let event = EventBuilder::new(
            Kind::Custom(beekeeper_core::kind::KIND_CODING_SESSION_POLICY as u16),
            content,
        )
        .tags([
            Tag::parse(["h", &w.channel_id.to_string()]).expect("h"),
            Tag::parse(["d", &other_session]).expect("d"),
            Tag::parse(["csp-v", CODING_SESSION_POLICY_SCHEMA]).expect("v"),
            Tag::parse(["csp-genesis", &w.genesis_ref]).expect("genesis"),
        ])
        .custom_created_at(Timestamp::from_secs(at + index as u64))
        .sign_with_keys(&w.founder)
        .expect("sign policy");
        w.state
            .db
            .insert_event(w.community, &event, Some(w.channel_id))
            .await
            .expect("insert crowding policy");
    }
}

/// **Finding 89, the production half.** The mission requires a verifier, and
/// its policy record is older than a full page of other missions' records on
/// the same channel. One page across the channel would never see it; the
/// per-mission query does, and arm (B) stays shut.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_policy_outside_the_old_page_size_is_still_found() {
    let w = watched().await;
    w.set_gate_policy(CodingSessionPolicyGates {
        red_first: None,
        review_every_lane: None,
        required_gates: None,
        verifier_required: Some(true),
    })
    .await;
    // Strictly more than the bound, all newer, all belonging to other
    // missions: exactly what a shared page would return.
    crowd_the_policy_page(&w, VERDICT_ADMISSION_MAX_POLICIES + 8, 2_000_000_000).await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a verifierRequired policy the page missed must not open arm (B) (body: {body})"
    );
    assert!(
        !body.contains("policy could not be read"),
        "the record is readable; it was merely not on the old page (body: {body})"
    );
}

/// **The audit's "authorized withdrawal".** The founder set a restrictive
/// policy and then took it back with a record that sets nothing (NIP-CSP rule
/// 3). The withdrawal is the newest record and it is what applies — the older
/// restrictive record does not come back.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_authorized_withdrawal_is_effective_and_the_old_gates_do_not_return() {
    let w = watched().await;
    publish_policy(
        &w,
        serde_json::json!({ "gates": { "verifierRequired": true } }),
        &w.founder.clone(),
        1_900_000_000,
    )
    .await;
    // `{schema, sessionRef, genesisRef}` and nothing else: the withdrawal.
    publish_policy(&w, serde_json::json!({}), &w.founder.clone(), 1_900_000_100).await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the withdrawal is the policy, so the default gates apply and arm (B) admits \
         (body: {body})"
    );
}

/// **The audit's "unreadable authoritative policy".** The newest record for
/// this mission carries a `gates` object this build cannot read. Nothing is
/// known about the policy, so the push is refused — never judged by the older,
/// weaker record underneath it.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_unreadable_newest_policy_refuses_rather_than_reading_an_older_one() {
    let w = watched().await;
    publish_policy(
        &w,
        serde_json::json!({ "gates": { "verifierRequired": false } }),
        &w.founder.clone(),
        1_900_000_000,
    )
    .await;
    publish_policy(
        &w,
        serde_json::json!({ "gates": { "verifierRequired": "yes-please" } }),
        &w.founder.clone(),
        1_900_000_100,
    )
    .await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an unreadable newest record closes both arms (body: {body})"
    );
    assert!(
        body.contains(&w.session_ref),
        "the refusal names the mission whose policy could not be read (body: {body})"
    );
}

/// **The audit's "provider self-certification", end to end.** A key that can
/// write to the channel publishes metadata naming this mission and then signs
/// `observed` green rows for the pushed commit. It was never commissioned by
/// the lifecycle, so it is not a provider, its rows fold to `declared`, and
/// arm (B) consumes none of them.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_metadata_only_provider_gains_no_authority() {
    let w = watched().await;
    // A second `Watched` over the same mission whose "provider" is the
    // impostor: `publish_provider_metadata` signs with `self.provider`, and
    // no lifecycle pair names this key.
    let impostor = Keys::generate();
    let claimed = Watched {
        state: w.state.clone(),
        community: w.community,
        channel_id: w.channel_id,
        session_ref: w.session_ref.clone(),
        genesis_ref: w.genesis_ref.clone(),
        founder: w.founder.clone(),
        provider: impostor.clone(),
        seat: w.seat.clone(),
    };
    claimed.publish_provider_metadata().await;
    claimed
        .observe(
            &impostor,
            CodingSessionObservationSource::Observed,
            default_green(HEAD_SHA),
        )
        .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "publishing metadata about a mission does not make you its observer (body: {body})"
    );

    // And the control: the *commissioned* provider's identical rows do admit
    // it, so the refusal above is about authority and not about the rows.
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;
    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the same rows from the key the lifecycle named do admit it (body: {body})"
    );
}

/// **Finding 91's other door.** `bound_repositories` is populated partly from
/// a mission's kind 44223 `repoRef`. A stranger's 44223 must not widen it, or
/// the binding would be a claim anybody could make — the same conversion
/// finding 90 closed, arriving through the repository field instead of the
/// provider set.
#[test]
fn a_strangers_metadata_cannot_widen_a_missions_binding() {
    let founder = Keys::generate();
    let provider = Keys::generate();
    let stranger = Keys::generate();
    let session_ref = Uuid::new_v4().to_string();
    let target = format!("30617:{}:beekeeper", founder.public_key().to_hex());
    let elsewhere = format!("30617:{}:tankloop", founder.public_key().to_hex());
    let channel = Uuid::new_v4();

    let metadata_of = |signer: &Keys, repo_ref: &str| -> nostr::Event {
        let metadata = beekeeper_core::coding_session_payload::SessionMetadata {
            schema: beekeeper_core::coding_session_payload::METADATA_SCHEMA.to_owned(),
            session: beekeeper_core::coding_session_command::CodingSessionTarget {
                driver: "claude-agent-acp".to_owned(),
                instance_id: "instance-1".to_owned(),
                session_id: "s-1".to_owned(),
                generation: 1,
            },
            project_ref: None,
            repo_ref: Some(repo_ref.to_owned()),
            title: None,
            agent_ref: None,
            role: None,
            provider: None,
            runtime: None,
            model: None,
            status: beekeeper_core::coding_session_payload::SessionStatus::Idle,
            branch: None,
            capabilities: beekeeper_core::coding_session_payload::Capabilities::v1_baseline(),
            session_ref: Some(session_ref.clone()),
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
            bee_stamp: None,
            pack_ref: None,
            handover: None,
            compose_ref: None,
        };
        let content = serde_json::to_string(&metadata).expect("metadata json");
        beekeeper_core::coding_session_payload::decode_coding_session_metadata(&content)
            .expect("the fixture's metadata must decode");
        EventBuilder::new(
            Kind::Custom(beekeeper_core::kind::KIND_CODING_SESSION_METADATA as u16),
            content,
        )
        .tags([Tag::parse(["h", &channel.to_string()]).expect("h")])
        .sign_with_keys(signer)
        .expect("sign metadata")
    };

    let providers = vec![provider.public_key().to_hex()];
    // The stranger names another repository; it must not appear.
    let bound = bound_repositories(
        &session_ref,
        &target,
        channel,
        &[],
        &founder.public_key().to_hex(),
        &providers,
        &[metadata_of(&stranger, &elsewhere)],
    );
    assert!(
        bound.is_empty(),
        "a key nobody commissioned cannot say what a mission works on: {bound:?}"
    );

    // The commissioned provider's own statement does.
    let bound = bound_repositories(
        &session_ref,
        &target,
        channel,
        &[],
        &founder.public_key().to_hex(),
        &providers,
        &[metadata_of(&provider, &elsewhere)],
    );
    assert_eq!(bound, vec![elsewhere.clone()]);

    // And a channel this repository grants binds the target itself — the
    // project half, read from the repository's side.
    let bound = bound_repositories(
        &session_ref,
        &target,
        channel,
        &[channel],
        &founder.public_key().to_hex(),
        &providers,
        &[],
    );
    assert_eq!(bound, vec![target.clone()]);

    // A mission in a channel this repository grants nothing to is bound to
    // nothing, and nothing it holds admits a push here.
    let bound = bound_repositories(
        &session_ref,
        &target,
        channel,
        &[Uuid::new_v4()],
        &founder.public_key().to_hex(),
        &providers,
        &[],
    );
    assert!(bound.is_empty(), "{bound:?}");
}

/// **Arm (A)'s receipt.** A founder's push is admitted with no mission read at
/// all, and the evidence the gate hands the audit says exactly that: the
/// founder exception, not a verifier's approval.
#[test]
fn a_founder_push_carries_the_policy_not_evaluated_receipt() {
    use beekeeper_core::coding_session_verdict_admission::{
        VerdictAdmissionEvidence, VerdictAdmissionPolicyNotEvaluated,
    };

    let founder = "ab".repeat(32);
    let evidence = VerdictAdmissionEvidence::FounderPush {
        pusher_pubkey: founder.clone(),
        policy_not_evaluated: VerdictAdmissionPolicyNotEvaluated::FounderException,
    };
    // The receipt writer must accept it without panicking and name the arm;
    // `record_admission` is what `policy.rs` calls on every admitted update.
    let founders = RepositoryFounders::from_parts(&founder, &[]);
    record_admission(
        "30617:aa:beekeeper",
        "refs/heads/main",
        HEAD_SHA,
        &founder,
        &founders,
        &evidence,
    );
    // F5: and the line names *which* founder source admitted the key. Three
    // grants live in three places; "founder" alone cannot be checked later.
    use beekeeper_core::repository_founders::FounderBasis;
    assert_eq!(
        founders.basis(&founder),
        Some(FounderBasis::AnnouncementSigner)
    );
    let maintainer = "cd".repeat(32);
    let roster = "ef".repeat(32);
    let two = RepositoryFounders::from_parts(
        &founder,
        &[vec!["maintainers".to_owned(), maintainer.clone()]],
    )
    .with_roster_owners([roster.clone()]);
    assert_eq!(two.basis(&maintainer), Some(FounderBasis::Maintainer));
    assert_eq!(two.basis(&roster), Some(FounderBasis::RosterOwner));
    assert_eq!(two.basis(&"11".repeat(32)), None);
    let VerdictAdmissionEvidence::FounderPush {
        policy_not_evaluated,
        ..
    } = evidence
    else {
        panic!("arm (A) evidence");
    };
    assert_eq!(
        policy_not_evaluated.as_str(),
        "founder_exception",
        "a founder's landing is the disclosed exception, never a verdict"
    );
}

/// Fill this mission's own policy slot with `count` structurally valid kind
/// 44245 records signed by a key entitled to set nothing — the flood.
///
/// Same `d`, same `csp-genesis`, newer than anything the founder published:
/// kind 44245 ingest is structure-only, so any channel member can publish
/// these. Before the signer set moved into the query they evicted the
/// founder's record from the page and the mission resolved to `Absent`.
async fn flood_the_missions_policies(w: &Watched, signer: &Keys, count: usize, at: u64) {
    for index in 0..count {
        publish_policy(
            w,
            serde_json::json!({ "gates": { "verifierRequired": false } }),
            signer,
            at + index as u64,
        )
        .await;
    }
}

/// **The 2026-09-06 counterexample, end to end.** After the founder publishes
/// a genuine hire, a seat borrows its event id in a self-signed kind-44221
/// `session.create`, names itself as `providerAuthorityPubkey`, answers it with
/// its own kind-44224 receipt, and signs `observed` green rows for the pushed
/// commit. The hire is attribution, so nothing in the create/receipt pair was
/// issued by a key entitled to steer the mission: the rows fold to `declared`
/// and the push is refused.
///
/// Then the founder signs a create for the *same* provider and the *same*
/// rows admit it — so the refusal is about who commissioned the execution,
/// not about the rows or the commit.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_seat_cannot_commission_itself_as_this_missions_provider() {
    let w = watched().await;
    let impostor = Keys::generate();
    let hire = publish_hire(&w, &w.founder).await;
    // The impostor signs both halves of its own lifecycle proof and cites the
    // genuine hire. The public reference changes attribution, not authority.
    w.commission_answering_hire_signed_by(&hire.id.to_hex(), &impostor, &impostor)
        .await;
    w.observe(
        &impostor,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a create its own subject signed commissions nobody (body: {body})"
    );

    // The current valid fulfillment path: the founder signs the create naming
    // a distinct provider, and that provider signs its own receipt.
    w.commission_answering_hire_signed_by(&hire.id.to_hex(), &w.founder, &impostor)
        .await;
    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the same rows, once a steering key commissioned their signer, do admit it (body: {body})"
    );
}

/// **The 2026-09-05 refuter's B2.** A channel member floods this mission's own
/// policy slot with more structurally valid kind 44245 records than the page
/// bound, all newer than the founder's `verifierRequired: true`. With the
/// signer set in the query the flood is never read, the founder's record is
/// still the newest authorized one, and arm (B) stays shut.
///
/// The asymmetry this closes: flooding the observation or lifecycle pages can
/// only refuse a push, and flooding this one failed **open**.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_unauthorized_flood_cannot_evict_this_missions_policy() {
    let w = watched().await;
    w.set_gate_policy(CodingSessionPolicyGates {
        red_first: None,
        review_every_lane: None,
        required_gates: None,
        verifier_required: Some(true),
    })
    .await;
    // The seat is a member of this channel and may publish; it may not set
    // this mission's policy. Strictly more records than the bound, all newer.
    flood_the_missions_policies(
        &w,
        &w.seat.clone(),
        VERDICT_ADMISSION_MAX_POLICIES + 8,
        2_100_000_000,
    )
    .await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an unauthorized flood must not turn a verifierRequired mission into a default one \
         (body: {body})"
    );
    assert!(
        !body.contains("policy could not be read"),
        "the founder's record is readable and authorized; the flood is simply not policy \
         (body: {body})"
    );
}

/// **The 2026-09-05 refuter's S3.** Two repositories bound to one channel —
/// the project-grant shape — and a mission whose own provider named the first
/// one. Its green rows do not admit a push to the second: the binding is what
/// the mission's authority said, and the project grant is only the fallback
/// for a mission that named nothing.
///
/// This is also where `MissionNotBoundToRepository` becomes reachable through
/// the relay at all: while the grant was unioned in, every candidate the relay
/// assembled was bound to the repository being pushed by construction.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_mission_bound_to_another_repository_of_the_same_project_does_not_admit() {
    let w = watched().await;
    // R1: a real second repository of the same founder, bound to this same
    // channel, and the one this mission says it works on.
    let first = format!("repo-{}", Uuid::new_v4().simple());
    let first_coordinate = format!("30617:{}:{first}", w.founder.public_key().to_hex());
    let announcement = EventBuilder::new(Kind::Custom(30617), "")
        .tags([
            Tag::parse(["d", &first]).expect("d"),
            Tag::parse(["buzz-channel", &w.channel_id.to_string()]).expect("binding"),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("protect"),
        ])
        .sign_with_keys(&w.founder)
        .expect("sign 30617");
    w.state
        .db
        .insert_event(w.community, &announcement, None)
        .await
        .expect("insert 30617");
    w.publish_provider_metadata_for(Some(&first_coordinate))
        .await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    // R2: the push, to the other repository on the same channel.
    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "green rows for R1 must not land a commit on R2 (body: {body})"
    );
    assert!(
        body.contains("is not bound to repository"),
        "the relay refuses with the binding rule itself, not merely with 'no mission found' \
         (body: {body})"
    );
    assert!(
        body.contains(&w.session_ref),
        "and it names the mission whose binding was checked (body: {body})"
    );
}
