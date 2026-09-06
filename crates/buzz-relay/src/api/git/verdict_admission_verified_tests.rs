//! Arm **(C)** against Postgres, after the 2026-09-03 follow-up ruling: a
//! verifier's clearance is half of what a verdict-gated ref wants, and the
//! other half is arm (B)'s own evidence.
//!
//! A sibling of [`super::tests`] and [`super::observed_tests`] because it
//! needs what neither of them assembles alone — a kind 44244 assignment →
//! report → disposition → refutation chain **and** provider-signed kind 44246
//! observations **and** a founder-signed kind 44245 policy — and because both
//! of those files are already close to the repository's 1,000-line ceiling.
//!
//! Every case here goes through the real `git-receive-pack` policy handler, so
//! the storage reads are exercised rather than assumed: the pure rule is
//! covered by `buzz_core::coding_session_verdict_admission::verified_tests`.

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
use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamRefutationDecision, CodingSessionTeamReport, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CodingSessionTeamVerdict,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES;
use buzz_core::kind::{
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_OBSERVATION,
    KIND_CODING_SESSION_POLICY, KIND_CODING_SESSION_TEAM_TRANSACTION,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::git::policy::tests::{body_string, policy_test_state, push_response, seat_of};
use crate::api::git::policy::HookRefUpdate;
use crate::state::AppState;

const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";

/// A mission with a settled, independently cleared report over `HEAD_SHA`, a
/// seated builder that pushes, and a provider identity that may watch gates.
struct Verified {
    state: Arc<AppState>,
    community: buzz_core::CommunityId,
    channel_id: Uuid,
    session_ref: String,
    genesis_ref: String,
    founder: Keys,
    provider: Keys,
    builder: Keys,
    verifier: Keys,
}

/// Build the mission and store every record it needs.
///
/// The **founder** signs the disposition — the governance fold authorises a
/// disposition from `may_lead` only — and the **verifier seat** signs the
/// `not-refuted` refutation, which is the only verdict verb a verifier may
/// author. That is arm (C)'s shape, unchanged by this ruling; what the ruling
/// adds is the gate rows each case below varies.
async fn verified() -> Verified {
    let state = policy_test_state().await;
    let host = format!("verified-{}.example", Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;
    let founder = Keys::generate();
    let provider = Keys::generate();
    let builder = Keys::generate();
    let verifier = Keys::generate();
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
            &format!("verified-{}", channel_id.simple()),
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

    let mission = Verified {
        state,
        community,
        channel_id,
        session_ref,
        genesis_ref,
        founder,
        provider,
        builder,
        verifier,
    };
    // Finding 90: the accepted lifecycle is what names the provider. The
    // metadata below describes an execution; it does not authorize one.
    super::observed_tests::commission_provider(
        &mission.state,
        mission.community,
        mission.channel_id,
        &mission.session_ref,
        &mission.genesis_ref,
        &mission.founder,
        &mission.provider,
    )
    .await;
    mission.publish_provider_metadata().await;
    // The authority chain is a chain: seq 1 has no predecessor and every seq
    // after it names the accepted transition before it, so the two grants are
    // linked rather than both claiming to be first.
    let first = mission
        .grant_seat(&mission.builder, "builder", 1, None)
        .await;
    mission
        .grant_seat(&mission.verifier, "verifier", 2, Some(&first))
        .await;
    mission.publish_the_chain().await;
    mission
}

impl Verified {
    /// One kind 44223 metadata event signed by the provider, naming this
    /// umbrella — what makes its key a provider identity of this mission.
    async fn publish_provider_metadata(&self) {
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

    /// Seat `keys` under `role`, and make it a channel member so the ordinary
    /// role check lets it reach the gate at all.
    ///
    /// `prev` is the accepted transition this one follows — `None` only for
    /// `seq` 1, which is the chain's own rule ("seq must be exactly 1 if and
    /// only if prevAccepted is null"). Returns this transition's event id so
    /// the next grant can name it.
    async fn grant_seat(&self, keys: &Keys, role: &str, seq: u32, prev: Option<&str>) -> String {
        seat_of(&self.state, self.community, keys, &self.founder).await;
        let payload =
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_grant_seat(
                self.genesis_ref.clone(),
                prev.map(str::to_owned),
                seq,
                keys.public_key().to_hex(),
                role,
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
        event.id.to_hex()
    }

    fn sign_transaction(
        &self,
        body: CodingSessionTeamTransactionBody,
        keys: &Keys,
    ) -> nostr::Event {
        let payload = CodingSessionTeamTransactionPayload {
            schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
            session_ref: self.session_ref.clone(),
            genesis_ref: self.genesis_ref.clone(),
            transaction_type: body.transaction_type(),
            supersedes: None,
            delivery_command_id: None,
            body,
        };
        EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
            serde_json::to_string(&payload).expect("payload json"),
        )
        .tags([
            Tag::parse(["h", &self.channel_id.to_string()]).expect("h"),
            Tag::parse(["d", &self.session_ref]).expect("d"),
            Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
            Tag::parse(["cstx-genesis", &self.genesis_ref]).expect("genesis"),
            Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type"),
        ])
        .sign_with_keys(keys)
        .expect("sign transaction")
    }

    /// Assignment → report → approving disposition → `not-refuted` refutation.
    async fn publish_the_chain(&self) {
        let assignment = self.sign_transaction(
            CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                assignee_actor: self.builder.public_key().to_hex(),
                assignee_role: "builder".into(),
                objective: "Land the branch".into(),
                brief: "Build it and report.".into(),
                branch: None,
                base_sha: None,
                file_ownership: vec!["crates".into()],
                acceptance_steps: vec!["cargo test".into()],
            }),
            &self.founder,
        );
        let report = self.sign_transaction(
            CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                assignment_ref: assignment.id.to_hex(),
                summary: "Done".into(),
                branch: None,
                base_sha: None,
                head_sha: Some(HEAD_SHA.into()),
                files: Vec::new(),
                tests: Vec::new(),
                red_before_green: None,
                deviations: Vec::new(),
                residuals: Vec::new(),
                anomalies: Vec::new(),
            }),
            &self.builder,
        );
        let disposition = self.sign_transaction(
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                assignment_ref: assignment.id.to_hex(),
                report_ref: report.id.to_hex(),
                refutation_ref: None,
                decision: CodingSessionTeamDispositionDecision::Approve,
                summary: "Ruled".into(),
                findings: Vec::new(),
                required_action: None,
            }),
            &self.founder,
        );
        let refutation = self.sign_transaction(
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
                assignment_ref: assignment.id.to_hex(),
                report_ref: report.id.to_hex(),
                decision: CodingSessionTeamRefutationDecision::NotRefuted,
                summary: "Could not break it.".into(),
                findings: Vec::new(),
                required_action: None,
            }),
            &self.verifier,
        );
        for event in [&assignment, &report, &disposition, &refutation] {
            self.state
                .db
                .insert_event(self.community, event, Some(self.channel_id))
                .await
                .expect("insert transaction");
        }
    }

    /// Publish one kind 44246 gate observation.
    async fn observe(
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

    /// `gates.verifierRequired: true`, founder-signed — the policy this ruling
    /// is about, and the one that closes arm (B) so arm (C) is the only route.
    async fn require_a_verifier(&self) {
        let payload = CodingSessionPolicyPayload {
            gates: Some(CodingSessionPolicyGates {
                red_first: None,
                review_every_lane: None,
                required_gates: None,
                verifier_required: Some(true),
            }),
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

    /// The builder's own fast-forward of a `require-verdict` `main`.
    async fn builder_push(&self) -> (StatusCode, String) {
        let response = push_response(
            &self.state,
            self.community,
            &self.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            vec![
                Tag::parse(["buzz-channel", &self.channel_id.to_string()]).expect("binding"),
                Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"])
                    .expect("protect"),
            ],
            &self.builder.public_key().to_hex(),
            HookRefUpdate {
                old_oid: "1".repeat(40),
                new_oid: HEAD_SHA.to_string(),
                ref_name: "refs/heads/main".to_string(),
                is_ancestor: true,
            },
        )
        .await;
        body_string(response).await
    }
}

/// One row, green on `HEAD_SHA` over a clean tree.
fn green(gate: &str) -> CodingSessionObservationGateRow {
    CodingSessionObservationGateRow {
        gate: gate.to_owned(),
        outcome: CodingSessionObservationGateOutcome::Passed,
        command: format!("{gate} --locked"),
        summary: None,
        duration_ms: Some(1_000),
        head_sha: Some(HEAD_SHA.to_owned()),
        dirty: Some(false),
    }
}

fn default_green() -> Vec<CodingSessionObservationGateRow> {
    DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| green(gate))
        .collect()
}

/// The headline: both halves, on a mission whose founder asked for a verifier.
/// The clearance settles who checked it; the rows say what was checked.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn c_a_clearance_over_observed_green_gates_admits_a_seats_push() {
    let mission = verified().await;
    mission.require_a_verifier().await;
    mission
        .observe(
            &mission.provider,
            CodingSessionObservationSource::Observed,
            default_green(),
        )
        .await;

    let (status, body) = mission.builder_push().await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a cleared report over gates observed green on this commit is the landing (body: {body})"
    );
}

/// The half this ruling adds. Before L27 this exact fixture landed, which made
/// `verifierRequired: true` the *weaker* of the two arms.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn c_a_clearance_with_no_gate_row_at_all_is_refused_and_names_the_rows() {
    let mission = verified().await;
    mission.require_a_verifier().await;

    let (status, body) = mission.builder_push().await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains(&format!(
            "verifier {} cleared {HEAD_SHA}",
            mission.verifier.public_key().to_hex()
        )),
        "the refusal names the half that is satisfied, first: {body}"
    );
    for gate in DEFAULT_REQUIRED_GATES {
        assert!(body.contains(gate), "and the gate list it wants: {body}");
    }
}

/// A verifier cleared it and a gate was observed red on the same commit. The
/// sentence names both, and arm (B)'s own red-gate words are carried whole.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn c_a_clearance_over_a_red_gate_is_refused_and_names_the_gate() {
    let mission = verified().await;
    mission.require_a_verifier().await;
    let mut rows = default_green();
    rows[2].outcome = CodingSessionObservationGateOutcome::Failed;
    mission
        .observe(
            &mission.provider,
            CodingSessionObservationSource::Observed,
            rows,
        )
        .await;

    let (status, body) = mission.builder_push().await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains(&format!(
            "gate `{}` was observed red on {HEAD_SHA}",
            DEFAULT_REQUIRED_GATES[2]
        )),
        "{body}"
    );
    assert!(
        body.contains(&mission.verifier.public_key().to_hex()),
        "neither half is hidden: {body}"
    );
}

/// Rows the pushing seat signed are `declared` after the provenance fold, and
/// a verifier's clearance does not launder them into evidence.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn c_a_clearance_over_rows_the_seat_signed_is_refused() {
    let mission = verified().await;
    mission.require_a_verifier().await;
    mission
        .observe(
            &mission.builder,
            CodingSessionObservationSource::Observed,
            default_green(),
        )
        .await;

    let (status, body) = mission.builder_push().await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains(&mission.verifier.public_key().to_hex()),
        "{body}"
    );
}

/// With **no** policy at all the same mission is admitted by arm (B) first —
/// it is the cheaper arm and nothing closed it. The ruling raises the bar on
/// the verifier-required class; it does not lower it anywhere.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn c_without_the_flag_the_cheaper_arm_still_answers_first() {
    let mission = verified().await;
    mission
        .observe(
            &mission.provider,
            CodingSessionObservationSource::Observed,
            default_green(),
        )
        .await;

    let (status, body) = mission.builder_push().await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

/// A founder still lands with neither half: arm (A) is answered before any
/// mission is read.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn c_the_new_half_does_not_disturb_a_founder_push() {
    let mission = verified().await;
    mission.require_a_verifier().await;
    let response = push_response(
        &mission.state,
        mission.community,
        &mission.founder,
        &format!("repo-{}", Uuid::new_v4().simple()),
        vec![
            Tag::parse(["buzz-channel", &mission.channel_id.to_string()]).expect("binding"),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("protect"),
        ],
        &mission.founder.public_key().to_hex(),
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
