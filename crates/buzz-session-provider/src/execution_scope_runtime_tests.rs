use super::*;

fn scratch() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let state = root.join("state");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&state).expect("state");
    (dir, state, home)
}

/// A disposable selected login: the fixture home's Codex file store, and
/// whether an API key is configured. Never the person's own, never the
/// process environment.
fn fixture_login(home: &Path, api_key: bool) -> CodexLogin {
    CodexLogin {
        operator_home: home.join(".codex"),
        api_key,
    }
}

#[cfg(unix)]
#[test]
fn codex_home_links_the_one_file_login_and_refuses_anything_else() {
    let (_dir, state, home) = scratch();
    let login = fixture_login(&home, false);
    // No login file and no key: said as such, without prescribing a sign-in.
    let error = prepare_codex_home(&state, &login).err().expect("no login");
    assert_eq!(error.code, EXECUTION_BOUNDARY_UNAVAILABLE);
    assert!(error.message.contains("API key"), "{}", error.message);
    // A keyring store is refused as unsupported, not silently lost.
    std::fs::create_dir_all(home.join(".codex")).expect("codex");
    std::fs::write(home.join(".codex/auth.json"), "{\"auth_mode\":\"test\"}").expect("auth");
    std::fs::write(
        home.join(".codex/config.toml"),
        "cli_auth_credentials_store = \"keyring\"\n",
    )
    .expect("config");
    let error = prepare_codex_home(&state, &login).err().expect("keyring");
    assert!(error.message.contains("keyring"), "{}", error.message);
    // File store: a private home whose auth.json links to the real file.
    std::fs::write(home.join(".codex/config.toml"), "model = \"x\"\n").expect("config");
    let prepared = prepare_codex_home(&state, &login).expect("file store");
    let link = prepared.home.join("auth.json");
    let real = home.join(".codex/auth.json").canonicalize().expect("real");
    assert_eq!(std::fs::read_link(&link).expect("link"), real);
    assert!(prepared.grants.iter().any(|g| g.target
        == buzz_acp::exec_boundary::GrantTarget::File(real.clone())
        && g.access == Access::ReadWrite));
    assert!(prepared.grants.iter().any(|g| g.target
        == buzz_acp::exec_boundary::GrantTarget::File(link.clone())
        && g.access == Access::NoUnlink));
    let config = std::fs::read_to_string(prepared.home.join("config.toml")).expect("config");
    assert!(
        !config.contains("model = \"x\""),
        "the operator's config is never copied"
    );
    // Idempotent on resume; a replaced link (a copy) is refused.
    prepare_codex_home(&state, &login).expect("again");
    std::fs::remove_file(&link).expect("rm");
    std::fs::write(&link, "{}").expect("copy");
    let error = prepare_codex_home(&state, &login)
        .err()
        .expect("copy refused");
    assert!(error.message.contains("copy"), "{}", error.message);
}

#[test]
fn a_configured_codex_api_key_needs_no_login_file() {
    let (_dir, state, home) = scratch();
    // A key the runtime descriptor sets selects the API-key route.
    let key = vec![("OPENAI_API_KEY".to_owned(), "sk-test-not-real".to_owned())];
    assert!(CodexLogin::from_host(&home, &key).api_key);
    let prepared = prepare_codex_home(&state, &fixture_login(&home, true)).expect("API-key route");
    assert!(
        !prepared.home.join("auth.json").exists(),
        "no login file is linked"
    );
}

#[cfg(unix)]
#[test]
fn a_child_replaced_codex_home_or_config_is_never_followed() {
    let (_dir, state, home) = scratch();
    let login = fixture_login(&home, false);
    std::fs::create_dir_all(home.join(".codex")).expect("codex");
    std::fs::write(home.join(".codex/auth.json"), "{}").expect("auth");
    let foreign = state.parent().expect("root").join("foreign");
    std::fs::create_dir_all(&foreign).expect("foreign");
    std::fs::write(foreign.join("canary"), "FOREIGN_CANARY").expect("canary");
    // The config file replaced by a link to a foreign file: rewritten in
    // place, the foreign file untouched.
    let prepared = prepare_codex_home(&state, &login).expect("first");
    let config = prepared.home.join("config.toml");
    std::fs::remove_file(&config).expect("rm");
    std::os::unix::fs::symlink(foreign.join("canary"), &config).expect("link");
    prepare_codex_home(&state, &login).expect("resume");
    assert_eq!(
        std::fs::read_to_string(foreign.join("canary")).expect("canary"),
        "FOREIGN_CANARY"
    );
    assert!(!std::fs::symlink_metadata(&config)
        .expect("config")
        .file_type()
        .is_symlink());
    // The whole home replaced by a link: refused, never granted its target.
    std::fs::remove_dir_all(&prepared.home).expect("rm home");
    std::os::unix::fs::symlink(&foreign, &prepared.home).expect("home link");
    let error = prepare_codex_home(&state, &login)
        .err()
        .expect("replaced home");
    assert_eq!(error.code, crate::execution_scope::EXECUTION_SCOPE_INVALID);
}

#[cfg(unix)]
#[test]
fn the_pinned_settings_hold_scope_and_identity_privately_and_follow_no_link() {
    use buzz_acp::exec_env::ModelAuth;
    use std::os::unix::fs::PermissionsExt;
    let (_dir, state, _home) = scratch();
    let fence = crate::agent_fence::FENCE;
    let mut env = ResolvedEnv::baseline(
        [
            ("PATH".into(), "/usr/bin".into()),
            ("HOME".into(), "/h".into()),
        ],
        ModelAuth::None,
        &fence,
        &|_| true,
    );
    env.scope("GIT_CONFIG_GLOBAL", "/x/control/gitconfig")
        .expect("scope");
    let config = state.join("claude-config");
    std::fs::create_dir(&config).expect("config");
    env.scope("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"))
        .expect("scope");
    env.identity("BUZZ_RELAY_URL", "wss://relay.example.invalid", &|_| true);
    env.identity("BUZZ_PRIVATE_KEY", "nsec-test-value", &|_| true);
    let foreign = state.join("foreign-file");
    std::fs::write(&foreign, "FOREIGN").expect("foreign");
    let settings = state.join("claude-settings.json");
    std::os::unix::fs::symlink(&foreign, &settings).expect("planted link");
    write_claude_settings(&settings, &env).expect("written");
    assert_eq!(
        std::fs::read_to_string(&foreign).expect("foreign"),
        "FOREIGN",
        "the link was followed"
    );
    let meta = std::fs::symlink_metadata(&settings).expect("settings");
    assert!(meta.is_file());
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).expect("read")).expect("json");
    assert_eq!(
        value["env"]["BUZZ_RELAY_URL"],
        "wss://relay.example.invalid"
    );
    assert_eq!(
        value["env"]["BUZZ_PRIVATE_KEY"], "nsec-test-value",
        "identity is pinned, key included"
    );
    assert_eq!(value["env"]["GIT_CONFIG_GLOBAL"], "/x/control/gitconfig");
    for protected in ["CLAUDE_CONFIG_DIR", "HOME", "PATH"] {
        assert!(
            value["env"].get(protected).is_none(),
            "{protected}: {value}"
        );
    }
    assert_eq!(value["disableClaudeAiConnectors"], true);
}

/// Every execution's private configuration offers the whole model list, so a
/// degraded flag cache cannot hide earlier versions (ledger 302(d)), and the
/// list replaces only its own key.
#[cfg(unix)]
#[test]
fn the_private_configuration_offers_every_claude_model_and_keeps_its_other_settings() {
    use buzz_acp::exec_env::ModelAuth;
    let (_dir, state, _home) = scratch();
    let config = state.join("claude-config");
    std::fs::create_dir(&config).expect("config");
    let user_settings = config.join("settings.json");
    std::fs::write(
        &user_settings,
        r#"{"theme":"dark","availableModels":["haiku"]}"#,
    )
    .expect("seeded");
    let fence = crate::agent_fence::FENCE;
    let mut env = ResolvedEnv::baseline(
        [("PATH".into(), "/usr/bin".into())],
        ModelAuth::None,
        &fence,
        &|_| true,
    );
    env.scope("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"))
        .expect("scope");
    write_claude_settings(&state.join("claude-settings.json"), &env).expect("written");

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&user_settings).expect("read"))
            .expect("json");
    assert_eq!(value["theme"], "dark", "another key survives");
    let listed: Vec<&str> = value["availableModels"]
        .as_array()
        .expect("list")
        .iter()
        .map(|model| model.as_str().expect("id"))
        .collect();
    assert_eq!(listed, CLAUDE_MODELS);
    for earlier in ["claude-opus-4-8", "claude-sonnet-5", "claude-opus-5"] {
        assert!(listed.contains(&earlier), "{earlier} offered");
    }

    // A file the runtime left unreadable is replaced, not a refusal.
    std::fs::write(&user_settings, "not json").expect("garbled");
    write_claude_settings(&state.join("claude-settings.json"), &env).expect("rewritten");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&user_settings).expect("read"))
            .expect("json");
    assert_eq!(
        value["availableModels"].as_array().map(Vec::len),
        Some(CLAUDE_MODELS.len())
    );
}

/// The preflight's four outcomes, through a fixture CLI run under a real
/// prepared boundary. The fixture answers `auth status --json` from its
/// environment the way the measured CLI does; no real login is involved.
#[cfg(target_os = "macos")]
mod preflight {
    use super::*;
    use buzz_acp::exec_boundary::{prepare, BoundarySpec};
    use buzz_acp::exec_env::{ModelAuth, ResolvedEnv};

    struct Case {
        _dir: tempfile::TempDir,
        own: PathBuf,
        config: PathBuf,
        cli: PathBuf,
    }

    fn case(script: &str) -> Case {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let own = root.join("own");
        let config = own.join("claude-config");
        std::fs::create_dir_all(&config).expect("config");
        let cli = root.join("claude");
        std::fs::write(&cli, format!("#!/bin/sh\n{script}\n")).expect("cli");
        let mut perms = std::fs::metadata(&cli).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&cli, perms).expect("chmod");
        Case {
            _dir: dir,
            own,
            config,
            cli,
        }
    }

    fn run(case: &Case) -> Result<(), CreateFailure> {
        std::fs::write(case.own.join("probe"), "p").expect("probe");
        let boundary = prepare(BoundarySpec {
            grants: vec![
                Grant::tree(&case.own, Access::ReadWrite, "own"),
                Grant::file(&case.cli, Access::ReadOnly, "cli"),
            ],
            policy_dir: case.own.parent().expect("root").join("host"),
            egress: Default::default(),
            probe_readable: case.own.join("probe"),
        })
        .expect("boundary");
        let fence = crate::agent_fence::FENCE;
        let mut env = ResolvedEnv::baseline(
            [("PATH".into(), "/usr/bin:/bin".into())],
            ModelAuth::None,
            &fence,
            &|_| true,
        );
        env.scope("CLAUDE_CONFIG_DIR", &case.config.display().to_string())
            .expect("scope");
        env.scope("CLAUDE_SECURESTORAGE_CONFIG_DIR", "")
            .expect("scope");
        claude_auth_preflight(&boundary, &env, &case.own, &case.cli, &case.config)
    }

    const REPORT: &str = r#"[ "$1" = --version ] && { echo "9.9.9 (fixture)"; exit 0; }
if [ -n "${CLAUDE_CONFIG_DIR+x}" ] && [ -n "${CLAUDE_SECURESTORAGE_CONFIG_DIR+x}" ]; then
  printf '{"loggedIn":%s,"configDirectory":"%s"}\n' "$INSIDE" "$CLAUDE_CONFIG_DIR"
else
  printf '{"loggedIn":%s,"configDirectory":"/Users/nobody/.claude"}\n' "$HOST"
fi"#;

    #[test]
    fn a_login_reachable_from_the_private_configuration_passes() {
        let case = case(&format!("INSIDE=true HOST=true\n{REPORT}"));
        run(&case).expect("preflight");
    }

    #[test]
    fn an_existing_login_the_boundary_cannot_reach_is_a_compatibility_refusal() {
        let case = case(&format!("INSIDE=false HOST=true\n{REPORT}"));
        let error = run(&case).expect_err("unreachable");
        assert_eq!(error.code, EXECUTION_BOUNDARY_UNAVAILABLE);
        assert!(
            error.message.contains("No new login is needed"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("9.9.9"),
            "names the version: {}",
            error.message
        );
        assert!(!error.message.contains("sign in"), "{}", error.message);
    }

    #[test]
    fn no_login_at_all_asks_for_a_sign_in() {
        let case = case(&format!("INSIDE=false HOST=false\n{REPORT}"));
        let error = run(&case).expect_err("logged out");
        assert_eq!(error.code, crate::payload::PROVIDER_AUTH_REQUIRED);
    }

    #[test]
    fn a_cli_that_ignores_the_private_configuration_is_refused() {
        let case =
            case(r#"printf '{"loggedIn":true,"configDirectory":"/Users/nobody/.claude"}\n'"#);
        let error = run(&case).expect_err("config ignored");
        assert_eq!(error.code, EXECUTION_BOUNDARY_UNAVAILABLE);
        assert!(
            error.message.contains("private directory"),
            "{}",
            error.message
        );
    }
}
