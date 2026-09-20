//! A run executes the definition it was created from, or it stops.
//!
//! These cases drive the real engine against Postgres. They are the W4 lane's
//! reproduction and its proof: a project action's approval is the operator's
//! consent to *one* definition running on their machine, so the run must be
//! bound to the definition selected when it was created, and every later
//! act — the approval itself, each resume, and the moment a host-step request
//! is emitted — must check that binding.
//!
//! The kind:46013 host-step request carries **no command text** (spec § 5.5):
//! it carries `definitionHash`, and the host compiles the command from its own
//! copy of the project's `actions.yml` and compares hashes
//! (`crates/buzz-session-provider/src/action_steps.rs`). So "the changed
//! command was emitted" is exactly "a host-step request naming the *new*
//! definition hash was published for a run the operator approved under the
//! *old* one" — the host would then compile and run the new command and find
//! nothing wrong.

use std::sync::Arc;

use buzz_db::workflow::{ApprovalStatus, RunStatus};
use uuid::Uuid;

use crate::executor::{self, TriggerContext};
use crate::suspend::{resume_index_after_approval, Suspension};
use crate::tests::{setup_channel, setup_db, RecordingSink};
use crate::{WorkflowConfig, WorkflowEngine};

/// A one-step project action whose single `run_on_host` command is `command`.
fn one_step_yaml(project: &str, command: &str) -> String {
    format!(
        "name: verify\nproject: '{project}'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"echo\", \"{command}\"]\n"
    )
}

/// How many published host-step requests name `hash` as the definition the
/// host should compile its command from.
fn requests_naming(sink: &RecordingSink, hash: &[u8]) -> usize {
    let hash = hex::encode(hash);
    sink.host_steps
        .lock()
        .expect("host steps lock")
        .iter()
        .filter(|request| request.definition_hash == hash)
        .count()
}

struct Fixture {
    db: buzz_db::Db,
    community: buzz_core::tenant::CommunityId,
    engine: Arc<WorkflowEngine>,
    sink: Arc<RecordingSink>,
    workflow_id: Uuid,
    project: String,
}

impl Fixture {
    /// A community, a channel, a published one-step action and an engine
    /// whose sink records what would have been published.
    async fn new(command: &str) -> (Self, crate::schema::WorkflowDef, Vec<u8>) {
        Self::with_yaml(|project| one_step_yaml(project, command)).await
    }

    async fn with_yaml(
        yaml: impl Fn(&str) -> String,
    ) -> (Self, crate::schema::WorkflowDef, Vec<u8>) {
        let db = setup_db().await;
        let owner = nostr::Keys::generate().public_key().to_bytes().to_vec();
        let member = nostr::Keys::generate().public_key().to_bytes().to_vec();
        let (community, channel_id) = setup_channel(&db, &owner, &member).await;
        let project = format!("30621:{}:pulse", "1".repeat(64));
        let (def, canonical) = WorkflowEngine::parse_yaml(&yaml(&project)).expect("parse v1");
        let hash = crate::hash::definition_hash(&def).expect("hash v1");
        let workflow_id = db
            .create_workflow(
                community,
                Some(channel_id),
                &owner,
                "verify",
                &canonical,
                &hash,
                Some(&project),
            )
            .await
            .expect("create workflow");
        let sink = Arc::new(RecordingSink::default());
        let engine = Arc::new(WorkflowEngine::new(db.clone(), WorkflowConfig::default()));
        engine.set_action_sink(sink.clone());
        (
            Self {
                db,
                community,
                engine,
                sink,
                workflow_id,
                project,
            },
            def,
            hash,
        )
    }

    /// Republish the action under the same id, as an edit does.
    async fn publish(&self, yaml: &str) -> (crate::schema::WorkflowDef, Vec<u8>) {
        let (def, canonical) = WorkflowEngine::parse_yaml(yaml).expect("parse edit");
        let hash = crate::hash::definition_hash(&def).expect("hash edit");
        self.db
            .update_workflow(
                self.community,
                self.workflow_id,
                "verify",
                &canonical,
                &hash,
                Some(&self.project),
            )
            .await
            .expect("update workflow");
        (def, hash)
    }

    /// Grant the run's single pending approval, as the operator's kind:46030
    /// does through `handle_approval_grant`.
    async fn grant_the_pending_approval(&self, run_id: Uuid) -> i32 {
        let approvals = self
            .db
            .get_run_approvals(self.community, self.workflow_id, run_id)
            .await
            .expect("approvals");
        let pending = approvals
            .iter()
            .find(|approval| approval.status == ApprovalStatus::Pending)
            .expect("one pending approval");
        assert!(
            self.db
                .update_approval_by_stored_hash(
                    self.community,
                    &pending.token,
                    ApprovalStatus::Granted,
                    None,
                    None,
                )
                .await
                .expect("grant"),
            "the approval must move to granted"
        );
        pending.step_index
    }

    /// Exactly what `resume_workflow_after_approval` does in the relay
    /// (`crates/buzz-relay/src/handlers/command_executor.rs`): read the
    /// **current** stored definition and continue the parked run in it.
    async fn resume_after_approval(&self, run_id: Uuid, approval_step_index: i32) {
        let run = self
            .db
            .get_workflow_run(self.community, run_id)
            .await
            .expect("run");
        assert_eq!(run.status, RunStatus::WaitingApproval);
        let workflow = self
            .db
            .get_workflow(self.community, self.workflow_id)
            .await
            .expect("workflow");
        let def: crate::schema::WorkflowDef =
            serde_json::from_value(workflow.definition.clone()).expect("current definition");
        let resume_index = resume_index_after_approval(&def, approval_step_index.max(0) as usize);
        let outputs = outputs_from_trace(&run.execution_trace);
        let trigger_ctx: TriggerContext = run
            .trigger_context
            .as_ref()
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        let existing_trace = run.execution_trace.as_array().cloned();
        let result = executor::execute_from_step(
            &self.engine,
            self.community,
            run_id,
            &def,
            &trigger_ctx,
            resume_index,
            Some(outputs),
        )
        .await;
        self.engine
            .finalize_run(self.community, run_id, result, existing_trace)
            .await;
    }

    /// Exactly what `resume_workflow_after_host_step` does in the relay
    /// (`crates/buzz-relay/src/handlers/host_steps.rs`): record the step's
    /// output in the trace and continue the run in the **current** stored
    /// definition. A run that is no longer waiting on a host is left alone,
    /// which is how a replayed result changes nothing.
    async fn resume_after_host_step(
        &self,
        run_id: Uuid,
        step_index: usize,
        step_id: &str,
        output: serde_json::Value,
    ) {
        let run = self
            .db
            .get_workflow_run(self.community, run_id)
            .await
            .expect("run");
        if run.status != RunStatus::WaitingHost {
            return;
        }
        let workflow = self
            .db
            .get_workflow(self.community, self.workflow_id)
            .await
            .expect("workflow");
        let def: crate::schema::WorkflowDef =
            serde_json::from_value(workflow.definition.clone()).expect("current definition");
        let mut outputs = outputs_from_trace(&run.execution_trace);
        outputs.insert(step_id.to_owned(), output.clone());
        let mut trace = run.execution_trace.as_array().cloned().unwrap_or_default();
        trace.push(serde_json::json!({
            "step_id": step_id,
            "status": "completed",
            "output": output,
        }));
        self.db
            .update_workflow_run(
                self.community,
                run_id,
                RunStatus::WaitingHost,
                step_index as i32,
                &serde_json::Value::Array(trace.clone()),
                None,
            )
            .await
            .expect("record host step output");
        let trigger_ctx: TriggerContext = run
            .trigger_context
            .as_ref()
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        let result = executor::execute_from_step(
            &self.engine,
            self.community,
            run_id,
            &def,
            &trigger_ctx,
            step_index + 1,
            Some(outputs),
        )
        .await;
        self.engine
            .finalize_run(self.community, run_id, result, Some(trace))
            .await;
    }

    /// The run row, after the fact.
    async fn run(&self, run_id: Uuid) -> buzz_db::workflow::WorkflowRunRecord {
        self.db
            .get_workflow_run(self.community, run_id)
            .await
            .expect("run")
    }
}

/// The relay's trace → outputs reconstruction, duplicated here so the engine
/// test does not depend on `buzz-relay`.
fn outputs_from_trace(
    trace: &serde_json::Value,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut outputs = std::collections::HashMap::new();
    if let Some(entries) = trace.as_array() {
        for entry in entries {
            if let (Some(step_id), Some(output)) = (
                entry.get("step_id").and_then(serde_json::Value::as_str),
                entry.get("output"),
            ) {
                outputs.insert(step_id.to_owned(), output.clone());
            }
        }
    }
    outputs
}

/// **The counterexample.** Publish v1, trigger it, leave it waiting for the
/// owner's approval; publish v2 with the same step id and a different
/// command; grant the *old* request. v2's command must be emitted to a host
/// exactly zero times.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_changed_command_never_runs_under_the_earlier_approval() {
    let (fixture, def_v1, hash_v1) = Fixture::new("v1").await;
    let run_id = fixture
        .db
        .create_workflow_run(fixture.community, fixture.workflow_id, None, None)
        .await
        .expect("create run");

    // 1. The run parks on the synthetic gate: nothing has been handed to a host.
    let result = executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_id,
        &def_v1,
        &TriggerContext::default(),
    )
    .await
    .expect("first segment");
    assert!(
        matches!(
            result.suspension,
            Some(Suspension::Approval {
                synthetic: true,
                ..
            })
        ),
        "a project action's host step gates on the owner's approval"
    );
    assert_eq!(requests_naming(&fixture.sink, &hash_v1), 0);

    // 2. The owner is looking at v1. Meanwhile the action is edited: same
    //    step id, a different command.
    let (_def_v2, hash_v2) = fixture
        .publish(&one_step_yaml(&fixture.project, "v2"))
        .await;
    assert_ne!(hash_v1, hash_v2, "an edited command is a different hash");

    // 3. The owner grants the request they were shown, and the relay resumes.
    let step_index = fixture.grant_the_pending_approval(run_id).await;
    fixture.resume_after_approval(run_id, step_index).await;

    // 4. The whole point.
    assert_eq!(
        requests_naming(&fixture.sink, &hash_v2),
        0,
        "a command the owner never approved was handed to a host"
    );
    assert_eq!(
        requests_naming(&fixture.sink, &hash_v1),
        0,
        "and the definition the owner did approve is gone, so nothing runs"
    );
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error_code.as_deref(),
        Some(crate::RUN_STOPPED_DEFINITION_CHANGED)
    );
}

/// How many published host-step requests name `step_id`.
fn requests_for_step(sink: &RecordingSink, step_id: &str) -> usize {
    sink.host_steps
        .lock()
        .expect("host steps lock")
        .iter()
        .filter(|request| request.step_id == step_id)
        .count()
}

/// The approval an operator granted for an unchanged definition still
/// releases the step: the fence refuses substitutions, not approvals.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_hash_stable_approval_still_releases_the_step() {
    let (fixture, def_v1, hash_v1) = Fixture::new("v1").await;
    let run_id = fixture
        .db
        .create_workflow_run(fixture.community, fixture.workflow_id, None, None)
        .await
        .expect("create run");
    executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_id,
        &def_v1,
        &TriggerContext::default(),
    )
    .await
    .expect("first segment");

    let step_index = fixture.grant_the_pending_approval(run_id).await;
    fixture.resume_after_approval(run_id, step_index).await;

    assert_eq!(
        requests_naming(&fixture.sink, &hash_v1),
        1,
        "the approved definition is handed to a host exactly once"
    );
    assert_eq!(fixture.run(run_id).await.status, RunStatus::WaitingHost);
}

/// Lane 184's bound commit is still bound on the far side of the approval:
/// the operator approved a run that names a commit, and the request the host
/// receives names the same one.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_bound_checkout_stays_bound_across_the_approval() {
    let (fixture, def_v1, _hash_v1) = Fixture::new("v1").await;
    let sha = "a".repeat(40);
    let trigger_ctx = TriggerContext {
        checkout: sha.clone(),
        ..Default::default()
    };
    let trigger_json = serde_json::to_value(&trigger_ctx).expect("trigger context");
    let run_id = fixture
        .db
        .create_workflow_run(
            fixture.community,
            fixture.workflow_id,
            None,
            Some(&trigger_json),
        )
        .await
        .expect("create run");
    executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_id,
        &def_v1,
        &trigger_ctx,
    )
    .await
    .expect("first segment");

    let step_index = fixture.grant_the_pending_approval(run_id).await;
    fixture.resume_after_approval(run_id, step_index).await;

    let requests = fixture.sink.host_steps.lock().expect("lock");
    let request = requests.first().expect("one host-step request");
    assert_eq!(
        request
            .trigger_context
            .get("checkout")
            .and_then(|v| v.as_str()),
        Some(sha.as_str()),
        "the commit the run was bound to survives the approval"
    );
}

/// An edit **between two steps** of a run in flight stops it: the second
/// command is never requested, even though the first already ran.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_edit_between_two_steps_stops_the_run() {
    let two_steps = |project: &str, first: &str, second: &str| {
        format!(
            "name: verify\nproject: '{project}'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"echo\", \"{first}\"]\n  - id: publish\n    action: run_on_host\n    command: [\"echo\", \"{second}\"]\n"
        )
    };
    let (fixture, def_v1, hash_v1) =
        Fixture::with_yaml(|project| two_steps(project, "build-v1", "publish-v1")).await;
    // An autorun grant on v1 releases both host steps without a gate, so the
    // only thing that can stop the second one is the binding.
    fixture
        .db
        .create_autorun_grant(
            fixture.community,
            fixture.workflow_id,
            &hash_v1,
            &[0x33; 32],
            &[0x11; 32],
        )
        .await
        .expect("grant");
    let run_id = fixture
        .db
        .create_workflow_run(fixture.community, fixture.workflow_id, None, None)
        .await
        .expect("create run");
    executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_id,
        &def_v1,
        &TriggerContext::default(),
    )
    .await
    .expect("first segment");
    assert_eq!(requests_for_step(&fixture.sink, "build"), 1);

    // `build` comes back, and the action is edited before `publish` is asked for.
    let (_def_v2, hash_v2) = fixture
        .publish(&two_steps(&fixture.project, "build-v1", "publish-v2"))
        .await;
    fixture
        .resume_after_host_step(run_id, 0, "build", serde_json::json!({ "exit_code": 0 }))
        .await;

    assert_eq!(
        requests_for_step(&fixture.sink, "publish"),
        0,
        "the second command was never requested"
    );
    assert_eq!(requests_naming(&fixture.sink, &hash_v2), 0);
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error_code.as_deref(),
        Some(crate::RUN_STOPPED_DEFINITION_CHANGED)
    );
}

/// A host's result delivered twice does not ask for the command again.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_duplicate_result_does_not_repeat_the_command() {
    let with_notify = |project: &str| {
        format!(
            "name: verify\nproject: '{project}'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"echo\", \"v1\"]\n  - id: notify\n    action: send_message\n    text: done\n"
        )
    };
    let (fixture, def_v1, hash_v1) = Fixture::with_yaml(with_notify).await;
    fixture
        .db
        .create_autorun_grant(
            fixture.community,
            fixture.workflow_id,
            &hash_v1,
            &[0x33; 32],
            &[0x11; 32],
        )
        .await
        .expect("grant");
    let run_id = fixture
        .db
        .create_workflow_run(fixture.community, fixture.workflow_id, None, None)
        .await
        .expect("create run");
    executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_id,
        &def_v1,
        &TriggerContext::default(),
    )
    .await
    .expect("first segment");
    assert_eq!(requests_for_step(&fixture.sink, "build"), 1);

    let output = serde_json::json!({ "exit_code": 0 });
    fixture
        .resume_after_host_step(run_id, 0, "build", output.clone())
        .await;
    assert_eq!(fixture.run(run_id).await.status, RunStatus::Completed);

    // The same result again: the run is no longer waiting on a host, so the
    // replay changes nothing and asks for nothing.
    fixture
        .resume_after_host_step(run_id, 0, "build", output)
        .await;
    assert_eq!(
        requests_for_step(&fixture.sink, "build"),
        1,
        "a replayed result must not request the command a second time"
    );
}

/// An autorun grant is bound to one definition, and an edit re-arms the gate
/// exactly as it did before runs carried a binding.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_autorun_grant_re_arms_on_a_changed_definition() {
    let (fixture, _def_v1, hash_v1) = Fixture::new("v1").await;
    fixture
        .db
        .create_autorun_grant(
            fixture.community,
            fixture.workflow_id,
            &hash_v1,
            &[0x33; 32],
            &[0x11; 32],
        )
        .await
        .expect("grant v1");

    // Edited: a fresh run binds v2, and v1's grant does not release it.
    let (def_v2, hash_v2) = fixture
        .publish(&one_step_yaml(&fixture.project, "v2"))
        .await;
    let run_v2 = fixture
        .db
        .create_workflow_run(fixture.community, fixture.workflow_id, None, None)
        .await
        .expect("run v2");
    let result = executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_v2,
        &def_v2,
        &TriggerContext::default(),
    )
    .await
    .expect("segment v2");
    assert!(
        matches!(
            result.suspension,
            Some(Suspension::Approval {
                synthetic: true,
                ..
            })
        ),
        "an edit re-arms the gate"
    );
    assert_eq!(requests_naming(&fixture.sink, &hash_v2), 0);

    // Granted for v2: a later run of v2 goes straight to a host.
    fixture
        .db
        .create_autorun_grant(
            fixture.community,
            fixture.workflow_id,
            &hash_v2,
            &[0x33; 32],
            &[0x22; 32],
        )
        .await
        .expect("grant v2");
    let run_v2b = fixture
        .db
        .create_workflow_run(fixture.community, fixture.workflow_id, None, None)
        .await
        .expect("run v2b");
    let result = executor::execute_run(
        &fixture.engine,
        fixture.community,
        run_v2b,
        &def_v2,
        &TriggerContext::default(),
    )
    .await
    .expect("segment v2b");
    assert!(matches!(
        result.suspension,
        Some(Suspension::HostStep { .. })
    ));
    assert_eq!(requests_naming(&fixture.sink, &hash_v2), 1);
}

/// The comparison itself, without a database: a run that carries no binding
/// is `definition_unknown`, never "close enough".
#[test]
fn a_run_without_a_binding_is_unknown_not_bound() {
    let workflow = buzz_db::workflow::WorkflowRecord {
        id: Uuid::nil(),
        community_id: buzz_core::tenant::CommunityId::from_uuid(Uuid::nil()),
        name: "verify".into(),
        owner_pubkey: vec![1; 32],
        channel_id: None,
        definition: serde_json::json!({}),
        definition_hash: vec![0xab; 32],
        project_ref: None,
        status: buzz_db::workflow::WorkflowStatus::Active,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let run = |definition_hash: Option<Vec<u8>>| buzz_db::workflow::WorkflowRunRecord {
        id: Uuid::nil(),
        community_id: buzz_core::tenant::CommunityId::from_uuid(Uuid::nil()),
        workflow_id: Uuid::nil(),
        status: RunStatus::WaitingApproval,
        trigger_event_id: None,
        current_step: 0,
        execution_trace: serde_json::json!([]),
        trigger_context: None,
        started_at: None,
        completed_at: None,
        error_message: None,
        error_code: None,
        definition_hash,
        created_at: chrono::Utc::now(),
    };
    assert_eq!(
        crate::run_definition_stop(&run(None), &workflow)
            .expect("unbound")
            .code(),
        crate::RUN_STOPPED_DEFINITION_UNKNOWN
    );
    assert_eq!(
        crate::run_definition_stop(&run(Some(vec![0xcd; 32])), &workflow)
            .expect("changed")
            .code(),
        crate::RUN_STOPPED_DEFINITION_CHANGED
    );
    assert!(crate::run_definition_stop(&run(Some(vec![0xab; 32])), &workflow).is_none());
}
