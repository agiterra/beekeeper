//! Who may delete a project-scoped addressable event (NIP-09 `a`-tag path).
//!
//! These pin the third admission arm added to
//! [`validate_standard_deletion_event`]: an Owner of the project that
//! *contains* a target may delete it, even though they did not sign it.
//! Every case is written as the property it protects, because each one of
//! them widens an authorization gate:
//!
//! * a roster Owner reaches the project head, a repo announcement and a
//!   shared-terminal announce inside their project;
//! * the project creator reaches the same three — creator and roster Owner
//!   are one tier, which is the whole point of the change;
//! * a Collaborator reaches only what they themselves signed;
//! * a Viewer and a stranger reach nothing;
//! * an Owner of a *different* project reaches nothing;
//! * an unlinked resource falls back to authorship, so the new arm can never
//!   widen anything outside a project.
//!
//! The refusal message is asserted verbatim throughout: `must be event
//! author` is what every pre-existing caller and test expects, and a denial
//! that changes its wording is a behaviour change even when the decision is
//! identical.

use buzz_core::channel::ProjectRole;
use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;

const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

async fn deletion_test_state() -> Arc<AppState> {
    let mut config = crate::config::Config::from_env().expect("default config loads");
    config.require_relay_membership = false;
    config.redis_url = "redis://127.0.0.1:1".to_string();
    config.database_url = std::env::var("BUZZ_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| TEST_DB_URL.to_string());
    let pool = sqlx::PgPool::connect(&config.database_url)
        .await
        .expect("connect test DB");
    let db = buzz_db::Db::from_pool(pool.clone());
    let redis_pool = deadpool_redis::Config::from_url(&config.redis_url)
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("redis pool");
    let pubsub = Arc::new(
        buzz_pubsub::PubSubManager::new(&config.redis_url, redis_pool.clone())
            .await
            .expect("pubsub manager"),
    );
    let audit = buzz_audit::AuditService::new(pool.clone());
    let auth = buzz_auth::AuthService::new(config.auth.clone());
    let search = buzz_search::SearchService::new(pool.clone());
    let workflow_engine = Arc::new(buzz_workflow::WorkflowEngine::new(
        db.clone(),
        buzz_workflow::WorkflowConfig::default(),
    ));
    let media_storage = buzz_media::MediaStorage::new(&config.media).expect("media storage");
    let (state, _audit_shutdown) = AppState::new(
        config,
        db,
        redis_pool,
        audit,
        pubsub,
        auth,
        search,
        workflow_engine,
        Keys::generate(),
        media_storage,
    );
    Arc::new(state)
}

/// One project with all four tiers seated, in its own community so tests
/// never observe each other's rows.
struct DeletionFixture {
    state: Arc<AppState>,
    tenant: TenantContext,
    dtag: String,
    coordinate: String,
    creator: Keys,
    owner: Keys,
    collaborator: Keys,
    viewer: Keys,
    stranger: Keys,
}

impl DeletionFixture {
    async fn new() -> Self {
        let state = deletion_test_state().await;
        let host = format!("delete-mp-{}.example", uuid::Uuid::new_v4().simple());
        let community = state
            .db
            .ensure_configured_community(&host)
            .await
            .expect("community")
            .id;
        let tenant = TenantContext::resolved(community, host);

        let creator = Keys::generate();
        let owner = Keys::generate();
        let collaborator = Keys::generate();
        let viewer = Keys::generate();
        let stranger = Keys::generate();

        let dtag = format!("proj-{}", uuid::Uuid::new_v4().simple());
        state
            .db
            .upsert_project_acl(
                community,
                &creator.public_key().to_bytes(),
                &dtag,
                "private",
                &[
                    (owner.public_key().to_bytes().to_vec(), ProjectRole::Owner),
                    (
                        collaborator.public_key().to_bytes().to_vec(),
                        ProjectRole::Collaborator,
                    ),
                    (viewer.public_key().to_bytes().to_vec(), ProjectRole::Viewer),
                ],
                1,
            )
            .await
            .expect("project acl");

        let coordinate = format!("{KIND_PROJECT}:{}:{dtag}", creator.public_key().to_hex());
        Self {
            state,
            tenant,
            dtag,
            coordinate,
            creator,
            owner,
            collaborator,
            viewer,
            stranger,
        }
    }

    /// Sign a NIP-09 deletion of one addressable coordinate as `actor`.
    fn tombstone(&self, actor: &Keys, coordinate: &str) -> nostr::Event {
        EventBuilder::new(Kind::EventDeletion, "")
            .tags(vec![Tag::parse(["a", coordinate]).expect("a tag")])
            .sign_with_keys(actor)
            .expect("sign tombstone")
    }

    async fn verdict(&self, actor: &Keys, coordinate: &str) -> Result<(), String> {
        validate_standard_deletion_event(
            &self.tenant,
            &self.tombstone(actor, coordinate),
            &self.state,
        )
        .await
        .map_err(|error| error.to_string())
    }

    /// Announce `repo_id` as `owner`, linked to this project.
    async fn link_repo(&self, repo_id: &str, owner: &Keys) {
        let owner_hex = owner.public_key().to_hex();
        self.state
            .db
            .reserve_repo_name(self.tenant.community(), repo_id, &owner_hex)
            .await
            .expect("reserve repo name");
        self.state
            .db
            .set_repo_project_ref(
                self.tenant.community(),
                repo_id,
                &owner_hex,
                Some(&self.coordinate),
                1,
            )
            .await
            .expect("link repo to project");
    }

    /// Project a shared-terminal announce owned by `owner` into this project.
    async fn announce_terminal(&self, session_id: &str, owner: &Keys) {
        self.state
            .db
            .upsert_shell_session_acl(
                self.tenant.community(),
                &owner.public_key().to_bytes(),
                session_id,
                &self.coordinate,
                "open",
                &[],
                1,
            )
            .await
            .expect("shell session acl");
    }
}

const DENIAL: &str = "must be event author";

// ── The project head ─────────────────────────────────────────────────────

/// The headline case. A pubkey seated as `owner` on the roster deletes the
/// project even though the head is addressed to the creator's key, which
/// their signature can never reproduce.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn roster_owner_deletes_the_project_head() {
    let f = DeletionFixture::new().await;
    assert_eq!(
        f.verdict(&f.owner, &f.coordinate).await,
        Ok(()),
        "a roster Owner must be able to delete the project they own"
    );
}

/// The creator reaches the same gate through the same lookup — the DB treats
/// them as an implicit Owner, so the two tiers are one.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn creator_and_roster_owner_are_one_tier_on_the_head() {
    let f = DeletionFixture::new().await;
    assert_eq!(f.verdict(&f.creator, &f.coordinate).await, Ok(()));
    assert_eq!(f.verdict(&f.owner, &f.coordinate).await, Ok(()));
}

/// Write access is not delete access. A Collaborator may post in the
/// project's channels and create things in it; the container is not theirs.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn collaborator_and_viewer_cannot_delete_the_project_head() {
    let f = DeletionFixture::new().await;
    for (label, actor) in [
        ("collaborator", &f.collaborator),
        ("viewer", &f.viewer),
        ("stranger", &f.stranger),
    ] {
        assert_eq!(
            f.verdict(actor, &f.coordinate).await,
            Err(DENIAL.to_string()),
            "{label} must not be able to delete the project head"
        );
    }
}

/// Being an Owner somewhere is not being an Owner here. Without this the new
/// arm would hand every project owner in a community a delete on every other
/// project.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_owner_of_another_project_is_refused() {
    let f = DeletionFixture::new().await;
    let other = DeletionFixture::new().await;
    // Same relay, different project. `other.owner` holds Owner on their own
    // coordinate and nothing on this one.
    assert_eq!(
        f.verdict(&other.owner, &f.coordinate).await,
        Err(DENIAL.to_string())
    );
}

/// A coordinate naming no project at all resolves to nothing, so the arm
/// cannot widen anything outside a project.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_unknown_project_coordinate_falls_back_to_authorship() {
    let f = DeletionFixture::new().await;
    let orphan = format!(
        "{KIND_PROJECT}:{}:never-projected",
        Keys::generate().public_key().to_hex()
    );
    assert_eq!(
        f.verdict(&f.owner, &orphan).await,
        Err(DENIAL.to_string()),
        "an owner elsewhere must not reach a coordinate no project claims"
    );
}

// ── Repositories ─────────────────────────────────────────────────────────

/// A repo linked into the project is reachable by its Owners, and by nobody
/// below that tier.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn roster_owner_deletes_a_repo_in_their_project() {
    let f = DeletionFixture::new().await;
    let repo_owner = Keys::generate();
    let repo_id = format!("repo-{}", uuid::Uuid::new_v4().simple());
    f.link_repo(&repo_id, &repo_owner).await;
    let coordinate = format!(
        "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{repo_id}",
        repo_owner.public_key().to_hex()
    );

    assert_eq!(f.verdict(&f.owner, &coordinate).await, Ok(()));
    assert_eq!(f.verdict(&f.creator, &coordinate).await, Ok(()));
    assert_eq!(
        f.verdict(&f.collaborator, &coordinate).await,
        Err(DENIAL.to_string()),
        "a collaborator deletes their own repo, not a teammate's"
    );
    assert_eq!(
        f.verdict(&f.viewer, &coordinate).await,
        Err(DENIAL.to_string())
    );
}

/// The authorship arm is untouched: a Collaborator still deletes the repo
/// they announced themselves, and needs no role to do it.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_collaborator_still_deletes_their_own_repo() {
    let f = DeletionFixture::new().await;
    let repo_id = format!("repo-{}", uuid::Uuid::new_v4().simple());
    f.link_repo(&repo_id, &f.collaborator).await;
    let coordinate = format!(
        "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{repo_id}",
        f.collaborator.public_key().to_hex()
    );
    assert_eq!(f.verdict(&f.collaborator, &coordinate).await, Ok(()));
}

/// A repo in no project is governed by authorship alone — the arm resolves
/// no coordinate and changes nothing.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_unlinked_repo_is_not_reachable_by_a_project_owner() {
    let f = DeletionFixture::new().await;
    let repo_owner = Keys::generate();
    let repo_id = format!("repo-{}", uuid::Uuid::new_v4().simple());
    f.state
        .db
        .reserve_repo_name(
            f.tenant.community(),
            &repo_id,
            &repo_owner.public_key().to_hex(),
        )
        .await
        .expect("reserve repo name");
    let coordinate = format!(
        "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{repo_id}",
        repo_owner.public_key().to_hex()
    );
    assert_eq!(
        f.verdict(&f.owner, &coordinate).await,
        Err(DENIAL.to_string()),
        "a project Owner must not reach a repo that is in no project"
    );
}

// ── Shared terminals ─────────────────────────────────────────────────────

/// A terminal announced into the project is reachable by its Owners. The
/// terminal's own roster is irrelevant here — that one gates watching and
/// typing, not deletion.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn roster_owner_deletes_a_terminal_in_their_project() {
    let f = DeletionFixture::new().await;
    let terminal_owner = Keys::generate();
    let session_id = uuid::Uuid::new_v4().to_string();
    f.announce_terminal(&session_id, &terminal_owner).await;
    let coordinate = format!(
        "{KIND_SHELL_SESSION}:{}:{session_id}",
        terminal_owner.public_key().to_hex()
    );

    assert_eq!(f.verdict(&f.owner, &coordinate).await, Ok(()));
    assert_eq!(
        f.verdict(&f.collaborator, &coordinate).await,
        Err(DENIAL.to_string())
    );
    assert_eq!(
        f.verdict(&f.stranger, &coordinate).await,
        Err(DENIAL.to_string())
    );
}

// ── Kinds the arm must not touch ─────────────────────────────────────────

/// Workflows keep their own owner-scoped delete path. A project Owner
/// deleting a teammate's workflow definition would be the same silent
/// no-op the cascade already goes out of its way to avoid.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_project_owner_does_not_reach_a_workflow_definition() {
    let f = DeletionFixture::new().await;
    let coordinate = format!(
        "{}:{}:{}",
        buzz_core::kind::KIND_WORKFLOW_DEF,
        f.collaborator.public_key().to_hex(),
        uuid::Uuid::new_v4()
    );
    assert_eq!(
        f.verdict(&f.owner, &coordinate).await,
        Err(DENIAL.to_string())
    );
}

/// A coordinate with no `d` component names no resource. It must not be
/// mistaken for the project whose slug happens to be empty.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_coordinate_without_a_d_component_is_refused() {
    let f = DeletionFixture::new().await;
    let coordinate = format!("{KIND_PROJECT}:{}", f.creator.public_key().to_hex());
    assert_eq!(
        f.verdict(&f.owner, &coordinate).await,
        Err(DENIAL.to_string())
    );
    // And the fixture's own slug is what a real head would carry, proving the
    // case above failed for the missing component and not for the project.
    assert!(!f.dtag.is_empty());
}
