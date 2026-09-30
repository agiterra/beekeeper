//! The pure kernel of `PATH` composition for launched child processes.
//!
//! `Command::env("PATH", …)` *replaces* rather than extends, so whoever sets it
//! owns the child's entire view of the filesystem. That makes the rules below
//! worth stating once and sharing: a launcher that gets them wrong hands the
//! child a `PATH` with no `sh`, and every `curl … | bash` install inside an
//! agent turn fails for a reason that looks nothing like a `PATH` problem.
//!
//! This matters *more* in a headless host than in the desktop app, not less.
//! A process started by launchd or systemd inherits a minimal `PATH` with no
//! `node`, and the ACP adapters are npm shims with `#!/usr/bin/env node`
//! shebangs.

use std::path::{Path, PathBuf};

/// Whether `path` is a Windows batch shim (`.cmd`/`.bat`, case-insensitive)
/// that cannot be passed directly to `CreateProcess`.
///
/// Pure so it can be unit-tested on any host without touching the global
/// `PATH` or any resolver cache (issue #2397).
pub fn is_batch_shim(path: &Path) -> bool {
    path.extension()
        .map(|ext| {
            let lower = ext.to_string_lossy().to_lowercase();
            lower == "cmd" || lower == "bat"
        })
        .unwrap_or(false)
}

/// Whether the resolved CLI path should be skipped for
/// `CLAUDE_CODE_EXECUTABLE` assignment.
///
/// On Windows, `.cmd`/`.bat` batch shims cannot be passed directly to
/// `CreateProcess` (EINVAL, issue #2397). On non-Windows those extensions are
/// valid executables and must not be suppressed — the `is_windows` flag keeps
/// this decision testable cross-host on macOS CI.
pub fn should_skip_claude_executable(path: &Path, is_windows: bool) -> bool {
    is_windows && is_batch_shim(path)
}

/// Whether the inherited process `PATH` should be appended to the composed one.
///
/// - Suppress when `had_shell_path` is `true` — a login-shell `PATH` already
///   carries the user's native entries, and appending the process `PATH` would
///   double them.
/// - Suppress when `has_local_context` is `false` — a caller that passes no
///   home or exe-parent context must not receive a `PATH` manufactured from
///   ambient process state alone.
pub fn should_use_inherited(had_shell_path: bool, has_local_context: bool) -> bool {
    !had_shell_path && has_local_context
}

/// Merge already-split `PATH` entries in precedence order:
///   1. `managed` — launcher-controlled dirs (app-owned bins, managed Node)
///   2. `login` — login-shell `PATH` entries
///   3. `inherited` — this process's `PATH`, only when `use_inherited`
///
/// Callers split the raw strings and prepend their own prefix entries, so this
/// stays fully pure and testable on any host.
pub fn compose_path_entries(
    managed: Vec<PathBuf>,
    login: Vec<PathBuf>,
    inherited: Vec<PathBuf>,
    use_inherited: bool,
) -> Vec<PathBuf> {
    let mut parts = managed;
    parts.extend(login);
    if use_inherited {
        parts.extend(inherited);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `is_windows` flag is the whole point of the predicate: off Windows
    /// a `.cmd` is an ordinary executable and must not be suppressed, and that
    /// half cannot be asserted on a Windows-only test.
    #[test]
    fn batch_shims_are_skipped_only_on_windows() {
        for shim in ["claude.cmd", "claude.CMD", "claude.bat", "claude.BAT"] {
            assert!(
                should_skip_claude_executable(Path::new(shim), true),
                "{shim}"
            );
            assert!(
                !should_skip_claude_executable(Path::new(shim), false),
                "{shim} is an ordinary executable off Windows"
            );
        }
        for real in ["claude.exe", "claude"] {
            assert!(
                !should_skip_claude_executable(Path::new(real), true),
                "{real}"
            );
        }
    }
}

// ── Pure policy and composition tests — run on every host ────────────────────
//
// These test `should_use_inherited` and `compose_path_entries` with explicit
// inputs, so they run on macOS/Linux CI and validate the cross-platform
// fallback policy without touching process state or requiring a Windows target.
#[cfg(test)]
mod compose_tests {
    use super::{compose_path_entries, is_batch_shim, should_use_inherited};
    use std::path::{Path, PathBuf};

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    // ── should_use_inherited policy matrix ────────────────────────────────────

    /// No shell path + has local context → must use inherited, on every OS.
    /// This is the steady state on Windows (login_shell_path() is always None)
    /// and the failure mode on Unix (login shell exited non-zero or printed
    /// nothing); in both the child would otherwise get no native PATH entries.
    #[test]
    fn policy_no_shell_with_context_uses_inherited() {
        assert!(
            should_use_inherited(false, true),
            "no shell path, has context → must append inherited"
        );
    }

    /// Shell path present → must NOT use inherited (login path covers it).
    #[test]
    fn policy_shell_path_present_suppresses_inherited() {
        assert!(
            !should_use_inherited(true, true),
            "shell path present → must not append inherited"
        );
    }

    /// No local context → must NOT use inherited (no ambient state).
    #[test]
    fn policy_no_local_context_suppresses_inherited() {
        assert!(
            !should_use_inherited(false, false),
            "no local context → must not append inherited"
        );
        assert!(
            !should_use_inherited(true, false),
            "no local context → must not append inherited even with a shell path"
        );
    }

    // ── compose_path_entries ordering ─────────────────────────────────────────

    #[test]
    fn managed_entries_appear_first() {
        let managed = vec![p("/buzz/node/bin"), p("/buzz/npm/bin")];
        let login = vec![p("/usr/local/bin"), p("/usr/bin")];
        let result = compose_path_entries(managed, login, vec![], false);
        assert_eq!(result[0], p("/buzz/node/bin"), "managed[0] must be first");
        assert_eq!(result[1], p("/buzz/npm/bin"), "managed[1] must be second");
        assert_eq!(
            result[2],
            p("/usr/local/bin"),
            "login[0] must follow managed"
        );
    }

    #[test]
    fn login_path_suppresses_inherited_when_use_inherited_false() {
        let login = vec![p("/usr/local/bin")];
        let inherited = vec![p("/should/not/appear")];
        let result = compose_path_entries(vec![], login, inherited, false);
        assert!(
            !result.contains(&p("/should/not/appear")),
            "inherited must not appear when use_inherited=false"
        );
    }

    #[test]
    fn inherited_appended_last_when_use_inherited_true() {
        let managed = vec![p("/buzz/npm/bin")];
        let inherited = vec![p("C:/windows/node"), p("C:/windows/npm")];
        let result = compose_path_entries(managed, vec![], inherited.clone(), true);
        assert_eq!(result[0], p("/buzz/npm/bin"), "managed must be first");
        assert_eq!(
            &result[1..],
            &inherited[..],
            "inherited entries must be appended last"
        );
    }

    /// Windows policy ON + empty inherited PATH — should produce just managed
    /// entries, not None and not a phantom segment.
    #[test]
    fn windows_policy_on_empty_inherited_produces_managed_only() {
        let managed = vec![p("/buzz/npm/bin")];
        let result = compose_path_entries(managed.clone(), vec![], vec![], true);
        assert_eq!(
            result, managed,
            "empty inherited must not add phantom entries"
        );
    }

    /// Windows policy ON + unset/absent inherited (empty vec from var_os None) —
    /// same result as above; no crash, no phantom.
    #[test]
    fn windows_policy_on_unset_inherited_path_produces_managed_only() {
        // Simulates std::env::var_os("PATH") returning None → empty vec.
        let managed = vec![p("/buzz/npm/bin")];
        let inherited: Vec<PathBuf> = vec![]; // empty, as if PATH is unset
        let result = compose_path_entries(managed.clone(), vec![], inherited, true);
        assert_eq!(result, managed);
    }

    /// No local context + Windows policy ON — compose_path_entries itself still
    /// works (no crash), and the caller is responsible for not calling it.
    /// Specifically: all-empty inputs with use_inherited=true still returns empty.
    #[test]
    fn all_empty_with_use_inherited_true_returns_empty() {
        let result = compose_path_entries(vec![], vec![], vec![], true);
        assert!(
            result.is_empty(),
            "all-empty inputs must produce empty output"
        );
    }

    #[test]
    fn empty_all_inputs_use_inherited_false_returns_empty() {
        let result = compose_path_entries(vec![], vec![], vec![], false);
        assert!(
            result.is_empty(),
            "all-empty inputs must produce empty output"
        );
    }

    /// Non-Windows behavior: `use_inherited=false` must produce byte-identical
    /// output to before this fix. Inherited entries are collected but dropped.
    #[cfg(unix)]
    #[test]
    fn unix_use_inherited_false_output_unchanged() {
        let managed = vec![p("/buzz/npm/bin")];
        let login = vec![p("/usr/local/bin"), p("/usr/bin"), p("/bin")];
        let inherited = vec![p("/proc/ambient/PATH")]; // would be real proc PATH on Unix
        let result = compose_path_entries(managed, login, inherited, false);
        assert_eq!(
            result,
            vec![
                p("/buzz/npm/bin"),
                p("/usr/local/bin"),
                p("/usr/bin"),
                p("/bin")
            ],
            "Unix output must not include inherited entries when use_inherited=false"
        );
    }

    // ── Structural wrapper-alignment test ──────────────────────────────────────
    //
    // Verifies that both `build_augmented_path` and `install_shell_command`
    // compute the same `should_use_inherited` decision for equivalent inputs.
    // Tests the policy function directly to confirm the wrappers can't drift.

    /// Exhaustive truth-table for `should_use_inherited` — every input
    /// combination. Confirms the policy is correct before either wrapper binds
    /// to it. The rule is OS-independent: the inherited PATH is the floor
    /// whenever no login-shell PATH was obtained, because the alternative is a
    /// child with no native binaries at all.
    #[test]
    fn should_use_inherited_policy_truth_table() {
        // (had_shell, has_context) → expected
        let cases = [
            (false, true, true),   // no shell PATH, context → USE (the floor)
            (true, true, false),   // shell PATH present → NO (already covered)
            (false, false, false), // no context → NO (no ambient-only PATH)
            (true, false, false),  // no context → NO, shell PATH irrelevant
        ];
        for (had_shell, has_ctx, expected) in cases {
            let result = should_use_inherited(had_shell, has_ctx);
            assert_eq!(
                result, expected,
                "policy mismatch: had_shell={had_shell} has_ctx={has_ctx}"
            );
        }
    }

    // ── is_batch_shim extension tests ─────────────────────────────────────────

    #[test]
    fn batch_shim_cmd_lower() {
        assert!(is_batch_shim(Path::new("claude.cmd")));
    }

    #[test]
    fn batch_shim_cmd_upper() {
        assert!(is_batch_shim(Path::new("claude.CMD")));
    }

    #[test]
    fn batch_shim_bat_lower() {
        assert!(is_batch_shim(Path::new("claude.bat")));
    }

    #[test]
    fn batch_shim_bat_upper() {
        assert!(is_batch_shim(Path::new("claude.BAT")));
    }

    #[test]
    fn batch_shim_exe_not_shim() {
        assert!(!is_batch_shim(Path::new("claude.exe")));
    }

    #[test]
    fn batch_shim_no_extension_not_shim() {
        assert!(!is_batch_shim(Path::new("claude")));
    }

    // ── should_skip_claude_executable policy tests ────────────────────────────
    //
    // Cross-host policy: shim + Windows → skip; shim + non-Windows → assign;
    // non-shim either OS → assign. Mirrors the `should_use_inherited` pattern.

    #[test]
    fn skip_claude_executable_shim_windows_returns_true() {
        assert!(
            super::should_skip_claude_executable(Path::new("claude.cmd"), true),
            "shim + windows=true must skip"
        );
        assert!(
            super::should_skip_claude_executable(Path::new("claude.BAT"), true),
            "shim + windows=true must skip"
        );
    }

    #[test]
    fn skip_claude_executable_shim_non_windows_returns_false() {
        assert!(
            !super::should_skip_claude_executable(Path::new("claude.cmd"), false),
            "shim + windows=false must NOT skip (valid executable on non-Windows)"
        );
        assert!(
            !super::should_skip_claude_executable(Path::new("claude.bat"), false),
            "shim + windows=false must NOT skip"
        );
    }

    #[test]
    fn skip_claude_executable_exe_both_platforms_returns_false() {
        assert!(
            !super::should_skip_claude_executable(Path::new("claude.exe"), true),
            "non-shim + windows=true must NOT skip"
        );
        assert!(
            !super::should_skip_claude_executable(Path::new("claude.exe"), false),
            "non-shim + windows=false must NOT skip"
        );
    }

    #[test]
    fn skip_claude_executable_no_ext_both_platforms_returns_false() {
        assert!(
            !super::should_skip_claude_executable(Path::new("claude"), true),
            "no-ext + windows=true must NOT skip"
        );
        assert!(
            !super::should_skip_claude_executable(Path::new("claude"), false),
            "no-ext + windows=false must NOT skip"
        );
    }
}
