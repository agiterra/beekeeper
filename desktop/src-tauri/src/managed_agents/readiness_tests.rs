//! Requirement/readiness unit tests for `readiness.rs`.
//!
//! In their own file so the parent module stays under the desktop
//! file-size ratchet — the same reason
//! `readiness_goose_file_config_tests.rs` exists.

use std::collections::BTreeMap;

use super::*;
use crate::managed_agents::discovery::known_acp_runtime_exact;

/// Build a minimal `EffectiveAgentEnv` with the given env map and command.
fn make_env(command: &str, env: BTreeMap<String, String>) -> EffectiveAgentEnv {
    let runtime = known_acp_runtime_exact(command);
    EffectiveAgentEnv {
        env,
        config_file_path: runtime.and_then(|r| r.config_file_path),
        effective_command: command.to_string(),
    }
}

fn env_with(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ── buzz-agent tests ──────────────────────────────────────────────────

#[test]
fn beekeeper_agent_missing_provider_returns_not_ready_with_normalized_field() {
    let env = make_env(
        "buzz-agent",
        env_with(&[("BEEKEEPER_AGENT_MODEL", "claude-opus-4-5")]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "missing BEEKEEPER_AGENT_PROVIDER should be NotReady"
    );
    let reqs = result.requirements();
    assert!(
        reqs.contains(&Requirement::NormalizedField {
            field: "provider".to_string()
        }),
        "requirements should include NormalizedField(provider); got {reqs:?}"
    );
}

#[test]
fn beekeeper_agent_missing_model_returns_not_ready_with_normalized_field() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "anthropic"),
            ("ANTHROPIC_API_KEY", "sk-test"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(!result.is_ready());
    assert!(result
        .requirements()
        .contains(&Requirement::NormalizedField {
            field: "model".to_string()
        }));
}

#[test]
fn beekeeper_agent_missing_anthropic_key_returns_not_ready_with_env_key() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "anthropic"),
            ("BEEKEEPER_AGENT_MODEL", "claude-opus-4-5"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(!result.is_ready());
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "ANTHROPIC_API_KEY".to_string()
    }));
}

#[test]
fn beekeeper_agent_missing_openai_key_returns_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "openai"),
            ("BEEKEEPER_AGENT_MODEL", "gpt-4o"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(!result.is_ready());
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "OPENAI_COMPAT_API_KEY".to_string()
    }));
}

#[test]
fn beekeeper_agent_anthropic_with_all_fields_is_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "anthropic"),
            ("BEEKEEPER_AGENT_MODEL", "claude-opus-4-5"),
            ("ANTHROPIC_API_KEY", "sk-test"),
        ]),
    );
    assert!(agent_readiness(&env).is_ready());
}

#[test]
fn beekeeper_agent_databricks_with_host_and_model_is_ready_without_token() {
    // DATABRICKS_TOKEN is NOT required — OAuth PKCE is the normal path.
    // No token present, no OAuth cache present → still Ready because we
    // cannot evaluate OAuth state from the env map alone.
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "databricks"),
            ("BEEKEEPER_AGENT_MODEL", "dbrx-instruct"),
            ("DATABRICKS_HOST", "https://dbc.example.com"),
            // NOTE: no DATABRICKS_TOKEN
        ]),
    );
    assert!(
        agent_readiness(&env).is_ready(),
        "Databricks with HOST+model but no TOKEN should still be Ready (OAuth path)"
    );
}

#[test]
fn beekeeper_agent_databricks_missing_host_returns_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "databricks"),
            ("BEEKEEPER_AGENT_MODEL", "dbrx-instruct"),
            // NOTE: no DATABRICKS_HOST
        ]),
    );
    let result = agent_readiness(&env);
    assert!(!result.is_ready());
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "DATABRICKS_HOST".to_string()
    }));
}

#[test]
fn beekeeper_agent_databricks_v2_missing_host_returns_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "databricks_v2"),
            (
                "BEEKEEPER_AGENT_MODEL",
                "databricks/meta-llama-4-maverick-17b-instruct",
            ),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(!result.is_ready());
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "DATABRICKS_HOST".to_string()
    }));
}

// ── goose tests ───────────────────────────────────────────────────────

#[test]
fn goose_missing_provider_returns_not_ready() {
    // Call goose_requirements directly with None file config so the test is
    // deterministic — the `agent_readiness` path reads the real
    // ~/.config/goose/config.yaml which may silence requirements on
    // developer machines.
    let env = make_env("goose", env_with(&[("GOOSE_MODEL", "claude-opus-4-5")]));
    let reqs = goose_requirements(&env, None);
    assert!(
        !reqs.is_empty(),
        "missing GOOSE_PROVIDER with no file config must produce requirements"
    );
    assert!(
        reqs.contains(&Requirement::NormalizedField {
            field: "provider".to_string()
        }),
        "requirements must include NormalizedField(provider); got {reqs:?}"
    );
}

#[test]
fn goose_with_provider_and_model_and_key_is_ready() {
    let env = make_env(
        "goose",
        env_with(&[
            ("GOOSE_PROVIDER", "anthropic"),
            ("GOOSE_MODEL", "claude-opus-4-5"),
            ("ANTHROPIC_API_KEY", "sk-test"),
        ]),
    );
    assert!(agent_readiness(&env).is_ready());
}

// ── empty-string semantics ────────────────────────────────────────────
//
// A key present with an empty value ("") must be treated as MISSING, to
// match the dialog's (envVars[key] ?? "").length === 0 emptiness check.

#[test]
fn beekeeper_agent_empty_string_provider_is_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", ""),
            ("BEEKEEPER_AGENT_MODEL", "claude-opus-4-5"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "empty-string BEEKEEPER_AGENT_PROVIDER must be treated as missing"
    );
    assert!(result
        .requirements()
        .contains(&Requirement::NormalizedField {
            field: "provider".to_string()
        }));
}

#[test]
fn beekeeper_agent_empty_string_model_is_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "anthropic"),
            ("BEEKEEPER_AGENT_MODEL", ""),
            ("ANTHROPIC_API_KEY", "sk-test"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "empty-string BEEKEEPER_AGENT_MODEL must be treated as missing"
    );
    assert!(result
        .requirements()
        .contains(&Requirement::NormalizedField {
            field: "model".to_string()
        }));
}

#[test]
fn beekeeper_agent_empty_string_anthropic_key_is_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "anthropic"),
            ("BEEKEEPER_AGENT_MODEL", "claude-opus-4-5"),
            ("ANTHROPIC_API_KEY", ""),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "empty-string ANTHROPIC_API_KEY must be treated as missing"
    );
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "ANTHROPIC_API_KEY".to_string()
    }));
}

#[test]
fn beekeeper_agent_empty_string_databricks_host_is_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BEEKEEPER_AGENT_PROVIDER", "databricks"),
            ("BEEKEEPER_AGENT_MODEL", "dbrx-instruct"),
            ("DATABRICKS_HOST", ""),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "empty-string DATABRICKS_HOST must be treated as missing"
    );
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "DATABRICKS_HOST".to_string()
    }));
}

#[test]
fn goose_empty_string_provider_is_not_ready() {
    // Call goose_requirements directly with None file config so the test is
    // deterministic — the `agent_readiness` path reads the real
    // ~/.config/goose/config.yaml which may silence requirements on
    // developer machines.
    let env = make_env(
        "goose",
        env_with(&[("GOOSE_PROVIDER", ""), ("GOOSE_MODEL", "claude-opus-4-5")]),
    );
    let reqs = goose_requirements(&env, None);
    assert!(
        !reqs.is_empty(),
        "empty-string GOOSE_PROVIDER must be treated as missing"
    );
    assert!(
        reqs.contains(&Requirement::NormalizedField {
            field: "provider".to_string()
        }),
        "requirements must include NormalizedField(provider); got {reqs:?}"
    );
}

#[test]
fn goose_empty_string_anthropic_key_is_not_ready() {
    // Call goose_requirements directly with None file config so the test is
    // deterministic — the `agent_readiness` path reads the real
    // ~/.config/goose/config.yaml which may silence requirements on
    // developer machines.
    let env = make_env(
        "goose",
        env_with(&[
            ("GOOSE_PROVIDER", "anthropic"),
            ("GOOSE_MODEL", "claude-opus-4-5"),
            ("ANTHROPIC_API_KEY", ""),
        ]),
    );
    let reqs = goose_requirements(&env, None);
    assert!(
        !reqs.is_empty(),
        "empty-string ANTHROPIC_API_KEY must be treated as missing (goose)"
    );
    assert!(
        reqs.contains(&Requirement::EnvKey {
            key: "ANTHROPIC_API_KEY".to_string()
        }),
        "requirements must include ANTHROPIC_API_KEY; got {reqs:?}"
    );
}

// ── codex tests ───────────────────────────────────────────────────────

#[test]
fn codex_not_ready_copy_does_not_mention_openai_api_key() {
    // codex uses its own credential store via `codex login` (OAuth or API key).
    // The nudge copy must NOT say "set OPENAI_API_KEY".
    // Use a not-installed runtime so the requirement is always emitted
    // regardless of whether codex is on the test machine's PATH.
    let rt = make_cli_runtime(&["__buzz_nonexistent_adapter_xyz789__"], None);
    let reqs = cli_login::requirements(&["codex", "login", "status"], "run `codex login`", &rt);
    // Whether codex is installed or not, the copy (if any) must not mention OPENAI_API_KEY.
    for req in &reqs {
        if let Requirement::CliLogin { setup_copy, .. } = req {
            assert!(
                !setup_copy.contains("OPENAI_API_KEY"),
                "codex nudge copy must not mention OPENAI_API_KEY; got: {setup_copy:?}"
            );
            assert!(
                setup_copy.contains("codex login"),
                "codex nudge copy should mention `codex login`; got: {setup_copy:?}"
            );
        }
    }
}

// ── cli_login_requirements: resolve_command integration ─────────────

/// Construct a minimal `KnownAcpRuntime` stub for testing cli_login_requirements.
/// `commands` are the adapter binaries; `underlying_cli` is the CLI name.
fn make_cli_runtime(
    commands: &'static [&'static str],
    underlying_cli: Option<&'static str>,
) -> KnownAcpRuntime {
    KnownAcpRuntime {
        id: "test-cli-runtime",
        label: "Test CLI",
        commands,
        aliases: &[],
        avatar_url: "",
        mcp_command: None,
        mcp_hooks: false,
        underlying_cli,
        cli_install_commands: &[],
        cli_install_commands_windows: &[],
        adapter_install_commands: &[],
        cli_install_instructions_url: "",
        adapter_install_instructions_url: "",
        cli_install_hint: "",
        adapter_install_hint: "",
        skill_dir: None,
        supports_acp_model_switching: false,
        config_file_path: None,
        config_file_format: None,
        model_env_var: None,
        provider_env_var: None,
        provider_locked: false,
        default_env: &[],
        supports_acp_native_config: false,
        thinking_env_var: None,
        max_tokens_env_var: None,
        context_limit_env_var: None,
        max_rounds_env_var: None,
        required_normalized_fields: &[],
        login_hint: None,
        auth_probe_args: None,
    }
}

/// Returns the absolute path of the currently-running test binary as a `&'static str`.
/// Host-portable stand-in for a "present" binary: absolute path so `find_command` resolves
/// it via `path.exists()`. Leaked allocation is intentional — process exits after tests.
fn present_binary_str() -> &'static str {
    let path = std::env::current_exe().expect("current_exe must be available in tests");
    Box::leak(path.to_string_lossy().into_owned().into_boxed_str())
}

/// Leak a runtime slice of `'static` strs for use in `make_cli_runtime`.
fn static_commands(commands: Vec<&'static str>) -> &'static [&'static str] {
    Box::leak(commands.into_boxed_slice())
}

#[test]
fn cli_login_requirements_missing_binary_is_not_ready() {
    // Both adapter and underlying CLI are nonexistent → NotInstalled state
    // → must return a CliLogin requirement with availability=NotInstalled.
    let rt = make_cli_runtime(
        &["__buzz_nonexistent_adapter_abc123__"],
        Some("__buzz_nonexistent_cli_abc123__"),
    );
    let reqs = cli_login::requirements(
        &["__buzz_nonexistent_binary_abc123__", "status"],
        "install the tool first",
        &rt,
    );
    assert!(
        !reqs.is_empty(),
        "missing binary must produce a CliLogin requirement (NotReady)"
    );
    assert!(
        matches!(reqs[0], Requirement::CliLogin { .. }),
        "requirement must be CliLogin; got {:?}",
        reqs[0]
    );
    if let Requirement::CliLogin {
        ref availability, ..
    } = reqs[0]
    {
        assert_eq!(
            *availability,
            crate::managed_agents::AcpAvailabilityStatus::NotInstalled,
            "both missing → NotInstalled"
        );
    }
}

#[test]
fn cli_login_requirements_adapter_missing_emits_adapter_missing() {
    // Underlying CLI present (use the running test binary as a portable
    // stand-in — it's always present and resolves via absolute path),
    // adapter absent.
    // → AdapterMissing state → no probe run → CliLogin{AdapterMissing}.
    let exe = present_binary_str();
    let rt = make_cli_runtime(&["__buzz_nonexistent_adapter_xyz789__"], Some(exe));
    let reqs = cli_login::requirements(&[exe, "--list"], "install the adapter", &rt);
    assert!(
        !reqs.is_empty(),
        "adapter missing must produce a CliLogin requirement"
    );
    if let Requirement::CliLogin {
        ref availability, ..
    } = reqs[0]
    {
        assert_eq!(
            *availability,
            crate::managed_agents::AcpAvailabilityStatus::AdapterMissing,
            "adapter absent, CLI present → AdapterMissing"
        );
    }
}

#[test]
fn cli_login_requirements_cli_missing_emits_cli_missing() {
    // Adapter present (use the running test binary as a portable stand-in),
    // underlying CLI absent.
    // → CliMissing state → no probe run → CliLogin{CliMissing}.
    let exe = present_binary_str();
    let rt = make_cli_runtime(
        static_commands(vec![exe]),              // adapter found via absolute path
        Some("__buzz_nonexistent_cli_abc123__"), // underlying CLI missing
    );
    let reqs = cli_login::requirements(&[exe, "--list"], "install the CLI", &rt);
    assert!(
        !reqs.is_empty(),
        "CLI missing must produce a CliLogin requirement"
    );
    if let Requirement::CliLogin {
        ref availability, ..
    } = reqs[0]
    {
        assert_eq!(
            *availability,
            crate::managed_agents::AcpAvailabilityStatus::CliMissing,
            "adapter present, CLI absent → CliMissing"
        );
    }
}

#[test]
fn cli_login_requirements_resolvable_binary_runs_probe_at_resolved_path() {
    // Both adapter and CLI present (use the running test binary as a
    // portable stand-in — always present, resolves via absolute path),
    // probe exits 0 (run with `--list` which lists tests and exits 0).
    // → logged_in = true → requirements is empty (Ready).
    let exe = present_binary_str();
    let rt = make_cli_runtime(static_commands(vec![exe]), Some(exe));
    let reqs = cli_login::requirements(
        &[exe, "--list"],
        "this should not show (probe exits 0)",
        &rt,
    );
    assert!(
        reqs.is_empty(),
        "expected Ready (no requirements) when probe binary resolves and exits 0; \
         got {:?}",
        reqs
    );
}

#[test]
fn cli_login_requirements_logged_out_emits_available() {
    // Both adapter and CLI present, but probe exits non-zero (logged out).
    // Use the test binary with an unrecognized argument as the probe —
    // libtest exits non-zero for unknown flags on all platforms.
    // → CliLogin{Available} (tooling installed, needs login).
    let exe = present_binary_str();
    let rt = make_cli_runtime(static_commands(vec![exe]), Some(exe));
    let reqs = cli_login::requirements(
        &[exe, "--beekeeper-probe-fail-xyz"],
        "run `tool login`",
        &rt,
    );
    assert!(
        !reqs.is_empty(),
        "non-zero probe must produce a CliLogin requirement (logged out)"
    );
    if let Requirement::CliLogin {
        ref availability, ..
    } = reqs[0]
    {
        assert_eq!(
            *availability,
            crate::managed_agents::AcpAvailabilityStatus::Available,
            "tooling installed, probe fails → Available (logged-out)"
        );
    }
}

// The codex version-gate tests live in a child file so both halves stay
// under the desktop file-size ratchet.
#[path = "readiness_codex_tests.rs"]
mod codex;
