use std::os::unix::fs::PermissionsExt;

use super::super::{
    install_at, registration_contents, registration_path, status_at, uninstall_at, DaemonLookup,
};
use super::*;

const RERUN: &str = "/usr/local/bin/bee host install --system --user persona";

/// An account that is this test process, living in `home`.
///
/// This process's own uid and gid, so the ownership checks that run whether or
/// not the install is live have something true to compare against.
fn account(home: &Path) -> Account {
    Account {
        name: "persona".to_string(),
        uid: nix::unistd::getuid().as_raw(),
        gid: nix::unistd::getgid().as_raw(),
        home: home.to_path_buf(),
    }
}

/// A binary every user can run, at an absolute path.
fn program(dir: &Path) -> PathBuf {
    let program = dir.join("beekeeper-host");
    std::fs::write(&program, b"#!/bin/sh\n").expect("write program");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    program
}

fn commission(home: &Path, instance: Instance) {
    let config = layout::host_config_path(home, instance);
    std::fs::create_dir_all(config.parent().expect("parent")).expect("mkdir");
    std::fs::write(&config, b"{}").expect("write host.json");
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("read_dir")
        .map(|entry| entry.expect("entry").path())
        .collect()
}

/// The line after `<key>{key}</key>`: the value, or a `<dict>` opening one.
fn value_after<'a>(plist: &'a str, key: &str) -> &'a str {
    let marker = format!("<key>{key}</key>");
    let at = plist
        .find(&marker)
        .unwrap_or_else(|| panic!("no {key} in {plist}"));
    plist[at + marker.len()..]
        .lines()
        .nth(1)
        .unwrap_or_default()
        .trim()
}

/// Every key the plan names, with the value that makes it mean what it should.
#[test]
fn the_daemon_plist_runs_as_the_persona_in_its_own_home() {
    let home = Path::new("/Users/persona");
    let program = Path::new("/usr/local/bin/beekeeper-host");
    let plist = plist_contents(&account(home), Instance::Production, program).expect("render");

    assert_eq!(value_after(&plist, "UserName"), "<string>persona</string>");
    assert_eq!(value_after(&plist, "RunAtLoad"), "<true/>");
    let keep_alive = value_after(&plist, "KeepAlive");
    assert!(
        keep_alive.starts_with("<dict>")
            && plist.contains("<key>SuccessfulExit</key>\n\t\t<false/>"),
        "KeepAlive must restart a crash and leave a clean exit down: {plist}"
    );
    assert_eq!(
        value_after(&plist, "HOME"),
        "<string>/Users/persona</string>"
    );
    assert_eq!(
        value_after(&plist, "WorkingDirectory"),
        "<string>/Users/persona</string>"
    );
    assert_eq!(value_after(&plist, "USER"), "<string>persona</string>");
    assert_eq!(value_after(&plist, "LOGNAME"), "<string>persona</string>");
    assert_eq!(
        value_after(&plist, "PATH"),
        format!("<string>{DAEMON_PATH}</string>")
    );
    let log = layout::host_log_path(home, Instance::Production);
    assert!(log.starts_with(home));
    for key in ["StandardOutPath", "StandardErrorPath"] {
        assert_eq!(
            value_after(&plist, key),
            format!("<string>{}</string>", log.display())
        );
    }
    assert_eq!(
        value_after(&plist, INSTANCE_VAR),
        "<string>production</string>"
    );
    assert_eq!(
        value_after(&plist, "ProcessType"),
        "<string>Standard</string>"
    );
    assert_eq!(
        value_after(&plist, "ThrottleInterval"),
        "<integer>30</integer>"
    );

    let dev = plist_contents(&account(home), Instance::Dev, program).expect("render");
    assert_eq!(value_after(&dev, INSTANCE_VAR), "<string>dev</string>");
    assert!(dev.contains(&format!(
        "<string>{}</string>",
        layout::host_log_path(home, Instance::Dev).display()
    )));
}

/// launchd must wait longer than the provider takes to flush its outbox, or
/// it SIGKILLs the host mid-handoff — the same bound systemd's unit carries.
#[test]
fn launchd_waits_longer_than_the_provider_takes_to_stop() {
    let plist = plist_contents(
        &account(Path::new("/Users/persona")),
        Instance::Production,
        Path::new("/usr/local/bin/beekeeper-host"),
    )
    .expect("render");
    let exit: u64 = value_after(&plist, "ExitTimeOut")
        .trim_start_matches("<integer>")
        .trim_end_matches("</integer>")
        .parse()
        .expect("a number");
    let provider = crate::terminate::GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
        + crate::terminate::ESCALATION_TIMEOUT.as_secs();
    assert!(exit > provider, "{exit} must exceed {provider}");
}

/// `status` reports the program a daemon names by parsing it back out.
#[test]
fn the_program_round_trips_out_of_the_daemon_plist() {
    let program = Path::new("/Applications/Beekeeper.app/Contents/MacOS/beekeeper-host");
    let plist = plist_contents(
        &account(Path::new("/Users/persona")),
        Instance::Production,
        program,
    )
    .expect("render");
    assert_eq!(
        super::super::program_from_plist(&plist),
        Some(program.to_path_buf())
    );
    #[cfg(target_os = "macos")]
    assert_eq!(
        super::super::program_from_registration(&plist),
        Some(program.to_path_buf())
    );
    assert!(
        plist.contains("<string>run</string>"),
        "same argv as the agent: {plist}"
    );
}

#[test]
fn daemon_labels_are_per_user_and_instance_and_never_an_agent_label() {
    let labels = [
        label("alice", Instance::Production),
        label("alice", Instance::Dev),
        label("bob", Instance::Production),
        label("bob", Instance::Dev),
    ];
    assert_eq!(labels[0], "io.agiterra.beekeeper.host.daemon.alice");
    assert_eq!(labels[1], "io.agiterra.beekeeper.host.daemon.alice.dev");
    for (i, a) in labels.iter().enumerate() {
        for b in &labels[i + 1..] {
            assert_ne!(a, b);
        }
        for service in [Service::AgentHost, Service::MenuBar] {
            for instance in [Instance::Production, Instance::Dev] {
                assert_ne!(a, &super::super::service_name(service, instance));
            }
        }
    }
    assert_eq!(
        plist_path(Path::new(DAEMON_DIR), "alice", Instance::Dev),
        Path::new("/Library/LaunchDaemons/io.agiterra.beekeeper.host.daemon.alice.dev.plist")
    );
}

#[test]
fn a_name_that_is_not_a_safe_label_component_is_refused() {
    for bad in [
        "", "a.b", "x.dev", "a/b", "../etc", "-x", "a b", "ünï", "a<b", "root",
    ] {
        assert!(validate_user_name(bad).is_err(), "{bad:?} must be refused");
    }
    for good in ["persona", "fondant", "_beekeeper", "bk-agent_2"] {
        validate_user_name(good).unwrap_or_else(|error| panic!("{good:?}: {error}"));
    }
}

#[test]
fn root_and_unknown_users_are_refused() {
    let root = resolve_account("root").expect_err("root");
    assert!(root.contains("must not run as root"), "{root}");
    let missing = resolve_account("bk-no-such-user-xyzzy").expect_err("missing");
    assert!(missing.contains("no user named"), "{missing}");
}

#[test]
fn launchctl_print_asks_the_system_domain() {
    assert_eq!(
        print_args("io.agiterra.beekeeper.host.daemon.alice"),
        [
            "print".to_string(),
            "system/io.agiterra.beekeeper.host.daemon.alice".to_string()
        ]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn system_mode_is_available_on_macos() {
    supported().expect("macOS has LaunchDaemons");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn system_mode_is_refused_off_macos() {
    let error = supported().expect_err("no LaunchDaemons here");
    assert!(error.contains("macOS"), "{error}");
    let error = install(
        "persona",
        Instance::Production,
        Path::new("/bin/true"),
        RERUN,
    )
    .expect_err("refused before anything else");
    assert!(error.contains("macOS"), "{error}");
}

/// The rendered file is a plist launchd will parse, not just one that looks
/// right to a string assertion.
#[cfg(target_os = "macos")]
#[test]
fn the_daemon_plist_passes_plutil_lint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("daemon.plist");
    std::fs::write(
        &path,
        plist_contents(
            &account(Path::new("/Users/persona")),
            Instance::Dev,
            Path::new("/usr/local/bin/beekeeper-host"),
        )
        .expect("render"),
    )
    .expect("write");
    let output = std::process::Command::new("plutil")
        .arg("-lint")
        .arg(&path)
        .output()
        .expect("plutil runs on macOS");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_path_that_would_need_escaping_is_refused_not_mangled() {
    let error = plist_contents(
        &account(Path::new("/Users/a&b")),
        Instance::Production,
        Path::new("/usr/local/bin/beekeeper-host"),
    )
    .expect_err("an & cannot round-trip");
    assert!(error.contains("escaped"), "{error}");
}

/// Not root: refused with the command to run, and nothing written anywhere —
/// not the plist, not the persona's log.
#[test]
fn a_system_install_without_root_writes_nothing() {
    let daemons = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    commission(home.path(), Instance::Production);
    let system = System {
        dir: daemons.path(),
        root: false,
    };
    let error = install_into(
        &system,
        &account(home.path()),
        Instance::Production,
        &program(home.path()),
        RERUN,
    )
    .expect_err("needs root");
    assert!(error.contains(&format!("sudo {RERUN}")), "{error}");
    assert!(entries(daemons.path()).is_empty());
    assert!(!layout::host_log_path(home.path(), Instance::Production).exists());
}

#[test]
fn an_uncommissioned_host_is_refused() {
    let daemons = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    let system = System {
        dir: daemons.path(),
        root: true,
    };
    let error = install_into(
        &system,
        &account(home.path()),
        Instance::Production,
        &program(home.path()),
        RERUN,
    )
    .expect_err("not commissioned");
    assert!(
        error.contains("not commissioned") && error.contains("host.json"),
        "{error}"
    );
    assert!(entries(daemons.path()).is_empty());
}

#[test]
fn an_existing_launch_agent_is_refused_with_its_bootout() {
    let daemons = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    commission(home.path(), Instance::Production);
    let agent =
        super::super::launch_agent_path(Service::AgentHost, home.path(), Instance::Production);
    std::fs::create_dir_all(agent.parent().expect("parent")).expect("mkdir");
    std::fs::write(&agent, b"<plist/>").expect("write agent");
    let system = System {
        dir: daemons.path(),
        root: true,
    };
    let error = install_into(
        &system,
        &account(home.path()),
        Instance::Production,
        &program(home.path()),
        RERUN,
    )
    .expect_err("would race");
    assert!(
        error.contains("launchctl bootout gui/") && error.contains("race"),
        "{error}"
    );
    assert!(entries(daemons.path()).is_empty());
}

/// Claimed root, a temporary directory: every file is written as it would be,
/// and nothing reaches launchd because the directory is not the real one.
#[test]
fn a_claimed_root_install_writes_the_plist_and_the_log_and_reads_back_as_system() {
    let daemons = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    commission(home.path(), Instance::Dev);
    let system = System {
        dir: daemons.path(),
        root: true,
    };
    assert!(!system.is_live(), "a tempdir must never be the live domain");
    let program = program(home.path());
    let registration = install_into(
        &system,
        &account(home.path()),
        Instance::Dev,
        &program,
        RERUN,
    )
    .expect("install");
    let path = plist_path(daemons.path(), "persona", Instance::Dev);
    assert_eq!(registration.path, path);
    assert!(registration.installed);
    assert_eq!(registration.domain, Domain::System);
    assert_eq!(registration.program, Some(program));
    assert!(
        registration.warnings.is_empty(),
        "{:?}",
        registration.warnings
    );
    let mode = std::fs::metadata(&path)
        .expect("plist")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o644,
        "launchd and an unprivileged status both read it"
    );
    let log = layout::host_log_path(home.path(), Instance::Dev);
    assert!(log.is_file(), "the log must exist before launchd opens it");

    // Uninstall is root's too, and idempotent.
    let refused = uninstall_from(
        &System {
            dir: daemons.path(),
            root: false,
        },
        "persona",
        Instance::Dev,
        RERUN,
    )
    .expect_err("needs root");
    assert!(refused.contains("sudo"), "{refused}");
    assert!(path.exists());
    uninstall_from(&system, "persona", Instance::Dev, RERUN).expect("uninstall");
    assert!(!path.exists());
    uninstall_from(&system, "persona", Instance::Dev, RERUN).expect("idempotent");
    assert!(
        !layout::login_refusal_path(home.path(), Instance::Dev).exists(),
        "a daemon uninstall records no refusal"
    );
}

#[test]
fn a_program_the_persona_cannot_run_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = program(dir.path());
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    // Somebody who is neither the file's owner nor in its group.
    let stranger = Account {
        uid: nix::unistd::getuid().as_raw() + 4242,
        gid: nix::unistd::getgid().as_raw() + 4242,
        ..account(dir.path())
    };
    let error = check_executable_by(&stranger, &program).expect_err("0700, not theirs");
    assert!(error.contains("cannot read and execute"), "{error}");
    // The owner can, and so can anyone once it is world-executable.
    check_executable_by(&account(dir.path()), &program).expect("owner");
}

#[test]
fn a_program_inside_a_closed_directory_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let closed = dir.path().join("closed");
    std::fs::create_dir(&closed).expect("mkdir");
    let program = program(&closed);
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let stranger = Account {
        uid: nix::unistd::getuid().as_raw() + 4242,
        gid: nix::unistd::getgid().as_raw() + 4242,
        ..account(dir.path())
    };
    let error = check_executable_by(&stranger, &program).expect_err("cannot enter");
    assert!(error.contains("cannot enter"), "{error}");
}

/// Root must not be steered by a link the persona planted at the log's name.
#[test]
fn a_linked_log_is_refused_and_its_target_untouched() {
    let home = tempfile::tempdir().expect("tempdir");
    let elsewhere = tempfile::tempdir().expect("tempdir");
    commission(home.path(), Instance::Production);
    let target = elsewhere.path().join("victim");
    std::fs::write(&target, b"untouched").expect("write");
    std::os::unix::fs::symlink(
        &target,
        layout::host_log_path(home.path(), Instance::Production),
    )
    .expect("symlink");
    let system = System {
        dir: elsewhere.path(),
        root: true,
    };
    let error =
        prepare_log(&system, &account(home.path()), Instance::Production).expect_err("a link");
    assert!(error.contains("not a link"), "{error}");
    assert_eq!(std::fs::read(&target).expect("read"), b"untouched");
}

#[test]
fn a_host_directory_the_persona_does_not_own_is_refused() {
    let home = tempfile::tempdir().expect("tempdir");
    commission(home.path(), Instance::Production);
    let system = System {
        dir: home.path(),
        root: true,
    };
    let stranger = Account {
        uid: nix::unistd::getuid().as_raw() + 4242,
        ..account(home.path())
    };
    let error = prepare_log(&system, &stranger, Instance::Production).expect_err("not theirs");
    assert!(error.contains("is not owned by"), "{error}");
    assert!(!layout::host_log_path(home.path(), Instance::Production).exists());
}

/// Absent directories are created — `0700` at the host's own — when the
/// persona owns where they go.
#[test]
fn missing_state_directories_are_created_restricted() {
    let home = tempfile::tempdir().expect("tempdir");
    let system = System {
        dir: home.path(),
        root: true,
    };
    prepare_log(&system, &account(home.path()), Instance::Production).expect("prepare");
    let host_dir = layout::host_dir(home.path(), Instance::Production);
    let mode = std::fs::metadata(&host_dir)
        .expect("dir")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    assert!(layout::host_log_path(home.path(), Instance::Production).is_file());
    // And a second run leaves the existing log alone.
    prepare_log(&system, &account(home.path()), Instance::Production).expect("again");
}

#[test]
fn the_sudo_hint_quotes_what_a_shell_would_split() {
    assert_eq!(shell_quote("--user"), "--user");
    assert_eq!(shell_quote("/usr/local/bin/bee"), "/usr/local/bin/bee");
    assert_eq!(shell_quote("a b"), "'a b'");
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
    assert_eq!(shell_quote(""), "''");
}

// --- the user-mode side: what `status`, `install` and `uninstall` make of a
// daemon. Directories are injected, so none of this reads the machine's own
// `/Library/LaunchDaemons`.

fn write_agent(home: &Path, program: &Path) {
    let path = registration_path(Service::AgentHost, home, Instance::Production);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(
        &path,
        registration_contents(Service::AgentHost, Instance::Production, program, home),
    )
    .expect("write agent");
}

fn write_daemon(dir: &Path, home: &Path, program: &Path) -> PathBuf {
    let path = plist_path(dir, "persona", Instance::Production);
    std::fs::write(
        &path,
        plist_contents(&account(home), Instance::Production, program).expect("render"),
    )
    .expect("write daemon");
    path
}

#[test]
fn status_tells_the_four_registrations_apart() {
    let home = tempfile::tempdir().expect("tempdir");
    let daemons = tempfile::tempdir().expect("tempdir");
    let program = program(home.path());
    let lookup = Some(DaemonLookup {
        dir: daemons.path(),
        user: "persona",
    });
    let read = || {
        status_at(
            Service::AgentHost,
            home.path(),
            Instance::Production,
            lookup,
        )
    };

    let neither = read();
    assert!(!neither.installed);
    assert_eq!(neither.domain, Domain::User);
    assert!(neither.warnings.is_empty(), "{:?}", neither.warnings);

    write_agent(home.path(), &program);
    let agent = read();
    assert!(agent.installed);
    assert_eq!(agent.domain, Domain::User);

    std::fs::remove_file(registration_path(
        Service::AgentHost,
        home.path(),
        Instance::Production,
    ))
    .expect("remove agent");
    let daemon_path = write_daemon(daemons.path(), home.path(), &program);
    let daemon = read();
    assert!(daemon.installed);
    assert_eq!(daemon.domain, Domain::System);
    assert_eq!(daemon.path, daemon_path);
    assert_eq!(daemon.program, Some(program.clone()));
    assert!(daemon.warnings.is_empty(), "{:?}", daemon.warnings);
    assert_eq!(
        daemon.label().as_deref(),
        Some("io.agiterra.beekeeper.host.daemon.persona")
    );

    write_agent(home.path(), &program);
    let both = read();
    assert_eq!(both.domain, Domain::System);
    assert!(
        both.warnings.iter().any(|warning| warning.contains("race")),
        "two hosts for one socket must be disclosed: {:?}",
        both.warnings
    );

    // Without a lookup — another user's home, or not macOS — only the agent.
    let blind = status_at(Service::AgentHost, home.path(), Instance::Production, None);
    assert_eq!(blind.domain, Domain::User);
}

/// The desktop's repair poll must never try to rewrite a daemon: it cannot,
/// and the attempt would add a racing LaunchAgent.
#[test]
fn a_system_registration_never_asks_for_a_rewrite() {
    let daemon = Registration {
        installed: true,
        path: PathBuf::from("/Library/LaunchDaemons/x.plist"),
        program: Some(PathBuf::from("/nowhere/beekeeper-host")),
        warnings: Vec::new(),
        domain: Domain::System,
    };
    assert!(!daemon.needs_rewrite());
    assert!(!daemon.needs_repair(false, Some(false)));
    assert!(
        Registration {
            domain: Domain::User,
            ..daemon
        }
        .needs_rewrite(),
        "the same file in the user's domain would be rewritten"
    );
}

#[test]
fn a_user_install_refuses_beside_a_daemon() {
    let home = tempfile::tempdir().expect("tempdir");
    let daemons = tempfile::tempdir().expect("tempdir");
    let program = program(home.path());
    write_daemon(daemons.path(), home.path(), &program);
    let error = install_at(
        Service::AgentHost,
        home.path(),
        Instance::Production,
        &program,
        Some(DaemonLookup {
            dir: daemons.path(),
            user: "persona",
        }),
    )
    .expect_err("would race the daemon");
    assert!(
        error.contains("system daemon") && error.contains("sudo bee host uninstall --system"),
        "{error}"
    );
    assert!(!registration_path(Service::AgentHost, home.path(), Instance::Production).exists());
}

#[test]
fn a_user_uninstall_beside_a_daemon_removes_the_agent_and_still_fails() {
    let home = tempfile::tempdir().expect("tempdir");
    let daemons = tempfile::tempdir().expect("tempdir");
    let program = program(home.path());
    write_agent(home.path(), &program);
    let daemon = write_daemon(daemons.path(), home.path(), &program);
    let error = uninstall_at(
        Service::AgentHost,
        home.path(),
        Instance::Production,
        Some(DaemonLookup {
            dir: daemons.path(),
            user: "persona",
        }),
    )
    .expect_err("the daemon is still there");
    assert!(error.contains("cannot remove it"), "{error}");
    assert!(!registration_path(Service::AgentHost, home.path(), Instance::Production).exists());
    assert!(
        daemon.exists(),
        "a user uninstall must not touch root's file"
    );
}
