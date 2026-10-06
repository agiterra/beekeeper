//! The codex readiness version-gate tests.
//!
//! A child of `readiness_tests` so both halves stay under the desktop
//! file-size ratchet; `use super::*` keeps the shared helpers in scope.

use super::*;

// ── codex readiness version gate ───────────────────────────────────────

/// Build a minimal `KnownAcpRuntime` for testing the codex version gate.
/// `adapter_commands` are the exact strings passed to `find_command` — use
/// `&["codex-acp"]` when the binary is on PATH, or `&[<absolute_path>]`
/// when resolving via absolute path.  `underlying_cli` is a portable
/// stand-in so the adapter is not misclassified as `CliMissing`.
fn make_codex_runtime(
    adapter_commands: &'static [&'static str],
    underlying_cli: Option<&'static str>,
) -> KnownAcpRuntime {
    KnownAcpRuntime {
        id: "codex",
        label: "Codex",
        commands: adapter_commands,
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

/// Build a temp dir containing a `codex-acp` script with the given body,
/// prepend it to PATH, and clear the resolve cache.  Returns the temp dir
/// and the original PATH string for restoration.
#[cfg(unix)]
fn setup_temp_codex_acp(script_body: &str) -> (tempfile::TempDir, String) {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("create temp dir");
    let bin = dir.path().join("codex-acp");
    std::fs::write(&bin, script_body).expect("write script");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod script");

    let original_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.path().display(), original_path);
    std::env::set_var("PATH", &new_path);
    crate::managed_agents::clear_resolve_cache();

    (dir, original_path)
}

#[cfg(unix)]
fn leaked_adapter_commands(bin: &std::path::Path) -> &'static [&'static str] {
    let command = Box::leak(bin.display().to_string().into_boxed_str());
    Box::leak(vec![command as &'static str].into_boxed_slice())
}

/// Restore PATH and clear the resolve cache after a PATH-mutating test.
#[cfg(unix)]
fn restore_path(original: &str) {
    std::env::set_var("PATH", original);
    crate::managed_agents::clear_resolve_cache();
}

/// Codex readiness: outdated adapter (exits non-zero) → AdapterOutdated,
/// login probe skipped.
#[cfg(unix)]
#[test]
fn cli_login_requirements_codex_outdated_adapter_emits_adapter_outdated() {
    let _guard = crate::managed_agents::lock_path_mutex();

    let (dir, orig) = setup_temp_codex_acp("#!/bin/sh\nexit 1\n");
    let exe = present_binary_str();
    // Use the fixture's absolute adapter path here. Bare `codex-acp`
    // intentionally prefers Buzz's managed npm shim when it exists, which
    // would make this version-gate regression test depend on machine state.
    let rt = make_codex_runtime(
        leaked_adapter_commands(&dir.path().join("codex-acp")),
        Some(exe),
    );
    let reqs = cli_login::requirements(
        &[exe, "--beekeeper-probe-must-not-run-xyz"],
        "run `codex login`",
        &rt,
    );

    restore_path(&orig);
    drop(dir);

    assert!(
        !reqs.is_empty(),
        "outdated codex adapter must produce a requirement; got {reqs:?}"
    );
    if let Requirement::CliLogin {
        ref availability, ..
    } = reqs[0]
    {
        assert_eq!(
            *availability,
            crate::managed_agents::AcpAvailabilityStatus::AdapterOutdated,
            "0.x codex adapter must yield AdapterOutdated; got {availability:?}"
        );
    } else {
        panic!("expected CliLogin requirement; got {:?}", reqs[0]);
    }
}

/// Codex readiness: adapter exits 0 but output is not a parseable version
/// → AdapterOutdated (garbage output treated as outdated, same as non-zero).
#[cfg(unix)]
#[test]
fn cli_login_requirements_codex_garbage_version_output_emits_adapter_outdated() {
    let _guard = crate::managed_agents::lock_path_mutex();

    let (dir, orig) = setup_temp_codex_acp("#!/bin/sh\necho 'not a version string'\nexit 0\n");
    let exe = present_binary_str();
    let rt = make_codex_runtime(
        leaked_adapter_commands(&dir.path().join("codex-acp")),
        Some(exe),
    );
    let reqs = cli_login::requirements(
        &[exe, "--beekeeper-probe-must-not-run-xyz"],
        "run `codex login`",
        &rt,
    );

    restore_path(&orig);
    drop(dir);

    assert!(
        !reqs.is_empty(),
        "garbage version output must produce a requirement; got {reqs:?}"
    );
    if let Requirement::CliLogin {
        ref availability, ..
    } = reqs[0]
    {
        assert_eq!(
            *availability,
            crate::managed_agents::AcpAvailabilityStatus::AdapterOutdated,
            "unparseable version output must yield AdapterOutdated; got {availability:?}"
        );
    } else {
        panic!("expected CliLogin requirement; got {:?}", reqs[0]);
    }
}

// ── custom/unknown command ─────────────────────────────────────────────

#[test]
fn unknown_command_is_always_ready() {
    // Since Phase B-7 (readiness exec-check), unknown/custom commands that are
    // not resolvable in PATH produce a MissingBinary requirement rather than
    // being unconditionally Ready.  A command that IS resolvable should be Ready.
    // Use a known-present binary so the test is not environment-sensitive.
    let env = make_env("sh", BTreeMap::new());
    assert!(
        agent_readiness(&env).is_ready(),
        "unknown/custom command present in PATH should be Ready"
    );
}

#[test]
fn unknown_command_missing_from_path_is_not_ready() {
    let env = make_env("my-custom-harness-that-does-not-exist", BTreeMap::new());
    let readiness = agent_readiness(&env);
    assert!(
        !readiness.is_ready(),
        "unknown/custom command absent from PATH should be NotReady"
    );
    let reqs = readiness.requirements();
    assert_eq!(reqs.len(), 1);
    assert!(
        matches!(&reqs[0], Requirement::MissingBinary { command } if command == "my-custom-harness-that-does-not-exist"),
        "should surface MissingBinary requirement"
    );
}

// ── AgentReadiness helpers ─────────────────────────────────────────────

#[test]
fn agent_readiness_ready_has_empty_requirements() {
    assert!(AgentReadiness::Ready.requirements().is_empty());
}

#[test]
fn agent_readiness_not_ready_exposes_requirements() {
    let r = AgentReadiness::NotReady {
        requirements: vec![Requirement::EnvKey {
            key: "FOO".to_string(),
        }],
    };
    assert!(!r.is_ready());
    assert_eq!(r.requirements().len(), 1);
}

// ── Requirement serialization ─────────────────────────────────────────

#[test]
fn requirement_serializes_with_surface_tag() {
    let r = Requirement::NormalizedField {
        field: "provider".to_string(),
    };
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["surface"], "normalized_field");
    assert_eq!(json["field"], "provider");
}

#[test]
fn git_bash_requirement_serializes_correctly() {
    let json = serde_json::to_value(Requirement::GitBash).unwrap();
    assert_eq!(json, serde_json::json!({ "surface": "git_bash" }));
}

#[test]
fn env_key_requirement_serializes_correctly() {
    let r = Requirement::EnvKey {
        key: "ANTHROPIC_API_KEY".to_string(),
    };
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["surface"], "env_key");
    assert_eq!(json["key"], "ANTHROPIC_API_KEY");
}

#[test]
fn cli_login_requirement_serializes_correctly() {
    let r = Requirement::CliLogin {
        probe_args: vec![
            "codex".to_string(),
            "login".to_string(),
            "status".to_string(),
        ],
        setup_copy: "run `codex login`".to_string(),
        availability: crate::managed_agents::AcpAvailabilityStatus::Available,
    };
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["surface"], "cli_login");
    assert!(json["probe_args"].is_array());
    assert!(json["setup_copy"].as_str().unwrap().contains("codex login"));
}

// ── resolve_effective_agent_env ─────────────────────────────────────────

#[test]
fn resolve_effective_agent_env_user_env_wins_over_structured_fields() {
    // A record whose env_vars explicitly set provider/model must win over
    // any baked defaults. In OSS test builds the baked map is empty, so
    // this test validates the user-env layer is present in the output.
    let mut env_vars = BTreeMap::new();
    env_vars.insert("BUZZ_AGENT_PROVIDER".to_string(), "anthropic".to_string());
    env_vars.insert(
        "BUZZ_AGENT_MODEL".to_string(),
        "claude-opus-4-5".to_string(),
    );

    // Minimal record: only the fields resolve_effective_agent_env reads.
    let record = crate::managed_agents::types::ManagedAgentRecord {
        reserves_name_globally: false,
        pubkey: "test-pubkey".to_string(),
        name: "test-agent".to_string(),
        persona_id: None,
        private_key_nsec: String::new(),
        auth_tag: None,
        relay_url: String::new(),
        avatar_url: None,
        acp_command: "buzz-acp".to_string(),
        agent_command: "buzz-agent".to_string(),
        agent_command_override: None,
        agent_args: vec![],
        mcp_command: String::new(),
        turn_timeout_seconds: 320,
        idle_timeout_seconds: None,
        max_turn_duration_seconds: None,
        parallelism: 1,
        system_prompt: None,
        model: None,
        provider: None,
        persona_source_version: None,
        env_vars,
        start_on_app_launch: false,
        auto_restart_on_config_change: true,
        runtime_pid: None,
        backend: Default::default(),
        backend_agent_id: None,
        provider_policy_pending: false,
        provider_binary_path: None,
        team_id: None,
        persona_team_dir: None,
        persona_name_in_team: None,
        home_role: None,
        project_ref: None,
        project_public: None,
        carried_project_digest: None,
        project_publication_withdrawn: false,
        created_at: String::new(),
        updated_at: String::new(),
        last_started_at: None,
        last_stopped_at: None,
        last_exit_code: None,
        last_error: None,
        last_error_code: None,
        respond_to: Default::default(),
        respond_to_allowlist: vec![],
        display_name: None,
        slug: None,
        runtime: None,
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        definition_respond_to: None,
        definition_respond_to_allowlist: Vec::new(),
        definition_parallelism: None,
        relay_mesh: None,
    };

    let runtime = known_acp_runtime_exact("buzz-agent");
    let effective = resolve_effective_agent_env(&record, &[], runtime, &Default::default());

    // User env_vars must be present in the output (last-write-wins).
    assert_eq!(
        effective.env.get("BUZZ_AGENT_PROVIDER").map(String::as_str),
        Some("anthropic")
    );
    assert_eq!(
        effective.env.get("BUZZ_AGENT_MODEL").map(String::as_str),
        Some("claude-opus-4-5")
    );
}

#[test]
fn buzz_agent_databricks_v2_with_databricks_model_but_no_buzz_agent_model_is_ready() {
    // The baked buzz-releases env sets DATABRICKS_MODEL but not BUZZ_AGENT_MODEL.
    // An agent with only DATABRICKS_MODEL must pass the readiness gate.
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "databricks_v2"),
            ("DATABRICKS_MODEL", "goose-claude-4-6-sonnet"),
            ("DATABRICKS_HOST", "https://dbc.example.com"),
        ]),
    );
    assert!(
        agent_readiness(&env).is_ready(),
        "DATABRICKS_MODEL must satisfy the model requirement for databricks_v2"
    );
}

#[test]
fn buzz_agent_databricks_v2_hyphen_alias_with_databricks_model_is_ready() {
    // buzz-agent accepts both "databricks_v2" and "databricks-v2". The
    // readiness gate must recognize the hyphen alias and accept DATABRICKS_MODEL.
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "databricks-v2"),
            ("DATABRICKS_MODEL", "goose-claude-4-6-sonnet"),
            ("DATABRICKS_HOST", "https://dbc.example.com"),
        ]),
    );
    assert!(
        agent_readiness(&env).is_ready(),
        "databricks-v2 alias with DATABRICKS_MODEL must be Ready"
    );
}

#[test]
fn buzz_agent_databricks_hyphen_alias_missing_host_returns_not_ready() {
    // The hyphen alias "databricks-v2" requires DATABRICKS_HOST just like
    // the underscore variants. Without it the agent cannot reach the endpoint.
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "databricks-v2"),
            ("DATABRICKS_MODEL", "goose-claude-4-6-sonnet"),
            // DATABRICKS_HOST intentionally absent
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "databricks-v2 without DATABRICKS_HOST must be NotReady"
    );
    let reqs = result.requirements();
    assert!(
        reqs.iter()
            .any(|r| matches!(r, Requirement::EnvKey { key } if key == "DATABRICKS_HOST")),
        "missing requirements must include DATABRICKS_HOST; got {reqs:?}"
    );
}

#[test]
fn buzz_agent_databricks_v1_with_databricks_model_but_no_buzz_agent_model_is_ready() {
    // V1 (Model Serving) also resolves DATABRICKS_MODEL — same fallback applies.
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "databricks"),
            ("DATABRICKS_MODEL", "dbrx-instruct"),
            ("DATABRICKS_HOST", "https://dbc.example.com"),
        ]),
    );
    assert!(
        agent_readiness(&env).is_ready(),
        "DATABRICKS_MODEL must satisfy the model requirement for databricks (V1)"
    );
}

#[test]
fn buzz_agent_anthropic_with_anthropic_model_but_no_buzz_agent_model_is_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "anthropic"),
            ("ANTHROPIC_MODEL", "claude-opus-4-5"),
            ("ANTHROPIC_API_KEY", "sk-test"),
        ]),
    );
    assert!(
        agent_readiness(&env).is_ready(),
        "ANTHROPIC_MODEL must satisfy the model requirement for anthropic"
    );
}

#[test]
fn buzz_agent_openai_with_openai_compat_model_but_no_buzz_agent_model_is_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "openai"),
            ("OPENAI_COMPAT_MODEL", "gpt-4o"),
            ("OPENAI_COMPAT_API_KEY", "sk-test"),
        ]),
    );
    assert!(
        agent_readiness(&env).is_ready(),
        "OPENAI_COMPAT_MODEL must satisfy the model requirement for openai"
    );
}

#[test]
fn buzz_agent_empty_provider_model_fallback_key_is_not_ready() {
    // An empty DATABRICKS_MODEL with no BUZZ_AGENT_MODEL must still be NotReady.
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "databricks_v2"),
            ("DATABRICKS_MODEL", ""),
            ("DATABRICKS_HOST", "https://dbc.example.com"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        !result.is_ready(),
        "empty DATABRICKS_MODEL with no BUZZ_AGENT_MODEL must be NotReady"
    );
    assert!(result
        .requirements()
        .contains(&Requirement::NormalizedField {
            field: "model".to_string()
        }));
}

// ── OpenRouter readiness ─────────────────────────────────────────────

#[test]
fn buzz_agent_openrouter_with_all_fields_is_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "openrouter"),
            ("BUZZ_AGENT_MODEL", "anthropic/claude-sonnet-4"),
            ("OPENROUTER_API_KEY", "sk-or-test-key"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        result.is_ready(),
        "openrouter with all fields should be ready"
    );
}

#[test]
fn buzz_agent_openrouter_missing_key_returns_not_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "openrouter"),
            ("BUZZ_AGENT_MODEL", "anthropic/claude-sonnet-4"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(!result.is_ready());
    assert!(result.requirements().contains(&Requirement::EnvKey {
        key: "OPENROUTER_API_KEY".to_string()
    }));
}

#[test]
fn buzz_agent_openrouter_with_provider_model_fallback_is_ready() {
    let env = make_env(
        "buzz-agent",
        env_with(&[
            ("BUZZ_AGENT_PROVIDER", "openrouter"),
            ("OPENROUTER_MODEL", "google/gemini-2.5-flash"),
            ("OPENROUTER_API_KEY", "sk-or-test-key"),
        ]),
    );
    let result = agent_readiness(&env);
    assert!(
        result.is_ready(),
        "OPENROUTER_MODEL fallback should satisfy model requirement"
    );
}
