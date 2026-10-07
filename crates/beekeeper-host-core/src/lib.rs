//! The contract between a coding-session provider's launcher and the app.
//!
//! `beekeeper-session-provider` takes its entire configuration from the
//! environment, holds a lock on its state directory for the life of the
//! process, and talks to the relay itself. That makes it launchable by
//! anything — and since 2026-09 there is more than one launcher: the desktop
//! app, and the headless `beekeeper-host` that owns the provider on a machine where
//! nobody is logged in.
//!
//! Everything both launchers must agree on lives here:
//!
//! - [`record`] — the provisioned identity, minus any opinion about secret stores
//! - [`env`] — the child's environment, assembled as data
//! - [`command_paths`] — where a launcher looks for the binaries it runs
//! - [`path_env`] — the pure kernel of `PATH` composition
//! - [`layout`] — where the host's own socket, config, key and log live
//! - [`config`] — `host.json`, what the host serves with no desktop present
//! - [`logs`] — the supervised child's log, so a bug report reads the same
//! - [`managed_node`] — the app-private Node and npm shim directories
//! - [`atomic_write`] — owner-only writes with no readable window
//!
//! This crate has no Tauri, no keyring and no async runtime, on purpose. The
//! secret resolution is deliberately *not* here: the desktop has a keychain
//! and a server does not, so each end resolves the key its own way and they
//! agree only on the record that names it.

pub mod atomic_write;
pub mod command_paths;
pub mod config;
pub mod env;
pub mod layout;
pub mod logs;
pub mod managed_node;
pub mod path_env;
pub mod record;

#[cfg(test)]
#[path = "contract_tests.rs"]
mod contract_tests;
