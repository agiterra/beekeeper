//! Finding the binaries the host runs, from a process with almost no `PATH`.
//!
//! This is the one place the host has to work harder than the desktop app, not
//! less. A launchd- or systemd-started process inherits a minimal environment:
//! on macOS typically `/usr/bin:/bin:/usr/sbin:/sbin` and nothing else. The
//! ACP adapters are npm shims with `#!/usr/bin/env node` shebangs, and `node`
//! is not in that list. So the login shell probe — which in the desktop app is
//! a fallback for an unusual case — is the *normal* path here.
//!
//! # Where this deliberately agrees with the desktop, and where it does not
//!
//! The two steps that carry a real hazard are shared, in
//! `beekeeper_host_core`:
//!
//! - [`beekeeper_host_core::command_paths::resolve_workspace_command`] runs
//!   first, so an installed bundle prefers the `beekeeper-session-provider` shipped
//!   inside it over a stale `target/debug` — the stale-binary hazard that
//!   function exists for.
//! - [`beekeeper_host_core::managed_node`] names the app-private npm and
//!   Node directories, so both launchers find the same `claude-agent-acp`.
//!
//! The tail — login shell, well-known directories, nvm — is host-local and
//! deliberately simpler than `managed_agents::discovery`, which additionally
//! handles Windows `.cmd` shims and caches across many agent spawns. The host
//! resolves four binaries, once per provider spawn.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use beekeeper_host_core::command_paths::{
    command_looks_like_path, executable_basename, is_executable_file, resolve_workspace_command,
};
use beekeeper_host_core::managed_node::{
    beekeeper_managed_command_path, beekeeper_managed_node_bin_dir, beekeeper_managed_npm_bin_dir,
};

/// Resolve `command` to an absolute path, or `None`.
pub fn resolve_command(command: &str) -> Option<PathBuf> {
    // Explicit paths stay explicit — an operator who wrote one in `host.json`
    // means it.
    if command_looks_like_path(command) {
        let path = PathBuf::from(command);
        return is_executable_file(&path).then_some(path);
    }

    // 1. Beside this executable, then this build's target dirs. First, so a
    //    bundled host runs the provider it shipped with.
    if let Some(path) = resolve_workspace_command(
        command,
        workspace_root_dir(),
        std::env::current_dir().ok().as_deref(),
        std::env::current_exe().ok().as_deref(),
    ) {
        return Some(path);
    }

    // 2. The app-private npm/Node directories, for the ACP adapters.
    let basename = executable_basename(command);
    if let Some(path) = beekeeper_managed_command_path(command, &basename) {
        return Some(path);
    }

    // 3. The inherited `PATH`, minimal though it is.
    if let Some(path) = find_in_path_string(&std::env::var("PATH").unwrap_or_default(), &basename) {
        return Some(path);
    }

    // 4. A login shell. Under launchd this is where `node` actually is.
    if let Some(path) = find_via_login_shell(command) {
        return Some(path);
    }

    // 5. Well-known directories, in case the login shell is unavailable too.
    well_known_dirs()
        .iter()
        .map(|dir| dir.join(&basename))
        .find(|candidate| is_executable_file(candidate))
}

/// The repository this host was compiled in, for the dev-build case.
///
/// `CARGO_MANIFEST_DIR` is `crates/beekeeper-host`, so the workspace root is two up.
fn workspace_root_dir() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn find_in_path_string(path_var: &str, basename: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(basename))
        .find(|candidate| is_executable_file(candidate))
}

/// The user's `PATH` as a login shell reports it, probed once.
///
/// Cached for the host's lifetime: the answer cannot change without the user
/// editing a shell profile, and a restart is how that takes effect — the same
/// rule the rest of the discovery surface follows.
pub fn login_shell_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        run_in_login_shell(&["-l", "-c", "echo $PATH"]).and_then(|stdout| {
            stdout
                .lines()
                .rfind(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
        })
    })
    .as_deref()
}

fn login_shell_candidates() -> Vec<PathBuf> {
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/bin/zsh"), PathBuf::from("/bin/bash")]
    }
    #[cfg(windows)]
    {
        // Git Bash reports POSIX colon-delimited paths, which poison native
        // Windows children that split on `;`. The desktop app refuses the
        // login-shell probe on Windows for that reason and so does this.
        Vec::new()
    }
}

/// How long one login shell gets to answer.
///
/// Not unbounded: the supervisor resolves binaries on its way to starting the
/// provider, so a shell that never returns is a provider that never starts. On
/// 2026-10-01, at load average 34 straight after a build, each `bash -l` sat
/// in the loader for over a minute and the provider started four minutes after
/// the host (ledger 302(g)). A shell that answers at all answers in well under
/// a second; past this the probe is abandoned and the next step tried.
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(10);

fn run_in_login_shell(args: &[&str]) -> Option<String> {
    for shell in login_shell_candidates() {
        let mut command = Command::new(&shell);
        command.args(args);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::null());
        let Some(stdout) = output_within(command, LOGIN_SHELL_TIMEOUT) else {
            continue;
        };
        let stdout = stdout.trim().to_string();
        if !stdout.is_empty() {
            return Some(stdout);
        }
    }
    None
}

/// Run `command` to completion and return its stdout if it succeeded within
/// `limit`; a run past the limit is killed and reaped, and reads as no answer.
///
/// The probes print a line or two, far under a pipe's buffer, so reading after
/// the exit cannot deadlock against a child blocked on a full pipe.
fn output_within(mut command: Command, limit: Duration) -> Option<String> {
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    Some(stdout)
}

fn find_via_login_shell(command: &str) -> Option<PathBuf> {
    let stdout = run_in_login_shell(&["-l", "-c", r#"command -v -- "$1""#, "_", command])?;
    let resolved = stdout.lines().rfind(|line| !line.trim().is_empty())?;
    let path = PathBuf::from(resolved.trim());
    (path.is_absolute() && is_executable_file(&path)).then_some(path)
}

fn well_known_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut dirs = Vec::new();
        dirs.extend(beekeeper_managed_npm_bin_dir());
        dirs.extend(beekeeper_managed_node_bin_dir());
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
        ]);
        if let Some(home) = dirs::home_dir() {
            dirs.extend([
                home.join(".local/share/mise/shims"),
                home.join(".local/bin"),
                home.join(".volta/bin"),
                home.join(".asdf/shims"),
                home.join(".bun/bin"),
                home.join(".nvm/versions/node"),
            ]);
        }
        dirs
    })
}

/// The `PATH` handed to the provider and every adapter it spawns.
///
/// Composed through the shared kernel so the precedence rules are the same
/// ones the desktop app applies: app-owned directories first, then the login
/// shell, then this process's own `PATH` as the floor.
///
/// `exe_parent` leads because what this build ships is what the provider runs
/// — the same reason the desktop's version puts it first (ledger 77, where a
/// hand-repointed `~/.local/bin/bee` shadowed the `bee` inside the app and a
/// seat ran a binary the build never produced).
pub fn augmented_path() -> Option<String> {
    use beekeeper_host_core::path_env::{compose_path_entries, should_use_inherited};

    let home = dirs::home_dir();
    let exe_parent = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let has_local_context = home.is_some() || exe_parent.is_some();

    let mut managed: Vec<PathBuf> = Vec::new();
    managed.extend(exe_parent);
    if has_local_context {
        managed.extend(beekeeper_managed_npm_bin_dir());
        managed.extend(beekeeper_managed_node_bin_dir());
    }
    if let Some(home) = &home {
        managed.push(home.join(".local").join("bin"));
    }

    let shell_path = login_shell_path();
    let login: Vec<PathBuf> = shell_path
        .map(|value| std::env::split_paths(value).collect())
        .unwrap_or_default();
    let inherited: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();

    let parts = compose_path_entries(
        managed,
        login,
        inherited,
        should_use_inherited(shell_path.is_some(), has_local_context),
    );
    if parts.is_empty() {
        return None;
    }
    std::env::join_paths(parts)
        .ok()
        .map(|joined| joined.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }

    /// A probe that never answers is abandoned at its limit, killed, and reads
    /// as no answer; one that answers in time is read (ledger 302(g)).
    #[cfg(unix)]
    #[test]
    fn a_probe_past_its_limit_is_abandoned_and_one_in_time_is_read() {
        let mut slow = Command::new("/bin/sh");
        slow.args(["-c", "sleep 30"])
            .stdout(std::process::Stdio::piped());
        let started = Instant::now();
        assert_eq!(output_within(slow, Duration::from_millis(200)), None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "abandoned at the limit, not at the child's exit: {:?}",
            started.elapsed()
        );

        let mut quick = Command::new("/bin/sh");
        quick
            .args(["-c", "echo /found/claude"])
            .stdout(std::process::Stdio::piped());
        assert_eq!(
            output_within(quick, Duration::from_secs(5)).as_deref(),
            Some("/found/claude\n")
        );

        let mut failed = Command::new("/bin/sh");
        failed
            .args(["-c", "echo partial; exit 1"])
            .stdout(std::process::Stdio::piped());
        assert_eq!(output_within(failed, Duration::from_secs(5)), None);
    }

    #[test]
    fn an_explicit_path_stays_explicit_and_a_missing_one_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("my-provider");
        executable(&binary);
        assert_eq!(
            resolve_command(binary.to_str().expect("utf8")),
            Some(binary.clone())
        );
        assert_eq!(
            resolve_command(dir.path().join("absent").to_str().expect("utf8")),
            None
        );
    }

    #[test]
    fn a_minimal_path_is_searched_entry_by_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("sbin");
        let name = executable_basename("host-probe-binary");
        executable(&bin.join(&name));
        let path_var = std::env::join_paths([dir.path().join("empty"), bin.clone()])
            .expect("join")
            .to_string_lossy()
            .into_owned();
        assert_eq!(find_in_path_string(&path_var, &name), Some(bin.join(&name)));
        assert_eq!(find_in_path_string(&path_var, "definitely-absent"), None);
    }

    /// The composed `PATH` is what the adapters' `#!/usr/bin/env node` shebang
    /// resolves against, so it must never come back empty on a machine with a
    /// home directory — that is the launchd failure this function exists for.
    #[test]
    fn the_composed_path_is_never_empty_when_there_is_local_context() {
        let composed = augmented_path().expect("a host with a home has a PATH to hand down");
        assert!(!composed.is_empty());
        if let Some(home) = dirs::home_dir() {
            let local_bin = home.join(".local").join("bin");
            assert!(
                composed.contains(&local_bin.to_string_lossy().into_owned()),
                "the user's own bin dir must be reachable: {composed}"
            );
        }
    }
}
