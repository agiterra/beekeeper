//! Conditional sources through real ingest, authorization and NIP-09 deletion.

use std::sync::Arc;

use axum::http::StatusCode;
use buzz_core::channel::ProjectRole;
use buzz_core::kind::{KIND_PROJECT, KIND_PROJECT_PACK_SOURCE};
use buzz_core::project_pack_source::{build_conditional_project_pack_source, PackPin};
use buzz_core::tenant::TenantContext;
use buzz_db::EventQuery;
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

use crate::handlers::ingest::{
    ingest_event, HttpAuthMethod, IngestAuth, IngestError, IngestResult,
};
use crate::state::AppState;

struct Fixture {
    state: Arc<AppState>,
    tenant: TenantContext,
    creator: Keys,
    owner: Keys,
    project: String,
    at: u64,
}

impl Fixture {
    async fn new() -> Self {
        let state = crate::api::git::policy::tests::policy_test_state().await;
        let host = format!("packs-cas-{}.example", uuid::Uuid::new_v4().simple());
        let community = state
            .db
            .ensure_configured_community(&host)
            .await
            .expect("community")
            .id;
        let tenant = TenantContext::resolved(community, host);
        let creator = Keys::generate();
        let owner = Keys::generate();
        let project = format!("30621:{}:conditional", creator.public_key().to_hex());
        let head = EventBuilder::new(
            Kind::Custom(KIND_PROJECT as u16),
            r#"{"name":"conditional"}"#,
        )
        .tags([Tag::parse(["d", "conditional"]).expect("d")])
        .sign_with_keys(&creator)
        .expect("project signature");
        ingest_event(&state, &tenant, head, auth(&creator))
            .await
            .expect("project ingest");
        state
            .db
            .upsert_project_acl(
                community,
                creator.public_key().as_bytes(),
                "conditional",
                "public",
                &[(owner.public_key().to_bytes().to_vec(), ProjectRole::Owner)],
                Timestamp::now().as_secs() as i64,
            )
            .await
            .expect("legitimate second Owner");
        Self {
            state,
            tenant,
            creator,
            owner,
            project,
            at: Timestamp::now().as_secs() - 30,
        }
    }

    fn source(&self, signer: &Keys, expected: Option<&str>, offset: u64) -> Event {
        let repo = format!(
            "30617:{}:conditional-packs",
            self.creator.public_key().to_hex()
        );
        let draft = build_conditional_project_pack_source(
            &self.project,
            &repo,
            &PackPin::Sha("a".repeat(40)),
            None,
            None,
            expected,
        )
        .expect("conditional draft");
        EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE as u16), draft.content)
            .tags(
                draft
                    .tags
                    .into_iter()
                    .map(|tag| Tag::parse(tag).expect("source tag")),
            )
            .custom_created_at(Timestamp::from(self.at + offset))
            .sign_with_keys(signer)
            .expect("source signature")
    }

    async fn ingest(&self, signer: &Keys, event: &Event) -> Result<IngestResult, IngestError> {
        ingest_event(&self.state, &self.tenant, event.clone(), auth(signer)).await
    }

    async fn head(&self) -> Option<String> {
        self.state
            .db
            .query_events(&EventQuery {
                kinds: Some(vec![KIND_PROJECT_PACK_SOURCE as i32]),
                d_tag: Some(self.project.clone()),
                limit: Some(1),
                ..EventQuery::for_community(self.tenant.community())
            })
            .await
            .expect("effective source query")
            .first()
            .map(|row| row.event.id.to_hex())
    }
}

fn auth(keys: &Keys) -> IngestAuth {
    IngestAuth::Http {
        pubkey: keys.public_key(),
        scopes: vec![
            buzz_auth::Scope::ReposWrite,
            buzz_auth::Scope::MessagesWrite,
            buzz_auth::Scope::ChannelsWrite,
        ],
        auth_method: HttpAuthMethod::Nip98,
    }
}

fn assert_conflict(error: IngestError) {
    assert!(matches!(error, IngestError::Conflict(_)), "{error:?}");
    let (status, message, category) = error.response();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(category, "conflict");
    assert!(
        message.starts_with("conflict: PACK_SOURCE_CONFLICT:"),
        "{message}"
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn conditional_ingest_checks_initial_and_existing_heads_after_authority() {
    let f = Fixture::new().await;
    let missing = f.source(&f.creator, Some(&"b".repeat(64)), 0);
    assert_conflict(
        f.ingest(&f.creator, &missing)
            .await
            .err()
            .expect("missing expected head"),
    );
    assert_eq!(f.head().await, None);

    let initial = f.source(&f.creator, None, 1);
    assert!(
        f.ingest(&f.creator, &initial)
            .await
            .expect("initial creation")
            .accepted
    );
    assert_eq!(f.head().await, Some(initial.id.to_hex()));
    let stale_create = f.source(&f.creator, None, 2);
    assert_conflict(
        f.ingest(&f.creator, &stale_create)
            .await
            .err()
            .expect("already initialized"),
    );
    assert_eq!(f.head().await, Some(initial.id.to_hex()));

    let successor = f.source(&f.creator, Some(&initial.id.to_hex()), 3);
    assert!(
        f.ingest(&f.creator, &successor)
            .await
            .expect("conditional replacement")
            .accepted
    );
    let stale_update = f.source(&f.creator, Some(&initial.id.to_hex()), 4);
    assert_conflict(
        f.ingest(&f.creator, &stale_update)
            .await
            .err()
            .expect("stale expected head"),
    );
    assert_eq!(f.head().await, Some(successor.id.to_hex()));

    let stranger = Keys::generate();
    let unauthorized = f.source(&stranger, Some(&initial.id.to_hex()), 5);
    let error = f
        .ingest(&stranger, &unauthorized)
        .await
        .err()
        .expect("stranger refused");
    assert!(matches!(error, IngestError::AuthFailed(_)), "{error:?}");
    let (status, message, category) = error.response();
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(category, "auth");
    assert!(!message.contains("PACK_SOURCE_CONFLICT"), "{message}");
    assert_eq!(f.head().await, Some(successor.id.to_hex()));

    let retry = f
        .ingest(&f.creator, &initial)
        .await
        .expect("retained initial retry");
    assert!(retry.accepted);
    assert_eq!(retry.message, "duplicate:");
    assert_eq!(retry.event_id, initial.id.to_hex());
    assert_eq!(f.head().await, Some(successor.id.to_hex()));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn signed_source_deletion_and_exact_retry_do_not_resurrect_a_source() {
    let f = Fixture::new().await;
    let source = f.source(&f.creator, None, 0);
    f.ingest(&f.creator, &source).await.expect("source stores");
    let address = format!(
        "{KIND_PROJECT_PACK_SOURCE}:{}:{}",
        f.creator.public_key().to_hex(),
        f.project
    );
    let deletion = EventBuilder::new(Kind::EventDeletion, "")
        .tags([Tag::parse(["a", &address]).expect("source address")])
        .sign_with_keys(&f.creator)
        .expect("signed NIP-09 deletion");
    assert!(
        f.ingest(&f.creator, &deletion)
            .await
            .expect("deletion through ingest")
            .accepted
    );
    assert_eq!(f.head().await, None);
    assert!(f
        .state
        .db
        .get_event_by_id(f.tenant.community(), source.id.as_bytes())
        .await
        .expect("live read")
        .is_none());
    assert!(f
        .state
        .db
        .get_event_by_id_including_deleted(f.tenant.community(), source.id.as_bytes())
        .await
        .expect("retained read")
        .is_some());

    let retry = f
        .ingest(&f.creator, &source)
        .await
        .expect("deleted exact retry reconciles");
    assert!(retry.accepted);
    assert_eq!(retry.message, "duplicate:");
    assert_eq!(f.head().await, None);
    assert!(f
        .state
        .db
        .get_event_by_id(f.tenant.community(), source.id.as_bytes())
        .await
        .expect("still deleted")
        .is_none());
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn competing_project_owners_get_one_conditional_successor_and_one_conflict() {
    let f = Fixture::new().await;
    let initial = f.source(&f.creator, None, 0);
    f.ingest(&f.creator, &initial)
        .await
        .expect("initial source");
    let left = f.source(&f.creator, Some(&initial.id.to_hex()), 1);
    let right = f.source(&f.owner, Some(&initial.id.to_hex()), 2);
    let (left_result, right_result) =
        tokio::join!(f.ingest(&f.creator, &left), f.ingest(&f.owner, &right));
    let winner = match (left_result, right_result) {
        (Ok(result), Err(error)) => {
            assert!(result.accepted);
            assert_conflict(error);
            left.id.to_hex()
        }
        (Err(error), Ok(result)) => {
            assert_conflict(error);
            assert!(result.accepted);
            right.id.to_hex()
        }
        (Ok(_), Ok(_)) => panic!("both Owners replaced the same expected source"),
        (Err(left), Err(right)) => {
            panic!("neither legitimate Owner was admitted: {left:?}; {right:?}")
        }
    };
    assert_eq!(f.head().await, Some(winner));
}
