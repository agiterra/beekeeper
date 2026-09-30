//! Where a launcher looks for the binaries it is about to run.
//!
//! Both launchers — the desktop app and `buzz-host` — must agree on this, and
//! for one specific reason: an installed bundle must prefer the sidecars
//! shipped inside it over any build artifact lying around in a source
//! checkout. Two launchers disagreeing means a bundled app running a
//! months-old provider from somebody's `target/debug`, which looks like a
//! product bug and reads like nothing at all in a log.
//!
//! These functions take the workspace root, the working directory and the
//! executable path as parameters rather than reading the process's own
//! environment, so every rule below is provable against directories a test
//! owns.

use std::path::{Path, PathBuf};

/// Whether `command` names a location rather than a binary to look up.
pub fn command_looks_like_path(command: &str) -> bool {
    let path = Path::new(command);
    path.is_absolute() || path.components().count() > 1
}

/// `command` with this platform's executable suffix, added only if absent.
pub fn executable_basename(command: &str) -> String {
    let suffix = std::env::consts::EXE_SUFFIX;
    if suffix.is_empty() || command.ends_with(suffix) {
        command.to_string()
    } else {
        format!("{command}{suffix}")
    }
}

/// Whether `path` is a file this process could execute.
///
/// On Unix this is a real permission check; on other platforms the existence
/// of a file is all the filesystem will tell us.
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn profile_target_dirs(root: &Path) -> [PathBuf; 2] {
    if cfg!(debug_assertions) {
        // `just dev` builds debug sidecars; never prefer stale release output.
        [root.join("target/debug"), root.join("target/release")]
    } else {
        [root.join("target/release"), root.join("target/debug")]
    }
}

fn macos_bundle_dir(executable: &Path) -> Option<&Path> {
    let binary_dir = executable.parent()?;
    let contents = binary_dir.parent()?;
    let bundle = contents.parent()?;
    (binary_dir.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then_some(binary_dir)
}

fn command_search_dirs(
    workspace: &Path,
    current_dir: Option<&Path>,
    executable: Option<&Path>,
) -> Vec<PathBuf> {
    // A packaged debug app is still an installed app. Its compiled-in source
    // checkout and launch directory must not substitute older build artifacts
    // for the sidecars shipped in that bundle.
    if let Some(bundle_dir) = executable.and_then(macos_bundle_dir) {
        return vec![bundle_dir.to_path_buf()];
    }

    let mut dirs = profile_target_dirs(workspace).to_vec();
    if let Some(current_dir) = current_dir {
        dirs.extend(profile_target_dirs(current_dir));
    }
    dirs.extend(executable.and_then(Path::parent).map(Path::to_path_buf));
    dirs.into_iter().fold(Vec::new(), |mut unique, dir| {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
        unique
    })
}

/// Resolve `command` against this build's own artifacts, or `None`.
///
/// This is the *first* thing a launcher tries; a miss falls through to the
/// caller's own `PATH` search.
pub fn resolve_workspace_command(
    command: &str,
    workspace: &Path,
    current_dir: Option<&Path>,
    executable: Option<&Path>,
) -> Option<PathBuf> {
    if command_looks_like_path(command) {
        let path = PathBuf::from(command);
        return is_executable_file(&path).then_some(path);
    }
    let file_name = executable_basename(command);
    command_search_dirs(workspace, current_dir, executable)
        .into_iter()
        .map(|dir| dir.join(&file_name))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(test)]
#[path = "command_paths_tests.rs"]
mod tests;
