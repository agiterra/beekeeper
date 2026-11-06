//! The headless host for Beekeeper's background agents.
//!
//! `beekeeper-session-provider` answers coding-session commands on the relay and
//! publishes transcripts back. Until now its body was borrowed: it ran while a
//! desktop app ran, on hardware that sleeps when a human does
//! (`VISION_REMOTE_AGENTS.md`). This crate is the body it gets instead — a
//! process that starts at login — or at boot, as a LaunchDaemon on a Mac
//! nobody logs in to — survives every desktop launch, quit and update, and
//! runs alone on a server with no GUI anywhere.
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
//! Unix uses authenticated AF_UNIX sockets; Windows uses user-restricted local
//! named pipes and kill-on-close Job Objects. Windows supports foreground run
//! and provider lifecycle controls; startup registration remains unsupported.

#[cfg(any(unix, windows))]
pub mod activity;
#[cfg(any(unix, windows))]
pub mod census;
#[cfg(any(unix, windows))]
pub mod cli;
#[cfg(any(unix, windows))]
pub mod client;
#[cfg(any(unix, windows))]
pub mod commission;
#[cfg(any(unix, windows))]
pub mod control;
#[cfg(any(unix, windows))]
pub mod discovery;
#[cfg(any(unix, windows))]
pub mod identity;
#[cfg(any(unix, windows))]
pub mod install;
pub mod inventory;
#[cfg(any(unix, windows))]
pub mod owner;
#[cfg(any(unix, windows))]
pub mod protocol;
#[cfg(any(unix, windows))]
pub mod restart_policy;
#[cfg(any(unix, windows))]
pub mod server;
#[cfg(any(unix, windows))]
pub mod sessions;
#[cfg(any(unix, windows))]
pub mod spawn;
#[cfg(any(unix, windows))]
pub mod state;
#[cfg(any(unix, windows))]
pub mod supervisor;
#[cfg(any(unix, windows))]
pub mod takeover;
#[cfg(any(unix, windows))]
pub mod terminate;

#[cfg(unix)]
#[path = "transport_unix.rs"]
mod transport;
#[cfg(windows)]
#[path = "transport_windows.rs"]
mod transport;
