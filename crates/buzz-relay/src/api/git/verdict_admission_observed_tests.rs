//! Arm **(B)** against Postgres: gate rows a provider watched, green on the
//! pushed commit, land a seat's push with no second seat.
//!
//! A sibling of [`super::tests`] because that file was already 980 lines and
//! the repository ceiling is 1,000. Every case here goes through the real
//! `git-receive-pack` policy handler, so it exercises the three storage reads
//! arm (B) added — the observation page, the founder-signed policy page, and
//! the provider-metadata page — rather than the pure rule, which
//! `buzz_core::coding_session_verdict_admission::observed_tests` covers.

use super::*;

use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
use buzz_core::coding_session_observation::{
    CodingSessionObservationBody, CodingSessionObservationGate,
    CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
    CodingSessionObservationPayload, CodingSessionObservationSource, CodingSessionObservationType,
    CODING_SESSION_OBSERVATION_SCHEMA,
};
use buzz_core::coding_session_policy::{
    CodingSessionPolicyGates, CodingSessionPolicyPayload, CODING_SESSION_POLICY_SCHEMA,
};
use buzz_core::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES;
use buzz_core::kind::{
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_OBSERVATION,
    KIND_CODING_SESSION_POLICY,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::git::policy::tests::{body_string, policy_test_state, push_response, seat_of};
use crate::api::git::policy::HookRefUpdate;
use crate::state::AppState;

pub(super) const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";
const OTHER_SHA: &str = "1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b";

/// A mission with a genesis, a seated builder, and a provider identity — and
/// deliberately **no** assignment, report or verdict of any kind. Arm (B) is
/// the arm that needs none of them.
pub(super) struct Watched {
    pub(super) state: Arc<AppState>,
    pub(super) community: buzz_core::CommunityId,
    pub(super) channel_id: Uuid,
    pub(super) session_ref: String,
    pub(super) genesis_ref: String,
    pub(super) founder: Keys,
    pub(super) provider: Keys,
    pub(super) seat: Keys,
}

pub(super) async fn watched() -> Watched {
    let state = policy_test_state().await;
    let host = format!("observed-{}.example", Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;
    let founder = Keys::generate();
    let provider = Keys::generate();
    let seat = Keys::generate();
    state
        .db
        .ensure_user(community, &founder.public_key().to_bytes())
        .await
        .expect("user");
    let channel_id = Uuid::new_v4();
    state
        .db
        .create_channel_with_id(
            community,
            channel_id,
            &format!("watched-{}", channel_id.simple()),
            buzz_core::channel::ChannelType::Stream,
            buzz_core::channel::ChannelVisibility::Open,
            None,
            &founder.public_key().to_bytes(),
            None,
            None,
        )
        .await
        .expect("channel");

    let session_ref = Uuid::new_v4().to_string();
    let genesis = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
        serde_json::to_string(&CodingSessionGenesisPayload::new(session_ref.clone()))
            .expect("genesis json"),
    )
    .tags([
        Tag::parse(["h", &channel_id.to_string()]).expect("h"),
        Tag::parse(["csg-v", "1"]).expect("v"),
        Tag::parse(["csg-session", &session_ref]).expect("session"),
    ])
    .sign_with_keys(&founder)
    .expect("sign genesis");
    state
        .db
        .insert_event(community, &genesis, Some(channel_id))
        .await
        .expect("insert genesis");
    let genesis_ref = genesis.id.to_hex();

    let watched = Watched {
        state,
        community,
        channel_id,
        session_ref,
        genesis_ref,
        founder,
        provider,
        seat,
    };
    // The provider's own metadata for this umbrella is what makes its key a
    // provider identity of this mission; without it every `observed` claim it
    // signs folds down to `declared`.
    watched.publish_provider_metadata().await;
    watched.grant_the_seat().await;
    watched
}

impl Watched {
    /// One kind 44223 metadata event, signed by the provider, naming this
    /// umbrella. The shape `mission_provider_pubkeys` reads.
    pub(super) async fn publish_provider_metadata(&self) {
        // Built through the real struct rather than by hand: the decoder is an
        // exact-key contract, and a hand-written body that silently failed to
        // decode would leave this fixture's provider unrecognised — which is
        // exactly how these cases first went red (every `observed` row folded
        // down to `declared`).
        let metadata = buzz_core::coding_session_payload::SessionMetadata {
            schema: buzz_core::coding_session_payload::METADATA_SCHEMA.to_owned(),
            session: buzz_core::coding_session_command::CodingSessionTarget {
                driver: "claude-agent-acp".to_owned(),
                instance_id: "instance-1".to_owned(),
                session_id: format!("s-{}", Uuid::new_v4().simple()),
                generation: 1,
            },
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: None,
            role: None,
            provider: None,
            runtime: None,
            model: None,
            status: buzz_core::coding_session_payload::SessionStatus::Idle,
            branch: None,
            capabilities: buzz_core::coding_session_payload::Capabilities::v1_baseline(),
            session_ref: Some(self.session_ref.clone()),
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
            bee_stamp: None,
            pack_ref: None,
        };
        let content = serde_json::to_string(&metadata).expect("metadata json");
        buzz_core::coding_session_payload::decode_coding_session_metadata(&content)
            .expect("the fixture's metadata must decode, or no provider is recognised");
        let event = EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_METADATA as u16), content)
            .tags([Tag::parse(["h", &self.channel_id.to_string()]).expect("h")])
            .sign_with_keys(&self.provider)
            .expect("sign metadata");
        self.state
            .db
            .insert_event(self.community, &event, Some(self.channel_id))
            .await
            .expect("insert metadata");
    }

    /// Seat the pusher as a builder of this mission, and make it a channel
    /// member so the ordinary role check lets it reach the gate at all.
    pub(super) async fn grant_the_seat(&self) {
        seat_of(&self.state, self.community, &self.seat, &self.founder).await;
        let payload =
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_grant_seat(
                self.genesis_ref.clone(),
                None,
                1,
                self.seat.public_key().to_hex(),
                "builder",
            );
        let event = buzz_sdk::builders::build_coding_session_authority_transition(
            self.channel_id,
            &payload,
        )
        .expect("transition builder")
        .sign_with_keys(&self.founder)
        .expect("sign transition");
        self.state
            .db
            .insert_event(self.community, &event, Some(self.channel_id))
            .await
            .expect("insert transition");
    }

    /// Publish one kind 44246 gate observation.
    pub(super) async fn observe(
        &self,
        signer: &Keys,
        source: CodingSessionObservationSource,
        rows: Vec<CodingSessionObservationGateRow>,
    ) {
        let payload = CodingSessionObservationPayload {
            schema: CODING_SESSION_OBSERVATION_SCHEMA.into(),
            session_ref: self.session_ref.clone(),
            genesis_ref: self.genesis_ref.clone(),
            observation_type: CodingSessionObservationType::Gate,
            source,
            assignment_ref: None,
            body: CodingSessionObservationBody::Gate(CodingSessionObservationGate { rows }),
        };
        let event = EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
            serde_json::to_string(&payload).expect("observation json"),
        )
        .tags([
            Tag::parse(["h", &self.channel_id.to_string()]).expect("h"),
            Tag::parse(["d", &self.session_ref]).expect("d"),
            Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("v"),
            Tag::parse(["csob-genesis", &self.genesis_ref]).expect("genesis"),
            Tag::parse(["csob-type", "gate"]).expect("type"),
        ])
        .sign_with_keys(signer)
        .expect("sign observation");
        self.state
            .db
            .insert_event(self.community, &event, Some(self.channel_id))
            .await
            .expect("insert observation");
    }

    /// Publish a founder-signed kind 44245 policy carrying only `gates`.
    async fn set_gate_policy(&self, gates: CodingSessionPolicyGates) {
        let payload = CodingSessionPolicyPayload {
            gates: Some(gates),
            ..CodingSessionPolicyPayload::empty(self.session_ref.clone(), self.genesis_ref.clone())
        };
        let event = EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_POLICY as u16),
            serde_json::to_string(&payload).expect("policy json"),
        )
        .tags([
            Tag::parse(["h", &self.channel_id.to_string()]).expect("h"),
            Tag::parse(["d", &self.session_ref]).expect("d"),
            Tag::parse(["csp-v", CODING_SESSION_POLICY_SCHEMA]).expect("v"),
            Tag::parse(["csp-genesis", &self.genesis_ref]).expect("genesis"),
        ])
        .sign_with_keys(&self.founder)
        .expect("sign policy");
        self.state
            .db
            .insert_event(self.community, &event, Some(self.channel_id))
            .await
            .expect("insert policy");
    }

    /// The seat's own fast-forward of a `require-verdict` `main`.
    async fn seat_push(&self, new_oid: &str) -> (StatusCode, String) {
        self.seat_push_announced_as(
            new_oid,
            vec![
                Tag::parse(["buzz-channel", &self.channel_id.to_string()]).expect("binding"),
                Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"])
                    .expect("protect"),
            ],
        )
        .await
    }

    /// The same push against an announcement the caller composes, so a sibling
    /// module can bind the repository somewhere other than this mission's own
    /// channel — the live shape finding 56 caught.
    pub(super) async fn seat_push_announced_as(
        &self,
        new_oid: &str,
        tags: Vec<Tag>,
    ) -> (StatusCode, String) {
        let response = push_response(
            &self.state,
            self.community,
            &self.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            tags,
            &self.seat.public_key().to_hex(),
            HookRefUpdate {
                old_oid: "1".repeat(40),
                new_oid: new_oid.to_string(),
                ref_name: "refs/heads/main".to_string(),
                is_ancestor: true,
            },
        )
        .await;
        body_string(response).await
    }
}

/// One row, green on `head_sha` over a clean tree.
fn green(gate: &str, head_sha: &str) -> CodingSessionObservationGateRow {
    CodingSessionObservationGateRow {
        gate: gate.to_owned(),
        outcome: CodingSessionObservationGateOutcome::Passed,
        command: format!("{gate} --locked"),
        summary: None,
        duration_ms: Some(1_000),
        head_sha: Some(head_sha.to_owned()),
        dirty: Some(false),
    }
}

pub(super) fn default_green(head_sha: &str) -> Vec<CodingSessionObservationGateRow> {
    DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| green(gate, head_sha))
        .collect()
}

/// The headline: a seat lands its own work on a gated `main`, with no
/// verifier, on the strength of rows its provider signed about this commit.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_observed_green_gates_on_the_pushed_sha_admit_a_seats_push() {
    let w = watched().await;
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
        "green observed gates on this commit are the landing (body: {body})"
    );
}

/// The same rows, a different commit. Without `headSha` this push would land
/// on somebody else's green — finding 27's shape.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_green_rows_for_another_commit_do_not_land_this_one() {
    let w = watched().await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(OTHER_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains(&format!("No observed gate row names {HEAD_SHA}")),
        "the refusal says the rows name another commit: {body}"
    );
}

/// A red gate is named, on the commit it was red on.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_a_red_gate_is_named_in_the_refusal() {
    let w = watched().await;
    let mut rows = default_green(HEAD_SHA);
    rows[1].outcome = CodingSessionObservationGateOutcome::Failed;
    w.observe(&w.provider, CodingSessionObservationSource::Observed, rows)
        .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert_eq!(
        body,
        format!(
            "refs/heads/main: gate `{}` was observed red on {HEAD_SHA}. Fix it and run it \
             again; the next observed row names the commit it ran at.",
            DEFAULT_REQUIRED_GATES[1]
        )
    );
}

/// Green over a worktree the commit does not name is not evidence about that
/// commit.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_a_dirty_worktree_refuses_however_green_the_rows() {
    let w = watched().await;
    let rows = default_green(HEAD_SHA)
        .into_iter()
        .map(|mut row| {
            row.dirty = Some(true);
            row
        })
        .collect();
    w.observe(&w.provider, CodingSessionObservationSource::Observed, rows)
        .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains(&format!("{HEAD_SHA} was observed dirty")),
        "{body}"
    );
}

/// The seat signing its own rows — `observed` and all — is a claim. The fold
/// downgrades it because the signer is no provider of this mission, and the
/// refusal says so.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_rows_the_seat_signed_are_declared_and_refuse() {
    let w = watched().await;
    w.observe(
        &w.seat,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains("its own subject saying so about itself"),
        "{body}"
    );
}

/// `gates.verifierRequired: true` turns arm (B) off, and the refusal is arm
/// (C)'s — the founder asked for a second seat and does not get told about
/// gate rows instead.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_is_off_when_the_founders_policy_requires_a_verifier() {
    let w = watched().await;
    w.set_gate_policy(CodingSessionPolicyGates {
        red_first: None,
        review_every_lane: None,
        required_gates: None,
        verifier_required: Some(true),
    })
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
        "a founder who asked for a verifier gets one (body: {body})"
    );
    assert!(
        body.contains("no mission verdict names this commit"),
        "{body}"
    );
}

/// The founder's own gate list replaces the default, and one green row under
/// it is the whole requirement.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_the_founders_own_gate_list_is_the_one_enforced() {
    let w = watched().await;
    w.set_gate_policy(CodingSessionPolicyGates {
        red_first: None,
        review_every_lane: None,
        required_gates: Some(vec!["just ci".into()]),
        verifier_required: Some(false),
    })
    .await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        vec![green("just ci", HEAD_SHA)],
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

/// A required gate nobody ran refuses, and names the whole list.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_a_required_gate_never_observed_refuses_and_names_the_list() {
    let w = watched().await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        vec![green(DEFAULT_REQUIRED_GATES[0], HEAD_SHA)],
    )
    .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    for gate in DEFAULT_REQUIRED_GATES {
        assert!(body.contains(gate), "the required list is named: {body}");
    }
}

/// A row signed before `headSha` existed decodes, renders, and admits
/// nothing: absent is not "the commit being pushed".
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_rows_from_before_the_key_existed_still_refuse() {
    let w = watched().await;
    let rows = DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| CodingSessionObservationGateRow {
            gate: (*gate).to_owned(),
            outcome: CodingSessionObservationGateOutcome::Passed,
            command: format!("{gate} --locked"),
            summary: None,
            duration_ms: Some(1_000),
            head_sha: None,
            dirty: None,
        })
        .collect();
    w.observe(&w.provider, CodingSessionObservationSource::Observed, rows)
        .await;

    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a row naming no commit lands nothing (body: {body})"
    );
}

/// A founder still lands with no gate row at all: arm (A) is answered first
/// and reads no mission.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn b_does_not_disturb_a_founder_push() {
    let w = watched().await;
    let response = push_response(
        &w.state,
        w.community,
        &w.founder,
        &format!("repo-{}", Uuid::new_v4().simple()),
        vec![
            Tag::parse(["buzz-channel", &w.channel_id.to_string()]).expect("binding"),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("protect"),
        ],
        &w.founder.public_key().to_hex(),
        HookRefUpdate {
            old_oid: "1".repeat(40),
            new_oid: HEAD_SHA.to_string(),
            ref_name: "refs/heads/main".to_string(),
            is_ancestor: true,
        },
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}
