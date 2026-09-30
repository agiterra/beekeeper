//! PATH augmentation for launched managed-agent child processes.

use std::path::PathBuf;

/// The pure kernel — `is_batch_shim`, `should_skip_claude_executable`,
/// `should_use_inherited` and `compose_path_entries` — lives in
/// `buzz_session_host_core::path_env`, because `buzz-host` composes a `PATH`
/// for the same child processes. It matters *more* there: a launchd- or
/// systemd-started process inherits a minimal `PATH` with no `node`, and the
/// ACP adapters are npm shims with `#!/usr/bin/env node` shebangs.
///
/// What stays here is [`build_augmented_path`], which is not portable: it
/// reaches for this app's managed npm and Node directories.
pub(crate) use buzz_session_host_core::path_env::{
    compose_path_entries, should_skip_claude_executable, should_use_inherited,
};

/// Assemble the augmented `PATH` for a launched managed-agent child process.
///
/// Concatenates, in priority order:
///   1. exe parent dir — the binaries shipped beside the app itself
///      (`Contents/MacOS/`: `bee`, `buzz-acp`, the other sidecars)
///   2. Buzz-managed npm prefix bin dir — app-private ACP adapter shims
///   3. Buzz-managed Node.js bin dir — app-private Node/npm runtime
///   4. `<home>/.local/bin` — the user's own CLI dir
///   5. `nvm_bin` — nvm's default Node.js bin dir (if the user uses nvm)
///   6. user's login-shell `PATH` — runtimes like node/python from other managers
///   7. the current process `PATH` — appended on every platform when no
///      login-shell PATH exists, because callers use `Command::env("PATH", …)`
///      which *replaces* the child's PATH. This is the steady state on Windows,
///      where `login_shell_path()` always returns `None` and without it the
///      child loses node/npm/git and every npm `.cmd` shim fails with
///      `'node' is not recognized`; on Unix it is the login-shell-probe failure
///      fallback, which keeps `curl`/`sh`/`tar` reachable. See
///      [`should_use_inherited`] for the suppression rules.
///
/// `shell_path` is the raw colon-delimited string from a login shell, so it is
/// split into individual entries before joining. Pushing it as a single segment
/// would make `join_paths` reject it (a segment containing the separator is an
/// error), collapsing the entire augmented `PATH` to `None` — the bug this
/// guards against, which left managed agents unable to find `buzz`. Returns
/// `None` only when no entries exist.
///
/// # Why the app's own directory comes first
///
/// `~/.local/bin` used to lead this list, on the reading that it holds the
/// bundled CLI symlink. It does not have to: it is an ordinary user directory
/// that anything may write. On 2026-08-27 a hand-repointed `~/.local/bin/bee`
/// shadowed the `bee` shipped inside the app, so a seat ran a binary this
/// build never produced and reported behaviour the build could not explain
/// (`plans/SESSION_STATE.md` item 77, *Fence* (c)). The app-owned directories
/// therefore outrank it: what the app ships is what a seat runs, and a user's
/// own `~/.local/bin` still resolves everything the app does not ship.
pub(in crate::managed_agents) fn build_augmented_path(
    home: Option<PathBuf>,
    exe_parent: Option<PathBuf>,
    shell_path: Option<String>,
    nvm_bin: Option<PathBuf>,
) -> Option<String> {
    let home_added = home.is_some();
    let exe_added = exe_parent.is_some();
    let has_local_context = home_added || exe_added;

    // Build the managed/prefix entries (everything before login-shell PATH).
    //
    // App-owned directories first — the exe parent, then the Buzz-managed npm
    // and Node bins — so the binaries this build ships win over anything a
    // user (or an earlier debugging session) left in `~/.local/bin`.
    let mut managed: Vec<PathBuf> = Vec::new();
    if let Some(parent) = exe_parent {
        managed.push(parent);
    }
    // Only add managed runtime dirs when a home or executable context exists.
    // This keeps tests/utility callers that intentionally pass no local context
    // from manufacturing a PATH out of ambient platform dirs alone.
    if has_local_context {
        if let Some(managed_npm_bin) = crate::managed_agents::buzz_managed_npm_bin_dir() {
            managed.push(managed_npm_bin);
        }
        if let Some(managed_node_bin) = crate::managed_agents::buzz_managed_node_bin_dir() {
            managed.push(managed_node_bin);
        }
    }
    if let Some(home) = home {
        managed.push(home.join(".local").join("bin"));
    }
    if let Some(nvm_bin) = nvm_bin {
        managed.push(nvm_bin);
    }

    // Split the login-shell PATH into individual entries.
    let had_shell_path = shell_path.is_some();
    let login: Vec<PathBuf> = shell_path
        .as_deref()
        .map(|s| std::env::split_paths(s).collect())
        .unwrap_or_default();

    let inherited: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let use_inherited = should_use_inherited(had_shell_path, has_local_context);

    let parts = compose_path_entries(managed, login, inherited, use_inherited);
    if parts.is_empty() {
        return None;
    }
    // join_paths uses the platform separator (':' on Unix, ';' on Windows).
    std::env::join_paths(parts)
        .ok()
        .map(|s| s.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::build_augmented_path;
    use std::path::PathBuf;

    /// Ledger 77 (Fence, c): a stale `~/.local/bin/bee` shadowed the `bee`
    /// shipped beside the app, so a seat ran a binary the build did not
    /// produce. The app's own executable directory and the Buzz-managed bins
    /// must therefore outrank `~/.local/bin`.
    #[cfg(unix)]
    #[test]
    fn bundled_binaries_outrank_local_bin() {
        let result = build_augmented_path(
            Some(PathBuf::from("/home/agent")),
            Some(PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS")),
            Some("/usr/local/bin:/usr/bin:/bin".to_string()),
            None,
        )
        .expect("path");
        let exe = result
            .find("/Applications/Beekeeper.app/Contents/MacOS")
            .expect("exe parent");
        let local = result.find("/home/agent/.local/bin").expect("local bin");
        assert!(
            exe < local,
            "the app's own executable dir must precede ~/.local/bin: {result}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn splits_colon_delimited_shell_path() {
        // Regression: the shell PATH arrives as one colon-delimited string. It
        // must be split into segments before join_paths, or join_paths rejects
        // it and the whole augmented PATH collapses to None (managed agents then
        // lose `buzz`).
        let result = build_augmented_path(
            Some(PathBuf::from("/home/agent")),
            Some(PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS")),
            Some("/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin".to_string()),
            None,
        );
        let result = result.expect("path");
        assert!(
            result.starts_with("/Applications/Beekeeper.app/Contents/MacOS:"),
            "{result}"
        );
        assert!(result.contains(":/home/agent/.local/bin:"), "{result}");
        assert!(
            result.ends_with(":/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"),
            "{result}"
        );
    }

    #[test]
    fn none_when_no_inputs() {
        assert_eq!(build_augmented_path(None, None, None, None), None);
    }

    #[cfg(unix)]
    #[test]
    fn shell_path_only() {
        let result = build_augmented_path(None, None, Some("/usr/bin:/bin".to_string()), None);
        assert_eq!(result.as_deref(), Some("/usr/bin:/bin"));
    }

    #[cfg(unix)]
    #[test]
    fn nvm_bin_inserted_after_local_bin_and_after_exe_parent() {
        let result = build_augmented_path(
            Some(PathBuf::from("/home/user")),
            Some(PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS")),
            Some("/usr/bin:/bin".to_string()),
            Some(PathBuf::from("/home/user/.nvm/versions/node/v20.0.0/bin")),
        );
        let result = result.expect("path");
        let local = result.find("/home/user/.local/bin").unwrap();
        let nvm = result
            .find("/home/user/.nvm/versions/node/v20.0.0/bin")
            .unwrap();
        let exe = result
            .find("/Applications/Beekeeper.app/Contents/MacOS")
            .unwrap();
        // The app's own directory leads; the user's own dirs keep their
        // relative order behind it.
        assert!(exe < local && local < nvm, "{result}");
        assert!(result.ends_with(":/usr/bin:/bin"), "{result}");
    }

    #[cfg(unix)]
    #[test]
    fn nvm_bin_none_does_not_add_segment() {
        let _guard = crate::managed_agents::lock_path_mutex();
        let previous = std::env::var_os("PATH");
        // With no shell_path the inherited process PATH is appended last, so
        // pin it to a sentinel to keep the assertion deterministic.
        std::env::set_var("PATH", "/sentinel/inherited");

        let result = build_augmented_path(
            Some(PathBuf::from("/home/user")),
            Some(PathBuf::from("/usr/local/bin")),
            None,
            None,
        );

        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        let result = result.expect("path");
        assert!(result.starts_with("/usr/local/bin:"), "{result}");
        assert!(!result.contains(".nvm"), "no nvm segment: {result}");
        assert!(
            result.contains(":/home/user/.local/bin:"),
            "the user's own bin dir must precede the inherited PATH: {result}"
        );
        assert!(
            result.ends_with(":/sentinel/inherited"),
            "inherited PATH must be appended last when no shell_path: {result}"
        );
    }

    /// On Unix with no login-shell PATH, `build_augmented_path` must fall back to
    /// the inherited process PATH — otherwise the child gets only Buzz-managed
    /// dirs and loses every system binary (`curl`, `sh`, `tar`).
    #[cfg(unix)]
    #[test]
    fn unix_appends_process_path_when_no_shell_path() {
        let _guard = crate::managed_agents::lock_path_mutex();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", "/usr/bin:/bin");

        let result = build_augmented_path(Some(PathBuf::from("/home/user")), None, None, None);

        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        let result = result.expect("path must not be None with a home dir");
        assert!(
            result.contains("/home/user/.local/bin:"),
            "home/.local/bin must be present: {result}"
        );
        assert!(
            result.ends_with(":/usr/bin:/bin"),
            "process PATH must be last: {result}"
        );
    }

    /// On Unix, supplying a `shell_path` must NOT also append the inherited
    /// process PATH — the login-shell PATH already carries the native entries.
    #[cfg(unix)]
    #[test]
    fn unix_shell_path_suppresses_inherited_fallback() {
        let _guard = crate::managed_agents::lock_path_mutex();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", "/should/not/appear");

        let result = build_augmented_path(
            Some(PathBuf::from("/home/user")),
            None,
            Some("/usr/local/bin:/usr/bin:/bin".to_string()),
            None,
        );

        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        let result = result.expect("path");
        assert!(
            result.ends_with(":/usr/local/bin:/usr/bin:/bin"),
            "Unix output must not append process PATH: {result}"
        );
    }

    /// On Windows: when no login-shell PATH is available, `build_augmented_path`
    /// must append the inherited process PATH so node/npm remain visible.
    #[cfg(windows)]
    #[test]
    fn windows_appends_process_path_when_no_shell_path() {
        let _guard = crate::managed_agents::lock_path_mutex();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", r"C:\Program Files\nodejs");

        let result = build_augmented_path(Some(PathBuf::from(r"C:\Users\agent")), None, None, None);

        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        let result = result.expect("path must not be None with a home dir");
        assert!(
            result.contains(r"C:\Users\agent\.local\bin;"),
            "home/.local/bin must be present: {result}"
        );
        assert!(
            result.ends_with(r";C:\Program Files\nodejs"),
            "process PATH must be last: {result}"
        );
    }

    /// On Windows: when a login-shell PATH IS supplied, the process PATH must
    /// NOT also be appended.
    #[cfg(windows)]
    #[test]
    fn windows_does_not_append_process_path_when_shell_path_present() {
        let _guard = crate::managed_agents::lock_path_mutex();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", r"C:\ShouldNotAppear");

        let result = build_augmented_path(
            Some(PathBuf::from(r"C:\Users\agent")),
            None,
            Some(r"C:\Program Files\nodejs".to_string()),
            None,
        );

        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        let result = result.expect("path");
        assert!(
            !result.contains("ShouldNotAppear"),
            "process PATH must not be appended when shell_path is present: {result}"
        );
    }

    /// On Windows: when no local context is provided, the function must return
    /// None even if the process PATH is set.
    #[cfg(windows)]
    #[test]
    fn windows_no_process_path_without_local_context() {
        let _guard = crate::managed_agents::lock_path_mutex();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", r"C:\Windows\System32");

        let result = build_augmented_path(None, None, None, None);

        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        assert_eq!(
            result, None,
            "must return None when no local context and no shell_path"
        );
    }
}
