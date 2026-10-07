use super::*;

const FENCE: EnvFence = EnvFence {
    keys: &["NOSTR_PRIVATE_KEY"],
    prefixes: &["BEEKEEPER_", "BUZZ_"],
    exempt: &[],
};

fn ambient(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
}

fn usable(path: &Path) -> bool {
    path.starts_with("/usr") || path.starts_with("/bin") || path.starts_with("/granted")
}

#[test]
fn only_named_baseline_categories_survive_from_the_host() {
    let env = ResolvedEnv::baseline(
        ambient(&[
            ("HOME", "/Users/op"),
            ("LANG", "en_US.UTF-8"),
            ("PROJECT_A_ONLY", "canary-a"),
            ("DATABASE_URL", "postgres://a"),
            ("KETTLE_FILE", "/granted/a/data.json"),
            ("BEEKEEPER_PRIVATE_KEY", "nsec-provider"),
            ("BUZZ_PRIVATE_KEY", "nsec-provider"),
            ("NOSTR_PRIVATE_KEY", "nsec-provider"),
            ("HTTPS_PROXY", "http://proxy:3128"),
        ]),
        ModelAuth::Claude,
        &FENCE,
        &usable,
    );
    assert_eq!(env.get("HOME"), Some("/Users/op"));
    assert_eq!(env.get("LANG"), Some("en_US.UTF-8"));
    assert_eq!(env.get("HTTPS_PROXY"), Some("http://proxy:3128"));
    for absent in [
        "PROJECT_A_ONLY",
        "DATABASE_URL",
        "KETTLE_FILE",
        "BEEKEEPER_PRIVATE_KEY",
        "BUZZ_PRIVATE_KEY",
        "NOSTR_PRIVATE_KEY",
    ] {
        assert_eq!(env.get(absent), None, "{absent} was inherited");
    }
    // Excluded names are disclosed, fenced ones are not even named.
    assert_eq!(
        env.excluded(),
        ["DATABASE_URL", "KETTLE_FILE", "PROJECT_A_ONLY"]
    );
}

#[test]
fn model_login_variables_follow_the_selected_runtime() {
    let host = ambient(&[
        ("ANTHROPIC_API_KEY", "sk-ant"),
        ("OPENAI_API_KEY", "sk-oai"),
    ]);
    let claude = ResolvedEnv::baseline(host.clone(), ModelAuth::Claude, &FENCE, &usable);
    assert_eq!(claude.get("ANTHROPIC_API_KEY"), Some("sk-ant"));
    assert_eq!(claude.get("OPENAI_API_KEY"), None);
    let codex = ResolvedEnv::baseline(host, ModelAuth::Codex, &FENCE, &usable);
    assert_eq!(codex.get("OPENAI_API_KEY"), Some("sk-oai"));
    assert_eq!(codex.get("ANTHROPIC_API_KEY"), None);
}

#[test]
fn path_values_must_be_usable_inside_the_boundary() {
    let env = ResolvedEnv::baseline(
        ambient(&[
            (
                "PATH",
                "/granted/tool/bin:/Users/op/other-project/.bin:/usr/bin:/usr/bin:relative",
            ),
            ("SSL_CERT_FILE", "/Users/op/other-project/ca.pem"),
            ("CARGO_HOME", "/granted/cargo"),
        ]),
        ModelAuth::None,
        &FENCE,
        &usable,
    );
    assert_eq!(env.get("PATH"), Some("/granted/tool/bin:/usr/bin"));
    assert_eq!(env.get("SSL_CERT_FILE"), None);
    assert_eq!(env.get("CARGO_HOME"), Some("/granted/cargo"));
    assert_eq!(
        env.path_components_dropped(),
        ["/Users/op/other-project/.bin", "relative"],
        "each dropped directory is named for diagnostics; the duplicate is folded, not dropped"
    );
}

#[test]
fn scope_values_are_protected_and_identity_is_last() {
    let mut env =
        ResolvedEnv::baseline(ambient(&[("HOME", "/h")]), ModelAuth::None, &FENCE, &usable);
    assert_eq!(
        env.project(
            "BEEKEEPER_PRIVATE_KEY",
            "project-said",
            EnvSource::Project,
            &FENCE,
            &usable
        ),
        Err(EnvRefusal::Fenced("BEEKEEPER_PRIVATE_KEY".into()))
    );
    for owned in [
        "HOME",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "TMPDIR",
        "GIT_DIR",
        "GIT_CONFIG_KEY_0",
    ] {
        assert_eq!(
            env.project(owned, "/elsewhere", EnvSource::Project, &FENCE, &usable),
            Err(EnvRefusal::ScopeOwned(owned.into())),
            "{owned}"
        );
    }
    assert_eq!(
        env.get("HOME"),
        Some("/h"),
        "a refused declaration changes nothing"
    );
    env.project(
        "KETTLE_FILE",
        "/granted/a/data.json",
        EnvSource::Project,
        &FENCE,
        &usable,
    )
    .expect("project value");
    assert_eq!(
        env.scope("KETTLE_FILE", "/scope"),
        Err(EnvRefusal::Conflict("KETTLE_FILE".into())),
        "a scope value never silently replaces a project declaration"
    );
    env.scope("TMPDIR", "/exec/tmp/").expect("scope");
    env.identity("BEEKEEPER_PRIVATE_KEY", "seat-key", &usable);
    assert_eq!(env.get("BEEKEEPER_PRIVATE_KEY"), Some("seat-key"));
    assert_eq!(env.get("TMPDIR"), Some("/exec/tmp/"));
}

#[test]
fn the_seat_path_is_prepended_to_the_effective_path_not_substituted() {
    let mut env = ResolvedEnv::baseline(
        ambient(&[("PATH", "/usr/bin:/bin")]),
        ModelAuth::None,
        &FENCE,
        &usable,
    );
    assert_eq!(
        env.project(
            "PATH",
            "/granted/project/tools:/Users/op/other/bin",
            EnvSource::Project,
            &FENCE,
            &usable
        ),
        Err(EnvRefusal::PathOutsideScope("/Users/op/other/bin".into())),
        "a declared PATH may not reach outside the boundary"
    );
    env.project(
        "PATH",
        "/granted/project/tools:/usr/bin:/bin",
        EnvSource::Project,
        &FENCE,
        &usable,
    )
    .expect("a project PATH of usable directories");
    // What the seat custody hands over: its bee first, then the *host* PATH.
    env.identity(
        "PATH",
        "/granted/bee:/usr/bin:/bin:/Users/op/host-only/bin",
        &usable,
    );
    assert_eq!(
        env.get("PATH"),
        Some("/granted/bee:/granted/project/tools:/usr/bin:/bin"),
        "the seat's bee first, the project's tools kept, the host-only directory never added"
    );
}

#[test]
fn diagnostics_never_carry_values() {
    let mut env = ResolvedEnv::baseline(
        ambient(&[
            ("ANTHROPIC_API_KEY", "sk-secret-value"),
            ("SECRET_X", "hidden"),
        ]),
        ModelAuth::Claude,
        &FENCE,
        &usable,
    );
    env.project(
        "TOKEN",
        "project-secret",
        EnvSource::ProjectFromHost,
        &FENCE,
        &usable,
    )
    .expect("declared");
    let rendered = format!("{env:?}");
    assert!(!rendered.contains("sk-secret-value"), "{rendered}");
    assert!(!rendered.contains("project-secret"), "{rendered}");
    assert!(!rendered.contains("hidden"), "{rendered}");
    assert!(rendered.contains("ANTHROPIC_API_KEY"));
    assert!(
        rendered.contains("SECRET_X"),
        "excluded names are disclosed"
    );
}

/// The child receives exactly the resolved environment, inside the verified
/// boundary — through a real spawn, not the builder's own bookkeeping.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_bounded_launch_gives_the_child_exactly_the_resolved_environment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let own = root.join("own");
    std::fs::create_dir_all(&own).expect("own");
    std::fs::write(root.join("outside"), "OUTSIDE").expect("outside");
    let boundary = crate::exec_boundary::prepare(crate::exec_boundary::BoundarySpec {
        grants: vec![crate::exec_boundary::Grant::tree(
            &own,
            crate::exec_boundary::Access::ReadWrite,
            "own",
        )],
        policy_dir: root.join("host"),
        egress: Default::default(),
        extra_loopback_ports: Vec::new(),
        probe_readable: {
            std::fs::write(own.join("probe"), "p").expect("probe");
            own.join("probe")
        },
    })
    .expect("boundary");
    let dump = own.join("env");
    let mut env = ResolvedEnv::baseline(
        ambient(&[("PATH", "/usr/bin:/bin"), ("PROJECT_A_ONLY", "canary")]),
        ModelAuth::None,
        &FENCE,
        &usable,
    );
    env.identity("SEAT_SENTINEL", "seat-a", &usable);
    let launch = crate::acp::BoundedLaunch::new(boundary, env, own.clone());
    let script = format!(
        "/usr/bin/env > '{dump}'; /bin/sh -c '/usr/bin/env' >> '{dump}'; \
         cat '{outside}' >> '{dump}' 2>&1 || echo OUTSIDE_REFUSED >> '{dump}'",
        dump = dump.display(),
        outside = root.join("outside").display(),
    );
    let mut client =
        crate::acp::AcpClient::spawn_bounded("/bin/sh", &["-c".to_owned(), script], &launch)
            .await
            .expect("spawn");
    for _ in 0..100 {
        if std::fs::read_to_string(&dump).is_ok_and(|text| text.contains("OUTSIDE")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    client.shutdown().await;
    let text = std::fs::read_to_string(&dump).expect("dump");
    assert!(text.contains("SEAT_SENTINEL=seat-a"), "{text}");
    assert!(
        text.contains("OUTSIDE_REFUSED") && !text.contains("OUTSIDE\n"),
        "{text}"
    );
    assert!(!text.contains("PROJECT_A_ONLY"), "{text}");
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("BUZZ_") || line.starts_with("BEEKEEPER_")),
        "the harness's own namespace leaked: {text}"
    );
}

/// Full access (ledger 303): the same prepared launch without its boundary.
/// The child reaches what the boundary refuses, and still receives exactly the
/// resolved environment — full access widens the filesystem, not the
/// environment.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_full_access_launch_reaches_outside_with_exactly_the_resolved_environment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let own = root.join("own");
    std::fs::create_dir_all(&own).expect("own");
    std::fs::write(root.join("outside"), "OUTSIDE").expect("outside");
    let boundary = crate::exec_boundary::prepare(crate::exec_boundary::BoundarySpec {
        grants: vec![crate::exec_boundary::Grant::tree(
            &own,
            crate::exec_boundary::Access::ReadWrite,
            "own",
        )],
        policy_dir: root.join("host"),
        egress: Default::default(),
        extra_loopback_ports: Vec::new(),
        probe_readable: {
            std::fs::write(own.join("probe"), "p").expect("probe");
            own.join("probe")
        },
    })
    .expect("boundary");
    let dump = own.join("env");
    let mut env = ResolvedEnv::baseline(
        ambient(&[("PATH", "/usr/bin:/bin"), ("PROJECT_A_ONLY", "canary")]),
        ModelAuth::None,
        &FENCE,
        &usable,
    );
    env.identity("SEAT_SENTINEL", "seat-a", &usable);
    let launch = crate::acp::BoundedLaunch::new(boundary, env, own.clone());
    let script = format!(
        "/usr/bin/env > '{dump}'; cat '{outside}' >> '{dump}' 2>&1 || echo OUTSIDE_REFUSED >> '{dump}'; \
         echo DONE >> '{dump}'",
        dump = dump.display(),
        outside = root.join("outside").display(),
    );
    let mut client = crate::acp::AcpClient::spawn_with_full_access(
        "/bin/sh",
        &["-c".to_owned(), script],
        &launch,
    )
    .await
    .expect("spawn");
    for _ in 0..100 {
        if std::fs::read_to_string(&dump).is_ok_and(|text| text.contains("DONE")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    client.shutdown().await;
    let text = std::fs::read_to_string(&dump).expect("dump");
    assert!(text.contains("SEAT_SENTINEL=seat-a"), "{text}");
    assert!(
        text.contains("OUTSIDE") && !text.contains("OUTSIDE_REFUSED"),
        "full access must reach what the boundary refuses: {text}"
    );
    assert!(!text.contains("PROJECT_A_ONLY"), "{text}");
}
