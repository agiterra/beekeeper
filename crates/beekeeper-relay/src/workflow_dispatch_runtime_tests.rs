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

/// Crash between admission and execution: the run row committed (the
/// creator's `create_dispatch_run`), then the process died before
/// `execute_run` started it and before `complete_dispatch`. After the lease
/// expires the next claim resumes the row and finds the run — still exactly
/// one run, outcome admitted, row done.
///
/// This pins the KNOWN GAP honestly: the resumed row does not start the
/// existing run (only its creator would have), so the run stays `pending`
/// with `started_at` NULL and nothing restarts it; the stranded-run query
/// is what reports it. The crash is simulated at the DB layer because an
/// engine admission in this process would spawn `execute_run`, which a real
/// crash would not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres and Redis"]
async fn a_crash_after_admission_resumes_without_a_second_run() {
    let fx = fixture().await;
    let workflow = fx.workflow("crash", PUSH).await;
    let event = fx.relay_event().await;
    let event_id = event.event.id.as_bytes().to_vec();

    let batch = fx
        .state
        .db
        .claim_dispatch_batch_in(Some(fx.community), 10, Utc::now() + TimeDelta::seconds(60))
        .await
        .expect("claim")
        .expect("due");
    fx.state
        .db
        .freeze_dispatch_candidates(fx.community, &event_id, batch.claim_id, &[workflow])
        .await
        .expect("freeze");
    let created = fx
        .state
        .db
        .create_dispatch_run(fx.community, &event_id, workflow, None, &[5_u8; 32])
        .await
        .expect("admit");
    let beekeeper_db::workflow_dispatch::DispatchRunOutcome::Created(run_id) = created else {
        panic!("expected Created, got {created:?}");
    };
    // ...and the worker dies here: no execute_run, no complete_dispatch.
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
    assert_eq!(runs[0].id, run_id);
    let outcomes = fx.outcomes(&event).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].disposition, "admitted");
    assert_eq!(outcomes[0].run_id, Some(run_id));
    assert_eq!(fx.queue(&event).await, ("done".to_owned(), 2, None));

    // The gap: admitted, never started, and nothing restarts it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let run = fx
        .state
        .db
        .get_workflow_run(fx.community, run_id)
        .await
        .expect("load run");
    assert_eq!(run.status, beekeeper_db::workflow::RunStatus::Pending);
    assert!(run.started_at.is_none(), "nothing started the stranded run");
    let (_, stranded) = fx
        .state
        .db
        .list_stranded_dispatch_runs(Duration::ZERO, 100_000)
        .await
        .expect("list stranded");
    assert!(
        stranded
            .iter()
            .any(|r| r.community_id == fx.community && r.run_id == run_id),
        "the stranded-run query reports it"
    );
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

// ── end to end: plugin host seal -> relay signature -> B1 -> dispatch ──────

/// The built `test-webhook` component (`scripts/build-plugins.sh`). Fails,
/// never skips, when it is missing: this test proves the host's seal.
fn webhook_fixture() -> std::path::PathBuf {
    let dir = std::env::var_os("BEEKEEPER_PLUGIN_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugins")
        });
    let file = dir.join("test-webhook.wasm");
    assert!(
        file.exists(),
        "no built webhook fixture at {} — run scripts/build-plugins.sh or set \
         BEEKEEPER_PLUGIN_ARTIFACTS",
        file.display()
    );
    file
}

/// A host with the `test-webhook` component, granted `com.example`, and an
/// application-event validator standing in for ingest's (B1 runs the real
/// one on the signed event).
fn webhook_host() -> beekeeper_plugin_host::PluginHost {
    let file = webhook_fixture();
    let sha = beekeeper_plugin_host::sha256_hex(&std::fs::read(&file).expect("read fixture"));
    let text = format!(
        "[[plugin]]\nname = \"test-webhook\"\nworld = \"webhook-plugin@1\"\npath = \"{}\"\n\
         sha256 = \"{sha}\"\n[plugin.grants]\ntype_prefix = \"com.example\"\n",
        file.display()
    );
    let registry = beekeeper_plugin_host::Registry::from_toml(&text, std::path::Path::new("/"))
        .expect("registry");
    let validator = beekeeper_plugin_host::DraftKind::new(
        &[],
        Arc::new(|tags: &[Vec<String>], _content: &str| {
            let count = |key: &str| tags.iter().filter(|t| t[0] == key).count();
            if count("h") != 1 || count("L") != 1 || count("l") != 1 {
                return Err("requires exactly one h, L and l tag".to_owned());
            }
            Ok(beekeeper_plugin_host::DraftReferences::default())
        }),
    );
    beekeeper_plugin_host::PluginHost::load_with(
        &registry,
        beekeeper_plugin_host::HostConfig {
            validators: beekeeper_plugin_host::DraftValidators::new(),
            application_event: Some(validator),
        },
    )
    .expect("webhook host loads")
}

const WEBHOOK_SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

/// Run one signed `push` delivery through the host for `installation` and
/// sign the sealed event with `signer`, as the relay's ingress will.
fn sealed_push(
    host: &beekeeper_plugin_host::PluginHost,
    installation: &beekeeper_db::vendor_ingress::VendorInstallation,
    signer: &Keys,
) -> nostr::Event {
    use beekeeper_plugin_host::webhook::{Header, WebhookOutcome};
    use hmac::{Hmac, KeyInit, Mac};
    let body = br#"{"ref":"refs/heads/main"}"#;
    let mut mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET).expect("key");
    mac.update(body);
    let signature = hex::encode(mac.finalize().into_bytes());
    let headers = [
        ("x-signature", signature.as_str()),
        ("x-delivery", "72d3162e-cc78-11e3-81ab-4c9367dc0958"),
        ("x-event", "push"),
    ]
    .map(|(name, value)| Header {
        name: name.to_owned(),
        value: value.as_bytes().to_vec(),
    });
    let call = host
        .webhook_call("test-webhook", &installation.id.to_string())
        .expect("webhook call");
    let plan = call
        .authenticate(&headers)
        .expect("authenticate ran")
        .expect("auth plan");
    let received_at = u64::try_from(Utc::now().timestamp()).expect("positive time");
    let verified = call
        .verify(plan, WEBHOOK_SECRET, body.to_vec(), received_at)
        .expect("MAC verifies");
    let channel = installation.channel_id.to_string();
    let WebhookOutcome::Accept(sealed) = call
        .project(
            &verified,
            beekeeper_plugin_host::Delivery {
                channel: &channel,
                settings_json: "{}",
                headers: &[],
            },
        )
        .expect("projected")
    else {
        panic!("the fixture accepts push");
    };
    EventBuilder::new(
        Kind::Custom(u16::try_from(sealed.kind).expect("kind fits")),
        sealed.content,
    )
    .tags(
        sealed
            .tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("sealed tag")),
    )
    .custom_created_at(nostr::Timestamp::from(sealed.created_at))
    .sign_with_keys(signer)
    .expect("sign sealed event")
}

impl Fixture {
    /// A `test-webhook` installation into this fixture's channel; returns
    /// it and a fixture whose workflows name it.
    async fn with_installation(self) -> (Self, beekeeper_db::vendor_ingress::VendorInstallation) {
        let installation = self
            .state
            .db
            .create_vendor_installation(
                self.community,
                &beekeeper_db::vendor_ingress::NewInstallation {
                    plugin: "test-webhook",
                    channel_id: self.channel_id,
                    created_by: &self.owner.public_key().to_bytes(),
                    secret: WEBHOOK_SECRET,
                    settings: &serde_json::json!({}),
                },
            )
            .await
            .expect("create installation");
        let fx = Fixture {
            installation: installation.id,
            ..self
        };
        (fx, installation)
    }
}

fn accepted<'a>(
    id: &'a str,
    event: &'a nostr::Event,
) -> beekeeper_db::vendor_ingress::Delivery<'a> {
    beekeeper_db::vendor_ingress::Delivery {
        delivery_id: id,
        vendor_event: "push",
        plugin_digest: &[9_u8; 32],
        disposition: beekeeper_db::vendor_ingress::DeliveryDisposition::Accept { event },
    }
}

/// The whole slice: a delivery the plugin host verified and sealed, signed
/// with the relay's key and recorded at B1, is claimed by the real worker and
/// admitted against an `application_event` workflow naming the installation.
/// A foreign-signed copy of the same seal never reaches the queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres, Redis and the built test-webhook plugin"]
async fn a_sealed_webhook_delivery_recorded_at_b1_starts_a_run() {
    let (fx, installation) = fixture().await.with_installation().await;
    let workflow = fx.workflow("on-push", "com.example.push").await;
    let host = webhook_host();
    let relay = fx.state.relay_keypair.clone();
    let relay_pk = relay.public_key();

    let forged = sealed_push(&host, &installation, &Keys::generate());
    let refused = fx
        .state
        .db
        .record_vendor_delivery(&installation, &relay_pk, &accepted("forged", &forged))
        .await
        .expect_err("a foreign-signed seal is refused at B1");
    assert!(
        refused.to_string().contains("not signed by the relay"),
        "{refused}"
    );

    let event = sealed_push(&host, &installation, &relay);
    let source: Vec<&[String]> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice())
        .filter(|parts| parts[0] == "beekeeper:source")
        .collect();
    assert_eq!(
        source,
        vec![
            &[
                "beekeeper:source".to_owned(),
                "test-webhook".to_owned(),
                installation.id.to_string()
            ][..]
        ],
        "the host seals the installation as the source"
    );
    let outcome = fx
        .state
        .db
        .record_vendor_delivery(&installation, &relay_pk, &accepted("genuine", &event))
        .await
        .expect("record at B1");
    let beekeeper_db::vendor_ingress::DeliveryOutcome::Accepted(stored) = outcome else {
        panic!("expected Accepted, got {outcome:?}");
    };

    fx.drain().await;
    let runs = fx.runs(workflow).await;
    assert_eq!(runs.len(), 1, "exactly one run");
    assert_eq!(
        runs[0].trigger_event_id.as_deref(),
        Some(event.id.as_bytes().as_slice())
    );
    let outcomes = fx.outcomes(&stored).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].disposition, "admitted");
    assert_eq!(outcomes[0].run_id, Some(runs[0].id));
    assert_eq!(fx.queue(&stored).await.0, "done");
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_dispatch_queue WHERE community_id = $1")
            .bind(fx.community.as_uuid())
            .fetch_one(&fx.pool)
            .await
            .expect("count queue");
    assert_eq!(queued, 1, "the refused forgery never reached the queue");
}
