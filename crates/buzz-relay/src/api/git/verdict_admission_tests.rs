//! Tests for the verdict gate's storage half and its denial rendering.
//!
//! The Postgres-backed cases drive the real `hook_policy_check` through the
//! fixtures in [`super::super::policy::tests`], because what has to be proven
//! is what a push gets back — not what a helper returns.

use super::*;

use buzz_core::git_perms::{parse_protection_tag, UpdateKind};

fn denial(ref_name: &str, reason: &str) -> Denial {
    Denial {
        ref_name: ref_name.to_string(),
        reason: reason.to_string(),
    }
}

async fn body_of(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

// ── L6.3: what a client nobody upgraded prints ───────────────────────────

/// The pre-receive hook `cat`s the 403 body to stderr unchanged, so the body
/// *is* the user interface. It used to be a serialized struct.
#[tokio::test]
async fn the_denial_body_is_one_readable_line_per_ref() {
    let denials = vec![
        denial("refs/heads/main", "require-verdict is set"),
        denial("refs/tags/v1", "ref deletion denied: no-delete is set"),
    ];
    let response = denial_response(&denials, None);
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(
        body_of(response).await,
        "refs/heads/main: require-verdict is set\n\
         refs/tags/v1: ref deletion denied: no-delete is set"
    );
}

/// The structured shape did not disappear; it moved to a header verbatim.
#[tokio::test]
async fn the_structured_denials_move_to_a_header_verbatim() {
    let denials = vec![denial("refs/heads/main", "nope")];
    let json = r#"{"allowed":false,"denials":[{"ref_name":"refs/heads/main","reason":"nope"}]}"#;
    let response = denial_response(&denials, Some(json.to_string()));
    assert_eq!(
        response
            .headers()
            .get(GIT_DENIALS_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(json)
    );
}

/// Above the cap the header is dropped, never truncated: half a JSON document
/// is worse than none, and the body is complete on its own.
#[tokio::test]
async fn an_oversized_structured_header_is_dropped_not_truncated() {
    let denials = vec![denial("refs/heads/main", "nope")];
    let json = "x".repeat(MAX_DENIAL_HEADER_BYTES + 1);
    let response = denial_response(&denials, Some(json));
    assert!(response.headers().get(GIT_DENIALS_HEADER).is_none());
    assert_eq!(body_of(response).await, "refs/heads/main: nope");
}

/// A ref name need not be ASCII. HTTP permits obs-text in a header value, so
/// the JSON survives; what must never happen is a mangled value, and the
/// renderer drops rather than mangles anything `HeaderValue` refuses (a
/// control byte — which JSON escaping already prevents). The body is the
/// contract either way.
#[tokio::test]
async fn a_non_ascii_ref_name_still_renders_in_the_body() {
    let denials = vec![denial("refs/heads/naïve", "nope")];
    let json = "{\"ref\":\"naïve\"}".to_string();
    let response = denial_response(&denials, Some(json.clone()));
    assert_eq!(
        response
            .headers()
            .get(GIT_DENIALS_HEADER)
            .map(|value| value.as_bytes().to_vec()),
        Some(json.into_bytes())
    );
    assert_eq!(body_of(response).await, "refs/heads/naïve: nope");

    // A control byte is what `HeaderValue` refuses; the body is unaffected.
    let response = denial_response(&denials, Some("{\"a\":\"\n\"}".to_string()));
    assert!(response.headers().get(GIT_DENIALS_HEADER).is_none());
    assert_eq!(body_of(response).await, "refs/heads/naïve: nope");
}

// ── L6.1: the gate runs only where the rule is set ───────────────────────

fn update(ref_name: &str, new_oid: &str) -> RefUpdate {
    RefUpdate {
        ref_name: ref_name.to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "1".repeat(40),
        new_oid: new_oid.to_string(),
    }
}

/// The cost claim, proven structurally: `refs_requiring_verdict` is the only
/// thing that decides whether a session query happens, and an ordinary push
/// gives it nothing to search.
#[test]
fn an_ordinary_push_asks_for_no_verdict_search_at_all() {
    let rules = vec![
        parse_protection_tag(&["refs/heads/*", "no-force-push"]).expect("rule"),
        parse_protection_tag(&["refs/heads/release", "push:admin"]).expect("rule"),
    ];
    let updates = vec![
        update("refs/heads/main", &"2".repeat(40)),
        update("refs/heads/topic", &"3".repeat(40)),
    ];
    assert!(
        refs_requiring_verdict(&updates, &rules, &[]).is_empty(),
        "no require-verdict rule matches, so no session is ever queried"
    );
}

#[test]
fn only_the_governed_ref_is_searched_and_never_an_already_denied_one() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "require-verdict"]).expect("rule")];
    let updates = vec![
        update("refs/heads/main", &"2".repeat(40)),
        update("refs/heads/topic", &"3".repeat(40)),
    ];
    let gated = refs_requiring_verdict(&updates, &rules, &[]);
    assert_eq!(gated.len(), 1);
    assert_eq!(gated[0].ref_name, "refs/heads/main");

    let denied = vec![denial(
        "refs/heads/main",
        "requires admin role (you have member)",
    )];
    assert!(
        refs_requiring_verdict(&updates, &rules, &denied).is_empty(),
        "an update the role check already refused is not searched again"
    );
}

// ── L6.1/L6.2 end to end, against Postgres ───────────────────────────────

mod live {
    use super::*;

    use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
    use buzz_core::coding_session_team_transaction::{
        CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision, CodingSessionTeamReport,
        CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
        CodingSessionTeamVerdict, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    };
    use buzz_core::kind::{KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_TEAM_TRANSACTION};
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use std::sync::Arc;
    use uuid::Uuid;

    use crate::api::git::policy::tests::{body_string, policy_test_state, push_response};
    use crate::api::git::policy::HookRefUpdate;
    use crate::state::AppState;

    const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";

    struct Mission {
        state: Arc<AppState>,
        community: buzz_core::CommunityId,
        channel_id: Uuid,
        founder: Keys,
    }

    /// A channel, a genesis founded by `founder`, and one assignment → report
    /// → disposition chain over `head_sha`.
    async fn mission(
        decision: CodingSessionTeamDispositionDecision,
        head_sha: Option<&str>,
        branch: Option<&str>,
    ) -> Mission {
        let state = policy_test_state().await;
        let host = format!("verdict-{}.example", Uuid::new_v4().simple());
        let community = state
            .db
            .ensure_configured_community(&host)
            .await
            .expect("community")
            .id;
        let founder = Keys::generate();
        let builder = Keys::generate();
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
                &format!("mission-{}", channel_id.simple()),
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
        let genesis_payload = CodingSessionGenesisPayload::new(session_ref.clone());
        let genesis = EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
            serde_json::to_string(&genesis_payload).expect("genesis json"),
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
        let payload =
            |body: CodingSessionTeamTransactionBody| CodingSessionTeamTransactionPayload {
                schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
                session_ref: session_ref.clone(),
                genesis_ref: genesis_ref.clone(),
                transaction_type: body.transaction_type(),
                supersedes: None,
                delivery_command_id: None,
                body,
            };
        let sign = |payload: CodingSessionTeamTransactionPayload, keys: &Keys| {
            EventBuilder::new(
                Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
                serde_json::to_string(&payload).expect("payload json"),
            )
            .tags([
                Tag::parse(["h", &channel_id.to_string()]).expect("h"),
                Tag::parse(["d", &session_ref]).expect("d"),
                Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
                Tag::parse(["cstx-genesis", &genesis_ref]).expect("genesis"),
                Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type"),
            ])
            .sign_with_keys(keys)
            .expect("sign transaction")
        };

        let assignment = sign(
            payload(CodingSessionTeamTransactionBody::Assignment(
                CodingSessionTeamAssignment {
                    assignee_actor: builder.public_key().to_hex(),
                    assignee_role: "builder".into(),
                    objective: "Land the branch".into(),
                    brief: "Build it and report.".into(),
                    branch: branch.map(str::to_string),
                    base_sha: None,
                    file_ownership: vec!["crates".into()],
                    acceptance_steps: vec!["cargo test".into()],
                },
            )),
            &founder,
        );
        let report = sign(
            payload(CodingSessionTeamTransactionBody::Report(
                CodingSessionTeamReport {
                    assignment_ref: assignment.id.to_hex(),
                    summary: "Done".into(),
                    branch: branch.map(str::to_string),
                    base_sha: None,
                    head_sha: head_sha.map(str::to_string),
                    files: Vec::new(),
                    tests: Vec::new(),
                    red_before_green: None,
                    deviations: Vec::new(),
                    residuals: Vec::new(),
                    anomalies: Vec::new(),
                },
            )),
            &builder,
        );
        let disposition = sign(
            payload(CodingSessionTeamTransactionBody::Verdict(
                CodingSessionTeamVerdict::Disposition {
                    assignment_ref: assignment.id.to_hex(),
                    report_ref: report.id.to_hex(),
                    refutation_ref: None,
                    decision,
                    summary: "Ruled".into(),
                    findings: Vec::new(),
                    required_action: None,
                },
            )),
            &founder,
        );
        for event in [&assignment, &report, &disposition] {
            state
                .db
                .insert_event(community, event, Some(channel_id))
                .await
                .expect("insert transaction");
        }

        Mission {
            state,
            community,
            channel_id,
            founder,
        }
    }

    fn fast_forward(new_oid: &str) -> HookRefUpdate {
        HookRefUpdate {
            old_oid: "1".repeat(40),
            new_oid: new_oid.to_string(),
            ref_name: "refs/heads/main".to_string(),
            is_ancestor: true,
        }
    }

    fn guarded_repo(channel_id: Uuid) -> Vec<Tag> {
        vec![
            Tag::parse(["buzz-channel", &channel_id.to_string()]).expect("binding"),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("protect"),
        ]
    }

    /// The headline: the founder's own push of an approved commit is admitted
    /// — the gate governs, it does not simply block.
    ///
    /// The report names `main` because that is the update it approves; since
    /// fix round 1 an approval is scoped to the branch its report named
    /// (F7), and the sibling test below pushes the same approved commit
    /// somewhere else and is refused.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_approved_head_sha_is_admitted_for_the_founder() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an approved commit must land (body: {body})"
        );
    }

    /// The same approval, the same commit, a different gated ref — refused.
    ///
    /// Before fix round 1 an approval admitted its commit onto every gated ref
    /// forever, which is also how an approved-but-old commit could be pushed
    /// back over `main`.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_approval_for_one_branch_does_not_admit_another_ref() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("whoami/cli"),
        )
        .await;
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            format!(
                "refs/heads/main: commit {HEAD_SHA} is approved for whoami/cli, and this is a \
                 different ref. A verdict admits a commit to the branch its report named."
            )
        );
    }

    /// Live run 3's shape, on the wire: `changes-requested`, and the same
    /// commit pushed anyway.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_changes_requested_verdict_refuses_the_same_commit() {
        let m = mission(
            CodingSessionTeamDispositionDecision::ChangesRequested,
            Some(HEAD_SHA),
            Some("whoami/cli"),
        )
        .await;
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            format!(
                "refs/heads/main: require-verdict is set and no mission verdict names this \
                 commit: no approved report names {HEAD_SHA}. Searched 1 mission(s) — the \
                 newest 16 on this channel whose founder is a founder of this repository — \
                 over one shared page of the newest 512 team transactions on that channel. An \
                 older ruling can fall outside both."
            ),
            "the body is what `git push` prints, prefixed by the ref"
        );
    }

    /// A branch name is not a commit.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_approved_report_naming_only_a_branch_refuses() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            None,
            Some("whoami/cli"),
        )
        .await;
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            "refs/heads/main: require-verdict is set and no mission verdict names this commit: \
             the approved report for this work names a branch and no headSha, and a branch name \
             is not a commit."
        );
    }

    /// The rule set on a repository bound to nothing says so, rather than
    /// searching an empty set and blaming the commit.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn the_rule_on_an_unbound_repository_says_it_can_read_no_verdict() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            None,
        )
        .await;
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            vec![
                Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"])
                    .expect("protect"),
            ],
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            "refs/heads/main: require-verdict is set and this repository is bound to no channel, \
             so no mission verdict can be read here. Remove the rule, or bind the repository to \
             the mission's channel."
        );
    }

    /// An ungoverned ref in the same repository is untouched: the rule only
    /// ever subtracts, and only where its pattern matches.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_ungoverned_ref_in_a_guarded_repository_still_pushes() {
        let m = mission(
            CodingSessionTeamDispositionDecision::ChangesRequested,
            Some(HEAD_SHA),
            None,
        )
        .await;
        let mut topic = fast_forward(HEAD_SHA);
        topic.ref_name = "refs/heads/topic".to_string();
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &m.founder.public_key().to_hex(),
            topic,
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "refs/heads/topic carries no require-verdict rule (body: {body})"
        );
    }

    /// **Finding 27, refused.** This is the exact update that landed
    /// unverified code: a hired seat of the repository owner
    /// fast-forwarding `refs/heads/main`. The inherited-authority cap does
    /// NOT stop it — a fast-forward needs only Member and a capped seat
    /// holds Member. `require-verdict` is what stops it, and only when it is
    /// set.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_seats_fast_forward_of_main_is_refused_once_the_rule_is_set() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let seat = Keys::generate();
        crate::api::git::policy::tests::seat_of(&m.state, m.community, &seat, &m.founder).await;

        // With the rule set: refused, and the refusal says whose rule it is.
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &seat.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "a seat must not land the trunk on its operator's authority (body: {body})"
        );
        assert_eq!(
            body,
            format!(
                "refs/heads/main: commit {HEAD_SHA} is approved, but the relay's \
                 require-verdict rule reserves a gated ref to a founder of this repository (1 \
                 founder(s): the announcement's signer, its maintainers tag, and the project \
                 roster's owners). Ask a founder to land it."
            ),
            "the reservation is attributed to the relay's rule, not to a mission policy \
             nobody read"
        );

        // WITHOUT the rule — today's behaviour, disclosed rather than implied:
        // the same seat performs the same fast-forward and it is allowed.
        // Landing this lane's code does not close finding 27; setting the
        // rule does.
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            vec![Tag::parse(["buzz-channel", &m.channel_id.to_string()]).expect("binding")],
            &seat.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an UNGOVERNED main still takes a seat's fast-forward — this is finding 27's \
             hole, and only the rule closes it (body: {body})"
        );
    }

    // ── finding 33: a repository has founders, not an owner ─────────────

    /// Make `pubkey` an ordinary member of the mission's channel, so the role
    /// check lets them reach the verdict gate at all. Member is exactly the
    /// tier a fast-forward of `refs/heads/main` needs.
    async fn channel_member(m: &Mission, keys: &Keys) {
        m.state
            .db
            .ensure_user(m.community, &keys.public_key().to_bytes())
            .await
            .expect("user");
        m.state
            .db
            .add_member(
                m.community,
                m.channel_id,
                &keys.public_key().to_bytes(),
                buzz_core::channel::MemberRole::Member,
                Some(&m.founder.public_key().to_bytes()),
            )
            .await
            .expect("member");
    }

    /// The live shape on hive: the announcement is Andy's, the mission is
    /// Brian's, and a `maintainers` tag says they are both founders. Before
    /// this lane the genesis query was author-scoped to Andy, so Brian's
    /// ruling was never even read.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_maintainers_tag_admits_the_other_founders_mission() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let announcer = Keys::generate();
        let mut tags = guarded_repo(m.channel_id);
        tags.push(
            Tag::parse(["maintainers", &m.founder.public_key().to_hex()]).expect("maintainers"),
        );
        let response = push_response(
            &m.state,
            m.community,
            &announcer,
            &format!("repo-{}", Uuid::new_v4().simple()),
            tags,
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a maintainer's own approved commit lands (body: {body})"
        );
    }

    /// The same announcement without the tag refuses exactly as it does today
    /// — the rule is not weakened for a single-founder repository, and the
    /// refusal discloses that nothing was found to search.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn without_the_maintainers_tag_the_same_push_is_refused() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let announcer = Keys::generate();
        let response = push_response(
            &m.state,
            m.community,
            &announcer,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            body.contains("Searched 0 mission(s)"),
            "no mission on this channel was founded by a founder: {body}"
        );
    }

    /// The reservation half: the ruling is the announcement signer's, and a
    /// maintainer who founded no mission still lands it. `admit_or_reserve`
    /// compared the pusher with the mission's own founder before this lane.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_maintainer_may_land_a_ruling_the_signer_made() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let maintainer = Keys::generate();
        channel_member(&m, &maintainer).await;
        let mut tags = guarded_repo(m.channel_id);
        tags.push(
            Tag::parse(["maintainers", &maintainer.public_key().to_hex()]).expect("maintainers"),
        );
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            tags,
            &maintainer.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an equal owner lands the other's ruling (body: {body})"
        );
    }

    /// A key with a channel row but no founder standing is still reserved out,
    /// and the refusal now discloses how many founders there are.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_non_founder_member_is_still_reserved_out() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let stranger = Keys::generate();
        channel_member(&m, &stranger).await;
        let response = push_response(
            &m.state,
            m.community,
            &m.founder,
            &format!("repo-{}", Uuid::new_v4().simple()),
            guarded_repo(m.channel_id),
            &stranger.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            format!(
                "refs/heads/main: commit {HEAD_SHA} is approved, but the relay's require-verdict \
                 rule reserves a gated ref to a founder of this repository (1 founder(s): the \
                 announcement's signer, its maintainers tag, and the project roster's owners). \
                 Ask a founder to land it."
            )
        );
    }

    /// The coordinator's case, and the live one: the second founder holds no
    /// `maintainers` row. They are an Owner on the project roster the
    /// announcement's `["project", …]` back-reference names — Andy's
    /// `a56ad5d01` model — and that is what makes their mission's ruling
    /// admit their own push.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_project_roster_owner_founds_the_repository() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let announcer = Keys::generate();
        let dtag = format!("proj-{}", Uuid::new_v4().simple());
        m.state
            .db
            .upsert_project_acl(
                m.community,
                &announcer.public_key().to_bytes(),
                &dtag,
                "public",
                &[(
                    m.founder.public_key().to_bytes().to_vec(),
                    buzz_core::channel::ProjectRole::Owner,
                )],
                1,
            )
            .await
            .expect("project acl");
        let coordinate = format!("30621:{}:{dtag}", announcer.public_key().to_hex());
        let mut tags = guarded_repo(m.channel_id);
        tags.push(Tag::parse(["project", &coordinate]).expect("project"));

        let response = push_response(
            &m.state,
            m.community,
            &announcer,
            &format!("repo-{}", Uuid::new_v4().simple()),
            tags,
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a project Owner founds the repository too (body: {body})"
        );
    }

    /// …and a project *collaborator* does not. The roster grants them push,
    /// not founder standing, so their own mission's ruling admits nothing.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_project_collaborator_does_not_found_the_repository() {
        let m = mission(
            CodingSessionTeamDispositionDecision::Approve,
            Some(HEAD_SHA),
            Some("main"),
        )
        .await;
        let announcer = Keys::generate();
        let dtag = format!("proj-{}", Uuid::new_v4().simple());
        m.state
            .db
            .upsert_project_acl(
                m.community,
                &announcer.public_key().to_bytes(),
                &dtag,
                "public",
                &[(
                    m.founder.public_key().to_bytes().to_vec(),
                    buzz_core::channel::ProjectRole::Collaborator,
                )],
                1,
            )
            .await
            .expect("project acl");
        let coordinate = format!("30621:{}:{dtag}", announcer.public_key().to_hex());
        let mut tags = guarded_repo(m.channel_id);
        tags.push(Tag::parse(["project", &coordinate]).expect("project"));

        let response = push_response(
            &m.state,
            m.community,
            &announcer,
            &format!("repo-{}", Uuid::new_v4().simple()),
            tags,
            &m.founder.public_key().to_hex(),
            fast_forward(HEAD_SHA),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
        assert!(
            body.contains("Searched 0 mission(s)"),
            "a collaborator's mission is not searched: {body}"
        );
    }
}
