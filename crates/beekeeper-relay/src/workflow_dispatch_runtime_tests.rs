// Postgres + Redis tests for the durable workflow dispatch worker.
//
// Every test inserts a relay-signed, plugin-sourced kind:50000 the way the
// webhook host will (event row + queue row in one insert), and drives the
// worker through `run_once(.., Some(community))` so other tests' rows in the
// shared scratch database cannot interfere.

use super::*;
use beekeeper_core::application_event::APPLICATION_EVENT_NAMESPACE;
use beekeeper_core::channel::{ChannelType, ChannelVisibility};
use beekeeper_core::kind::KIND_APPLICATION_EVENT;
use beekeeper_db::CreateCommunityWithOwnerResult;
use nostr::{EventBuilder, Keys, Kind, Tag};
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

const PUSH: &str = "com.github.push";

fn database_url() -> String {
    std::env::var("BEEKEEPER_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| "postgres://buzz:buzz_dev@localhost:5432/buzz".to_owned())
    // sadscan:disable np.postgres.1
}

async fn test_state() -> Arc<AppState> {
    let mut config = crate::config::Config::from_env().expect("default config loads");
    config.database_url = database_url();
    config.redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_owned());
    config.require_relay_membership = false;
    let pool = PgPool::connect(&config.database_url)
        .await
        .expect("connect test DB");
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
    let media_storage = beekeeper_media::MediaStorage::new(&config.media).expect("media storage");
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

struct Fixture {
    state: Arc<AppState>,
    pool: PgPool,
    community: CommunityId,
    host: String,
    owner: Keys,
    channel_id: Uuid,
    installation: Uuid,
}

async fn fixture() -> Fixture {
    let state = test_state().await;
    let pool = PgPool::connect(&database_url()).await.expect("raw pool");
    let owner = Keys::generate();
    let owner_bytes = owner.public_key().to_bytes().to_vec();
    let host = format!("wf-dispatch-{}.example", Uuid::new_v4().simple());
    let community = match state
        .db
        .create_community_with_owner(&host, &owner.public_key().to_hex())
        .await
        .expect("create community")
    {
        CreateCommunityWithOwnerResult::Created(record) => record.id,
        other => panic!("expected a fresh community, got {other:?}"),
    };
    state
        .db
        .ensure_user(community, &owner_bytes)
        .await
        .expect("ensure owner");
    let channel = state
        .db
        .create_channel(
            community,
            "wf-dispatch",
            ChannelType::Stream,
            ChannelVisibility::Open,
            None,
            &owner_bytes,
            None,
            None,
        )
        .await
        .expect("create channel");
    Fixture {
        state,
        pool,
        community,
        host,
        owner,
        channel_id: channel.id,
        installation: Uuid::new_v4(),
    }
}

impl Fixture {
    /// Store an `application_event` workflow for `event_type` in the channel.
    async fn workflow(&self, name: &str, event_type: &str) -> Uuid {
        let id = Uuid::new_v4();
        let def = beekeeper_workflow::WorkflowDef {
            name: name.into(),
            description: None,
            project: None,
            trigger: beekeeper_workflow::TriggerDef::ApplicationEvent {
                event_type: event_type.into(),
                installation: self.installation.to_string(),
                filter: None,
            },
            steps: vec![beekeeper_workflow::Step {
                id: "note".into(),
                name: None,
                if_expr: None,
                timeout_secs: None,
                action: beekeeper_workflow::ActionDef::Delay {
                    duration: "1s".into(),
                },
            }],
            enabled: true,
        };
        def.validate().expect("valid application_event workflow");
        self.state
            .db
            .upsert_workflow(
                self.community,
                id,
                Some(self.channel_id),
                &self.owner.public_key().to_bytes(),
                name,
                &serde_json::to_string(&def).expect("serialize"),
                &[5_u8; 32],
                None,
            )
            .await
            .expect("store workflow");
        id
    }

    /// Store a plugin-style kind:50000 signed by `signer`; returns it.
    async fn application_event(&self, signer: &Keys) -> beekeeper_core::StoredEvent {
        let event = EventBuilder::new(
            Kind::Custom(KIND_APPLICATION_EVENT as u16),
            r#"{"schema":1,"data":{"ref":"refs/heads/main"}}"#,
        )
        .tags([
            Tag::parse(["h", &self.channel_id.to_string()]).expect("h"),
            Tag::parse(["L", APPLICATION_EVENT_NAMESPACE]).expect("L"),
            Tag::parse(["l", PUSH, APPLICATION_EVENT_NAMESPACE]).expect("l"),
            Tag::parse(["beekeeper:source", "github", &self.installation.to_string()])
                .expect("source"),
        ])
        .sign_with_keys(signer)
        .expect("sign application event");
        let (stored, inserted) = self
            .state
            .db
            .insert_event(self.community, &event, Some(self.channel_id))
            .await
            .expect("insert application event");
        assert!(inserted);
        stored
    }

    async fn relay_event(&self) -> beekeeper_core::StoredEvent {
        self.application_event(&self.state.relay_keypair).await
    }

    async fn runs(&self, workflow: Uuid) -> Vec<beekeeper_db::workflow::WorkflowRunRecord> {
        self.state
            .db
            .list_workflow_runs(self.community, workflow, 20)
            .await
            .expect("list runs")
    }

    async fn outcomes(
        &self,
        event: &beekeeper_core::StoredEvent,
    ) -> Vec<beekeeper_db::workflow_dispatch::DispatchOutcome> {
        self.state
            .db
            .list_dispatch_outcomes(self.community, event.event.id.as_bytes())
            .await
            .expect("list outcomes")
    }

    async fn queue(&self, event: &beekeeper_core::StoredEvent) -> (String, i32, Option<String>) {
        sqlx::query_as(
            "SELECT state, attempts, terminal_reason FROM workflow_dispatch_queue \
             WHERE community_id = $1 AND event_id = $2",
        )
        .bind(self.community.as_uuid())
        .bind(event.event.id.as_bytes().as_slice())
        .fetch_one(&self.pool)
        .await
        .expect("read queue row")
    }

    async fn make_due(&self, event: &beekeeper_core::StoredEvent) {
        sqlx::query(
            "UPDATE workflow_dispatch_queue SET next_attempt_at = now() \
             WHERE community_id = $1 AND event_id = $2",
        )
        .bind(self.community.as_uuid())
        .bind(event.event.id.as_bytes().as_slice())
        .execute(&self.pool)
        .await
        .expect("make due");
    }

    /// Process this community's due rows until none are left.
    async fn drain(&self) {
        while run_once(&self.state, Some(self.community))
            .await
            .expect("dispatch batch")
            > 0
        {}
    }

    /// Make every run insert for `workflow` fail, as a DB error would.
    /// Returns the trigger name, for [`Self::heal`].
    async fn inject_run_insert_failure(&self, workflow: Uuid) -> String {
        let name = format!("inject_fail_{}", workflow.simple());
        sqlx::raw_sql(AssertSqlSafe(format!(
            "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN IF NEW.workflow_id = '{workflow}'::uuid THEN \
             RAISE EXCEPTION 'injected run insert failure'; END IF; RETURN NEW; END $$; \
             CREATE TRIGGER {name} BEFORE INSERT ON workflow_runs \
             FOR EACH ROW EXECUTE FUNCTION {name}();"
        )))
        .execute(&self.pool)
        .await
        .expect("inject failure");
        name
    }

    async fn heal(&self, name: &str) {
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP TRIGGER {name} ON workflow_runs; DROP FUNCTION {name}();"
        )))
        .execute(&self.pool)
        .await
        .expect("remove injected failure");
    }
}

fn disposition_of(
    outcomes: &[beekeeper_db::workflow_dispatch::DispatchOutcome],
    workflow: Uuid,
) -> &beekeeper_db::workflow_dispatch::DispatchOutcome {
    outcomes
        .iter()
        .find(|o| o.workflow_id == workflow)
        .unwrap_or_else(|| panic!("no outcome for {workflow}"))
}

/// The production fan-out (`dispatch_persistent_event`) plus the worker
/// start exactly one run per matching workflow, record a skip for the
/// non-matching one, and a member-signed copy fires nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres and Redis"]
async fn an_application_event_starts_exactly_one_run_per_matching_workflow() {
    let fx = fixture().await;
    let first = fx.workflow("first", PUSH).await;
    let second = fx.workflow("second", "com.github.*").await;
    let other_type = fx.workflow("other", "com.github.pull-request.*").await;
    let event = fx.relay_event().await;

    // The post-commit fan-out every stored event goes through. It must not
    // start a run itself: the queue is the only dispatch path for 50000.
    let tenant = beekeeper_core::tenant::TenantContext::resolved(fx.community, fx.host.clone());
    let relay_hex = fx.state.relay_keypair.public_key().to_hex();
    crate::handlers::event::dispatch_persistent_event(
        &tenant,
        &fx.state,
        &event,
        KIND_APPLICATION_EVENT,
        &relay_hex,
        None,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    fx.drain().await;
    fx.drain().await;

    for workflow in [first, second] {
        let runs = fx.runs(workflow).await;
        assert_eq!(runs.len(), 1, "exactly one run for {workflow}");
        assert_eq!(
            runs[0].trigger_event_id.as_deref(),
            Some(event.event.id.as_bytes().as_slice())
        );
    }
    assert!(fx.runs(other_type).await.is_empty());
    let outcomes = fx.outcomes(&event).await;
    assert_eq!(outcomes.len(), 3, "one frozen outcome per candidate");
    for workflow in [first, second] {
        let outcome = disposition_of(&outcomes, workflow);
        assert_eq!(outcome.disposition, "admitted");
        assert_eq!(outcome.run_id, Some(fx.runs(workflow).await[0].id));
    }
    let skipped = disposition_of(&outcomes, other_type);
    assert_eq!(skipped.disposition, "skipped");
    assert_eq!(
        skipped.reason.as_deref(),
        Some("application event type does not match")
    );
    assert_eq!(fx.queue(&event).await.0, "done");

    // A member can publish the same envelope; it is enqueued and frozen, but
    // never fires.
    let forged = fx.application_event(&fx.owner).await;
    fx.drain().await;
    assert_eq!(fx.runs(first).await.len(), 1);
    assert_eq!(fx.runs(second).await.len(), 1);
    let forged_outcomes = fx.outcomes(&forged).await;
    assert!(forged_outcomes.iter().all(|o| o.disposition == "skipped"
        && o.reason.as_deref() == Some("application event is not signed by the relay")));
    assert_eq!(fx.queue(&forged).await.0, "done");
}

/// Crash between admission and completion: the run exists, the row is still
/// claimed. After the lease expires the next claim resumes it and finds the
/// run — still exactly one run, outcome admitted, row done.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres and Redis"]
async fn a_crash_after_admission_resumes_without_a_second_run() {
    let fx = fixture().await;
    let workflow = fx.workflow("crash", PUSH).await;
    let event = fx.relay_event().await;

    let batch = fx
        .state
        .db
        .claim_dispatch_batch_in(Some(fx.community), 10, Utc::now() + TimeDelta::seconds(60))
        .await
        .expect("claim")
        .expect("due");
    let report = fx
        .state
        .workflow_engine
        .dispatch_claimed_event(fx.community, batch.claim_id, &batch.jobs[0].event, false)
        .await
        .expect("admit");
    assert_eq!(report.admitted.len(), 1);
    // ...and the worker dies here: no complete_dispatch.
    assert_eq!(fx.queue(&event).await.0, "claimed");
    sqlx::query(
        "UPDATE workflow_dispatch_queue SET lease_until = now() - interval '1 second' \
         WHERE community_id = $1 AND event_id = $2",
    )
    .bind(fx.community.as_uuid())
    .bind(event.event.id.as_bytes().as_slice())
    .execute(&fx.pool)
    .await
    .expect("expire lease");

    fx.drain().await;
    let runs = fx.runs(workflow).await;
    assert_eq!(runs.len(), 1, "the resumed row must not start a second run");
    let outcomes = fx.outcomes(&event).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].disposition, "admitted");
    assert_eq!(outcomes[0].run_id, Some(runs[0].id));
    assert_eq!(fx.queue(&event).await, ("done".to_owned(), 2, None));
}

/// Partial admission: A admits, B hits a DB error. The row goes back to
/// pending; the retry admits B and does not re-run A.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres and Redis"]
async fn a_partial_admission_retries_only_the_failed_workflow() {
    let fx = fixture().await;
    let healthy = fx.workflow("healthy", PUSH).await;
    let flaky = fx.workflow("flaky", PUSH).await;
    let injected = fx.inject_run_insert_failure(flaky).await;
    let event = fx.relay_event().await;

    fx.drain().await;
    let (state, attempts, _) = fx.queue(&event).await;
    assert_eq!((state.as_str(), attempts), ("pending", 1));
    assert_eq!(fx.runs(healthy).await.len(), 1);
    assert!(fx.runs(flaky).await.is_empty());
    let outcomes = fx.outcomes(&event).await;
    assert_eq!(disposition_of(&outcomes, healthy).disposition, "admitted");
    let pending = disposition_of(&outcomes, flaky);
    assert_eq!(pending.disposition, "pending");
    assert_eq!(pending.attempts, 1);
    assert!(pending
        .reason
        .as_deref()
        .is_some_and(|r| r.contains("injected run insert failure")));

    fx.heal(&injected).await;
    fx.make_due(&event).await;
    fx.drain().await;
    assert_eq!(fx.runs(healthy).await.len(), 1, "A is not re-run on retry");
    assert_eq!(fx.runs(flaky).await.len(), 1);
    let outcomes = fx.outcomes(&event).await;
    assert!(outcomes.iter().all(|o| o.disposition == "admitted"));
    assert_eq!(fx.queue(&event).await, ("done".to_owned(), 2, None));
}

/// A workflow that fails every attempt: after the 8th the row is failed and
/// kept, the failure is on the outcome, and the healthy admission stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres and Redis"]
async fn exhaustion_is_visible_on_the_row_and_the_outcome() {
    let fx = fixture().await;
    let healthy = fx.workflow("healthy", PUSH).await;
    let broken = fx.workflow("broken", PUSH).await;
    let injected = fx.inject_run_insert_failure(broken).await;
    let event = fx.relay_event().await;

    for _ in 0..beekeeper_db::workflow_dispatch::MAX_DISPATCH_ATTEMPTS {
        fx.make_due(&event).await;
        fx.drain().await;
    }
    fx.heal(&injected).await;
    assert_eq!(
        fx.queue(&event).await,
        (
            "failed".to_owned(),
            beekeeper_db::workflow_dispatch::MAX_DISPATCH_ATTEMPTS,
            Some(beekeeper_db::workflow_dispatch::REASON_ATTEMPTS_EXHAUSTED.to_owned())
        )
    );
    let outcomes = fx.outcomes(&event).await;
    assert_eq!(disposition_of(&outcomes, healthy).disposition, "admitted");
    let failed = disposition_of(&outcomes, broken);
    assert_eq!(failed.disposition, "failed");
    assert!(failed.reason.as_deref().is_some_and(
        |r| r.starts_with("attempts_exhausted") && r.contains("injected run insert failure")
    ));
    assert_eq!(fx.runs(healthy).await.len(), 1);
    assert!(fx.runs(broken).await.is_empty());
    fx.make_due(&event).await;
    fx.drain().await;
    assert_eq!(
        fx.queue(&event).await.0,
        "failed",
        "a failed row stays failed"
    );
}

/// The real worker loop, started the way `main` starts it, with no push
/// gateway configured: a committed application event becomes a run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres and Redis"]
async fn the_spawned_worker_dispatches_without_a_push_gateway() {
    let fx = fixture().await;
    assert!(fx.state.config.push_gateway_delivery_url.is_none());
    let workflow = fx.workflow("spawned", PUSH).await;
    let worker = spawn(Arc::clone(&fx.state));
    fx.relay_event().await;
    fx.state.workflow_dispatch_notify.notify_one();
    let fired = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !fx.runs(workflow).await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    worker.abort();
    fired.expect("the spawned worker must start the run");
    assert_eq!(fx.runs(workflow).await.len(), 1);
}

/// `main` starts the worker unconditionally: not inside the push-gateway
/// gate (or any other `if`), and after the action sink is wired.
#[test]
fn main_starts_the_worker_outside_any_feature_gate() {
    let main = include_str!("main.rs");
    let call = main
        .find("workflow_dispatch_runtime::spawn(")
        .expect("main starts the dispatch worker");
    let sink = main
        .find("workflow_engine.set_action_sink(")
        .expect("main wires the action sink");
    assert!(
        sink < call,
        "the worker starts after the action sink is wired"
    );
    // The statement sits at the cron-loop spawn's indentation: a top-level
    // statement of the startup body, not nested in a conditional block.
    let indent = |pos: usize| {
        let line_start = main[..pos].rfind('\n').map_or(0, |i| i + 1);
        main[line_start..].chars().take_while(|c| *c == ' ').count()
    };
    let cron = main
        .find("tokio::spawn(async move { wf_cron.run().await });")
        .expect("cron loop spawn");
    assert_eq!(
        indent(call),
        indent(cron),
        "the worker start must be at the cron loop's nesting level"
    );
    let gate = main
        .find("if state.config.push_gateway_delivery_url.is_some() {")
        .expect("push gateway gate");
    assert!(
        call < gate,
        "the worker start is not inside the push gateway gate"
    );
}
