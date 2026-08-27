//! Terminal git access to the relay — the bridge out of the app's hermetic
//! git configuration.
//!
//! Every git subprocess the app runs gets an *ephemeral, env-only* auth config
//! (see [`super::project_git_exec::configure_git_auth`]): `GIT_CONFIG_GLOBAL`
//! pointed at `/dev/null`, the helper named by absolute path, and the nsec
//! passed in the child environment. Nothing is written to disk, which is the
//! right default — the identity key never lands anywhere it could be read.
//!
//! The cost is that importing a repository persists a Beekeeper remote into
//! `.git/config` while the credentials that made the app's own first push work
//! evaporate when the child exits. The user is then left with a remote they
//! cannot push to from a terminal, and git's failure — a username prompt
//! against a server that will never accept one — does not point at the cause.
//!
//! This module closes that gap **on request only**. It is deliberately not
//! automatic: it writes the user's identity to a file, and a control that does
//! that silently would be the app quietly lowering a protection the user never
//! chose.
//!
//! ## Why shell out to `bee`
//!
//! `bee git setup` already implements the config contract, with unit tests for
//! the URL scoping, the 0600 enforcement, and the refusal to overwrite a
//! different identity (`crates/buzz-cli/src/commands/git_setup.rs`). `bee` is
//! already a bundled sidecar (`tauri.conf.json`). Reimplementing the same rules
//! here would be a second copy to keep in step — and the failure mode of drift
//! between them is a config that looks right and does not authenticate.

use std::path::{Path, PathBuf};
use std::process::Command;

use nostr::ToBech32;
use tauri::{AppHandle, Manager, State};

use crate::app_state::AppState;
use crate::managed_agents::resolve_command;

/// Where the pinned helper lives, relative to the app data directory.
const PINNED_HELPER_DIR: &str = "bin";
const HELPER_NAME: &str = if cfg!(windows) {
    "git-credential-nostr.exe"
} else {
    "git-credential-nostr"
};

/// Resolve the stable path the helper is pinned at.
///
/// The bundled sidecar lives beside the executable — `Contents/MacOS` on macOS
/// — which is exactly right for spawning a child and exactly wrong for a git
/// config entry that must outlive the install. An app upgrade, or the user
/// dragging the bundle to a different folder, moves it; git would then be
/// pointed at a path that no longer exists, and every push would fail with a
/// credential error that names nothing useful.
fn pinned_helper_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve the app data directory: {error}"))?;
    Ok(data_dir.join(PINNED_HELPER_DIR).join(HELPER_NAME))
}

/// Copy the bundled helper to its pinned path, if it is not already there.
///
/// Returns the pinned path. Re-copies whenever the bytes differ, so an app
/// upgrade refreshes the pinned copy rather than leaving an old helper in place
/// against a newer relay.
fn pin_helper(app: &AppHandle) -> Result<PathBuf, String> {
    let source = resolve_command("git-credential-nostr").ok_or_else(|| {
        "git-credential-nostr was not found. It ships with the app, so this \
         usually means a broken or partial install."
            .to_string()
    })?;
    let target = pinned_helper_path(app)?;

    let source_bytes = std::fs::read(&source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let already_current = std::fs::read(&target)
        .map(|existing| existing == source_bytes)
        .unwrap_or(false);
    if already_current {
        return Ok(target);
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    // Replace rather than write in place: overwriting a running binary fails on
    // some platforms, and a partial write would leave an unusable helper that
    // still looks installed.
    let staged = target.with_extension("staged");
    std::fs::write(&staged, &source_bytes)
        .map_err(|error| format!("cannot write {}: {error}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("cannot mark {} executable: {error}", staged.display()))?;
    }
    std::fs::rename(&staged, &target)
        .map_err(|error| format!("cannot install {}: {error}", target.display()))?;
    Ok(target)
}

/// Build a `bee` invocation with the app's git-config environment stripped.
///
/// `project_git_exec` sets `GIT_CONFIG_GLOBAL=/dev/null` and friends on the
/// commands *it* spawns, not process-wide — but clearing them here is what
/// makes that guarantee explicit rather than incidental. `bee git setup` writes
/// the user's real `~/.gitconfig`; inheriting a redirected one would send the
/// config to `/dev/null` and report success.
fn bee_command(bee: &Path, relay_url: &str) -> Command {
    let mut command = Command::new(bee);
    command.args(["--relay", relay_url, "git"]);
    for key in [
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_DIR",
        "GIT_WORK_TREE",
    ] {
        command.env_remove(key);
    }
    crate::util::configure_no_window(&mut command);
    command
}

fn resolve_bee() -> Result<PathBuf, String> {
    resolve_command("bee").ok_or_else(|| {
        "the bee CLI was not found. It ships with the app, so this usually \
         means a broken or partial install."
            .to_string()
    })
}

fn run_bee(mut command: Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("cannot run bee: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Report whether terminal git access is configured, and whether it would work.
///
/// The distinction matters: config naming a helper that is no longer on disk
/// reads as "set up" to anyone inspecting `~/.gitconfig`, and fails at push
/// time. `bee git status` reports `helper_resolvable` for exactly that reason;
/// this command adds whether the *pinned* copy is present, which is the part
/// the app owns.
#[tauri::command]
pub fn git_terminal_access_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let relay_url = crate::relay::relay_api_base_url_with_override(&state);
    let bee = resolve_bee()?;
    let stdout = run_bee({
        let mut command = bee_command(&bee, &relay_url);
        command.arg("status");
        command
    })?;
    let mut report: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|error| format!("bee git status returned unreadable output: {error}"))?;

    let pinned = pinned_helper_path(&app)?;
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "pinned_helper".into(),
            serde_json::Value::String(pinned.to_string_lossy().to_string()),
        );
        object.insert(
            "pinned_helper_present".into(),
            serde_json::Value::Bool(pinned.is_file()),
        );
    }
    Ok(report)
}

/// Provision terminal git access: pin the helper, write the git config, and
/// write the identity to the key file.
///
/// **Only ever call this from an explicit user action that has told the user
/// the key is being written to disk and where.** The nsec leaving the OS
/// keyring is the whole point of the operation and also its only real cost;
/// presenting it as a routine toggle would misrepresent it.
#[tauri::command]
pub fn enable_git_terminal_access(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let relay_url = crate::relay::relay_api_base_url_with_override(&state);
    let keys = state.signing_keys()?;
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|error| format!("encode identity key: {error}"))?;

    let helper = pin_helper(&app)?;
    let bee = resolve_bee()?;
    let mut command = bee_command(&bee, &relay_url);
    command.args(["setup", "--write-key", "--helper"]);
    command.arg(&helper);
    command.env("BUZZ_PRIVATE_KEY", &nsec);
    run_bee(command)?;

    git_terminal_access_status(app, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_name_matches_the_platform_executable() {
        #[cfg(windows)]
        assert_eq!(HELPER_NAME, "git-credential-nostr.exe");
        #[cfg(not(windows))]
        assert_eq!(HELPER_NAME, "git-credential-nostr");
    }

    #[test]
    fn bee_command_clears_the_apps_redirected_git_config() {
        let command = bee_command(Path::new("/usr/bin/true"), "https://relay.example");
        let removed: Vec<&str> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .filter_map(|(key, _)| key.to_str())
            .collect();
        // Without this, `git config --global` inside bee would write to
        // /dev/null and still report success.
        assert!(removed.contains(&"GIT_CONFIG_GLOBAL"));
        assert!(removed.contains(&"GIT_CONFIG_NOSYSTEM"));
        assert!(removed.contains(&"GIT_CONFIG_COUNT"));
    }

    #[test]
    fn bee_command_passes_the_relay_through() {
        let command = bee_command(Path::new("/usr/bin/true"), "https://relay.example");
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, vec!["--relay", "https://relay.example", "git"]);
    }
}
