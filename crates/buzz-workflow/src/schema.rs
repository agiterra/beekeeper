//! YAML/JSON workflow definition types.
//!
//! Workflow definitions are authored in YAML and stored as canonical JSON.
//! All types must round-trip through both formats without loss.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;

/// Default `run_on_host` timeout when the step names none.
pub const RUN_ON_HOST_DEFAULT_TIMEOUT_SECS: u64 = 1800;
/// Ceiling on a `run_on_host` timeout; the host refuses anything above it.
pub const RUN_ON_HOST_MAX_TIMEOUT_SECS: u64 = 3600;
/// Default bytes of stdout/stderr tail a host step captures.
pub const RUN_ON_HOST_DEFAULT_TAIL_BYTES: u64 = 8192;
/// Ceiling on the captured tail.
pub const RUN_ON_HOST_MAX_TAIL_BYTES: u64 = 65_536;
/// Default ceiling on the on-disk artifact a host step keeps.
pub const RUN_ON_HOST_DEFAULT_ARTIFACT_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Top-level workflow definition, authored in YAML and stored as canonical JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    /// Human-readable workflow name (must be non-empty).
    pub name: String,
    /// Optional description shown in the UI.
    #[serde(default)]
    pub description: Option<String>,
    /// The event trigger that starts this workflow.
    pub trigger: TriggerDef,
    /// Full canonical kind:30621 project coordinate this definition acts
    /// for. Required when any step is `run_on_host`: it is the `a` tag on the
    /// kind:46013 request, which is how a host knows the request is for a
    /// project it serves. Literal; templates are refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Ordered list of steps to execute when triggered.
    pub steps: Vec<Step>,
    /// Whether this workflow is active. Defaults to `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Trigger definition. The `on` field is the tag.
///
/// Serde internally-tagged: `on: message_posted`, `on: reaction_added`, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum TriggerDef {
    /// Fires when any message is posted in the workflow's channel.
    MessagePosted {
        /// Optional evalexpr filter (flat var names, e.g. `trigger_text`).
        #[serde(default)]
        filter: Option<String>,
    },
    /// Fires when an emoji reaction is added to a message.
    ReactionAdded {
        /// Optional: only fire for this specific emoji.
        #[serde(default)]
        emoji: Option<String>,
    },
    /// Fires when a diff message (kind:40008) is posted in the workflow's channel.
    DiffPosted {
        /// Optional evalexpr filter expression (same variables as MessagePosted).
        #[serde(default)]
        filter: Option<String>,
    },
    /// Fires on a cron schedule.
    Schedule {
        /// Cron expression, evaluated in `timezone` (UTC when unset).
        /// Mutually exclusive with `interval`.
        #[serde(default)]
        cron: Option<String>,
        /// Simple interval string (e.g. "1h", "30m"). Mutually exclusive with `cron`.
        #[serde(default)]
        interval: Option<String>,
        /// IANA zone the cron expression is written in (`America/New_York`).
        /// The fire instant is converted to UTC before the durable claim, so
        /// a local time a DST change skips fires at the next valid instant.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timezone: Option<String>,
    },
    /// Fires when the relay records a terminal CI result (kind:46008) for
    /// the definition's project whose check name and conclusion match.
    CiResult {
        /// The check name the result must carry (literal).
        check: String,
        /// The conclusions that fire; at least one.
        conclusion: Vec<buzz_core::ci_result::CiConclusion>,
    },
    /// Fires when a push to the relay's git hosting changes a ref that
    /// matches `ref` (a glob: `refs/heads/main`, `refs/heads/*`,
    /// `refs/tags/v*`). Derived from the committed kind:30618 state, never
    /// from the pre-receive policy path, so a denied push cannot fire.
    RefUpdated {
        /// Glob over full ref names.
        #[serde(rename = "ref")]
        ref_glob: String,
        /// Full kind:30617 repository coordinate. When unset, every
        /// repository attached to the definition's `project` matches.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repository: Option<String>,
    },
    /// Fires when HTTP POST arrives at `/hooks/{id}`.
    Webhook,
    /// Never fires on its own: only a kind:46020 trigger (`bee workflows
    /// trigger`, the Run button) starts it. The trigger for an action a
    /// person runs by hand (spec § 5.1).
    Manual,
}

/// A single step in a workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Unique step identifier within this workflow.
    pub id: String,
    /// Optional human-readable step name.
    #[serde(default)]
    pub name: Option<String>,
    /// evalexpr condition. Step is skipped (not failed) if false.
    #[serde(rename = "if", default)]
    pub if_expr: Option<String>,
    /// Maximum seconds this step may run before timing out.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// The action to perform when this step executes.
    #[serde(flatten)]
    pub action: ActionDef,
}

/// Action definition. The `action` field is the tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActionDef {
    /// Post a message to the workflow's channel (or an override channel).
    SendMessage {
        /// Message text (supports template variables).
        text: String,
        /// Optional channel UUID override. Must be a valid UUID string.
        #[serde(default)]
        channel: Option<String>,
    },
    /// Send a direct message to a user.
    SendDm {
        /// Recipient — pubkey hex or `{{trigger.author}}`.
        to: String,
        /// Message text (supports template variables).
        text: String,
    },
    /// Update the channel topic.
    SetChannelTopic {
        /// New topic string.
        topic: String,
    },
    /// Add an emoji reaction to the triggering message.
    AddReaction {
        /// Emoji name (e.g. `"thumbsup"`).
        emoji: String,
    },
    /// HTTP POST to an external URL.
    CallWebhook {
        /// Target URL (must be a public HTTPS endpoint).
        url: String,
        /// HTTP method override (default: `"POST"`).
        #[serde(default)]
        method: Option<String>,
        /// Additional request headers.
        #[serde(default)]
        headers: Option<HashMap<String, String>>,
        /// Request body template.
        #[serde(default)]
        body: Option<String>,
    },
    /// Suspend execution and request approval.
    RequestApproval {
        /// User mention or role (e.g. `"@release-manager"`).
        from: String,
        /// Message shown to the approver.
        message: String,
        /// Duration string (e.g. `"24h"`). Defaults to 24h.
        #[serde(default)]
        timeout: Option<String>,
    },
    /// Pause execution for a duration (e.g. `"5m"`, `"1h"`).
    Delay {
        /// Duration string (e.g. `"5m"`, `"1h"`).
        duration: String,
    },
    /// Record an authenticated CI completion as a durable relay-signed event.
    RecordCiResult {
        /// Full kind:30621 project coordinate. This binding is literal.
        project: String,
        /// Full kind:30617 repository coordinate. This binding is literal.
        repository: String,
        /// Stable check name. This binding is literal.
        check: String,
        /// CI phase covered by this result. This binding is literal.
        phase: buzz_core::ci_result::CiPhase,
        /// Full commit hash (supports template variables).
        commit: String,
        /// Provider run identifier (supports template variables).
        run: String,
        /// Positive provider attempt number (supports template variables).
        attempt: String,
        /// Terminal conclusion: success, failure, or cancelled (supports templates).
        conclusion: String,
        /// Optional provider evidence URL (supports template variables).
        #[serde(default)]
        evidence_url: Option<String>,
        /// Optional human-readable result summary (supports template variables).
        #[serde(default)]
        summary: Option<String>,
    },
    /// Suspend the run and ask an operator's host to deliver a brief — and
    /// the preceding host step's result, when there is one — to a persistent
    /// agent's open execution as a turn. The brief may use `{{trigger.*}}`
    /// and `{{steps.<id>.output.*}}`; the host resolves them from the
    /// request. Like `run_on_host`, the text travels only in the project's
    /// own `actions.yml`.
    WakeAgent {
        /// The agent to wake.
        to: WakeTarget,
        /// What to tell it.
        brief: String,
    },
    /// Suspend the run and ask an operator's host to hire a seat of `role`
    /// into the named agent's umbrella with `brief` as its first turn — the
    /// ephemeral agent of spec § 5.7, and mode (3) when the brief tells it to
    /// run and watch a command itself. Like `wake_agent`, the text travels
    /// only in the project's own `actions.yml`.
    HireAgent {
        /// The role slug to seat.
        role: String,
        /// Whose umbrella the seat joins.
        session: HireSession,
        /// The seat's first turn; templates resolve on the host.
        brief: String,
    },
    /// Suspend the run and ask an operator's host to execute a command in the
    /// project's checkout. Every field is literal: the host never receives
    /// these values over the wire, it recompiles them from the project's own
    /// `actions.yml` (the agents repository's root) and refuses when the definition hash differs.
    RunOnHost {
        /// Program and arguments, exec-style (no shell).
        command: Vec<String>,
        /// Directory relative to the project checkout. Defaults to `"."`;
        /// absolute paths and `..` components are refused.
        #[serde(default)]
        working_directory: Option<String>,
        /// Duration string (`"30m"`, `"1800s"`). Defaults to 30 minutes;
        /// at most one hour.
        #[serde(default)]
        timeout: Option<String>,
        /// Literal environment values. No secrets: this file lives in git.
        #[serde(default)]
        env: Option<BTreeMap<String, String>>,
        /// Names whose values the host supplies from its own environment.
        /// Those values are scrubbed from the captured tails.
        #[serde(default)]
        env_from_host: Option<Vec<String>>,
        /// Output capture limits.
        #[serde(default)]
        capture: Option<HostCapture>,
        /// Where the command runs; defaults to the checkout as it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkout: Option<HostCheckout>,
    },
}

/// Who a `wake_agent` step addresses: an agent named in the project's
/// `team.yml`, which the host resolves to a role and then to that agent's
/// open execution for the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WakeTarget {
    /// The agent's name in `team.yml` `agents`.
    pub agent: String,
}

/// Maximum UTF-8 byte length of a `wake_agent` brief.
pub const WAKE_BRIEF_MAX_BYTES: usize = 4096;

/// Whose umbrella a `hire_agent` step seats the new agent in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HireSession {
    /// The agent (in `team.yml`) whose open execution's umbrella the seat joins.
    pub agent: String,
}

/// Where a `run_on_host` command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostCheckout {
    /// The project's recorded repository folder as it is: whatever is
    /// checked out, dirty or not. The result records `headSha` and `dirty`.
    #[default]
    Current,
    /// A fresh detached worktree at the commit the trigger names —
    /// `ref_updated`'s `after` or `ci_result`'s `commit` — removed after the
    /// run. Refused at save time for triggers that name no commit.
    TriggeringCommit,
}

/// Output capture limits for a `run_on_host` step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCapture {
    /// Bytes of stdout and stderr tail carried in the result.
    #[serde(default)]
    pub tail_bytes: Option<u64>,
    /// Bytes of full output kept on the host's disk.
    #[serde(default)]
    pub artifact_max_bytes: Option<u64>,
    /// Upload the scrubbed logs to the relay's media store and carry their
    /// URLs on the result, so every channel member can read them. Off by
    /// default: only the host's own secrets are scrubbed, and a log can
    /// print others.
    #[serde(default)]
    pub upload: Option<bool>,
}

/// One `run_on_host` step with defaults applied and limits checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRunOnHost {
    /// Program and arguments.
    pub command: Vec<String>,
    /// Directory relative to the checkout, never escaping it.
    pub working_directory: String,
    /// Timeout in seconds, at most [`RUN_ON_HOST_MAX_TIMEOUT_SECS`].
    pub timeout_secs: u64,
    /// Literal environment.
    pub env: BTreeMap<String, String>,
    /// Names the host supplies.
    pub env_from_host: Vec<String>,
    /// Tail bytes to capture.
    pub tail_bytes: u64,
    /// Artifact ceiling.
    pub artifact_max_bytes: u64,
    /// Whether to upload the scrubbed logs after the run.
    pub upload: bool,
    /// Where the command runs.
    pub checkout: HostCheckout,
}

/// Apply defaults to a `run_on_host` step and refuse anything outside the
/// host's limits. The same function runs on the relay when the definition is
/// saved and on the host before it executes, so both refuse identically.
pub fn resolve_run_on_host(action: &ActionDef) -> Result<ResolvedRunOnHost, WorkflowError> {
    let ActionDef::RunOnHost {
        command,
        working_directory,
        timeout,
        env,
        env_from_host,
        capture,
        checkout,
    } = action
    else {
        return Err(WorkflowError::InvalidDefinition(
            "step is not run_on_host".into(),
        ));
    };
    let invalid =
        |detail: String| WorkflowError::InvalidDefinition(format!("run_on_host {detail}"));
    if command.is_empty() || command.iter().any(|arg| arg.is_empty()) {
        return Err(invalid(
            "command must be a non-empty list of non-empty strings".into(),
        ));
    }
    if command.iter().any(|arg| arg.contains("{{")) {
        return Err(invalid(
            "command must be literal; templates are not supported".into(),
        ));
    }
    let working_directory = working_directory
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(".")
        .to_owned();
    if working_directory.contains("{{") {
        return Err(invalid("working_directory must be literal".into()));
    }
    let path = std::path::Path::new(&working_directory);
    if path.is_absolute()
        || working_directory.starts_with('/')
        || working_directory.starts_with('\\')
    {
        return Err(invalid(
            "working_directory must be relative to the checkout".into(),
        ));
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::Prefix(_)
                | std::path::Component::RootDir
        )
    }) {
        return Err(invalid(
            "working_directory must not escape the checkout".into(),
        ));
    }
    let timeout_secs = match timeout.as_deref().map(str::trim) {
        None | Some("") => RUN_ON_HOST_DEFAULT_TIMEOUT_SECS,
        Some(value) => crate::executor::parse_duration_secs(value).map_err(|_| {
            invalid(format!(
                "timeout '{value}' is invalid: expected a duration like '30m', '1h', or '1800s'"
            ))
        })?,
    };
    if timeout_secs == 0 || timeout_secs > RUN_ON_HOST_MAX_TIMEOUT_SECS {
        return Err(invalid(format!(
            "timeout must be between 1s and {RUN_ON_HOST_MAX_TIMEOUT_SECS}s (got {timeout_secs}s)"
        )));
    }
    let env = env.clone().unwrap_or_default();
    for (key, value) in &env {
        if !is_env_name(key) {
            return Err(invalid(format!(
                "env name '{key}' is not a valid environment name"
            )));
        }
        if value.contains("{{") {
            return Err(invalid(format!("env value for '{key}' must be literal")));
        }
    }
    let env_from_host = env_from_host.clone().unwrap_or_default();
    for key in &env_from_host {
        if !is_env_name(key) {
            return Err(invalid(format!(
                "env_from_host name '{key}' is not a valid environment name"
            )));
        }
        if env.contains_key(key) {
            return Err(invalid(format!(
                "'{key}' cannot be both a literal env value and env_from_host"
            )));
        }
    }
    let capture = capture.clone().unwrap_or_default();
    let tail_bytes = capture.tail_bytes.unwrap_or(RUN_ON_HOST_DEFAULT_TAIL_BYTES);
    if tail_bytes == 0 || tail_bytes > RUN_ON_HOST_MAX_TAIL_BYTES {
        return Err(invalid(format!(
            "capture.tail_bytes must be between 1 and {RUN_ON_HOST_MAX_TAIL_BYTES}"
        )));
    }
    let artifact_max_bytes = capture
        .artifact_max_bytes
        .unwrap_or(RUN_ON_HOST_DEFAULT_ARTIFACT_MAX_BYTES);
    if artifact_max_bytes < tail_bytes {
        return Err(invalid(
            "capture.artifact_max_bytes must be at least capture.tail_bytes".into(),
        ));
    }
    Ok(ResolvedRunOnHost {
        command: command.clone(),
        working_directory,
        timeout_secs,
        env,
        env_from_host,
        tail_bytes,
        artifact_max_bytes,
        upload: capture.upload.unwrap_or(false),
        checkout: checkout.unwrap_or_default(),
    })
}

fn is_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphabetic() || first == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl WorkflowDef {
    /// True when any step performs an action that can exfiltrate channel data
    /// to an arbitrary external destination (`call_webhook`).
    ///
    /// Definitions with such steps require elevated (owner/admin) channel
    /// authority both to save and to run — plain membership is not enough,
    /// because a workflow forwards channel content with the *owner's* standing
    /// authority long after the save (SEC-006).
    pub fn requires_elevated_authority(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s.action, ActionDef::CallWebhook { .. }))
    }

    /// True when any step asks an operator's host to execute a command.
    pub fn has_host_steps(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s.action, ActionDef::RunOnHost { .. }))
    }

    /// True when any step is executed by an operator's host at all: a
    /// command (`run_on_host`) or a routed brief (`wake_agent`).
    pub fn has_host_executed_steps(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(
                s.action,
                ActionDef::RunOnHost { .. }
                    | ActionDef::WakeAgent { .. }
                    | ActionDef::HireAgent { .. }
            )
        })
    }

    /// Validate the workflow definition. Returns `Err` with a descriptive message
    /// if any invariant is violated.
    pub fn validate(&self) -> Result<(), WorkflowError> {
        if self.name.trim().is_empty() {
            return Err(WorkflowError::InvalidDefinition(
                "name is required and must not be empty".into(),
            ));
        }

        if self.steps.is_empty() {
            return Err(WorkflowError::InvalidDefinition(
                "at least one step is required".into(),
            ));
        }

        // Validate step IDs are safe for use in evalexpr variable names.
        // Step IDs become variable names like `steps_{id}_output_{field}`,
        // so they must only contain alphanumeric chars and underscores.
        let valid_step_id = |id: &str| -> bool {
            !id.is_empty()
                && id.len() <= 64
                && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };

        if let Some(project) = &self.project {
            let normalized =
                buzz_core::kind::normalize_project_coordinate(project).ok_or_else(|| {
                    WorkflowError::InvalidDefinition(
                        "project must be a full 30621:<64-hex>:<id> coordinate".into(),
                    )
                })?;
            if &normalized != project {
                return Err(WorkflowError::InvalidDefinition(
                    "project must use a lowercase canonical owner key".into(),
                ));
            }
        }
        if self.has_host_executed_steps() && self.project.is_none() {
            return Err(WorkflowError::InvalidDefinition(
                "run_on_host, wake_agent and hire_agent steps require a top-level project \
                 coordinate"
                    .into(),
            ));
        }
        if let TriggerDef::CiResult { check, conclusion } = &self.trigger {
            if self.project.is_none() {
                return Err(WorkflowError::InvalidDefinition(
                    "a ci_result trigger requires a top-level project coordinate".into(),
                ));
            }
            if check.trim().is_empty() || check.len() > 128 || check.contains("{{") {
                return Err(WorkflowError::InvalidDefinition(
                    "ci_result check must be a literal name of 1 to 128 bytes".into(),
                ));
            }
            if conclusion.is_empty() {
                return Err(WorkflowError::InvalidDefinition(
                    "ci_result conclusion must list at least one of success, failure, cancelled"
                        .into(),
                ));
            }
        }
        // The relay hashes a webhook definition after injecting its secret,
        // and a host hashes the plain entry from actions.yml: the two could
        // never agree, so every request would be refused as drift. Refuse
        // the combination here, where the author sees it.
        if self.has_host_steps() && matches!(self.trigger, TriggerDef::Webhook) {
            return Err(WorkflowError::InvalidDefinition(
                "run_on_host steps cannot use a webhook trigger; use manual, schedule, \
                 message_posted, reaction_added or diff_posted"
                    .into(),
            ));
        }

        let mut seen_ids: HashSet<&str> = HashSet::new();
        for step in &self.steps {
            if step.id.trim().is_empty() {
                return Err(WorkflowError::InvalidDefinition(
                    "step id must not be empty".into(),
                ));
            }
            if !valid_step_id(&step.id) {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "step id '{}' is invalid: must contain only alphanumeric characters and underscores",
                    step.id
                )));
            }
            if !seen_ids.insert(step.id.as_str()) {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "duplicate step id: {}",
                    step.id
                )));
            }

            if matches!(step.action, ActionDef::RunOnHost { .. }) {
                let resolved = resolve_run_on_host(&step.action)?;
                if resolved.checkout == HostCheckout::TriggeringCommit
                    && !matches!(
                        self.trigger,
                        TriggerDef::RefUpdated { .. } | TriggerDef::CiResult { .. }
                    )
                {
                    return Err(WorkflowError::InvalidDefinition(format!(
                        "step '{}': checkout: triggering_commit needs a ref_updated or ci_result \
                         trigger, which names the commit",
                        step.id
                    )));
                }
            }
            if let ActionDef::HireAgent {
                role,
                session,
                brief,
            } = &step.action
            {
                if let Err(reason) =
                    buzz_core::coding_session_lifecycle_command::validate_role_slug(role)
                {
                    return Err(WorkflowError::InvalidDefinition(format!(
                        "step '{}': hire_agent role {role:?}: {reason}",
                        step.id
                    )));
                }
                if session.agent.trim().is_empty() || session.agent.len() > 64 {
                    return Err(WorkflowError::InvalidDefinition(format!(
                        "step '{}': hire_agent session.agent must be 1 to 64 bytes",
                        step.id
                    )));
                }
                if brief.trim().is_empty() || brief.len() > WAKE_BRIEF_MAX_BYTES {
                    return Err(WorkflowError::InvalidDefinition(format!(
                        "step '{}': hire_agent brief must be 1 to {WAKE_BRIEF_MAX_BYTES} bytes",
                        step.id
                    )));
                }
            }
            if let ActionDef::WakeAgent { to, brief } = &step.action {
                if to.agent.trim().is_empty() || to.agent.len() > 64 {
                    return Err(WorkflowError::InvalidDefinition(format!(
                        "step '{}': wake_agent to.agent must be 1 to 64 bytes",
                        step.id
                    )));
                }
                if brief.trim().is_empty() || brief.len() > WAKE_BRIEF_MAX_BYTES {
                    return Err(WorkflowError::InvalidDefinition(format!(
                        "step '{}': wake_agent brief must be 1 to {WAKE_BRIEF_MAX_BYTES} bytes",
                        step.id
                    )));
                }
            }

            if let ActionDef::RecordCiResult {
                project,
                repository,
                check,
                ..
            } = &step.action
            {
                if !matches!(self.trigger, TriggerDef::Webhook) {
                    return Err(WorkflowError::InvalidDefinition(
                        "record_ci_result requires a webhook trigger".into(),
                    ));
                }
                if project.trim().is_empty() || repository.trim().is_empty() {
                    return Err(WorkflowError::InvalidDefinition(
                        "record_ci_result requires literal project and repository coordinates"
                            .into(),
                    ));
                }
                if project.contains("{{") || repository.contains("{{") || check.contains("{{") {
                    return Err(WorkflowError::InvalidDefinition(
                        "record_ci_result project, repository, and check must be literal".into(),
                    ));
                }
                if check.trim().is_empty() || check.len() > 128 {
                    return Err(WorkflowError::InvalidDefinition(
                        "record_ci_result check must be 1 to 128 bytes".into(),
                    ));
                }
            }
        }

        if let TriggerDef::RefUpdated {
            ref_glob,
            repository,
        } = &self.trigger
        {
            if ref_glob.trim().is_empty() {
                return Err(WorkflowError::InvalidDefinition(
                    "ref_updated trigger requires a 'ref' glob such as refs/heads/main".into(),
                ));
            }
            ref_glob_matcher(ref_glob)?;
            match repository {
                Some(repository) => {
                    let normalized =
                        buzz_core::project_pack_source::normalize_repository_coordinate(repository)
                            .ok_or_else(|| {
                                WorkflowError::InvalidDefinition(
                            "ref_updated repository must be a full 30617:<64-hex>:<name> coordinate"
                                .into(),
                        )
                            })?;
                    if &normalized != repository {
                        return Err(WorkflowError::InvalidDefinition(
                            "ref_updated repository must use a lowercase canonical owner key"
                                .into(),
                        ));
                    }
                }
                None if self.project.is_none() => {
                    return Err(WorkflowError::InvalidDefinition(
                        "ref_updated without a repository requires a top-level project coordinate"
                            .into(),
                    ));
                }
                None => {}
            }
        }

        if let TriggerDef::Schedule {
            cron,
            interval,
            timezone,
        } = &self.trigger
        {
            if let Some(timezone) = timezone {
                parse_timezone(timezone)?;
            }
            if cron.is_none() && interval.is_none() {
                return Err(WorkflowError::InvalidDefinition(
                    "schedule trigger requires either 'cron' or 'interval'".into(),
                ));
            }

            if cron.is_some() && interval.is_some() {
                return Err(WorkflowError::InvalidDefinition(
                    "schedule trigger cannot specify both 'cron' and 'interval'; use one or the other".into(),
                ));
            }

            if let Some(expr) = cron {
                validate_cron(expr)?;
            }

            if let Some(dur) = interval {
                let secs = crate::executor::parse_duration_secs(dur).map_err(|_| {
                    WorkflowError::InvalidDefinition(format!(
                        "invalid interval '{dur}': expected a duration like '30m', '1h', or '60s'"
                    ))
                })?;
                // Fix 4: the cron loop ticks every 60s, so sub-minute intervals
                // can never fire correctly. Reject them at definition time.
                if secs < 60 {
                    return Err(WorkflowError::InvalidDefinition(
                        "interval must be at least 60s (cron loop ticks every 60 seconds)".into(),
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Compile a `ref_updated` glob. Full ref names only; `*` does not cross a
/// `/`, `**` does, so `refs/heads/*` is the branches and `refs/**` is every
/// ref.
pub fn ref_glob_matcher(glob: &str) -> Result<globset::GlobMatcher, WorkflowError> {
    globset::GlobBuilder::new(glob.trim())
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|error| {
            WorkflowError::InvalidDefinition(format!("invalid ref glob '{glob}': {error}"))
        })
}

/// Parse an IANA timezone name.
pub fn parse_timezone(name: &str) -> Result<chrono_tz::Tz, WorkflowError> {
    name.trim().parse::<chrono_tz::Tz>().map_err(|_| {
        WorkflowError::InvalidDefinition(format!(
            "unknown timezone '{name}': use an IANA name such as America/New_York"
        ))
    })
}

/// Validate a cron expression using the `cron` crate.
///
/// The `cron` crate requires 7 fields: `sec min hour dom month dow year`.
/// Standard 5-field cron (`min hour dom month dow`) is normalized by prepending
/// `0` (seconds) and appending `*` (any year).
fn validate_cron(expr: &str) -> Result<(), WorkflowError> {
    let normalized = normalize_cron(expr);
    normalized.parse::<cron::Schedule>().map_err(|e| {
        WorkflowError::InvalidDefinition(format!("invalid cron expression '{expr}': {e}"))
    })?;
    Ok(())
}

/// Normalize a cron expression to the 7-field format required by the `cron` crate.
///
/// - 5 fields (`min hour dom month dow`) → prepend `0` (sec), append `*` (year)
/// - 6 fields → append `*` (year)
/// - 7 fields → unchanged
pub(crate) fn normalize_cron(expr: &str) -> String {
    let field_count = expr.split_whitespace().count();
    match field_count {
        5 => format!("0 {expr} *"),
        6 => format!("{expr} *"),
        _ => expr.to_owned(),
    }
}

/// Parse a YAML workflow definition, validate it, and return the canonical JSON.
///
/// Returns `(WorkflowDef, canonical_json)` on success.
pub fn parse_yaml(yaml: &str) -> Result<(WorkflowDef, String), WorkflowError> {
    let def: WorkflowDef = serde_yaml::from_str(yaml)?;
    def.validate()?;
    let json =
        serde_json::to_string(&def).map_err(|e| WorkflowError::InvalidDefinition(e.to_string()))?;
    Ok((def, json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_message_posted_workflow() {
        // Use single-quoted YAML strings to avoid raw-string delimiter conflicts.
        let yaml = "name: 'Incident Alert'\ndescription: 'Alert on P1 messages'\ntrigger:\n  on: message_posted\n  filter: 'str_contains(trigger_text, \"P1\")'\nsteps:\n  - id: notify\n    action: send_message\n    text: 'P1 alert'\n";
        let (def, json) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.name, "Incident Alert");
        assert!(def.enabled); // default true
        assert_eq!(def.steps.len(), 1);
        assert_eq!(def.steps[0].id, "notify");

        let reparsed: WorkflowDef = serde_json::from_str(&json).expect("json round-trip");
        assert_eq!(reparsed.name, def.name);
    }

    #[test]
    fn parse_reaction_added_trigger() {
        let yaml = "name: Triage\ntrigger:\n  on: reaction_added\n  emoji: clipboard\nsteps:\n  - id: ack\n    action: add_reaction\n    emoji: eyes\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.trigger {
            TriggerDef::ReactionAdded { emoji } => {
                assert_eq!(emoji.as_deref(), Some("clipboard"));
            }
            other => panic!("unexpected trigger: {other:?}"),
        }
    }

    #[test]
    fn parse_schedule_trigger() {
        let yaml = "name: Daily Standup\ntrigger:\n  on: schedule\n  cron: '0 9 * * 1-5'\nsteps:\n  - id: prompt\n    action: send_message\n    text: Standup time\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.trigger {
            TriggerDef::Schedule { cron, .. } => {
                assert_eq!(cron.as_deref(), Some("0 9 * * 1-5"));
            }
            other => panic!("unexpected trigger: {other:?}"),
        }
    }

    #[test]
    fn parse_workflow_with_conditions() {
        // Use single-quoted YAML strings; evalexpr expressions use double quotes inside.
        let yaml = concat!(
            "name: Conditional Workflow\n",
            "trigger:\n  on: message_posted\n",
            "steps:\n",
            "  - id: escalate\n",
            "    if: 'str_contains(trigger_text, \"P1\") || str_contains(trigger_text, \"SEV1\")'\n",
            "    action: send_message\n",
            "    text: P1 escalation\n",
            "  - id: normal\n",
            "    if: '!str_contains(trigger_text, \"P1\")'\n",
            "    action: send_message\n",
            "    text: Normal message\n",
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.steps.len(), 2);
        assert!(def.steps[0].if_expr.is_some());
        assert!(def.steps[1].if_expr.is_some());
    }

    #[test]
    fn parse_all_action_types() {
        // Avoid "# in YAML values (would close r# raw strings).
        // Use unquoted or single-quoted YAML values throughout.
        let yaml = concat!(
            "name: All Actions\n",
            "trigger:\n  on: webhook\n",
            "steps:\n",
            "  - id: msg\n    action: send_message\n    text: Hello\n    channel: general\n",
            "  - id: dm\n    action: send_dm\n    to: '{{trigger.author}}'\n    text: You triggered this\n",
            "  - id: topic\n    action: set_channel_topic\n    topic: Status active\n",
            "  - id: react\n    action: add_reaction\n    emoji: white_check_mark\n",
            "  - id: hook\n    action: call_webhook\n    url: https://hooks.example.com/notify\n    method: POST\n",
            "  - id: approve\n    action: request_approval\n    from: '@manager'\n    message: Approve?\n    timeout: 4h\n",
            "  - id: wait\n    action: delay\n    duration: 5m\n",
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.steps.len(), 7);

        assert!(matches!(
            &def.steps[0].action,
            ActionDef::SendMessage { .. }
        ));
        assert!(matches!(&def.steps[1].action, ActionDef::SendDm { .. }));
        assert!(matches!(
            &def.steps[2].action,
            ActionDef::SetChannelTopic { .. }
        ));
        assert!(matches!(
            &def.steps[3].action,
            ActionDef::AddReaction { .. }
        ));
        assert!(matches!(
            &def.steps[4].action,
            ActionDef::CallWebhook { .. }
        ));
        assert!(matches!(
            &def.steps[5].action,
            ActionDef::RequestApproval { .. }
        ));
        assert!(matches!(&def.steps[6].action, ActionDef::Delay { .. }));
    }

    #[test]
    fn parse_record_ci_result_preserves_literal_binding_and_dynamic_templates() {
        let yaml = concat!(
            "name: CI completion\n",
            "trigger:\n  on: webhook\n",
            "steps:\n",
            "  - id: record\n",
            "    action: record_ci_result\n",
            "    project: '30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:agiterra'\n",
            "    repository: '30617:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:beekeeper'\n",
            "    check: relay-ci\n",
            "    phase: build\n",
            "    commit: '{{trigger.commit}}'\n",
            "    run: '{{trigger.run}}'\n",
            "    attempt: '{{trigger.attempt}}'\n",
            "    conclusion: '{{trigger.conclusion}}'\n",
            "    evidence_url: '{{trigger.url}}'\n",
            "    summary: '{{trigger.summary}}'\n",
        );
        let (def, canonical) = parse_yaml(yaml).expect("record action should parse");
        assert!(canonical.contains("record_ci_result"));
        assert!(matches!(
            &def.steps[0].action,
            ActionDef::RecordCiResult {
                phase: buzz_core::ci_result::CiPhase::Build,
                commit,
                attempt,
                ..
            } if commit == "{{trigger.commit}}" && attempt == "{{trigger.attempt}}"
        ));
    }

    #[test]
    fn record_ci_result_requires_webhook_and_literal_authority_binding() {
        let non_webhook = concat!(
            "name: CI completion\ntrigger:\n  on: message_posted\nsteps:\n",
            "  - id: record\n    action: record_ci_result\n",
            "    project: p\n    repository: r\n    check: ci\n    phase: build\n",
            "    commit: c\n    run: r\n    attempt: '1'\n    conclusion: success\n",
        );
        assert!(parse_yaml(non_webhook)
            .unwrap_err()
            .to_string()
            .contains("requires a webhook trigger"));

        let templated_binding = non_webhook
            .replace("on: message_posted", "on: webhook")
            .replace("project: p", "project: '{{trigger.project}}'");
        assert!(parse_yaml(&templated_binding)
            .unwrap_err()
            .to_string()
            .contains("must be literal"));
    }

    #[test]
    fn parse_approval_gate_example() {
        let yaml = concat!(
            "name: Deploy Approval\n",
            "trigger:\n  on: webhook\n",
            "steps:\n",
            "  - id: request\n    action: request_approval\n    from: '@engineering-lead'\n",
            "    message: Approve deploy?\n    timeout: 4h\n",
            "  - id: notify_approved\n    if: 'steps_request_output_approved == true'\n",
            "    action: send_message\n    text: Deploy approved\n",
            "  - id: notify_denied\n    if: 'steps_request_output_approved == false'\n",
            "    action: send_message\n    text: Deploy denied\n",
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.steps.len(), 3);
    }

    #[test]
    fn validate_rejects_empty_name() {
        let yaml =
            "name: ''\ntrigger:\n  on: message_posted\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(
            matches!(err, WorkflowError::InvalidDefinition(_)),
            "expected InvalidDefinition, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_empty_steps() {
        let yaml = "name: No Steps\ntrigger:\n  on: message_posted\nsteps: []\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
    }

    #[test]
    fn validate_rejects_duplicate_step_ids() {
        let yaml = concat!(
            "name: Duplicate IDs\n",
            "trigger:\n  on: message_posted\n",
            "steps:\n",
            "  - id: step1\n    action: send_message\n    text: first\n",
            "  - id: step1\n    action: send_message\n    text: second\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        match &err {
            WorkflowError::InvalidDefinition(msg) => {
                assert!(msg.contains("duplicate"), "expected 'duplicate' in: {msg}");
            }
            other => panic!("expected InvalidDefinition, got: {other}"),
        }
    }

    #[test]
    fn validate_rejects_invalid_cron() {
        let yaml = "name: Bad Cron\ntrigger:\n  on: schedule\n  cron: not-a-cron\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
    }

    #[test]
    fn validate_rejects_schedule_without_cron_or_interval() {
        let yaml = "name: Empty Schedule\ntrigger:\n  on: schedule\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
    }

    #[test]
    fn enabled_defaults_to_true() {
        let yaml = "name: Test\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 1m\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert!(def.enabled);
    }

    #[test]
    fn enabled_can_be_set_false() {
        let yaml = "name: Disabled\nenabled: false\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 1m\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert!(!def.enabled);
    }

    #[test]
    fn parse_missing_optional_description_defaults_to_none() {
        let yaml = "name: No Desc\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 1m\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert!(def.description.is_none());
    }

    #[test]
    fn parse_explicit_description_is_present() {
        let yaml = "name: With Desc\ndescription: 'A helpful description'\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 1m\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.description.as_deref(), Some("A helpful description"));
    }

    #[test]
    fn parse_reaction_added_without_emoji_defaults_to_none() {
        // emoji is optional on ReactionAdded — omitting it means match any emoji.
        let yaml = "name: Any Reaction\ntrigger:\n  on: reaction_added\nsteps:\n  - id: s1\n    action: add_reaction\n    emoji: eyes\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.trigger {
            TriggerDef::ReactionAdded { emoji } => {
                assert!(emoji.is_none(), "emoji should default to None");
            }
            other => panic!("unexpected trigger: {other:?}"),
        }
    }

    #[test]
    fn parse_message_posted_without_filter_defaults_to_none() {
        let yaml = "name: All Messages\ntrigger:\n  on: message_posted\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.trigger {
            TriggerDef::MessagePosted { filter } => {
                assert!(filter.is_none(), "filter should default to None");
            }
            other => panic!("unexpected trigger: {other:?}"),
        }
    }

    #[test]
    fn parse_schedule_with_interval_instead_of_cron() {
        let yaml = "name: Interval Schedule\ntrigger:\n  on: schedule\n  interval: 30m\nsteps:\n  - id: s1\n    action: send_message\n    text: tick\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.trigger {
            TriggerDef::Schedule { cron, interval, .. } => {
                assert!(cron.is_none());
                assert_eq!(interval.as_deref(), Some("30m"));
            }
            other => panic!("unexpected trigger: {other:?}"),
        }
    }

    #[test]
    fn parse_step_without_optional_name_defaults_to_none() {
        let yaml = "name: Test\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 5s\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert!(def.steps[0].name.is_none());
    }

    #[test]
    fn parse_step_with_optional_name() {
        let yaml = concat!(
            "name: Test\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: s1\n    name: 'Wait a bit'\n    action: delay\n    duration: 5s\n"
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.steps[0].name.as_deref(), Some("Wait a bit"));
    }

    #[test]
    fn parse_step_without_if_expr_defaults_to_none() {
        let yaml = "name: Test\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 5s\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert!(def.steps[0].if_expr.is_none());
    }

    #[test]
    fn parse_step_without_timeout_defaults_to_none() {
        let yaml = "name: Test\ntrigger:\n  on: webhook\nsteps:\n  - id: s1\n    action: delay\n    duration: 5s\n";
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert!(def.steps[0].timeout_secs.is_none());
    }

    #[test]
    fn parse_step_with_timeout_secs() {
        let yaml = concat!(
            "name: Test\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: s1\n    timeout_secs: 120\n    action: delay\n    duration: 5s\n"
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        assert_eq!(def.steps[0].timeout_secs, Some(120));
    }

    #[test]
    fn parse_call_webhook_with_all_optional_fields() {
        let yaml = concat!(
            "name: Full Webhook\ntrigger:\n  on: webhook\n",
            "steps:\n",
            "  - id: call\n    action: call_webhook\n",
            "    url: https://example.com/hook\n",
            "    method: PUT\n",
            "    headers:\n      Authorization: 'Bearer token123'\n      Content-Type: application/json\n",
            "    body: '{\"key\": \"value\"}'\n",
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.steps[0].action {
            ActionDef::CallWebhook {
                url,
                method,
                headers,
                body,
            } => {
                assert_eq!(url, "https://example.com/hook");
                assert_eq!(method.as_deref(), Some("PUT"));
                let hdrs = headers.as_ref().expect("headers should be present");
                assert_eq!(
                    hdrs.get("Authorization").map(|s| s.as_str()),
                    Some("Bearer token123")
                );
                assert!(body.is_some());
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_call_webhook_minimal_only_url() {
        let yaml = concat!(
            "name: Min Webhook\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: call\n    action: call_webhook\n    url: https://example.com/hook\n",
        );
        let (def, _) = parse_yaml(yaml).expect("parse failed");
        match &def.steps[0].action {
            ActionDef::CallWebhook {
                url,
                method,
                headers,
                body,
            } => {
                assert_eq!(url, "https://example.com/hook");
                assert!(method.is_none());
                assert!(headers.is_none());
                assert!(body.is_none());
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let yaml = "name: [unclosed bracket\ntrigger:\n  on: message_posted\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(
            matches!(err, WorkflowError::InvalidYaml(_)),
            "expected InvalidYaml, got: {err}"
        );
    }

    #[test]
    fn parse_yaml_with_unknown_trigger_type_returns_error() {
        // Unknown trigger `on:` value should fail deserialization.
        let yaml = "name: Bad Trigger\ntrigger:\n  on: unknown_trigger_type\nsteps:\n  - id: s1\n    action: delay\n    duration: 1m\n";
        let err = parse_yaml(yaml).unwrap_err();
        // serde_yaml will return an InvalidYaml error for unknown enum variant.
        assert!(
            matches!(
                err,
                WorkflowError::InvalidYaml(_) | WorkflowError::InvalidDefinition(_)
            ),
            "expected parse error, got: {err}"
        );
    }

    #[test]
    fn parse_yaml_with_unknown_action_type_returns_error() {
        let yaml = concat!(
            "name: Bad Action\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: s1\n    action: fly_to_moon\n    destination: moon\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        assert!(
            matches!(
                err,
                WorkflowError::InvalidYaml(_) | WorkflowError::InvalidDefinition(_)
            ),
            "expected parse error, got: {err}"
        );
    }

    #[test]
    fn canonical_json_round_trips_all_fields() {
        let yaml = concat!(
            "name: 'Full Round Trip'\n",
            "description: 'Tests all fields'\n",
            "enabled: true\n",
            "trigger:\n  on: message_posted\n  filter: 'str_contains(trigger_text, \"alert\")'\n",
            "steps:\n",
            "  - id: notify\n    name: 'Send Alert'\n    timeout_secs: 60\n",
            "    if: 'str_len(trigger_text) > 5'\n",
            "    action: send_message\n    text: 'Alert: {{trigger.text}}'\n",
        );
        let (def, json) = parse_yaml(yaml).expect("parse failed");

        let reparsed: WorkflowDef = serde_json::from_str(&json).expect("json round-trip");

        assert_eq!(reparsed.name, def.name);
        assert_eq!(reparsed.description, def.description);
        assert_eq!(reparsed.enabled, def.enabled);
        assert_eq!(reparsed.steps.len(), def.steps.len());
        assert_eq!(reparsed.steps[0].id, def.steps[0].id);
        assert_eq!(reparsed.steps[0].name, def.steps[0].name);
        assert_eq!(reparsed.steps[0].timeout_secs, def.steps[0].timeout_secs);
        assert_eq!(reparsed.steps[0].if_expr, def.steps[0].if_expr);
    }

    #[test]
    fn validate_rejects_whitespace_only_name() {
        let yaml =
            "name: '   '\ntrigger:\n  on: message_posted\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(
            matches!(err, WorkflowError::InvalidDefinition(_)),
            "expected InvalidDefinition for whitespace-only name, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_empty_step_id() {
        let yaml = concat!(
            "name: Empty Step ID\ntrigger:\n  on: message_posted\n",
            "steps:\n  - id: ''\n    action: send_message\n    text: hi\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
    }

    #[test]
    fn validate_rejects_whitespace_only_step_id() {
        let yaml = concat!(
            "name: Whitespace Step ID\ntrigger:\n  on: message_posted\n",
            "steps:\n  - id: '  '\n    action: send_message\n    text: hi\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
    }

    #[test]
    fn validate_accepts_valid_5_field_cron() {
        // Standard 5-field cron: min hour dom month dow
        let yaml = "name: Cron5\ntrigger:\n  on: schedule\n  cron: '0 9 * * 1-5'\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        assert!(parse_yaml(yaml).is_ok(), "5-field cron should be valid");
    }

    #[test]
    fn validate_accepts_valid_6_field_cron() {
        // 6-field cron: sec min hour dom month dow
        let yaml = "name: Cron6\ntrigger:\n  on: schedule\n  cron: '0 0 9 * * 1-5'\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        assert!(parse_yaml(yaml).is_ok(), "6-field cron should be valid");
    }

    #[test]
    fn validate_accepts_valid_7_field_cron() {
        // 7-field cron: sec min hour dom month dow year
        let yaml = "name: Cron7\ntrigger:\n  on: schedule\n  cron: '0 0 9 * * 1-5 *'\nsteps:\n  - id: s1\n    action: send_message\n    text: hi\n";
        assert!(parse_yaml(yaml).is_ok(), "7-field cron should be valid");
    }

    #[test]
    fn validate_rejects_three_duplicate_step_ids() {
        let yaml = concat!(
            "name: Triple Duplicate\ntrigger:\n  on: message_posted\n",
            "steps:\n",
            "  - id: step1\n    action: send_message\n    text: first\n",
            "  - id: step1\n    action: send_message\n    text: second\n",
            "  - id: step1\n    action: send_message\n    text: third\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        match &err {
            WorkflowError::InvalidDefinition(msg) => {
                assert!(msg.contains("duplicate"), "expected 'duplicate' in: {msg}");
            }
            other => panic!("expected InvalidDefinition, got: {other}"),
        }
    }

    #[test]
    fn validate_accepts_multiple_steps_with_unique_ids() {
        let yaml = concat!(
            "name: Multi Step\ntrigger:\n  on: message_posted\n",
            "steps:\n",
            "  - id: step1\n    action: send_message\n    text: first\n",
            "  - id: step2\n    action: send_message\n    text: second\n",
            "  - id: step3\n    action: send_message\n    text: third\n",
        );
        let (def, _) = parse_yaml(yaml).expect("unique step IDs should be valid");
        assert_eq!(def.steps.len(), 3);
    }

    #[test]
    fn step_id_validation_rejects_dashes() {
        // Step ID with dash would cause evalexpr to interpret as subtraction:
        // `steps_my-step_output_field` → `steps_my` minus `step_output_field`
        let yaml = concat!(
            "name: Dash Step\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: my-step\n    action: send_message\n    text: hi\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        match &err {
            WorkflowError::InvalidDefinition(msg) => {
                assert!(
                    msg.contains("my-step"),
                    "error message should mention the invalid id, got: {msg}"
                );
            }
            other => panic!("expected InvalidDefinition, got: {other}"),
        }
    }

    #[test]
    fn step_id_validation_accepts_underscores() {
        // Underscores are safe in evalexpr variable names.
        let yaml = concat!(
            "name: Underscore Step\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: my_step\n    action: send_message\n    text: hi\n",
        );
        let (def, _) = parse_yaml(yaml).expect("underscore step id should be valid");
        assert_eq!(def.steps[0].id, "my_step");
    }

    #[test]
    fn step_id_validation_rejects_special_chars() {
        // Special characters (semicolons, spaces, etc.) must be rejected.
        let yaml = concat!(
            "name: Special Chars\ntrigger:\n  on: webhook\n",
            "steps:\n  - id: 'step;drop table'\n    action: send_message\n    text: hi\n",
        );
        let err = parse_yaml(yaml).unwrap_err();
        assert!(
            matches!(err, WorkflowError::InvalidDefinition(_)),
            "expected InvalidDefinition for step id with special chars, got: {err}"
        );
    }

    #[test]
    fn normalize_cron_5_fields_prepends_sec_appends_year() {
        let result = normalize_cron("0 9 * * 1-5");
        assert_eq!(result, "0 0 9 * * 1-5 *");
    }

    #[test]
    fn normalize_cron_6_fields_appends_year() {
        let result = normalize_cron("0 0 9 * * 1-5");
        assert_eq!(result, "0 0 9 * * 1-5 *");
    }

    #[test]
    fn normalize_cron_7_fields_unchanged() {
        let result = normalize_cron("0 0 9 * * 1-5 *");
        assert_eq!(result, "0 0 9 * * 1-5 *");
    }

    #[test]
    fn normalize_cron_every_minute_5_fields() {
        let result = normalize_cron("* * * * *");
        assert_eq!(result, "0 * * * * * *");
    }

    #[test]
    fn validate_rejects_sub_minute_interval() {
        let yaml = "name: Too Fast\ntrigger:\n  on: schedule\n  interval: 30s\nsteps:\n  - id: s1\n    action: send_message\n    text: tick\n";
        let err = parse_yaml(yaml).unwrap_err();
        match &err {
            WorkflowError::InvalidDefinition(msg) => {
                assert!(
                    msg.contains("60s") || msg.contains("60 seconds"),
                    "error should mention 60s minimum, got: {msg}"
                );
            }
            other => panic!("expected InvalidDefinition, got: {other}"),
        }
    }

    #[test]
    fn validate_rejects_sub_minute_interval_59s() {
        let yaml = "name: Too Fast\ntrigger:\n  on: schedule\n  interval: 59s\nsteps:\n  - id: s1\n    action: send_message\n    text: tick\n";
        let err = parse_yaml(yaml).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
    }

    #[test]
    fn validate_accepts_exactly_60s_interval() {
        let yaml = "name: Exactly 60s\ntrigger:\n  on: schedule\n  interval: 60s\nsteps:\n  - id: s1\n    action: send_message\n    text: tick\n";
        assert!(parse_yaml(yaml).is_ok(), "60s interval should be valid");
    }

    #[test]
    fn validate_accepts_interval_above_60s() {
        // 30m = 1800s, well above the 60s minimum.
        let yaml = "name: Interval Schedule\ntrigger:\n  on: schedule\n  interval: 30m\nsteps:\n  - id: s1\n    action: send_message\n    text: tick\n";
        assert!(parse_yaml(yaml).is_ok(), "30m interval should be valid");
    }

    #[test]
    fn diff_posted_trigger_roundtrips_yaml() {
        let yaml = "on: diff_posted\n";
        let trigger: TriggerDef = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(trigger, TriggerDef::DiffPosted { filter: None }));
        let back = serde_yaml::to_string(&trigger).unwrap();
        assert!(back.contains("diff_posted"));
    }

    #[test]
    fn diff_posted_trigger_with_filter_roundtrips_yaml() {
        let yaml = "on: diff_posted\nfilter: 'str_contains(trigger_text, \"src/\")'\n";
        let trigger: TriggerDef = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(
            trigger,
            TriggerDef::DiffPosted { filter: Some(_) }
        ));
    }
}
