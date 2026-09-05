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
        // `.map(|_| ())` throughout: the validator now reports which shape it
        // admitted, and every assertion in this file is about the verdict, not
        // the shape. Keeping them byte-identical is the point — a refactor
        // that changed what they assert would prove nothing.
        validate_standard_deletion_event(
            &self.tenant,
            &self.tombstone(actor, coordinate),
            &self.state,
        )
        .await
        .map(|_| ())
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

// ── Repository deletion reaches the git state, not just the listing ──────
//
// Soft-deleting the kind:30617 removes the repo from every listing. On its
// own that left the repository fully cloneable, because the git transport
// resolves an object-store pointer and a relay-signed kind:30618 ref state,
// neither of which the owner's tombstone can address. These pin the two
// extra reaches.

/// The kind:30618 ref state is signed by the *relay*, so it lives at
/// `30618:<relay-pubkey>:<repo-id>` and the owner's own coordinate deletion
/// cannot touch it. Left behind, it is a second public record of every
/// branch and tag in a repository the product says is deleted.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn deleting_a_repo_soft_deletes_its_relay_signed_ref_state() {
    let f = DeletionFixture::new().await;
    let repo_owner = Keys::generate();
    let repo_id = format!("repo-{}", uuid::Uuid::new_v4().simple());

    let ref_state = EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_GIT_REPO_STATE as u16),
        "",
    )
    .tags(vec![Tag::parse(["d", repo_id.as_str()]).expect("d tag")])
    .sign_with_keys(&f.state.relay_keypair)
    .expect("sign ref state");
    f.state
        .db
        .insert_event(f.tenant.community(), &ref_state, None)
        .await
        .expect("store ref state");

    let tombstone = f.tombstone(
        &repo_owner,
        &format!(
            "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{repo_id}",
            repo_owner.public_key().to_hex()
        ),
    );
    delete_repo_git_state(
        &f.tenant,
        &tombstone,
        &f.state,
        &repo_id,
        &repo_owner.public_key().to_hex(),
    )
    .await;

    let live = f
        .state
        .db
        .query_events(&buzz_db::event::EventQuery {
            kinds: Some(vec![buzz_core::kind::KIND_GIT_REPO_STATE as i32]),
            d_tag: Some(repo_id.clone()),
            ..buzz_db::event::EventQuery::for_community(f.tenant.community())
        })
        .await
        .expect("query ref state");
    assert!(
        live.is_empty(),
        "the repo's branches and tags must not stay publicly readable after a delete"
    );
}

/// A stale replayed tombstone must not erase the ref state of a *newer*
/// re-announce — the same `created_at` fence every other branch of the
/// deletion handler carries.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_stale_repo_tombstone_does_not_erase_a_newer_ref_state() {
    let f = DeletionFixture::new().await;
    let repo_owner = Keys::generate();
    let repo_id = format!("repo-{}", uuid::Uuid::new_v4().simple());

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let ref_state = EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_GIT_REPO_STATE as u16),
        "",
    )
    .tags(vec![Tag::parse(["d", repo_id.as_str()]).expect("d tag")])
    .custom_created_at(nostr::Timestamp::from(now))
    .sign_with_keys(&f.state.relay_keypair)
    .expect("sign ref state");
    f.state
        .db
        .insert_event(f.tenant.community(), &ref_state, None)
        .await
        .expect("store ref state");

    let stale = EventBuilder::new(Kind::EventDeletion, "")
        .tags(vec![Tag::parse([
            "a",
            &format!(
                "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{repo_id}",
                repo_owner.public_key().to_hex()
            ),
        ])
        .expect("a tag")])
        .custom_created_at(nostr::Timestamp::from(now - 60))
        .sign_with_keys(&repo_owner)
        .expect("sign stale tombstone");
    delete_repo_git_state(
        &f.tenant,
        &stale,
        &f.state,
        &repo_id,
        &repo_owner.public_key().to_hex(),
    )
    .await;

    let live = f
        .state
        .db
        .query_events(&buzz_db::event::EventQuery {
            kinds: Some(vec![buzz_core::kind::KIND_GIT_REPO_STATE as i32]),
            d_tag: Some(repo_id.clone()),
            ..buzz_db::event::EventQuery::for_community(f.tenant.community())
        })
        .await
        .expect("query ref state");
    assert_eq!(
        live.len(),
        1,
        "a tombstone older than the ref state must leave it alone"
    );
}

// ── Deleting a whole coding session ──────────────────────────────────────
//
// A genesis and a closure stay undeletable on their own — the reasons for
// that never went away. What is new is that a session can be deleted
// outright: one kind:5 naming the genesis and every live closure together,
// signed by the founder or an Owner of the project the session sits in.
// Then there is no half-state to be inconsistent about.

/// A whole-session delete, and the fixture every case below builds on.
struct SessionFixture {
    channel_id: uuid::Uuid,
    session_ref: String,
    genesis_id: Vec<u8>,
    closure_ids: Vec<Vec<u8>>,
    /// 44227, `d`-tagged like the closures and signed by the founder.
    goal_id: Vec<u8>,
    /// 44223, signed by the **provider** — not the founder, and not the
    /// person deleting. The authorship exemption exists for exactly these.
    metadata_id: Vec<u8>,
    /// 44225, also the provider's, and reachable only through the metadata
    /// above: it names an execution, never the umbrella.
    transcript_id: Vec<u8>,
}

impl DeletionFixture {
    /// A channel inside this project holding one genesis and `closures`
    /// closure revisions, all signed by `founder`.
    async fn session(&self, founder: &Keys, closures: usize) -> SessionFixture {
        let channel = self
            .state
            .db
            .create_channel(
                self.tenant.community(),
                &format!("sessions-{}", uuid::Uuid::new_v4().simple()),
                buzz_db::channel::ChannelType::Stream,
                buzz_db::channel::ChannelVisibility::Open,
                None,
                &founder.public_key().to_bytes(),
                None,
                Some(&self.coordinate),
            )
            .await
            .expect("channel");
        let session_ref = uuid::Uuid::new_v4().to_string();

        // The real NIP-CSG shape: `h`, `csg-v`, `csg-session` — and no `d`
        // tag at all. An earlier version of this fixture gave the genesis a
        // `d` tag, which meant these tests exercised an event shape the
        // product never produces; the gate reads `sessionRef` out of the
        // content, so it was right, but nothing here proved it.
        let genesis = EventBuilder::new(
            Kind::Custom(buzz_core::kind::KIND_CODING_SESSION_GENESIS as u16),
            serde_json::json!({ "sessionRef": session_ref, "v": 1 }).to_string(),
        )
        .tags(vec![
            Tag::parse(["h", &channel.id.to_string()]).expect("h tag"),
            Tag::parse(["csg-v", "csg1-1"]).expect("csg-v tag"),
            Tag::parse(["csg-session", session_ref.as_str()]).expect("csg-session tag"),
        ])
        .sign_with_keys(founder)
        .expect("sign genesis");
        self.state
            .db
            .insert_event(self.tenant.community(), &genesis, Some(channel.id))
            .await
            .expect("store genesis");

        let mut closure_ids = Vec::new();
        for index in 0..closures {
            let closure = EventBuilder::new(
                Kind::Custom(buzz_core::kind::KIND_CODING_SESSION_CLOSURE as u16),
                serde_json::json!({
                    "action": if index % 2 == 0 { "closed" } else { "open" },
                    "genesisRef": genesis.id.to_hex(),
                    "sessionRef": session_ref,
                    "v": 1,
                })
                .to_string(),
            )
            // Closures genuinely are `d`-tagged — NIP-CSG groups them by
            // `d = sessionRef`, and that is what the gate's completeness
            // check scans for.
            .tags(vec![
                Tag::parse(["h", &channel.id.to_string()]).expect("h tag"),
                Tag::parse(["d", session_ref.as_str()]).expect("d tag"),
            ])
            .custom_created_at(nostr::Timestamp::from(1_800_000_000 + index as u64))
            .sign_with_keys(founder)
            .expect("sign closure");
            self.state
                .db
                .insert_event(self.tenant.community(), &closure, Some(channel.id))
                .await
                .expect("store closure");
            closure_ids.push(closure.id.to_bytes().to_vec());
        }

        // A goal, `d`-tagged onto the umbrella the way a closure is.
        let goal = EventBuilder::new(
            Kind::Custom(buzz_core::kind::KIND_CODING_SESSION_GOAL as u16),
            "Fix the flaky timeout.",
        )
        .tags(vec![
            Tag::parse(["h", &channel.id.to_string()]).expect("h tag"),
            Tag::parse(["d", session_ref.as_str()]).expect("d tag"),
        ])
        .sign_with_keys(founder)
        .expect("sign goal");
        self.state
            .db
            .insert_event(self.tenant.community(), &goal, Some(channel.id))
            .await
            .expect("store goal");

        // The provider's two. A session's metadata and transcript are signed
        // by the host that ran it, so a founder deleting their own session
        // reaches them by the exemption and by nothing else — which is what
        // makes them the interesting half of every case below.
        let provider = Keys::generate();
        let target_key = format!("cs-target-{}", uuid::Uuid::new_v4().simple());
        let metadata = EventBuilder::new(
            Kind::Custom(buzz_core::kind::KIND_CODING_SESSION_METADATA as u16),
            serde_json::json!({ "sessionRef": session_ref, "v": 1 }).to_string(),
        )
        .tags(vec![
            Tag::parse(["h", &channel.id.to_string()]).expect("h tag"),
            Tag::parse(["cs-target", target_key.as_str()]).expect("cs-target tag"),
        ])
        .sign_with_keys(&provider)
        .expect("sign metadata");
        self.state
            .db
            .insert_event(self.tenant.community(), &metadata, Some(channel.id))
            .await
            .expect("store metadata");

        let transcript = EventBuilder::new(
            Kind::Custom(buzz_core::kind::KIND_CODING_SESSION_TRANSCRIPT as u16),
            "{}",
        )
        .tags(vec![
            Tag::parse(["h", &channel.id.to_string()]).expect("h tag"),
            Tag::parse(["cs-target", target_key.as_str()]).expect("cs-target tag"),
        ])
        .sign_with_keys(&provider)
        .expect("sign transcript");
        self.state
            .db
            .insert_event(self.tenant.community(), &transcript, Some(channel.id))
            .await
            .expect("store transcript");

        SessionFixture {
            channel_id: channel.id,
            session_ref,
            genesis_id: genesis.id.to_bytes().to_vec(),
            closure_ids,
            goal_id: goal.id.to_bytes().to_vec(),
            metadata_id: metadata.id.to_bytes().to_vec(),
            transcript_id: transcript.id.to_bytes().to_vec(),
        }
    }

    /// A kind:5 naming `ids` by `e` tag, signed by `actor`.
    fn delete_events(&self, actor: &Keys, ids: &[Vec<u8>]) -> nostr::Event {
        EventBuilder::new(Kind::EventDeletion, "")
            .tags(
                ids.iter()
                    .map(|id| Tag::parse(["e", &hex::encode(id)]).expect("e tag"))
                    .collect::<Vec<_>>(),
            )
            .sign_with_keys(actor)
            .expect("sign deletion")
    }

    async fn verdict_events(&self, actor: &Keys, ids: &[Vec<u8>]) -> Result<(), String> {
        self.shape_events(actor, ids).await.map(|_| ())
    }

    /// The same verdict, keeping the shape the validator reported.
    async fn shape_events(&self, actor: &Keys, ids: &[Vec<u8>]) -> Result<DeletionShape, String> {
        validate_standard_deletion_event(&self.tenant, &self.delete_events(actor, ids), &self.state)
            .await
            .map_err(|error| error.to_string())
    }
}

impl SessionFixture {
    /// Everything one session owns: the genesis, every closure, the goal,
    /// and the provider-signed metadata and transcript — the set both the
    /// desktop and `bee sessions delete` assemble.
    fn whole_chain(&self) -> Vec<Vec<u8>> {
        let mut ids = vec![self.genesis_id.clone()];
        ids.extend(self.closure_ids.iter().cloned());
        ids.push(self.goal_id.clone());
        ids.push(self.metadata_id.clone());
        ids.push(self.transcript_id.clone());
        ids
    }

    /// The same, without the metadata that binds the transcript to it.
    fn chain_without_metadata(&self) -> Vec<Vec<u8>> {
        self.whole_chain()
            .into_iter()
            .filter(|id| id != &self.metadata_id)
            .collect()
    }
}

/// The capability. The founder deletes their own session outright.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_founder_deletes_their_whole_session() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 2).await;
    assert_eq!(
        f.verdict_events(&founder, &session.whole_chain()).await,
        Ok(())
    );
    assert!(!session.session_ref.is_empty());
}

/// And a project Owner reaches a session they did not found — the case an
/// authorship rule could never express, and the reason this is authorized
/// by role at all.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_project_owner_deletes_a_session_they_did_not_found() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 1).await;
    assert_eq!(
        f.verdict_events(&f.owner, &session.whole_chain()).await,
        Ok(())
    );
    assert_eq!(
        f.verdict_events(&f.creator, &session.whole_chain()).await,
        Ok(())
    );
}

/// Write access into a project is not authority over its sessions.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_collaborator_viewer_and_stranger_cannot_delete_someone_elses_session() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 1).await;
    for (label, actor) in [
        ("collaborator", &f.collaborator),
        ("viewer", &f.viewer),
        ("stranger", &f.stranger),
    ] {
        let verdict = f.verdict_events(actor, &session.whole_chain()).await;
        assert_eq!(
            verdict,
            Err("only this session's founder, or an owner of the project it belongs to, may delete it"
                .to_string()),
            "{label} must not be able to delete a session they neither founded nor own"
        );
    }
}

/// The piecemeal refusals are the whole reason the old blanket rule existed,
/// and they survive intact: a genesis on its own still cannot go.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_genesis_alone_is_still_refused_even_for_its_founder() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 2).await;
    let verdict = f
        .verdict_events(&founder, std::slice::from_ref(&session.genesis_id))
        .await;
    let message = verdict.expect_err("a partial chain must be refused");
    assert!(
        message.contains("closure revisions too"),
        "the refusal must name what is missing, got {message:?}"
    );
    assert!(message.contains('2'), "and how many, got {message:?}");
}

/// A closure on its own would roll shared state back to an older action with
/// no counter-revision, which is exactly what it always was.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_closure_alone_is_still_refused() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 1).await;
    let verdict = f
        .verdict_events(&founder, &session.closure_ids.clone())
        .await;
    let message = verdict.expect_err("a lone closure deletion must be refused");
    assert!(
        message.contains("cannot be deleted on their own"),
        "got {message:?}"
    );
    assert!(
        message.contains("another closure revision"),
        "the refusal must still point at the supported way to change state, got {message:?}"
    );
}

/// Naming *some* closures is the dangerous near-miss: it looks like a
/// session delete and leaves revisions describing a session that is gone.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_partial_chain_is_refused_and_says_how_many_are_missing() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 3).await;
    let mut partial = vec![session.genesis_id.clone()];
    partial.push(session.closure_ids[0].clone());
    let message = f
        .verdict_events(&founder, &partial)
        .await
        .expect_err("a partial chain must be refused");
    assert!(
        message.contains("2 of them are not named"),
        "got {message:?}"
    );
}

/// Two sessions in one deletion is refused rather than half-applied.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn two_sessions_in_one_deletion_are_refused() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let first = f.session(&founder, 0).await;
    let second = f.session(&founder, 0).await;
    let message = f
        .verdict_events(&founder, &[first.genesis_id, second.genesis_id])
        .await
        .expect_err("two geneses must be refused");
    assert!(message.contains("one session at a time"), "got {message:?}");
}

/// A session with no closures at all is a complete chain by itself.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_session_that_was_never_closed_deletes_with_its_genesis_alone() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 0).await;
    assert_eq!(
        f.verdict_events(&founder, std::slice::from_ref(&session.genesis_id))
            .await,
        Ok(())
    );
    assert!(session.closure_ids.is_empty());
    assert_ne!(session.channel_id, uuid::Uuid::nil());
}

/// The authorship exemption a session delete needs is a closed list. It must
/// not become a way to delete a teammate's chat messages by naming them in
/// the same event as a genesis.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_session_delete_cannot_smuggle_in_someone_elses_message() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 0).await;

    let bystander = Keys::generate();
    let message = EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_STREAM_MESSAGE_V2 as u16),
        "not yours to delete",
    )
    .tags(vec![
        Tag::parse(["h", &session.channel_id.to_string()]).expect("h tag")
    ])
    .sign_with_keys(&bystander)
    .expect("sign message");
    f.state
        .db
        .insert_event(f.tenant.community(), &message, Some(session.channel_id))
        .await
        .expect("store message");

    let verdict = f
        .verdict_events(
            &founder,
            &[session.genesis_id.clone(), message.id.to_bytes().to_vec()],
        )
        .await;
    assert_eq!(
        verdict,
        Err(DENIAL.to_string()),
        "a genesis in the same deletion must not launder authorship over an unrelated event"
    );
}

// ── What a session delete may reach, and what it may not ─────────────────
//
// The authorship exemption is granted by *kind*, and a kind is not an
// identity. These pin the second half of the admission: the target has to
// belong to the session actually being deleted. Until the ingest gate
// admitted a multi-target `kind:5` at all this was unreachable — a deletion
// naming a genesis and anything else was refused before it got here — so
// the two changes are one change, and this is the half that keeps it safe.

/// The shape ingest needs, and the reason it needs it: a real session is
/// many events, and there is no version of it that fits in one `e` tag.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_whole_session_delete_reports_its_shape_and_names_more_than_one_target() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 2).await;
    let chain = session.whole_chain();

    assert!(
        chain.len() > 1,
        "a session delete is many targets by construction, got {}",
        chain.len()
    );
    assert_eq!(
        f.shape_events(&founder, &chain).await,
        Ok(DeletionShape::WholeCodingSession),
        "ingest reads this to know the single-target rule does not apply"
    );
}

/// And an ordinary deletion still reports the shape that keeps the rule.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_ordinary_self_authored_deletion_is_still_a_single_target() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 0).await;
    assert_eq!(
        f.shape_events(&founder, std::slice::from_ref(&session.goal_id))
            .await,
        Ok(DeletionShape::SingleTarget),
    );
}

/// The headline protection. Deleting your session must not be a way to
/// delete somebody else's session's events by naming them alongside it —
/// same kinds, different umbrella, and the actor authored none of them.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_session_delete_cannot_reach_another_sessions_events() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let mine = f.session(&founder, 1).await;
    let theirs = f.session(&Keys::generate(), 1).await;

    for (label, smuggled) in [
        ("goal", theirs.goal_id.clone()),
        ("metadata", theirs.metadata_id.clone()),
        ("transcript", theirs.transcript_id.clone()),
    ] {
        let mut ids = mine.whole_chain();
        ids.push(smuggled);
        let message = f
            .verdict_events(&founder, &ids)
            .await
            .expect_err("another session's events must not be reachable");
        assert!(
            message.contains("does not belong to the session it deletes"),
            "the refusal must say which problem it is — {label} gave {message:?}"
        );
    }
}

/// A transcript reaches its umbrella through the metadata that names both.
/// Drop the metadata from the deletion and the transcript is no longer
/// attributable to this session by anything on the wire, so it stays.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_transcript_whose_metadata_is_not_named_is_refused() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 0).await;

    let message = f
        .verdict_events(&founder, &session.chain_without_metadata())
        .await
        .expect_err("an unattributable transcript must not be swept in");
    assert!(
        message.contains("does not belong to the session it deletes"),
        "got {message:?}"
    );
}

/// The positive case for the same rule: with the metadata named, the
/// provider-signed transcript goes too — by the exemption, since the founder
/// signed neither.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn the_providers_metadata_and_transcript_go_with_the_session() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let session = f.session(&founder, 1).await;

    assert_eq!(
        f.verdict_events(&founder, &session.whole_chain()).await,
        Ok(())
    );

    // Named on their own, with no genesis to authorize anything, the same two
    // events are refused — the exemption is a property of the whole act.
    let message = f
        .verdict_events(
            &founder,
            &[session.metadata_id.clone(), session.transcript_id.clone()],
        )
        .await
        .expect_err("without a session delete these are somebody else's events");
    assert_eq!(message, "must be event author");
}

/// Signing both sessions is not a way around the piecemeal refusal.
///
/// A founder deleting session A, naming session B's closure alongside it:
/// the authorship check would happily pass it — they *did* sign it — and
/// the result would be exactly what `refuse_permanent_identity_deletion`
/// exists to prevent, a closure gone from a session that still exists. So
/// the exemption from that refusal is scoped to the session being deleted,
/// not to the deletion as a whole.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_session_delete_does_not_exempt_another_sessions_closure_from_the_piecemeal_rule() {
    let f = DeletionFixture::new().await;
    let founder = Keys::generate();
    let mine = f.session(&founder, 1).await;
    // Same founder, so authorship alone would admit every one of its events.
    let other = f.session(&founder, 2).await;

    let mut ids = mine.whole_chain();
    ids.push(other.closure_ids[0].clone());
    let message = f
        .verdict_events(&founder, &ids)
        .await
        .expect_err("another session's closure must not ride along");
    assert!(
        message.contains("cannot be deleted on their own"),
        "the piecemeal refusal is what must fire, got {message:?}"
    );

    // And its genesis, which would strand every closure it still has.
    let mut ids = mine.whole_chain();
    ids.push(other.genesis_id.clone());
    let message = f
        .verdict_events(&founder, &ids)
        .await
        .expect_err("a second genesis must not ride along");
    assert!(message.contains("one session at a time"), "got {message:?}");
}
