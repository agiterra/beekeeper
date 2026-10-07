// Canonical reserved-env-key list, `include!`d into BOTH `build.rs`
// (compile-time rejection of baked `BEEKEEPER_BUILD_AGENT_ENV` collisions) and
// `managed_agents/env_vars.rs` (save-time validation and spawn-time
// filtering). Build scripts cannot import from the crate, so sharing the
// source via `include!` is what guarantees the build-time check and the
// runtime filter use one identical list — zero drift surface. See
// `commands/reconnect_hook_config.rs` for the same pattern.
//
// Keep this file dependency-free: no crate-internal imports, no external
// crates. Both consumers compile it as-is.

/// Env var keys that Beekeeper sets itself and users must not override from
/// the persona/agent env_vars UI. Three categories:
///
/// 1. **Identity / secrets** — overriding would swap the agent's nsec or
///    leak credentials.
/// 2. **Code-execution surface** — overriding the binary/args lets the
///    user run arbitrary code as the agent process.
/// 3. **Security gates** — overriding the respond-to mode/allowlist or
///    relay URL would silently break the saved security settings (the UI
///    shows owner-only while the running agent answers anyone, for
///    example), or redirect the agent to an attacker-controlled relay.
///
/// Every `BEEKEEPER_*` name is listed with its pre-rename `BUZZ_*` twin
/// beside it. Children adopt a `BUZZ_<X>` as `BEEKEEPER_<X>` when the new name
/// is unset (`beekeeper_core::env_compat`), so a user-supplied
/// `BUZZ_PRIVATE_KEY` is exactly as dangerous as `BEEKEEPER_PRIVATE_KEY`. The
/// list is matched exactly (ignoring ASCII case), so both spellings must be
/// written out here: this file cannot call `env_compat::both_spellings`.
///
/// This list is deliberately narrow — it only covers keys with security
/// implications. Behavior knobs (GOOSE_MODE, BEEKEEPER_ACP_MODEL, BEEKEEPER_ACP_SYSTEM_PROMPT, …) remain freely
/// overridable; those have dedicated UI fields but power users may want
/// to bypass them.
pub(crate) const RESERVED_ENV_KEYS: &[&str] = &[
    // Identity / secrets.
    "BEEKEEPER_PRIVATE_KEY",
    "BUZZ_PRIVATE_KEY",
    "NOSTR_PRIVATE_KEY",
    "BEEKEEPER_AUTH_TAG",
    "BUZZ_AUTH_TAG",
    "BEEKEEPER_API_TOKEN",
    "BUZZ_API_TOKEN",
    "BEEKEEPER_ACP_PRIVATE_KEY",
    "BUZZ_ACP_PRIVATE_KEY",
    "BEEKEEPER_ACP_API_TOKEN",
    "BUZZ_ACP_API_TOKEN",
    // Relay URL: overriding would let a malicious config redirect the
    // agent to an attacker-controlled relay.
    "BEEKEEPER_RELAY_URL",
    "BUZZ_RELAY_URL",
    // Code-execution surface: overriding would let the user run arbitrary
    // binaries/args as the agent process.
    "BEEKEEPER_ACP_AGENT_COMMAND",
    "BUZZ_ACP_AGENT_COMMAND",
    "BEEKEEPER_ACP_AGENT_ARGS",
    "BUZZ_ACP_AGENT_ARGS",
    "BEEKEEPER_ACP_MCP_COMMAND",
    "BUZZ_ACP_MCP_COMMAND",
    // Control-plane parallelism: the Desktop resolves the effective
    // worker-pool size (applying any per-harness cap) and writes it into
    // launch.policy_env. A user-supplied BEEKEEPER_ACP_AGENTS would bypass the
    // harness cap and cause OpenClaw agents to spawn uncapped workers.
    "BEEKEEPER_ACP_AGENTS",
    "BUZZ_ACP_AGENTS",
    // Security gates: respond-to mode + allowlist + deployment allowlist +
    // legacy owner-only fallback. Overriding would make the running agent's
    // gate diverge from the saved/UI-visible settings.
    "BEEKEEPER_ACP_RESPOND_TO",
    "BUZZ_ACP_RESPOND_TO",
    "BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST",
    "BUZZ_ACP_RESPOND_TO_ALLOWLIST",
    "BEEKEEPER_ACP_ALLOWED_RESPOND_TO",
    "BUZZ_ACP_ALLOWED_RESPOND_TO",
    "BEEKEEPER_ACP_AGENT_OWNER",
    "BUZZ_ACP_AGENT_OWNER",
    // Stable agent identity used for git attribution and private-conversation
    // provenance must come from the managed-agent record, not user overrides.
    "BEEKEEPER_ACP_DISPLAY_NAME",
    "BUZZ_ACP_DISPLAY_NAME",
    // Remote lifetime/presence policy: user env must not disable the
    // desktop/provider-owned bounds while the saved record still promises them.
    "BEEKEEPER_ACP_EXIT_AFTER_INACTIVITY",
    "BUZZ_ACP_EXIT_AFTER_INACTIVITY",
    // Desktop-owned pool lifetime policy: user env must not disable or reset
    // the idle worker-reclamation window while the desktop launcher sets it.
    "BEEKEEPER_ACP_IDLE_POOL_SLEEP",
    "BUZZ_ACP_IDLE_POOL_SLEEP",
    // Desktop experiment policy: the Settings toggle is the sole authority
    // for whether channel threads receive independent ACP sessions.
    "BEEKEEPER_ACP_SESSION_POLICY",
    "BUZZ_ACP_SESSION_POLICY",
    "BEEKEEPER_ACP_NO_PRESENCE",
    "BUZZ_ACP_NO_PRESENCE",
    // Readiness handoff: desktop is the ONLY readiness source. A saved or
    // ambient env var must not be able to forge setup mode (NotReady) on a
    // Ready agent or suppress it (empty/stale payload) on a NotReady one.
    "BEEKEEPER_ACP_SETUP_PAYLOAD",
    "BUZZ_ACP_SETUP_PAYLOAD",
    // Desktop ownership markers: these brand every spawned harness with the
    // launching Desktop instance. A user-supplied override would let a
    // definition masquerade as a different instance or fake the nonce used
    // for same-session sweep decisions.
    "BEEKEEPER_MANAGED_AGENT",
    "BUZZ_MANAGED_AGENT",
    "BEEKEEPER_MANAGED_AGENT_START_NONCE",
    "BUZZ_MANAGED_AGENT_START_NONCE",
];

pub(crate) fn is_reserved_env_key(key: &str) -> bool {
    RESERVED_ENV_KEYS
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(key))
}
