// Real HTTP webhook composition for the terminal CI-result action.
//
// This deliberately enters through `/hooks/{workflow}`. Direct sink tests
// cover individual authority refusals; this test proves the production router,
// webhook authentication, trigger templating, executor, relay sink, and event
// store compose into one durable relay-signed result.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use buzz_core::channel::{ChannelType, ChannelVisibility};
use buzz_core::ci_result::{build_ci_result, correlation_id, decode_ci_result, CiResult};
use buzz_core::kind::{KIND_CI_RESULT, KIND_GIT_REPO_ANNOUNCEMENT};
use buzz_db::{CreateCommunityWithOwnerResult, EventQuery};
use nostr::{EventBuilder, Kind, Tag};
use serde_json::{json, Value};
use std::process::{Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

struct WebhookCiFixture {
    state: Arc<crate::state::AppState>,
    community: buzz_core::CommunityId,
    host: String,
    owner: nostr::Keys,
    workflow_id: Uuid,
    secret: String,
    result: CiResult,
    correlation: String,
}

async fn fixture(request_host: Option<String>) -> WebhookCiFixture {
    let mut state = bridge_handler_test_state()
        .await
        .expect("configured Postgres, Redis and bridge test state must initialize");
    let owner = nostr::Keys::generate();
    let owner_hex = owner.public_key().to_hex();
    let bound_tcp = request_host.is_some();
    let host =
        request_host.unwrap_or_else(|| format!("webhook-ci-{}.example", Uuid::new_v4().simple()));
    // The real relay always has a stable signing key. The shared bridge test
    // state deliberately models an ephemeral open relay, whose NIP-11 document
    // omits `self`; `bee ci wait` correctly refuses to trust CI facts from it.
    // Mark this composition fixture's generated relay key as stable so NIP-11
    // publishes the exact key that signs the result below.
    let state_mut =
        Arc::get_mut(&mut state).expect("fresh bridge test state must be uniquely owned");
    let relay_secret = state_mut.relay_keypair.secret_key().to_secret_hex();
    let config = Arc::get_mut(&mut state_mut.config)
        .expect("fresh bridge test config must be uniquely owned");
    config.relay_private_key = Some(relay_secret);
    if bound_tcp {
        // AUTH verification derives its expected ws/wss scheme from the
        // deployment URL and its authority from this request's tenant host.
        config.relay_url = format!("ws://{host}");
    }
    state
        .workflow_engine
        .set_action_sink(Arc::new(crate::workflow_sink::RelayActionSink::new(&state)));

    let community = match state
        .db
        .create_community_with_owner(&host, &owner_hex)
        .await
        .expect("create test community")
    {
        CreateCommunityWithOwnerResult::Created(record) => record.id,
        other => panic!("expected a fresh test community, got {other:?}"),
    };
    state
        .db
        .ensure_user(community, &owner.public_key().to_bytes())
        .await
        .expect("register workflow owner");
    let channel = state
        .db
        .create_channel(
            community,
            "webhook-ci",
            ChannelType::Stream,
            ChannelVisibility::Open,
            None,
            &owner.public_key().to_bytes(),
            None,
            None,
        )
        .await
        .expect("create workflow channel");

    let project_slug = format!("ci-{}", Uuid::new_v4().simple());
    let repo_id = format!("repo-{}", Uuid::new_v4().simple());
    let project = format!("30621:{owner_hex}:{project_slug}");
    let repository = format!("30617:{owner_hex}:{repo_id}");
    let announcement = EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
        .tags([
            Tag::parse(["d", &repo_id]).expect("repo d tag"),
            Tag::parse(["project", &project]).expect("repo project tag"),
        ])
        .sign_with_keys(&owner)
        .expect("sign repository announcement");
    state
        .db
        .insert_event(community, &announcement, None)
        .await
        .expect("store repository announcement");

    // Maintain the same read-gate projections the normal 30621/30617 ingest
    // side effects maintain. This lets the final assertions exercise the real
    // private-project HTTP query boundary without re-testing project ingest.
    state
        .db
        .upsert_project_acl(
            community,
            &owner.public_key().to_bytes(),
            &project_slug,
            "private",
            &[],
            announcement.created_at.as_secs() as i64,
        )
        .await
        .expect("project private ACL projection");
    state
        .db
        .reserve_repo_name(community, &repo_id, &owner_hex)
        .await
        .expect("reserve repository name");
    state
        .db
        .set_repo_project_ref(
            community,
            &repo_id,
            &owner_hex,
            Some(&project),
            announcement.created_at.as_secs() as i64,
        )
        .await
        .expect("project repository link");

    let workflow_id = Uuid::new_v4();
    let secret = format!("test-secret-{}", Uuid::new_v4().simple());
    let definition = buzz_workflow::WorkflowDef {
        name: "Record exact CI completion".into(),
        description: None,
        trigger: buzz_workflow::TriggerDef::Webhook,
        steps: vec![buzz_workflow::Step {
            id: "record".into(),
            name: None,
            if_expr: None,
            timeout_secs: None,
            action: buzz_workflow::ActionDef::RecordCiResult {
                project: project.clone(),
                repository: repository.clone(),
                check: "required-ci".into(),
                phase: buzz_core::ci_result::CiPhase::Build,
                commit: "{{trigger.commit}}".into(),
                run: "{{trigger.run}}".into(),
                attempt: "{{trigger.attempt}}".into(),
                conclusion: "{{trigger.conclusion}}".into(),
                evidence_url: Some("{{trigger.evidence_url}}".into()),
                summary: Some("{{trigger.summary}}".into()),
            },
        }],
        enabled: true,
    };
    let mut definition_value = serde_json::to_value(&definition).expect("serialize definition");
    crate::webhook_secret::inject_secret(&mut definition_value, &secret);
    state
        .db
        .upsert_workflow(
            community,
            workflow_id,
            Some(channel.id),
            &owner.public_key().to_bytes(),
            &definition.name,
            &definition_value.to_string(),
            &[9_u8; 32],
        )
        .await
        .expect("store webhook workflow");

    let result = CiResult {
        schema: buzz_core::ci_result::CI_RESULT_SCHEMA.into(),
        identity: buzz_core::ci_result::CiResultIdentity {
            project,
            repository,
            commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            check: "required-ci".into(),
            run: "pipeline/731".into(),
            attempt: 2,
            workflow: workflow_id.to_string(),
            phase: buzz_core::ci_result::CiPhase::Build,
        },
        conclusion: buzz_core::ci_result::CiConclusion::Success,
        evidence_url: Some("https://ci.example/runs/731".into()),
        summary: Some("all required jobs passed".into()),
    };
    let correlation = correlation_id(&result.identity).expect("valid correlation");
    WebhookCiFixture {
        state,
        community,
        host,
        owner,
        workflow_id,
        secret,
        result,
        correlation,
    }
}

async fn post_hook(fixture: &WebhookCiFixture, secret: &str) -> axum::response::Response<Body> {
    post_hook_body(fixture, secret, webhook_body(fixture)).await
}

async fn post_hook_body(
    fixture: &WebhookCiFixture,
    secret: &str,
    body: String,
) -> axum::response::Response<Body> {
    crate::router::build_router(Arc::clone(&fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/hooks/{}", fixture.workflow_id))
                .header(header::HOST, &fixture.host)
                .header("x-webhook-secret", secret)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .expect("build webhook request"),
        )
        .await
        .expect("route webhook request")
}

fn webhook_body(fixture: &WebhookCiFixture) -> String {
    json!({
        "commit": fixture.result.identity.commit,
        "run": fixture.result.identity.run,
        "attempt": fixture.result.identity.attempt,
        "conclusion": "success",
        "evidence_url": fixture.result.evidence_url,
        "summary": fixture.result.summary,
    })
    .to_string()
}

async fn post_hook_over_tcp(fixture: &WebhookCiFixture, relay_url: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(format!("{relay_url}/hooks/{}", fixture.workflow_id))
        .header("x-webhook-secret", &fixture.secret)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(webhook_body(fixture))
        .send()
        .await
        .expect("post callback to bound relay")
        .status()
}

async fn stored_results(fixture: &WebhookCiFixture) -> Vec<buzz_core::StoredEvent> {
    fixture
        .state
        .db
        .query_events(&EventQuery {
            kinds: Some(vec![KIND_CI_RESULT as i32]),
            tags_containing: Some(vec![("d".into(), fixture.correlation.clone())]),
            global_only: true,
            limit: Some(10),
            ..EventQuery::for_community(fixture.community)
        })
        .await
        .expect("query stored CI results")
}

async fn query_as(fixture: &WebhookCiFixture, reader: &nostr::Keys) -> (StatusCode, Value) {
    let response = crate::router::build_router(Arc::clone(&fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/query")
                .header(header::HOST, &fixture.host)
                .header("x-pubkey", reader.public_key().to_hex())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!([{"kinds": [KIND_CI_RESULT], "#d": [fixture.correlation]}]).to_string(),
                ))
                .expect("build query request"),
        )
        .await
        .expect("route query request");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read query response");
    let value = serde_json::from_slice(&body).expect("query JSON response");
    (status, value)
}

fn bee_wait_command(
    fixture: &WebhookCiFixture,
    bee_bin: &str,
    relay_url: &str,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(bee_bin);
    command
        .args([
            "ci",
            "wait",
            "--project",
            &fixture.result.identity.project,
            "--repo",
            &fixture.result.identity.repository,
            "--commit",
            &fixture.result.identity.commit,
            "--check",
            &fixture.result.identity.check,
            "--run",
            &fixture.result.identity.run,
            "--attempt",
            &fixture.result.identity.attempt.to_string(),
            "--workflow",
            &fixture.result.identity.workflow,
            "--phase",
            "build",
            "--timeout",
            "10",
        ])
        .env("BUZZ_RELAY_URL", relay_url)
        .env(
            "BUZZ_PRIVATE_KEY",
            fixture.owner.secret_key().to_secret_hex(),
        )
        .env_remove("BUZZ_AUTH_TAG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

fn assert_bee_success(output: &Output, expected: &CiResult) -> Value {
    assert!(
        output.status.success(),
        "bee ci wait must exit successfully; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("bee JSON output");
    assert_eq!(value["result"], serde_json::to_value(expected).unwrap());
    assert!(
        value["event_id"]
            .as_str()
            .is_some_and(|event_id| event_id.len() == 64),
        "bee output must identify the accepted relay event"
    );
    value
}

#[tokio::test]
#[ignore = "requires Postgres and Redis"]
async fn ci_result_binding_rejects_unknown_repo_wrong_project_and_foreign_owner() {
    use crate::workflow_ci_result::{authorize_ci_result_binding, CiResultAuthorityError};

    let fixture = fixture(None).await;
    let owner_bytes = fixture.owner.public_key().to_bytes();
    let owner_hex = fixture.owner.public_key().to_hex();
    let unknown_repo = format!("30617:{owner_hex}:missing-{}", Uuid::new_v4().simple());
    let unknown = authorize_ci_result_binding(
        &fixture.state,
        fixture.community,
        &owner_bytes,
        &fixture.result.identity.project,
        &unknown_repo,
    )
    .await;
    assert!(matches!(
        unknown,
        Err(CiResultAuthorityError::Unauthorized(_))
    ));

    let wrong_project = format!("30621:{owner_hex}:other-{}", Uuid::new_v4().simple());
    let mismatched = authorize_ci_result_binding(
        &fixture.state,
        fixture.community,
        &owner_bytes,
        &wrong_project,
        &fixture.result.identity.repository,
    )
    .await;
    assert!(matches!(
        mismatched,
        Err(CiResultAuthorityError::Unauthorized(_))
    ));

    let foreign = nostr::Keys::generate();
    let unowned = authorize_ci_result_binding(
        &fixture.state,
        fixture.community,
        &foreign.public_key().to_bytes(),
        &fixture.result.identity.project,
        &fixture.result.identity.repository,
    )
    .await;
    assert!(matches!(
        unowned,
        Err(CiResultAuthorityError::Unauthorized(_))
    ));
}

#[tokio::test]
#[ignore = "requires Postgres and Redis"]
async fn ci_result_webhook_route_authenticates_executes_and_stores_relay_fact() {
    let fixture = fixture(None).await;

    let rejected = post_hook(&fixture, "wrong-secret").await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    assert!(stored_results(&fixture).await.is_empty());

    // A callback cannot create a fake identity by leaving one of the strict
    // producer templates unresolved. HTTP acceptance only means the webhook
    // was authenticated and queued, so prove the durable asynchronous result.
    let missing_run = post_hook_body(
        &fixture,
        &fixture.secret,
        json!({
            "commit": fixture.result.identity.commit,
            "attempt": fixture.result.identity.attempt,
            "conclusion": "success",
            "evidence_url": fixture.result.evidence_url,
            "summary": fixture.result.summary,
        })
        .to_string(),
    )
    .await;
    assert_eq!(missing_run.status(), StatusCode::ACCEPTED);
    let missing_run_body = to_bytes(missing_run.into_body(), 1024 * 1024)
        .await
        .expect("read missing-run webhook response");
    let missing_run_id = serde_json::from_slice::<Value>(&missing_run_body)
        .expect("missing-run webhook JSON response")["run_id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("missing-run webhook response must identify its workflow run");
    let failed_run = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let run = fixture
                .state
                .db
                .get_workflow_run(fixture.community, missing_run_id)
                .await
                .expect("load missing-run workflow execution");
            if run.status == buzz_db::workflow::RunStatus::Failed {
                break run;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("missing-run workflow execution must fail");
    assert_eq!(
        failed_run.error_code.as_deref(),
        Some("template_resolution_failed")
    );
    assert!(stored_results(&fixture).await.is_empty());

    // Even a structurally valid client-signed 46008 cannot bypass the
    // authenticated producer route.
    let client = nostr::Keys::generate();
    let (tags, content) = build_ci_result(&fixture.result).expect("build direct result");
    let direct = EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), content)
        .tags(tags.into_iter().map(|tag| Tag::parse(tag).expect("CI tag")))
        .sign_with_keys(&client)
        .expect("sign direct result");
    let direct_status = post_events(
        Arc::clone(&fixture.state),
        &fixture.host,
        &client.public_key().to_hex(),
        &serde_json::to_vec(&direct).expect("serialize direct result"),
    )
    .await;
    assert_eq!(direct_status, StatusCode::BAD_REQUEST);
    assert!(stored_results(&fixture).await.is_empty());

    let accepted = post_hook(&fixture, &fixture.secret).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);

    let stored = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = stored_results(&fixture).await;
            if let Some(event) = events.into_iter().next() {
                break event;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("webhook execution must store a CI result");
    assert_eq!(
        stored.event.pubkey,
        fixture.state.relay_keypair.public_key()
    );
    stored.event.verify().expect("relay result signature");
    assert_eq!(decode_ci_result(&stored.event).unwrap(), fixture.result);

    // The event is repository-bound, so the real HTTP query path follows the
    // repository's private-project visibility projection.
    let outsider = nostr::Keys::generate();
    let (status, outside) = query_as(&fixture, &outsider).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outside, json!([]));
    let (status, owner_view) = query_as(&fixture, &fixture.owner).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_view.as_array().map(Vec::len), Some(1));
}

#[tokio::test]
#[ignore = "requires Postgres, Redis, and BUZZ_TEST_BEE_BIN"]
async fn ci_result_real_bee_wait_receives_live_then_replays_stored_result() {
    let bee_bin = std::env::var("BUZZ_TEST_BEE_BIN")
        .expect("BUZZ_TEST_BEE_BIN must name the explicitly built bee binary");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind relay test listener");
    let address = listener.local_addr().expect("relay test address");
    let fixture = fixture(Some(address.to_string())).await;
    let relay_url = format!("http://{address}");
    let router = crate::router::build_router(Arc::clone(&fixture.state));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .expect("serve local relay router");
    });

    // Start the real process first and wait for its exact REQ to enter the
    // relay registry. This distinguishes the live-delivery path from a replay
    // that happened to win a scheduling race.
    let mut live_child = bee_wait_command(&fixture, &bee_bin, &relay_url)
        .spawn()
        .expect("spawn live bee wait");
    let subscription = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if fixture.state.sub_registry.total_subscriptions() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if subscription.is_err() {
        match live_child.try_wait().expect("inspect bee process status") {
            Some(status) => {
                let output = live_child
                    .wait_with_output()
                    .await
                    .expect("collect failed bee output");
                panic!(
                    "bee exited as {status} before registering its REQ; stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => panic!("bee remained running but did not register its REQ within five seconds"),
        }
    }

    let callback_status = post_hook_over_tcp(&fixture, &relay_url).await;
    assert_eq!(callback_status, StatusCode::ACCEPTED);
    let live_output = tokio::time::timeout(Duration::from_secs(12), live_child.wait_with_output())
        .await
        .expect("live bee wait must terminate")
        .expect("collect live bee output");
    let live = assert_bee_success(&live_output, &fixture.result);

    // A fresh process starts only after the event is durable. It must return
    // the same event from replay through EOSE, proving the command works on
    // both sides of the subscription timing boundary.
    let replay_output = tokio::time::timeout(
        Duration::from_secs(12),
        bee_wait_command(&fixture, &bee_bin, &relay_url).output(),
    )
    .await
    .expect("replay bee wait must terminate")
    .expect("run replay bee wait");
    let replay = assert_bee_success(&replay_output, &fixture.result);
    assert_eq!(replay["event_id"], live["event_id"]);

    server.abort();
    let _ = server.await;
}
