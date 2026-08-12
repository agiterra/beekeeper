//! Environment surface for the coding-session provider.
//!
//! Everything the provider needs to run comes from the environment, because the
//! provider is spawned as a supervised child of the desktop app rather than
//! launched by a human with a config file. The variable names are therefore a
//! contract with that host, not a convenience: see `docs/nips/NIP-CSPC.md` and
//! the crate-level docs for the full list.
//!
//! Host-local execution state — most importantly the working directory a
//! session runs in — is deliberately **not** here. It arrives through
//! [`Config::projects_file`], is re-read per command, and never reaches signed
//! content.

use std::path::PathBuf;
use std::time::Duration;

use nostr::Keys;

/// Default ACP adapter binary for the `claude-agent-acp` driver.
pub const DEFAULT_AGENT_COMMAND: &str = "claude-agent-acp";
/// The driver slug this provider advertises and answers to.
pub const DRIVER: &str = "claude-agent-acp";
/// The runtime slug reported in catalog and metadata content.
pub const RUNTIME: &str = "claude";
/// The single provider instance reference this adapter advertises.
pub const PROVIDER_INSTANCE_REF: &str = "claude-primary";
/// Safe adapter-owned model alias used when live discovery is unavailable.
pub const DEFAULT_MODEL: &str = "default";

/// Default ceiling on concurrently live sessions.
pub const DEFAULT_MAX_SESSIONS: usize = 4;
/// Default idle window before a live session's subprocess is reclaimed.
pub const DEFAULT_SESSION_IDLE_SHUTDOWN_SECS: u64 = 1800;
/// Default per-turn silence budget, mirroring the buzz-acp harness.
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 900;
/// Default per-turn wall-clock ceiling, mirroring the buzz-acp harness.
pub const DEFAULT_MAX_TURN_DURATION_SECS: u64 = 7200;
/// Default age past which an unseen command is treated as history, not intent.
pub const DEFAULT_COMMAND_HORIZON_SECS: u64 = 86_400;
/// Number of hex characters of the provider pubkey used as the default instance id.
pub const INSTANCE_ID_PUBKEY_PREFIX_LEN: usize = 16;

/// Why the environment could not be turned into a runnable configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A variable with no default was absent.
    #[error("{0} must be set")]
    Missing(&'static str),
    /// A variable was present but could not be interpreted.
    #[error("{name} is invalid: {reason}")]
    Invalid {
        /// The offending variable.
        name: &'static str,
        /// What was wrong with it.
        reason: String,
    },
}

/// Fully resolved provider configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Signing identity of this provider instance.
    pub keys: Keys,
    /// Relay WebSocket URL.
    pub relay_url: String,
    /// Optional NIP-OA owner attestation presented during NIP-42 AUTH.
    pub auth_tag: Option<nostr::Tag>,
    /// Directory holding watermarks, session records, sequence counters, outbox.
    pub state_dir: PathBuf,
    /// Host-local working-directory map, re-read on every lifecycle command.
    pub projects_file: Option<PathBuf>,
    /// Stable provider instance id carried in every `cs-target`.
    pub instance_id: String,
    /// ACP adapter binary to spawn per session.
    pub agent_command: String,
    /// Model advertised as `defaultModel` and used when a command names none.
    pub default_model: String,
    /// Models this provider will accept, `allowedModels[0] == default_model`.
    pub allowed_models: Vec<String>,
    /// Whether startup should replace the fallback models with the adapter's
    /// live ACP catalog. Explicit model environment variables disable this.
    pub(crate) discover_models: bool,
    /// Ceiling on concurrently live sessions.
    pub max_sessions: usize,
    /// Idle window before a live session's subprocess is reclaimed.
    pub session_idle_shutdown: Duration,
    /// Per-turn silence budget passed to `session/prompt`.
    pub idle_timeout: Duration,
    /// Per-turn wall-clock ceiling passed to `session/prompt`.
    pub max_turn_duration: Duration,
    /// Whether `agent_thought_chunk` updates become `reasoning` transcript items.
    pub include_thoughts: bool,
    /// Age past which an unseen command is ignored rather than acted on.
    pub command_horizon: Duration,
}

impl Config {
    /// Read and validate the full `BUZZ_*` environment surface.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Same as [`Config::from_env`], against an injected lookup — the seam the
    /// tests use so they never mutate the process environment.
    pub fn from_lookup(
        lookup: impl Fn(&'static str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let private_key = required(&lookup, "BUZZ_PRIVATE_KEY")?;
        let keys = Keys::parse(private_key.trim()).map_err(|error| ConfigError::Invalid {
            name: "BUZZ_PRIVATE_KEY",
            reason: error.to_string(),
        })?;
        let relay_url = required(&lookup, "BUZZ_RELAY_URL")?;
        let auth_tag = parse_auth_tag(lookup("BUZZ_AUTH_TAG").as_deref())?;

        let state_dir = PathBuf::from(required(&lookup, "BUZZ_CSP_STATE_DIR")?);
        let projects_file = lookup("BUZZ_CSP_PROJECTS_FILE")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        let pubkey_hex = keys.public_key().to_hex();
        let instance_id = match lookup("BUZZ_CSP_INSTANCE_ID") {
            Some(value) if !value.trim().is_empty() => value.trim().to_owned(),
            _ => pubkey_hex
                .get(..INSTANCE_ID_PUBKEY_PREFIX_LEN)
                .unwrap_or(&pubkey_hex)
                .to_owned(),
        };

        let agent_command = non_empty(&lookup, "BUZZ_CSP_AGENT_COMMAND")
            .unwrap_or_else(|| DEFAULT_AGENT_COMMAND.to_owned());
        let configured_default_model = non_empty(&lookup, "BUZZ_CSP_DEFAULT_MODEL");
        let configured_allowed_models = non_empty(&lookup, "BUZZ_CSP_ALLOWED_MODELS");
        let discover_models =
            configured_default_model.is_none() && configured_allowed_models.is_none();
        let default_model = configured_default_model.unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let allowed_models =
            parse_allowed_models(configured_allowed_models.as_deref(), &default_model);

        let max_sessions = parse_usize(&lookup, "BUZZ_CSP_MAX_SESSIONS", DEFAULT_MAX_SESSIONS)?;
        if max_sessions == 0 {
            return Err(ConfigError::Invalid {
                name: "BUZZ_CSP_MAX_SESSIONS",
                reason: "must be at least 1".into(),
            });
        }
        let session_idle_shutdown = parse_secs(
            &lookup,
            "BUZZ_CSP_SESSION_IDLE_SHUTDOWN_SECS",
            DEFAULT_SESSION_IDLE_SHUTDOWN_SECS,
        )?;
        let idle_timeout = parse_secs(&lookup, "BUZZ_CSP_IDLE_TIMEOUT", DEFAULT_IDLE_TIMEOUT_SECS)?;
        let max_turn_duration = parse_secs(
            &lookup,
            "BUZZ_CSP_MAX_TURN_DURATION",
            DEFAULT_MAX_TURN_DURATION_SECS,
        )?;
        let command_horizon = parse_secs(
            &lookup,
            "BUZZ_CSP_COMMAND_HORIZON_SECS",
            DEFAULT_COMMAND_HORIZON_SECS,
        )?;
        let include_thoughts = parse_bool(&lookup, "BUZZ_CSP_INCLUDE_THOUGHTS", true)?;

        Ok(Self {
            keys,
            relay_url,
            auth_tag,
            state_dir,
            projects_file,
            instance_id,
            agent_command,
            default_model,
            allowed_models,
            discover_models,
            max_sessions,
            session_idle_shutdown,
            idle_timeout,
            max_turn_duration,
            include_thoughts,
            command_horizon,
        })
    }

    /// Lowercase hex of the provider's public key.
    pub fn pubkey_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }
}

fn required(
    lookup: &impl Fn(&'static str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    match lookup(name) {
        Some(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        _ => Err(ConfigError::Missing(name)),
    }
}

fn non_empty(
    lookup: &impl Fn(&'static str) -> Option<String>,
    name: &'static str,
) -> Option<String> {
    lookup(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_usize(
    lookup: &impl Fn(&'static str) -> Option<String>,
    name: &'static str,
    default: usize,
) -> Result<usize, ConfigError> {
    match non_empty(lookup, name) {
        None => Ok(default),
        Some(value) => value.parse().map_err(|_| ConfigError::Invalid {
            name,
            reason: format!("expected a non-negative integer, got {value:?}"),
        }),
    }
}

fn parse_secs(
    lookup: &impl Fn(&'static str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<Duration, ConfigError> {
    match non_empty(lookup, name) {
        None => Ok(Duration::from_secs(default)),
        Some(value) => {
            value
                .parse::<u64>()
                .map(Duration::from_secs)
                .map_err(|_| ConfigError::Invalid {
                    name,
                    reason: format!("expected whole seconds, got {value:?}"),
                })
        }
    }
}

fn parse_bool(
    lookup: &impl Fn(&'static str) -> Option<String>,
    name: &'static str,
    default: bool,
) -> Result<bool, ConfigError> {
    match non_empty(lookup, name) {
        None => Ok(default),
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(ConfigError::Invalid {
                name,
                reason: format!("expected a boolean, got {other:?}"),
            }),
        },
    }
}

/// Parse `BUZZ_CSP_ALLOWED_MODELS` into the catalog's canonical order.
///
/// The catalog's canonical form pins `allowedModels[0] == defaultModel` with the
/// remainder sorted, so the ordering is decided here once rather than at each
/// call site. The default model is always a member, even if the operator forgot
/// to list it — advertising a default the provider would then refuse is a worse
/// failure than silently widening the list by one.
fn parse_allowed_models(raw: Option<&str>, default_model: &str) -> Vec<String> {
    let mut rest: Vec<String> = raw
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && *entry != default_model)
        .map(str::to_owned)
        .collect();
    rest.sort();
    rest.dedup();
    let mut models = Vec::with_capacity(rest.len() + 1);
    models.push(default_model.to_owned());
    models.extend(rest);
    models
}

/// Decode `BUZZ_AUTH_TAG` — a JSON array of strings forming one Nostr tag.
fn parse_auth_tag(raw: Option<&str>) -> Result<Option<nostr::Tag>, ConfigError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parts: Vec<String> = serde_json::from_str(raw).map_err(|error| ConfigError::Invalid {
        name: "BUZZ_AUTH_TAG",
        reason: format!("expected a JSON array of strings: {error}"),
    })?;
    let tag = nostr::Tag::parse(parts).map_err(|error| ConfigError::Invalid {
        name: "BUZZ_AUTH_TAG",
        reason: error.to_string(),
    })?;
    Ok(Some(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs
            .iter()
            .map(|(key, value)| (*key, (*value).to_owned()))
            .collect()
    }

    fn minimal() -> HashMap<&'static str, String> {
        env(&[
            (
                "BUZZ_PRIVATE_KEY",
                "0000000000000000000000000000000000000000000000000000000000000001",
            ),
            ("BUZZ_RELAY_URL", "ws://localhost:3000"),
            ("BUZZ_CSP_STATE_DIR", "/tmp/csp"),
        ])
    }

    fn load(vars: &HashMap<&'static str, String>) -> Result<Config, ConfigError> {
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn defaults_fill_in_every_optional_variable() {
        let config = load(&minimal()).expect("minimal env should load");
        assert_eq!(config.agent_command, DEFAULT_AGENT_COMMAND);
        assert_eq!(config.default_model, DEFAULT_MODEL);
        assert_eq!(config.allowed_models, vec![DEFAULT_MODEL.to_owned()]);
        assert!(config.discover_models);
        assert_eq!(config.max_sessions, DEFAULT_MAX_SESSIONS);
        assert_eq!(config.command_horizon, Duration::from_secs(86_400));
        assert_eq!(config.session_idle_shutdown, Duration::from_secs(1800));
        assert!(config.include_thoughts);
        assert!(config.projects_file.is_none());
        assert!(config.auth_tag.is_none());
    }

    /// The instance id has to be stable across restarts without any persisted
    /// state, because it is half of every `cs-target` a consumer has already
    /// stored. Deriving it from the pubkey is what makes that true by default.
    #[test]
    fn instance_id_defaults_to_the_pubkey_prefix() {
        let config = load(&minimal()).expect("minimal env should load");
        assert_eq!(config.instance_id.len(), INSTANCE_ID_PUBKEY_PREFIX_LEN);
        assert!(config.pubkey_hex().starts_with(&config.instance_id));

        let mut vars = minimal();
        vars.insert("BUZZ_CSP_INSTANCE_ID", "workstation-a".into());
        assert_eq!(load(&vars).unwrap().instance_id, "workstation-a");
    }

    #[test]
    fn allowed_models_put_the_default_first_and_sort_the_rest() {
        let mut vars = minimal();
        vars.insert("BUZZ_CSP_DEFAULT_MODEL", "model-b".into());
        vars.insert(
            "BUZZ_CSP_ALLOWED_MODELS",
            " model-c , model-a ,model-b, model-a ".into(),
        );
        let config = load(&vars).expect("env should load");
        assert_eq!(
            config.allowed_models,
            vec![
                "model-b".to_owned(),
                "model-a".to_owned(),
                "model-c".to_owned()
            ]
        );
        assert!(!config.discover_models);
    }

    #[test]
    fn either_model_override_disables_live_discovery() {
        let mut default_only = minimal();
        default_only.insert("BUZZ_CSP_DEFAULT_MODEL", "model-a".into());
        assert!(!load(&default_only).unwrap().discover_models);

        let mut allowed_only = minimal();
        allowed_only.insert("BUZZ_CSP_ALLOWED_MODELS", "model-a,model-b".into());
        assert!(!load(&allowed_only).unwrap().discover_models);
    }

    #[test]
    fn rejects_missing_and_malformed_variables() {
        let mut vars = minimal();
        vars.remove("BUZZ_CSP_STATE_DIR");
        assert!(matches!(
            load(&vars),
            Err(ConfigError::Missing("BUZZ_CSP_STATE_DIR"))
        ));

        let mut vars = minimal();
        vars.insert("BUZZ_PRIVATE_KEY", "not-a-key".into());
        assert!(matches!(load(&vars), Err(ConfigError::Invalid { .. })));

        let mut vars = minimal();
        vars.insert("BUZZ_CSP_MAX_SESSIONS", "0".into());
        assert!(matches!(load(&vars), Err(ConfigError::Invalid { .. })));

        let mut vars = minimal();
        vars.insert("BUZZ_CSP_INCLUDE_THOUGHTS", "maybe".into());
        assert!(matches!(load(&vars), Err(ConfigError::Invalid { .. })));

        let mut vars = minimal();
        vars.insert("BUZZ_AUTH_TAG", "{\"not\":\"an array\"}".into());
        assert!(matches!(load(&vars), Err(ConfigError::Invalid { .. })));
    }

    #[test]
    fn parses_a_nip_oa_auth_tag() {
        let mut vars = minimal();
        vars.insert("BUZZ_AUTH_TAG", r#"["owner-attestation","payload"]"#.into());
        let config = load(&vars).expect("env should load");
        let tag = config.auth_tag.expect("auth tag should parse");
        assert_eq!(tag.as_slice()[0], "owner-attestation");
    }

    #[test]
    fn include_thoughts_can_be_switched_off() {
        let mut vars = minimal();
        vars.insert("BUZZ_CSP_INCLUDE_THOUGHTS", "false".into());
        assert!(!load(&vars).unwrap().include_thoughts);
    }
}
