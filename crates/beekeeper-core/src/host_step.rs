//! Host-executed workflow steps: the request, the claim, the result and the
//! relay's echo of it.
//!
//! A `run_on_host` workflow step does not run on the relay. The relay suspends
//! the run and publishes a kind:46013 request; an operator's session provider
//! claims it with a kind:46022, executes the command it recompiles from the
//! project's own `actions.yml` (the agents repository's root), and publishes a kind:46023 result;
//! the relay records the result, echoes it as kind:46014 and resumes the run.
//!
//! The request carries **no command text**. The host verifies that the
//! `definitionHash` in the request equals the hash of the entry it compiles
//! from its own checkout, so the relay cannot inject a command the repository
//! does not hold (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 5.6).
//!
//! This module validates only the wire representation. Signature checks and
//! "is the signer the witnessed relay" are consumer responsibilities, exactly
//! as for [`crate::ci_result`].

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kind::{
    event_kind_u32, normalize_project_coordinate, KIND_HOST_STEP_CLAIM, KIND_HOST_STEP_RESULT,
    KIND_WORKFLOW_HOST_STEP_EXITED, KIND_WORKFLOW_HOST_STEP_REQUESTED,
};

/// Exact schema named by every host-step payload and its `schema` tag.
pub const HOST_STEP_SCHEMA: &str = "buzz-host-step/v1";
/// A `run_on_host` step: the host runs a command from its own actions.yml.
pub const HOST_STEP_KIND_RUN_ON_HOST: &str = "run_on_host";
/// A `wake_agent` step: the host delivers a brief to an agent's execution.
pub const HOST_STEP_KIND_WAKE_AGENT: &str = "wake_agent";
/// A `hire_agent` step: the host hires a seat into an agent's umbrella.
pub const HOST_STEP_KIND_HIRE_AGENT: &str = "hire_agent";
/// Maximum UTF-8 byte length of one captured output tail.
pub const MAX_HOST_STEP_TAIL_BYTES: usize = 65_536;
/// Maximum UTF-8 byte length of a workflow step id (the engine's own rule).
pub const MAX_HOST_STEP_ID_BYTES: usize = 64;
/// Maximum UTF-8 byte length of a workflow name.
pub const MAX_HOST_STEP_WORKFLOW_NAME_BYTES: usize = 255;
/// Maximum UTF-8 byte length of a host's self-chosen display name.
pub const MAX_HOST_NAME_BYTES: usize = 128;
/// Maximum UTF-8 byte length of a refusal message or artifact path.
pub const MAX_HOST_STEP_TEXT_BYTES: usize = 4096;

/// Approval that released a host step: the granting event and its scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepApproval {
    /// Lowercase hex of the stored approval token hash — the `approval_ref`
    /// the relay's approvals listing and the kind:46030 grant's `d` tag name.
    pub approval_ref: String,
    /// `run` (this run only) or `action` (every run of this definition hash).
    pub scope: String,
}

/// Content of a relay-signed kind:46013 request.
///
/// Tags: `d` = `<runId>:<stepId>`, `h` = channel, `a` = project coordinate,
/// `schema` = [`HOST_STEP_SCHEMA`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepRequested {
    /// Must equal [`HOST_STEP_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the workflow run.
    pub run_id: String,
    /// Canonical lowercase UUID of the workflow definition.
    pub workflow_id: String,
    /// The definition's name, which is how the host finds the entry in its
    /// own `actions.yml` (the agents repository's root).
    pub workflow_name: String,
    /// The suspended step's id.
    pub step_id: String,
    /// The suspended step's zero-based index.
    pub step_index: u32,
    /// Lowercase hex SHA-256 of the canonical definition JSON stored on the
    /// relay. The host refuses on mismatch (`ACTION_DEFINITION_DRIFT`).
    pub definition_hash: String,
    /// Which host action the step is; [`HOST_STEP_KIND_RUN_ON_HOST`] in C1.
    pub step_kind: String,
    /// Canonical lowercase UUID of the workflow's channel.
    pub channel_id: String,
    /// Full canonical kind:30621 project coordinate the step runs for.
    pub project: String,
    /// The approval that released this step, when one was required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<HostStepApproval>,
    /// The run's trigger context, verbatim, so the host can brief an agent.
    pub trigger_context: serde_json::Value,
    /// The outputs of the run's earlier steps, keyed by step id — what a
    /// `wake_agent` brief's `{{steps.<id>.output.*}}` resolves against, and
    /// how the preceding `run_on_host` result reaches the woken agent. An
    /// object; empty for a step with nothing before it.
    #[serde(default = "empty_object")]
    pub inputs: serde_json::Value,
    /// Unix seconds after which the relay stops accepting a claim.
    pub expires_at: u64,
}

fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// The host's self-description on a claim.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostIdentity {
    /// A display name the operator chose, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Content of a host-signed kind:46022 claim.
///
/// Tags: `d` = `<runId>:<stepId>`, `e` = the requested event id, `h` =
/// channel, `schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepClaim {
    /// Must equal [`HOST_STEP_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the workflow run.
    pub run_id: String,
    /// The claimed step's id.
    pub step_id: String,
    /// Event id (64 lowercase hex) of the kind:46013 being claimed.
    pub requested_event_id: String,
    /// Canonical lowercase UUID of the workflow's channel.
    pub channel_id: String,
    /// Who is claiming.
    #[serde(default)]
    pub host: HostIdentity,
}

/// How a claimed host step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStepDisposition {
    /// The command ran to completion; `exitCode` is its status.
    Exited,
    /// The command exceeded its timeout and was killed; `exitCode` is 124.
    TimedOut,
    /// The host restarted while the command ran and cannot say how it ended.
    LostOnRestart,
    /// The host declined to run the step; `refusal` says why.
    Refused,
}

/// Why a host refused a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepRefusal {
    /// Stable code, e.g. `ACTION_DEFINITION_DRIFT`, `ACTION_CHECKOUT_NOT_RECORDED`.
    pub code: String,
    /// One-line human-readable reason.
    pub message: String,
}

/// Where a `wake_agent` step delivered its brief.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepRouted {
    /// The agent name the step addressed, as `team.yml` spells it.
    pub agent: String,
    /// The role `team.yml` gives that agent.
    pub role: String,
    /// The provider session the turn was addressed to.
    pub session_id: String,
    /// The umbrella session reference that execution belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    /// The kind:44220 command id the host minted for the turn — or, for a
    /// `hire_agent` step, the kind:44221 `session.hire` command id.
    pub command_id: String,
    /// For a `hire_agent` step: the role the hire asked for. Absent on a
    /// `wake_agent` result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hired_role: Option<String>,
}

/// One uploaded log of a `run_on_host` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepArtifact {
    /// `stdout.log` or `stderr.log`.
    pub name: String,
    /// Where the relay serves it.
    pub url: String,
    /// Lowercase hex SHA-256 of the uploaded (scrubbed) bytes.
    pub sha256: String,
    /// Size of the uploaded bytes.
    pub bytes: u64,
}

/// How a host step's working tree was established, and what it held before
/// the command ran.
///
/// Present on every `run_on_host` result, including a refusal made after the
/// tree was established, so a reader never has to infer the commit from a
/// post-execution sample alone (ledger 178(g)). `mode` is the disclosure a
/// person reads: [`HOST_STEP_CHECKOUT_AS_FOUND`] when the run named no commit
/// and the command ran in the recorded project directory as it was found, or
/// [`host_step_checkout_commit`] when the host cut a detached worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepCheckout {
    /// One line naming how the tree was established. Never a guess.
    pub mode: String,
    /// The commit the host checked out, lowercase 40-hex. Absent exactly when
    /// the run bound no commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// `git rev-parse HEAD` of that tree *before* the command ran; `None`
    /// when git could not answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha_before: Option<String>,
    /// Whether that tree had uncommitted changes before the command ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_before: Option<bool>,
}

/// `mode` of a host step that ran in the recorded project directory as found.
pub const HOST_STEP_CHECKOUT_AS_FOUND: &str = "working directory as found";

/// `mode` of a host step the host cut a detached worktree for.
pub fn host_step_checkout_commit(sha: &str) -> String {
    format!("commit {sha}")
}

/// Content of a host-signed kind:46023 result.
///
/// Tags: `d` = `<runId>:<stepId>`, `e` = the requested event id, `h` =
/// channel, `schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepResult {
    /// Must equal [`HOST_STEP_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the workflow run.
    pub run_id: String,
    /// The step's id.
    pub step_id: String,
    /// Event id (64 lowercase hex) of the kind:46013 this answers.
    pub requested_event_id: String,
    /// Event id (64 lowercase hex) of this host's kind:46022 claim, when one
    /// was accepted. A `refused` result may carry none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_event_id: Option<String>,
    /// Canonical lowercase UUID of the workflow's channel.
    pub channel_id: String,
    /// How the step ended.
    pub disposition: HostStepDisposition,
    /// Process exit status; `None` unless the disposition is `exited` or
    /// `timed_out`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Present exactly when the disposition is `refused`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<HostStepRefusal>,
    /// Whether the timeout fired.
    pub timed_out: bool,
    /// Wall-clock duration of the command, when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// `git rev-parse HEAD` of the checkout the command ran in, sampled
    /// *after* it ran. The pre-execution sample and how the tree was
    /// established are in [`HostStepResult::checkout`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    /// The agents-repository commit whose `actions.yml` this host resolved
    /// the step against, from a fetch made for this request (ledger 250).
    /// Absent when the host never read the file (e.g. no clone recorded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_commit: Option<String>,
    /// Whether that checkout had uncommitted changes after the command ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    /// How the working tree was established, and what it held before the
    /// command ran. Absent on a result for a step that never established one
    /// (a routed brief, or a refusal made before the tree was touched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout: Option<HostStepCheckout>,
    /// Last bytes of standard output, secrets scrubbed.
    #[serde(default)]
    pub stdout_tail: String,
    /// Last bytes of standard error, secrets scrubbed.
    #[serde(default)]
    pub stderr_tail: String,
    /// Whether either tail was cut.
    #[serde(default)]
    pub truncated: bool,
    /// Host-local path of the full logs. Meaningful only on that host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    /// For a `wake_agent` step: where the brief was delivered. Absent on a
    /// `run_on_host` result and on a refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed: Option<HostStepRouted>,
    /// The scrubbed logs the host uploaded, when the step asked for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<HostStepArtifact>,
}

/// Content of the relay-signed kind:46014 echo.
///
/// Tags: `d` = `<runId>:<stepId>`, `e` = the result event id, `h` = channel,
/// `p` = the claiming host, `schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStepExited {
    /// Must equal [`HOST_STEP_SCHEMA`].
    pub schema: String,
    /// The accepted result, verbatim.
    pub result: HostStepResult,
    /// Lowercase hex pubkey of the host whose claim the relay recorded — or,
    /// for a refusal made before any claim (`result.claimEventId` absent),
    /// of the host that refused. The relay echoes refusals too (ledger 250),
    /// so the seat that triggered the run learns it was refused.
    pub claimed_by: String,
    /// Event id of the accepted kind:46023.
    pub result_event_id: String,
}

/// The `d` tag every event about one host step carries.
pub fn host_step_d_tag(run_id: &str, step_id: &str) -> String {
    format!("{run_id}:{step_id}")
}

/// Split a `d` tag back into `(run_id, step_id)`, validating both halves.
pub fn parse_host_step_d_tag(value: &str) -> Result<(Uuid, String), String> {
    let (run, step) = value
        .split_once(':')
        .ok_or_else(|| "host step d tag must be <runId>:<stepId>".to_string())?;
    let run_id = canonical_uuid("run id", run)?;
    validate_step_id(step)?;
    Ok((run_id, step.to_owned()))
}

/// Build the exact tags and canonical JSON content of a kind:46013 request.
pub fn build_host_step_requested(
    request: &HostStepRequested,
) -> Result<(Vec<Vec<String>>, String), String> {
    validate_requested(request)?;
    let tags = vec![
        vec![
            "d".into(),
            host_step_d_tag(&request.run_id, &request.step_id),
        ],
        vec!["h".into(), request.channel_id.clone()],
        vec!["a".into(), request.project.clone()],
        vec!["schema".into(), HOST_STEP_SCHEMA.into()],
    ];
    let content = serde_json::to_string(request)
        .map_err(|error| format!("host step request could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46013 request, including exact tag agreement.
pub fn decode_host_step_requested(event: &nostr::Event) -> Result<HostStepRequested, String> {
    if event_kind_u32(event) != KIND_WORKFLOW_HOST_STEP_REQUESTED {
        return Err(format!(
            "host step request must be kind {KIND_WORKFLOW_HOST_STEP_REQUESTED}"
        ));
    }
    let request: HostStepRequested = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed host step request content: {error}"))?;
    let (expected, _) = build_host_step_requested(&request)?;
    require_exact_tags(event, &expected, "host step request")?;
    Ok(request)
}

/// Build the exact tags and canonical JSON content of a kind:46022 claim.
pub fn build_host_step_claim(claim: &HostStepClaim) -> Result<(Vec<Vec<String>>, String), String> {
    validate_claim(claim)?;
    let tags = vec![
        vec!["d".into(), host_step_d_tag(&claim.run_id, &claim.step_id)],
        vec!["e".into(), claim.requested_event_id.clone()],
        vec!["h".into(), claim.channel_id.clone()],
        vec!["schema".into(), HOST_STEP_SCHEMA.into()],
    ];
    let content = serde_json::to_string(claim)
        .map_err(|error| format!("host step claim could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46022 claim, including exact tag agreement.
pub fn decode_host_step_claim(event: &nostr::Event) -> Result<HostStepClaim, String> {
    if event_kind_u32(event) != KIND_HOST_STEP_CLAIM {
        return Err(format!(
            "host step claim must be kind {KIND_HOST_STEP_CLAIM}"
        ));
    }
    let claim: HostStepClaim = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed host step claim content: {error}"))?;
    let (expected, _) = build_host_step_claim(&claim)?;
    require_exact_tags(event, &expected, "host step claim")?;
    Ok(claim)
}

/// Build the exact tags and canonical JSON content of a kind:46023 result.
pub fn build_host_step_result(
    result: &HostStepResult,
) -> Result<(Vec<Vec<String>>, String), String> {
    validate_result(result)?;
    let tags = vec![
        vec!["d".into(), host_step_d_tag(&result.run_id, &result.step_id)],
        vec!["e".into(), result.requested_event_id.clone()],
        vec!["h".into(), result.channel_id.clone()],
        vec!["schema".into(), HOST_STEP_SCHEMA.into()],
    ];
    let content = serde_json::to_string(result)
        .map_err(|error| format!("host step result could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46023 result, including exact tag agreement.
pub fn decode_host_step_result(event: &nostr::Event) -> Result<HostStepResult, String> {
    if event_kind_u32(event) != KIND_HOST_STEP_RESULT {
        return Err(format!(
            "host step result must be kind {KIND_HOST_STEP_RESULT}"
        ));
    }
    let result: HostStepResult = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed host step result content: {error}"))?;
    let (expected, _) = build_host_step_result(&result)?;
    require_exact_tags(event, &expected, "host step result")?;
    Ok(result)
}

/// Build the exact tags and canonical JSON content of a kind:46014 echo.
pub fn build_host_step_exited(
    exited: &HostStepExited,
) -> Result<(Vec<Vec<String>>, String), String> {
    validate_exited(exited)?;
    let tags = vec![
        vec![
            "d".into(),
            host_step_d_tag(&exited.result.run_id, &exited.result.step_id),
        ],
        vec!["e".into(), exited.result_event_id.clone()],
        vec!["h".into(), exited.result.channel_id.clone()],
        vec!["p".into(), exited.claimed_by.clone()],
        vec!["schema".into(), HOST_STEP_SCHEMA.into()],
    ];
    let content = serde_json::to_string(exited)
        .map_err(|error| format!("host step echo could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46014 echo, including exact tag agreement.
pub fn decode_host_step_exited(event: &nostr::Event) -> Result<HostStepExited, String> {
    if event_kind_u32(event) != KIND_WORKFLOW_HOST_STEP_EXITED {
        return Err(format!(
            "host step echo must be kind {KIND_WORKFLOW_HOST_STEP_EXITED}"
        ));
    }
    let exited: HostStepExited = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed host step echo content: {error}"))?;
    let (expected, _) = build_host_step_exited(&exited)?;
    require_exact_tags(event, &expected, "host step echo")?;
    Ok(exited)
}

/// A workflow step id: 1–64 ASCII alphanumerics or underscores, the engine's
/// own rule (`beekeeper_workflow::schema::WorkflowDef::validate`).
pub fn validate_step_id(step_id: &str) -> Result<(), String> {
    if step_id.is_empty() || step_id.len() > MAX_HOST_STEP_ID_BYTES {
        return Err(format!(
            "host step id must be 1 to {MAX_HOST_STEP_ID_BYTES} bytes"
        ));
    }
    if !step_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("host step id must contain only alphanumerics and underscores".into());
    }
    Ok(())
}

fn validate_requested(request: &HostStepRequested) -> Result<(), String> {
    require_schema(&request.schema)?;
    canonical_uuid("run id", &request.run_id)?;
    canonical_uuid("workflow id", &request.workflow_id)?;
    canonical_uuid("channel id", &request.channel_id)?;
    validate_nonempty_bounded(
        "host step workflow name",
        &request.workflow_name,
        MAX_HOST_STEP_WORKFLOW_NAME_BYTES,
    )?;
    validate_step_id(&request.step_id)?;
    validate_hex64("host step definition hash", &request.definition_hash)?;
    if request.step_kind != HOST_STEP_KIND_RUN_ON_HOST
        && request.step_kind != HOST_STEP_KIND_WAKE_AGENT
        && request.step_kind != HOST_STEP_KIND_HIRE_AGENT
    {
        return Err(format!(
            "host step kind must be {HOST_STEP_KIND_RUN_ON_HOST:?}, {HOST_STEP_KIND_WAKE_AGENT:?} or {HOST_STEP_KIND_HIRE_AGENT:?}"
        ));
    }
    if !request.inputs.is_object() {
        return Err("host step inputs must be a JSON object".into());
    }
    validate_project(&request.project)?;
    if let Some(approval) = &request.approval {
        validate_hex64("host step approval ref", &approval.approval_ref)?;
        if !matches!(approval.scope.as_str(), "run" | "action") {
            return Err("host step approval scope must be run or action".into());
        }
    }
    if !request.trigger_context.is_object() {
        return Err("host step trigger context must be a JSON object".into());
    }
    if request.expires_at == 0 {
        return Err("host step expiresAt must be set".into());
    }
    Ok(())
}

fn validate_claim(claim: &HostStepClaim) -> Result<(), String> {
    require_schema(&claim.schema)?;
    canonical_uuid("run id", &claim.run_id)?;
    validate_step_id(&claim.step_id)?;
    validate_hex64("host step requested event id", &claim.requested_event_id)?;
    canonical_uuid("channel id", &claim.channel_id)?;
    if let Some(name) = &claim.host.name {
        validate_nonempty_bounded("host name", name, MAX_HOST_NAME_BYTES)?;
    }
    Ok(())
}

fn validate_result(result: &HostStepResult) -> Result<(), String> {
    require_schema(&result.schema)?;
    canonical_uuid("run id", &result.run_id)?;
    validate_step_id(&result.step_id)?;
    validate_hex64("host step requested event id", &result.requested_event_id)?;
    if let Some(claim) = &result.claim_event_id {
        validate_hex64("host step claim event id", claim)?;
    }
    canonical_uuid("channel id", &result.channel_id)?;
    match result.disposition {
        HostStepDisposition::Exited => {
            if result.exit_code.is_none() {
                return Err("an exited host step must carry an exit code".into());
            }
            if result.timed_out {
                return Err("an exited host step cannot also have timed out".into());
            }
        }
        HostStepDisposition::TimedOut => {
            if !result.timed_out {
                return Err("a timed-out host step must set timedOut".into());
            }
        }
        HostStepDisposition::LostOnRestart => {
            if result.exit_code.is_some() {
                return Err("a lost host step cannot carry an exit code".into());
            }
        }
        HostStepDisposition::Refused => {
            if result.exit_code.is_some() {
                return Err("a refused host step cannot carry an exit code".into());
            }
        }
    }
    match (&result.refusal, result.disposition) {
        (Some(refusal), HostStepDisposition::Refused) => {
            validate_nonempty_bounded("host step refusal code", &refusal.code, 64)?;
            validate_nonempty_bounded(
                "host step refusal message",
                &refusal.message,
                MAX_HOST_STEP_TEXT_BYTES,
            )?;
        }
        (Some(_), _) => return Err("only a refused host step carries a refusal".into()),
        (None, HostStepDisposition::Refused) => {
            return Err("a refused host step must carry a refusal".into())
        }
        (None, _) => {}
    }
    if result.claim_event_id.is_none() && result.disposition != HostStepDisposition::Refused {
        return Err("only a refused host step may omit the claim event id".into());
    }
    if let Some(head) = &result.head_sha {
        if head.len() != 40
            || !head
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("host step headSha must be lowercase 40-hex".into());
        }
    }
    if let Some(commit) = &result.agents_commit {
        if commit.len() != 40
            || !commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("host step agentsCommit must be lowercase 40-hex".into());
        }
    }
    if let Some(checkout) = &result.checkout {
        validate_nonempty_bounded(
            "host step checkout mode",
            &checkout.mode,
            MAX_HOST_STEP_TEXT_BYTES,
        )?;
        for (label, value) in [
            ("host step checkout sha", &checkout.sha),
            (
                "host step checkout headShaBefore",
                &checkout.head_sha_before,
            ),
        ] {
            if let Some(sha) = value {
                if sha.len() != 40
                    || !sha
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                {
                    return Err(format!("{label} must be lowercase 40-hex"));
                }
            }
        }
    }
    if result.stdout_tail.len() > MAX_HOST_STEP_TAIL_BYTES {
        return Err(format!(
            "host step stdoutTail exceeds {MAX_HOST_STEP_TAIL_BYTES} bytes"
        ));
    }
    if result.stderr_tail.len() > MAX_HOST_STEP_TAIL_BYTES {
        return Err(format!(
            "host step stderrTail exceeds {MAX_HOST_STEP_TAIL_BYTES} bytes"
        ));
    }
    if let Some(path) = &result.artifact_path {
        validate_nonempty_bounded("host step artifact path", path, MAX_HOST_STEP_TEXT_BYTES)?;
    }
    if result.artifacts.len() > 8 {
        return Err("a host step result carries at most 8 artifacts".into());
    }
    for artifact in &result.artifacts {
        validate_nonempty_bounded("host step artifact name", &artifact.name, 64)?;
        validate_nonempty_bounded("host step artifact url", &artifact.url, 2048)?;
        validate_hex64("host step artifact sha256", &artifact.sha256)?;
    }
    if let Some(routed) = &result.routed {
        if result.disposition == HostStepDisposition::Refused {
            return Err("a refused host step cannot also be routed".into());
        }
        validate_nonempty_bounded("host step routed agent", &routed.agent, 64)?;
        validate_nonempty_bounded("host step routed role", &routed.role, 64)?;
        validate_nonempty_bounded("host step routed session id", &routed.session_id, 128)?;
        validate_nonempty_bounded("host step routed command id", &routed.command_id, 128)?;
        if let Some(session_ref) = &routed.session_ref {
            validate_nonempty_bounded("host step routed session ref", session_ref, 256)?;
        }
        if let Some(hired_role) = &routed.hired_role {
            validate_nonempty_bounded("host step routed hired role", hired_role, 64)?;
        }
    }
    Ok(())
}

fn validate_exited(exited: &HostStepExited) -> Result<(), String> {
    require_schema(&exited.schema)?;
    validate_result(&exited.result)?;
    validate_hex64("host step claimedBy", &exited.claimed_by)?;
    validate_hex64("host step result event id", &exited.result_event_id)?;
    Ok(())
}

fn require_schema(schema: &str) -> Result<(), String> {
    if schema != HOST_STEP_SCHEMA {
        return Err(format!("host step schema must be {HOST_STEP_SCHEMA:?}"));
    }
    Ok(())
}

fn validate_project(project: &str) -> Result<(), String> {
    let normalized = normalize_project_coordinate(project).ok_or_else(|| {
        "host step project must be a full 30621:<64-hex>:<id> coordinate".to_string()
    })?;
    if normalized != project {
        return Err("host step project must use a lowercase canonical owner key".into());
    }
    Ok(())
}

fn canonical_uuid(label: &str, value: &str) -> Result<Uuid, String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("host step {label} must be a UUID"))?;
    if parsed.to_string() != value {
        return Err(format!(
            "host step {label} must be a lowercase canonical UUID"
        ));
    }
    Ok(parsed)
}

fn validate_hex64(label: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} must be lowercase 64-hex"));
    }
    Ok(())
}

fn validate_nonempty_bounded(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.len() > max {
        return Err(format!("{label} exceeds {max} bytes"));
    }
    Ok(())
}

fn require_exact_tags(
    event: &nostr::Event,
    expected: &[Vec<String>],
    label: &str,
) -> Result<(), String> {
    if event.tags.len() != expected.len() {
        return Err(format!("{label} requires exactly {} tags", expected.len()));
    }
    for tag in expected {
        let matches = event
            .tags
            .iter()
            .filter(|candidate| candidate.as_slice() == tag.as_slice())
            .count();
        if matches != 1 {
            return Err(format!(
                "{label} tags must exactly match its validated content"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn hex64(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn project() -> String {
        format!("30621:{}:pulse", hex64(0x11))
    }

    fn request() -> HostStepRequested {
        HostStepRequested {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: Uuid::nil().to_string(),
            workflow_id: Uuid::from_u128(7).to_string(),
            workflow_name: "nightly-build".into(),
            step_id: "build".into(),
            step_index: 0,
            definition_hash: hex64(0x22),
            step_kind: HOST_STEP_KIND_RUN_ON_HOST.into(),
            channel_id: Uuid::from_u128(9).to_string(),
            project: project(),
            approval: Some(HostStepApproval {
                approval_ref: hex64(0x33),
                scope: "run".into(),
            }),
            trigger_context: serde_json::json!({"author": hex64(0x44)}),
            inputs: serde_json::json!({"build": {"exit_code": 1}}),
            expires_at: 1_800_000_000,
        }
    }

    fn result() -> HostStepResult {
        HostStepResult {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: Uuid::nil().to_string(),
            step_id: "build".into(),
            requested_event_id: hex64(0x55),
            claim_event_id: Some(hex64(0x66)),
            channel_id: Uuid::from_u128(9).to_string(),
            disposition: HostStepDisposition::Exited,
            exit_code: Some(1),
            refusal: None,
            timed_out: false,
            duration_ms: Some(1234),
            head_sha: Some("a".repeat(40)),
            agents_commit: None,
            dirty: Some(false),
            checkout: Some(HostStepCheckout {
                mode: host_step_checkout_commit(&"a".repeat(40)),
                sha: Some("a".repeat(40)),
                head_sha_before: Some("a".repeat(40)),
                dirty_before: Some(false),
            }),
            stdout_tail: "ok".into(),
            stderr_tail: String::new(),
            truncated: false,
            artifact_path: Some("/tmp/actions/run/build".into()),
            routed: None,
            artifacts: Vec::new(),
        }
    }

    fn signed(kind: u32, tags: Vec<Vec<String>>, content: String) -> nostr::Event {
        let tags: Vec<Tag> = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("tag"))
            .collect();
        EventBuilder::new(Kind::from(kind as u16), content)
            .tags(tags)
            .sign_with_keys(&Keys::generate())
            .expect("sign")
    }

    #[test]
    fn request_round_trips_through_build_and_decode() {
        let request = request();
        let (tags, content) = build_host_step_requested(&request).expect("build");
        assert_eq!(
            tags[0],
            vec!["d".to_string(), format!("{}:build", Uuid::nil())]
        );
        let event = signed(KIND_WORKFLOW_HOST_STEP_REQUESTED, tags, content);
        assert_eq!(decode_host_step_requested(&event).expect("decode"), request);
    }

    #[test]
    fn request_refuses_command_text_and_bad_hash() {
        let mut request = request();
        request.definition_hash = "abc".into();
        assert!(build_host_step_requested(&request)
            .unwrap_err()
            .contains("definition hash"));
        let (tags, mut content) = build_host_step_requested(&self::request()).expect("build");
        content = content.replacen("\"stepKind\"", "\"command\":[\"rm\"],\"stepKind\"", 1);
        let event = signed(KIND_WORKFLOW_HOST_STEP_REQUESTED, tags, content);
        assert!(decode_host_step_requested(&event)
            .unwrap_err()
            .contains("unknown field"));
    }

    #[test]
    fn request_decode_demands_exact_tags() {
        let (mut tags, content) = build_host_step_requested(&request()).expect("build");
        tags.push(vec!["a".into(), project()]);
        let event = signed(KIND_WORKFLOW_HOST_STEP_REQUESTED, tags, content);
        assert!(decode_host_step_requested(&event)
            .unwrap_err()
            .contains("exactly 4 tags"));
    }

    #[test]
    fn claim_round_trips() {
        let claim = HostStepClaim {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: Uuid::nil().to_string(),
            step_id: "build".into(),
            requested_event_id: hex64(0x55),
            channel_id: Uuid::from_u128(9).to_string(),
            host: HostIdentity {
                name: Some("andy-mbp".into()),
            },
        };
        let (tags, content) = build_host_step_claim(&claim).expect("build");
        let event = signed(KIND_HOST_STEP_CLAIM, tags, content);
        assert_eq!(decode_host_step_claim(&event).expect("decode"), claim);
    }

    /// Closed-record vector (ledger 250): a kind:46023 whose content carries
    /// `agentsCommit` decodes under `deny_unknown_fields`, and a malformed
    /// value is refused rather than carried.
    #[test]
    fn result_with_agents_commit_decodes_and_malformed_is_refused() {
        let (tags, content) = build_host_step_result(&result()).expect("build");
        let mut body: serde_json::Value = serde_json::from_str(&content).expect("json");
        body["agentsCommit"] = serde_json::Value::String("4".repeat(40));
        let event = signed(KIND_HOST_STEP_RESULT, tags.clone(), body.to_string());
        let decoded = decode_host_step_result(&event).expect("decode with agentsCommit");
        assert_eq!(decoded.agents_commit, Some("4".repeat(40)));

        body["agentsCommit"] = serde_json::Value::String("A".repeat(40));
        let event = signed(KIND_HOST_STEP_RESULT, tags, body.to_string());
        assert!(decode_host_step_result(&event).is_err());
    }

    #[test]
    fn result_round_trips_and_checks_disposition_consistency() {
        let result = result();
        let (tags, content) = build_host_step_result(&result).expect("build");
        let event = signed(KIND_HOST_STEP_RESULT, tags, content);
        assert_eq!(decode_host_step_result(&event).expect("decode"), result);

        let mut lost = self::result();
        lost.disposition = HostStepDisposition::LostOnRestart;
        assert!(build_host_step_result(&lost)
            .unwrap_err()
            .contains("cannot carry an exit code"));
        lost.exit_code = None;
        assert!(build_host_step_result(&lost).is_ok());

        let mut refused = self::result();
        refused.disposition = HostStepDisposition::Refused;
        refused.exit_code = None;
        refused.claim_event_id = None;
        assert!(build_host_step_result(&refused)
            .unwrap_err()
            .contains("must carry a refusal"));
        refused.refusal = Some(HostStepRefusal {
            code: "ACTION_DEFINITION_DRIFT".into(),
            message: "hash differs".into(),
        });
        assert!(build_host_step_result(&refused).is_ok());

        let mut unclaimed = self::result();
        unclaimed.claim_event_id = None;
        assert!(build_host_step_result(&unclaimed)
            .unwrap_err()
            .contains("omit the claim event id"));
    }

    #[test]
    fn exited_echo_round_trips() {
        let exited = HostStepExited {
            schema: HOST_STEP_SCHEMA.into(),
            result: result(),
            claimed_by: hex64(0x77),
            result_event_id: hex64(0x88),
        };
        let (tags, content) = build_host_step_exited(&exited).expect("build");
        assert!(tags
            .iter()
            .any(|tag| tag[0] == "p" && tag[1] == hex64(0x77)));
        let event = signed(KIND_WORKFLOW_HOST_STEP_EXITED, tags, content);
        assert_eq!(decode_host_step_exited(&event).expect("decode"), exited);
    }

    #[test]
    fn d_tag_parses_back() {
        let (run, step) =
            parse_host_step_d_tag(&host_step_d_tag(&Uuid::nil().to_string(), "build"))
                .expect("parse");
        assert_eq!(run, Uuid::nil());
        assert_eq!(step, "build");
        assert!(parse_host_step_d_tag("nope").is_err());
        assert!(parse_host_step_d_tag(&format!("{}:bad-id", Uuid::nil())).is_err());
    }
}
