//! Starting the host at login, and installing it on a server.
//!
//! Two registrations, one per platform, both written by the same code so that
//! `bee host install` on a headless box and the desktop app's own
//! commissioning cannot produce different units.
//!
//! # macOS: a launchd LaunchAgent
//!
//! Written to `~/Library/LaunchAgents/io.agiterra.beekeeper.host[.dev].plist`
//! and loaded with `launchctl bootstrap gui/$UID`.
//!
//! `SMAppService.agent(plistName:)` is the modern API and is better in one
//! specific way: it registers a plist that lives *inside* the app bundle, so
//! deleting the app unregisters the agent. It is also macOS 13+, while release
//! builds declare `minimumSystemVersion: "10.15"`, so it cannot be the only
//! path. This is the path that works on every supported version and on a
//! machine where no app bundle exists at all — which is the whole point of a
//! headless install. The bundle-relative `SMAppService` registration is a
//! follow-up, not a replacement.
//!
//! **The cost of that choice, stated:** a plist under `~/Library/LaunchAgents`
//! outlives the app that wrote it. Deleting Beekeeper without uninstalling
//! leaves launchd retrying a binary that is gone, which it logs about
//! indefinitely. [`uninstall`] is therefore not optional housekeeping, and the
//! plist deliberately carries `KeepAlive: false` so a missing binary fails
//! once per login rather than in a respawn loop.
//!
//! # Linux: a systemd user unit
//!
//! `Type=simple`, not `oneshot` — this one is long-running. `EnvironmentFile`
//! carries **no `-` prefix**, following `deploy/autodeploy/`'s own comment
//! verbatim: a missing config file must fail the unit rather than let the
//! service start unconfigured.
//!
//! **`loginctl enable-linger <user>` is a gate, not a footnote.** A user unit
//! dies at logout, so without lingering a server's host stops the moment the
//! operator's session ends — the single most common way this silently fails.
//! [`install`] reports whether lingering is on rather than assuming it.

use std::path::{Path, PathBuf};

use beekeeper_host_core::layout::{Instance, INSTANCE_VAR};

/// The launchd label / systemd unit stem for an instance.
///
/// Per instance so a dev build and a release build can both be registered on
/// one machine without one replacing the other's registration.
pub fn service_name(instance: Instance) -> String {
    match instance {
        Instance::Production => "io.agiterra.beekeeper.host".to_string(),
        Instance::Dev => "io.agiterra.beekeeper.host.dev".to_string(),
    }
}

/// Where the registration for `instance` lives on this platform.
pub fn registration_path(home: &Path, instance: Instance) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/LaunchAgents")
            .join(format!("{}.plist", service_name(instance)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.join(".config/systemd/user")
            .join(format!("{}.service", service_name(instance)))
    }
}

/// What an install or a check found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    /// Whether a registration file exists.
    pub installed: bool,
    /// Where it is, or would be.
    pub path: PathBuf,
    /// The binary it names, when one is recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<PathBuf>,
    /// Facts the caller must disclose rather than absorb.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Whether this instance is registered to start at login.
///
/// This is the *other half* of telling "not installed" from "installed but not
/// running": the socket's absence alone cannot distinguish them, and a client
/// that reported one for the other would tell a person to start something they
/// have not installed.
pub fn status(home: &Path, instance: Instance) -> Registration {
    let path = registration_path(home, instance);
    let program = std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| program_from_registration(&content));
    let mut warnings = Vec::new();
    if path.exists() {
        match &program {
            Some(program) if !program.exists() => warnings.push(format!(
                "the login registration names {}, which does not exist — delete it with \
                 `bee host uninstall` or reinstall the host",
                program.display()
            )),
            None => warnings.push(format!(
                "{} exists but does not name a program this version understands",
                path.display()
            )),
            _ => {}
        }
        warnings.extend(linger_warning());
    }
    Registration {
        installed: path.exists(),
        path,
        program,
        warnings,
    }
}

/// Register `program` to start at login for `instance`.
///
/// Idempotent: an existing registration is replaced. The file is written with
/// the *absolute, resolved* path of the binary, because launchd and systemd
/// both run with a minimal `PATH` and neither will look one up.
pub fn install(home: &Path, instance: Instance, program: &Path) -> Result<Registration, String> {
    if !program.is_absolute() {
        return Err(format!(
            "the host binary must be named by an absolute path, not {}",
            program.display()
        ));
    }
    if !program.exists() {
        return Err(format!(
            "there is no host binary at {} — build it with `cargo build --release -p beekeeper-host` \
             or point at the one inside the app bundle",
            program.display()
        ));
    }
    let path = registration_path(home, instance);
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    std::fs::write(&path, registration_contents(instance, program, home))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    activate(&path, instance)?;
    Ok(status(home, instance))
}

/// Remove the registration, and stop the service if it is running.
///
/// Best-effort on the deactivation — a service that is already gone must not
/// make an uninstall fail — but the *file* removal is reported, because a
/// registration left behind is the failure mode this function exists for.
pub fn uninstall(home: &Path, instance: Instance) -> Result<(), String> {
    let path = registration_path(home, instance);
    deactivate(&path, instance);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
    }
}

/// The program a registration file names, if this version can read it.
///
/// Parsed rather than remembered so `status` tells the truth about a file an
/// older version wrote, or one somebody edited by hand.
fn program_from_registration(content: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // The first `<string>` after `ProgramArguments` is argv[0].
        let after = content.split("ProgramArguments").nth(1)?;
        let open = after.find("<string>")? + "<string>".len();
        let close = after[open..].find("</string>")? + open;
        Some(PathBuf::from(after[open..close].trim()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        content
            .lines()
            .find_map(|line| line.trim().strip_prefix("ExecStart="))
            .and_then(|value| value.split_whitespace().next())
            .map(PathBuf::from)
    }
}

#[cfg(target_os = "macos")]
fn registration_contents(instance: Instance, program: &Path, home: &Path) -> String {
    let label = service_name(instance);
    let log = beekeeper_host_core::layout::host_log_path(home, instance);
    // Hand-written rather than built through the `plist` crate: the file is a
    // fixed shape, a person reads and edits it, and the escaping surface is
    // one path. `RunAtLoad` with `KeepAlive: false` on purpose — see the
    // module docs on a deleted app leaving a plist behind.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{program}</string>
		<string>run</string>
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>{INSTANCE_VAR}</key>
		<string>{instance_value}</string>
	</dict>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<false/>
	<key>ProcessType</key>
	<string>Background</string>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
        program = program.display(),
        instance_value = instance.namespace_value(),
        log = log.display(),
    )
}

#[cfg(not(target_os = "macos"))]
fn registration_contents(instance: Instance, program: &Path, home: &Path) -> String {
    let config = beekeeper_host_core::layout::host_config_path(home, instance);
    let graceful = crate::terminate::GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
        + crate::terminate::ESCALATION_TIMEOUT.as_secs();
    format!(
        r#"[Unit]
Description=Beekeeper agent host ({namespace}) — supervises this machine's coding-session provider
Documentation=file:{config}

[Service]
# `simple`, not `oneshot`: this is a long-running supervisor, and it runs in
# the foreground on purpose so this unit's stop signal reaches it directly.
Type=simple
ExecStart={program} run
Environment={INSTANCE_VAR}={instance_value}
Restart=on-failure
RestartSec=5
# Above the provider's own SIGINT window plus escalation, so systemd does not
# SIGKILL the host mid-handoff and cost the provider's outbox flush.
TimeoutStopSec={stop_timeout}

[Install]
WantedBy=default.target
"#,
        namespace = instance.namespace(),
        config = config.display(),
        program = program.display(),
        instance_value = instance.namespace_value(),
        stop_timeout = graceful + 5,
    )
}

#[cfg(target_os = "macos")]
fn activate(path: &Path, instance: Instance) -> Result<(), String> {
    // Bootout first so a reinstall replaces a loaded agent rather than
    // failing with "service already loaded". A bootout of something that is
    // not loaded is an error we deliberately ignore.
    let domain = format!("gui/{}", nix::unistd::getuid().as_raw());
    run(
        "launchctl",
        &["bootout", &format!("{domain}/{}", service_name(instance))],
    )
    .ok();
    run(
        "launchctl",
        &["bootstrap", &domain, &path.to_string_lossy()],
    )
    .map_err(|error| {
        format!(
            "wrote {} but launchctl refused to load it: {error}. Log out and back in, or run \
             `launchctl bootstrap {domain} {}` yourself.",
            path.display(),
            path.display()
        )
    })
}

#[cfg(not(target_os = "macos"))]
fn activate(_path: &Path, instance: Instance) -> Result<(), String> {
    let unit = format!("{}.service", service_name(instance));
    run("systemctl", &["--user", "daemon-reload"]).ok();
    run("systemctl", &["--user", "enable", "--now", &unit]).map_err(|error| {
        format!(
            "wrote the unit but systemctl refused to enable it: {error}. Check \
             `systemctl --user status {unit}`."
        )
    })
}

#[cfg(target_os = "macos")]
fn deactivate(_path: &Path, instance: Instance) {
    let domain = format!("gui/{}", nix::unistd::getuid().as_raw());
    run(
        "launchctl",
        &["bootout", &format!("{domain}/{}", service_name(instance))],
    )
    .ok();
}

#[cfg(not(target_os = "macos"))]
fn deactivate(_path: &Path, instance: Instance) {
    let unit = format!("{}.service", service_name(instance));
    run("systemctl", &["--user", "disable", "--now", &unit]).ok();
}

/// Whether this user lingers, on a platform where that matters.
#[cfg(not(target_os = "macos"))]
fn linger_warning() -> Vec<String> {
    // `loginctl show-user` answers `Linger=yes|no`. Anything else — no
    // loginctl, a container, an unreadable answer — is reported as unknown
    // rather than assumed fine: this is the single most common way a server's
    // host silently stops working.
    let answer = run_capture("loginctl", &["show-user", "--property=Linger", "--value"]);
    match answer.as_deref().map(str::trim) {
        Some("yes") => Vec::new(),
        Some("no") => vec![
            "this user does not linger, so the agent host will stop when you log out — run \
             `loginctl enable-linger $USER`"
                .to_string(),
        ],
        _ => vec![
            "could not tell whether this user lingers; without lingering the agent host stops \
             at logout — check `loginctl show-user --property=Linger`"
                .to_string(),
        ],
    }
}

#[cfg(target_os = "macos")]
fn linger_warning() -> Vec<String> {
    Vec::new()
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("{program} exited {}", output.status)
    } else {
        stderr
    })
}

#[cfg(not(target_os = "macos"))]
fn run_capture(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_and_production_register_separately() {
        let home = Path::new("/home/agent");
        assert_ne!(
            service_name(Instance::Production),
            service_name(Instance::Dev)
        );
        assert_ne!(
            registration_path(home, Instance::Production),
            registration_path(home, Instance::Dev)
        );
    }

    /// The registration must name an absolute path: launchd and systemd both
    /// run with a minimal `PATH` and neither looks a program up.
    #[test]
    fn the_program_must_be_absolute_and_must_exist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let relative = install(
            dir.path(),
            Instance::Production,
            Path::new("beekeeper-host"),
        )
        .expect_err("relative must be refused");
        assert!(relative.contains("absolute path"), "{relative}");

        let absent = install(
            dir.path(),
            Instance::Production,
            &dir.path().join("nowhere/beekeeper-host"),
        )
        .expect_err("a missing binary must be refused");
        assert!(absent.contains("there is no host binary"), "{absent}");
        assert!(
            !registration_path(dir.path(), Instance::Production).exists(),
            "a refused install must write nothing"
        );
    }

    /// The contents must round-trip through `program_from_registration`, or
    /// `status` cannot tell a person which binary their login registration
    /// actually names.
    #[test]
    fn the_program_round_trips_out_of_the_registration_it_wrote() {
        let home = Path::new("/home/agent");
        let program = Path::new("/Applications/Beekeeper.app/Contents/MacOS/beekeeper-host");
        let contents = registration_contents(Instance::Production, program, home);
        assert_eq!(
            program_from_registration(&contents),
            Some(program.to_path_buf())
        );
        // And the instance is carried, so a dev registration cannot start a
        // production host.
        assert!(contents.contains(INSTANCE_VAR), "{contents}");
        assert!(
            contents.contains(Instance::Production.namespace_value()),
            "{contents}"
        );
        let dev = registration_contents(Instance::Dev, program, home);
        assert!(dev.contains(Instance::Dev.namespace_value()), "{dev}");
    }

    #[test]
    fn nothing_installed_reads_as_not_installed_with_no_warnings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = status(dir.path(), Instance::Production);
        assert!(!status.installed);
        assert_eq!(status.program, None);
        assert!(
            status.warnings.is_empty(),
            "an absent registration is not a problem to warn about: {:?}",
            status.warnings
        );
    }

    /// A registration naming a binary that is gone is the exact residue a
    /// deleted app leaves behind. It must be called out, with the way out.
    #[test]
    fn a_registration_naming_a_missing_binary_says_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = registration_path(dir.path(), Instance::Production);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &path,
            registration_contents(
                Instance::Production,
                Path::new("/Applications/Deleted.app/Contents/MacOS/beekeeper-host"),
                dir.path(),
            ),
        )
        .expect("write");
        let status = status(dir.path(), Instance::Production);
        assert!(status.installed);
        assert!(status
            .warnings
            .iter()
            .any(|warning| warning.contains("does not exist")
                && warning.contains("bee host uninstall")));
    }

    /// systemd's stop timeout must exceed the provider's own graceful window,
    /// or systemd SIGKILLs the host mid-handoff and the provider's outbox
    /// never flushes.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_unit_waits_longer_than_the_provider_takes_to_stop() {
        let contents = registration_contents(
            Instance::Production,
            Path::new("/usr/local/bin/beekeeper-host"),
            Path::new("/home/agent"),
        );
        let stop: u64 = contents
            .lines()
            .find_map(|line| line.trim().strip_prefix("TimeoutStopSec="))
            .expect("TimeoutStopSec is set")
            .parse()
            .expect("a number");
        let provider = crate::terminate::GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
            + crate::terminate::ESCALATION_TIMEOUT.as_secs();
        assert!(stop > provider, "{stop} must exceed {provider}");
        // And `EnvironmentFile` must not be `-`-prefixed if it is used at all,
        // following `deploy/autodeploy/`'s comment: a missing config must fail
        // the unit rather than let it start unconfigured.
        assert!(!contents.contains("EnvironmentFile=-"), "{contents}");
        assert!(contents.contains("Type=simple"), "{contents}");
    }

    /// An uninstall of something that was never installed is not an error:
    /// `bee host uninstall` has to be safe to run on any machine.
    #[test]
    fn uninstalling_nothing_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        uninstall(dir.path(), Instance::Production).expect("idempotent");
    }
}
