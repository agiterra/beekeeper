//! Routing a project action's result to a persistent agent (spec § 5.7).
//!
//! A `wake_agent` step is a host step like `run_on_host`: the relay parks the
//! run and publishes a kind:46013 whose `stepKind` is `wake_agent` and whose
//! `inputs` carry the earlier steps' outputs. This host recompiles the step
//! from its own `actions.yml` (the agents repository's root) (so the brief text never rides the
//! wire), resolves the agent's name to a role through `team.yml`,
//! finds that role's open execution for the project **on this computer**,
//! and delivers the brief as an ordinary boundary turn — a kind:44220 this
//! provider signs and publishes, exactly as a team wake is delivered.
//!
//! Exactly once, across a duplicate 46013 and a provider restart: the
//! action-step store refuses a request it has any record of, the operation
//! ledger fences the run's step, and the turn's command id is a pure
//! function of the run and step, so even a second publish converges on the
//! receiving side's command ledger. What is *not* done here: creating the
//! agent's execution when none is open (seat custody is staged only by the
//! desktop), and waking an execution another host serves — both are refused
//! by name rather than guessed at.

use std::collections::HashMap;
use std::path::Path;

use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery,
    CODING_SESSION_COMMAND_SCHEMA,
};
use buzz_core::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
};
use buzz_core::host_step::{
    HostStepDisposition, HostStepRefusal, HostStepRequested, HostStepResult, HostStepRouted,
    HOST_STEP_SCHEMA,
};
use buzz_workflow::executor::{resolve_template, TriggerContext};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::action_step_store::ActionStepRecord;
use crate::gate_observer::ObservedGateRow;
use crate::state::SessionRecord;

/// The checkout has no `team.yml`, so an agent name resolves to
/// nothing.
pub const ROUTE_NO_TEAM: &str = "ROUTE_NO_TEAM";
/// `team.yml` names no agent by that name.
pub const ROUTE_UNKNOWN_AGENT: &str = "ROUTE_UNKNOWN_AGENT";
/// No open execution of that role for this project runs on this computer.
pub const ROUTE_NO_SESSION: &str = "ROUTE_NO_SESSION";
/// The relay refused the turn this host published.
pub const ROUTE_REJECTED: &str = "ROUTE_REJECTED";
/// The brief could not be rendered.
pub const ROUTE_BRIEF_INVALID: &str = "ROUTE_BRIEF_INVALID";
/// The named agent's execution has no umbrella (session and genesis refs)
/// a seat could be hired into.
pub const HIRE_NO_UMBRELLA: &str = "HIRE_NO_UMBRELLA";
/// The relay refused the `session.hire` this host published.
pub const HIRE_REJECTED: &str = "HIRE_REJECTED";
/// The rendered brief exceeds what a `session.hire` may carry.
pub const HIRE_BRIEF_TOO_LONG: &str = "HIRE_BRIEF_TOO_LONG";

/// The `type` every routed brief carries, so the woken agent can recognise it.
pub const ACTION_RESULT_TYPE: &str = "action_result";
/// Resolve an agent name to its role through the agents repository's
/// `team.yml` (spec § 4.11), as this host read it from the fetched tip.
pub fn resolve_wake_role(team_yml: Option<&str>, agent: &str) -> Result<String, HostStepRefusal> {
    let Some(text) = team_yml else {
        return Err(refusal(
            ROUTE_NO_TEAM,
            format!(
                "the project's agents repository has no {} to resolve agent {agent:?} with",
                buzz_persona::team::TEAM_YML
            ),
        ));
    };
    let manifest =
        buzz_persona::team::parse_team_yml(text, Path::new(buzz_persona::team::TEAM_YML))
            .map_err(|error| refusal(ROUTE_NO_TEAM, error.to_string()))?;
    let wanted = agent.trim();
    manifest
        .agents
        .iter()
        .find(|entry| entry.name.trim() == wanted)
        .map(|entry| entry.role.clone())
        .ok_or_else(|| {
            refusal(
                ROUTE_UNKNOWN_AGENT,
                format!(
                    "the agents repository's {} names no agent {agent:?}",
                    buzz_persona::team::TEAM_YML
                ),
            )
        })
}

/// The open execution a routed brief goes to: the newest generation of a
/// seat whose role and project match, on this computer.
pub fn pick_open_execution<'a>(
    sessions: impl Iterator<Item = &'a SessionRecord>,
    project: &str,
    role: &str,
) -> Option<&'a SessionRecord> {
    sessions
        .filter(|record| {
            !record.closed
                && record.actor.is_some()
                && record.project_ref.as_deref() == Some(project)
                && record.role.as_deref() == Some(role)
        })
        .max_by(|a, b| {
            a.generation
                .cmp(&b.generation)
                .then_with(|| a.session_id.cmp(&b.session_id))
        })
}

/// The kind:44220 command id for one run's routing step: a pure function of
/// the run and step, so a second delivery of the same request converges.
pub fn route_command_id(run_id: &str, step_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"buzz-provider-action-route/v1\0");
    digest.update(run_id.as_bytes());
    digest.update(b"\0");
    digest.update(step_id.as_bytes());
    format!("action-route-{}", hex::encode(digest.finalize()))
}

/// The operation pointer the routed turn is fenced under.
pub fn route_operation_id(run_id: &str, step_id: &str) -> String {
    format!("action-route:{run_id}:{step_id}")
}

/// The kind:44221 command id for one run's hire step: a pure function of
/// the run and step, so a second delivery converges on the desktop's answer.
pub fn hire_command_id(run_id: &str, step_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"buzz-provider-action-hire/v1\0");
    digest.update(run_id.as_bytes());
    digest.update(b"\0");
    digest.update(step_id.as_bytes());
    format!("action-hire-{}", hex::encode(digest.finalize()))
}

/// Build the signed kind:44221 `session.hire` that seats `role` in the
/// umbrella (`session_ref`, `genesis_ref`) with `brief` as its first turn.
/// The relay admits it from the umbrella's founder, a granted operator, or
/// an active lead hiring a non-lead role; anything else is refused there.
pub fn build_hire_event(
    keys: &nostr::Keys,
    channel_id: Uuid,
    command_id: String,
    session_ref: String,
    genesis_ref: String,
    role: String,
    brief: String,
) -> Result<nostr::Event, String> {
    use buzz_core::coding_session_lifecycle_command::{
        CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
        CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA, MAX_LIFECYCLE_HIRE_BRIEF_BYTES,
    };
    if brief.len() > MAX_LIFECYCLE_HIRE_BRIEF_BYTES {
        return Err(format!(
            "the rendered brief is {} bytes; a session.hire carries at most {MAX_LIFECYCLE_HIRE_BRIEF_BYTES}",
            brief.len()
        ));
    }
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id,
        action: CodingSessionLifecycleAction::SessionHire {
            session_ref,
            genesis_ref,
            role,
            provider_instance_ref: None,
            model: None,
            brief,
            // The signer is the requester: this host's own key.
            requested_by: None,
            // No routing request: the seat runs where the desktop's policy puts it.
            routing: None,
        },
    };
    buzz_sdk::builders::build_coding_session_lifecycle_command(channel_id, &payload)
        .map_err(|error| error.to_string())?
        .sign_with_keys(keys)
        .map_err(|error| error.to_string())
}

/// The kind:46023 for a hire that reached the relay.
pub fn hired_result(
    record: &ActionStepRecord,
    claim_event_id: &str,
    routed: HostStepRouted,
) -> HostStepResult {
    let line = format!(
        "hired a {} into {}'s umbrella as {}",
        routed.hired_role.as_deref().unwrap_or("seat"),
        routed.agent,
        routed.command_id
    );
    let mut result = routed_result(record, claim_event_id, routed);
    result.stdout_tail = line;
    result
}

/// Render the brief: `{{trigger.*}}` from the request's trigger context and
/// `{{steps.<id>.output.*}}` from its inputs, then wrapped in the
/// `action_result` envelope the spec names, pretty-printed so the agent can
/// read it and a reviewer can diff it.
pub fn wake_brief_text(
    request: &HostStepRequested,
    requested_event_id: &str,
    brief: &str,
) -> Result<String, String> {
    let trigger: TriggerContext =
        serde_json::from_value(request.trigger_context.clone()).unwrap_or_default();
    let outputs: HashMap<String, serde_json::Value> = request
        .inputs
        .as_object()
        .map(|object| object.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let rendered =
        resolve_template(brief, &trigger, &outputs).map_err(|error| error.to_string())?;
    let envelope = serde_json::json!({
        "type": ACTION_RESULT_TYPE,
        "operationId": route_operation_id(&request.run_id, &request.step_id),
        "run": {
            "workflowId": request.workflow_id,
            "workflowName": request.workflow_name,
            "runId": request.run_id,
            "stepId": request.step_id,
            "requestedEventId": requested_event_id,
        },
        "trigger": request.trigger_context,
        "steps": request.inputs,
        "brief": rendered,
    });
    serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())
}

/// The gate row a routed result leaves in the woken agent's session: the
/// preceding host step's outcome, as its output recorded it, or `not_run`
/// when nothing ran before the wake.
pub fn gate_row_for(request: &HostStepRequested) -> CodingSessionObservationGateRow {
    let mut exit_code: Option<i64> = None;
    let mut head_sha: Option<String> = None;
    if let Some(steps) = request.inputs.as_object() {
        for output in steps.values() {
            if let Some(code) = output.get("exit_code").and_then(serde_json::Value::as_i64) {
                exit_code = Some(code);
                head_sha = output
                    .get("head_sha")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
            }
        }
    }
    let outcome = match exit_code {
        Some(0) => CodingSessionObservationGateOutcome::Passed,
        Some(_) => CodingSessionObservationGateOutcome::Failed,
        None => CodingSessionObservationGateOutcome::NotRun,
    };
    CodingSessionObservationGateRow {
        gate: format!("action:{}", request.workflow_name),
        outcome,
        command: format!("{}/{}", request.workflow_name, request.step_id),
        summary: exit_code.map(|code| format!("host step exited {code}")),
        duration_ms: None,
        head_sha,
        dirty: None,
    }
}

/// The kind:46023 for a brief that reached the relay as a turn.
pub fn routed_result(
    record: &ActionStepRecord,
    claim_event_id: &str,
    routed: HostStepRouted,
) -> HostStepResult {
    let line = format!(
        "delivered the brief to {} ({}) as turn {}",
        routed.agent, routed.role, routed.command_id
    );
    HostStepResult {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        requested_event_id: record.requested_event_id.clone(),
        claim_event_id: Some(claim_event_id.to_owned()),
        channel_id: record.channel_id.clone(),
        disposition: HostStepDisposition::Exited,
        exit_code: Some(0),
        refusal: None,
        timed_out: false,
        duration_ms: None,
        head_sha: None,
        dirty: None,
        stdout_tail: line,
        stderr_tail: String::new(),
        truncated: false,
        artifact_path: None,
        routed: Some(routed),
        artifacts: Vec::new(),
    }
}

/// The kind:46023 for a brief this host claimed but could not deliver.
pub fn route_refused_result(
    record: &ActionStepRecord,
    claim_event_id: &str,
    refusal: &HostStepRefusal,
) -> HostStepResult {
    HostStepResult {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        requested_event_id: record.requested_event_id.clone(),
        claim_event_id: Some(claim_event_id.to_owned()),
        channel_id: record.channel_id.clone(),
        disposition: HostStepDisposition::Refused,
        exit_code: None,
        refusal: Some(refusal.clone()),
        timed_out: false,
        duration_ms: None,
        head_sha: None,
        dirty: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        truncated: false,
        artifact_path: None,
        routed: None,
        artifacts: Vec::new(),
    }
}

/// Build the signed kind:44220 that delivers `text` to `target` as a
/// boundary turn.
pub fn build_route_event(
    keys: &nostr::Keys,
    channel_id: Uuid,
    target: buzz_core::coding_session_command::CodingSessionTarget,
    command_id: String,
    text: String,
) -> Result<nostr::Event, String> {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id,
        target,
        action: CodingSessionAction::ThreadTurnStart {
            text,
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        },
    };
    buzz_sdk::builders::build_coding_session_command(channel_id, &payload)
        .map_err(|error| error.to_string())?
        .sign_with_keys(keys)
        .map_err(|error| error.to_string())
}

/// The observed gate row wrapper the provider's publisher takes.
pub fn observed_gate_row(row: CodingSessionObservationGateRow) -> ObservedGateRow {
    ObservedGateRow { row }
}

fn refusal(code: &str, message: impl Into<String>) -> HostStepRefusal {
    HostStepRefusal {
        code: code.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::host_step::HOST_STEP_KIND_WAKE_AGENT;

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse";

    fn request() -> HostStepRequested {
        HostStepRequested {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: Uuid::nil().to_string(),
            workflow_id: Uuid::from_u128(7).to_string(),
            workflow_name: "nightly".into(),
            step_id: "fix".into(),
            step_index: 1,
            definition_hash: "a".repeat(64),
            step_kind: HOST_STEP_KIND_WAKE_AGENT.into(),
            channel_id: Uuid::from_u128(9).to_string(),
            project: PROJECT.into(),
            approval: None,
            trigger_context: serde_json::json!({"author": "b".repeat(64), "ref": "refs/heads/main"}),
            inputs: serde_json::json!({"build": {"exit_code": 1, "head_sha": "c".repeat(40), "stderr_tail": "boom"}}),
            expires_at: u64::MAX,
        }
    }

    #[test]
    fn an_agent_name_resolves_to_its_role_through_team_yml() {
        assert_eq!(
            resolve_wake_role(None, "Levain").unwrap_err().code,
            ROUTE_NO_TEAM
        );
        let team = "schema: beekeeper-team/v1\nversion: 0.1.0\nroles:\n  builder: {}\nagents:\n  - name: Levain\n    role: builder\n    lifetime: persistent\n";
        assert_eq!(
            resolve_wake_role(Some(team), "Levain").expect("role"),
            "builder"
        );
        assert_eq!(
            resolve_wake_role(Some(team), "Nobody").unwrap_err().code,
            ROUTE_UNKNOWN_AGENT
        );
        assert_eq!(
            resolve_wake_role(Some("schema: nope\n"), "Levain")
                .unwrap_err()
                .code,
            ROUTE_NO_TEAM
        );
    }

    #[test]
    fn the_brief_renders_templates_and_wraps_the_result() {
        let text = wake_brief_text(
            &request(),
            &"e".repeat(64),
            "Build on {{trigger.ref}} exited {{steps.build.output.exit_code}}: {{steps.build.output.stderr_tail}}",
        )
        .expect("brief");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["type"], ACTION_RESULT_TYPE);
        assert_eq!(
            value["operationId"],
            route_operation_id(&Uuid::nil().to_string(), "fix")
        );
        assert_eq!(value["brief"], "Build on refs/heads/main exited 1: boom");
        assert_eq!(value["steps"]["build"]["exit_code"], 1);
        assert_eq!(value["run"]["workflowName"], "nightly");
    }

    #[test]
    fn the_command_id_is_a_pure_function_of_run_and_step() {
        let a = route_command_id("run-1", "fix");
        assert_eq!(a, route_command_id("run-1", "fix"));
        assert_ne!(a, route_command_id("run-1", "fix2"));
        assert!(a.starts_with("action-route-"));
        assert_eq!(a.len(), "action-route-".len() + 64);
    }

    #[test]
    fn the_gate_row_reports_the_preceding_host_steps_outcome() {
        let row = gate_row_for(&request());
        assert_eq!(row.gate, "action:nightly");
        assert_eq!(row.outcome, CodingSessionObservationGateOutcome::Failed);
        assert_eq!(row.command, "nightly/fix");
        assert_eq!(row.head_sha.as_deref(), Some(&"c".repeat(40)[..]));
        let mut bare = request();
        bare.inputs = serde_json::json!({});
        assert_eq!(
            gate_row_for(&bare).outcome,
            CodingSessionObservationGateOutcome::NotRun
        );
    }

    #[test]
    fn a_hire_event_is_a_session_hire_into_the_umbrella_with_the_brief() {
        let keys = nostr::Keys::generate();
        let event = build_hire_event(
            &keys,
            Uuid::from_u128(9),
            hire_command_id("run-1", "run"),
            Uuid::from_u128(5).to_string(),
            "a".repeat(64),
            "runner".into(),
            "Run `just test-e2e` yourself.".into(),
        )
        .expect("hire event");
        let payload =
            buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                &event.content,
            )
            .expect("decode");
        match payload.action {
            buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire {
                role,
                brief,
                session_ref,
                ..
            } => {
                assert_eq!(role, "runner");
                assert_eq!(brief, "Run `just test-e2e` yourself.");
                assert_eq!(session_ref, Uuid::from_u128(5).to_string());
            }
            other => panic!("expected a hire, got {other:?}"),
        }
        assert!(payload.command_id.starts_with("action-hire-"));
        assert_eq!(
            hire_command_id("run-1", "run"),
            hire_command_id("run-1", "run")
        );
        assert!(build_hire_event(
            &keys,
            Uuid::from_u128(9),
            hire_command_id("run-1", "run"),
            Uuid::from_u128(5).to_string(),
            "a".repeat(64),
            "runner".into(),
            "x".repeat(20_000),
        )
        .is_err());
    }
}
