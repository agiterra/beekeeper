//! Live model discovery for ACP adapters that opt in via `discoverModels`.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use beekeeper_acp::acp::{extract_model_config_options, extract_model_state, AcpClient};
use beekeeper_acp::model_options::{
    config_option_id, config_option_values, extract_fast_mode_option, extract_thought_level_option,
    has_config_options,
};

use crate::config::{Config, ModelControls, ModelDetail};

/// Budget for spawning the adapter and reading its `session/new` catalog.
///
/// Not 10 s: the first run of a new Codex CLI in the private `CODEX_HOME`
/// migrates its state databases and refetches its model list before it
/// answers, and on 2026-10-01 that outlasted 10 s, so the provider published
/// no Codex models at all. Discovery runs once per provider start; a slow
/// adapter costs that start up to this long, a healthy one ~2 s.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
/// Separate budget for switching the discovery session through each model to
/// read its effort values and fast-mode switch. Probing every Claude model
/// measured ~10 s; a model not reached before this runs out simply publishes
/// no controls. It never costs the base model list.
const PER_MODEL_PROBE_BUDGET: Duration = Duration::from_secs(30);
/// Ceiling on the adapter's shutdown after discovery; past it the client is
/// dropped, which kills the process group.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveredModels {
    default_model: String,
    allowed_models: Vec<String>,
    /// Keyed by model option value; ids only the unstable surface named
    /// (Codex's bracketed `gpt-5.6-sol[high]`) get no entry.
    details: BTreeMap<String, ModelDetail>,
}

/// The model option the per-model probe switches through: its config id and
/// its values in the adapter's order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeTarget {
    config_id: String,
    current: Option<String>,
    values: Vec<String>,
}

/// Replace fallback model data with each adapter's current ACP catalog.
///
/// Runs sequentially per opted-in descriptor, best effort with a per-descriptor
/// timeout: an unavailable or signed-out adapter must not prevent the provider
/// from starting. A later session still asks the fresh ACP response whether a
/// requested model can be applied.
pub(crate) async fn discover(config: &mut Config) {
    for descriptor in config
        .runtimes
        .iter_mut()
        .filter(|descriptor| descriptor.discover_models)
    {
        let cli_env: Vec<(String, String)> = descriptor
            .cli_env
            .iter()
            .map(|env| (env.name.clone(), env.value.clone()))
            .collect();
        let plan = discovery_plan(
            &config.state_dir,
            config.runtime_profile_override,
            descriptor,
            &cli_env,
        );
        let plan = match plan {
            Ok(plan) => plan,
            Err(failure) => {
                tracing::warn!(
                    target: "csp::models",
                    "{} model discovery was not run: {}",
                    descriptor.runtime,
                    failure.message
                );
                continue;
            }
        };
        let runtime = descriptor.runtime.as_str();
        let discovered = match probe(
            &descriptor.agent_command,
            &descriptor.agent_args,
            &cli_env,
            &plan,
            PER_MODEL_PROBE_BUDGET,
        )
        .await
        {
            Ok(discovered) => discovered,
            Err(error) => {
                tracing::warn!(target: "csp::models", "{runtime} model discovery failed: {error}");
                continue;
            }
        };

        descriptor.default_model = discovered.default_model;
        descriptor.allowed_models = discovered.allowed_models;
        config
            .model_details
            .insert(descriptor.instance_ref.clone(), discovered.details);
    }
}

/// The scope a runtime's model discovery runs in: an empty host-owned
/// working directory and the runtime's private state, inside the boundary
/// where one exists — so discovery opens no conversation in the provider's
/// own directory or in the operator's runtime history.
fn discovery_plan(
    state_dir: &std::path::Path,
    runtime_override: Option<crate::execution_scope::RuntimeProfile>,
    descriptor: &beekeeper_core::coding_session_runtime::RuntimeDescriptor,
    cli_env: &[(String, String)],
) -> Result<crate::execution_scope::ExecutionPlan, crate::session::CreateFailure> {
    let slug: String = descriptor
        .instance_ref
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cwd = state_dir
        .join(crate::execution_scope::DISCOVERY_DIR)
        .join(&slug);
    std::fs::create_dir_all(&cwd).map_err(|error| crate::session::CreateFailure {
        code: crate::execution_scope::EXECUTION_BOUNDARY_UNAVAILABLE,
        message: format!("could not create the discovery directory: {error}"),
    })?;
    let session_id = format!("discovery-{slug}");
    let mut inputs = crate::execution_scope::ScopeInputs::new(
        crate::execution_scope::ScopePurpose::Discovery,
        state_dir,
        &session_id,
        &cwd,
    );
    inputs.driver = &descriptor.driver;
    inputs.runtime = runtime_override
        .unwrap_or_else(|| crate::execution_scope::RuntimeProfile::for_driver(&descriptor.driver));
    inputs.agent_command = &descriptor.agent_command;
    inputs.agent_args = &descriptor.agent_args;
    inputs.agent_env = cli_env;
    crate::execution_scope::prepare(&inputs)
}

async fn probe(
    agent_command: &str,
    agent_args: &[String],
    cli_env: &[(String, String)],
    plan: &crate::execution_scope::ExecutionPlan,
    per_model_budget: Duration,
) -> anyhow::Result<DiscoveredModels> {
    let opened = tokio::time::timeout(
        DISCOVERY_TIMEOUT,
        open(agent_command, agent_args, cli_env, plan),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out after {DISCOVERY_TIMEOUT:?}"))??;
    let (mut client, response) = opened;
    let result = match models_from_session_new(&response.raw) {
        Some(mut discovered) => {
            if let Some(target) = probe_target(&response.raw) {
                let mut switcher = ClientSwitcher {
                    client: &mut client,
                    session_id: &response.session_id,
                    config_id: &target.config_id,
                };
                probe_controls(
                    &mut switcher,
                    &target,
                    &response.raw,
                    &mut discovered.details,
                    per_model_budget,
                )
                .await;
            }
            Ok(discovered)
        }
        None => Err(anyhow::anyhow!("adapter returned no selectable models")),
    };
    // Bounded: a wedged adapter must not hold provider startup. Dropping the
    // client past the grace kills its process group.
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, client.shutdown()).await;
    result
}

/// Spawn the adapter, initialize it and open the discovery session. On a
/// failure after the spawn the client is shut down here.
async fn open(
    agent_command: &str,
    agent_args: &[String],
    cli_env: &[(String, String)],
    plan: &crate::execution_scope::ExecutionPlan,
) -> anyhow::Result<(AcpClient, beekeeper_acp::acp::SessionNewResponse)> {
    // Fenced for the same reason a session spawn is: this is the same adapter
    // binary, started by the same sidecar, and a startup probe is no more
    // entitled to the provider's signing key than a session is. Where a
    // boundary exists it runs inside one, in an empty host-owned directory.
    let (mut client, cwd) = match plan {
        crate::execution_scope::ExecutionPlan::Prepared(prepared) => {
            let mut client =
                AcpClient::spawn_bounded(agent_command, agent_args, &prepared.launch).await?;
            if let Some(options) = &prepared.claude_options {
                client.set_claude_options(options.clone());
            }
            (client, prepared.launch.cwd().to_string_lossy().into_owned())
        }
        crate::execution_scope::ExecutionPlan::Legacy { .. } => {
            let client = AcpClient::spawn_with_env_fence(
                agent_command,
                agent_args,
                cli_env,
                false,
                &crate::agent_fence::FENCE,
            )
            .await?;
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("/"))
                .to_string_lossy()
                .into_owned();
            (client, cwd)
        }
    };
    let opened = async {
        client.initialize().await?;
        Ok::<_, anyhow::Error>(client.session_new_full(&cwd, vec![], None, None).await?)
    }
    .await;
    match opened {
        Ok(response) => Ok((client, response)),
        Err(error) => {
            client.shutdown().await;
            Err(error)
        }
    }
}

/// Switches the discovery session to one model and returns the adapter's
/// fresh response. A seam so the budget logic is testable without a process.
trait ModelSwitcher {
    async fn switch(&mut self, model: &str) -> anyhow::Result<serde_json::Value>;
}

struct ClientSwitcher<'a> {
    client: &'a mut AcpClient,
    session_id: &'a str,
    config_id: &'a str,
}

impl ModelSwitcher for ClientSwitcher<'_> {
    async fn switch(&mut self, model: &str) -> anyhow::Result<serde_json::Value> {
        Ok(self
            .client
            .session_set_config_option(self.session_id, self.config_id, model)
            .await?)
    }
}

/// The first model option with an id: the one the probe switches through.
fn probe_target(raw: &serde_json::Value) -> Option<ProbeTarget> {
    extract_model_config_options(raw).iter().find_map(|option| {
        let config_id = config_option_id(option)?.to_owned();
        let values = config_option_values(option);
        (!values.is_empty()).then(|| ProbeTarget {
            config_id,
            current: option
                .get("currentValue")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            values,
        })
    })
}

/// The controls a response describes for the model it has selected, or
/// `None` when it carries no `configOptions` — nothing was said, so nothing
/// is recorded.
fn controls_from(response: &serde_json::Value) -> Option<ModelControls> {
    if !has_config_options(response) {
        return None;
    }
    Some(ModelControls {
        efforts: extract_thought_level_option(response)
            .map(config_option_values)
            .unwrap_or_default(),
        fast_mode: extract_fast_mode_option(response).is_some(),
    })
}

/// Switch through every model option value within `budget`, recording each
/// model's effort values and fast-mode switch.
///
/// The currently selected model is read from `session/new` itself, without a
/// switch. Partial results stand: a model not reached before the deadline, or
/// one whose switch the adapter refused, keeps `controls: None`.
async fn probe_controls(
    switcher: &mut impl ModelSwitcher,
    target: &ProbeTarget,
    session_new: &serde_json::Value,
    details: &mut BTreeMap<String, ModelDetail>,
    budget: Duration,
) {
    let deadline = tokio::time::Instant::now() + budget;
    if let Some(current) = target.current.as_deref() {
        if let (Some(detail), Some(controls)) =
            (details.get_mut(current), controls_from(session_new))
        {
            detail.controls = Some(controls);
        }
    }
    let mut probed = 0usize;
    for value in &target.values {
        if target.current.as_deref() == Some(value.as_str()) {
            continue;
        }
        match tokio::time::timeout_at(deadline, switcher.switch(value)).await {
            Err(_) => {
                tracing::info!(
                    target: "csp::models",
                    probed,
                    total = target.values.len(),
                    "per-model probe budget of {budget:?} spent; the remaining models publish no controls"
                );
                return;
            }
            Ok(Err(error)) => {
                tracing::debug!(target: "csp::models", "could not probe model {value}: {error}");
            }
            Ok(Ok(response)) => {
                if let (Some(detail), Some(controls)) =
                    (details.get_mut(value), controls_from(&response))
                {
                    detail.controls = Some(controls);
                }
            }
        }
        probed += 1;
    }
}

fn models_from_session_new(raw: &serde_json::Value) -> Option<DiscoveredModels> {
    let mut model_ids = BTreeSet::new();
    let mut details: BTreeMap<String, ModelDetail> = BTreeMap::new();
    let mut stable_current = None;

    for option in extract_model_config_options(raw) {
        if stable_current.is_none() {
            stable_current = option
                .get("currentValue")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
        }
        if let Some(options) = option.get("options").and_then(serde_json::Value::as_array) {
            for model in options {
                if let Some(id) = model
                    .get("value")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    model_ids.insert(id.to_owned());
                    if !details.contains_key(id) {
                        let text = |key: &str| {
                            model
                                .get(key)
                                .and_then(serde_json::Value::as_str)
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .map(str::to_owned)
                        };
                        details.insert(
                            id.to_owned(),
                            ModelDetail {
                                name: text("name"),
                                description: text("description"),
                                rank: u32::try_from(details.len()).ok(),
                                controls: None,
                            },
                        );
                    }
                }
            }
        }
    }

    let unstable = extract_model_state(raw);
    let unstable_current = unstable
        .as_ref()
        .and_then(|models| models.get("currentModelId"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if let Some(available) = unstable
        .as_ref()
        .and_then(|models| models.get("availableModels"))
        .and_then(serde_json::Value::as_array)
    {
        for model in available {
            if let Some(id) = model
                .get("modelId")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                model_ids.insert(id.to_owned());
            }
        }
    }

    let default_model = stable_current
        .or(unstable_current)
        .filter(|model| model_ids.contains(model))
        .or_else(|| model_ids.first().cloned())?;
    model_ids.remove(&default_model);
    let mut allowed_models = Vec::with_capacity(model_ids.len() + 1);
    allowed_models.push(default_model.clone());
    allowed_models.extend(model_ids);
    Some(DiscoveredModels {
        default_model,
        allowed_models,
        details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_stable_options_become_the_full_catalog() {
        let raw = serde_json::json!({
            "configOptions": [{
                "category": "model",
                "currentValue": "claude-fable-5[1m]",
                "id": "model",
                "options": [
                    { "value": "default" },
                    { "value": "opus[1m]" },
                    { "value": "claude-fable-5[1m]" },
                    { "value": "sonnet" },
                    { "value": "haiku" }
                ]
            }]
        });

        let catalog = models_from_session_new(&raw).expect("model catalog");
        assert_eq!(catalog.default_model, "claude-fable-5[1m]");
        assert_eq!(
            catalog.allowed_models,
            vec![
                "claude-fable-5[1m]",
                "default",
                "haiku",
                "opus[1m]",
                "sonnet"
            ]
        );
    }

    #[test]
    fn unstable_models_are_used_when_stable_options_are_absent() {
        let raw = serde_json::json!({
            "models": {
                "currentModelId": "opus",
                "availableModels": [
                    { "modelId": "sonnet" },
                    { "modelId": "opus" }
                ]
            }
        });

        let catalog = models_from_session_new(&raw).expect("model catalog");
        assert_eq!(catalog.default_model, "opus");
        assert_eq!(catalog.allowed_models, vec!["opus", "sonnet"]);
    }

    #[test]
    fn an_empty_response_has_no_catalog() {
        assert_eq!(models_from_session_new(&serde_json::json!({})), None);
    }

    fn claude_session_new() -> serde_json::Value {
        serde_json::json!({
            "sessionId": "discovery",
            "configOptions": [
                {"id": "model", "category": "model", "type": "select", "currentValue": "default",
                 "options": [
                    {"value": "default", "name": "Default (recommended)", "description": "Opus 5.5 with 1M context"},
                    {"value": "opus[1m]", "name": "Opus 5.5", "description": "  "},
                    {"value": "haiku", "name": "Haiku 4.5", "description": "Fastest for quick answers"},
                    {"value": "sonnet"}
                 ]},
                {"id": "effort", "category": "thought_level", "type": "select", "currentValue": "default",
                 "options": [{"value": "default"}, {"value": "low"}, {"value": "high"}, {"value": "xhigh"}]},
                {"id": "fast", "category": "model_config", "type": "select", "currentValue": "off",
                 "options": [{"value": "on"}, {"value": "off"}]}
            ]
        })
    }

    /// The runtime's own words and order ride along with the ids, and the
    /// allowed list is exactly what it was before names existed.
    #[test]
    fn names_descriptions_and_rank_are_kept_from_session_new() {
        let discovered = models_from_session_new(&claude_session_new()).expect("catalog");
        assert_eq!(
            discovered.allowed_models,
            vec!["default", "haiku", "opus[1m]", "sonnet"]
        );
        let default = &discovered.details["default"];
        assert_eq!(default.name.as_deref(), Some("Default (recommended)"));
        assert_eq!(
            default.description.as_deref(),
            Some("Opus 5.5 with 1M context")
        );
        assert_eq!(default.rank, Some(0));
        let opus = &discovered.details["opus[1m]"];
        assert_eq!(opus.name.as_deref(), Some("Opus 5.5"));
        assert_eq!(
            opus.description, None,
            "a blank description is no description"
        );
        assert_eq!(opus.rank, Some(1));
        assert_eq!(discovered.details["haiku"].rank, Some(2));
        let sonnet = &discovered.details["sonnet"];
        assert_eq!((sonnet.name.as_ref(), sonnet.rank), (None, Some(3)));
        // Nothing is measured until the probe runs.
        assert!(discovered.details.values().all(|d| d.controls.is_none()));
    }

    /// Codex's unstable bracketed ids stay in the offer but get no detail row:
    /// the adapter described only its model option values.
    #[test]
    fn unstable_bracketed_ids_are_offered_but_not_described() {
        let raw = serde_json::json!({
            "configOptions": [{"id": "model", "category": "model", "currentValue": "gpt-5.6-sol",
                "options": [{"value": "gpt-5.6-sol", "name": "GPT-5.6 Sol"}]}],
            "models": {"currentModelId": "gpt-5.6-sol[high]",
                "availableModels": [{"modelId": "gpt-5.6-sol[high]"}, {"modelId": "gpt-5.6-sol[low]"}]}
        });
        let discovered = models_from_session_new(&raw).expect("catalog");
        assert_eq!(
            discovered.allowed_models,
            vec!["gpt-5.6-sol", "gpt-5.6-sol[high]", "gpt-5.6-sol[low]"]
        );
        assert_eq!(
            discovered.details.keys().collect::<Vec<_>>(),
            vec!["gpt-5.6-sol"]
        );
    }

    /// Answers each switch from a table; a model listed in `hang` never
    /// answers, which is how a spent budget looks from here.
    struct FakeSwitcher {
        responses: BTreeMap<&'static str, serde_json::Value>,
        hang: &'static [&'static str],
        asked: Vec<String>,
    }

    impl ModelSwitcher for FakeSwitcher {
        async fn switch(&mut self, model: &str) -> anyhow::Result<serde_json::Value> {
            self.asked.push(model.to_owned());
            if self.hang.contains(&model) {
                std::future::pending::<()>().await;
            }
            self.responses
                .get(model)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Invalid value for config option model"))
        }
    }

    fn controls(efforts: &[&str], fast_mode: bool) -> Option<ModelControls> {
        Some(ModelControls {
            efforts: efforts.iter().map(|e| (*e).to_owned()).collect(),
            fast_mode,
        })
    }

    #[tokio::test]
    async fn each_model_records_its_own_efforts_and_fast_mode() {
        let raw = claude_session_new();
        let mut discovered = models_from_session_new(&raw).expect("catalog");
        let target = probe_target(&raw).expect("target");
        assert_eq!(target.config_id, "model");
        let mut switcher = FakeSwitcher {
            responses: BTreeMap::from([
                (
                    "opus[1m]",
                    serde_json::json!({"configOptions": [
                        {"id": "effort", "category": "thought_level",
                         "options": [{"value": "default"}, {"value": "low"}, {"value": "max"}]},
                        {"id": "fast", "category": "model_config", "type": "boolean", "currentValue": false}
                    ]}),
                ),
                // Haiku: the adapter offers no effort control and no fast mode.
                (
                    "haiku",
                    serde_json::json!({"configOptions": [{"id": "model", "category": "model"}]}),
                ),
            ]),
            hang: &[],
            asked: Vec::new(),
        };
        probe_controls(
            &mut switcher,
            &target,
            &raw,
            &mut discovered.details,
            Duration::from_secs(5),
        )
        .await;
        // The current model is read from session/new, never switched to.
        assert_eq!(switcher.asked, vec!["opus[1m]", "haiku", "sonnet"]);
        assert_eq!(
            discovered.details["default"].controls,
            controls(&["default", "low", "high", "xhigh"], true)
        );
        assert_eq!(
            discovered.details["opus[1m]"].controls,
            controls(&["default", "low", "max"], true)
        );
        assert_eq!(discovered.details["haiku"].controls, controls(&[], false));
        // A refused switch is unknown, not "no controls".
        assert_eq!(discovered.details["sonnet"].controls, None);
    }

    #[tokio::test]
    async fn a_spent_budget_keeps_what_was_measured_and_the_model_list() {
        let raw = claude_session_new();
        let mut discovered = models_from_session_new(&raw).expect("catalog");
        let target = probe_target(&raw).expect("target");
        let mut switcher = FakeSwitcher {
            responses: BTreeMap::from([(
                "opus[1m]",
                serde_json::json!({"configOptions": [
                    {"id": "effort", "category": "thought_level", "options": [{"value": "low"}]}
                ]}),
            )]),
            hang: &["haiku"],
            asked: Vec::new(),
        };
        probe_controls(
            &mut switcher,
            &target,
            &raw,
            &mut discovered.details,
            Duration::from_millis(100),
        )
        .await;
        assert_eq!(switcher.asked, vec!["opus[1m]", "haiku"]);
        assert_eq!(
            discovered.details["opus[1m]"].controls,
            controls(&["low"], false)
        );
        assert_eq!(discovered.details["haiku"].controls, None);
        assert_eq!(discovered.details["sonnet"].controls, None);
        assert_eq!(discovered.allowed_models.len(), 4);
    }

    /// End to end against a scripted adapter: `session/new`, then one
    /// `session/set_config_option` per model on the discovery session.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_probe_switches_a_real_adapter_through_its_models() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("agent.sh");
        std::fs::write(
            &script,
            r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"d1","configOptions":[{"id":"model","category":"model","currentValue":"a","options":[{"value":"a","name":"Model A"},{"value":"b","name":"Model B"}]},{"id":"effort","category":"thought_level","options":[{"value":"low"},{"value":"high"}]}]}}\n' "$id" ;;
    *'"method":"session/set_config_option"'*'"value":"b"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"id":"model","category":"model"},{"id":"fast-mode","category":"model_config","type":"select","options":[{"value":"off"},{"value":"on"}]}]}}\n' "$id" ;;
  esac
done
"#,
        )
        .expect("write agent");
        let discovered = probe(
            "bash",
            &[script.to_string_lossy().into_owned()],
            &[],
            &crate::execution_scope::ExecutionPlan::Legacy { reason: "test" },
            Duration::from_secs(5),
        )
        .await
        .expect("discovery");
        assert_eq!(discovered.allowed_models, vec!["a", "b"]);
        assert_eq!(discovered.details["a"].name.as_deref(), Some("Model A"));
        assert_eq!(
            discovered.details["a"].controls,
            controls(&["low", "high"], false)
        );
        assert_eq!(discovered.details["b"].controls, controls(&[], true));
    }
}
