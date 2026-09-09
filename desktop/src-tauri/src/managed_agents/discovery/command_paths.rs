use std::path::{Path, PathBuf};

use super::{command_looks_like_path, executable_basename, is_executable_file};

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

pub(super) fn resolve_workspace_command(
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
