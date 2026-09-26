//! Live model discovery for ACP adapters that opt in via `discoverModels`.

use std::collections::BTreeSet;
use std::time::Duration;

use buzz_acp::acp::{extract_model_config_options, extract_model_state, AcpClient};

use crate::config::Config;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveredModels {
    default_model: String,
    allowed_models: Vec<String>,
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
        let result = tokio::time::timeout(
            DISCOVERY_TIMEOUT,
            probe(
                &descriptor.agent_command,
                &descriptor.agent_args,
                &cli_env,
                &plan,
            ),
        )
        .await;
        let runtime = descriptor.runtime.as_str();
        let discovered = match result {
            Ok(Ok(discovered)) => discovered,
            Ok(Err(error)) => {
                tracing::warn!(target: "csp::models", "{runtime} model discovery failed: {error}");
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    target: "csp::models",
                    "{runtime} model discovery timed out after {DISCOVERY_TIMEOUT:?}"
                );
                continue;
            }
        };

        descriptor.default_model = discovered.default_model;
        descriptor.allowed_models = discovered.allowed_models;
    }
}

/// The scope a runtime's model discovery runs in: an empty host-owned
/// working directory and the runtime's private state, inside the boundary
/// where one exists — so discovery opens no conversation in the provider's
/// own directory or in the operator's runtime history.
fn discovery_plan(
    state_dir: &std::path::Path,
    runtime_override: Option<crate::execution_scope::RuntimeProfile>,
    descriptor: &buzz_core::coding_session_runtime::RuntimeDescriptor,
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
) -> anyhow::Result<DiscoveredModels> {
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
    let result = async {
        client.initialize().await?;
        let response = client.session_new_full(&cwd, vec![], None, None).await?;
        models_from_session_new(&response.raw)
            .ok_or_else(|| anyhow::anyhow!("adapter returned no selectable models"))
    }
    .await;
    client.shutdown().await;
    result
}

fn models_from_session_new(raw: &serde_json::Value) -> Option<DiscoveredModels> {
    let mut model_ids = BTreeSet::new();
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
}
