//! Stamp `buzz-core` — and so every binary that links it — with the commit it
//! was built from.
//!
//! The resolution mirrors `crates/beekeeper-relay/build.rs`, deliberately: the relay
//! answers "what am I running" over NIP-11 and the client had no equivalent, so
//! the two are only comparable if they resolve the same way. The rules
//! themselves (what a commit looks like, what a count looks like, which
//! absences are disclosed and how) live in `src/build_info.rs`, which is
//! `include!`d here and compiled into the crate proper — `cargo test` never
//! runs a build script, and resolution logic that lives only in one has already
//! cost this repository two defects (REVIEW-A1 F1, F12).
//!
//! Order, and why:
//! 1. `BUZZ_SOURCE_SHA` + `BUZZ_SOURCE_COMMIT_COUNT`, the pair every packaging
//!    path in this repo already threads through (`Dockerfile`,
//!    `deploy/autodeploy/autodeploy`, `scripts/app-from.sh`). Checked first because a pipeline supplying it is
//!    *stating* the artifact identity, which should not be second-guessed by
//!    whatever `.git` happens to be in the build context.
//! 2. This checkout's `HEAD`, with its own `rev-list --count`.
//! 3. Neither — the vars are not emitted at all and `build_info()` discloses
//!    `unknown`/`null`. Never a guess.
//!
//! The count is only ever taken from whichever source gave the commit, so the
//! two can never describe different histories, and never from a shallow clone,
//! where `rev-list --count` returns the graft's size (a client subtracting that
//! would announce a drift of the entire history).
//!
//! **The build time is cached in `OUT_DIR` and refreshed only when the resolved
//! commit changes.** That is a load-bearing choice, not a micro-optimisation:
//! `buzz-core` is the root of this workspace's dependency graph, so any change
//! in this script's output recompiles every crate that depends on it. A wall
//! clock read fresh on each run would differ every time the script re-ran and
//! turn every incremental build into a workspace rebuild. Keyed on the stamp,
//! the output is byte-identical until the checkout actually moves — the one
//! case where a rebuild is what honesty requires.

#![allow(dead_code)] // `include!` brings in the whole module; the build only uses part of it.

use std::path::{Path, PathBuf};
use std::process::Command;

include!("src/build_info.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=BUZZ_SOURCE_SHA");
    println!("cargo:rerun-if-env-changed=BUZZ_SOURCE_COMMIT_COUNT");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    println!("cargo:rerun-if-changed={manifest_dir}/src/build_info.rs");
    watch_git_refs(&manifest_dir);

    let (commit, count) = resolve_stamp(&manifest_dir);
    let dirty = commit.is_some() && observed_dirty(&manifest_dir);
    if let Some(commit) = &commit {
        println!("cargo:rustc-env=BUZZ_CORE_SOURCE_SHA={commit}");
        if let Some(count) = count {
            println!("cargo:rustc-env=BUZZ_CORE_SOURCE_COMMIT_COUNT={count}");
        }
    }
    if dirty {
        // A dirty observation only. A clean claim is never embedded: cargo
        // cannot cheaply watch every path, so a later edit would leave a stale
        // false-clean in an incrementally rebuilt binary.
        println!("cargo:rustc-env=BUZZ_CORE_SOURCE_DIRTY=1");
    }
    println!(
        "cargo:rustc-env=BUZZ_CORE_BUILD_TIME={}",
        build_time(commit.as_deref(), count, dirty)
    );
}

/// The commit this binary is stamped with, and that commit's ordinal —
/// resolved **together**, never independently.
fn resolve_stamp(manifest_dir: &str) -> (Option<String>, Option<u64>) {
    if let Some(commit) = env_override("BUZZ_SOURCE_SHA") {
        let count = std::env::var("BUZZ_SOURCE_COMMIT_COUNT")
            .ok()
            .and_then(|value| parse_commit_count(&value));
        return (Some(commit), count);
    }
    match git(manifest_dir, &["rev-parse", "HEAD"]).filter(|sha| is_full_sha(sha)) {
        Some(commit) => {
            let count = git(manifest_dir, &["rev-list", "--count", "HEAD"])
                .filter(|_| !is_shallow(manifest_dir))
                .as_deref()
                .and_then(parse_commit_count);
            (Some(commit), count)
        }
        None => (None, None),
    }
}

/// `BUZZ_SOURCE_SHA`, when it is plausibly a commit. Never echoed back
/// unvalidated: an unset or malformed value falls through to the checkout.
fn env_override(var: &str) -> Option<String> {
    let value = std::env::var(var).ok()?;
    let value = value.trim().to_ascii_lowercase();
    is_full_sha(&value).then_some(value)
}

/// Whether the checkout carries uncommitted changes to tracked files, or
/// untracked non-ignored ones. Either makes the binary something other than
/// the commit it names.
fn observed_dirty(manifest_dir: &str) -> bool {
    let repo_root = match git(manifest_dir, &["rev-parse", "--show-toplevel"]) {
        Some(root) => root,
        None => return false,
    };
    git(&repo_root, &["status", "--porcelain"]).is_some_and(|out| !out.is_empty())
}

/// Whether this is a shallow clone, where `rev-list --count` returns the size
/// of the graft rather than the commit's ordinal.
fn is_shallow(manifest_dir: &str) -> bool {
    git(manifest_dir, &["rev-parse", "--is-shallow-repository"]).as_deref() != Some("false")
}

/// The stamp's build time, cached in `OUT_DIR` and refreshed only when the
/// commit it describes changes. See this module's header for why.
fn build_time(commit: Option<&str>, count: Option<u64>, dirty: bool) -> String {
    let key = format!(
        "{}|{}|{}",
        commit.unwrap_or(UNKNOWN_COMMIT),
        count.map_or(UNKNOWN_COUNT.to_owned(), |n| n.to_string()),
        u8::from(dirty)
    );
    let cache = std::env::var("OUT_DIR")
        .ok()
        .map(|dir| PathBuf::from(dir).join("build-time"));
    if let Some(cache) = &cache {
        if let Some(cached) = std::fs::read_to_string(cache)
            .ok()
            .and_then(|text| {
                text.split_once('\n')
                    .map(|(k, t)| (k.to_owned(), t.to_owned()))
            })
            .and_then(|(cached_key, time)| (cached_key == key).then_some(time))
        {
            return cached;
        }
    }
    let now = rfc3339_utc_now();
    if let Some(cache) = &cache {
        let _ = std::fs::write(cache, format!("{key}\n{now}"));
    }
    now
}

/// Rebuild the stamp when the checkout moves to another commit.
///
/// `HEAD` alone is not enough: on a branch it holds `ref: refs/heads/<name>`,
/// which a commit does not rewrite, and in a linked worktree that ref lives in
/// the *common* gitdir rather than beside its `HEAD`. Asking git for each path
/// (`rev-parse --git-path`) rather than resolving the layout by hand is the
/// approach `desktop/src-tauri/build.rs` already uses, and it gets worktrees
/// right for free. Only existing paths are handed to cargo: a watch on a
/// missing file re-runs the build script on every build, forever.
fn watch_git_refs(manifest_dir: &str) {
    let mut paths = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    if let Some(head_ref) = git(manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
        paths.push(head_ref);
    }
    for path in paths {
        if let Some(resolved) = git(manifest_dir, &["rev-parse", "--git-path", &path]) {
            let resolved = Path::new(manifest_dir).join(resolved);
            if resolved.exists() {
                println!("cargo:rerun-if-changed={}", resolved.display());
            }
        }
    }
}

/// Run one git command in `dir`, returning its trimmed stdout.
///
/// Clears the seven variables git exports into every hook it runs. A `cargo
/// build` under a hook — the pre-push gate is exactly that — would otherwise
/// read the *hook's* repository while `-C` pointed somewhere else, and a
/// binary that names the wrong commit is worse than one that says `unknown`.
/// `core.fsmonitor` and `core.hooksPath` are disabled for the same reason
/// `desktop/src-tauri/build.rs` disables them: a build script must not invoke
/// somebody's hook or wait on a daemon.
fn git(dir: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["-C", dir])
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args);
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(var);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned())
}
