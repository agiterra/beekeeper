//! Authorization and durable production of workflow CI completion events.

use std::sync::{Arc, Weak};

use beekeeper_core::ci_result::{build_ci_result, correlation_id, CiResult};
use beekeeper_core::kind::{normalize_project_coordinate, repo_project_ref, KIND_CI_RESULT};
use beekeeper_core::project_pack_source::normalize_repository_coordinate;
use beekeeper_core::repository_founders::RepositoryFounders;
use beekeeper_core::tenant::CommunityId;
use beekeeper_db::CiResultInsertOutcome;
use beekeeper_workflow::action_sink::ActionSinkError;
use nostr::{EventBuilder, Kind, Tag};
use uuid::Uuid;

use crate::handlers::event::dispatch_persistent_event;
use crate::handlers::repo_protection::{project_roster_rows, repository_announcement};
use crate::state::AppState;

#[derive(Debug)]
pub(crate) enum CiResultAuthorityError {
    Invalid(String),
    Unauthorized(String),
    Storage(String),
}

pub(crate) async fn authorize_ci_result_binding(
    state: &Arc<AppState>,
    community_id: CommunityId,
    owner_pubkey: &[u8],
    project: &str,
    repository: &str,
) -> Result<(), CiResultAuthorityError> {
    let normalized_project = normalize_project_coordinate(project).ok_or_else(|| {
        CiResultAuthorityError::Invalid(
            "project must be a full 30621:<64-hex>:<id> coordinate".into(),
        )
    })?;
    if normalized_project != project {
        return Err(CiResultAuthorityError::Invalid(
            "project coordinate must be canonical".into(),
        ));
    }
    let normalized_repository = normalize_repository_coordinate(repository).ok_or_else(|| {
        CiResultAuthorityError::Invalid(
            "repository must be a full 30617:<64-hex>:<id> coordinate".into(),
        )
    })?;
    if normalized_repository != repository {
        return Err(CiResultAuthorityError::Invalid(
            "repository coordinate must be canonical".into(),
        ));
    }
    let mut parts = repository.splitn(3, ':');
    let _kind = parts.next();
    let repo_owner = parts.next().ok_or_else(|| {
        CiResultAuthorityError::Invalid("repository coordinate is missing its owner".into())
    })?;
    let repo_id = parts.next().ok_or_else(|| {
        CiResultAuthorityError::Invalid("repository coordinate is missing its id".into())
    })?;

    let announcement = repository_announcement(state, community_id, repo_owner, repo_id)
        .await
        .map_err(|_| {
            CiResultAuthorityError::Storage("repository announcement lookup failed".into())
        })?
        .ok_or_else(|| {
            CiResultAuthorityError::Unauthorized(format!(
                "no repository is announced at {repository}"
            ))
        })?;
    if repo_project_ref(&announcement).as_deref() != Some(project) {
        return Err(CiResultAuthorityError::Unauthorized(format!(
            "repository {repository} does not belong to project {project}"
        )));
    }

    let (roster, _) = project_roster_rows(state, community_id, &announcement)
        .await
        .map_err(|_| CiResultAuthorityError::Storage("project roster lookup failed".into()))?;
    let founders = RepositoryFounders::from_announcement(&announcement).with_roster_roles(roster);
    if !founders.contains(&hex::encode(owner_pubkey)) {
        return Err(CiResultAuthorityError::Unauthorized(format!(
            "workflow owner is not a founder of {repository}"
        )));
    }
    Ok(())
}

pub(crate) async fn record_ci_result(
    weak_state: &Weak<AppState>,
    community_id: CommunityId,
    result: &CiResult,
) -> Result<String, ActionSinkError> {
    let state = weak_state
        .upgrade()
        .ok_or_else(|| ActionSinkError::Database("relay is shutting down".into()))?;
    let workflow_id = Uuid::parse_str(&result.identity.workflow)
        .map_err(|_| ActionSinkError::InvalidInput("CI result workflow is not a UUID".into()))?;
    let workflow = state
        .db
        .get_workflow(community_id, workflow_id)
        .await
        .map_err(|e| ActionSinkError::Database(format!("workflow lookup failed: {e}")))?;

    if !workflow.enabled || workflow.status != beekeeper_db::workflow::WorkflowStatus::Active {
        return Err(ActionSinkError::Unauthorized(
            "workflow is disabled or inactive".into(),
        ));
    }
    let definition: beekeeper_workflow::WorkflowDef =
        serde_json::from_value(workflow.definition.clone()).map_err(|e| {
            ActionSinkError::Database(format!("stored workflow definition is invalid: {e}"))
        })?;
    let channel_id = workflow
        .channel_id
        .ok_or_else(|| ActionSinkError::Unauthorized("workflow has no channel scope".into()))?;
    state
        .workflow_engine
        .check_owner_authority(
            community_id,
            channel_id,
            &workflow.owner_pubkey,
            &definition,
        )
        .await
        .map_err(|e| ActionSinkError::Unauthorized(e.to_string()))?;
    let binding_still_present = definition.steps.iter().any(|step| {
        matches!(
            &step.action,
            beekeeper_workflow::ActionDef::RecordCiResult {
                project,
                repository,
                check,
                phase,
                ..
            } if project == &result.identity.project
                && repository == &result.identity.repository
                && check == &result.identity.check
                && phase == &result.identity.phase
        )
    });
    if !binding_still_present {
        return Err(ActionSinkError::Unauthorized(
            "stored workflow no longer contains the executing CI result binding".into(),
        ));
    }

    authorize_ci_result_binding(
        &state,
        community_id,
        &workflow.owner_pubkey,
        &result.identity.project,
        &result.identity.repository,
    )
    .await
    .map_err(|error| match error {
        CiResultAuthorityError::Invalid(detail) => ActionSinkError::InvalidInput(detail),
        CiResultAuthorityError::Unauthorized(detail) => ActionSinkError::Unauthorized(detail),
        CiResultAuthorityError::Storage(detail) => ActionSinkError::Database(detail),
    })?;

    let host = state
        .db
        .lookup_community_host(community_id)
        .await
        .map_err(|e| ActionSinkError::Database(e.to_string()))?
        .ok_or_else(|| {
            ActionSinkError::Database(format!(
                "workflow run community {community_id} is not mapped to a host"
            ))
        })?;
    let tenant = beekeeper_core::tenant::TenantContext::resolved(community_id, host);
    let (raw_tags, content) = build_ci_result(result).map_err(ActionSinkError::InvalidInput)?;
    let correlation = correlation_id(&result.identity).map_err(ActionSinkError::InvalidInput)?;
    let tags = raw_tags
        .into_iter()
        .map(|tag| {
            Tag::parse(tag).map_err(|e| ActionSinkError::EventBuild(format!("CI result tag: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let event = EventBuilder::new(Kind::from(KIND_CI_RESULT as u16), content)
        .tags(tags)
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| ActionSinkError::EventBuild(format!("CI result signing: {e}")))?;

    match state
        .db
        .insert_ci_result_event(community_id, &event, &correlation)
        .await
        .map_err(|e| ActionSinkError::Database(e.to_string()))?
    {
        CiResultInsertOutcome::Inserted(stored) => {
            let event_id = stored.event.id.to_hex();
            let relay_pubkey = state.relay_keypair.public_key().to_hex();
            let _ = dispatch_persistent_event(
                &tenant,
                &state,
                &stored,
                KIND_CI_RESULT,
                &relay_pubkey,
                None,
            )
            .await;
            Ok(event_id)
        }
        CiResultInsertOutcome::Duplicate(stored) => Ok(stored.event.id.to_hex()),
        CiResultInsertOutcome::Conflict(stored) => Err(ActionSinkError::CiResultConflict(format!(
            "identity already recorded by event {}",
            stored.event.id.to_hex()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::channel::{ChannelType, ChannelVisibility, MemberRole};
    use beekeeper_db::CreateCommunityWithOwnerResult;

    async fn test_state() -> Arc<AppState> {
        let mut config = crate::config::Config::from_env().expect("default config loads");
        config.require_relay_membership = false;
        config.redis_url = "redis://127.0.0.1:1".to_string();
        let pool = sqlx::PgPool::connect_lazy(&config.database_url).expect("lazy pg pool");
        let db = beekeeper_db::Db::from_pool(pool.clone());
        let redis_pool = deadpool_redis::Config::from_url(&config.redis_url)
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("redis pool");
        let pubsub = Arc::new(
            beekeeper_pubsub::PubSubManager::new(&config.redis_url, redis_pool.clone())
                .await
                .expect("pubsub manager"),
        );
        let audit = beekeeper_audit::AuditService::new(pool.clone());
        let auth = beekeeper_auth::AuthService::new(config.auth.clone());
        let search = beekeeper_search::SearchService::new(pool.clone());
        let workflow_engine = Arc::new(beekeeper_workflow::WorkflowEngine::new(
            db.clone(),
            beekeeper_workflow::WorkflowConfig::default(),
        ));
        let media_storage =
            beekeeper_media::MediaStorage::new(&config.media).expect("media storage");
        let (state, _audit_shutdown) = AppState::new(
            config,
            db,
            redis_pool,
            audit,
            pubsub,
            auth,
            search,
            workflow_engine,
            nostr::Keys::generate(),
            media_storage,
        );
        Arc::new(state)
    }

    struct Fixture {
        state: Arc<AppState>,
        community: CommunityId,
        channel_id: Uuid,
        owner: nostr::Keys,
        workflow_id: Uuid,
        result: CiResult,
    }

    fn definition(result: &CiResult, check: &str) -> beekeeper_workflow::WorkflowDef {
        beekeeper_workflow::WorkflowDef {
            name: "CI completion".into(),
            description: None,
            project: None,
            trigger: beekeeper_workflow::TriggerDef::Webhook,
            steps: vec![beekeeper_workflow::Step {
                id: "record".into(),
                name: None,
                if_expr: None,
                timeout_secs: None,
                action: beekeeper_workflow::ActionDef::RecordCiResult {
                    project: result.identity.project.clone(),
                    repository: result.identity.repository.clone(),
                    check: check.into(),
                    phase: result.identity.phase,
                    commit: "{{trigger.commit}}".into(),
                    run: "{{trigger.run}}".into(),
                    attempt: "{{trigger.attempt}}".into(),
                    conclusion: "{{trigger.conclusion}}".into(),
                    evidence_url: None,
                    summary: None,
                },
            }],
            enabled: true,
        }
    }

    async fn fixture() -> Fixture {
        let state = test_state().await;
        let owner = nostr::Keys::generate();
        let owner_bytes = owner.public_key().to_bytes().to_vec();
        let host = format!("wf-ci-action-{}.example", Uuid::new_v4().simple());
        let community = match state
            .db
            .create_community_with_owner(&host, &owner.public_key().to_hex())
            .await
            .expect("create community")
        {
            CreateCommunityWithOwnerResult::Created(record) => record.id,
            other => panic!("expected fresh community, got {other:?}"),
        };
        state
            .db
            .ensure_user(community, &owner_bytes)
            .await
            .expect("ensure workflow owner");
        let channel = state
            .db
            .create_channel(
                community,
                "wf-ci-action",
                ChannelType::Stream,
                ChannelVisibility::Open,
                None,
                &owner_bytes,
                None,
                None,
            )
            .await
            .expect("create workflow channel");
        let workflow_id = Uuid::new_v4();
        let result = CiResult {
            schema: beekeeper_core::ci_result::CI_RESULT_SCHEMA.into(),
            identity: beekeeper_core::ci_result::CiResultIdentity {
                project: format!("30621:{}:agiterra", "a".repeat(64)),
                repository: format!("30617:{}:beekeeper", "b".repeat(64)),
                commit: "c".repeat(40),
                check: "relay-ci".into(),
                run: "42".into(),
                attempt: 1,
                workflow: workflow_id.to_string(),
                phase: beekeeper_core::ci_result::CiPhase::Build,
            },
            conclusion: beekeeper_core::ci_result::CiConclusion::Success,
            evidence_url: None,
            summary: None,
        };
        let definition = definition(&result, &result.identity.check);
        state
            .db
            .upsert_workflow(
                community,
                workflow_id,
                Some(channel.id),
                &owner_bytes,
                &definition.name,
                &serde_json::to_string(&definition).expect("serialize definition"),
                &[7_u8; 32],
                None,
            )
            .await
            .expect("insert workflow");
        Fixture {
            state,
            community,
            channel_id: channel.id,
            owner,
            workflow_id,
            result,
        }
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn denies_disabled_workflow_at_action_boundary() {
        let fixture = fixture().await;
        fixture
            .state
            .db
            .set_workflow_enabled(fixture.community, fixture.workflow_id, false)
            .await
            .expect("disable workflow");
        let error = record_ci_result(
            &Arc::downgrade(&fixture.state),
            fixture.community,
            &fixture.result,
        )
        .await
        .expect_err("disabled workflow must not record");
        assert!(matches!(error, ActionSinkError::Unauthorized(_)));
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn denies_revoked_channel_owner_at_action_boundary() {
        let fixture = fixture().await;
        let replacement_owner = nostr::Keys::generate();
        let replacement_bytes = replacement_owner.public_key().to_bytes().to_vec();
        fixture
            .state
            .db
            .ensure_user(fixture.community, &replacement_bytes)
            .await
            .expect("ensure replacement owner");
        fixture
            .state
            .db
            .add_member(
                fixture.community,
                fixture.channel_id,
                &replacement_bytes,
                MemberRole::Owner,
                Some(&fixture.owner.public_key().to_bytes()),
            )
            .await
            .expect("add replacement owner");
        fixture
            .state
            .db
            .remove_member(
                fixture.community,
                fixture.channel_id,
                &fixture.owner.public_key().to_bytes(),
                &replacement_bytes,
            )
            .await
            .expect("remove workflow owner");
        let error = record_ci_result(
            &Arc::downgrade(&fixture.state),
            fixture.community,
            &fixture.result,
        )
        .await
        .expect_err("removed owner must not record");
        assert!(matches!(error, ActionSinkError::Unauthorized(_)));
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn denies_binding_changed_after_run_started() {
        let fixture = fixture().await;
        let changed = definition(&fixture.result, "different-check");
        fixture
            .state
            .db
            .upsert_workflow(
                fixture.community,
                fixture.workflow_id,
                Some(fixture.channel_id),
                &fixture.owner.public_key().to_bytes(),
                &changed.name,
                &serde_json::to_string(&changed).expect("serialize changed definition"),
                &[8_u8; 32],
                None,
            )
            .await
            .expect("change binding");
        let error = record_ci_result(
            &Arc::downgrade(&fixture.state),
            fixture.community,
            &fixture.result,
        )
        .await
        .expect_err("stale binding must not record");
        assert!(matches!(error, ActionSinkError::Unauthorized(_)));
        assert!(error.to_string().contains("no longer contains"));
    }
}
