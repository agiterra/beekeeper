//! Sequential workflow executor.
//!
//! Responsibilities:
//! - Template variable resolution (`{{trigger.X}}`, `{{steps.ID.output.X}}`)
//! - Condition evaluation (`if:` expressions via `evalexpr`)
//! - Sequential step dispatch
//! - Execution trace updates in DB
//!
//! Action dispatch uses placeholder implementations that log intent.
//! Real event emission is wired in WF-07/08 (relay integration).

use std::collections::HashMap;

use beekeeper_core::tenant::CommunityId;
use evalexpr::HashMapContext;
use nostr::ToBech32;
use serde_json::Value as JsonValue;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::WorkflowError;
use crate::schema::{ActionDef, Step, WorkflowDef};
use crate::suspend::Suspension;
use crate::WorkflowEngine;

/// Data extracted from the triggering event, passed to every step.
///
/// Container-defaulted on the way in, so a context recorded before a field
/// existed — or one a host reads off a kind:46013 — still decodes with the
/// fields it has.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TriggerContext {
    /// Message content (message_posted trigger).
    pub text: String,
    /// Pubkey of the event author (hex string).
    pub author: String,
    /// Channel UUID as string.
    pub channel_id: String,
    /// Unix timestamp of the triggering event (as string for template use).
    pub timestamp: String,
    /// Emoji name (reaction_added trigger).
    pub emoji: String,
    /// Event ID of the triggering message (hex string).
    pub message_id: String,
    /// Arbitrary webhook body fields (webhook trigger).
    pub webhook_fields: HashMap<String, String>,
    /// Full kind:30617 coordinate of the pushed repository (ref_updated).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
    /// The ref the push changed (ref_updated), e.g. `refs/heads/main`.
    #[serde(default, rename = "ref", skip_serializing_if = "String::is_empty")]
    pub ref_name: String,
    /// The ref's commit before the push; empty when the ref was created.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before: String,
    /// The ref's commit after the push; empty when the ref was deleted.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after: String,
    /// Hex pubkey of the pusher (ref_updated).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pusher: String,
    /// Set when a schedule's local time was skipped by a DST change and the
    /// run fired at the next valid instant instead (RFC 3339, in the zone).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scheduled_local: String,
    /// The commit a CI result names (ci_result trigger).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit: String,
    /// The check name a CI result names (ci_result trigger).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub check: String,
    /// `success`, `failure` or `cancelled` (ci_result trigger).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conclusion: String,
    /// The CI result's evidence URL, if it carried one (ci_result trigger).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evidence_url: String,
    /// The commit a manual trigger bound the run to — `bee workflows trigger
    /// --checkout <sha>`, lowercase 40-hex. Empty when the run named none, in
    /// which case a host step runs in the recorded project directory as it is
    /// found and says so (ledger 178(g)).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checkout: String,
}

impl TriggerContext {
    /// Look up a trigger field by name.
    ///
    /// Returns `Some(&str)` for known fields; for webhook triggers, also
    /// checks `webhook_fields`. Returns `None` for unknown names.
    pub fn get_field(&self, name: &str) -> Option<&str> {
        match name {
            "text" => Some(&self.text),
            "author" => Some(&self.author),
            "channel_id" => Some(&self.channel_id),
            "timestamp" => Some(&self.timestamp),
            "emoji" => Some(&self.emoji),
            "message_id" => Some(&self.message_id),
            // The trigger-specific fields answer only when set, so a webhook
            // body field of the same name (`commit`, `conclusion`, …) keeps
            // resolving exactly as it did before these fields existed.
            "repository" if !self.repository.is_empty() => Some(&self.repository),
            "ref" if !self.ref_name.is_empty() => Some(&self.ref_name),
            "before" if !self.before.is_empty() => Some(&self.before),
            "after" if !self.after.is_empty() => Some(&self.after),
            "pusher" if !self.pusher.is_empty() => Some(&self.pusher),
            "scheduled_local" if !self.scheduled_local.is_empty() => Some(&self.scheduled_local),
            "commit" if !self.commit.is_empty() => Some(&self.commit),
            "check" if !self.check.is_empty() => Some(&self.check),
            "conclusion" if !self.conclusion.is_empty() => Some(&self.conclusion),
            "evidence_url" if !self.evidence_url.is_empty() => Some(&self.evidence_url),
            "checkout" if !self.checkout.is_empty() => Some(&self.checkout),
            other => self.webhook_fields.get(other).map(|s| s.as_str()),
        }
    }
}

/// Resolve `{{trigger.X}}` and `{{steps.ID.output.X}}` placeholders in a string.
///
/// Supports filters:
/// - `| truncate(N)` — truncate to N characters
/// - `| npub` — encode a hex pubkey as its full bech32 `npub` (non-pubkey
///   values pass through unchanged); `truncate_pubkey` is a legacy alias
///
/// Unknown `{{keys}}` are left as literal text (no error, no substitution).
pub fn resolve_template(
    template: &str,
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Result<String, WorkflowError> {
    if !template.contains("{{") {
        return Ok(template.to_owned());
    }

    let mut result = String::with_capacity(template.len());
    let mut remaining = template;

    while let Some(start) = remaining.find("{{") {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start + 2..];

        let end = match remaining.find("}}") {
            Some(e) => e,
            None => {
                // Unclosed `{{` — emit literally and stop.
                result.push_str("{{");
                result.push_str(remaining);
                return Ok(result);
            }
        };

        let expr = remaining[..end].trim();
        remaining = &remaining[end + 2..];

        // Split on `|` to extract filters.
        let mut parts = expr.splitn(2, '|');
        let var_path = parts.next().unwrap_or("").trim();
        let filter = parts.next().map(|s| s.trim());

        let raw_value = resolve_variable(var_path, trigger_ctx, step_outputs);

        let value = match (raw_value, filter) {
            (Some(v), Some(f)) => apply_filter(v, f)?,
            (Some(v), None) => v,
            (None, _) => {
                // Unknown variable — emit the original `{{expr}}` literally.
                result.push_str("{{");
                result.push_str(expr);
                result.push_str("}}");
                continue;
            }
        };

        result.push_str(&value);
    }

    result.push_str(remaining);
    Ok(result)
}

/// Resolve a CI action field only after proving every placeholder in the
/// configured template has a value. Validation happens before substitution so
/// braces introduced by a callback value remain ordinary result text rather
/// than being mistaken for a second template pass.
fn resolve_ci_template(
    field: &str,
    template: &str,
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Result<String, WorkflowError> {
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        if remaining[..start].contains("}}") {
            return Err(WorkflowError::TemplateError(format!(
                "record_ci_result {field} contains a malformed template"
            )));
        }
        remaining = &remaining[start + 2..];
        let end = remaining.find("}}").ok_or_else(|| {
            WorkflowError::TemplateError(format!(
                "record_ci_result {field} contains an unclosed template"
            ))
        })?;
        let expr = remaining[..end].trim();
        let var_path = expr.split('|').next().unwrap_or("").trim();
        if var_path.is_empty() || resolve_variable(var_path, trigger_ctx, step_outputs).is_none() {
            return Err(WorkflowError::TemplateError(format!(
                "record_ci_result {field} references missing variable {var_path:?}"
            )));
        }
        remaining = &remaining[end + 2..];
    }
    if remaining.contains("}}") {
        return Err(WorkflowError::TemplateError(format!(
            "record_ci_result {field} contains a malformed template"
        )));
    }
    resolve_template(template, trigger_ctx, step_outputs)
}

/// Resolve a single variable path to its string value.
fn resolve_variable(
    path: &str,
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Option<String> {
    if let Some(field) = path.strip_prefix("trigger.") {
        return trigger_ctx.get_field(field).map(|s| s.to_owned());
    }

    // Pattern: `steps.STEP_ID.output.FIELD`
    if let Some(rest) = path.strip_prefix("steps.") {
        let mut parts = rest.splitn(3, '.');
        let step_id = parts.next()?;
        let middle = parts.next()?; // must be "output"
        let field = parts.next()?;

        if middle != "output" {
            return None;
        }

        let output = step_outputs.get(step_id)?;
        return json_get_str(output, field);
    }

    None
}

/// Navigate a JSON value by a single key and return it as a string.
fn json_get_str(value: &JsonValue, key: &str) -> Option<String> {
    match value {
        JsonValue::Object(map) => {
            let v = map.get(key)?;
            Some(json_to_string(v))
        }
        _ => None,
    }
}

/// Convert a JSON value to a plain string for template substitution.
fn json_to_string(v: &JsonValue) -> String {
    match v {
        JsonValue::String(s) => s.clone(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Null => String::new(),
        other => other.to_string(),
    }
}

/// Apply a filter expression to a resolved value.
fn apply_filter(value: String, filter: &str) -> Result<String, WorkflowError> {
    let filter = filter.trim();

    if let Some(inner) = filter
        .strip_prefix("truncate(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let n: usize = inner.trim().parse().map_err(|_| {
            WorkflowError::TemplateError(format!("truncate() requires a number, got: {inner}"))
        })?;
        let truncated: String = value.chars().take(n).collect();
        return Ok(truncated);
    }

    // `npub` (alias `truncate_pubkey`): full bech32 npub — truncated prefixes are grindable.
    if filter == "npub" || filter == "truncate_pubkey" {
        if let Ok(pk) = nostr::PublicKey::from_hex(&value) {
            return Ok(pk.to_bech32().unwrap_or(value));
        }
        return Ok(value);
    }

    Err(WorkflowError::TemplateError(format!(
        "unknown filter: {filter}"
    )))
}

/// Build an `evalexpr::HashMapContext` from trigger context and step outputs.
///
/// Variable names use underscores (not dots) because `evalexpr` does not
/// support dotted identifiers:
///
/// | YAML reference                    | evalexpr variable         |
/// |-----------------------------------|---------------------------|
/// | `trigger.text`                    | `trigger_text`            |
/// | `trigger.author`                  | `trigger_author`          |
/// | `trigger.channel_id`              | `trigger_channel_id`      |
/// | `trigger.timestamp`               | `trigger_timestamp`       |
/// | `trigger.emoji`                   | `trigger_emoji`           |
/// | `trigger.message_id`              | `trigger_message_id`      |
/// | `steps.STEP_ID.output.FIELD`      | `steps_STEP_ID_output_FIELD` |
///
/// Also registers string helper functions that the `cron` crate's `evalexpr` v11
/// does not include by default:
/// - `str_contains(haystack, needle)` → bool
/// - `str_starts_with(s, prefix)` → bool
/// - `str_ends_with(s, suffix)` → bool
/// - `str_len(s)` → int
pub fn build_eval_context(
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Result<HashMapContext, WorkflowError> {
    use evalexpr::*;

    let mut ctx = HashMapContext::new();

    // evalexpr v11 does not ship str_contains / str_starts_with / str_ends_with.
    // Register them as custom functions so workflow YAML can use them.

    ctx.set_function(
        "str_contains".into(),
        Function::new(|args| {
            let args = args.as_fixed_len_tuple(2)?;
            let haystack = args[0].as_string()?;
            let needle = args[1].as_string()?;
            Ok(Value::Boolean(haystack.contains(needle.as_str())))
        }),
    )
    .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;

    ctx.set_function(
        "str_starts_with".into(),
        Function::new(|args| {
            let args = args.as_fixed_len_tuple(2)?;
            let s = args[0].as_string()?;
            let prefix = args[1].as_string()?;
            Ok(Value::Boolean(s.starts_with(prefix.as_str())))
        }),
    )
    .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;

    ctx.set_function(
        "str_ends_with".into(),
        Function::new(|args| {
            let args = args.as_fixed_len_tuple(2)?;
            let s = args[0].as_string()?;
            let suffix = args[1].as_string()?;
            Ok(Value::Boolean(s.ends_with(suffix.as_str())))
        }),
    )
    .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;

    ctx.set_function(
        "str_len".into(),
        Function::new(|arg| {
            let s = arg.as_string()?;
            Ok(Value::Int(s.len() as i64))
        }),
    )
    .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;

    // Register webhook fields first as `trigger_FIELD` so that standard trigger
    // fields inserted below always take precedence and cannot be spoofed.
    for (key, val) in &trigger_ctx.webhook_fields {
        // Skip any key that would collide with a standard trigger_ or steps_ variable.
        if key.starts_with("trigger_") || key.starts_with("steps_") {
            continue;
        }
        let var_name = format!("trigger_{key}");
        ctx.set_value(var_name, Value::String(val.clone()))
            .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;
    }

    let trigger_fields = [
        ("trigger_text", trigger_ctx.text.as_str()),
        ("trigger_author", trigger_ctx.author.as_str()),
        ("trigger_channel_id", trigger_ctx.channel_id.as_str()),
        ("trigger_timestamp", trigger_ctx.timestamp.as_str()),
        ("trigger_emoji", trigger_ctx.emoji.as_str()),
        ("trigger_message_id", trigger_ctx.message_id.as_str()),
    ];

    for (name, val) in &trigger_fields {
        ctx.set_value((*name).into(), Value::String((*val).to_owned()))
            .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;
    }

    for (step_id, output) in step_outputs {
        if let JsonValue::Object(map) = output {
            for (field, val) in map {
                let var_name = format!("steps_{step_id}_output_{field}");
                let eval_val = json_value_to_eval(val);
                ctx.set_value(var_name, eval_val)
                    .map_err(|e| WorkflowError::ConditionError(e.to_string()))?;
            }
        }
    }

    Ok(ctx)
}

/// Convert a `serde_json::Value` to an `evalexpr::Value`.
fn json_value_to_eval(v: &JsonValue) -> evalexpr::Value {
    use evalexpr::Value as EV;
    match v {
        JsonValue::String(s) => EV::String(s.clone()),
        JsonValue::Bool(b) => EV::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                EV::Int(i)
            } else if let Some(f) = n.as_f64() {
                EV::Float(f)
            } else {
                EV::String(n.to_string())
            }
        }
        JsonValue::Null => EV::Empty,
        other => EV::String(other.to_string()),
    }
}

/// Maximum wall-clock time allowed for a single `evalexpr` evaluation.
///
/// `evalexpr` is not designed for adversarial input — a deeply nested or
/// recursive expression can spin indefinitely. We run the evaluation on a
/// blocking thread and impose a hard timeout.
const EVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

/// Evaluate a boolean `if:` expression against the current execution context.
///
/// Returns `true` if the step should run, `false` if it should be skipped.
///
/// The evaluation is wrapped in a [`tokio::time::timeout`] to prevent a
/// malicious or pathological expression from blocking a Tokio worker thread.
pub async fn evaluate_condition(
    expr: &str,
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Result<bool, WorkflowError> {
    let ctx = build_eval_context(trigger_ctx, step_outputs)?;
    let expr_owned = expr.to_owned();

    // Bound expression complexity to prevent pathological evaluation times.
    // The spawn_blocking thread cannot be cancelled by tokio::time::timeout —
    // it will run to completion even after timeout. Length-limiting the expression
    // prevents worst-case O(2^n) evaluation paths.
    const MAX_EXPR_LEN: usize = 4096;
    if expr_owned.len() > MAX_EXPR_LEN {
        return Err(WorkflowError::ConditionError(format!(
            "condition expression exceeds {} byte limit",
            MAX_EXPR_LEN
        )));
    }

    let result = tokio::time::timeout(
        EVAL_TIMEOUT,
        tokio::task::spawn_blocking(move || evalexpr::eval_boolean_with_context(&expr_owned, &ctx)),
    )
    .await
    .map_err(|_| {
        WorkflowError::ConditionError(format!(
            "'{expr}': evaluation timed out after {}ms",
            EVAL_TIMEOUT.as_millis()
        ))
    })?
    .map_err(|e| WorkflowError::ConditionError(format!("'{expr}': eval task panicked: {e}")))?
    .map_err(|e| WorkflowError::ConditionError(format!("'{expr}': {e}")))?;

    Ok(result)
}

/// Resolve all template variables in a step's action fields.
///
/// Returns a new `ActionDef` with all `{{...}}` placeholders substituted.
pub fn resolve_step_templates(
    step: &Step,
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Result<ActionDef, WorkflowError> {
    use ActionDef::*;

    let t = |s: &str| resolve_template(s, trigger_ctx, step_outputs);
    let t_opt = |s: &Option<String>| -> Result<Option<String>, WorkflowError> {
        match s {
            Some(v) => Ok(Some(t(v)?)),
            None => Ok(None),
        }
    };
    let t_strict = |field: &str, s: &str| resolve_ci_template(field, s, trigger_ctx, step_outputs);
    let t_opt_strict = |field: &str, s: &Option<String>| -> Result<Option<String>, WorkflowError> {
        match s {
            Some(value) => Ok(Some(t_strict(field, value)?)),
            None => Ok(None),
        }
    };

    match &step.action {
        SendMessage { text, channel } => Ok(SendMessage {
            text: t(text)?,
            channel: t_opt(channel)?,
        }),
        SendDm { to, text } => Ok(SendDm {
            to: t(to)?,
            text: t(text)?,
        }),
        SetChannelTopic { topic } => Ok(SetChannelTopic { topic: t(topic)? }),
        AddReaction { emoji } => Ok(AddReaction { emoji: t(emoji)? }),
        CallWebhook {
            url,
            method,
            headers,
            body,
        } => {
            let resolved_headers = match headers {
                Some(h) => {
                    let mut out = std::collections::HashMap::new();
                    for (k, v) in h {
                        out.insert(k.clone(), t(v)?);
                    }
                    Some(out)
                }
                None => None,
            };
            Ok(CallWebhook {
                url: t(url)?,
                method: method.clone(),
                headers: resolved_headers,
                body: t_opt(body)?,
            })
        }
        RequestApproval {
            from,
            message,
            timeout,
        } => Ok(RequestApproval {
            from: t(from)?,
            message: t(message)?,
            timeout: timeout.clone(),
        }),
        Delay { duration } => Ok(Delay {
            duration: duration.clone(),
        }),
        // Literal by construction: the host never receives these values, it
        // recompiles them from the project's own actions.yml (and resolves a
        // wake_agent brief's templates itself, from the request's context).
        RunOnHost { .. } | WakeAgent { .. } | HireAgent { .. } => Ok(step.action.clone()),
        RecordCiResult {
            project,
            repository,
            check,
            phase,
            commit,
            run,
            attempt,
            conclusion,
            evidence_url,
            summary,
        } => Ok(RecordCiResult {
            project: project.clone(),
            repository: repository.clone(),
            check: check.clone(),
            phase: *phase,
            commit: t_strict("commit", commit)?,
            run: t_strict("run", run)?,
            attempt: t_strict("attempt", attempt)?,
            conclusion: t_strict("conclusion", conclusion)?,
            evidence_url: t_opt_strict("evidence_url", evidence_url)?,
            summary: t_opt_strict("summary", summary)?,
        }),
    }
}

/// Result of dispatching a single step action.
#[derive(Debug)]
pub enum StepResult {
    /// Step completed normally. Output is stored in `step_outputs`.
    Completed(JsonValue),
    /// Step requests suspension (approval gate or host step). Execution must
    /// pause; the step loop makes the suspension durable and announces it.
    Suspended(Suspension),
    /// Step was skipped due to `if:` condition being false.
    Skipped,
}

fn resolve_send_message_channel(
    explicit_channel: Option<&str>,
    trigger_channel: &str,
    workflow_channel_id: Option<Uuid>,
) -> Result<String, WorkflowError> {
    let explicit_channel = explicit_channel
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(workflow_channel_id) = workflow_channel_id {
        if let Some(explicit_channel) = explicit_channel {
            let override_channel_id = explicit_channel.parse::<Uuid>().map_err(|e| {
                WorkflowError::InvalidDefinition(format!(
                    "SendMessage: invalid channel override UUID: {e}"
                ))
            })?;
            if override_channel_id != workflow_channel_id {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "SendMessage: channel override must match the workflow channel ({workflow_channel_id})"
                )));
            }
        }
        return Ok(workflow_channel_id.to_string());
    }

    if let Some(explicit_channel) = explicit_channel {
        let override_channel_id = explicit_channel.parse::<Uuid>().map_err(|e| {
            WorkflowError::InvalidDefinition(format!(
                "SendMessage: invalid channel override UUID: {e}"
            ))
        })?;
        return Ok(override_channel_id.to_string());
    }

    if trigger_channel.trim().is_empty() {
        return Err(WorkflowError::InvalidDefinition(
            "SendMessage: no channel_id available (trigger has no channel context and no channel override was specified)"
                .into(),
        ));
    }

    Ok(trigger_channel.trim().to_string())
}

/// Dispatch a resolved action and return its output.
///
/// For MVP, most actions log their intent and return a success output.
/// Real event emission is wired in WF-07/08 (relay integration).
///
/// `RequestApproval` returns `StepResult::Suspended` — the caller must
/// persist state and stop the execution loop.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_action(
    step_id: &str,
    action: &ActionDef,
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    trigger_ctx: &TriggerContext,
    step_outputs: &HashMap<String, JsonValue>,
) -> Result<StepResult, WorkflowError> {
    use ActionDef::*;

    // The workflow engine can outlive the serving request that spawned it.
    // Revalidate the durable community fence immediately before every external
    // side effect (message publish, webhook, delay/resume). A storage failure is
    // a denial, never permission to continue.
    let serving_write =
        beekeeper_deletion::acquire_serving_write(&engine.db, community_id, "workflow_action")
            .await
            .map_err(|error| {
                WorkflowError::WebhookError(format!(
                    "community write fence rejected workflow side effect: {error}"
                ))
            })?;

    serving_write.verify().await.map_err(|error| {
        WorkflowError::WebhookError(format!("community write lease lost: {error}"))
    })?;

    let result = serving_write
        .protect(async {
            match action {
                SendMessage { text, channel } => {
                    // Look up workflow metadata for destination validation and
                    // attribution, scoped to the run's community — the same run/workflow
                    // UUID may exist in another community, so a bare-id lookup could
                    // load the wrong row and drive a side effect under it.
                    let wf_run = engine
                        .db
                        .get_workflow_run(community_id, run_id)
                        .await
                        .map_err(|e| {
                            WorkflowError::WebhookError(format!(
                                "SendMessage: failed to load workflow run {run_id}: {e}"
                            ))
                        })?;
                    let workflow = engine
                        .db
                        .get_workflow(community_id, wf_run.workflow_id)
                        .await
                        .map_err(|e| {
                            WorkflowError::WebhookError(format!(
                                "SendMessage: failed to load workflow {}: {e}",
                                wf_run.workflow_id
                            ))
                        })?;
                    let channel_id = resolve_send_message_channel(
                        channel.as_deref(),
                        &trigger_ctx.channel_id,
                        workflow.channel_id,
                    )?;
                    let owner_pubkey_hex = hex::encode(&workflow.owner_pubkey);

                    info!(
                        run_id = %run_id,
                        step = step_id,
                        channel = %channel_id,
                        "SendMessage → {channel_id}: {text}"
                    );

                    let event_id = engine
                        .action_sink()?
                        .send_message(community_id, &channel_id, text, &owner_pubkey_hex)
                        .await
                        .map_err(WorkflowError::from)?;

                    Ok(StepResult::Completed(serde_json::json!({
                        "sent": true,
                        "event_id": event_id,
                    })))
                }

                SendDm { to, text: _ } => {
                    warn!(run_id = %run_id, step = step_id, "SendDm not yet implemented (to={to})");
                    // TODO (WF-07): emit DM event.
                    Err(WorkflowError::NotImplemented("SendDm".into()))
                }

                SetChannelTopic { topic: _ } => {
                    warn!(run_id = %run_id, step = step_id, "SetChannelTopic not yet implemented");
                    // TODO (WF-07): update channel topic via DB.
                    Err(WorkflowError::NotImplemented("SetChannelTopic".into()))
                }

                AddReaction { emoji } => {
                    info!(run_id = %run_id, step = step_id, "AddReaction → :{emoji}:");
                    if trigger_ctx.message_id.is_empty() {
                        Err(WorkflowError::InvalidDefinition(
                            "AddReaction: no trigger.message_id available".into(),
                        ))
                    } else {
                        #[cfg(feature = "reqwest")]
                        {
                            let result = add_reaction_impl(&trigger_ctx.message_id, emoji).await?;
                            Ok(StepResult::Completed(result))
                        }

                        #[cfg(not(feature = "reqwest"))]
                        {
                            warn!(
                                run_id = %run_id,
                                step = step_id,
                                "AddReaction: reqwest feature not enabled, skipping HTTP call"
                            );
                            Ok(StepResult::Completed(
                                serde_json::json!({ "added": false, "skipped": true }),
                            ))
                        }
                    }
                }

                CallWebhook {
                    url,
                    method,
                    headers,
                    body,
                } => {
                    let method_str = method.as_deref().unwrap_or("POST");
                    info!(run_id = %run_id, step = step_id, "CallWebhook → {method_str} {url}");

                    #[cfg(feature = "reqwest")]
                    {
                        let result = call_webhook_impl(url, method_str, headers, body).await?;
                        Ok(StepResult::Completed(result))
                    }

                    #[cfg(not(feature = "reqwest"))]
                    {
                        // reqwest not enabled — log and return placeholder.
                        warn!(
                            run_id = %run_id, step = step_id,
                            "CallWebhook: reqwest feature not enabled, skipping HTTP call"
                        );
                        let _ = (headers, body); // suppress unused warnings
                        Ok(StepResult::Completed(serde_json::json!({
                            "status": 0,
                            "body": null,
                            "skipped": true
                        })))
                    }
                }

                RequestApproval {
                    from,
                    message,
                    timeout,
                } => {
                    let timeout_str = timeout.as_deref().unwrap_or("24h");
                    let timeout_secs = parse_duration_secs(timeout_str)?;
                    info!(
                        run_id = %run_id, step = step_id,
                        "RequestApproval from={from} timeout={timeout_str}: {message}"
                    );
                    // The step loop writes the approval row, parks the run as
                    // waiting_approval and publishes the kind:46010 — in that
                    // order — via `suspend::persist_and_publish`.
                    Ok(StepResult::Suspended(Suspension::Approval {
                        step_id: step_id.to_owned(),
                        approver_spec: from.clone(),
                        message: message.clone(),
                        timeout_secs,
                        synthetic: false,
                    }))
                }

                WakeAgent { to, .. } => {
                    // A routed brief is not a command, so no approval gate;
                    // the host is handed the earlier steps' outputs so the
                    // brief can carry the preceding host step's result.
                    info!(
                        run_id = %run_id, step = step_id, agent = %to.agent,
                        "wake_agent: handing the brief to a host"
                    );
                    Ok(StepResult::Suspended(Suspension::HostStep {
                        step_id: step_id.to_owned(),
                        step_kind: beekeeper_core::host_step::HOST_STEP_KIND_WAKE_AGENT.to_owned(),
                        approval: None,
                        inputs: serde_json::Value::Object(
                            step_outputs
                                .iter()
                                .map(|(id, output)| (id.clone(), output.clone()))
                                .collect(),
                        ),
                    }))
                }

                HireAgent { role, session, .. } => {
                    // A hire is not a command either: no approval gate. The
                    // host recompiles the role and brief from its own file.
                    info!(
                        run_id = %run_id, step = step_id, %role, agent = %session.agent,
                        "hire_agent: handing the hire to a host"
                    );
                    Ok(StepResult::Suspended(Suspension::HostStep {
                        step_id: step_id.to_owned(),
                        step_kind: beekeeper_core::host_step::HOST_STEP_KIND_HIRE_AGENT.to_owned(),
                        approval: None,
                        inputs: serde_json::Value::Object(
                            step_outputs
                                .iter()
                                .map(|(id, output)| (id.clone(), output.clone()))
                                .collect(),
                        ),
                    }))
                }

                RunOnHost { .. } => {
                    // Validate limits exactly as the host will, so a step the
                    // host would refuse never reaches it.
                    crate::schema::resolve_run_on_host(action)?;
                    let wf_run = engine
                        .db
                        .get_workflow_run(community_id, run_id)
                        .await
                        .map_err(|e| {
                            WorkflowError::Database(format!(
                                "run_on_host: failed to load workflow run {run_id}: {e}"
                            ))
                        })?;
                    let workflow = engine
                        .db
                        .get_workflow(community_id, wf_run.workflow_id)
                        .await
                        .map_err(|e| {
                            WorkflowError::Database(format!(
                                "run_on_host: failed to load workflow {}: {e}",
                                wf_run.workflow_id
                            ))
                        })?;
                    // Spec § 5.4: every host step is gated on the operator's
                    // approval unless a grant already released this step —
                    // this run's own grant, or an unrevoked autorun grant
                    // bound to exactly this definition hash (an edit changes
                    // the hash and re-arms the gate). The gate is bound to
                    // the host step's own index, so the grant resumes *at*
                    // the step (`resume_index_after_approval`).
                    let autorun = engine
                        .db
                        .find_active_autorun_grant(
                            community_id,
                            wf_run.workflow_id,
                            &workflow.definition_hash,
                        )
                        .await?
                        .map(|grant| beekeeper_core::host_step::HostStepApproval {
                            approval_ref: hex::encode(&grant.grant_event_id),
                            scope: "action".into(),
                        });
                    let released = match autorun {
                        Some(grant) => Some(grant),
                        None => {
                            crate::suspend::granted_approval_for_step(
                                engine,
                                community_id,
                                wf_run.workflow_id,
                                run_id,
                                step_id,
                            )
                            .await?
                        }
                    };
                    match released {
                        Some(approval) => {
                            info!(
                                run_id = %run_id, step = step_id,
                                "run_on_host: approval granted — handing the step to a host"
                            );
                            Ok(StepResult::Suspended(Suspension::HostStep {
                                step_id: step_id.to_owned(),
                                step_kind: beekeeper_core::host_step::HOST_STEP_KIND_RUN_ON_HOST
                                    .to_owned(),
                                approval: Some(approval),
                                inputs: serde_json::Value::Object(Default::default()),
                            }))
                        }
                        None => {
                            let owner_hex = nostr::PublicKey::from_slice(&workflow.owner_pubkey)
                                .map(|key| key.to_hex())
                                .map_err(|e| {
                                    WorkflowError::Database(format!(
                                        "run_on_host: workflow owner pubkey is invalid: {e}"
                                    ))
                                })?;
                            // A project action may be approved by any project
                            // Owner (spec § 5.4's `project-owner:<coord>`);
                            // a plain workflow only by its owner.
                            let approver_spec = match workflow.project_ref.as_deref() {
                                Some(project) => format!("project-owner:{project}"),
                                None => owner_hex,
                            };
                            info!(
                                run_id = %run_id, step = step_id, %approver_spec,
                                "run_on_host: no approval yet — asking for one"
                            );
                            Ok(StepResult::Suspended(Suspension::Approval {
                                step_id: step_id.to_owned(),
                                approver_spec,
                                message: format!(
                                    "Run step `{step_id}` of `{}` on your host?",
                                    workflow.name
                                ),
                                timeout_secs: crate::suspend::SYNTHETIC_APPROVAL_TIMEOUT_SECS,
                                synthetic: true,
                            }))
                        }
                    }
                }

                Delay { duration } => {
                    let secs = parse_duration_secs(duration)?;
                    // Cap delay at 270 seconds (4.5 minutes) — must be less than default_timeout_secs (300s)
                    // to avoid non-deterministic StepTimeout. Long delays (hours/days)
                    // should use the scheduled resume pattern (future work: WF-09).
                    const MAX_DELAY_SECS: u64 = 270;
                    if secs > MAX_DELAY_SECS {
                        return Err(WorkflowError::InvalidDefinition(format!(
                            "delay exceeds maximum of {MAX_DELAY_SECS} seconds (got {secs}s); \
                     use the scheduled resume pattern for long delays"
                        )));
                    }
                    info!(run_id = %run_id, step = step_id, "Delay {duration} ({secs}s)");
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                    Ok(StepResult::Completed(
                        serde_json::json!({ "slept_secs": secs }),
                    ))
                }

                RecordCiResult {
                    project,
                    repository,
                    check,
                    phase,
                    commit,
                    run,
                    attempt,
                    conclusion,
                    evidence_url,
                    summary,
                } => {
                    let attempt = attempt.parse::<u32>().map_err(|_| {
                        WorkflowError::InvalidDefinition(
                            "record_ci_result attempt must resolve to a positive integer".into(),
                        )
                    })?;
                    let conclusion = match conclusion.as_str() {
                        "success" => beekeeper_core::ci_result::CiConclusion::Success,
                        "failure" => beekeeper_core::ci_result::CiConclusion::Failure,
                        "cancelled" => beekeeper_core::ci_result::CiConclusion::Cancelled,
                        _ => {
                            return Err(WorkflowError::InvalidDefinition(
                                "record_ci_result conclusion must resolve to success, failure, or cancelled"
                                    .into(),
                            ));
                        }
                    };

                    // The workflow identity is loaded from the stored run. The
                    // callback cannot choose or spoof this correlation field.
                    let wf_run = engine
                        .db
                        .get_workflow_run(community_id, run_id)
                        .await
                        .map_err(|e| {
                            WorkflowError::Database(format!(
                                "record_ci_result: failed to load workflow run {run_id}: {e}"
                            ))
                        })?;
                    let result = beekeeper_core::ci_result::CiResult {
                        schema: beekeeper_core::ci_result::CI_RESULT_SCHEMA.to_owned(),
                        identity: beekeeper_core::ci_result::CiResultIdentity {
                            project: project.clone(),
                            repository: repository.clone(),
                            commit: commit.clone(),
                            check: check.clone(),
                            run: run.clone(),
                            attempt,
                            workflow: wf_run.workflow_id.to_string(),
                            phase: *phase,
                        },
                        conclusion,
                        evidence_url: evidence_url.clone(),
                        summary: summary.clone(),
                    };

                    let event_id = engine
                        .action_sink()?
                        .record_ci_result(community_id, &result)
                        .await
                        .map_err(WorkflowError::from)?;
                    Ok(StepResult::Completed(serde_json::json!({
                        "recorded": true,
                        "event_id": event_id,
                    })))
                }
            }
        })
        .await
        .map_err(|error| {
            WorkflowError::WebhookError(format!("community write lease lost: {error}"))
        })?;
    let release = serving_write.finish().await.map_err(|error| {
        WorkflowError::WebhookError(format!("community write lease release failed: {error}"))
    });
    match result {
        Ok(value) => {
            release?;
            Ok(value)
        }
        Err(error) => {
            let _ = release;
            Err(error)
        }
    }
}

/// Parse a duration string like "5m", "1h", "30s" into seconds.
///
/// Exposed as `pub(crate)` so `schema.rs` can use it for interval validation.
pub(crate) fn parse_duration_secs(duration: &str) -> Result<u64, WorkflowError> {
    let duration = duration.trim();
    if let Some(n) = duration.strip_suffix('h') {
        let hours: u64 = n.trim().parse().map_err(|_| {
            WorkflowError::InvalidDefinition(format!("invalid duration: {duration}"))
        })?;
        return hours.checked_mul(3600).ok_or_else(|| {
            WorkflowError::InvalidDefinition(format!("duration overflow: {duration}"))
        });
    }
    if let Some(n) = duration.strip_suffix('m') {
        let mins: u64 = n.trim().parse().map_err(|_| {
            WorkflowError::InvalidDefinition(format!("invalid duration: {duration}"))
        })?;
        return mins.checked_mul(60).ok_or_else(|| {
            WorkflowError::InvalidDefinition(format!("duration overflow: {duration}"))
        });
    }
    if let Some(n) = duration.strip_suffix('s') {
        let secs: u64 = n.trim().parse().map_err(|_| {
            WorkflowError::InvalidDefinition(format!("invalid duration: {duration}"))
        })?;
        return Ok(secs);
    }
    // Plain number — assume seconds.
    duration
        .parse()
        .map_err(|_| WorkflowError::InvalidDefinition(format!("invalid duration: {duration}")))
}

// is_private_ip is provided by beekeeper_core::network::is_private_ip

/// Resolve `host` to IP addresses and reject if any are private/reserved.
///
/// Uses the OS resolver (blocking, run on a threadpool via `spawn_blocking`).
/// Rejects the request if DNS resolution fails or returns zero addresses.
///
/// Returns the first validated IP address so the caller can pin DNS resolution
/// in the HTTP client, preventing DNS rebinding TOCTOU attacks.
#[cfg(feature = "reqwest")]
async fn check_ssrf(host: &str, port: u16) -> Result<std::net::IpAddr, WorkflowError> {
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<std::net::IpAddr> = tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        addr_str
            .to_socket_addrs()
            .map(|iter| iter.map(|sa| sa.ip()).collect::<Vec<_>>())
    })
    .await
    .map_err(|e| WorkflowError::WebhookError(format!("SSRF check task failed: {e}")))?
    .map_err(|e| WorkflowError::WebhookError(format!("DNS resolution failed: {e}")))?;

    if addrs.is_empty() {
        return Err(WorkflowError::WebhookError(
            "DNS resolution returned no addresses".into(),
        ));
    }

    debug!("Resolved webhook host '{}' → {:?}", host, addrs);

    for ip in &addrs {
        if beekeeper_core::network::is_private_ip(ip) {
            return Err(WorkflowError::WebhookError(format!(
                "SSRF blocked: '{host}' resolved to private/reserved address {ip}"
            )));
        }
    }

    Ok(addrs[0])
}

/// Maximum response body size for webhook calls (1 MiB).
#[cfg(feature = "reqwest")]
const WEBHOOK_MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[cfg(feature = "reqwest")]
async fn call_webhook_impl(
    url: &str,
    method: &str,
    headers: &Option<std::collections::HashMap<String, String>>,
    body: &Option<String>,
) -> Result<JsonValue, WorkflowError> {
    use reqwest::Client;
    use std::time::Duration;

    let parsed_url = reqwest::Url::parse(url)
        .map_err(|e| WorkflowError::WebhookError(format!("invalid URL: {e}")))?;

    let host = parsed_url
        .host_str()
        .ok_or_else(|| WorkflowError::WebhookError("URL has no host".into()))?;

    // Default ports: 443 for https, 80 for http.
    let port = parsed_url.port_or_known_default().unwrap_or(80);

    let safe_ip = check_ssrf(host, port).await?;

    // Client is built per-request because `resolve()` pins DNS for a specific host.
    // This disables connection pooling but is required for SSRF safety: without
    // pinning, reqwest performs its own DNS resolution which could return a
    // different address than the one validated above (DNS rebinding TOCTOU).
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        // A system proxy would resolve the original hostname itself, bypassing
        // the validated and pinned address above.
        .no_proxy()
        // Disable redirects — a redirect to an internal host bypasses the SSRF check.
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, std::net::SocketAddr::new(safe_ip, port))
        .build()
        .map_err(|e| WorkflowError::WebhookError(e.to_string()))?;

    let method_parsed = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|e| WorkflowError::WebhookError(e.to_string()))?;

    let mut req = client.request(method_parsed, url);

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            req = req.header(k, v);
        }
    }

    if let Some(b) = body {
        req = req.body(b.clone());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| WorkflowError::WebhookError(e.to_string()))?;

    let status = resp.status().as_u16();

    // Read incrementally to prevent OOM from a malicious server returning a
    // multi-GB payload. `resp.bytes()` would buffer the entire body before we
    // could check the size; chunked reading lets us abort early.
    let mut body_bytes = Vec::new();
    let mut resp = resp;
    loop {
        let chunk = resp
            .chunk()
            .await
            .map_err(|e| WorkflowError::WebhookError(format!("reading response body: {e}")))?;
        match chunk {
            Some(bytes) => {
                body_bytes.extend_from_slice(&bytes);
                if body_bytes.len() > WEBHOOK_MAX_RESPONSE_BYTES {
                    return Err(WorkflowError::WebhookError(format!(
                        "response body exceeds {} byte limit",
                        WEBHOOK_MAX_RESPONSE_BYTES
                    )));
                }
            }
            None => break,
        }
    }

    let body_text = String::from_utf8_lossy(&body_bytes).into_owned();

    Ok(serde_json::json!({
        "status": status,
        "body": body_text,
    }))
}

/// Returns a shared `reqwest::Client` reused across all workflow HTTP calls.
/// Sharing a single client reuses the underlying connection pool.
#[cfg(feature = "reqwest")]
fn shared_http_client() -> &'static reqwest::Client {
    use std::sync::LazyLock;
    use std::time::Duration;
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP client build must succeed")
    });
    &CLIENT
}

/// POST `{"emoji": emoji}` to `POST /api/messages/{message_id}/reactions`.
#[cfg(feature = "reqwest")]
async fn add_reaction_impl(message_id: &str, emoji: &str) -> Result<JsonValue, WorkflowError> {
    let base_url = std::env::var("BEEKEEPER_RELAY_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_owned());

    let url = format!("{base_url}/api/messages/{message_id}/reactions");

    let client = shared_http_client();

    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "emoji": emoji }));

    if let Ok(token) = std::env::var("BEEKEEPER_API_TOKEN") {
        req = req.header("Authorization", format!("Bearer {token}"));
    } else if let Ok(pubkey) = std::env::var("BEEKEEPER_RELAY_PUBKEY") {
        req = req.header("X-Pubkey", pubkey);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| WorkflowError::WebhookError(format!("AddReaction HTTP error: {e}")))?;

    let status = resp.status();

    if !status.is_success() {
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable>".to_owned());
        return Err(WorkflowError::WebhookError(format!(
            "AddReaction: relay returned {status} for message {message_id}: {body}"
        )));
    }

    let body_text = resp.text().await.unwrap_or_else(|_| String::new());
    let body_json: JsonValue = serde_json::from_str(&body_text)
        .unwrap_or_else(|_| serde_json::json!({ "raw": body_text }));

    Ok(serde_json::json!({
        "added": true,
        "status": status.as_u16(),
        "response": body_json,
    }))
}

/// Terminal reason for a run whose action definition changed after the run
/// was created: the operator's approval named a definition that is no longer
/// the one the relay would execute.
pub const RUN_STOPPED_DEFINITION_CHANGED: &str = "definition_changed";

/// Terminal reason for a run created before runs carried their definition
/// binding (migration 0046). It stays readable; it never executes.
pub const RUN_STOPPED_DEFINITION_UNKNOWN: &str = "definition_unknown";

/// Why a run was stopped short of its next step by a fact about the *run*
/// rather than a failure of a step.
///
/// A project action's approval is the operator's consent for one definition
/// to run a command on their machine (spec § 5.4). A run is bound to the
/// definition selected when it was created, and the binding is checked at
/// every entry into execution and again before any request is published. A
/// mismatch is not a retry: the run ends here and the work needs a fresh run
/// and a fresh approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStop {
    /// The stored definition is not the one this run was created from.
    DefinitionChanged {
        /// Lowercase hex of the definition the run was created from.
        run_definition_hash: String,
        /// Lowercase hex of the definition stored now.
        current_definition_hash: String,
    },
    /// The run carries no definition binding at all.
    DefinitionUnknown {
        /// Lowercase hex of the definition stored now.
        current_definition_hash: String,
    },
    /// The body the engine was handed is not the published definition the run
    /// is bound to. A trigger handler parses a definition and then asks for a
    /// run; a republication in between must not leave the engine evaluating
    /// one definition's conditions and steps under another's binding (ledger
    /// 199, review finding 2). Both hashes are normalized through
    /// [`WorkflowDef`](crate::schema::WorkflowDef), so a webhook definition's
    /// injected secret — which the stored hash covers and the parsed form
    /// does not — cannot make an honest pair look like a substitution.
    ExecutingADifferentDefinition {
        /// Lowercase hex of the normalized body the engine was handed.
        executed_definition_hash: String,
        /// Lowercase hex of the normalized published definition.
        current_definition_hash: String,
    },
}

impl RunStop {
    /// Stable machine-readable reason, written to `workflow_runs.error_code`.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DefinitionChanged { .. } | Self::ExecutingADifferentDefinition { .. } => {
                RUN_STOPPED_DEFINITION_CHANGED
            }
            Self::DefinitionUnknown { .. } => RUN_STOPPED_DEFINITION_UNKNOWN,
        }
    }

    /// What a reader of the run is told. Names both hashes so the disagreement
    /// is checkable rather than asserted.
    pub fn message(&self) -> String {
        match self {
            Self::DefinitionChanged {
                run_definition_hash,
                current_definition_hash,
            } => format!(
                "the action definition changed after this run started (run was created from \
                 {run_definition_hash}, the published definition is now \
                 {current_definition_hash}); an approval names one definition, so this run \
                 stops — start a fresh run and approve it"
            ),
            Self::DefinitionUnknown {
                current_definition_hash,
            } => format!(
                "this run predates definition binding, so nothing can say which definition it \
                 was approved for (the published definition is {current_definition_hash}); it \
                 stays readable but cannot execute — start a fresh run"
            ),
            Self::ExecutingADifferentDefinition {
                executed_definition_hash,
                current_definition_hash,
            } => format!(
                "the body handed to the engine ({executed_definition_hash}) is not the \
                 published definition this run is bound to ({current_definition_hash}), so the \
                 conditions and steps it would evaluate are not the ones anyone published; \
                 this run stops — start a fresh run"
            ),
        }
    }
}

/// Compare the definition a run was created from with the one stored now.
///
/// `None` means the run may proceed. This is the whole rule, in one place, so
/// the approval check, the resume check and the emission check cannot drift.
pub fn run_definition_stop(
    run: &beekeeper_db::workflow::WorkflowRunRecord,
    workflow: &beekeeper_db::workflow::WorkflowRecord,
) -> Option<RunStop> {
    match run.definition_hash.as_deref() {
        Some(bound) if bound == workflow.definition_hash.as_slice() => None,
        Some(bound) => Some(RunStop::DefinitionChanged {
            run_definition_hash: hex::encode(bound),
            current_definition_hash: hex::encode(&workflow.definition_hash),
        }),
        None => Some(RunStop::DefinitionUnknown {
            current_definition_hash: hex::encode(&workflow.definition_hash),
        }),
    }
}

/// Compare the body the engine is about to execute with the published
/// definition, both normalized through [`WorkflowDef`](crate::schema::WorkflowDef).
///
/// [`run_definition_stop`] answers "is the run still the row it was created
/// from"; this answers "is the body in my hand that row". Both are needed:
/// the first catches an edit after creation, the second catches a body parsed
/// before an edit and carried past it.
pub fn executed_definition_stop(
    def: &WorkflowDef,
    workflow: &beekeeper_db::workflow::WorkflowRecord,
) -> Result<Option<RunStop>, WorkflowError> {
    let executed = crate::hash::definition_hash(def)?;
    let published: WorkflowDef = serde_json::from_value(workflow.definition.clone())
        .map_err(|error| WorkflowError::InvalidDefinition(format!("stored definition: {error}")))?;
    let current = crate::hash::definition_hash(&published)?;
    if executed == current {
        return Ok(None);
    }
    Ok(Some(RunStop::ExecutingADifferentDefinition {
        executed_definition_hash: hex::encode(executed),
        current_definition_hash: hex::encode(current),
    }))
}

/// Load the run and its workflow and apply both comparisons:
/// [`run_definition_stop`] (the run against the published row) and
/// [`executed_definition_stop`] (the body in hand against the same row).
///
/// A database error is *not* an answer: it propagates, so a run is never
/// allowed to proceed because the check could not be made.
pub async fn check_run_definition(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    def: &WorkflowDef,
) -> Result<Option<RunStop>, WorkflowError> {
    let run = engine.db.get_workflow_run(community_id, run_id).await?;
    let workflow = engine
        .db
        .get_workflow(community_id, run.workflow_id)
        .await?;
    if let Some(stop) = run_definition_stop(&run, &workflow) {
        return Ok(Some(stop));
    }
    executed_definition_stop(def, &workflow)
}

/// Rich return type from `execute_run` / `execute_from_step`.
///
/// Carries enough information for the caller to:
/// - Update the run's execution trace and current step in the DB.
/// - Resume execution from the correct step after approval.
///
/// A suspended result is already durable: the step loop wrote the approval
/// or host-step row, parked the run and published the request before
/// returning (see [`crate::suspend`]). `finalize_run` writes nothing for it.
#[derive(Debug)]
pub struct ExecutionResult {
    /// Set when execution suspended at an approval gate or a host step.
    /// `None` means the run completed normally.
    pub suspension: Option<Suspension>,
    /// Index of the step that suspended (or the total step count on completion).
    pub step_index: usize,
    /// Accumulated step outputs at the point of suspension or completion.
    pub step_outputs: HashMap<String, JsonValue>,
    /// Execution trace: one entry per completed/skipped step.
    pub trace: Vec<JsonValue>,
    /// Set when the run was stopped by a fact about the run itself — today,
    /// its definition binding. `finalize_run` writes the named terminal
    /// reason; no step ran and nothing was published.
    pub stopped: Option<RunStop>,
}

/// Execute a workflow run sequentially.
///
/// Steps run in order. Each step:
/// 1. Evaluates `if:` condition (skip if false).
/// 2. Resolves template variables in action fields.
/// 3. Dispatches the action.
/// 4. Stores the step output for use by later steps.
///
/// On an approval gate or a host step: returns `ExecutionResult` with
/// `suspension = Some(..)`, the run already parked and its request published.
///
/// Returns `ExecutionResult` with `suspension = None` on normal completion.
///
/// Enforces `engine.config.max_concurrent` via a semaphore — returns
/// [`WorkflowError::CapacityExceeded`] immediately if all permits are taken.
/// Transitions the run to `Running` after acquiring a permit.
pub async fn execute_run(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    def: &WorkflowDef,
    trigger_ctx: &TriggerContext,
) -> Result<ExecutionResult, (WorkflowError, crate::error::PartialProgress)> {
    // Fail fast if all concurrency permits are in use — no queuing.
    let _permit = engine.run_semaphore.try_acquire().map_err(|_| {
        (
            WorkflowError::CapacityExceeded,
            crate::error::PartialProgress::default(),
        )
    })?;

    engine
        .db
        .update_workflow_run(
            community_id,
            run_id,
            beekeeper_db::workflow::RunStatus::Running,
            0,
            &serde_json::json!([]),
            None,
        )
        .await
        .map_err(|e| {
            (
                WorkflowError::from(e),
                crate::error::PartialProgress::default(),
            )
        })?;

    execute_steps(engine, community_id, run_id, def, trigger_ctx, 0, None).await
}

/// Resume execution from a specific step index (used for approval resume).
///
/// Acquires a concurrency permit from `engine.run_semaphore` before executing —
/// returns [`WorkflowError::CapacityExceeded`] immediately if all permits are
/// taken.
///
/// Transitions the run to `Running` after acquiring a permit, so that
/// approval-resumed runs correctly reflect their active state.
///
/// `initial_outputs` should be reconstructed from the execution trace before
/// calling this function on resume, so that steps after the resume point can
/// reference `{{steps.PREV_STEP.output.X}}` correctly.
pub async fn execute_from_step(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    def: &WorkflowDef,
    trigger_ctx: &TriggerContext,
    start_index: usize,
    initial_outputs: Option<HashMap<String, JsonValue>>,
) -> Result<ExecutionResult, (WorkflowError, crate::error::PartialProgress)> {
    // Fail fast if all concurrency permits are in use — no queuing.
    let _permit = engine.run_semaphore.try_acquire().map_err(|_| {
        (
            WorkflowError::CapacityExceeded,
            crate::error::PartialProgress::default(),
        )
    })?;

    // Mark run as Running now that we have a permit (resume from approval).
    // Preserve the existing execution trace from pre-approval steps.
    let existing_trace = match engine.db.get_workflow_run(community_id, run_id).await {
        Ok(r) => r.execution_trace,
        Err(e) => {
            warn!(
                run_id = %run_id,
                "Failed to read existing trace for resume — pre-approval trace will be lost: {e}"
            );
            serde_json::json!([])
        }
    };
    engine
        .db
        .update_workflow_run(
            community_id,
            run_id,
            beekeeper_db::workflow::RunStatus::Running,
            start_index as i32,
            &existing_trace,
            None,
        )
        .await
        .map_err(|e| {
            (
                WorkflowError::from(e),
                crate::error::PartialProgress::default(),
            )
        })?;

    execute_steps(
        engine,
        community_id,
        run_id,
        def,
        trigger_ctx,
        start_index,
        initial_outputs,
    )
    .await
}

/// Internal: execute workflow steps starting from `start_index`, without
/// acquiring the semaphore. Called by both [`execute_run`] and
/// [`execute_from_step`] after they have already acquired a permit.
///
/// On error, returns `(WorkflowError, PartialProgress)` so callers can persist
/// the trace of steps completed before the failure.
async fn execute_steps(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    def: &WorkflowDef,
    trigger_ctx: &TriggerContext,
    start_index: usize,
    initial_outputs: Option<HashMap<String, JsonValue>>,
) -> Result<ExecutionResult, (WorkflowError, crate::error::PartialProgress)> {
    let mut step_outputs: HashMap<String, JsonValue> = initial_outputs.unwrap_or_default();
    let mut trace: Vec<JsonValue> = Vec::new();

    // Every entry into execution — the first segment and every resume after
    // an approval or a host result — starts by checking that the run is still
    // the definition it was created from. A step that already ran is not
    // re-run; a step that has not run yet does not run at all.
    match check_run_definition(engine, community_id, run_id, def).await {
        Ok(None) => {}
        Ok(Some(stop)) => {
            warn!(
                run_id = %run_id,
                reason = stop.code(),
                "Run stopped before step {start_index}: {}",
                stop.message()
            );
            return Ok(ExecutionResult {
                suspension: None,
                step_index: start_index,
                step_outputs,
                trace,
                stopped: Some(stop),
            });
        }
        Err(error) => {
            return Err((
                error,
                crate::error::PartialProgress {
                    step_index: start_index,
                    trace,
                },
            ))
        }
    }

    for (i, step) in def.steps.iter().enumerate() {
        if i < start_index {
            debug!(run_id = %run_id, step = %step.id, "Skipping already-executed step");
            continue;
        }

        if let Some(expr) = &step.if_expr {
            match evaluate_condition(expr, trigger_ctx, &step_outputs).await {
                Ok(true) => {
                    debug!(run_id = %run_id, step = %step.id, "Condition true — running step");
                }
                Ok(false) => {
                    info!(run_id = %run_id, step = %step.id, "Condition false — skipping step");
                    trace.push(serde_json::json!({
                        "step_id": step.id,
                        "status": "skipped",
                    }));
                    continue;
                }
                Err(e) => {
                    warn!(run_id = %run_id, step = %step.id, "Condition error: {e}");
                    let progress = crate::error::PartialProgress {
                        step_index: i,
                        trace,
                    };
                    return Err((e, progress));
                }
            }
        }

        let resolved_action = match resolve_step_templates(step, trigger_ctx, &step_outputs) {
            Ok(a) => a,
            Err(e) => {
                let progress = crate::error::PartialProgress {
                    step_index: i,
                    trace,
                };
                return Err((e, progress));
            }
        };

        let timeout_secs = step
            .timeout_secs
            .unwrap_or(engine.config.default_timeout_secs);
        let dispatch_result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            dispatch_action(
                &step.id,
                &resolved_action,
                engine,
                community_id,
                run_id,
                trigger_ctx,
                &step_outputs,
            ),
        )
        .await;

        let result = match dispatch_result {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                let progress = crate::error::PartialProgress {
                    step_index: i,
                    trace,
                };
                return Err((e, progress));
            }
            Err(_timeout) => {
                let progress = crate::error::PartialProgress {
                    step_index: i,
                    trace,
                };
                return Err((
                    WorkflowError::StepTimeout {
                        step_id: step.id.clone(),
                        timeout_secs,
                    },
                    progress,
                ));
            }
        };

        match result {
            StepResult::Completed(output) => {
                debug!(run_id = %run_id, step = %step.id, "Step completed");
                trace.push(serde_json::json!({
                    "step_id": step.id,
                    "status": "completed",
                    "output": output,
                }));
                step_outputs.insert(step.id.clone(), output);
            }
            StepResult::Suspended(suspension) => {
                info!(
                    run_id = %run_id, step = %step.id,
                    "Step suspended — {}",
                    match &suspension {
                        Suspension::Approval { synthetic: true, .. } => "awaiting the operator's approval of a host step",
                        Suspension::Approval { .. } => "awaiting approval",
                        Suspension::HostStep { .. } => "handed to a host",
                    }
                );
                // The run row holds the trace prefix (execute_from_step wrote
                // it back); this segment is appended and the suspension row,
                // status and request are made durable in that order.
                let prefix = engine
                    .db
                    .get_workflow_run(community_id, run_id)
                    .await
                    .ok()
                    .and_then(|run| run.execution_trace.as_array().cloned())
                    .unwrap_or_default();
                let mut full_trace = prefix;
                full_trace.extend(trace.iter().cloned());
                match crate::suspend::persist_and_publish(
                    engine,
                    community_id,
                    run_id,
                    i,
                    &suspension,
                    &mut full_trace,
                )
                .await
                {
                    Ok(None) => {}
                    Ok(Some(stop)) => {
                        // The fence held at the emission point: nothing was
                        // written and nothing was published.
                        return Ok(ExecutionResult {
                            suspension: None,
                            step_index: i,
                            step_outputs,
                            trace,
                            stopped: Some(stop),
                        });
                    }
                    Err(e) => {
                        let progress = crate::error::PartialProgress {
                            step_index: i,
                            trace,
                        };
                        return Err((e, progress));
                    }
                }
                return Ok(ExecutionResult {
                    suspension: Some(suspension),
                    step_index: i,
                    step_outputs,
                    trace,
                    stopped: None,
                });
            }
            StepResult::Skipped => {
                debug!(run_id = %run_id, step = %step.id, "Step skipped");
                trace.push(serde_json::json!({
                    "step_id": step.id,
                    "status": "skipped",
                }));
            }
        }
    }

    info!(run_id = %run_id, "Workflow run completed");
    Ok(ExecutionResult {
        suspension: None,
        step_index: def.steps.len(),
        step_outputs,
        trace,
        stopped: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_trigger() -> TriggerContext {
        TriggerContext {
            text: "P1 incident in production".to_owned(),
            author: "abc123def456".to_owned(),
            channel_id: "channel-uuid-here".to_owned(),
            timestamp: "1700000000".to_owned(),
            emoji: "fire".to_owned(),
            message_id: "event-id-hex".to_owned(),
            webhook_fields: HashMap::new(),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_trigger_text() {
        let ctx = make_trigger();
        let out = resolve_template("Alert: {{trigger.text}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "Alert: P1 incident in production");
    }

    #[test]
    fn resolve_trigger_author() {
        let ctx = make_trigger();
        let out = resolve_template("By {{trigger.author}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "By abc123def456");
    }

    #[test]
    fn resolve_step_output() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("ask".to_owned(), json!({ "replied": "yes" }));
        let out = resolve_template("Reply: {{steps.ask.output.replied}}", &ctx, &outputs).unwrap();
        assert_eq!(out, "Reply: yes");
    }

    #[test]
    fn resolve_unknown_variable_left_literal() {
        let ctx = make_trigger();
        let out = resolve_template("{{unknown.var}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "{{unknown.var}}");
    }

    #[test]
    fn resolve_truncate_filter() {
        let ctx = make_trigger();
        let out =
            resolve_template("{{trigger.text | truncate(5)}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "P1 in");
        assert_eq!(out.chars().count(), 5);
    }

    #[test]
    fn resolve_npub_filter_encodes_hex_pubkey() {
        let mut ctx = make_trigger();
        ctx.author = "e17e5abf7b1dbd363f0ed6fbda2455609727b2555428dea251388c542cd2f03f".to_owned();
        let out = resolve_template("{{trigger.author | npub}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(
            out,
            "npub1u9l940mmrk7nv0cw6maa5fz4vztj0vj42s5dagj38zx9gtxj7qls94fpux"
        );
    }

    #[test]
    fn resolve_truncate_pubkey_is_alias_for_npub() {
        let mut ctx = make_trigger();
        ctx.author = "e17e5abf7b1dbd363f0ed6fbda2455609727b2555428dea251388c542cd2f03f".to_owned();
        let out = resolve_template(
            "{{trigger.author | truncate_pubkey}}",
            &ctx,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            out,
            "npub1u9l940mmrk7nv0cw6maa5fz4vztj0vj42s5dagj38zx9gtxj7qls94fpux"
        );
    }

    #[test]
    fn resolve_no_templates_fast_path() {
        let ctx = make_trigger();
        let out = resolve_template("no templates here", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "no templates here");
    }

    #[test]
    fn resolve_multiple_templates_in_one_string() {
        let ctx = make_trigger();
        let out = resolve_template(
            "{{trigger.author}} said: {{trigger.text}}",
            &ctx,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(out, "abc123def456 said: P1 incident in production");
    }

    #[test]
    fn resolve_webhook_field() {
        let mut ctx = make_trigger();
        ctx.webhook_fields
            .insert("service".to_owned(), "api-gateway".to_owned());
        let out = resolve_template("Service: {{trigger.service}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "Service: api-gateway");
    }

    #[tokio::test]
    async fn condition_true_when_text_contains_p1() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result =
            evaluate_condition("str_contains(trigger_text, \"P1\")", &ctx, &HashMap::new())
                .await
                .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_false_when_text_does_not_contain_p1() {
        let mut ctx = make_trigger();
        ctx.text = "normal message".to_owned();
        let result =
            evaluate_condition("str_contains(trigger_text, \"P1\")", &ctx, &HashMap::new())
                .await
                .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn condition_or_expression() {
        let ctx = make_trigger(); // text contains "P1"
        let result = evaluate_condition(
            "str_contains(trigger_text, \"P1\") || str_contains(trigger_text, \"SEV1\")",
            &ctx,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_step_output_bool() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("request".to_owned(), json!({ "approved": true }));
        let result = evaluate_condition("steps_request_output_approved == true", &ctx, &outputs)
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_step_output_bool_false() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("request".to_owned(), json!({ "approved": false }));
        let result = evaluate_condition("steps_request_output_approved == false", &ctx, &outputs)
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_invalid_expression_returns_error() {
        let ctx = make_trigger();
        let err = evaluate_condition("this is not valid evalexpr @@@@", &ctx, &HashMap::new())
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowError::ConditionError(_)));
    }

    #[tokio::test]
    async fn condition_exceeding_max_expr_len_is_rejected() {
        let ctx = make_trigger();
        // Construct an expression that exceeds MAX_EXPR_LEN (4096 bytes).
        let long_expr = "true || ".repeat(625); // 8 * 625 = 5000 bytes
        let err = evaluate_condition(&long_expr, &ctx, &HashMap::new())
            .await
            .unwrap_err();
        match &err {
            WorkflowError::ConditionError(msg) => {
                assert!(
                    msg.contains("exceeds") || msg.contains("limit"),
                    "expected 'exceeds' or 'limit' in error message, got: {msg}"
                );
            }
            other => panic!("expected ConditionError, got: {other:?}"),
        }
    }

    #[test]
    fn parse_duration_hours() {
        assert_eq!(parse_duration_secs("1h").unwrap(), 3600);
        assert_eq!(parse_duration_secs("2h").unwrap(), 7200);
    }

    #[test]
    fn parse_duration_minutes() {
        assert_eq!(parse_duration_secs("5m").unwrap(), 300);
        assert_eq!(parse_duration_secs("30m").unwrap(), 1800);
    }

    #[test]
    fn parse_duration_seconds() {
        assert_eq!(parse_duration_secs("10s").unwrap(), 10);
        assert_eq!(parse_duration_secs("60s").unwrap(), 60);
    }

    #[test]
    fn parse_duration_plain_number() {
        assert_eq!(parse_duration_secs("42").unwrap(), 42);
    }

    #[test]
    fn parse_duration_invalid() {
        assert!(parse_duration_secs("not-a-duration").is_err());
    }

    #[test]
    fn resolve_unclosed_template_emits_literally() {
        // An unclosed `{{` should be emitted literally without panicking.
        let ctx = make_trigger();
        let out = resolve_template("Hello {{trigger.text", &ctx, &HashMap::new()).unwrap();
        // The unclosed `{{` and remaining text are emitted as-is.
        assert!(
            out.contains("{{"),
            "unclosed {{ should appear literally in output"
        );
    }

    #[test]
    fn resolve_empty_template_string() {
        let ctx = make_trigger();
        let out = resolve_template("", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn resolve_template_with_only_literal_text() {
        let ctx = make_trigger();
        let out = resolve_template("no placeholders at all", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "no placeholders at all");
    }

    #[test]
    fn resolve_multiple_different_trigger_fields() {
        let ctx = make_trigger();
        let out = resolve_template(
            "channel={{trigger.channel_id}} ts={{trigger.timestamp}} emoji={{trigger.emoji}}",
            &ctx,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(out, "channel=channel-uuid-here ts=1700000000 emoji=fire");
    }

    #[test]
    fn resolve_trigger_message_id() {
        let ctx = make_trigger();
        let out = resolve_template("msg={{trigger.message_id}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "msg=event-id-hex");
    }

    #[test]
    fn resolve_step_output_boolean_value() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("gate".to_owned(), json!({ "approved": true }));
        let out =
            resolve_template("Approved: {{steps.gate.output.approved}}", &ctx, &outputs).unwrap();
        assert_eq!(out, "Approved: true");
    }

    #[test]
    fn resolve_step_output_number_value() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("count".to_owned(), json!({ "total": 42 }));
        let out = resolve_template("Total: {{steps.count.output.total}}", &ctx, &outputs).unwrap();
        assert_eq!(out, "Total: 42");
    }

    #[test]
    fn resolve_step_output_null_value_is_empty_string() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("step".to_owned(), json!({ "val": null }));
        let out = resolve_template("Val: {{steps.step.output.val}}", &ctx, &outputs).unwrap();
        assert_eq!(out, "Val: ");
    }

    #[test]
    fn resolve_unknown_step_id_left_literal() {
        let ctx = make_trigger();
        let out =
            resolve_template("{{steps.nonexistent.output.field}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "{{steps.nonexistent.output.field}}");
    }

    #[test]
    fn resolve_step_output_missing_field_left_literal() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("step".to_owned(), json!({ "other": "value" }));
        let out = resolve_template("{{steps.step.output.missing}}", &ctx, &outputs).unwrap();
        assert_eq!(out, "{{steps.step.output.missing}}");
    }

    #[test]
    fn resolve_truncate_zero_chars() {
        let ctx = make_trigger();
        let out =
            resolve_template("{{trigger.text | truncate(0)}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn resolve_truncate_longer_than_string() {
        let ctx = make_trigger(); // text = "P1 incident in production" (25 chars)
        let out =
            resolve_template("{{trigger.text | truncate(1000)}}", &ctx, &HashMap::new()).unwrap();
        // Truncating to more than the string length returns the full string.
        assert_eq!(out, "P1 incident in production");
    }

    #[test]
    fn resolve_pubkey_filter_non_pubkey_passes_through() {
        // Values that are not valid hex pubkeys are returned unchanged.
        let mut ctx = make_trigger();
        ctx.author = "short".to_owned();
        let out = resolve_template(
            "{{trigger.author | truncate_pubkey}}",
            &ctx,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(out, "short");
    }

    #[test]
    fn resolve_npub_filter_passes_npub_through() {
        // Already-encoded npubs are not valid hex, so they pass through intact.
        let mut ctx = make_trigger();
        ctx.author = "npub1u9l940mmrk7nv0cw6maa5fz4vztj0vj42s5dagj38zx9gtxj7qls94fpux".to_owned();
        let out = resolve_template("{{trigger.author | npub}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, ctx.author);
    }

    #[test]
    fn resolve_unknown_filter_returns_error() {
        let ctx = make_trigger();
        let err = resolve_template(
            "{{trigger.text | nonexistent_filter}}",
            &ctx,
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(matches!(err, WorkflowError::TemplateError(_)));
    }

    #[test]
    fn resolve_truncate_invalid_number_returns_error() {
        let ctx = make_trigger();
        let err = resolve_template("{{trigger.text | truncate(abc)}}", &ctx, &HashMap::new())
            .unwrap_err();
        assert!(matches!(err, WorkflowError::TemplateError(_)));
    }

    #[test]
    fn resolve_adjacent_templates_no_separator() {
        let ctx = make_trigger();
        let out =
            resolve_template("{{trigger.author}}{{trigger.emoji}}", &ctx, &HashMap::new()).unwrap();
        assert_eq!(out, "abc123def456fire");
    }

    #[tokio::test]
    async fn condition_and_expression_both_true() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result = evaluate_condition(
            "str_contains(trigger_text, \"P1\") && str_contains(trigger_text, \"production\")",
            &ctx,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_and_expression_one_false() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result = evaluate_condition(
            "str_contains(trigger_text, \"P1\") && str_contains(trigger_text, \"staging\")",
            &ctx,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn condition_not_expression() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result =
            evaluate_condition("!str_contains(trigger_text, \"P2\")", &ctx, &HashMap::new())
                .await
                .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_str_starts_with() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result = evaluate_condition(
            "str_starts_with(trigger_text, \"P1\")",
            &ctx,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_str_ends_with() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result = evaluate_condition(
            "str_ends_with(trigger_text, \"production\")",
            &ctx,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_str_len() {
        let ctx = make_trigger(); // text = "P1 incident in production" (25 chars)
        let result = evaluate_condition("str_len(trigger_text) > 10", &ctx, &HashMap::new())
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_str_len_exact() {
        let mut ctx = make_trigger();
        ctx.text = "hello".to_owned(); // exactly 5 chars
        let result = evaluate_condition("str_len(trigger_text) == 5", &ctx, &HashMap::new())
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_emoji_field() {
        let ctx = make_trigger(); // emoji = "fire"
        let result = evaluate_condition("trigger_emoji == \"fire\"", &ctx, &HashMap::new())
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_author_field() {
        let ctx = make_trigger(); // author = "abc123def456"
        let result = evaluate_condition(
            "str_starts_with(trigger_author, \"abc\")",
            &ctx,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_webhook_field_registered() {
        let mut ctx = make_trigger();
        ctx.webhook_fields
            .insert("severity".to_owned(), "critical".to_owned());
        let result = evaluate_condition("trigger_severity == \"critical\"", &ctx, &HashMap::new())
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_step_output_string_comparison() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("fetch".to_owned(), json!({ "status": "ok" }));
        let result = evaluate_condition("steps_fetch_output_status == \"ok\"", &ctx, &outputs)
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_step_output_integer_comparison() {
        let ctx = make_trigger();
        let mut outputs = HashMap::new();
        outputs.insert("count".to_owned(), json!({ "n": 5 }));
        let result = evaluate_condition("steps_count_output_n >= 5", &ctx, &outputs)
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_complex_nested_boolean() {
        let ctx = make_trigger(); // text = "P1 incident in production"
        let result = evaluate_condition(
            "(str_contains(trigger_text, \"P1\") || str_contains(trigger_text, \"P2\")) && str_contains(trigger_text, \"production\")",
            &ctx,
            &HashMap::new(),
        )
        .await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn condition_false_literal() {
        let ctx = make_trigger();
        let result = evaluate_condition("false", &ctx, &HashMap::new())
            .await
            .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn condition_true_literal() {
        let ctx = make_trigger();
        let result = evaluate_condition("true", &ctx, &HashMap::new())
            .await
            .unwrap();
        assert!(result);
    }

    #[test]
    fn trigger_context_get_field_known_fields() {
        let ctx = make_trigger();
        assert_eq!(ctx.get_field("text"), Some("P1 incident in production"));
        assert_eq!(ctx.get_field("author"), Some("abc123def456"));
        assert_eq!(ctx.get_field("channel_id"), Some("channel-uuid-here"));
        assert_eq!(ctx.get_field("timestamp"), Some("1700000000"));
        assert_eq!(ctx.get_field("emoji"), Some("fire"));
        assert_eq!(ctx.get_field("message_id"), Some("event-id-hex"));
    }

    #[test]
    fn trigger_context_get_field_unknown_returns_none() {
        let ctx = make_trigger();
        assert!(ctx.get_field("nonexistent").is_none());
        assert!(ctx.get_field("").is_none());
    }

    #[test]
    fn trigger_context_get_field_webhook_field() {
        let mut ctx = make_trigger();
        ctx.webhook_fields
            .insert("repo".to_owned(), "beekeeper".to_owned());
        assert_eq!(ctx.get_field("repo"), Some("beekeeper"));
    }

    #[test]
    fn trigger_context_default_has_empty_fields() {
        let ctx = TriggerContext::default();
        assert_eq!(ctx.text, "");
        assert_eq!(ctx.author, "");
        assert_eq!(ctx.channel_id, "");
        assert_eq!(ctx.timestamp, "");
        assert_eq!(ctx.emoji, "");
        assert_eq!(ctx.message_id, "");
        assert!(ctx.webhook_fields.is_empty());
    }

    #[test]
    fn send_message_uses_bound_workflow_channel_by_default() {
        let workflow_channel_id = Uuid::new_v4();
        let resolved = resolve_send_message_channel(None, "", Some(workflow_channel_id))
            .expect("bound channel should be used");
        assert_eq!(resolved, workflow_channel_id.to_string());
    }

    #[test]
    fn send_message_rejects_cross_channel_override_for_bound_workflow() {
        let workflow_channel_id = Uuid::new_v4();
        let other_channel_id = Uuid::new_v4();
        let err = resolve_send_message_channel(
            Some(&other_channel_id.to_string()),
            "",
            Some(workflow_channel_id),
        )
        .unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidDefinition(_)));
        assert!(
            err.to_string().contains("channel override must match"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn send_message_canonicalizes_valid_explicit_override_for_global_workflow() {
        let override_channel_id = Uuid::new_v4();
        let resolved =
            resolve_send_message_channel(Some(&override_channel_id.to_string()), "", None)
                .expect("override should be accepted");
        assert_eq!(resolved, override_channel_id.to_string());
    }

    fn record_ci_result_template_step() -> Step {
        Step {
            id: "record".into(),
            name: None,
            if_expr: None,
            timeout_secs: None,
            action: ActionDef::RecordCiResult {
                project: "literal-project".into(),
                repository: "literal-repository".into(),
                check: "literal-check".into(),
                phase: beekeeper_core::ci_result::CiPhase::Deploy,
                commit: "{{trigger.commit}}".into(),
                run: "{{trigger.run}}".into(),
                attempt: "{{trigger.attempt}}".into(),
                conclusion: "{{trigger.conclusion}}".into(),
                evidence_url: Some("{{trigger.url}}".into()),
                summary: Some("{{trigger.summary}}".into()),
            },
        }
    }

    fn valid_ci_trigger_fields() -> TriggerContext {
        let mut trigger = TriggerContext::default();
        for (key, value) in [
            ("commit", "a".repeat(40)),
            ("run", "42".into()),
            ("attempt", "2".into()),
            ("conclusion", "success".into()),
            ("url", "https://ci.example/run/42".into()),
            ("summary", "passed".into()),
        ] {
            trigger.webhook_fields.insert(key.into(), value);
        }
        trigger
    }

    #[test]
    fn record_ci_result_templates_only_callback_owned_fields() {
        let step = record_ci_result_template_step();
        let trigger = valid_ci_trigger_fields();
        let resolved = resolve_step_templates(&step, &trigger, &HashMap::new())
            .expect("templates should resolve");
        assert!(matches!(
            resolved,
            ActionDef::RecordCiResult {
                project,
                repository,
                check,
                phase: beekeeper_core::ci_result::CiPhase::Deploy,
                commit,
                attempt,
                conclusion,
                ..
            } if project == "literal-project"
                && repository == "literal-repository"
                && check == "literal-check"
                && commit == "a".repeat(40)
                && attempt == "2"
                && conclusion == "success"
        ));
    }

    #[test]
    fn record_ci_result_rejects_missing_required_template_field() {
        let step = record_ci_result_template_step();
        let mut trigger = valid_ci_trigger_fields();
        trigger.webhook_fields.remove("run");
        let error = resolve_step_templates(&step, &trigger, &HashMap::new())
            .expect_err("missing trigger.run must fail before the action sink");
        assert!(matches!(error, WorkflowError::TemplateError(_)));
        assert!(error.to_string().contains("record_ci_result run"));
    }

    #[test]
    fn record_ci_result_rejects_missing_optional_template_fields_when_configured() {
        let step = record_ci_result_template_step();
        let mut trigger = valid_ci_trigger_fields();
        trigger.webhook_fields.remove("url");
        trigger.webhook_fields.remove("summary");
        let evidence_error = resolve_step_templates(&step, &trigger, &HashMap::new())
            .expect_err("configured evidence template must resolve");
        assert!(evidence_error
            .to_string()
            .contains("record_ci_result evidence_url"));

        trigger
            .webhook_fields
            .insert("url".into(), "https://ci.example/run/42".into());
        let summary_error = resolve_step_templates(&step, &trigger, &HashMap::new())
            .expect_err("configured summary template must resolve");
        assert!(summary_error
            .to_string()
            .contains("record_ci_result summary"));
    }

    #[test]
    fn record_ci_result_preserves_literal_braces_from_callback_value() {
        let step = record_ci_result_template_step();
        let mut trigger = valid_ci_trigger_fields();
        let compiler_output = "compiler preserved {{template_source}} literally";
        trigger
            .webhook_fields
            .insert("summary".into(), compiler_output.into());
        let resolved = resolve_step_templates(&step, &trigger, &HashMap::new())
            .expect("callback brace text is data, not a recursive template");
        assert!(matches!(
            resolved,
            ActionDef::RecordCiResult {
                summary: Some(summary),
                ..
            } if summary == compiler_output
        ));
    }
}
