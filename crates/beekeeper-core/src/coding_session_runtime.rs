//! Runtime descriptors for the coding-session provider.
//!
//! The desktop host decides which agent runtimes the sidecar offers and hands
//! the whole list over in one environment variable, `BEEKEEPER_CSP_RUNTIMES` — a
//! JSON array of [`RuntimeDescriptor`]s. The definition lives here because
//! both sides of that contract (the Tauri host that writes it and the
//! `beekeeper-session-provider` sidecar that reads it) already depend on
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
    /// The model the host's auto-title one-shot asks this runtime for
    /// (SV-31). Tri-state on the wire:
    ///
    /// - key absent (`None`): the default — see [`RuntimeDescriptor::effective_title_model`];
    /// - explicit `null` (`Some(None)`): titling is disabled for this runtime;
    /// - a string (`Some(Some(model))`): that model.
    ///
    /// Omitted when absent, so a list an older sidecar parses stays parseable
    /// under `deny_unknown_fields`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_nullable"
    )]
    pub title_model: Option<Option<String>>,
    /// The environment variable names this runtime accepts from the restricted
    /// credentials file (`bee host commission --credential-file`).
    ///
    /// Empty means "the built-in defaults for this runtime word", which exist
    /// only for `claude` and `codex` (see
    /// [`RuntimeDescriptor::effective_credential_env`]). Any other runtime
    /// that is to be started with file custody has to say which variables it
    /// reads; a descriptor that names none and has no defaults is refused at
    /// adapter start with `MODEL_AUTH_UNAVAILABLE`, never started against an
    /// ambient login. Names only — the values live in the credentials file and
    /// never cross the wire. Omitted when empty, so an older sidecar still
    /// parses the list under `deny_unknown_fields`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_env: Vec<String>,
    /// An opaque operator label for where this runtime's credential comes
    /// from (`"acme-vault"`, `"ops-key-2026q4"`), echoed into session
    /// metadata (44223) and auth-failure receipts so a reader can tell which
    /// custody a failure is about. A label, never a secret and never a path.
    /// Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_source: Option<String>,
    /// Where this runtime's work executes. Absent means
    /// [`RuntimeExecution::Local`]: the work runs in the adapter's own process
    /// tree, inside whatever boundary the host put around it.
    /// [`RuntimeExecution::Remote`] declares that the adapter delegates the
    /// work to a process Beekeeper does not launch (a runtime's own server),
    /// so a boundary around the adapter does not surround the work and the
    /// transcript must not say it does. A declared fact about the installed
    /// adapter, set by whoever commissions it. Omitted when absent, so an
    /// older sidecar still parses the list under `deny_unknown_fields`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<RuntimeExecution>,
    /// Who owns the conversation context. Absent means
    /// [`RuntimeContext::Beekeeper`]: every new session starts the runtime in
    /// a fresh conversation, and only what Beekeeper hands it (a verified
    /// brief, a native resume) carries over. [`RuntimeContext::Runtime`]
    /// declares that the runtime keeps its own conversation across Beekeeper
    /// sessions (an adapter pinned to one thread of its own server), so a new
    /// session may carry context Beekeeper did not provide and cannot see,
    /// and the transcript must not call it fresh. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<RuntimeContext>,
}

/// Who owns a runtime's conversation context across Beekeeper sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeContext {
    /// Beekeeper: a new session is a new conversation. The default.
    Beekeeper,
    /// The runtime keeps its own conversation, which may outlive a session.
    Runtime,
}

/// Where a runtime's work executes relative to the adapter Beekeeper starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeExecution {
    /// In the adapter's own process tree. The default.
    Local,
    /// In a process the adapter talks to and Beekeeper does not launch.
    Remote,
}

/// The environment variables the `claude` runtime word accepts from the
/// credentials file when its descriptor names none.
pub const CLAUDE_CREDENTIAL_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

/// The environment variables the `codex` runtime word accepts from the
/// credentials file when its descriptor names none.
pub const CODEX_CREDENTIAL_ENV: &[&str] = &["OPENAI_API_KEY", "CODEX_API_KEY"];

/// Upper bound on `credentialEnv` entries per descriptor.
pub const MAX_CREDENTIAL_ENV: usize = 16;

/// The built-in credential variable list for a runtime word, if it has one.
///
/// Only `claude` and `codex` do; every other word must declare
/// `credentialEnv` on its descriptor.
pub fn default_credential_env(runtime: &str) -> Option<&'static [&'static str]> {
    match runtime {
        "claude" => Some(CLAUDE_CREDENTIAL_ENV),
        "codex" => Some(CODEX_CREDENTIAL_ENV),
        _ => None,
    }
}

/// Whether `name` is a well-formed environment variable name for
/// `credentialEnv`: ASCII letters, digits and underscores, not starting with
/// a digit, at most 64 bytes — and not one of the names
/// [`credential_env_name_rejection`] refuses.
pub fn is_valid_credential_env_name(name: &str) -> bool {
    credential_env_name_rejection(name).is_none()
}

/// Why `name` may not appear in `credentialEnv`, or `None` when it may.
///
/// A credentials file is loaded into the runtime's environment, so a name
/// that would redirect the loader, the dynamic linker or Beekeeper's own
/// seat variables is refused by name: `HOME`, `PATH`, and anything under
/// `BEEKEEPER_`, `BUZZ_`, `LD_` or `DYLD_`. The message is the one the
/// commission command and the descriptor parser both print.
pub fn credential_env_name_rejection(name: &str) -> Option<String> {
    let well_formed = !name.is_empty()
        && name.len() <= 64
        && !name.as_bytes()[0].is_ascii_digit()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if !well_formed {
        return Some(format!(
            "credentialEnv entry {name:?} is not an environment variable name"
        ));
    }
    if name == "HOME" || name == "PATH" {
        return Some(format!(
            "credentialEnv may not name {name}: it would redirect the runtime itself, not a credential"
        ));
    }
    for reserved in ["BEEKEEPER_", "BUZZ_", "LD_", "DYLD_"] {
        if name.starts_with(reserved) {
            return Some(format!(
                "credentialEnv may not name {name}: the {reserved}* namespace is reserved \
                 (seat identity, relay, and the dynamic linker are never credentials)"
            ));
        }
    }
    None
}

impl RuntimeDescriptor {
    /// The environment variable names this runtime accepts from the
    /// credentials file: the descriptor's own `credentialEnv` when it names
    /// any, else the built-in list for its runtime word, else `None` — which
    /// the credential loader turns into a refusal naming the fix.
    pub fn effective_credential_env(&self) -> Option<Vec<String>> {
        if !self.credential_env.is_empty() {
            return Some(self.credential_env.clone());
        }
        default_credential_env(&self.runtime)
            .map(|names| names.iter().map(|name| (*name).to_owned()).collect())
    }

    /// Whether this runtime's work executes outside the adapter Beekeeper
    /// launches ([`RuntimeExecution::Remote`]).
    pub fn executes_remotely(&self) -> bool {
        self.execution == Some(RuntimeExecution::Remote)
    }

    /// Whether the runtime keeps its own conversation context across
    /// Beekeeper sessions ([`RuntimeContext::Runtime`]).
    pub fn keeps_own_context(&self) -> bool {
        self.context == Some(RuntimeContext::Runtime)
    }

    /// The model the auto-title one-shot uses, or `None` when titling is
    /// disabled for this runtime.
    ///
    /// Absent `titleModel`: `haiku` for the `claude` runtime (the cheap model
    /// the Claude CLI always accepts as an alias), otherwise the runtime's
    /// `defaultModel`. An explicit `null` disables titling; a blank string is
    /// treated the same way rather than sent to an adapter as a model id.
    pub fn effective_title_model(&self) -> Option<String> {
        match &self.title_model {
            None if self.runtime == "claude" => Some("haiku".to_owned()),
            None => Some(self.default_model.clone()),
            Some(None) => None,
            Some(Some(model)) if model.trim().is_empty() => None,
            Some(Some(model)) => Some(model.trim().to_owned()),
        }
    }
}

/// Deserialize a key that is present into `Some(..)`, keeping an explicit
/// `null` distinct from absence (`#[serde(default)]` supplies the `None`).
fn deserialize_present_nullable<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
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

/// Parse and validate a `BEEKEEPER_CSP_RUNTIMES` JSON array.
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
        if descriptor.credential_env.len() > MAX_CREDENTIAL_ENV {
            return Err(format!(
                "credentialEnv must name at most {MAX_CREDENTIAL_ENV} variables (instanceRef {:?})",
                descriptor.instance_ref
            ));
        }
        for name in &descriptor.credential_env {
            if let Some(reason) = credential_env_name_rejection(name) {
                return Err(format!(
                    "{reason} (instanceRef {:?})",
                    descriptor.instance_ref
                ));
            }
        }
        if descriptor
            .credential_source
            .as_deref()
            .is_some_and(|label| label.trim().is_empty() || label.len() > 128)
        {
            return Err(format!(
                "credentialSource must be a non-empty label of at most 128 bytes \
                 (instanceRef {:?})",
                descriptor.instance_ref
            ));
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
        assert!(descriptor.credential_env.is_empty());
        assert!(descriptor.credential_source.is_none());
        // Known words keep their built-in credential lists when none is named.
        assert_eq!(
            descriptor.effective_credential_env().as_deref(),
            Some(
                &[
                    "ANTHROPIC_API_KEY".to_owned(),
                    "ANTHROPIC_AUTH_TOKEN".to_owned(),
                    "CLAUDE_CODE_OAUTH_TOKEN".to_owned()
                ][..]
            )
        );
        // ...and neither key is written back, so an older sidecar still parses it.
        let serialized = serde_json::to_value(descriptor).expect("serialize");
        let object = serialized.as_object().expect("object");
        assert!(!object.contains_key("credentialEnv"));
        assert!(!object.contains_key("credentialSource"));
    }

    /// `execution`: absent is local and is never written back; `remote` and
    /// `local` round-trip; anything else is refused.
    #[test]
    fn execution_defaults_to_local_and_round_trips_remote() {
        let parse = |entry: serde_json::Value| {
            parse_runtime_descriptors(&serde_json::json!([entry]).to_string())
        };
        let absent = parse(minimal("a-primary")).expect("parse");
        assert_eq!(absent[0].execution, None);
        assert!(!absent[0].executes_remotely());
        let serialized = serde_json::to_value(&absent[0]).expect("serialize");
        assert!(!serialized
            .as_object()
            .expect("object")
            .contains_key("execution"));

        let mut remote = minimal("t3-primary");
        remote["execution"] = serde_json::json!("remote");
        let parsed = parse(remote).expect("parse");
        assert_eq!(parsed[0].execution, Some(RuntimeExecution::Remote));
        assert!(parsed[0].executes_remotely());
        assert_eq!(
            serde_json::to_value(&parsed[0]).expect("ser")["execution"],
            "remote"
        );

        let mut local = minimal("l-primary");
        local["execution"] = serde_json::json!("local");
        assert!(!parse(local).expect("parse")[0].executes_remotely());

        let mut bogus = minimal("b-primary");
        bogus["execution"] = serde_json::json!("elsewhere");
        assert!(parse(bogus).is_err());
    }

    /// `context`: absent is Beekeeper-owned and never written back; `runtime`
    /// round-trips; anything else is refused.
    #[test]
    fn context_defaults_to_beekeeper_and_round_trips_runtime() {
        let parse = |entry: serde_json::Value| {
            parse_runtime_descriptors(&serde_json::json!([entry]).to_string())
        };
        let absent = parse(minimal("a-primary")).expect("parse");
        assert!(!absent[0].keeps_own_context());
        let serialized = serde_json::to_value(&absent[0]).expect("serialize");
        assert!(!serialized
            .as_object()
            .expect("object")
            .contains_key("context"));
        let mut runtime = minimal("t3-primary");
        runtime["context"] = serde_json::json!("runtime");
        let parsed = parse(runtime).expect("parse");
        assert!(parsed[0].keeps_own_context());
        assert_eq!(
            serde_json::to_value(&parsed[0]).expect("ser")["context"],
            "runtime"
        );
        let mut beekeeper = minimal("b-primary");
        beekeeper["context"] = serde_json::json!("beekeeper");
        assert!(!parse(beekeeper).expect("parse")[0].keeps_own_context());
        let mut bogus = minimal("x-primary");
        bogus["context"] = serde_json::json!("shared");
        assert!(parse(bogus).is_err());
    }

    /// The plug: any runtime word, with the variables it reads named on the
    /// descriptor and an operator label that is echoed but never a secret.
    #[test]
    fn credential_env_and_source_round_trip_and_are_validated() {
        let mut entry = minimal("acme-primary");
        entry["driver"] = serde_json::json!("acme-acp");
        entry["runtime"] = serde_json::json!("acme");
        entry["agentCommand"] = serde_json::json!("/opt/acme/bin/acme-acp");
        entry["credentialEnv"] = serde_json::json!(["ACME_TOKEN", "ACME_REGION"]);
        entry["credentialSource"] = serde_json::json!("acme-vault");
        let descriptors =
            parse_runtime_descriptors(&serde_json::json!([entry.clone()]).to_string())
                .expect("parse");
        let descriptor = &descriptors[0];
        assert_eq!(
            descriptor.credential_env,
            vec!["ACME_TOKEN".to_owned(), "ACME_REGION".to_owned()]
        );
        assert_eq!(descriptor.credential_source.as_deref(), Some("acme-vault"));
        assert_eq!(
            descriptor.effective_credential_env(),
            Some(vec!["ACME_TOKEN".to_owned(), "ACME_REGION".to_owned()])
        );
        let serialized = serde_json::to_value(descriptor).expect("serialize");
        assert_eq!(
            serialized["credentialEnv"],
            serde_json::json!(["ACME_TOKEN", "ACME_REGION"])
        );
        assert_eq!(serialized["credentialSource"], "acme-vault");
        assert_eq!(
            parse_runtime_descriptors(&serde_json::json!([serialized]).to_string())
                .expect("reparse"),
            descriptors
        );

        // An unknown word with nothing named has no list: the loader refuses.
        let mut bare = minimal("acme-primary");
        bare["runtime"] = serde_json::json!("acme");
        let bare =
            parse_runtime_descriptors(&serde_json::json!([bare]).to_string()).expect("parse");
        assert_eq!(bare[0].effective_credential_env(), None);

        // A descriptor's own list overrides a known word's defaults.
        let mut narrowed = minimal("claude-primary");
        narrowed["credentialEnv"] = serde_json::json!(["ANTHROPIC_API_KEY"]);
        let narrowed =
            parse_runtime_descriptors(&serde_json::json!([narrowed]).to_string()).expect("parse");
        assert_eq!(
            narrowed[0].effective_credential_env(),
            Some(vec!["ANTHROPIC_API_KEY".to_owned()])
        );

        // Names that are not environment variable names, and blank labels.
        for bad in ["", "1TOKEN", "ACME-TOKEN", "ACME TOKEN", "ACME=1"] {
            let mut entry = entry.clone();
            entry["credentialEnv"] = serde_json::json!([bad]);
            assert!(
                parse_runtime_descriptors(&serde_json::json!([entry]).to_string()).is_err(),
                "{bad:?} should be refused"
            );
        }
        // Well-formed names that would redirect the runtime, the linker or
        // the seat rather than carry a credential: refused by name, and the
        // reason says which namespace.
        for (reserved, expect) in [
            ("HOME", "would redirect the runtime"),
            ("PATH", "would redirect the runtime"),
            ("BEEKEEPER_PRIVATE_KEY", "BEEKEEPER_* namespace is reserved"),
            ("BUZZ_AUTH_TAG", "BUZZ_* namespace is reserved"),
            ("LD_PRELOAD", "LD_* namespace is reserved"),
            ("DYLD_INSERT_LIBRARIES", "DYLD_* namespace is reserved"),
        ] {
            assert!(!is_valid_credential_env_name(reserved), "{reserved}");
            let reason = credential_env_name_rejection(reserved).expect(reserved);
            assert!(reason.contains(expect), "{reserved}: {reason}");
            let mut entry = entry.clone();
            entry["credentialEnv"] = serde_json::json!([reserved]);
            let error = parse_runtime_descriptors(&serde_json::json!([entry]).to_string())
                .expect_err(reserved);
            assert!(error.contains(expect), "{reserved}: {error}");
        }
        // Prefix-shaped names that are not in the namespaces stay allowed.
        for fine in ["HOMEPAGE_TOKEN", "PATHWAY_KEY", "LDAP_PASSWORD", "BUZZWORD"] {
            assert!(is_valid_credential_env_name(fine), "{fine}");
        }
        let mut blank = entry.clone();
        blank["credentialSource"] = serde_json::json!("   ");
        assert!(parse_runtime_descriptors(&serde_json::json!([blank]).to_string()).is_err());
        let mut many = entry;
        many["credentialEnv"] = serde_json::json!((0..MAX_CREDENTIAL_ENV + 1)
            .map(|index| format!("V{index}"))
            .collect::<Vec<_>>());
        assert!(parse_runtime_descriptors(&serde_json::json!([many]).to_string()).is_err());
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
    /// to survive the trip through `BEEKEEPER_CSP_RUNTIMES` byte-for-byte — and
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

    /// `titleModel` is tri-state: absent means the default (haiku for
    /// claude, otherwise the default model), `null` disables titling, a
    /// string is the model. Absence survives a round trip as omission.
    #[test]
    fn the_title_model_is_tri_state_and_omitted_when_absent() {
        let parse = |entry: serde_json::Value| {
            parse_runtime_descriptors(&serde_json::json!([entry]).to_string()).expect("parse")[0]
                .clone()
        };
        let claude = parse(minimal("claude-primary"));
        assert_eq!(claude.title_model, None);
        assert_eq!(claude.effective_title_model().as_deref(), Some("haiku"));
        let serialized = serde_json::to_value(&claude).expect("serialize");
        assert!(!serialized
            .as_object()
            .expect("object")
            .contains_key("titleModel"));

        let mut codex = minimal("codex-primary");
        codex["runtime"] = serde_json::json!("codex");
        codex["defaultModel"] = serde_json::json!("gpt-5.1-codex-mini");
        assert_eq!(
            parse(codex).effective_title_model().as_deref(),
            Some("gpt-5.1-codex-mini")
        );

        let mut disabled = minimal("claude-primary");
        disabled["titleModel"] = serde_json::Value::Null;
        let disabled = parse(disabled);
        assert_eq!(disabled.title_model, Some(None));
        assert_eq!(disabled.effective_title_model(), None);
        let serialized = serde_json::to_value(&disabled).expect("serialize");
        assert_eq!(serialized["titleModel"], serde_json::Value::Null);
        assert!(serialized
            .as_object()
            .expect("object")
            .contains_key("titleModel"));
        assert_eq!(parse(serialized).effective_title_model(), None);

        let mut chosen = minimal("claude-primary");
        chosen["titleModel"] = serde_json::json!("sonnet");
        assert_eq!(
            parse(chosen).effective_title_model().as_deref(),
            Some("sonnet")
        );

        let mut blank = minimal("claude-primary");
        blank["titleModel"] = serde_json::json!("  ");
        assert_eq!(parse(blank).effective_title_model(), None);
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
