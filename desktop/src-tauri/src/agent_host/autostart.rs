//! Making the agent host start at login.
//!
//! A daemon that starts at every login is a machine-level change, so a person
//! is *asked* — the poll reports the question (see [`poll_action`]) and a
//! surface puts it to them. What the poll does on its own is narrower: it
//! repairs a registration that already exists, so an app that was updated, or
//! whose registration somebody moved, does not quietly stop starting the host
//! for a person who already said yes.
//!
//! On a headless machine there is nobody to ask, and `bee host install` is the
//! whole story.
//!
//! # A failed registration must not read as success
//!
//! It is written and then *read back*, and the answer travels in
//! `status.host.autostart` for the UI to render. A commissioning that
//! succeeded and a registration that silently did not is precisely the state
//! that would leave a person believing their agents survive a reboot when they
//! do not — and they would only find out the next morning.

use std::path::PathBuf;

use beekeeper_host::install::{self, Domain, LoginAutostart, Registration, Service};
use beekeeper_host_core::layout;
use tauri::AppHandle;

/// Binary name of the agent host, as it is shipped beside the app.
pub(crate) const HOST_BINARY: &str = "beekeeper-host";

/// The menu bar app, nested inside this bundle's `LoginItems`.
///
/// Not a sidecar beside the executable: it is a whole `.app`, and macOS
/// requires a login item to be a bundle. `scripts/stage-menubar.sh` builds it
/// and `bundle.macOS.files` copies it in *during* bundling, so it is inside
/// the `.app` before anything signs it and the outer signature covers it.
#[cfg(target_os = "macos")]
const MENUBAR_RELATIVE_PATH: &str = "../Library/LoginItems/Beekeeper Menu Bar.app";

/// The host binary this build should register.
///
/// Resolved through the same discovery order every other Beekeeper-spawned binary
/// uses, so a bundled app registers the `beekeeper-host` inside its own bundle and
/// a dev build registers the one in `target/`. Never a bare name: launchd and
/// systemd run with a minimal `PATH` and neither will look one up.
pub(crate) fn resolve_host_binary() -> Option<PathBuf> {
    crate::managed_agents::resolve_command(HOST_BINARY)
}

/// The nested menu bar app's executable, when this build has one.
///
/// Resolved relative to *this* executable rather than searched for: there is
/// exactly one right answer inside a bundle, and a `PATH` hit would be some
/// other install's copy. `None` in a dev build, which has no bundle — so
/// `just dev` never registers a menu bar app at login, which is what you want
/// from a dev build.
#[cfg(target_os = "macos")]
pub(crate) fn resolve_menubar_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.join(MENUBAR_RELATIVE_PATH);
    // Named after the *binary*, not the product name: Tauri puts
    // `beekeeper-menubar` in `Contents/MacOS`, and "Beekeeper Menu Bar" is
    // only the bundle's display name. Guessing the latter is how the
    // registration comes to name a path that does not exist — which
    // `install` would then refuse, correctly but confusingly.
    let binary = bundle
        .join("Contents/MacOS/beekeeper-menubar")
        .canonicalize()
        .ok()?;
    binary.is_file().then_some(binary)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn resolve_menubar_binary() -> Option<PathBuf> {
    None
}

/// What a status poll should do about the login registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollAction {
    /// Registered already: rewrite it only if it has drifted, and never ask.
    Repair,
    /// Nothing registered and nobody has said no. Report the question; the
    /// person answers it, not the poll.
    Ask,
    /// Nothing to do. No identity here, or the operator declined.
    Leave,
}

/// Decide from the three facts that determine it.
///
/// **The poll does not install.** It did, briefly, and that was wrong in a way
/// worth recording: a launchd agent that runs a daemon at every login, forever,
/// is a machine-level change, and helping yourself to it because somebody
/// opened an app is not a thing to do quietly. It also meant nothing ever told
/// a person the agent host existed.
///
/// It does still *repair*, because a person who has already said yes must not
/// be asked again every time an update moves the binary the registration
/// names. `Granted` is read off the filesystem rather than from a stored
/// grant — see `beekeeper_host::install::login_autostart`.
///
/// The trigger this module's header used to name — commissioning — is not
/// enough on its own: a machine commissioned by an *older* build never runs
/// that command again, so it would never be asked at all. That is what
/// happened on the first install of this branch onto such a machine, where
/// `bee host installed` reported both services absent while `host.json` and
/// the key file sat there freshly written.
pub(crate) fn poll_action(decision: LoginAutostart) -> PollAction {
    match decision {
        LoginAutostart::Granted => PollAction::Repair,
        LoginAutostart::ShouldAsk => PollAction::Ask,
        LoginAutostart::NotApplicable | LoginAutostart::Declined => PollAction::Leave,
    }
}

/// The login decision for this instance, and the registration it was read
/// from.
///
/// One function so the two can never be read a moment apart and disagree.
pub(crate) fn decide(provisioned: bool) -> (LoginAutostart, Registration) {
    let registration = status();
    let refused = match layout::home_dir() {
        Ok(home) => install::login_refused(&home, super::instance()),
        // Without a home directory there is nowhere to have recorded a
        // refusal, and `status()` above has already put the real problem in
        // the registration's warnings.
        Err(_) => false,
    };
    let decision = install::login_autostart(provisioned, &registration, refused);
    (decision, registration)
}

/// Repair a login registration that exists but is not loaded, once per launch.
///
/// **Not on the status poll, and not in a React component.** The poll repairs
/// too, but only when something asks it for the status — and for a while
/// nothing did at launch, so an app whose services had been booted out (which
/// is what replacing the bundle under a loaded job does, and therefore what
/// every `just prod-desktop` install does) came up with no menu bar icon, no
/// host, and every surface reporting the machine as registered. A change to
/// this machine's services must not depend on which view happens to be
/// mounted.
///
/// Only repairs; never registers something that was not registered, so this is
/// not a way around the consent question in [`poll_action`]. A registration
/// that exists was asked for.
///
/// Spawned on a thread: it runs `launchctl` twice and `setup` must not block.
pub(crate) fn repair_at_launch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(home) = layout::home_dir() else { return };
        let instance = super::instance();
        // Nothing registered means nothing to repair — and asking would be the
        // app installing a service nobody agreed to.
        let host = install::status(Service::AgentHost, &home, instance);
        let menubar = install::status(Service::MenuBar, &home, instance);
        if !host.installed && !menubar.installed {
            return;
        }
        for warning in &ensure_registered(&app, Probe::both()).warnings {
            eprintln!("beekeeper-desktop: agent-host: {warning}");
        }
    });
}

/// Record that the operator does not want the host registered at login.
///
/// The disclosure of what it costs them is the surface's job; this only makes
/// the answer stick, so nothing proposes it again.
pub(crate) fn decline() -> Result<Registration, String> {
    let home = layout::home_dir()?;
    let instance = super::instance();
    install::refuse_login(&home, instance)?;
    // Both services, for the same reason `unregister` removes both: a menu bar
    // icon for a host that will not start is worse than neither.
    let menubar = install::uninstall(Service::MenuBar, &home, instance);
    let host = install::uninstall(Service::AgentHost, &home, instance);
    host.and(menubar)?;
    Ok(status())
}

/// Whether this app's instance is registered to start at login.
pub(crate) fn status() -> Registration {
    match layout::home_dir() {
        Ok(home) => install::status(Service::AgentHost, &home, super::instance()),
        Err(error) => Registration {
            installed: false,
            path: PathBuf::new(),
            program: None,
            warnings: vec![format!("cannot resolve the home directory: {error}")],
            domain: Domain::User,
        },
    }
}

/// Register the host to start at login, unless it already is and still points
/// at a binary that exists.
///
/// Called when a person asks for it, and by the poll only to *repair* a
/// registration that already exists. `install` clears any recorded refusal,
/// because asking for the registration supersedes having declined it once.
///
/// Returns the registration as read back afterwards, warnings included. Never
/// an `Err`: a commissioning must not fail because a login registration could
/// not be written — the provider can still be started by hand, and the
/// *disclosure* is what matters. The failure travels in
/// [`Registration::warnings`].
pub(crate) fn ensure_registered(_app: &AppHandle, probe: Probe) -> Registration {
    let Ok(home) = layout::home_dir() else {
        return status();
    };
    let instance = super::instance();

    // The menu bar app is registered too, and separately: a person may turn it
    // off without turning their agents off, and one uninstall must not take the
    // other with it. Its absence is not worth a warning on the host's
    // registration — a dev build has no bundle and therefore no menu bar app,
    // which is correct rather than broken.
    if let Some(menubar) = resolve_menubar_binary() {
        if let Err(error) =
            ensure_service(Service::MenuBar, &home, instance, &menubar, probe.menubar)
        {
            eprintln!("beekeeper-desktop: agent-host: menu bar app not registered: {error}");
        }
    }

    let Some(program) = resolve_host_binary() else {
        let mut registration = install::status(Service::AgentHost, &home, instance);
        registration.warnings.push(format!(
            "{HOST_BINARY} was not found beside this app, so the agent host cannot be registered \
             to start at login — your agents will stop when you quit Beekeeper. Reinstall the \
             app, or install the host yourself with `bee host install`."
        ));
        return registration;
    };
    match ensure_service(Service::AgentHost, &home, instance, &program, probe.host) {
        Ok(registration) => registration,
        Err(error) => {
            let mut registration = install::status(Service::AgentHost, &home, instance);
            registration.warnings.push(format!(
                "the agent host could not be registered to start at login ({error}) — your \
                 agents will stop when you quit Beekeeper"
            ));
            registration
        }
    }
}

/// How hard to look before deciding a service does not need touching.
///
/// Asking the service manager costs a subprocess, so it is asked for
/// deliberately: once per launch for both services, and on the status poll only
/// for the host, only while its socket is silent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Probe {
    /// Consult `launchctl`/`systemctl` about the agent host.
    pub host: bool,
    /// Consult it about the menu bar app.
    pub menubar: bool,
}

impl Probe {
    /// Ask about both. Used once at launch.
    pub(crate) fn both() -> Self {
        Self {
            host: true,
            menubar: true,
        }
    }

    /// The status poll: ask about the host only while it is unreachable, and
    /// never about the menu bar, which the launch probe has already covered.
    pub(crate) fn polling(host_reachable: bool) -> Self {
        Self {
            host: !host_reachable,
            menubar: false,
        }
    }
}

/// Register one service if it needs it, and answer with what is true afterwards.
///
/// **The condition lives here and only here.** It used to be in two places:
/// the caller decided a repair was needed because the service manager did not
/// have the service loaded, and then this function re-checked with the weaker
/// filesystem-only question, found the file present, and returned early without
/// installing anything. The outer decision was inert, and the symptom was a
/// menu bar icon that never came back and a host that never started, with
/// every surface reporting the machine as registered. Exactly the two-copies-of
/// -one-rule shape as the tray's subtitle indices (ledger 298(c)).
fn ensure_service(
    service: Service,
    home: &std::path::Path,
    instance: layout::Instance,
    program: &std::path::Path,
    probe: bool,
) -> Result<Registration, String> {
    let current = install::status(service, home, instance);
    // `loaded` is only asked for when the cheap question found nothing and the
    // caller said to look: an unknown answer is never treated as a negative,
    // so a machine where the probe cannot run behaves as it did before.
    let loaded = if probe && !current.needs_rewrite() {
        install::service_loaded(service, instance)
    } else {
        None
    };
    // `host_reachable: false` because reachability is the *host's* signal and
    // this function serves both services; `probe` already encodes whether the
    // caller thinks something might be wrong.
    if !current.needs_repair(false, loaded) {
        return Ok(current);
    }
    install::install(service, home, instance, program)
}

/// Remove the registration. Used by the app's reset path.
///
/// **Not the same as [`decline`], and the difference is the refusal.**
/// `install::uninstall` records one, because a person running `bee host
/// uninstall` is saying no and must not be asked again. A reset is not saying
/// no — it is erasing what this machine remembers — so the refusal is cleared
/// afterwards. Leaving it would mean a reset machine is silently never offered
/// background agents again, with nothing anywhere to explain why.
#[allow(dead_code)] // Wired into `reset.rs` in a later slice.
pub(crate) fn unregister() -> Result<(), String> {
    let home = layout::home_dir()?;
    let instance = super::instance();
    // Both, and the menu bar app's failure does not stop the host's: a reset
    // that half-removed the registrations would leave launchd retrying a
    // binary this app is about to delete.
    let menubar = install::uninstall(Service::MenuBar, &home, instance);
    let host = install::uninstall(Service::AgentHost, &home, instance);
    let cleared = install::allow_login(&home, instance);
    host.and(menubar).and(cleared)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What each caller asks the service manager about.
    ///
    /// The launch repair asks about both, because replacing the app bundle
    /// under loaded jobs boots both out and nothing else will notice. The poll
    /// asks about the host only while its socket is silent — a reachable host
    /// is loaded by definition — and never about the menu bar, whose only
    /// realistic way to come unloaded is the one the launch probe covers.
    #[test]
    fn the_launch_probe_looks_at_both_services_and_the_poll_does_not() {
        let launch = Probe::both();
        assert!(launch.host && launch.menubar);

        let silent = Probe::polling(false);
        assert!(silent.host, "a silent host is worth a launchctl call");
        assert!(!silent.menubar, "the launch probe already covered it");

        let answering = Probe::polling(true);
        assert!(
            !answering.host,
            "a host that answers its socket is loaded, so asking is waste"
        );
        assert!(!answering.menubar);
    }

    /// The poll repairs, and does not install. The distinction is the whole
    /// consent model: a registration that exists was asked for once and must
    /// keep working across updates without asking again, and one that does
    /// not exist is a question for a person.
    #[test]
    fn the_poll_repairs_a_grant_and_asks_for_everything_else() {
        assert_eq!(poll_action(LoginAutostart::Granted), PollAction::Repair);
        assert_eq!(poll_action(LoginAutostart::ShouldAsk), PollAction::Ask);
        // Declined is not a question and not a repair. Re-proposing it every
        // poll would be the app arguing with the person using it.
        assert_eq!(poll_action(LoginAutostart::Declined), PollAction::Leave);
        assert_eq!(
            poll_action(LoginAutostart::NotApplicable),
            PollAction::Leave
        );
    }

    /// Nothing registered and no binary to register must produce a *warning*,
    /// not a silent success. This is the state that would otherwise have a
    /// person believing their agents survive a reboot.
    #[test]
    fn a_registration_that_cannot_be_written_warns_rather_than_claiming_success() {
        // `resolve_host_binary` consults this machine, so this asserts the
        // shape of the disclosure rather than the resolution: whatever the
        // answer, an un-installed registration must carry a warning that says
        // what a person loses.
        let registration = Registration {
            installed: false,
            path: PathBuf::from("/home/agent/Library/LaunchAgents/x.plist"),
            program: None,
            warnings: vec![format!(
                "{HOST_BINARY} was not found beside this app, so the agent host cannot be \
                 registered to start at login — your agents will stop when you quit Beekeeper."
            )],
            domain: Domain::User,
        };
        assert!(!registration.installed);
        assert!(registration.warnings[0].contains("stop when you quit"));
        // And the wire keeps it machine-readable *and* human-readable.
        let json = serde_json::to_string(&registration).expect("encode");
        assert!(json.contains("\"installed\":false"), "{json}");
        assert!(json.contains("warnings"), "{json}");
    }
}
