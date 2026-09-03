//! The Postgres-backed `hook_policy_check` cases: the binding gate, the
//! project/channel roster grants, and the batch-3 inheritance cap.
//!
//! Split out of `policy_tests.rs` for size alone. Every fixture these use
//! lives in the parent module, so `use super::*` reaches both it and
//! `policy.rs` itself.

use super::*;

/// The tri-state trap the resolver exists to prevent: a broken (malformed
/// or ambiguous-first) binding must fail closed for EVERYONE on push —
/// including the announcement author — *before* the owner short-circuit
/// grants `MemberRole::Owner`. Collapsing `Broken` into "unbound" hands
/// the owner a push path through a binding the read gate refuses to
/// honor. The remediation token stays reserved for genuinely NotBound.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_denies_owner_through_broken_binding() {
    use nostr::{Keys, Tag};

    let state = policy_test_state().await;
    let host = format!("policy-{}.example", uuid::Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;
    let keys = Keys::generate();

    // Malformed first + valid-looking second: the ambiguity must deny,
    // and the parseable duplicate must not rescue the push.
    let response = owner_push_response(
        &state,
        community,
        &keys,
        &format!("repo-{}", uuid::Uuid::new_v4().simple()),
        vec![
            Tag::parse(["buzz-channel", "not-a-uuid"]).unwrap(),
            Tag::parse(["buzz-channel", &uuid::Uuid::new_v4().to_string()]).unwrap(),
        ],
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body, "invalid channel binding",
        "owner pushing through a broken binding must be denied generically"
    );
    assert!(
        !body.contains(buzz_core::git_perms::GIT_NO_CHANNEL_BINDING_TOKEN),
        "remediation token is NotBound-only; Broken must never earn it"
    );

    // Control: the same owner pushing a genuinely NEVER-BOUND repo is
    // allowed (owner authority over an unbound announcement is the
    // long-standing push semantics). This pins the denial above to
    // Broken specifically, not to some broader regression.
    let response = owner_push_response(
        &state,
        community,
        &keys,
        &format!("repo-{}", uuid::Uuid::new_v4().simple()),
        vec![],
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "owner push to a never-bound repo must remain allowed (got body: {body})"
    );
}

/// The whole point of the change: a repo with **no** `buzz-channel` tag
/// at all is pushable by the project's roster. Owner pushes as Owner,
/// collaborator as Member, viewer not at all.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_grants_channel_less_repo_from_the_project_roster() {
    let f = project_fixture("public").await;

    for (label, keys) in [
        ("project creator", &f.creator),
        ("collaborator", &f.collaborator),
    ] {
        let response = push_response(
            &f.state,
            f.community,
            &f.repo_owner,
            &fresh_repo(),
            project_tag(&f.coordinate),
            &keys.public_key().to_hex(),
            create_main(),
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{label} must be able to push a channel-less repo in their project (body: {body})"
        );
    }

    // A viewer is read-only across the project. With no channel binding
    // to fall back on, they have no grant at all.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &f.viewer.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "not a project member");

    // And a stranger gets the same denial — never the remediation token,
    // which would tell them to bind a channel this repo does not need.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &nostr::Keys::generate().public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "not a project member");
    assert!(
        !body.contains(buzz_core::git_perms::GIT_NO_CHANNEL_BINDING_TOKEN),
        "a repo inside a project must never be told to bind a channel"
    );
}

/// Project *visibility* is about the event surface, not about granting.
/// A private project's roster pushes exactly like a public one's — and
/// neither makes a non-member able to push.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_project_grant_is_visibility_agnostic() {
    let f = project_fixture("private").await;

    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &f.collaborator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a private project's collaborator must push too (body: {body})"
    );

    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &nostr::Keys::generate().public_key().to_hex(),
        create_main(),
    )
    .await;
    assert_eq!(body_string(response).await.0, StatusCode::FORBIDDEN);
}

/// The two ACLs are additive and neither may demote the other. Both
/// directions are tested with a **force push**, which needs `Admin`, so
/// the assertion is about the resulting *tier* rather than about being
/// allowed to push at all.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_takes_the_more_permissive_of_project_and_channel() {
    use buzz_core::channel::MemberRole;

    let f = project_fixture("public").await;

    // The channel is created by a third party, so the project creator can
    // hold the Guest role here without tripping the last-owner guard.
    let channel_creator = nostr::Keys::generate();
    let channel_creator_pk = channel_creator.public_key().to_bytes().to_vec();
    f.state
        .db
        .ensure_user(f.community, &channel_creator_pk)
        .await
        .expect("user");

    // One channel, two members: the project's viewer joins as a channel
    // Admin; the project's creator joins as a channel Guest.
    let channel = uuid::Uuid::new_v4();
    f.state
        .db
        .create_channel_with_id(
            f.community,
            channel,
            &format!("ch-{}", channel.simple()),
            buzz_db::channel::ChannelType::Stream,
            buzz_db::channel::ChannelVisibility::Open,
            None,
            &channel_creator_pk,
            None,
            None,
        )
        .await
        .expect("channel");
    for (keys, role) in [
        (&f.viewer, MemberRole::Admin),
        (&f.creator, MemberRole::Guest),
    ] {
        let pk = keys.public_key().to_bytes().to_vec();
        f.state
            .db
            .ensure_user(f.community, &pk)
            .await
            .expect("user");
        f.state
            .db
            .add_member(f.community, channel, &pk, role, Some(&channel_creator_pk))
            .await
            .expect("member");
    }

    let both_tags = vec![
        nostr::Tag::parse(["project", &f.coordinate]).unwrap(),
        nostr::Tag::parse(["buzz-channel", &channel.to_string()]).unwrap(),
    ];

    // Project Viewer (no grant) + channel Admin ⇒ Admin. If the project
    // path shadowed the channel path, this would deny.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        both_tags.clone(),
        &f.viewer.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a channel Admin must not be demoted by also being a project viewer (body: {body})"
    );

    // Project Owner + channel Guest ⇒ Owner. If the channel path won, or
    // the two were min()'d, this would deny.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        both_tags,
        &f.creator.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a project owner must not be demoted by also being a channel guest (body: {body})"
    );
}

/// A hired seat signs git as itself and holds no roster row of its own.
/// Its push must resolve through the owner it is attested to — otherwise
/// the read gate lets the seat in and the pre-receive hook throws it out.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_grants_a_seat_its_owners_project_role() {
    let f = project_fixture("public").await;
    let seat = nostr::Keys::generate();

    // Unattested, the seat is a stranger.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &seat.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "not a project member");

    // Attested to a collaborator, it pushes at the collaborator's tier.
    seat_of(&f.state, f.community, &seat, &f.collaborator).await;
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &seat.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a seat attested to a collaborator must be able to push (body: {body})"
    );
}

/// The seat inherits its owner's *tier*, not merely permission to push.
/// A viewer's seat pushes nothing; an owner's seat force-pushes.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_gives_a_seat_exactly_its_owners_tier() {
    let f = project_fixture("public").await;

    let viewer_seat = nostr::Keys::generate();
    seat_of(&f.state, f.community, &viewer_seat, &f.viewer).await;
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &viewer_seat.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a viewer's seat is read-only"
    );
    assert_eq!(body, "not a project member");

    // A collaborator maps to Member, and force-push needs Admin, so a
    // collaborator's seat must be refused the force-push too.
    let collab_seat = nostr::Keys::generate();
    seat_of(&f.state, f.community, &collab_seat, &f.collaborator).await;
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &collab_seat.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, _) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a collaborator's seat must not force-push; the tier travels with the grant"
    );

    // The project creator holds Owner — and since batch 3 lane L6 the seat
    // does NOT inherit it. This assertion used to read `OK`; that is the
    // exact authority leak live run 3 found (finding 27), where a hired
    // lead held the repo owner's tier and landed unverified work on
    // `main`. An inherited role is capped at Member.
    let owner_seat = nostr::Keys::generate();
    seat_of(&f.state, f.community, &owner_seat, &f.creator).await;
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &owner_seat.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an inherited Owner role is capped at Member; force-push needs Admin (body: {body})"
    );
    assert_eq!(
        body, "refs/heads/main: requires admin role (you have member), using built-in defaults",
        "the denial body now carries one readable ref-prefixed line"
    );

    // The same seat still fast-forwards an ordinary branch: Member is a
    // real grant, and a seat that cannot push at all is useless.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &owner_seat.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an inherited Member still creates and fast-forwards branches (body: {body})"
    );

    // The creator's OWN key is untouched: the cap is on inheritance, not
    // on the grant.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        project_tag(&f.coordinate),
        &f.creator.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the project owner's own key force-pushes exactly as before (body: {body})"
    );
}

/// The repo owner's own seat keeps a push path with no roster row
/// anywhere — that is what makes `git push origin` work for a seat hired
/// into its operator's checkout — but on INHERITED authority, which since
/// batch 3 lane L6 is capped at Member **on a guarded ref**
/// (`refs/heads/main`, or any ref carrying a `buzz-protect` rule).
///
/// The force-push assertion here used to read `OK`, and that is the hole
/// this closes: a seat holding its operator's Owner grant could rewrite the
/// trunk. **It is not the hole that landed unverified code.** Run 3's push
/// was a *fast-forward* (`1dd98e876..07c470be0`), which needs only Member and
/// is still allowed below — see
/// `verdict_admission_tests::live::a_seats_fast_forward_of_main_is_refused_once_the_rule_is_set`
/// for the rule that does refuse it, and for the same push succeeding while
/// no rule is set.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_caps_the_repo_owners_seat_at_member() {
    let f = project_fixture("public").await;
    let seat = nostr::Keys::generate();
    seat_of(&f.state, f.community, &seat, &f.repo_owner).await;

    // No channel binding and no project tag at all: only the attestation
    // to the repo owner can carry this push, so it isolates inheritance.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        vec![],
        &seat.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a seat must not force-push on its operator's authority (body: {body})"
    );
    assert_eq!(
        body, "refs/heads/main: requires admin role (you have member), using built-in defaults",
        "the denial names the ref, the tier it wanted and the tier the seat holds"
    );

    // Create and fast-forward are unchanged — Member covers both, and a
    // capped seat still holds Member. This is exactly why the cap does not,
    // on its own, close finding 27.
    for update in [create_main(), fast_forward_main()] {
        let response = push_response(
            &f.state,
            f.community,
            &f.repo_owner,
            &fresh_repo(),
            vec![],
            &seat.public_key().to_hex(),
            update,
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the repo owner's seat still pushes branches (body: {body})"
        );
    }

    // Deleting a ref is Admin too, and inheritance no longer reaches it.
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        vec![],
        &seat.public_key().to_hex(),
        delete_main(),
    )
    .await;
    let (status, _) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a seat must not delete a ref on its operator's authority"
    );

    // The repo owner's OWN key is untouched on every update kind.
    for update in [
        create_main(),
        fast_forward_main(),
        force_push_main(),
        delete_main(),
    ] {
        let response = push_response(
            &f.state,
            f.community,
            &f.repo_owner,
            &fresh_repo(),
            vec![],
            &f.repo_owner.public_key().to_hex(),
            update,
        )
        .await;
        let (status, body) = body_string(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the announcement author keeps owner authority (body: {body})"
        );
    }
}

/// A seat's OWN roster row is not inherited and is not capped: a seat
/// added to the channel as Admin is Admin by its own grant.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_leaves_a_seats_own_admin_row_uncapped() {
    let f = project_fixture("public").await;
    let seat = nostr::Keys::generate();
    seat_of(&f.state, f.community, &seat, &f.repo_owner).await;

    let channel_id = uuid::Uuid::new_v4();
    f.state
        .db
        .create_channel_with_id(
            f.community,
            channel_id,
            &format!("seat-{}", channel_id.simple()),
            buzz_core::channel::ChannelType::Stream,
            buzz_core::channel::ChannelVisibility::Open,
            None,
            &f.repo_owner.public_key().to_bytes(),
            None,
            None,
        )
        .await
        .expect("channel");
    f.state
        .db
        .ensure_user(f.community, &seat.public_key().to_bytes())
        .await
        .expect("user");
    f.state
        .db
        .add_member(
            f.community,
            channel_id,
            &seat.public_key().to_bytes(),
            MemberRole::Admin,
            Some(&f.repo_owner.public_key().to_bytes()),
        )
        .await
        .expect("admin row");

    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        vec![nostr::Tag::parse(["buzz-channel", &channel_id.to_string()]).unwrap()],
        &seat.public_key().to_hex(),
        force_push_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a seat holding its own Admin row force-pushes on that grant (body: {body})"
    );
}

/// The remediation token's contract narrows but does not move: it still
/// fires, byte-identical, for a repo that names neither a channel nor a
/// project — the vanilla-NIP-34-client case from issue #3527.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_still_emits_the_remediation_token_for_a_repo_with_no_acl_at_all() {
    let f = project_fixture("public").await;
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        vec![],
        &nostr::Keys::generate().public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, GIT_NO_CHANNEL_BINDING_BODY);
}

/// **The cap is scoped, and this is the workflow that made it so.**
///
/// `CLAUDE.md` ("Topic branches are rebased onto `main`… Force-push the topic
/// branch afterwards; that is expected"), `docs/CREW_SESSIONS_PLAN.md` (the
/// finalizer seat "rebases lanes onto `main` … force-pushes the topic
/// branch") and `docs/INTEGRATION.md` (`git push --force-with-lease`) all put
/// exactly these operations on a hired seat. A blanket inherited-authority cap
/// broke all three silently; fix round 1 scopes the cap to guarded refs, so a
/// seat rewrites and deletes its own lane branch exactly as before.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn push_gate_lets_a_seat_rewrite_and_delete_its_own_lane_branch() {
    let f = project_fixture("public").await;
    let seat = nostr::Keys::generate();
    seat_of(&f.state, f.community, &seat, &f.repo_owner).await;

    let lane = |update: HookRefUpdate, ref_name: &str| {
        let mut update = update;
        update.ref_name = ref_name.to_string();
        update
    };

    // No protection rules at all: only `refs/heads/main` is guarded.
    for ref_name in ["refs/heads/lane/batch3-l6", "refs/heads/wip/spike"] {
        for update in [
            create_main(),
            fast_forward_main(),
            force_push_main(),
            delete_main(),
        ] {
            let response = push_response(
                &f.state,
                f.community,
                &f.repo_owner,
                &fresh_repo(),
                vec![],
                &seat.public_key().to_hex(),
                lane(update, ref_name),
            )
            .await;
            let (status, body) = body_string(response).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "a seat rebases and deletes {ref_name} on its operator's grant (body: {body})"
            );
        }
    }

    // A ref an operator explicitly protected IS guarded, even off `main`: a
    // `buzz-protect` tag is a person saying "this history is shared".
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        vec![
            nostr::Tag::parse(["buzz-protect", "refs/heads/release", "no-delete"])
                .expect("protect"),
        ],
        &seat.public_key().to_hex(),
        lane(force_push_main(), "refs/heads/release"),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an explicitly protected ref is guarded wherever it is (body: {body})"
    );
}
