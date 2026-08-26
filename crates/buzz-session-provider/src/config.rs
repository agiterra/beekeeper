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

use buzz_acp::acp::RawSdkFrames;
use buzz_core::coding_session_runtime::{parse_runtime_descriptors, RuntimeDescriptor};
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
/// `BUZZ_CSP_MAX_SESSIONS` value meaning "no ceiling".
///
/// Zero used to be rejected outright. It is the natural spelling of unlimited,
/// nothing depended on the old error, and a person who has decided their own
/// machine can hold more than four agents at once should be able to say so
/// (asked for 2026-08-24). Every other slot rule still applies: idle sessions
/// are still reclaimed, and a create still fails if the adapter will not start.
pub const UNLIMITED_MAX_SESSIONS: usize = 0;
/// Default idle window before a live session's subprocess is reclaimed.
///
/// Thirty minutes reclaimed adapters out from under people mid-workday: a
/// session left while a meeting ran came back Disconnected, and every return
/// cost a reconnect. Four hours matches how long a coding session actually
/// rests between turns; the cap on concurrently live sessions still bounds
/// how many adapters can be held. Override with
/// `BUZZ_CSP_SESSION_IDLE_SHUTDOWN_SECS`.
pub const DEFAULT_SESSION_IDLE_SHUTDOWN_SECS: u64 = 14_400;
/// Default per-turn silence budget, mirroring the buzz-acp harness.
///
/// Off-round on purpose — see the harness constant of the same name. Claude
/// Code's `Monitor` budget is 900000ms; while this was also 900s, a turn that
/// died at ~900s could have been ended by either clock and the duration said
/// nothing about which.
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 870;
/// Default per-turn wall-clock ceiling, mirroring the buzz-acp harness.
pub const DEFAULT_MAX_TURN_DURATION_SECS: u64 = 7200;
/// Default budget for silence *after* a turn has finished answering.
///
/// Not a second idle timeout. It arms only once the agent has streamed prose
/// and every tool call it opened has reported terminal — a shape in which the
/// SDK result normally follows within milliseconds. Two minutes is generous
/// against that, leaving room for a slow first token on a cold cache, while
/// still ending a dropped prompt in a fraction of the fifteen-minute silence
/// budget the idle timer has to reserve for genuinely long tools.
///
/// `0` disables the watch, leaving the idle timer as the only backstop.
/// Override with `BUZZ_CSP_ANSWER_STALL_TIMEOUT`.
pub const DEFAULT_ANSWER_STALL_TIMEOUT_SECS: u64 = 120;
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
    /// Optional read-only MCP binary used to expose verified relay context to
    /// every ACP session-open path. Absent keeps legacy behavior.
    pub context_mcp_command: Option<PathBuf>,
    /// Stable provider instance id carried in every `cs-target`.
    pub instance_id: String,
    /// Every runtime this provider offers, in the host's order. Never empty:
    /// when `BUZZ_CSP_RUNTIMES` is absent a single legacy Claude descriptor is
    /// synthesized from the older per-variable surface.
    pub runtimes: Vec<RuntimeDescriptor>,
    /// Ceiling on concurrently live sessions.
    pub max_sessions: usize,
    /// Idle window before a live session's subprocess is reclaimed.
    pub session_idle_shutdown: Duration,
    /// Per-turn silence budget passed to `session/prompt`.
    pub idle_timeout: Duration,
    /// Budget for silence after the turn has finished answering, or `None` when
    /// disabled. See [`DEFAULT_ANSWER_STALL_TIMEOUT_SECS`].
    pub answer_stall_timeout: Option<Duration>,
    /// Per-turn wall-clock ceiling passed to `session/prompt`.
    pub max_turn_duration: Duration,
    /// Whether `agent_thought_chunk` updates become `reasoning` transcript items.
    pub include_thoughts: bool,
    /// How much of the SDK's own message stream to ask the adapter to forward.
    ///
    /// Defaults to the filtered lifecycle set — those frames are the only way
    /// this provider can learn that a session has background work running, and
    /// at ~80 frames per session they are affordable to leave on. `all` is the
    /// unfiltered firehose and is for one debugging session at a time.
    /// See `BUZZ_CSP_EMIT_RAW_SDK_FRAMES`.
    pub raw_sdk_frames: RawSdkFrames,
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
        let context_mcp_command = lookup("BUZZ_CSP_CONTEXT_MCP_COMMAND")
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

        let runtimes = parse_runtimes(&lookup)?;

        // 0 is unlimited, not invalid — see `UNLIMITED_MAX_SESSIONS`.
        let max_sessions = parse_usize(&lookup, "BUZZ_CSP_MAX_SESSIONS", DEFAULT_MAX_SESSIONS)?;
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
        // The harness validates this pair (`buzz-acp/src/config.rs`); this side
        // never did, so an idle budget at or above the wall-clock ceiling made
        // the idle guard unreachable — it can only fire if it comes first.
        // Silently having no silence guard is worse than being told the numbers
        // are contradictory.
        if idle_timeout >= max_turn_duration {
            return Err(ConfigError::Invalid {
                name: "BUZZ_CSP_IDLE_TIMEOUT",
                reason: format!(
                    "must be less than BUZZ_CSP_MAX_TURN_DURATION ({}s), otherwise the idle \
                     guard can never fire; got {}s",
                    max_turn_duration.as_secs(),
                    idle_timeout.as_secs(),
                ),
            });
        }
        let answer_stall_timeout = match parse_secs(
            &lookup,
            "BUZZ_CSP_ANSWER_STALL_TIMEOUT",
            DEFAULT_ANSWER_STALL_TIMEOUT_SECS,
        )? {
            zero if zero.is_zero() => None,
            timeout => Some(timeout),
        };
        let command_horizon = parse_secs(
            &lookup,
            "BUZZ_CSP_COMMAND_HORIZON_SECS",
            DEFAULT_COMMAND_HORIZON_SECS,
        )?;
        let include_thoughts = parse_bool(&lookup, "BUZZ_CSP_INCLUDE_THOUGHTS", true)?;
        let raw_sdk_frames = match lookup("BUZZ_CSP_EMIT_RAW_SDK_FRAMES") {
            None => RawSdkFrames::default(),
            Some(raw) => raw
                .parse::<RawSdkFrames>()
                .map_err(|reason| ConfigError::Invalid {
                    name: "BUZZ_CSP_EMIT_RAW_SDK_FRAMES",
                    reason,
                })?,
        };

        Ok(Self {
            keys,
            relay_url,
            auth_tag,
            state_dir,
            projects_file,
            context_mcp_command,
            instance_id,
            runtimes,
            max_sessions,
            session_idle_shutdown,
            idle_timeout,
            max_turn_duration,
            answer_stall_timeout,
            include_thoughts,
            raw_sdk_frames,
            command_horizon,
        })
    }

    /// Lowercase hex of the provider's public key.
    pub fn pubkey_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }

    /// The runtime descriptor advertised under `instance_ref`, if any.
    pub fn runtime(&self, instance_ref: &str) -> Option<&RuntimeDescriptor> {
        self.runtimes
            .iter()
            .find(|descriptor| descriptor.instance_ref == instance_ref)
    }
}

/// Resolve the runtime list: `BUZZ_CSP_RUNTIMES` when present, otherwise a
/// single Claude descriptor synthesized from the legacy per-variable surface.
///
/// When `BUZZ_CSP_RUNTIMES` is set it is the complete list — the legacy
/// variables are ignored rather than merged, because a partial merge would
/// make the effective configuration depend on which of two writers ran last.
/// A malformed list fails startup: the supervisor is the only writer, so a
/// parse error is a host bug, not operator input to be papered over.
fn parse_runtimes(
    lookup: &impl Fn(&'static str) -> Option<String>,
) -> Result<Vec<RuntimeDescriptor>, ConfigError> {
    let Some(raw) = non_empty(lookup, "BUZZ_CSP_RUNTIMES") else {
        return Ok(vec![legacy_claude_descriptor(lookup)]);
    };
    let descriptors = parse_runtime_descriptors(&raw).map_err(|reason| ConfigError::Invalid {
        name: "BUZZ_CSP_RUNTIMES",
        reason,
    })?;
    warn_on_diverging_legacy_variables(lookup, &descriptors);
    Ok(descriptors)
}

/// Flag legacy per-variable configuration that the runtime list overrides.
///
/// The desktop host deliberately exports `BUZZ_CSP_AGENT_COMMAND` *alongside*
/// `BUZZ_CSP_RUNTIMES` so an older sidecar binary keeps working, which makes
/// "both are set" the normal shape of every spawn — warning on mere presence
/// would put permanent noise in the log and train people to ignore it. A
/// warning therefore fires only when a legacy value would have changed
/// behavior had the list not won: an agent command no descriptor uses, or a
/// model override, which the dual-exporting host never sets.
fn warn_on_diverging_legacy_variables(
    lookup: &impl Fn(&'static str) -> Option<String>,
    descriptors: &[RuntimeDescriptor],
) {
    if let Some(command) = non_empty(lookup, "BUZZ_CSP_AGENT_COMMAND") {
        let matches_a_descriptor = descriptors
            .iter()
            .any(|descriptor| descriptor.agent_command == command);
        if !matches_a_descriptor {
            tracing::warn!(
                target: "csp::config",
                "BUZZ_CSP_AGENT_COMMAND names {command:?}, which no BUZZ_CSP_RUNTIMES \
                 descriptor uses — the list wins and the variable is ignored"
            );
        }
    }
    for legacy in ["BUZZ_CSP_DEFAULT_MODEL", "BUZZ_CSP_ALLOWED_MODELS"] {
        if non_empty(lookup, legacy).is_some() {
            tracing::warn!(
                target: "csp::config",
                "{legacy} is ignored because BUZZ_CSP_RUNTIMES is set"
            );
        }
    }
}

/// The single-runtime configuration older hosts express through
/// `BUZZ_CSP_AGENT_COMMAND` / `BUZZ_CSP_DEFAULT_MODEL` /
/// `BUZZ_CSP_ALLOWED_MODELS` — today's zero-config Claude default.
fn legacy_claude_descriptor(lookup: &impl Fn(&'static str) -> Option<String>) -> RuntimeDescriptor {
    let agent_command = non_empty(lookup, "BUZZ_CSP_AGENT_COMMAND")
        .unwrap_or_else(|| DEFAULT_AGENT_COMMAND.to_owned());
    let configured_default_model = non_empty(lookup, "BUZZ_CSP_DEFAULT_MODEL");
    let configured_allowed_models = non_empty(lookup, "BUZZ_CSP_ALLOWED_MODELS");
    let discover_models = configured_default_model.is_none() && configured_allowed_models.is_none();
    let default_model = configured_default_model.unwrap_or_else(|| DEFAULT_MODEL.to_owned());
    let allowed_models = parse_allowed_models(configured_allowed_models.as_deref(), &default_model);
    RuntimeDescriptor {
        instance_ref: PROVIDER_INSTANCE_REF.to_owned(),
        driver: DRIVER.to_owned(),
        runtime: RUNTIME.to_owned(),
        agent_command,
        agent_args: Vec::new(),
        cli_env: None,
        default_model,
        allowed_models,
        discover_models,
        capabilities: None,
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

    /// An idle budget that can never fire is a configuration error, not a
    /// silent loss of the guard.
    ///
    /// The guard only fires if it comes before the wall-clock ceiling, so
    /// setting it at or above that ceiling removes the silence guard entirely
    /// while looking like it raised it. The harness has always rejected this
    /// pair; this side accepted it.
    /// Raw frames are a debugging instrument, so the default has to be off —
    /// a switch that defaults on stops being a switch.
    /// The default has to be the filtered set, not the firehose and not off:
    /// background-task visibility is built on these frames, so a default of
    /// `off` would make the feature depend on somebody remembering a switch,
    /// and a default of `all` would cost 8447 frames a session to get 80.
    #[test]
    fn raw_frames_default_to_the_filtered_lifecycle() {
        let mut vars = minimal();
        assert_eq!(
            load(&vars).expect("config").raw_sdk_frames,
            RawSdkFrames::Lifecycle
        );

        for (value, expected) in [
            ("all", RawSdkFrames::All),
            ("off", RawSdkFrames::Off),
            ("lifecycle", RawSdkFrames::Lifecycle),
        ] {
            vars.insert("BUZZ_CSP_EMIT_RAW_SDK_FRAMES", value.to_owned());
            assert_eq!(
                load(&vars).expect("config").raw_sdk_frames,
                expected,
                "for {value:?}"
            );
        }
    }

    /// A typo must be refused rather than silently falling back — a mode that
    /// quietly reverts to the default is how somebody debugs for an hour
    /// wondering why the frames never arrived.
    #[test]
    fn an_unrecognized_raw_frame_mode_is_refused_by_name() {
        let mut vars = minimal();
        vars.insert("BUZZ_CSP_EMIT_RAW_SDK_FRAMES", "verbose".to_owned());
        let error = load(&vars).expect_err("must refuse");
        assert!(
            matches!(
                &error,
                ConfigError::Invalid {
                    name: "BUZZ_CSP_EMIT_RAW_SDK_FRAMES",
                    reason,
                } if reason.contains("lifecycle") && reason.contains("all")
            ),
            "the refusal must name the variable and the valid modes: {error:?}"
        );
    }

    #[test]
    fn an_idle_budget_that_could_never_fire_is_refused() {
        let mut vars = minimal();
        vars.insert("BUZZ_CSP_IDLE_TIMEOUT", "7200".to_owned());
        vars.insert("BUZZ_CSP_MAX_TURN_DURATION", "7200".to_owned());
        assert!(
            matches!(
                load(&vars),
                Err(ConfigError::Invalid {
                    name: "BUZZ_CSP_IDLE_TIMEOUT",
                    ..
                })
            ),
            "an idle budget at the wall-clock ceiling must be refused"
        );

        vars.insert("BUZZ_CSP_IDLE_TIMEOUT", "7199".to_owned());
        assert!(load(&vars).is_ok(), "one second under the ceiling is fine");
    }

    /// The stall watch defaults on, and `0` is how it is turned off.
    ///
    /// `0` has to mean *disabled* rather than "fire immediately", which is what
    /// a bare `Duration::from_secs(0)` would do — it would end every turn the
    /// instant it finished answering.
    #[test]
    fn the_answer_stall_budget_defaults_on_and_zero_disables_it() {
        let config = load(&minimal()).expect("minimal env should load");
        assert_eq!(
            config.answer_stall_timeout,
            Some(Duration::from_secs(DEFAULT_ANSWER_STALL_TIMEOUT_SECS)),
        );

        let mut vars = minimal();
        vars.insert("BUZZ_CSP_ANSWER_STALL_TIMEOUT", "0".to_owned());
        assert_eq!(
            load(&vars).expect("zero should load").answer_stall_timeout,
            None,
            "zero must disable the watch, not arm it at zero"
        );

        vars.insert("BUZZ_CSP_ANSWER_STALL_TIMEOUT", "45".to_owned());
        assert_eq!(
            load(&vars).expect("should load").answer_stall_timeout,
            Some(Duration::from_secs(45)),
        );
    }

    #[test]
    fn defaults_fill_in_every_optional_variable() {
        let config = load(&minimal()).expect("minimal env should load");
        assert_eq!(config.runtimes.len(), 1);
        let claude = &config.runtimes[0];
        assert_eq!(claude.instance_ref, PROVIDER_INSTANCE_REF);
        assert_eq!(claude.driver, DRIVER);
        assert_eq!(claude.runtime, RUNTIME);
        assert_eq!(claude.agent_command, DEFAULT_AGENT_COMMAND);
        assert!(claude.agent_args.is_empty());
        assert!(claude.cli_env.is_none());
        assert_eq!(claude.default_model, DEFAULT_MODEL);
        assert_eq!(claude.allowed_models, vec![DEFAULT_MODEL.to_owned()]);
        assert!(claude.discover_models);
        assert!(claude.capabilities.is_none());
        assert_eq!(config.max_sessions, DEFAULT_MAX_SESSIONS);
        assert_eq!(config.command_horizon, Duration::from_secs(86_400));
        assert_eq!(config.session_idle_shutdown, Duration::from_secs(14_400));
        assert!(config.include_thoughts);
        assert!(config.projects_file.is_none());
        assert!(config.context_mcp_command.is_none());
        assert!(config.auth_tag.is_none());
    }

    #[test]
    fn parses_the_optional_context_mcp_command() {
        let mut vars = minimal();
        vars.insert(
            "BUZZ_CSP_CONTEXT_MCP_COMMAND",
            " /opt/buzz/bin/buzz-dev-mcp ".into(),
        );
        assert_eq!(
            load(&vars).unwrap().context_mcp_command,
            Some(PathBuf::from("/opt/buzz/bin/buzz-dev-mcp"))
        );
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
            config.runtimes[0].allowed_models,
            vec![
                "model-b".to_owned(),
                "model-a".to_owned(),
                "model-c".to_owned()
            ]
        );
        assert!(!config.runtimes[0].discover_models);
    }

    #[test]
    fn either_model_override_disables_live_discovery() {
        let mut default_only = minimal();
        default_only.insert("BUZZ_CSP_DEFAULT_MODEL", "model-a".into());
        assert!(!load(&default_only).unwrap().runtimes[0].discover_models);

        let mut allowed_only = minimal();
        allowed_only.insert("BUZZ_CSP_ALLOWED_MODELS", "model-a,model-b".into());
        assert!(!load(&allowed_only).unwrap().runtimes[0].discover_models);
    }

    #[test]
    fn a_runtimes_list_replaces_the_legacy_surface_entirely() {
        let mut vars = minimal();
        // Legacy variables present but ignored: the list is the whole truth.
        vars.insert("BUZZ_CSP_AGENT_COMMAND", "/somewhere/else".into());
        vars.insert("BUZZ_CSP_DEFAULT_MODEL", "legacy-model".into());
        vars.insert(
            "BUZZ_CSP_RUNTIMES",
            serde_json::json!([
                {
                    "instanceRef": "claude-primary",
                    "driver": "claude-agent-acp",
                    "runtime": "claude",
                    "agentCommand": "/tools/claude-agent-acp",
                    "cliEnv": { "name": "CLAUDE_CODE_EXECUTABLE", "value": "/bin/claude" },
                    "discoverModels": true
                },
                {
                    "instanceRef": "goose-primary",
                    "driver": "goose-acp",
                    "runtime": "goose",
                    "agentCommand": "/bin/goose",
                    "agentArgs": ["acp"]
                }
            ])
            .to_string(),
        );
        let config = load(&vars).expect("env should load");
        assert_eq!(config.runtimes.len(), 2);
        assert_eq!(config.runtimes[0].agent_command, "/tools/claude-agent-acp");
        assert_eq!(
            config
                .runtime("goose-primary")
                .expect("goose descriptor")
                .agent_args,
            vec!["acp".to_owned()]
        );
        assert!(config.runtime("codex-primary").is_none());
        // The empty allowedModels default is normalized to [defaultModel].
        assert_eq!(
            config.runtimes[1].allowed_models,
            vec!["default".to_owned()]
        );
    }

    #[test]
    fn a_malformed_runtimes_list_refuses_to_start() {
        for raw in ["not json", "[]", r#"[{"instanceRef":"x"}]"#] {
            let mut vars = minimal();
            vars.insert("BUZZ_CSP_RUNTIMES", raw.into());
            assert!(
                matches!(
                    load(&vars),
                    Err(ConfigError::Invalid {
                        name: "BUZZ_CSP_RUNTIMES",
                        ..
                    })
                ),
                "should reject {raw}"
            );
        }
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

    /// Zero is how a person says "no ceiling" (asked for 2026-08-24). It used
    /// to be rejected, so nothing depended on the old error.
    #[test]
    fn zero_max_sessions_means_unlimited_rather_than_invalid() {
        let mut vars = minimal();
        vars.insert("BUZZ_CSP_MAX_SESSIONS", "0".into());
        let config = load(&vars).expect("zero is a valid ceiling");
        assert_eq!(config.max_sessions, UNLIMITED_MAX_SESSIONS);

        let mut vars = minimal();
        vars.insert("BUZZ_CSP_MAX_SESSIONS", "12".into());
        assert_eq!(load(&vars).expect("explicit ceiling").max_sessions, 12);

        // A value that is not a number at all is still a configuration error.
        let mut vars = minimal();
        vars.insert("BUZZ_CSP_MAX_SESSIONS", "lots".into());
        assert!(matches!(load(&vars), Err(ConfigError::Invalid { .. })));
    }
}
