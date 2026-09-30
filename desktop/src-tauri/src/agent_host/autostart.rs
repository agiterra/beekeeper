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

use buzz_host::install::{self, Registration};
use buzz_session_host_core::layout;
use tauri::AppHandle;

/// Binary name of the agent host, as it is shipped beside the app.
pub(crate) const HOST_BINARY: &str = "buzz-host";

/// The host binary this build should register.
///
/// Resolved through the same discovery order every other Buzz-spawned binary
/// uses, so a bundled app registers the `buzz-host` inside its own bundle and
/// a dev build registers the one in `target/`. Never a bare name: launchd and
/// systemd run with a minimal `PATH` and neither will look one up.
pub(crate) fn resolve_host_binary() -> Option<PathBuf> {
    crate::managed_agents::resolve_command(HOST_BINARY)
}

/// Whether this app's instance is registered to start at login.
pub(crate) fn status() -> Registration {
    match layout::home_dir() {
        Ok(home) => install::status(&home, super::instance()),
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
    let current = install::status(&home, instance);
    // Already registered, pointing at something that exists: leave it. A
    // rewrite would be harmless but it would also bounce the agent through
    // `launchctl bootout`, and doing that on every status poll is how a host
    // comes to restart every few seconds.
    if current.installed && current.warnings.is_empty() {
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
    match install::install(&home, instance, &program) {
        Ok(registration) => registration,
        Err(error) => {
            let mut registration = install::status(&home, instance);
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
    install::uninstall(&layout::home_dir()?, super::instance())
}

#[cfg(test)]
mod tests {
    use super::*;

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
