//! Beekeeper's menu bar app.
//!
//! An accessory process whose job is to show what this machine's agents are
//! doing — separately from the desktop app, and separately from the agents
//! themselves. It is a *client* of `beekeeper-host`'s control socket: it
//! supervises nothing, and quitting it stops nothing. Its one window, Manage
//! Host, opens on request and changes things only through this app's own `bee`.
//!
//! That last point is the whole reason it exists as its own process. The
//! desktop tray's "Quit" called `app.exit(0)`, which ended every agent on the
//! machine — the conflation this split removes. This app's Quit ends this app.
//!
//! # Platform
//!
//! macOS only in substance: the tray is an `NSStatusItem` and the host's
//! socket is `AF_UNIX`. The crate stays a workspace member everywhere and
//! gates *inside*, so the Windows and Linux desktop gates compile it as a
//! no-op rather than skipping it — a skipped crate's gate passes
//! green-and-empty over a real defect.

pub mod model;
#[cfg(unix)]
pub mod poll;

#[cfg(all(unix, target_os = "macos"))]
mod imp;
#[cfg(all(unix, target_os = "macos"))]
mod manage;
pub mod manage_actions;

/// Start the menu bar app.
#[cfg(all(unix, target_os = "macos"))]
pub fn run() {
    imp::run();
}

/// Everywhere else this is a no-op that says so, rather than a crate that does
/// not compile: a build producing no binary at all would make a missing
/// artifact look like a build-list mistake.
#[cfg(not(all(unix, target_os = "macos")))]
pub fn run() {
    eprintln!(
        "beekeeper-menubar is macOS-only: its menu is an NSStatusItem and the agent host's \
         control socket is an AF_UNIX socket."
    );
}
