//! Cargo's Git transport inside an execution: the scope's default reaches both
//! execution purposes and the Claude flag settings, a project's own value
//! stands, and the variable makes Cargo run `git` against the staged
//! configuration. No network, and every Git configuration here is a fixture.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::tests::git;
use super::CARGO_GIT_FETCH_WITH_CLI;

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// A committed dependency and a crate that names it by `file://` URL.
fn dependency_and_consumer(root: &Path) -> PathBuf {
    let dep = root.join("dep");
    std::fs::create_dir_all(dep.join("src")).expect("dep");
    std::fs::write(dep.join("src/lib.rs"), "").expect("dep lib");
    std::fs::write(
        dep.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("dep manifest");
    git(&dep, &["init", "-q", "-b", "main"]);
    git(&dep, &["add", "."]);
    git(&dep, &["commit", "-qm", "dep"]);

    let consumer = root.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).expect("consumer");
    std::fs::write(consumer.join("src/lib.rs"), "").expect("consumer lib");
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"consumer\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
             [dependencies]\ndep = {{ git = \"file://{}\" }}\n",
            dep.display()
        ),
    )
    .expect("consumer manifest");
    consumer
}

/// `true` sends the fetch through `git`, reading the staged file; `false` keeps
/// it in-process and `git` never runs. Without a boundary both succeed, so what
/// differs is the transport alone.
#[test]
fn the_variable_runs_git_against_the_staged_configuration() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let consumer = dependency_and_consumer(&root);

    let staged = root.join("staged-gitconfig");
    std::fs::write(&staged, "[fixture]\n\tmarker = staged\n").expect("staged");
    let home = root.join("home");
    std::fs::create_dir(&home).expect("home");
    std::fs::write(home.join(".gitconfig"), "[fixture]\n\tmarker = home\n").expect("home config");

    let path = std::env::var_os("PATH").expect("PATH");
    let real_git = std::env::split_paths(&path)
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("git on PATH");
    let log = root.join("git-calls");
    let shims = root.join("shims");
    std::fs::create_dir(&shims).expect("shims");
    let shim = shims.join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$('{git}' config --global --get fixture.marker)\" >> '{log}'\nexec '{git}' \"$@\"\n",
            git = real_git.display(),
            log = log.display(),
        ),
    )
    .expect("shim");
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let shimmed_path =
        std::env::join_paths(std::iter::once(shims.clone()).chain(std::env::split_paths(&path)))
            .expect("join PATH");

    let fetch = |value: &str| {
        std::fs::remove_file(consumer.join("Cargo.lock")).ok();
        let output = Command::new(cargo())
            .arg("fetch")
            .current_dir(&consumer)
            .env("PATH", &shimmed_path)
            .env("HOME", &home)
            .env("CARGO_HOME", root.join(format!("cargo-home-{value}")))
            .env("GIT_CONFIG_GLOBAL", &staged)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(CARGO_GIT_FETCH_WITH_CLI, value)
            .output()
            .expect("cargo fetch");
        assert!(output.status.success(), "{value}: {output:?}");
    };

    fetch("false");
    assert!(!log.exists(), "libgit2 fetched without the git program");

    fetch("true");
    let calls = std::fs::read_to_string(&log).expect("git ran");
    assert!(
        !calls.is_empty() && calls.lines().all(|line| line == "staged"),
        "{calls}"
    );
    assert!(consumer.join("Cargo.lock").is_file());
}

#[cfg(target_os = "macos")]
mod in_scope {
    use std::ffi::OsString;

    use super::super::tests::launched::run_in;
    use super::super::tests::{fixture, inputs, Fixture};
    use super::super::{prepare, ExecutionPlan, PreparedExecution, ScopePurpose};
    use super::*;
    use crate::execution_scope_git::TEST_OPERATOR_CONFIG;
    use beekeeper_acp::exec_env::EnvSource;

    fn prepared(
        plan: Result<ExecutionPlan, crate::session::CreateFailure>,
    ) -> Box<PreparedExecution> {
        match plan {
            Ok(ExecutionPlan::Prepared(plan)) => plan,
            other => panic!("prepared: {other:?}"),
        }
    }

    fn fake_operator_config(fx: &Fixture) {
        let config = fx.root.join("fixture-operator-gitconfig");
        std::fs::write(&config, "[user]\n\tname = Fixture\n").expect("config");
        TEST_OPERATOR_CONFIG.with(|file| *file.borrow_mut() = Some(config));
    }

    #[test]
    fn the_default_reaches_each_purpose_and_the_flag_settings_and_a_project_value_stands() {
        let fx = fixture();
        fake_operator_config(&fx);
        let ambient: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            (CARGO_GIT_FETCH_WITH_CLI.into(), "false".into()),
        ];
        for purpose in [ScopePurpose::Session, ScopePurpose::HostCommand] {
            let mut scope = inputs(&fx, "default", &fx.seat_a, &[], &[]);
            scope.purpose = purpose;
            scope.ambient_env = Some(&ambient);
            let plan = prepared(prepare(&scope));
            assert_eq!(
                plan.launch.env().get(CARGO_GIT_FETCH_WITH_CLI),
                Some("true"),
                "{purpose:?}: an operator's ambient value does not reach an execution"
            );
        }

        let session = prepared(prepare(&inputs(&fx, "settings", &fx.seat_a, &[], &[])));
        let settings = fx.root.join("fixture-claude-settings.json");
        crate::execution_scope_runtime::write_claude_settings(&settings, session.launch.env())
            .expect("settings");
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).expect("read")).expect("json");
        assert_eq!(settings["env"][CARGO_GIT_FETCH_WITH_CLI], "true");

        let declared = [(
            CARGO_GIT_FETCH_WITH_CLI.to_owned(),
            "false".to_owned(),
            EnvSource::Project,
        )];
        let mut scope = inputs(&fx, "declared", &fx.seat_a, &[], &[]);
        scope.purpose = ScopePurpose::HostCommand;
        scope.project_env = &declared;
        let plan = prepared(prepare(&scope));
        assert_eq!(
            plan.launch.env().get(CARGO_GIT_FETCH_WITH_CLI),
            Some("false")
        );
        TEST_OPERATOR_CONFIG.with(|file| *file.borrow_mut() = None);
    }

    /// The Keystone failure under the real boundary, with a stand-in `HOME`
    /// holding a `.gitconfig` the boundary refuses: the scope's own environment
    /// fetches, and only taking the variable away brings the refusal back.
    #[test]
    fn a_git_dependency_fetches_under_the_real_boundary_without_a_home_grant() {
        let fx = fixture();
        fake_operator_config(&fx);
        let consumer = dependency_and_consumer(&fx.seat_a);
        let home = fx.root.join("forbidden-home");
        std::fs::create_dir(&home).expect("home");
        std::fs::write(home.join(".gitconfig"), "[fixture]\n\tmarker = home\n")
            .expect("home config");
        let ambient: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), std::env::var_os("PATH").expect("PATH")),
            ("HOME".into(), home.clone().into()),
        ];
        let mut scope = inputs(&fx, "fetch", &fx.seat_a, &[], &[]);
        scope.ambient_env = Some(&ambient);
        let plan = prepared(prepare(&scope));
        let cargo = cargo();
        let cargo = cargo.to_str().expect("utf-8");
        // `cargo fetch` asks `rustc` for its version; find it beside this cargo.
        let toolchain = Path::new(cargo)
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty());
        let on_path = toolchain.map_or_else(String::new, |dir| {
            format!("PATH='{}':\"$PATH\" ", dir.display())
        });

        let refused = run_in(
            &plan,
            &consumer,
            &format!("cat '{}/.gitconfig'", home.display()),
        );
        assert!(!refused.status.success(), "{refused:?}");

        let fetched = run_in(&plan, &consumer, &format!("{on_path}'{cargo}' fetch"));
        assert!(fetched.status.success(), "{fetched:?}");
        assert!(consumer.join("Cargo.lock").is_file());

        std::fs::remove_file(consumer.join("Cargo.lock")).expect("lock");
        let libgit2 = run_in(
            &plan,
            &consumer,
            &format!("CARGO_HOME=\"$CARGO_HOME-libgit2\" {on_path}{CARGO_GIT_FETCH_WITH_CLI}=false '{cargo}' fetch"),
        );
        assert!(!libgit2.status.success(), "{libgit2:?}");
        let stderr = String::from_utf8_lossy(&libgit2.stderr);
        assert!(stderr.contains("code=NotFound (-3)"), "{stderr}");
        TEST_OPERATOR_CONFIG.with(|file| *file.borrow_mut() = None);
    }
}
