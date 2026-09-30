//! Where Beekeeper keeps the Node runtime and npm shims it manages itself.
//!
//! The definitions live in `beekeeper_host_core::managed_node` because
//! `beekeeper-host` must look in the same two directories. A host that did not
//! would fail to find `claude-agent-acp` on a machine where the desktop finds
//! it, and the symptom — a coding session that works while the app is open and
//! not otherwise — reads as a relay problem rather than a `PATH` one.

pub(crate) use beekeeper_host_core::managed_node::{
    buzz_managed_command_path, buzz_managed_node_bin_dir, buzz_managed_node_bin_path,
    buzz_managed_node_root, buzz_managed_npm_bin_dir, buzz_managed_npm_prefix,
};
