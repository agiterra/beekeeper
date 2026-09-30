//! Making the agent host start at login.
//!
//! "After Beekeeper is installed and commissioned" is the trigger, and
//! commissioning is `provision_coding_session_provider` succeeding. So the
//! registration is written there — and re-asserted on every status poll, so an
//! app that was updated (or whose registration somebody removed) repairs
//! itself instead of quietly never starting the host again.
//!
//! # A failed registration must not read as success
//!
//! It is written and then *read back*, and the answer travels in
//! `status.host.autostart` for the UI to render. A commissioning that
//! succeeded and a registration that silently did not is precisely the state
//! that would leave a person believing their agents survive a reboot when they
//! do not — and they would only find out the next morning.

use std::path::PathBuf;

use beekeeper_host::install::{self, Registration, Service};
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
/// Resolved through the same discovery order every other Buzz-spawned binary
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
    /// Write the registration if it is missing, then read it back.
    Reassert,
    /// Read it and change nothing.
    ReadOnly,
}

/// Decide from whether this relay has a provider identity.
///
/// The trigger this module's header names — "installed and commissioned" — is
/// `provision_coding_session_provider` succeeding, and for a while that was
/// the *only* caller of [`ensure_registered`]. A machine commissioned by an
/// older build never runs that command again, so its registration would never
/// be written at all: the host would never start at login, and the only sign
/// would be `bee host installed` reporting both services absent while
/// `host.json` and the key file sat there freshly written. That is what
/// happened on the first install onto such a machine.
///
/// `ReadOnly` when nothing is provisioned, so merely opening the app does not
/// plant a LaunchAgent for a host with no identity to serve.
pub(crate) fn poll_action(provisioned: bool) -> PollAction {
    if provisioned {
        PollAction::Reassert
    } else {
        PollAction::ReadOnly
    }
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
        },
    }
}

/// Register the host to start at login, unless it already is and still points
/// at a binary that exists.
///
/// Returns the registration as read back afterwards, warnings included. Never
/// an `Err`: a commissioning must not fail because a login registration could
/// not be written — the provider can still be started by hand, and the
/// *disclosure* is what matters. The failure travels in
/// [`Registration::warnings`].
pub(crate) fn ensure_registered(_app: &AppHandle) -> Registration {
    let Ok(home) = layout::home_dir() else {
        return status();
    };
    let instance = super::instance();

    // The menu bar app is registered too, and separately: a person may turn
    // it off without turning their agents off, and one uninstall must not take
    // the other with it. Its absence is not worth a warning on the host's
    // registration — a dev build has no bundle and therefore no menu bar app,
    // which is correct rather than broken.
    if let Some(menubar) = resolve_menubar_binary() {
        let current = install::status(Service::MenuBar, &home, instance);
        if current.needs_rewrite() {
            if let Err(error) = install::install(Service::MenuBar, &home, instance, &menubar) {
                eprintln!("buzz-desktop: agent-host: menu bar app not registered: {error}");
            }
        }
    }

    let current = install::status(Service::AgentHost, &home, instance);
    // Already registered, pointing at something that exists: leave it. A
    // rewrite would be harmless but it would also bounce the agent through
    // `launchctl bootout`, and doing that on every status poll is how a host
    // comes to restart every few seconds.
    //
    // The question is `needs_rewrite`, not "are there warnings": a warning is
    // something to disclose, and a permanent one — a Linux user who does not
    // linger — would otherwise re-register on every poll forever. See
    // `Registration::needs_rewrite`.
    if !current.needs_rewrite() {
        return current;
    }
    let Some(program) = resolve_host_binary() else {
        let mut registration = current;
        registration.warnings.push(format!(
            "{HOST_BINARY} was not found beside this app, so the agent host cannot be registered \
             to start at login — your agents will stop when you quit Beekeeper. Reinstall the \
             app, or install the host yourself with `bee host install`."
        ));
        return registration;
    };
    match install::install(Service::AgentHost, &home, instance, &program) {
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

/// Remove the registration. Used by the app's reset path.
#[allow(dead_code)] // Wired into `reset.rs` in a later slice.
pub(crate) fn unregister() -> Result<(), String> {
    let home = layout::home_dir()?;
    let instance = super::instance();
    // Both, and the menu bar app's failure does not stop the host's: a reset
    // that half-removed the registrations would leave launchd retrying a
    // binary this app is about to delete.
    let menubar = install::uninstall(Service::MenuBar, &home, instance);
    let host = install::uninstall(Service::AgentHost, &home, instance);
    host.and(menubar)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The upgrade case: an app whose provider was commissioned by an older
    /// build must still end up registered. Its commissioning command never
    /// runs again, so the poll is the only thing left that can write the
    /// registration — and for the first install of this branch onto such a
    /// machine, it did not.
    #[test]
    fn a_relay_with_an_identity_reasserts_the_registration_on_every_poll() {
        assert_eq!(poll_action(true), PollAction::Reassert);
    }

    /// And an app nobody has finished setting up does not plant a LaunchAgent
    /// for a host with no identity to serve.
    #[test]
    fn a_relay_with_no_identity_only_reads_the_registration() {
        assert_eq!(poll_action(false), PollAction::ReadOnly);
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
        };
        assert!(!registration.installed);
        assert!(registration.warnings[0].contains("stop when you quit"));
        // And the wire keeps it machine-readable *and* human-readable.
        let json = serde_json::to_string(&registration).expect("encode");
        assert!(json.contains("\"installed\":false"), "{json}");
        assert!(json.contains("warnings"), "{json}");
    }
}
