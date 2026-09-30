//! Where Beekeeper keeps the Node runtime and npm shims it manages itself.
//!
//! The ACP adapters are npm packages installed into an app-private prefix, and
//! the Node that runs them is an app-private download. Both launchers must look
//! in the same two directories: a host that did not would fail to find
//! `claude-agent-acp` on a machine where the desktop finds it, and the symptom
//! would be a coding session that works when the app is open and not otherwise
//! — which reads as a relay problem, not a `PATH` one.

use std::path::PathBuf;

use crate::command_paths::is_executable_file;

pub fn buzz_managed_npm_prefix() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("Beekeeper").join("node-tools"))
}

const BUZZ_MANAGED_NODE_VERSION: &str = "v24.18.0";

pub fn buzz_managed_node_root() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("Beekeeper").join("runtimes").join("node"))
}

pub fn buzz_managed_node_bin_dir() -> Option<PathBuf> {
    let (platform, bin_subdir): (&str, Option<&str>) =
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => ("darwin-arm64", Some("bin")),
            ("macos", "x86_64") => ("darwin-x64", Some("bin")),
            ("linux", "x86_64") => ("linux-x64", Some("bin")),
            ("linux", "aarch64") => ("linux-arm64", Some("bin")),
            // Windows zips have node.exe + npm.cmd at the archive root — no bin/ subdir
            ("windows", "x86_64") => ("win-x64", None),
            ("windows", "aarch64") => ("win-arm64", None),
            _ => return None,
        };
    buzz_managed_node_root().map(|root| {
        let dir = root.join(BUZZ_MANAGED_NODE_VERSION).join(platform);
        match bin_subdir {
            Some(sub) => dir.join(sub),
            None => dir,
        }
    })
}

pub fn buzz_managed_node_bin_path() -> Option<PathBuf> {
    buzz_managed_node_bin_dir().map(|bin| {
        #[cfg(windows)]
        {
            bin.join("node.exe")
        }
        #[cfg(not(windows))]
        {
            bin.join("node")
        }
    })
}

pub fn buzz_managed_npm_bin_dir() -> Option<PathBuf> {
    buzz_managed_npm_prefix().map(|prefix| {
        #[cfg(windows)]
        {
            prefix
        }
        #[cfg(not(windows))]
        {
            prefix.join("bin")
        }
    })
}

pub fn buzz_managed_command_path(command: &str, basename: &str) -> Option<PathBuf> {
    if command.contains(std::path::MAIN_SEPARATOR)
        || !matches!(
            command,
            "codex-acp" | "claude-agent-acp" | "claude-code-acp" | "node" | "npm"
        )
    {
        return None;
    }

    let mut dirs = Vec::new();
    if let Some(managed_bin) = buzz_managed_npm_bin_dir() {
        dirs.push(managed_bin);
    }
    if let Some(managed_node_bin) = buzz_managed_node_bin_dir() {
        dirs.push(managed_node_bin);
    }

    dirs.into_iter()
        .map(|dir| dir.join(basename))
        .find(|candidate| is_executable_file(candidate))
}
