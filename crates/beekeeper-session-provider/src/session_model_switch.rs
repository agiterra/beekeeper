//! The actor half of SV-35: applying a model selection to a live adapter
//! session, at open and at a turn boundary (`thread.model.set`).
//!
//! Open and switch share one code path, [`apply_model`], which works against a
//! [`ModelControls`] snapshot rather than the `session/new` response itself:
//! the snapshot starts as that response and is replaced by every set response
//! that carries `configOptions`, so a later switch is judged against what the
//! adapter now offers (effort values are per model) and A → B → A works.
//!
//! What this module reports is evidence, never the request: a switch is
//! `Applied` only when the adapter answered the set call without an error, and
//! the applied value prefers the adapter's own fresh report of its model.

use std::collections::VecDeque;

use beekeeper_acp::acp::{AcpClient, ModelSwitchMethod};
use tokio::sync::mpsc;

use super::{SessionActor, SessionCommand, SessionEvent, SESSION_QUEUE_DEPTH};
use crate::model_switch::ModelSwitchOutcome;
use crate::payload::{
    MODEL_NOT_OFFERED, MODEL_SWITCH_FAILED, MODEL_SWITCH_UNSUPPORTED, QUEUE_FULL,
};

/// The latest model-control facts the adapter has given this execution.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelControls {
    /// The `session/new` result, with `configOptions` (and
    /// `models.currentModelId`) replaced by the adapter's later answers.
    raw: serde_json::Value,
    /// The mode a host boundary requires this runtime to stay in (Codex's
    /// bounded mode), re-asserted after every switch; `None` when nothing is
    /// required.
    bounded_mode: Option<&'static str>,
}

impl ModelControls {
    /// A snapshot of one `session/new` result.
    pub(crate) fn new(raw: serde_json::Value, bounded_mode: Option<&'static str>) -> Self {
        Self { raw, bounded_mode }
    }

    /// Whether the adapter offered any model control at `session/new`.
    pub(crate) fn offered(&self) -> bool {
        crate::model_switch::offers_model_control(&self.raw)
    }

    /// Fold a set response into the snapshot. Only a response that carries
    /// `configOptions` replaces them; anything else leaves the snapshot alone.
    pub(crate) fn absorb(&mut self, fresh: &serde_json::Value, applied: Option<&str>) {
        if beekeeper_acp::model_options::has_config_options(fresh) {
            self.raw["configOptions"] = fresh["configOptions"].clone();
        }
        if let (Some(models), Some(applied)) = (self.raw.get_mut("models"), applied) {
            if models.is_object() {
                models["currentModelId"] = serde_json::Value::String(applied.to_owned());
            }
        }
    }
}

/// What one application of a selection established.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ApplyOutcome {
    /// The adapter accepted it. `applied` is what was applied (a modifier the
    /// model does not offer is left out); `fresh` is the adapter's latest
    /// answer carrying `configOptions`, when any did.
    Applied {
        applied: String,
        fresh: Option<serde_json::Value>,
    },
    /// The adapter offers no control matching the selection's model.
    NotOffered,
    /// The adapter answered the set call with an error.
    Failed(String),
}

/// Ask the adapter to use `desired`, judged against `raw`.
///
/// A model the adapter does not offer is reported, never guessed at: at open
/// that means the session runs on the adapter's own default and metadata says
/// so; at a switch it is a refusal.
pub(super) async fn apply_model(
    client: &mut AcpClient,
    session_id: &str,
    raw: &serde_json::Value,
    desired: &str,
) -> ApplyOutcome {
    let method = beekeeper_acp::acp::resolve_model_switch_method(raw, desired);
    if method.is_none() {
        if let Some(plan) = beekeeper_acp::model_options::resolve_model_selection(raw, desired) {
            return apply_model_selection(client, session_id, desired, plan).await;
        }
    }
    let outcome = match method {
        Some(ModelSwitchMethod::ConfigOption {
            config_id,
            option_value,
        }) => {
            client
                .session_set_config_option(session_id, &config_id, &option_value)
                .await
        }
        Some(ModelSwitchMethod::SetModel { model_id }) => {
            client.session_set_model(session_id, &model_id).await
        }
        None => {
            // Which values it does offer, so a mismatch (a full model id
            // asked of an adapter that offers aliases) is readable from the
            // log without spending a turn.
            let offered: Vec<String> = beekeeper_acp::acp::extract_model_config_options(raw)
                .iter()
                .flat_map(|option| option["options"].as_array().cloned().unwrap_or_default())
                .chain(
                    beekeeper_acp::acp::extract_model_state(raw)
                        .and_then(|models| models["availableModels"].as_array().cloned())
                        .unwrap_or_default(),
                )
                .filter_map(|entry| {
                    entry["value"]
                        .as_str()
                        .or_else(|| entry["modelId"].as_str())
                        .map(str::to_owned)
                })
                .collect();
            tracing::warn!(
                target: "csp::session",
                offered = ?offered,
                "agent does not offer model {desired}"
            );
            return ApplyOutcome::NotOffered;
        }
    };
    match outcome {
        Ok(fresh) => ApplyOutcome::Applied {
            applied: desired.to_owned(),
            fresh: beekeeper_acp::model_options::has_config_options(&fresh).then_some(fresh),
        },
        Err(error) => {
            tracing::warn!(target: "csp::session", "model switch to {desired} failed: {error}");
            ApplyOutcome::Failed(error.to_string())
        }
    }
}

/// Apply a selection the adapter does not advertise whole: its advertised
/// base model, then each modifier against the controls the adapter returns
/// *for that model*.
///
/// `[fast]` turns the fast-mode switch on; any other token is applied only
/// when it is one of that model's reasoning-effort values. A token that is
/// neither is not guessed at — it is logged with what was on offer, exactly
/// as an unofferable model is — and left out of the returned selection, so
/// what the session records is what was applied (`opus[1m][fast]` for a
/// request of `opus[1m][ultra][fast]` on a model without `ultra`).
async fn apply_model_selection(
    client: &mut AcpClient,
    session_id: &str,
    desired: &str,
    plan: beekeeper_acp::model_options::ModelSelectionPlan,
) -> ApplyOutcome {
    use beekeeper_acp::model_options::{
        config_option_id, config_option_values, extract_fast_mode_option,
        extract_thought_level_option, fast_mode_on_value, FAST_MODIFIER,
    };
    let mut controls = match client
        .session_set_config_option(session_id, &plan.config_id, &plan.base)
        .await
    {
        Ok(controls) => controls,
        Err(error) => {
            tracing::warn!(
                target: "csp::session",
                "model switch to {} (for {desired}) failed: {error}",
                plan.base
            );
            return ApplyOutcome::Failed(error.to_string());
        }
    };
    let mut applied = plan.base.clone();
    for token in &plan.modifiers {
        let (option, value) = if token == FAST_MODIFIER {
            let option = extract_fast_mode_option(&controls);
            (option, option.and_then(fast_mode_on_value))
        } else {
            let option = extract_thought_level_option(&controls);
            let offered = option.is_some_and(|option| {
                config_option_values(option)
                    .iter()
                    .any(|value| value == token)
            });
            (
                option,
                offered.then(|| serde_json::Value::String(token.clone())),
            )
        };
        let target = option.and_then(|option| config_option_id(option).map(str::to_owned));
        let (Some(config_id), Some(value)) = (target, value) else {
            let offered: Vec<String> = extract_thought_level_option(&controls)
                .map(config_option_values)
                .unwrap_or_default();
            tracing::warn!(
                target: "csp::session",
                offered_efforts = ?offered,
                fast_mode = extract_fast_mode_option(&controls).is_some(),
                "agent does not offer [{token}] with model {} — not applied",
                plan.base
            );
            continue;
        };
        match client
            .session_set_config_value(session_id, &config_id, value)
            .await
        {
            Ok(fresh) => {
                applied.push_str(&format!("[{token}]"));
                // A later token is judged against what the adapter now says.
                if beekeeper_acp::model_options::has_config_options(&fresh) {
                    controls = fresh;
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "csp::session",
                    "setting [{token}] on model {} failed: {error}",
                    plan.base
                );
            }
        }
    }
    ApplyOutcome::Applied {
        applied,
        fresh: beekeeper_acp::model_options::has_config_options(&controls).then_some(controls),
    }
}

/// The open-time application: the applied selection, or `None` when nothing
/// was applied, with the snapshot updated from the adapter's answer.
pub(super) async fn apply_model_at_open(
    client: &mut AcpClient,
    session_id: &str,
    controls: &mut ModelControls,
    desired: Option<&str>,
) -> Option<String> {
    let desired = desired?;
    match apply_model(client, session_id, &controls.raw, desired).await {
        ApplyOutcome::Applied { applied, fresh } => {
            if let Some(fresh) = fresh {
                controls.absorb(&fresh, Some(&applied));
            }
            Some(applied)
        }
        ApplyOutcome::NotOffered | ApplyOutcome::Failed(_) => None,
    }
}

/// Put the session in `mode`, if the adapter offers it (an adapter that lists
/// no modes is asked anyway, as at open). `Err` carries the detail.
pub(super) async fn assert_mode(
    client: &mut AcpClient,
    session_id: &str,
    raw: &serde_json::Value,
    mode: &str,
) -> Result<(), String> {
    let offered = raw
        .pointer("/modes/availableModes")
        .and_then(serde_json::Value::as_array)
        .is_none_or(|modes| modes.iter().any(|m| m["id"] == mode));
    if !offered {
        return Err("the adapter does not offer it".to_owned());
    }
    client
        .session_set_mode(session_id, mode)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The model value a switch publishes: what was applied, unless the adapter's
/// fresh report names a different base model, which then wins — publishing a
/// request the adapter contradicts is the "default label hiding the real
/// model" bug. A report that is the applied selection's base (`opus[1m]` for
/// `opus[1m][high]`) agrees with it.
pub(crate) fn reconcile_applied(applied: String, reported: Option<String>) -> String {
    match reported {
        Some(reported) if reported != applied && !applied.starts_with(&format!("{reported}[")) => {
            reported
        }
        _ => applied,
    }
}

/// Queue a switch that arrived while a turn runs, behind everything already
/// queued, or report it dropped when the queue is full.
pub(super) async fn queue_model_switch(
    events: &mpsc::Sender<SessionEvent>,
    session_id: &str,
    queued: &mut VecDeque<SessionCommand>,
    command: SessionCommand,
) {
    if queued.len() < SESSION_QUEUE_DEPTH {
        queued.push_back(command);
        return;
    }
    if let SessionCommand::SetModel { command_id, .. } = command {
        let _ = events
            .send(SessionEvent::ModelSwitch {
                session_id: session_id.to_owned(),
                command_id,
                outcome: ModelSwitchOutcome::Dropped {
                    code: QUEUE_FULL,
                    message: "the execution's queue is full, so the model switch was not \
                              delivered; it keeps its model"
                        .to_owned(),
                },
            })
            .await;
    }
}

impl SessionActor {
    /// Apply one `thread.model.set` at the boundary and report it. Returns
    /// `true` when the execution must end: the switch went through but the
    /// mode the host boundary requires could not be re-asserted.
    pub(super) async fn apply_model_switch(
        &mut self,
        command_id: String,
        selection: String,
    ) -> bool {
        let (outcome, fatal) = self.switch_model(&selection).await;
        let _ = self
            .events
            .send(SessionEvent::ModelSwitch {
                session_id: self.session_id.clone(),
                command_id,
                outcome,
            })
            .await;
        fatal
    }

    async fn switch_model(&mut self, selection: &str) -> (ModelSwitchOutcome, bool) {
        switch_on(
            &mut self.client,
            &self.acp_session_id,
            &mut self.model_controls,
            selection,
        )
        .await
    }
}

/// Apply `selection` to a live adapter session against `controls`, updating
/// the snapshot from the adapter's answer. The `bool` is `true` when the
/// execution must end (the required mode could not be re-asserted).
pub(super) async fn switch_on(
    client: &mut AcpClient,
    session_id: &str,
    controls: &mut ModelControls,
    selection: &str,
) -> (ModelSwitchOutcome, bool) {
    let refused = |code: &'static str, message: String| {
        (ModelSwitchOutcome::Refused { code, message }, false)
    };
    if !controls.offered() {
        return refused(
            MODEL_SWITCH_UNSUPPORTED,
            "this execution's adapter offered no model control when it opened, so it cannot \
             change models mid-session"
                .into(),
        );
    }
    let raw = controls.raw.clone();
    let (applied, fresh) = match apply_model(client, session_id, &raw, selection).await {
        ApplyOutcome::Applied { applied, fresh } => (applied, fresh),
        ApplyOutcome::NotOffered => {
            return refused(
                MODEL_NOT_OFFERED,
                format!(
                    "the adapter behind this execution does not offer {selection}; it keeps its \
                     model"
                ),
            )
        }
        ApplyOutcome::Failed(detail) => {
            return refused(
                MODEL_SWITCH_FAILED,
                format!(
                    "the adapter refused the switch to {selection} ({detail}); the execution \
                     keeps its previous model"
                ),
            )
        }
    };
    let reported = fresh.as_ref().and_then(beekeeper_acp::acp::reported_model);
    let applied = reconcile_applied(applied, reported);
    if let Some(fresh) = &fresh {
        controls.absorb(fresh, Some(&applied));
    }
    // A switch must never quietly widen the mode the host boundary requires
    // (Codex re-reports its default mode after a config change).
    if let Some(mode) = controls.bounded_mode {
        let raw = controls.raw.clone();
        if let Err(detail) = assert_mode(client, session_id, &raw, mode).await {
            return (
                ModelSwitchOutcome::Refused {
                    code: MODEL_SWITCH_FAILED,
                    message: format!(
                        "the switch to {selection} left the session outside the {mode} mode the \
                         project boundary requires ({detail}); the execution was shut down"
                    ),
                },
                true,
            );
        }
    }
    (
        ModelSwitchOutcome::Applied {
            requested: selection.to_owned(),
            applied,
        },
        false,
    )
}

#[cfg(test)]
#[path = "session_model_switch_tests.rs"]
mod tests;
