//! **Finding 56**: where the gate looks for a mission.
//!
//! Live run 4 (2026-09-03, 17:40) refused a seat's push of a commit its own
//! mission had watched green with "Searched 0 mission(s) — the newest 16 on
//! this channel". The founder set was right; the *lookup* was wrong. Every
//! coding session lives in its own channel, and the gate only ever read the
//! channel the repository is bound to, so arms (B) and (C) could never find a
//! real mission.
//!
//! These cases go through the real `git-receive-pack` policy handler, and each
//! one binds the repository somewhere **other** than the mission's channel —
//! the live shape.
//!
//! **Finding 91 narrowed step 1.** The seat lookup no longer returns the
//! pusher's seats community-wide: it is intersected with the channels this
//! repository grants (its project's session channels ∪ the channel it binds).
//! So the live shape here is *repository names the project the mission lives
//! in, and binds some other channel* — which is the shape live run 4 actually
//! had. A repository that grants **nothing** the mission is in is no longer a
//! repository that mission can land on, and
//! [`a_seat_of_a_mission_this_repository_does_not_grant_is_not_searched`] is
//! that case.

use super::*;

use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
use beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload;
use beekeeper_core::coding_session_observation::CodingSessionObservationSource;
use beekeeper_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_GENESIS};
use nostr::{EventBuilder, Keys, Kind, Tag};
use uuid::Uuid;

use super::observed_tests::{default_green, watched, Watched, HEAD_SHA};
use crate::api::git::policy::tests::{body_string, push_response, seat_of};
use crate::api::git::policy::HookRefUpdate;

/// A second, empty channel in the same community: somewhere for a repository
/// to be bound that is not the mission's own channel.
async fn other_channel(w: &Watched) -> Uuid {
    let channel_id = Uuid::new_v4();
    w.state
        .db
        .create_channel_with_id(
            w.community,
            channel_id,
            &format!("repo-home-{}", channel_id.simple()),
            beekeeper_core::channel::ChannelType::Stream,
            beekeeper_core::channel::ChannelVisibility::Open,
            None,
            &w.founder.public_key().to_bytes(),
            None,
            None,
        )
        .await
        .expect("channel");
    channel_id
}

/// The announcement of a repository bound to `channel_id` with `main` gated.
fn guarded_repo_bound_to(channel_id: Uuid) -> Vec<Tag> {
    vec![
        Tag::parse(["buzz-channel", &channel_id.to_string()]).expect("binding"),
        Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("protect"),
    ]
}

/// The same announcement, back-referencing `project` as well — the live shape
/// since finding 91: a repository grants the session channels of its project,
/// and the seat lookup is narrowed to them.
fn guarded_repo_in_project(channel_id: Uuid, project: &str) -> Vec<Tag> {
    let mut tags = guarded_repo_bound_to(channel_id);
    tags.push(Tag::parse(["project", project]).expect("project"));
    tags
}

/// A project coordinate owned by `w`'s founder.
fn project_of(w: &Watched) -> String {
    format!("30621:{}:beekeeper", w.founder.public_key().to_hex())
}

/// A second mission, in its own channel, founded by `founder`, seating the
/// same key as `w` and watched by the same provider.
async fn mission_founded_by(w: &Watched, founder: &Keys) -> Watched {
    w.state
        .db
        .ensure_user(w.community, &founder.public_key().to_bytes())
        .await
        .expect("user");
    let channel_id = Uuid::new_v4();
    w.state
        .db
        .create_channel_with_id(
            w.community,
            channel_id,
            &format!("mission-{}", channel_id.simple()),
            beekeeper_core::channel::ChannelType::Stream,
            beekeeper_core::channel::ChannelVisibility::Open,
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
    .sign_with_keys(founder)
    .expect("sign genesis");
    w.state
        .db
        .insert_event(w.community, &genesis, Some(channel_id))
        .await
        .expect("insert genesis");
    let sibling = Watched {
        state: w.state.clone(),
        community: w.community,
        channel_id,
        session_ref,
        genesis_ref: genesis.id.to_hex(),
        founder: founder.clone(),
        provider: w.provider.clone(),
        seat: w.seat.clone(),
    };
    sibling.commission_the_provider().await;
    sibling.publish_provider_metadata().await;
    // The seat is already attested to `w`'s founder, so `grant_the_seat`'s
    // "newly attested" fixture assertion cannot run twice for one key; the
    // 44228 grant is the half this lookup reads.
    grant_seat_in(&sibling, 1).await;
    sibling
}

/// Seat `w.seat` as a builder of `w`'s mission, on the wire.
async fn grant_seat_in(w: &Watched, seq: u32) {
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_seat(
        w.genesis_ref.clone(),
        None,
        seq,
        w.seat.public_key().to_hex(),
        "builder",
    );
    let event =
        beekeeper_sdk::builders::build_coding_session_authority_transition(w.channel_id, &payload)
            .expect("transition builder")
            .sign_with_keys(&w.founder)
            .expect("sign transition");
    w.state
        .db
        .insert_event(w.community, &event, Some(w.channel_id))
        .await
        .expect("insert transition");
}

/// **The live refusal, as a test.** The mission is in its own channel; the
/// repository is bound to another one; the seat pushes the very commit its
/// provider watched three gates pass on. Before this lane the gate searched
/// only the bound channel and answered "Searched 0 mission(s)".
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_seats_push_is_judged_by_the_mission_that_seated_it() {
    let w = watched().await;
    let project = project_of(&w);
    let mission = mission_in_project(&w, &project).await;
    mission.commission_the_provider().await;
    grant_seat_in(&mission, 1).await;
    mission
        .observe(
            &mission.provider,
            CodingSessionObservationSource::Observed,
            default_green(HEAD_SHA),
        )
        .await;
    let elsewhere = other_channel(&w).await;

    let (status, body) = w
        .seat_push_announced_as(HEAD_SHA, guarded_repo_in_project(elsewhere, &project))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the gate must find the mission that seated the pusher in this repository's own \
         project, not the repository's bound channel (body: {body})"
    );
}

/// **Finding 91.** The same seat, the same green rows, the same commit — and a
/// repository that grants neither the mission's channel nor a project holding
/// it. Before this lane the seat lookup swept the community and the mission
/// admitted the push on nothing but a shared founder.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_seat_of_a_mission_this_repository_does_not_grant_is_not_searched() {
    let w = watched().await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;
    let elsewhere = other_channel(&w).await;

    let (status, body) = w
        .seat_push_announced_as(HEAD_SHA, guarded_repo_bound_to(elsewhere))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a mission on a channel this repository grants nothing to proves nothing about it \
         (body: {body})"
    );
    assert!(
        body.contains("none of them is on a mission of this repository's own channels"),
        "the refusal must say the seat lookup was narrowed, not that the key holds no seat \
         anywhere (body: {body})"
    );
}

/// The same lookup must not become a way in. A mission founded by somebody who
/// founds no part of this repository seats the pusher and watches its gates
/// green — and the push is still refused.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_seat_of_a_strangers_mission_is_still_refused() {
    let w = watched().await;
    let stranger = Keys::generate();
    let theirs = mission_founded_by(&w, &stranger).await;
    theirs
        .observe(
            &theirs.provider,
            CodingSessionObservationSource::Observed,
            default_green(HEAD_SHA),
        )
        .await;
    let elsewhere = other_channel(&w).await;

    // `w`'s own mission publishes nothing, so the only green rows anywhere
    // name the stranger's mission.
    let (status, body) = w
        .seat_push_announced_as(
            HEAD_SHA,
            guarded_repo_in_project(elsewhere, &project_of(&theirs)),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a mission nobody who founds this repository founded admits nothing (body: {body})"
    );
}

/// The refusal says which lookup ran, so a person reading it can tell "your
/// mission had nothing to say" from "the gate never looked at your mission".
#[tokio::test]
#[ignore = "requires Postgres"]
async fn the_refusal_names_the_lookup_that_ran() {
    let w = watched().await;
    let project = project_of(&w);
    let mission = mission_in_project(&w, &project).await;
    grant_seat_in(&mission, 1).await;
    let elsewhere = other_channel(&w).await;

    let (status, body) = w
        .seat_push_announced_as(HEAD_SHA, guarded_repo_in_project(elsewhere, &project))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no rows exist yet");
    let short = &w.seat.public_key().to_hex()[..8];
    assert!(
        body.contains(&format!("seat(s) held by {short}")),
        "the refusal must name the seat lookup it ran (body: {body})"
    );
    assert!(
        body.contains("that lie in"),
        "and must say what the seats were narrowed to (finding 91) (body: {body})"
    );
}

/// Take the seat away again. The grant stays on the wire — the chain is
/// append-only — so the lookup has to read the *newest* transition for this
/// key, not merely find a grant somewhere in the page.
///
/// The revocation links to the real grant: a payload whose `seq` is 2 with a
/// null `prevAccepted` is refused by the builder, and a fixture that faked the
/// link would be testing a chain the relay would never have accepted.
async fn revoke_the_seat(w: &Watched) {
    let grant = w
        .state
        .db
        .query_events(&beekeeper_db::EventQuery {
            kinds: Some(vec![KIND_CODING_SESSION_AUTHORITY_TRANSITION as i32]),
            channel_id: Some(w.channel_id),
            limit: Some(8),
            ..beekeeper_db::EventQuery::for_community(w.community)
        })
        .await
        .expect("read the grant back")
        .into_iter()
        .next()
        .expect("the fixture granted a seat");
    let payload = CodingSessionAuthorityTransitionPayload::new_revoke_seat(
        w.genesis_ref.clone(),
        Some(grant.event.id.to_hex()),
        2,
        w.seat.public_key().to_hex(),
        "builder",
    );
    let event =
        beekeeper_sdk::builders::build_coding_session_authority_transition(w.channel_id, &payload)
            .expect("transition builder")
            .sign_with_keys(&w.founder)
            .expect("sign transition");
    w.state
        .db
        .insert_event(w.community, &event, Some(w.channel_id))
        .await
        .expect("insert transition");
}

/// A seat that was revoked is not a seat, and its mission is not searched: the
/// refusal names the *bound-channel* fallback, which is what the gate falls
/// back to for a key holding nothing.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_revoked_seat_no_longer_chooses_the_mission() {
    let w = watched().await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;
    revoke_the_seat(&w).await;
    let elsewhere = other_channel(&w).await;

    let (status, body) = w
        .seat_push_announced_as(HEAD_SHA, guarded_repo_bound_to(elsewhere))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "green rows do not survive the seat that earned them (body: {body})"
    );
    assert!(
        body.contains("fell back to the channel it is bound to"),
        "a revoked key holds no seat, so the seat lookup must not claim it ran (body: {body})"
    );
}

// ── the project fallback ─────────────────────────────────────────────────

/// A mission in a **transport** channel that the project `project_ref` owns,
/// founded by `w`'s founder, watched by `w`'s provider — and seating nobody.
///
/// No 44228 grant on purpose: the project lookup is what a key holding no seat
/// falls back to, and a grant here would make the seat lookup win instead.
async fn mission_in_project(w: &Watched, project_ref: &str) -> Watched {
    let channel_id = Uuid::new_v4();
    w.state
        .db
        .create_channel_with_id(
            w.community,
            channel_id,
            &format!("session-{}", channel_id.simple()),
            beekeeper_core::channel::ChannelType::Transport,
            beekeeper_core::channel::ChannelVisibility::Open,
            None,
            &w.founder.public_key().to_bytes(),
            None,
            Some(project_ref),
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
    .sign_with_keys(&w.founder)
    .expect("sign genesis");
    w.state
        .db
        .insert_event(w.community, &genesis, Some(channel_id))
        .await
        .expect("insert genesis");
    let mission = Watched {
        state: w.state.clone(),
        community: w.community,
        channel_id,
        session_ref,
        genesis_ref: genesis.id.to_hex(),
        founder: w.founder.clone(),
        provider: w.provider.clone(),
        seat: w.seat.clone(),
    };
    mission.commission_the_provider().await;
    mission.publish_provider_metadata().await;
    mission
}

/// One push by `pusher` against a repository that names `project` and is bound
/// to nothing.
async fn project_push(w: &Watched, project: &str, pusher: &Keys) -> (StatusCode, String) {
    let response = push_response(
        &w.state,
        w.community,
        &w.founder,
        &format!("repo-{}", Uuid::new_v4().simple()),
        vec![
            Tag::parse(["project", project]).expect("project"),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("protect"),
        ],
        &pusher.public_key().to_hex(),
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

/// A key holding no seat pushes a repository that names a project. The search
/// falls back to that project's session channels, **finds the mission**, and
/// says so — where the old lookup read the bound channel and reported zero.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn the_project_fallback_finds_the_missions_of_the_project() {
    let w = watched().await;
    let project = format!("30621:{}:beekeeper", w.founder.public_key().to_hex());
    // The mission exists and has published nothing, so the answer is the
    // no-verdict sentence — the one that carries the count.
    let _mission = mission_in_project(&w, &project).await;

    // Attested to the repository owner, so the role check lets it reach the
    // gate — and holding no mission seat of any kind.
    let outsider = Keys::generate();
    seat_of(&w.state, w.community, &outsider, &w.founder).await;

    let (status, body) = project_push(&w, &project, &outsider).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "nothing ruled (body: {body})"
    );
    assert!(
        body.contains("so the search fell back to the project this repository names"),
        "the fallback names itself (body: {body})"
    );
    assert!(
        body.contains(&format!("session channel(s) of {project}")),
        "and names the project it read (body: {body})"
    );
    assert!(
        body.contains("Searched 1 mission(s)"),
        "the project's session channel holds the mission, so the count is not zero (body: {body})"
    );
}

/// The same fallback, with the mission's gates observed green on the commit.
///
/// The push is still refused — arms (B) and (C) both want an active seat and
/// this key holds none — but the refusal is now about **that mission**, which
/// is the whole point: the gate read the work instead of an empty channel.
/// This is also the recovery path when a seat's own 44228 grant has fallen
/// outside the newest page of transitions.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn the_project_fallback_reads_that_missions_gate_rows() {
    let w = watched().await;
    let project = format!("30621:{}:beekeeper", w.founder.public_key().to_hex());
    let mission = mission_in_project(&w, &project).await;
    mission
        .observe(
            &mission.provider,
            CodingSessionObservationSource::Observed,
            default_green(HEAD_SHA),
        )
        .await;
    let outsider = Keys::generate();
    seat_of(&w.state, w.community, &outsider, &w.founder).await;

    let (status, body) = project_push(&w, &project, &outsider).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a key holding no seat lands nothing (body: {body})"
    );
    assert!(
        body.contains(&mission.session_ref),
        "the refusal is about the mission the project lookup found (body: {body})"
    );
    assert!(
        body.contains("is not an active seat of it"),
        "and the missing fact is the seat, not the ruling (body: {body})"
    );
}
