//! Detached PTY host for Beekeeper built-in shell sessions.
//!
//! This crate ships two things: the `beekeeper-shell-host` binary (a dtach-style
//! process that owns a shell in a PTY and outlives the desktop app), and the
//! wire protocol + receipt types the desktop app reuses to talk to and discover
//! it. Keeping the protocol here makes it the single source of truth for both
//! sides of the socket.

// The host process is Unix-only (PTY + Unix-socket + setsid detachment). The
// protocol and receipt types are cross-platform so the desktop app can depend
// on this crate everywhere, even where the shell feature itself is gated off.
#[cfg(unix)]
pub mod host;
pub mod proto;
pub mod receipt;
