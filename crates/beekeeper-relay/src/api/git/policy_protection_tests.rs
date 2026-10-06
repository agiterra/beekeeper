//! The push gate reads a founder's rule record (kind 30625), not only the
//! announcement's own rows — finding 33's residual R2.
//!
//! A child of `gate` so the recipe filter `api::git::policy::tests::gate`
//! reaches these too; every fixture is the parent's, by `use super::*`.

use beekeeper_core::repository_protection::{build_repository_protection, PROTECTION_RULE_CLEAR};
use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;

/// Store a founder-signed rule record for `repo_owner`/`repo_id` directly,
/// bypassing ingest: these cases measure the **push gate's** read, and the
/// write gate has its own cases in `handlers::repo_protection`.
async fn store_rule_record(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    author: &Keys,
    repo_owner_hex: &str,
    repo_id: &str,
    rows: &[&[&str]],
    created_at: u64,
) -> nostr::Event {
    let draft = build_repository_protection(
        repo_owner_hex,
        repo_id,
        &rows
            .iter()
            .map(|row| row.iter().map(|value| (*value).to_string()).collect())
            .collect::<Vec<Vec<String>>>(),
    )
    .expect("a valid draft");
    let tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| Tag::parse(tag.clone()).expect("tag parses"))
        .collect();
    let event = EventBuilder::new(
        Kind::Custom(beekeeper_core::kind::KIND_GIT_REPO_PROTECTION as u16),
        draft.content,
    )
    .tags(tags)
    .custom_created_at(nostr::Timestamp::from(created_at))
    .sign_with_keys(author)
    .expect("signs");
    state
        .db
        .insert_event(community, &event, None)
        .await
        .expect("insert 30625");
    event
}

/// The lane's definition of done: a co-founder who cannot rewrite the
/// announcement sets `require-verdict` on `main` with a record of their own,
/// and the gate honours it against a collaborator's push.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn gate_honours_a_co_founders_rule_record() {
    let f = project_fixture("public").await;
    let repo = fresh_repo();
    // The project's creator is a roster Owner, hence a founder of a
    // repository that back-references the project — but not its announcement
    // signer, so before kind 30625 they had no way to set this rule.
    store_rule_record(
        &f.state,
        f.community,
        &f.creator,
        &f.repo_owner.public_key().to_hex(),
        &repo,
        &[&["refs/heads/main", "require-verdict"]],
        2_000_000_000,
    )
    .await;

    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &repo,
        project_tag(&f.coordinate),
        &f.collaborator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a co-founder's rule record must govern the push (body: {body})"
    );
    assert!(
        body.contains("require-verdict"),
        "the denial must name the rule that produced it: {body}"
    );
}

/// Arm (A) still applies through a record: the founder who set the rule may
/// push under it with no verdict at all.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_founder_still_pushes_under_a_rule_record() {
    let f = project_fixture("public").await;
    let repo = fresh_repo();
    store_rule_record(
        &f.state,
        f.community,
        &f.creator,
        &f.repo_owner.public_key().to_hex(),
        &repo,
        &[&["refs/heads/main", "require-verdict"]],
        2_000_000_000,
    )
    .await;

    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &repo,
        project_tag(&f.coordinate),
        &f.creator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the founder who set the rule is admitted by arm (A): {body}"
    );
}

/// The ruling this lane applies: a co-founder MAY remove protection the
/// signer set. The announcement carries `require-verdict`; the roster Owner
/// clears the pattern with a newer record; the collaborator's push is
/// admitted again.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_co_founder_may_clear_the_signers_rule() {
    let f = project_fixture("public").await;
    let repo = fresh_repo();
    store_rule_record(
        &f.state,
        f.community,
        &f.creator,
        &f.repo_owner.public_key().to_hex(),
        &repo,
        &[&["refs/heads/main", PROTECTION_RULE_CLEAR]],
        2_000_000_000,
    )
    .await;

    let mut tags = project_tag(&f.coordinate);
    tags.push(Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).unwrap());
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &repo,
        tags,
        &f.collaborator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a founder's clear must retire the signer's rule (body: {body})"
    );
}

/// A stranger's rule record must not govern anything. The write gate refuses
/// one, but a record whose author was a founder when it was written and is
/// not one now is a real state, so the **read** filters by the founder set
/// too rather than trusting that the write gate was the only door.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_non_founders_rule_record_is_ignored_by_the_gate() {
    let f = project_fixture("public").await;
    let repo = fresh_repo();
    store_rule_record(
        &f.state,
        f.community,
        &f.viewer,
        &f.repo_owner.public_key().to_hex(),
        &repo,
        &[&["refs/heads/main", "require-verdict"]],
        2_000_000_000,
    )
    .await;

    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &repo,
        project_tag(&f.coordinate),
        &f.collaborator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a viewer is no founder; their record must not govern (body: {body})"
    );
}

/// A newer announcement beats an older record, so a stale record cannot
/// resurrect a rule the repository's own signer later dropped.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_older_record_loses_to_the_announcement() {
    let f = project_fixture("public").await;
    let repo = fresh_repo();
    store_rule_record(
        &f.state,
        f.community,
        &f.creator,
        &f.repo_owner.public_key().to_hex(),
        &repo,
        &[&["refs/heads/main", "require-verdict"]],
        // Well before the announcement `push_response` signs with `now`.
        1_000,
    )
    .await;

    let mut tags = project_tag(&f.coordinate);
    tags.push(Tag::parse(["buzz-protect", "refs/heads/main", "no-delete"]).unwrap());
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &repo,
        tags,
        &f.collaborator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the newer announcement governs the pattern (body: {body})"
    );
}

/// Finding 31's mandatory case, on the relay's own reader: a repository whose
/// rules were signed before kind 30625 existed — no record anywhere — is
/// governed by its announcement exactly as it always was.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn rules_signed_before_the_kind_existed_still_govern() {
    let f = project_fixture("public").await;
    let mut tags = project_tag(&f.coordinate);
    tags.push(Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).unwrap());
    let response = push_response(
        &f.state,
        f.community,
        &f.repo_owner,
        &fresh_repo(),
        tags,
        &f.collaborator.public_key().to_hex(),
        create_main(),
    )
    .await;
    let (status, body) = body_string(response).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an announcement-only repository must still be governed (body: {body})"
    );
    assert!(body.contains("require-verdict"), "{body}");
}

/// A repository with no rule record and no protection rows issues the same
/// ungoverned push it always did — the cost claim, stated as a test.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_ungoverned_push_is_unchanged() {
    let f = project_fixture("public").await;
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
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// A kind-5 tombstone by a record's own author retires that founder's rows:
/// the relay's ordinary addressable deletion, which the gate inherits by
/// reading through the same query every other reader uses.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_tombstone_retires_that_founders_rows() {
    let f = project_fixture("public").await;
    let repo = fresh_repo();
    // Dated in the recent past, deliberately: NIP-09 scopes an a-tag deletion
    // to versions at or before the tombstone's own `created_at`, so a record
    // dated in the future cannot be tombstoned until that time arrives. The
    // announcement here carries no rows at all, so the record wins
    // `refs/heads/main` on any timestamp and needs no future date to do it.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    // The tombstone names the record's *coordinate*, not its id, so the
    // event itself is not needed here.
    store_rule_record(
        &f.state,
        f.community,
        &f.creator,
        &f.repo_owner.public_key().to_hex(),
        &repo,
        &[&["refs/heads/main", "require-verdict"]],
        now - 60,
    )
    .await;

    // Governed first — otherwise the assertion after the delete proves
    // nothing about the delete.
    let (status, body) = body_string(
        push_response(
            &f.state,
            f.community,
            &f.repo_owner,
            &repo,
            project_tag(&f.coordinate),
            &f.collaborator.public_key().to_hex(),
            create_main(),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // The real path, not a soft delete behind ingest's back: a kind-5 naming
    // the record's own addressable coordinate, signed by its author. The
    // coordinate carries two colons of its own (`30625:<author>:<owner>:<id>`)
    // and ingest splits on the first two only, which is what keeps a
    // colon-bearing address deletable.
    let coordinate = format!(
        "{}:{}:{}",
        beekeeper_core::kind::KIND_GIT_REPO_PROTECTION,
        f.creator.public_key().to_hex(),
        beekeeper_core::repository_protection::repository_protection_d_tag(
            &f.repo_owner.public_key().to_hex(),
            &repo,
        )
    );
    let tombstone = EventBuilder::new(nostr::Kind::EventDeletion, "")
        .tags(vec![Tag::parse(["a", &coordinate]).expect("a")])
        .sign_with_keys(&f.creator)
        .expect("signs");
    crate::handlers::ingest::ingest_event(
        &f.state,
        &beekeeper_core::tenant::TenantContext::resolved(f.community, f.host.clone()),
        tombstone,
        crate::handlers::ingest::IngestAuth::Http {
            pubkey: f.creator.public_key(),
            scopes: vec![
                beekeeper_auth::Scope::ReposWrite,
                beekeeper_auth::Scope::MessagesWrite,
                beekeeper_auth::Scope::ChannelsWrite,
            ],
            auth_method: crate::handlers::ingest::HttpAuthMethod::Nip98,
        },
    )
    .await
    .expect("the author's own tombstone is accepted");

    let (status, body) = body_string(
        push_response(
            &f.state,
            f.community,
            &f.repo_owner,
            &repo,
            project_tag(&f.coordinate),
            &f.collaborator.public_key().to_hex(),
            create_main(),
        )
        .await,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a tombstoned record must stop governing (body: {body})"
    );
}
