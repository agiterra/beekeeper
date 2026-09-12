//! Runtime descriptors for the coding-session provider.
//!
//! The desktop host decides which agent runtimes the sidecar offers and hands
//! the whole list over in one environment variable, `BUZZ_CSP_RUNTIMES` — a
//! JSON array of [`RuntimeDescriptor`]s. The definition lives here because
//! both sides of that contract (the Tauri host that writes it and the
//! `buzz-session-provider` sidecar that reads it) already depend on
//! `buzz-core`, so a single serde definition keeps them from drifting apart.

use serde::{Deserialize, Serialize};

use crate::coding_session_payload::Capabilities;

/// Upper bound on descriptors, matching the catalog's `MAX_PROVIDERS`.
pub const MAX_RUNTIME_DESCRIPTORS: usize = 32;

/// One agent runtime the coding-session provider offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeDescriptor {
    /// Unique within the sidecar; a 44221 create names it. Convention:
    /// `"<runtime>-primary"`.
    pub instance_ref: String,
    /// Driver slug minted into every `cs-target` for this runtime's sessions.
    pub driver: String,
    /// Runtime slug (`"claude"`, `"codex"`, `"goose"`) reported in catalog and
    /// metadata content.
    pub runtime: String,
    /// ACP adapter executable (absolute path or bare name for PATH lookup).
    pub agent_command: String,
    /// Adapter argv after the command. Default `[]`.
    #[serde(default)]
    pub agent_args: Vec<String>,
    /// Env var injected into each per-session adapter spawn
    /// (e.g. `CLAUDE_CODE_EXECUTABLE`).
    #[serde(default)]
    pub cli_env: Option<CliEnvVar>,
    /// Model advertised as `defaultModel`. Default `"default"`.
    #[serde(default = "default_model_alias")]
    pub default_model: String,
    /// Accepted models. Default `[defaultModel]`. Canonicalized by the sidecar.
    #[serde(default)]
    pub allowed_models: Vec<String>,
    /// Whether the sidecar probes the adapter's live ACP model catalog at
    /// startup.
    #[serde(default)]
    pub discover_models: bool,
    /// Capability vector. Absent means
    /// [`Capabilities::v1_for_runtime`]`(&runtime)`.
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
    /// The idle guard the installed adapter honours on `_session/steering`.
    ///
    /// A declared fact about the pinned adapter package, set by whoever
    /// installs it, never inferred from the driver slug: no adapter
    /// advertises it. Native mid-turn injection is offered only when this is
    /// [`SteerIdleGuard::PromptRequired`] **and** the process behind the
    /// execution advertised `_meta.steering.supported` at `initialize`. Absent
    /// means no guard is known, so a steer can only be boundary-delivered —
    /// an adapter without a guard would start a detached turn nobody observes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steer_idle_guard: Option<SteerIdleGuard>,
}

/// How an adapter answers a `_session/steering` request that finds no
/// running turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SteerIdleGuard {
    /// The adapter honours request `_meta.steering.idleBehavior:
    /// "promptRequired"`: with no running turn it answers
    /// `{outcome: "promptRequired"}` and leaves the content with the caller
    /// (claude-agent-acp 0.70.0, `dist/acp-agent.js:1146-1150`).
    PromptRequired,
}

/// One env-var name/value pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CliEnvVar {
    /// Variable name.
    pub name: String,
    /// Variable value.
    pub value: String,
}

fn default_model_alias() -> String {
    "default".to_owned()
}

/// Parse and validate a `BUZZ_CSP_RUNTIMES` JSON array.
///
/// Enforces: non-empty array, at most [`MAX_RUNTIME_DESCRIPTORS`] entries,
/// unique non-empty `instanceRef`s, and non-empty `driver` / `runtime` /
/// `agentCommand` per entry. Duplicate drivers are permitted — session UUIDs
/// make turn routing unambiguous. An empty `allowedModels` is normalized to
/// `[defaultModel]`.
pub fn parse_runtime_descriptors(json: &str) -> Result<Vec<RuntimeDescriptor>, String> {
    let mut descriptors: Vec<RuntimeDescriptor> =
        serde_json::from_str(json).map_err(|error| format!("expected a JSON array: {error}"))?;
    if descriptors.is_empty() {
        return Err("expected at least one runtime descriptor".to_owned());
    }
    if descriptors.len() > MAX_RUNTIME_DESCRIPTORS {
        return Err(format!(
            "expected at most {MAX_RUNTIME_DESCRIPTORS} runtime descriptors, got {}",
            descriptors.len()
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for descriptor in &mut descriptors {
        if descriptor.instance_ref.trim().is_empty() {
            return Err("instanceRef must not be empty".to_owned());
        }
        if !seen.insert(descriptor.instance_ref.clone()) {
            return Err(format!(
                "duplicate instanceRef {:?}",
                descriptor.instance_ref
            ));
        }
        for (field, value) in [
            ("driver", &descriptor.driver),
            ("runtime", &descriptor.runtime),
            ("agentCommand", &descriptor.agent_command),
        ] {
            if value.trim().is_empty() {
                return Err(format!(
                    "{field} must not be empty (instanceRef {:?})",
                    descriptor.instance_ref
                ));
            }
        }
        if descriptor.allowed_models.is_empty() {
            descriptor
                .allowed_models
                .push(descriptor.default_model.clone());
        }
    }
    Ok(descriptors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal(instance_ref: &str) -> serde_json::Value {
        serde_json::json!({
            "instanceRef": instance_ref,
            "driver": "claude-agent-acp",
            "runtime": "claude",
            "agentCommand": "claude-agent-acp",
        })
    }

    #[test]
    fn defaults_fill_in_every_optional_field() {
        let json = serde_json::json!([minimal("claude-primary")]).to_string();
        let descriptors = parse_runtime_descriptors(&json).expect("parse");
        assert_eq!(descriptors.len(), 1);
        let descriptor = &descriptors[0];
        assert_eq!(descriptor.instance_ref, "claude-primary");
        assert!(descriptor.agent_args.is_empty());
        assert!(descriptor.cli_env.is_none());
        assert_eq!(descriptor.default_model, "default");
        assert_eq!(descriptor.allowed_models, vec!["default".to_owned()]);
        assert!(!descriptor.discover_models);
        assert!(descriptor.capabilities.is_none());
    }

    #[test]
    fn the_full_shape_round_trips() {
        let json = serde_json::json!([{
            "instanceRef": "goose-primary",
            "driver": "goose-acp",
            "runtime": "goose",
            "agentCommand": "/opt/homebrew/bin/goose",
            "agentArgs": ["acp"],
            "cliEnv": { "name": "GOOSE_MODE", "value": "auto" },
            "defaultModel": "gpt-4o",
            "allowedModels": ["gpt-4o", "gpt-4o-mini"],
            "discoverModels": true,
            "capabilities": {
                "threadTurnStart": true,
                "threadTurnInterrupt": true,
                "threadSteer": true,
                "context": false,
                "diff": false,
                "plan": false
            }
        }])
        .to_string();
        let descriptors = parse_runtime_descriptors(&json).expect("parse");
        let descriptor = &descriptors[0];
        assert_eq!(descriptor.agent_args, vec!["acp".to_owned()]);
        assert_eq!(
            descriptor.cli_env,
            Some(CliEnvVar {
                name: "GOOSE_MODE".into(),
                value: "auto".into()
            })
        );
        assert!(descriptor.discover_models);
        assert!(descriptor.capabilities.expect("capabilities").thread_steer);
        let serialized = serde_json::to_string(&descriptors).expect("serialize");
        assert_eq!(
            parse_runtime_descriptors(&serialized).expect("reparse"),
            descriptors
        );
    }

    /// The idle guard is a declared fact about the pinned adapter, so it has
    /// to survive the trip through `BUZZ_CSP_RUNTIMES` byte-for-byte — and
    /// an older host that never heard of it must produce a list an older
    /// sidecar still parses, which is why absence is *omission*, not `null`.
    #[test]
    fn the_steer_idle_guard_round_trips_and_is_omitted_when_absent() {
        let mut entry = minimal("claude-primary");
        entry["steerIdleGuard"] = serde_json::json!("promptRequired");
        let descriptors =
            parse_runtime_descriptors(&serde_json::json!([entry]).to_string()).expect("parse");
        assert_eq!(
            descriptors[0].steer_idle_guard,
            Some(SteerIdleGuard::PromptRequired)
        );
        let serialized = serde_json::to_value(&descriptors[0]).expect("serialize");
        assert_eq!(serialized["steerIdleGuard"], "promptRequired");
        assert_eq!(
            parse_runtime_descriptors(&serde_json::json!([serialized]).to_string())
                .expect("reparse")[0]
                .steer_idle_guard,
            Some(SteerIdleGuard::PromptRequired)
        );

        // No guard declared: the key is absent from the wire, not null.
        let bare =
            parse_runtime_descriptors(&serde_json::json!([minimal("codex-primary")]).to_string())
                .expect("parse");
        assert_eq!(bare[0].steer_idle_guard, None);
        let serialized = serde_json::to_value(&bare[0]).expect("serialize");
        assert!(
            !serialized
                .as_object()
                .expect("object")
                .contains_key("steerIdleGuard"),
            "{serialized}"
        );

        // An unknown guard name is a contract violation: the sidecar must not
        // read "some guard" as "the guard it knows".
        let mut unknown = minimal("claude-primary");
        unknown["steerIdleGuard"] = serde_json::json!("startNewTurn");
        assert!(parse_runtime_descriptors(&serde_json::json!([unknown]).to_string()).is_err());
    }

    #[test]
    fn invalid_lists_are_rejected() {
        assert!(parse_runtime_descriptors("not json").is_err());
        assert!(parse_runtime_descriptors("[]").is_err());
        assert!(parse_runtime_descriptors(r#"{"instanceRef":"x"}"#).is_err());

        // Duplicate refs.
        let dup =
            serde_json::json!([minimal("claude-primary"), minimal("claude-primary")]).to_string();
        assert!(parse_runtime_descriptors(&dup)
            .unwrap_err()
            .contains("duplicate"));

        // Empty required fields.
        for field in ["instanceRef", "driver", "runtime", "agentCommand"] {
            let mut entry = minimal("claude-primary");
            entry[field] = serde_json::json!("  ");
            let json = serde_json::json!([entry]).to_string();
            assert!(
                parse_runtime_descriptors(&json).is_err(),
                "blank {field} should be rejected"
            );
        }

        // Unknown keys are a contract violation, not an extension point.
        let mut entry = minimal("claude-primary");
        entry["surprise"] = serde_json::json!(true);
        assert!(parse_runtime_descriptors(&serde_json::json!([entry]).to_string()).is_err());

        // Too many entries.
        let many: Vec<_> = (0..MAX_RUNTIME_DESCRIPTORS + 1)
            .map(|index| minimal(&format!("runtime-{index}")))
            .collect();
        assert!(parse_runtime_descriptors(&serde_json::json!(many).to_string()).is_err());
    }
}
