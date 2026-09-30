//! The headless host for Beekeeper's background agents.
//!
//! `buzz-session-provider` answers coding-session commands on the relay and
//! publishes transcripts back. Until now its body was borrowed: it ran while a
//! desktop app ran, on hardware that sleeps when a human does
//! (`VISION_REMOTE_AGENTS.md`). This crate is the body it gets instead — a
//! process that starts at login, survives every desktop launch, quit and
//! update, and runs alone on a server with no GUI anywhere.
//!
//! What it does, and deliberately does not do:
//!
//! - It **supervises** the provider as a child process rather than linking it
//!   in. The provider already locks its own state directory for the life of the
//!   process, handles its own relay reconnection and owns a durable outbox.
//!   Keeping it a child preserves crash isolation — a panicking provider must
//!   not take the host's control socket down with it — and lets the host
//!   restart it on a new config without restarting itself.
//! - It is **never a keychain client**. See [`identity`].
//! - It holds **no relay connection of its own**, which is the cost of
//!   supervising rather than linking: it cannot see inside the provider, so it
//!   reports the relay state as unknown rather than synthesising "connected"
//!   from "the child is alive".
//!
//! Everything it shares with the desktop app lives in `beekeeper-host-core`.

//! # Platform
//!
//! Unix only, and the crate says so rather than failing to compile: the
//! control socket is an `AF_UNIX` socket and the login registrations are
//! launchd and systemd. A Windows host needs named pipes and a different
//! service manager, which is separate work. `buzz-shell-host` already has
//! this shape, and `beekeeper-host` follows it — including being absent from
//! `tauri.windows.conf.json`.

#[cfg(unix)]
pub mod activity;
#[cfg(unix)]
pub mod cli;
#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod commission;
#[cfg(unix)]
pub mod control;
#[cfg(unix)]
pub mod discovery;
#[cfg(unix)]
pub mod identity;
#[cfg(unix)]
pub mod install;
#[cfg(unix)]
pub mod owner;
#[cfg(unix)]
pub mod protocol;
#[cfg(unix)]
pub mod restart_policy;
#[cfg(unix)]
pub mod server;
#[cfg(unix)]
pub mod sessions;
#[cfg(unix)]
pub mod spawn;
#[cfg(unix)]
pub mod state;
#[cfg(unix)]
pub mod supervisor;
#[cfg(unix)]
pub mod takeover;
#[cfg(unix)]
pub mod terminate;
