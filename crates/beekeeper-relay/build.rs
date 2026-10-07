//! Stamp the relay binary with the commit and time it was built.
//!
//! Finding 32 (`review-2026-09-01/LIVE-RUN-TeamRolesV1.md`): the relay's
//! NIP-11 document advertised only `version: 0.2.1` — no build identity — so
//! whether a push had actually redeployed hive could not be observed by
//! mechanism. `source_sha()`/`build_time()` (`src/build_info.rs`) answer
//! that; this script produces the two compile-time env vars they read.
//!
//! Resolution order for the commit:
//! 1. `git rev-parse HEAD` in this crate's own checkout, when `.git` is
//!    present — a native `cargo build`, or a Docker build stage that `COPY`s
//!    the full working tree.
//! 2. `BEEKEEPER_SOURCE_SHA` — the build-arg `Dockerfile` already declares
//!    (`ARG`/`ENV`, consumed here at `cargo build` time) and every build path
//!    already threads through: `deploy/autodeploy/autodeploy` for hive. This is
//!    the case `git` cannot answer: the relay's own `.dockerignore` excludes
//!    `.git/`, and `deploy/autodeploy/autodeploy` builds from a `git archive`
//!    export, which never had one.
//! 3. `unknown` — a legal, disclosed value, never invented.
//!
//! `BEEKEEPER_SOURCE_SHA` is checked first, not the checkout: it is what the
//! Dockerfile calls the "compile immutable artifact identity" input, and a
//! packaging pipeline supplying it is stating an intent that should not be
//! second-guessed by whatever `.git` happens to be lying around in the same
//! build context (there normally is none, per `.dockerignore`, but a native
//! build invoked with the var set for testing should still honor it).
//!
//! Resolution logic lives in `src/build_provenance.rs` and is included
//! verbatim, because `cargo test` never runs a build script.

use std::process::Command;

include!("src/build_provenance.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=BEEKEEPER_SOURCE_SHA");
    println!("cargo:rerun-if-env-changed=BEEKEEPER_SOURCE_COMMIT_COUNT");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    println!("cargo:rerun-if-changed={manifest_dir}/src");
    watch_git_refs(&manifest_dir);

    let (commit, count) = resolve_stamp(&manifest_dir);
    println!("cargo:rustc-env=BEEKEEPER_RELAY_SOURCE_SHA={commit}");
    println!(
        "cargo:rustc-env=BEEKEEPER_RELAY_SOURCE_COMMIT_COUNT={}",
        count.map(|n| n.to_string()).unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=BEEKEEPER_RELAY_BUILD_TIME={}",
        rfc3339_utc_now()
    );
}

/// The commit this binary is stamped with, and that commit's ordinal —
/// resolved **together**, never independently.
///
/// The coupling is the point. A count read from this checkout while the SHA
/// came from `BEEKEEPER_SOURCE_SHA` would disclose `software_commit` from one
/// history and `software_commit_count` from another: two internally
/// consistent-looking fields describing different objects, which no consumer
/// could detect. So a source that answers for the commit answers for the
/// count or yields none at all.
///
/// 1. `BEEKEEPER_SOURCE_SHA`, paired with `BEEKEEPER_SOURCE_COMMIT_COUNT`. When that
///    var is absent or malformed the count is `None` — deliberately *not*
///    falling through to the checkout, whose history is by assumption not
///    this commit's.
/// 2. This checkout's `HEAD`, paired with its own `rev-list --count`.
/// 3. (`unknown`, `None`).
fn resolve_stamp(manifest_dir: &str) -> (String, Option<u32>) {
    if let Some(commit) = env_override("BEEKEEPER_SOURCE_SHA") {
        let count = std::env::var("BEEKEEPER_SOURCE_COMMIT_COUNT")
            .ok()
            .and_then(|value| parse_commit_count(&value));
        return (commit, count);
    }
    match git_full_sha(manifest_dir) {
        Some(commit) => (commit, git_commit_count(manifest_dir)),
        None => (UNKNOWN_STAMP.to_string(), None),
    }
}

/// This checkout's `git rev-list --count HEAD`, or `None`.
///
/// `None` on a shallow clone, where the number would be the graft's size
/// rather than the commit's ordinal — see [`is_shallow_checkout`]. Asked two
/// ways because they fail differently: the marker file catches the case
/// `git` cannot be spawned, and `--is-shallow-repository` catches a layout
/// the marker check does not model. Either saying shallow drops the count.
fn git_commit_count(manifest_dir: &str) -> Option<u32> {
    if resolve_git_dir(Path::new(manifest_dir))
        .as_deref()
        .is_some_and(is_shallow_checkout)
    {
        return None;
    }
    if git_stdout(manifest_dir, &["rev-parse", "--is-shallow-repository"])?.trim() != "false" {
        return None;
    }
    parse_commit_count(&git_stdout(manifest_dir, &["rev-list", "--count", "HEAD"])?)
}

/// `BEEKEEPER_SOURCE_SHA` from the build environment, when it is plausibly a
/// commit. Never invented: an unset or malformed value falls through to the
/// checkout, then to `unknown` — it is never echoed back unvalidated.
fn env_override(var: &str) -> Option<String> {
    let value = std::env::var(var).ok()?;
    let value = value.trim();
    is_full_sha(value).then(|| value.to_owned())
}

/// This checkout's own `HEAD`, as a full 40-hex commit, or `None` when `git`
/// is unavailable, this is not a git checkout, or `HEAD` is unborn.
fn git_full_sha(manifest_dir: &str) -> Option<String> {
    let sha = git_stdout(manifest_dir, &["rev-parse", "HEAD"])?;
    let sha = sha.trim();
    is_full_sha(sha).then(|| sha.to_owned())
}

/// `git -C <manifest_dir> <args>`'s stdout, or `None` when git is
/// unavailable, this is not a checkout, or the command failed.
///
/// Clears the same seven variables every other git spawn in this repository
/// clears (`buzz-cli`'s `git_command`, the desktop's
/// `GIT_REPO_SELECTION_VARS`): git exports `GIT_DIR` and friends into every
/// hook it runs, and a `cargo build` under one — the pre-push gate is
/// exactly that — would otherwise read the *hook's* repository while `-C`
/// was pointing somewhere else. A relay that names the wrong commit, or
/// counts the wrong history, is worse than one that says `unknown`.
fn git_stdout(manifest_dir: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.args(["-C", manifest_dir]);
    command.args(args);
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
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

/// Rebuild the stamp when the checkout moves to another commit.
fn watch_git_refs(manifest_dir: &str) {
    let Some(git_dir) = resolve_git_dir(Path::new(manifest_dir)) else {
        return;
    };
    for path in stamp_watch_paths(&git_dir) {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
