//! Ledger 248, 252: what project setup publishes for the seeded `verify`
//! action, and (Brian's 2026-09-25 ruling) the standing grant creation itself
//! now publishes right after it.

use super::project_verify_setup::{
    actions_channel_spec, plan_verify_publication, project_verify_setup_with_state,
    ProjectVerifySetup,
};

const PROJECT: &str =
    "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

/// The publication lands on the id `bee actions publish` uses and carries the
/// hash the relay will store, so the grant collected at setup binds the
/// definition every later run of the unchanged action names.
#[test]
fn setup_publishes_the_seeded_verify_under_the_shared_id_and_relay_hash() {
    let command: Vec<String> = ["cargo", "test", "--workspace"].map(str::to_owned).to_vec();
    let yml = buzz_persona_pkg::seed::seeded_actions_yml_with_verify(&command);
    let plan = plan_verify_publication(PROJECT, &yml).expect("plan");
    assert_eq!(
        plan.workflow_id,
        buzz_workflow_pkg::actions_file::action_workflow_id(PROJECT, "verify")
    );
    assert_eq!(plan.command, command);
    let (def, _) = buzz_workflow_pkg::WorkflowEngine::parse_yaml(&plan.yaml).expect("relay parse");
    assert_eq!(
        buzz_workflow_pkg::hash::definition_hash_hex(&def).expect("hash"),
        plan.definition_hash,
        "the relay hashes the published YAML to the hash setup shows"
    );
    assert_eq!(def.project.as_deref(), Some(PROJECT));
}

/// A file with no `verify`, or one whose `verify` runs nothing on a host, is
/// refused with words rather than publishing something no grant can cover.
#[test]
fn setup_refuses_a_file_without_a_host_verify() {
    let yml = "schema: buzz-project-actions/v1\nactions:\n  - name: other\n    trigger:\n      on: manual\n    steps:\n      - id: a\n        action: run_on_host\n        command: [\"true\"]\n";
    let error = plan_verify_publication(PROJECT, yml).expect_err("no verify");
    assert!(error.contains("verify"), "{error}");
}

/// Ledger 252, control run 6: the prior setup started a real run at the code
/// repository's empty seed commit purely to park it on a synthetic approval
/// gate and manufacture a kind:46010 for the "Approve and allow future runs"
/// click to answer — a run nothing had asked for, on a provider setup never
/// starts, that sat unclaimed and then ran red on a commit with no tests.
/// `ProjectVerifySetup` now carries no `run_id`/`trigger_event_id` at all:
/// the type itself proves setup can publish only the definition, never a
/// run.
#[test]
fn project_verify_setup_result_carries_no_run_or_trigger_fields() {
    let result = ProjectVerifySetup::default();
    let value = serde_json::to_value(&result).expect("serialize");
    let object = value.as_object().expect("object");
    assert!(
        !object.contains_key("runId"),
        "setup must not report a run id — it starts no run"
    );
    assert!(
        !object.contains_key("triggerEventId"),
        "setup must not report a trigger event id — it publishes no trigger"
    );
}

/// Ledger 252 (control run 3): setup filed `verify` in a private
/// `<slug>-actions` stream only the owner held, so the relay hid every
/// kind:46013 it published there from the project's host and every seat —
/// the run sat "requested on host" forever, and the lead that triggered it
/// was refused its status. Setup files it in the project's sessions
/// transport instead: the channel the host and the seats already read, named
/// exactly as `projectSessionsChannel.ts` names it so a later session create
/// resolves this channel rather than minting a second one.
#[test]
fn setup_files_verify_in_the_project_sessions_transport_not_a_private_stream() {
    let spec = actions_channel_spec("  Kettle   Control 3 ", "kettle-control-3");
    assert_eq!(spec.channel_type, "transport");
    assert_eq!(spec.visibility, "private");
    assert_eq!(spec.name, "Kettle Control 3 sessions");
    assert_eq!(spec.about, "Coding sessions for Kettle Control 3.");
    assert!(!spec.name.ends_with("-actions"));
    // No display name: the slug stands in, never a bare "sessions".
    assert_eq!(actions_channel_spec(" ", "kettle").name, "kettle sessions");
}

// ── Creation is consent: setup publishes the standing grant itself ─────────
//
// Run 7 (2026-09-25) found the control run 6 shape — setup publishes only
// the kind:30620, and a person must find and click a separate card before
// anything may run on their own computer — asked a question creation had
// already answered: this computer, with the owner's key, created the
// project. These tests flip that: setup must publish the grant too, in the
// same call, and must never publish it twice for the same hash.
#[cfg(not(target_os = "windows"))]
mod publishes_its_own_standing_grant {
    use std::sync::{Arc, Mutex};

    use axum::{
        extract::{Path, State},
        http::StatusCode,
        routing::{get, post},
        Router,
    };
    use nostr::{Event, JsonUtil, Keys};

    use super::project_verify_setup_with_state;

    const PROJECT: &str =
        "30621:2222222222222222222222222222222222222222222222222222222222222222:garden";

    fn tag_value(event: &Event, name: &str) -> Option<String> {
        event.tags.iter().find_map(|tag| {
            let parts = tag.as_slice();
            (parts.first().map(String::as_str) == Some(name))
                .then(|| parts.get(1).cloned())
                .flatten()
        })
    }

    /// A relay stand-in that stores every submitted event and answers
    /// `GET /workflows/{id}/autorun` the way the real relay's handler does:
    /// a grant is `active` only when its `definitionHash` tag matches the
    /// hash the relay would compute today from the latest stored kind:30620
    /// for that workflow id — never from a cached hash setup already moved
    /// past.
    #[derive(Default)]
    struct RelayStub {
        events: Mutex<Vec<Event>>,
    }

    async fn publish(State(stub): State<Arc<RelayStub>>, body: String) -> (StatusCode, String) {
        let event = Event::from_json(&body).expect("signed event");
        event.verify().expect("signature");
        stub.events.lock().expect("events").push(event.clone());
        (
            StatusCode::OK,
            serde_json::json!({
                "event_id": event.id.to_hex(),
                "accepted": true,
                "message": "stored",
            })
            .to_string(),
        )
    }

    async fn query(State(stub): State<Arc<RelayStub>>, body: String) -> (StatusCode, String) {
        let filters: Vec<serde_json::Value> = serde_json::from_str(&body).expect("filters");
        let filter = &filters[0];
        let kinds: Vec<u64> = filter["kinds"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_u64)
                    .collect()
            })
            .unwrap_or_default();
        let d_tag = filter["#d"][0].as_str().map(str::to_owned);
        let events = stub.events.lock().expect("events");
        let matches: Vec<Event> = events
            .iter()
            .filter(|event| kinds.is_empty() || kinds.contains(&(event.kind.as_u16() as u64)))
            .filter(|event| {
                d_tag
                    .as_deref()
                    .is_none_or(|wanted| tag_value(event, "d").as_deref() == Some(wanted))
            })
            .cloned()
            .collect();
        (
            StatusCode::OK,
            serde_json::to_string(&matches).expect("json"),
        )
    }

    async fn autorun(
        State(stub): State<Arc<RelayStub>>,
        Path(workflow_id): Path<String>,
    ) -> (StatusCode, String) {
        let events = stub.events.lock().expect("events");
        let current_hash = events
            .iter()
            .rev()
            .find(|event| {
                event.kind.as_u16() == 30620
                    && tag_value(event, "d").as_deref() == Some(&workflow_id)
            })
            .and_then(|event| {
                let (def, _) =
                    buzz_workflow_pkg::WorkflowEngine::parse_yaml(&event.content).ok()?;
                buzz_workflow_pkg::hash::definition_hash_hex(&def).ok()
            });
        let grants: Vec<serde_json::Value> = events
            .iter()
            .filter(|event| event.kind.as_u16() == 46030)
            .filter(|event| tag_value(event, "workflow").as_deref() == Some(&workflow_id))
            .filter_map(|event| {
                let hash = tag_value(event, "definitionHash")?;
                let matches_current = current_hash.as_deref() == Some(hash.as_str());
                Some(serde_json::json!({
                    "grant_event_id": event.id.to_hex(),
                    "definition_hash": hash,
                    "matches_current": matches_current,
                    "revoked_at": null,
                }))
            })
            .collect();
        let active = grants
            .iter()
            .any(|grant| grant["matches_current"] == serde_json::Value::Bool(true));
        (
            StatusCode::OK,
            serde_json::json!({
                "definition_hash": current_hash,
                "active": active,
                "grants": grants,
            })
            .to_string(),
        )
    }

    async fn relay_stub(stub: Arc<RelayStub>) -> (String, tokio::task::JoinHandle<()>) {
        let router = Router::new()
            .route("/events", post(publish))
            .route("/query", post(query))
            .route("/workflows/{workflow_id}/autorun", get(autorun))
            .with_state(stub);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        (url, task)
    }

    fn events_of_kind(events: &[Event], kind: u16) -> usize {
        events
            .iter()
            .filter(|event| event.kind.as_u16() == kind)
            .count()
    }

    /// The flip of the control-run-6/run-7 shape: creating the project must
    /// itself carry the standing grant — not a run, not a kind:46010, and
    /// not a second click. Both events land in the same call.
    #[tokio::test]
    async fn creating_a_project_publishes_the_verify_definition_and_its_standing_grant() {
        let stub = Arc::new(RelayStub::default());
        let (url, server) = relay_stub(stub.clone()).await;
        let state = crate::app_state::build_app_state();
        *state.keys.lock().expect("keys") = Keys::generate();
        *state.relay_url_override.lock().expect("relay") = Some(url);

        let command: Vec<String> = ["true"].map(str::to_owned).to_vec();
        let yml = buzz_persona_pkg::seed::seeded_actions_yml_with_verify(&command);
        let result = project_verify_setup_with_state(
            &state,
            PROJECT.to_string(),
            "Garden".to_string(),
            yml,
            "0".repeat(40),
        )
        .await
        .expect("setup");

        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.publish_event_id.is_some());
        assert!(
            result.grant_error.is_none(),
            "grant should not fail: {:?}",
            result.grant_error
        );
        assert!(
            result.grant_event_id.is_some(),
            "setup must publish the standing grant itself — creation is consent"
        );

        let events = stub.events.lock().expect("events").clone();
        assert_eq!(events_of_kind(&events, 30620), 1);
        assert_eq!(events_of_kind(&events, 46030), 1);
        assert_eq!(
            events_of_kind(&events, 46013),
            0,
            "no host-step request — setup starts no run"
        );
        assert_eq!(
            events_of_kind(&events, 46020),
            0,
            "no trigger — setup starts no run"
        );
        server.abort();
    }

    /// A second creation-shaped call (e.g. "Finish repository setup" run
    /// again against an unchanged definition) must find the grant it already
    /// holds and publish nothing a second time.
    #[tokio::test]
    async fn a_second_call_with_the_same_hash_grants_nothing_twice() {
        let stub = Arc::new(RelayStub::default());
        let (url, server) = relay_stub(stub.clone()).await;
        let state = crate::app_state::build_app_state();
        *state.keys.lock().expect("keys") = Keys::generate();
        *state.relay_url_override.lock().expect("relay") = Some(url);

        let command: Vec<String> = ["true"].map(str::to_owned).to_vec();
        let yml = buzz_persona_pkg::seed::seeded_actions_yml_with_verify(&command);

        let first = project_verify_setup_with_state(
            &state,
            PROJECT.to_string(),
            "Garden".to_string(),
            yml.clone(),
            "0".repeat(40),
        )
        .await
        .expect("first setup");
        let second = project_verify_setup_with_state(
            &state,
            PROJECT.to_string(),
            "Garden".to_string(),
            yml,
            "1".repeat(40),
        )
        .await
        .expect("second setup");

        assert_eq!(first.grant_event_id, second.grant_event_id);
        assert!(second.channel_reused, "the second call reuses the channel");
        let events = stub.events.lock().expect("events").clone();
        assert_eq!(
            events_of_kind(&events, 46030),
            1,
            "an unchanged hash must never be granted twice"
        );
        server.abort();
    }
}
