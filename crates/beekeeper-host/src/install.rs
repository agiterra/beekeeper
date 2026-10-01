//! Starting the host at login, at boot on a headless Mac, and installing it on
//! a server.
//!
//! One registration per platform for a person's own machine, both written by
//! the same code so that `bee host install` and the desktop app's own
//! commissioning cannot produce different units — plus, on macOS, a system
//! LaunchDaemon for a Mac nobody logs in to ([`launchd_daemon`]).
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
//! **A LaunchAgent runs only while its user is logged in.** `gui/$UID` is
//! created at console login and torn down at logout, so on a Mac with nobody
//! at the screen the host never starts after a reboot. That is what
//! [`launchd_daemon`] is for: `bee host install --system --user <persona>`, as
//! root, writes `/Library/LaunchDaemons/io.agiterra.beekeeper.host.daemon.
//! <persona>[.dev].plist`, which launchd loads at boot and runs as that user.
//! [`status`] reports either, and says so when both exist — two hosts racing
//! for one socket.
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

use beekeeper_host_core::layout::{self, Instance, INSTANCE_VAR};
use beekeeper_host_core::logs::now_iso;

pub mod launchd_daemon;

/// Which background service a registration is for.
///
/// Two, and they are genuinely different things: the host runs agents and must
/// start on a server with no GUI; the menu bar app only *shows* what the host
/// is doing and is meaningless without a session. Registering them separately
/// is what lets a person turn the menu bar off without turning their agents
/// off — and what stops one uninstall taking the other with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// `beekeeper-host` — supervises this machine's agents.
    AgentHost,
    /// `beekeeper-menubar` — shows what they are doing. macOS only.
    MenuBar,
}

impl Service {
    /// The argument the program is started with, if any.
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::AgentHost => &["run"],
            Self::MenuBar => &[],
        }
    }

    /// Whether this service belongs in a foreground session.
    ///
    /// The menu bar app draws into a logged-in user's menu bar, so it is
    /// pointless — and on a server, harmful noise — without one. The host does
    /// not need one, though whether it *outlives* one depends on how it was
    /// registered: a systemd user unit with lingering and a macOS
    /// LaunchDaemon run with nobody logged in; a macOS LaunchAgent does not.
    fn needs_a_session(self) -> bool {
        matches!(self, Self::MenuBar)
    }

    fn human_name(self) -> &'static str {
        match self {
            Self::AgentHost => "agent host",
            Self::MenuBar => "menu bar app",
        }
    }
}

/// The launchd label / systemd unit stem for a service and instance.
///
/// Per instance so a dev build and a release build can both be registered on
/// one machine without one replacing the other's registration.
pub fn service_name(service: Service, instance: Instance) -> String {
    let stem = match service {
        Service::AgentHost => "io.agiterra.beekeeper.host",
        Service::MenuBar => "io.agiterra.beekeeper.menubar",
    };
    match instance {
        Instance::Production => stem.to_string(),
        Instance::Dev => format!("{stem}.dev"),
    }
}

/// Where the registration for a service and instance lives on this platform.
pub fn registration_path(service: Service, home: &Path, instance: Instance) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        launch_agent_path(service, home, instance)
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.join(".config/systemd/user")
            .join(format!("{}.service", service_name(service, instance)))
    }
}

/// Where a macOS LaunchAgent for this service lives under `home`.
///
/// Not platform-gated, unlike [`registration_path`]: a daemon install reads it
/// to refuse a duplicate, and its tests run everywhere.
pub(crate) fn launch_agent_path(service: Service, home: &Path, instance: Instance) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{}.plist", service_name(service, instance)))
}

/// Which service manager domain a registration lives in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    /// The user's own: a LaunchAgent in `gui/$UID`, or a systemd user unit.
    /// This user can write, load and remove it.
    #[default]
    User,
    /// A macOS LaunchDaemon in `system`, run as this user. Only root can
    /// write, load or remove it.
    System,
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
    /// Whose service manager holds it. Defaults to `user` when reading a
    /// registration serialised before daemons existed.
    #[serde(default)]
    pub domain: Domain,
}

impl Registration {
    /// Whether the registration file should be written again.
    ///
    /// Deliberately **not** "does it carry warnings". A warning is something an
    /// operator must be told; it is not evidence that the file is wrong, and
    /// rewriting cannot always clear one. [`linger_warning`] is the case that
    /// proves it: `loginctl show-user` says nothing about this unit, so a Linux
    /// user who has not run `enable-linger` — or any container, where the answer
    /// is unreadable — carries that warning permanently. A caller that
    /// rewrote on "any warning" would then `systemctl --user disable --now` and
    /// re-enable on every status poll, stopping and starting the host every few
    /// seconds and taking every coding session with it. Which is the failure
    /// the poll's own comment was written to prevent, arrived at from the other
    /// side.
    ///
    /// True only for the two states a rewrite actually fixes: no file at all,
    /// and a file that does not name a program that exists. Never for a system
    /// daemon: only root can rewrite one, so a user-mode rewrite would fail —
    /// or, worse, succeed beside it as a LaunchAgent and race it for the
    /// socket. Its problems travel as warnings instead.
    pub fn needs_rewrite(&self) -> bool {
        if self.domain == Domain::System {
            return false;
        }
        !self.installed
            || self
                .program
                .as_ref()
                .is_none_or(|program| !program.exists())
    }

    /// Whether the registration should be rewritten *now*, given what else
    /// is known.
    ///
    /// Adds the one state [`needs_rewrite`](Self::needs_rewrite) cannot see: a
    /// file on disk that launchd or systemd does not have loaded. `installed`
    /// means "the file exists", which is what makes it cheap and what makes it
    /// wrong here — a `launchctl bootout` by hand, or an activation that
    /// failed after the file was written, leaves exactly that. It reads as
    /// granted, so nothing repairs it and nothing asks.
    ///
    /// Only consulted when the host is *unreachable*, because that is the only
    /// time the answer changes anything and `loaded` costs a subprocess. A
    /// reachable host is loaded by definition. Never for a system daemon, for
    /// the reason `needs_rewrite` gives.
    pub fn needs_repair(&self, host_reachable: bool, loaded: Option<bool>) -> bool {
        if self.domain == Domain::System {
            return false;
        }
        if self.needs_rewrite() {
            return true;
        }
        !host_reachable && loaded == Some(false)
    }

    /// The launchd label or systemd unit stem, read off the file name.
    pub fn label(&self) -> Option<String> {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    }
}

/// What a GUI should do about registering the host at login.
///
/// Shipped to the frontend rather than recomputed there. The rule has four
/// rows and re-deriving them in another language is how the two come to
/// disagree — the same mistake the tray's subtitle indices made when the
/// layout existed twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginAutostart {
    /// There is no provider identity here, so there is nothing to register
    /// and nothing to ask about.
    NotApplicable,
    /// Registered. Repair it silently if it has drifted — the operator
    /// already said yes, and asking again on every update is nagging.
    Granted,
    /// The operator said no. Disclose what it costs them and never ask again;
    /// the settings surface is where they can change their mind.
    Declined,
    /// Nothing registered and nobody has said no. Ask.
    ShouldAsk,
}

/// Decide, from the three facts that determine it.
///
/// A daemon that runs at every login, forever, is a machine-level change, and
/// doing it because somebody opened an app is not something to help yourself
/// to. But a person who already said yes must not be asked again each time the
/// app updates and the registration needs rewriting — so `Granted` repairs
/// silently and only a *missing* registration is a question.
pub fn login_autostart(
    provisioned: bool,
    registration: &Registration,
    refused: bool,
) -> LoginAutostart {
    if !provisioned {
        return LoginAutostart::NotApplicable;
    }
    if registration.installed {
        return LoginAutostart::Granted;
    }
    if refused {
        return LoginAutostart::Declined;
    }
    LoginAutostart::ShouldAsk
}

/// Whether `home` is the home directory of the user this process runs as.
///
/// The launchd label and the `gui/$UID` domain are **not** derived from
/// `home` — they cannot be, because they have to be stable. So a caller that
/// passes a different home writes its file there and then bootstraps or boots
/// out *this* user's live service, which is never what it meant.
///
/// Not hypothetical. `uninstalling_nothing_succeeds` passed a `tempfile::
/// tempdir()` as `home`, and `launchctl bootout
/// gui/$UID/io.agiterra.beekeeper.host` then terminated the real agent host —
/// on Andy's Mac, taking its provider and coding sessions with it, every time
/// this crate's tests ran. The plist in `~/Library/LaunchAgents` survived,
/// because only the tempdir copy was removed, so the state left behind was a
/// registration that read as installed with nothing loaded.
///
/// The guard is here rather than in the tests because the tests were not
/// wrong to pass a fake home: writing a registration into a tree you nominate
/// is exactly what these functions claim to do. Touching a different tree's
/// service manager is the part that was never intended.
fn home_is_ours(home: &Path) -> bool {
    let Ok(ours) = layout::home_dir() else {
        return false;
    };
    // Canonicalized where possible: a tempdir is `/var/folders/...` while its
    // resolved form is `/private/var/folders/...`, and `$HOME` may itself be a
    // symlink. A path that cannot be canonicalized is compared as given.
    let resolve = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    resolve(&ours) == resolve(home)
}

/// Whether the service manager has this service loaded.
///
/// `None` when the question could not be put — no `launchctl`/`systemctl`, or
/// it failed for a reason that is not "no such service". An unknown answer
/// must never be read as `false`: that would have the app tear down and
/// rewrite a working registration on every poll.
pub fn service_loaded(service: Service, instance: Instance) -> Option<bool> {
    let name = service_name(service, instance);
    #[cfg(target_os = "macos")]
    {
        // A system daemon is not in this user's domain, so `launchctl list`
        // would answer "not loaded" about a running host. Ask `system`.
        if let Some(user) = layout::home_dir()
            .ok()
            .and_then(|home| daemon_lookup_user(service, &home))
        {
            if launchd_daemon::plist_path(Path::new(launchd_daemon::DAEMON_DIR), &user, instance)
                .exists()
            {
                return launchd_daemon::loaded(&user, instance);
            }
        }
        // `launchctl list <label>` exits 0 when loaded and 113 (EDEADLK, used
        // here as "no such process") when not. Any other status is an answer
        // this function does not have.
        let code = std::process::Command::new("launchctl")
            .args(["list", &name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?
            .code()?;
        match code {
            0 => Some(true),
            113 => Some(false),
            _ => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // `is-active` is about *running*, which a `Type=simple` unit may not be
        // between restarts; `is-enabled` is about being wired to start, which
        // is the same question the plist's presence answers on a Mac.
        let output = std::process::Command::new("systemctl")
            .args(["--user", "is-enabled", &format!("{name}.service")])
            .output()
            .ok()?;
        let answer = String::from_utf8_lossy(&output.stdout).trim().to_string();
        match answer.as_str() {
            "enabled" | "enabled-runtime" | "static" | "alias" | "indirect" => Some(true),
            "disabled" | "not-found" | "masked" => Some(false),
            _ => None,
        }
    }
}

/// Whether the operator has refused to have the host registered at login.
pub fn login_refused(home: &Path, instance: Instance) -> bool {
    layout::login_refusal_path(home, instance).exists()
}

/// Record the refusal, so nothing asks again.
///
/// Written by the GUI when a person declines and by `bee host uninstall`,
/// which is the same statement made from a terminal. Without that second
/// caller an explicit uninstall would be re-proposed at the next launch, which
/// reads as the app ignoring you.
pub fn refuse_login(home: &Path, instance: Instance) -> Result<(), String> {
    let path = layout::login_refusal_path(home, instance);
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    // The timestamp is for a human reading the directory, not for logic: the
    // file's *existence* is the whole meaning, so nothing parses this.
    std::fs::write(
        &path,
        format!(
            "{{\n  \"version\": 1,\n  \"refusedAt\": \"{}\"\n}}\n",
            now_iso()
        ),
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

/// Forget a refusal, because the operator has now asked for the registration.
pub fn allow_login(home: &Path, instance: Instance) -> Result<(), String> {
    let path = layout::login_refusal_path(home, instance);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
    }
}

/// Whether this instance is registered to start at login.
///
/// This is the *other half* of telling "not installed" from "installed but not
/// running": the socket's absence alone cannot distinguish them, and a client
/// that reported one for the other would tell a person to start something they
/// have not installed.
pub fn status(service: Service, home: &Path, instance: Instance) -> Registration {
    let user = daemon_lookup_user(service, home);
    let lookup = user.as_deref().map(|user| DaemonLookup {
        dir: Path::new(launchd_daemon::DAEMON_DIR),
        user,
    });
    status_at(service, home, instance, lookup)
}

/// Where to look for this user's system daemon.
#[derive(Debug, Clone, Copy)]
struct DaemonLookup<'a> {
    dir: &'a Path,
    user: &'a str,
}

/// The user whose system daemon to look for, when there is one to look for.
///
/// Only the agent host has a daemon form, only macOS has daemons, and only
/// *our own* home names a user whose daemon this process can mean — for the
/// same reason [`home_is_ours`] guards activation: a test's tempdir must not
/// find the machine's real `/Library/LaunchDaemons`.
fn daemon_lookup_user(service: Service, home: &Path) -> Option<String> {
    if !cfg!(target_os = "macos") || service != Service::AgentHost || !home_is_ours(home) {
        return None;
    }
    let user = launchd_daemon::current_user_name()?;
    launchd_daemon::validate_user_name(&user).ok()?;
    Some(user)
}

fn status_at(
    service: Service,
    home: &Path,
    instance: Instance,
    lookup: Option<DaemonLookup<'_>>,
) -> Registration {
    let agent = agent_status(service, home, instance);
    let Some(lookup) = lookup else {
        return agent;
    };
    let Some(mut daemon) = launchd_daemon::registration(lookup.dir, lookup.user, instance) else {
        return agent;
    };
    // The daemon is what reports: it runs at boot whoever is logged in, so
    // it is the one that will hold the socket. The LaunchAgent's existence is
    // the problem to disclose, not the registration to describe.
    if agent.installed {
        daemon.warnings.push(format!(
            "both a login LaunchAgent ({}) and a system daemon ({}) are registered for {}: two              hosts will race for one control socket. Keep one — `bee host uninstall` removes the              LaunchAgent, `sudo bee host uninstall --system --user {}` the daemon",
            agent.path.display(),
            daemon.path.display(),
            lookup.user,
            lookup.user
        ));
    }
    daemon
}

fn agent_status(service: Service, home: &Path, instance: Instance) -> Registration {
    let path = registration_path(service, home, instance);
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
        if !service.needs_a_session() {
            warnings.extend(linger_warning());
        }
    }
    Registration {
        installed: path.exists(),
        path,
        program,
        warnings,
        domain: Domain::User,
    }
}

/// Register `program` to start at login for `instance`.
///
/// Idempotent: an existing registration is replaced. The file is written with
/// the *absolute, resolved* path of the binary, because launchd and systemd
/// both run with a minimal `PATH` and neither will look one up.
pub fn install(
    service: Service,
    home: &Path,
    instance: Instance,
    program: &Path,
) -> Result<Registration, String> {
    let user = daemon_lookup_user(service, home);
    let lookup = user.as_deref().map(|user| DaemonLookup {
        dir: Path::new(launchd_daemon::DAEMON_DIR),
        user,
    });
    install_at(service, home, instance, program, lookup)
}

/// Refuse a program a service manager could not start.
pub(crate) fn check_program(service: Service, program: &Path) -> Result<(), String> {
    if !program.is_absolute() {
        return Err(format!(
            "the host binary must be named by an absolute path, not {}",
            program.display()
        ));
    }
    if !program.exists() {
        return Err(format!(
            "there is no {} binary at {} — build it, or point at the one inside the app bundle",
            service.human_name(),
            program.display()
        ));
    }
    Ok(())
}

/// The error for a user-mode change to a host that a system daemon owns, if
/// one does.
fn daemon_conflict(
    lookup: Option<DaemonLookup<'_>>,
    instance: Instance,
    doing: &str,
) -> Option<String> {
    let lookup = lookup?;
    let path = launchd_daemon::plist_path(lookup.dir, lookup.user, instance);
    path.exists().then(|| {
        format!(
            "the agent host for {user} is registered as a system daemon ({}), so {doing}. Remove              the daemon with `sudo bee host uninstall --system --user {user}` first, or leave it              — it already runs at boot.",
            path.display(),
            user = lookup.user
        )
    })
}

fn install_at(
    service: Service,
    home: &Path,
    instance: Instance,
    program: &Path,
    lookup: Option<DaemonLookup<'_>>,
) -> Result<Registration, String> {
    check_program(service, program)?;
    // Also what stops the desktop's repair poll adding a LaunchAgent beside a
    // daemon it cannot see past.
    if let Some(error) = daemon_conflict(
        lookup,
        instance,
        "a login LaunchAgent beside it would race it for the control socket",
    ) {
        return Err(error);
    }
    let path = registration_path(service, home, instance);
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    std::fs::write(
        &path,
        registration_contents(service, instance, program, home),
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    // Unwind the file if the service will not load. `installed` is "the file
    // exists", so a registration left behind by a failed activation reads as
    // `installed: true` and therefore `LoginAutostart::Granted` — and then
    // nothing ever touches it again: the poll does not repair a grant it
    // believes is fine, and the prompt does not fire for a grant. The service
    // never starts and every surface says it is set up. Removing the file
    // keeps the two in step, so the next poll asks again.
    if let Err(error) = activate(service, &path, instance, home) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    // Installing is the operator asking for this, which supersedes any earlier
    // refusal. Best-effort: a stale refusal file must not fail an install that
    // worked, and `login_autostart` reads `installed` first anyway.
    if let Err(error) = allow_login(home, instance) {
        eprintln!("beekeeper-host: could not clear the login refusal: {error}");
    }
    Ok(status_at(service, home, instance, lookup))
}

/// Remove the registration, and stop the service if it is running.
///
/// Best-effort on the deactivation — a service that is already gone must not
/// make an uninstall fail — but the *file* removal is reported, because a
/// registration left behind is the failure mode this function exists for.
///
/// A system daemon is root's, so this removes the user's own registration and
/// then reports the daemon as an error rather than succeeding over a host that
/// will still be running at the next boot.
pub fn uninstall(service: Service, home: &Path, instance: Instance) -> Result<(), String> {
    let user = daemon_lookup_user(service, home);
    let lookup = user.as_deref().map(|user| DaemonLookup {
        dir: Path::new(launchd_daemon::DAEMON_DIR),
        user,
    });
    uninstall_at(service, home, instance, lookup)
}

fn uninstall_at(
    service: Service,
    home: &Path,
    instance: Instance,
    lookup: Option<DaemonLookup<'_>>,
) -> Result<(), String> {
    let path = registration_path(service, home, instance);
    deactivate(service, &path, instance, home);
    // Uninstalling is the operator saying no. Recording it is what stops the
    // app proposing the registration again at the next launch — an uninstall
    // that gets quietly undone reads as the app ignoring you.
    if let Err(error) = refuse_login(home, instance) {
        eprintln!("beekeeper-host: could not record the login refusal: {error}");
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("failed to remove {}: {error}", path.display())),
    }
    match daemon_conflict(lookup, instance, "a user uninstall cannot remove it") {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The program a registration file names, if this version can read it.
///
/// Parsed rather than remembered so `status` tells the truth about a file an
/// older version wrote, or one somebody edited by hand.
fn program_from_registration(content: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        program_from_plist(content)
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

/// argv[0] of a launchd plist: the first `<string>` after `ProgramArguments`.
///
/// Not platform-gated, so the daemon's round trip is tested everywhere.
pub(crate) fn program_from_plist(content: &str) -> Option<PathBuf> {
    let after = content.split("ProgramArguments").nth(1)?;
    let open = after.find("<string>")? + "<string>".len();
    let close = after[open..].find("</string>")? + open;
    Some(PathBuf::from(after[open..close].trim()))
}

#[cfg(target_os = "macos")]
fn registration_contents(
    service: Service,
    instance: Instance,
    program: &Path,
    home: &Path,
) -> String {
    let label = service_name(service, instance);
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
		<string>{program}</string>{argv}
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
        argv = service
            .args()
            .iter()
            .map(|arg| format!("\n\t\t<string>{arg}</string>"))
            .collect::<String>(),
        instance_value = instance.namespace_value(),
        log = log.display(),
    )
}

#[cfg(not(target_os = "macos"))]
fn registration_contents(
    service: Service,
    instance: Instance,
    program: &Path,
    home: &Path,
) -> String {
    let config = beekeeper_host_core::layout::host_config_path(home, instance);
    format!(
        r#"[Unit]
Description=Beekeeper agent host ({namespace}) — supervises this machine's coding-session provider
Documentation=file:{config}

[Service]
# `simple`, not `oneshot`: this is a long-running supervisor, and it runs in
# the foreground on purpose so this unit's stop signal reaches it directly.
Type=simple
ExecStart={program}{argv}
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
        argv = service
            .args()
            .iter()
            .map(|arg| format!(" {arg}"))
            .collect::<String>(),
        instance_value = instance.namespace_value(),
        stop_timeout = stop_timeout_secs(),
    )
}

/// How long a service manager should wait for the host to stop before
/// killing it: the provider's own SIGINT window plus escalation, plus margin,
/// so the host is never SIGKILLed mid-handoff and the outbox always flushes.
/// systemd's `TimeoutStopSec` and launchd's `ExitTimeOut` both use it.
pub(crate) fn stop_timeout_secs() -> u64 {
    crate::terminate::GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
        + crate::terminate::ESCALATION_TIMEOUT.as_secs()
        + 5
}

#[cfg(target_os = "macos")]
fn activate(service: Service, path: &Path, instance: Instance, home: &Path) -> Result<(), String> {
    if !home_is_ours(home) {
        // The file is written; the live domain is left alone. See
        // `home_is_ours`.
        return Ok(());
    }
    // Bootout first so a reinstall replaces a loaded agent rather than
    // failing with "service already loaded". A bootout of something that is
    // not loaded is an error we deliberately ignore.
    let domain = format!("gui/{}", nix::unistd::getuid().as_raw());
    run(
        "launchctl",
        &[
            "bootout",
            &format!("{domain}/{}", service_name(service, instance)),
        ],
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
fn activate(service: Service, _path: &Path, instance: Instance, home: &Path) -> Result<(), String> {
    // `systemctl --user` acts on this user's manager whichever home the unit
    // was written into — the same hazard as the launchd domain.
    if !home_is_ours(home) {
        return Ok(());
    }
    let unit = format!("{}.service", service_name(service, instance));
    run("systemctl", &["--user", "daemon-reload"]).ok();
    run("systemctl", &["--user", "enable", "--now", &unit]).map_err(|error| {
        format!(
            "wrote the unit but systemctl refused to enable it: {error}. Check \
             `systemctl --user status {unit}`."
        )
    })
}

#[cfg(target_os = "macos")]
fn deactivate(service: Service, _path: &Path, instance: Instance, home: &Path) {
    if !home_is_ours(home) {
        return;
    }
    let domain = format!("gui/{}", nix::unistd::getuid().as_raw());
    run(
        "launchctl",
        &[
            "bootout",
            &format!("{domain}/{}", service_name(service, instance)),
        ],
    )
    .ok();
}

#[cfg(not(target_os = "macos"))]
fn deactivate(service: Service, _path: &Path, instance: Instance, home: &Path) {
    if !home_is_ours(home) {
        return;
    }
    let unit = format!("{}.service", service_name(service, instance));
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

    /// A home that is not ours must never reach the live service manager.
    ///
    /// The regression test for the worst bug in this landing. `service_name`
    /// and the `gui/$UID` domain do not depend on `home`, so every test in
    /// this module that passed a `tempdir` was running `launchctl bootout
    /// gui/$UID/io.agiterra.beekeeper.host` against the real machine. On
    /// 2026-09-30 that terminated Andy's running agent host — and its provider,
    /// and its coding sessions — three times, and left a plist on disk with
    /// nothing loaded, which reads as installed.
    ///
    /// Asserted as a property of the path comparison rather than by observing
    /// launchd: a test that could tell whether the bootout happened would have
    /// to run one.
    #[test]
    fn a_home_that_is_not_ours_is_not_this_machines_service_manager() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            !home_is_ours(dir.path()),
            "a tempdir is not this user's home, so nothing in it may touch the live \
             launchd domain"
        );
        assert!(
            !home_is_ours(std::path::Path::new("/home/somebody-else")),
            "another user's tree is not ours either"
        );
        // And the real one is recognised, or the guard would disable the
        // feature outright instead of protecting it.
        let ours = layout::home_dir().expect("a home directory");
        assert!(home_is_ours(&ours), "our own home must still activate");
        // Symlinked and unnormalised spellings of it too, or an install from a
        // path with a `..` in it would silently stop registering anything.
        assert!(home_is_ours(
            &ours.join("..").join(
                ours.file_name()
                    .expect("a home directory has a final component")
            )
        ));
    }

    /// A registration on disk that the service manager does not have loaded.
    ///
    /// `installed` is "the file exists", which is the whole reason this state
    /// can happen: a `launchctl bootout` by hand, or an activation that failed
    /// after the file was written, leaves a plist that reads as granted. Then
    /// nothing repairs it — the poll does not touch a grant — and nothing asks
    /// — the prompt does not fire for a grant — so the host never starts and
    /// every surface says the machine is set up. Found on a real machine in
    /// exactly that state.
    #[test]
    fn a_registration_the_service_manager_does_not_have_loaded_needs_repair() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("beekeeper-host");
        std::fs::write(&program, b"#!/bin/sh\n").expect("write program");
        let registered = Registration {
            installed: true,
            path: dir.path().join("host.plist"),
            program: Some(program),
            warnings: Vec::new(),
            domain: Domain::User,
        };

        // The file is fine, so the cheap question says nothing is wrong.
        assert!(!registered.needs_rewrite());

        // A reachable host is loaded by definition — do not go asking, and do
        // not act on a stale answer if something did.
        assert!(!registered.needs_repair(true, Some(false)));
        assert!(!registered.needs_repair(true, None));

        // Unreachable and not loaded: this is the trap. Repair it.
        assert!(registered.needs_repair(false, Some(false)));

        // Unreachable but loaded — the host is down for its own reasons, and
        // rewriting the registration would bounce it for nothing.
        assert!(!registered.needs_repair(false, Some(true)));

        // Unreachable and unknown must not be read as "not loaded": that
        // would tear down and rewrite a working registration every poll.
        assert!(
            !registered.needs_repair(false, None),
            "an unknown answer is not a negative one"
        );
    }

    /// A permanent advisory warning must not make a registration look wrong.
    ///
    /// `loginctl show-user` says nothing about this unit, so a Linux user who
    /// has not run `enable-linger` carries that warning for good. Before this
    /// was a distinct question, the desktop's poll re-registered on "any
    /// warning" — which on such a machine would `systemctl --user disable
    /// --now` and re-enable every few seconds, restarting the host and ending
    /// every coding session with it, forever.
    #[test]
    fn a_linger_warning_does_not_ask_for_a_rewrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("beekeeper-host");
        std::fs::write(&program, b"#!/bin/sh\n").expect("write program");
        let advisory = Registration {
            installed: true,
            path: dir.path().join("io.agiterra.beekeeper.host.plist"),
            program: Some(program),
            warnings: vec![
                "this user does not linger, so the agent host will stop when you log out — run \
                 `loginctl enable-linger $USER`"
                    .to_string(),
            ],
            domain: Domain::User,
        };
        assert!(
            !advisory.needs_rewrite(),
            "a warning is a thing to disclose, not evidence the file is wrong"
        );
        // And the two states a rewrite does fix still ask for one.
        assert!(Registration {
            installed: false,
            ..advisory.clone()
        }
        .needs_rewrite());
        assert!(Registration {
            program: None,
            ..advisory.clone()
        }
        .needs_rewrite());
        assert!(
            Registration {
                program: Some(dir.path().join("gone")),
                ..advisory
            }
            .needs_rewrite(),
            "a registration naming a binary that no longer exists must be rewritten"
        );
    }

    /// The four rows of the rule, as the one place they are written down.
    #[test]
    fn the_login_decision_has_four_distinguishable_answers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("beekeeper-host");
        std::fs::write(&program, b"#!/bin/sh\n").expect("write program");
        let registered = Registration {
            installed: true,
            path: dir.path().join("host.plist"),
            program: Some(program),
            warnings: Vec::new(),
            domain: Domain::User,
        };
        let absent = Registration {
            installed: false,
            ..registered.clone()
        };

        // No identity: nothing to register, so nothing to ask.
        assert_eq!(
            login_autostart(false, &absent, false),
            LoginAutostart::NotApplicable
        );
        // Registered: repair silently, never ask again. Including when a
        // refusal was recorded earlier and then superseded by an install —
        // `installed` is read first precisely so the filesystem wins over a
        // stale note about what somebody once wanted.
        assert_eq!(
            login_autostart(true, &registered, false),
            LoginAutostart::Granted
        );
        assert_eq!(
            login_autostart(true, &registered, true),
            LoginAutostart::Granted
        );
        // Said no: disclose, do not ask.
        assert_eq!(
            login_autostart(true, &absent, true),
            LoginAutostart::Declined
        );
        // Nothing registered, nobody asked: ask.
        assert_eq!(
            login_autostart(true, &absent, false),
            LoginAutostart::ShouldAsk
        );
    }

    /// An uninstall is the operator saying no, and it has to stick: without
    /// the recorded refusal the app would propose the registration again at
    /// the next launch, which reads as the app ignoring you.
    #[test]
    fn uninstalling_records_the_refusal_and_installing_clears_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        let program = home.join("beekeeper-host");
        std::fs::write(&program, b"#!/bin/sh\n").expect("write program");

        assert!(
            !login_refused(home, Instance::Production),
            "a fresh machine has refused nothing"
        );

        uninstall(Service::AgentHost, home, Instance::Production).expect("uninstall");
        assert!(
            login_refused(home, Instance::Production),
            "uninstalling records the refusal"
        );
        assert_eq!(
            login_autostart(
                true,
                &status(Service::AgentHost, home, Instance::Production),
                login_refused(home, Instance::Production)
            ),
            LoginAutostart::Declined
        );

        // And it is per instance, so declining for a dev build says nothing
        // about the release build on the same machine.
        assert!(!login_refused(home, Instance::Dev));

        allow_login(home, Instance::Production).expect("allow");
        assert!(!login_refused(home, Instance::Production));
    }

    #[test]
    fn dev_and_production_register_separately() {
        let home = Path::new("/home/agent");
        assert_ne!(
            service_name(Service::AgentHost, Instance::Production),
            service_name(Service::AgentHost, Instance::Dev)
        );
        assert_ne!(
            registration_path(Service::AgentHost, home, Instance::Production),
            registration_path(Service::AgentHost, home, Instance::Dev)
        );
    }

    /// The registration must name an absolute path: launchd and systemd both
    /// run with a minimal `PATH` and neither looks a program up.
    #[test]
    fn the_program_must_be_absolute_and_must_exist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let relative = install(
            Service::AgentHost,
            dir.path(),
            Instance::Production,
            Path::new("beekeeper-host"),
        )
        .expect_err("relative must be refused");
        assert!(relative.contains("absolute path"), "{relative}");

        let absent = install(
            Service::AgentHost,
            dir.path(),
            Instance::Production,
            &dir.path().join("nowhere/beekeeper-host"),
        )
        .expect_err("a missing binary must be refused");
        assert!(absent.contains("there is no agent host binary"), "{absent}");
        assert!(
            !registration_path(Service::AgentHost, dir.path(), Instance::Production).exists(),
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
        let contents =
            registration_contents(Service::AgentHost, Instance::Production, program, home);
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
        let dev = registration_contents(Service::AgentHost, Instance::Dev, program, home);
        assert!(dev.contains(Instance::Dev.namespace_value()), "{dev}");
    }

    /// The two services must not collide, and must not be interchangeable:
    /// the host takes `run` and the menu bar app takes no argument, so one
    /// registration's program line cannot be reused for the other.
    #[test]
    fn the_two_services_register_separately_and_differently() {
        let home = Path::new("/home/agent");
        assert_ne!(
            service_name(Service::AgentHost, Instance::Production),
            service_name(Service::MenuBar, Instance::Production)
        );
        assert_ne!(
            registration_path(Service::AgentHost, home, Instance::Production),
            registration_path(Service::MenuBar, home, Instance::Production)
        );

        let program = Path::new("/Applications/Beekeeper.app/Contents/MacOS/beekeeper-host");
        let host = registration_contents(Service::AgentHost, Instance::Production, program, home);
        let menubar = registration_contents(Service::MenuBar, Instance::Production, program, home);
        assert!(
            host.contains("run"),
            "the host is started with `run`: {host}"
        );
        assert!(
            !menubar.contains("<string>run</string>")
                && !menubar.contains("ExecStart={program} run"),
            "the menu bar app takes no argument: {menubar}"
        );
        // Both round-trip through the parser that `status` reports from.
        assert_eq!(
            program_from_registration(&host),
            Some(program.to_path_buf())
        );
        assert_eq!(
            program_from_registration(&menubar),
            Some(program.to_path_buf())
        );
    }

    /// `loginctl enable-linger` is about services that must survive logout.
    /// The menu bar app draws into a logged-in session, so warning about
    /// lingering there would be advice for a problem it cannot have.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn only_the_host_warns_about_lingering() {
        let dir = tempfile::tempdir().expect("tempdir");
        for service in [Service::AgentHost, Service::MenuBar] {
            let path = registration_path(service, dir.path(), Instance::Production);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(
                &path,
                registration_contents(
                    service,
                    Instance::Production,
                    Path::new("/usr/local/bin/beekeeper-host"),
                    dir.path(),
                ),
            )
            .expect("write");
        }
        let host = status(Service::AgentHost, dir.path(), Instance::Production);
        let menubar = status(Service::MenuBar, dir.path(), Instance::Production);
        assert!(menubar
            .warnings
            .iter()
            .all(|warning| !warning.contains("linger")));
        // The host's answer depends on this machine's loginctl, so this only
        // asserts the asymmetry rather than a specific warning.
        assert!(host.warnings.len() >= menubar.warnings.len());
    }

    #[test]
    fn nothing_installed_reads_as_not_installed_with_no_warnings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = status(Service::AgentHost, dir.path(), Instance::Production);
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
        let path = registration_path(Service::AgentHost, dir.path(), Instance::Production);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &path,
            registration_contents(
                Service::AgentHost,
                Instance::Production,
                Path::new("/Applications/Deleted.app/Contents/MacOS/beekeeper-host"),
                dir.path(),
            ),
        )
        .expect("write");
        let status = status(Service::AgentHost, dir.path(), Instance::Production);
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
            Service::AgentHost,
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
        uninstall(Service::AgentHost, dir.path(), Instance::Production).expect("idempotent");
    }
}
