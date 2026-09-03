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
//! 2. `BUZZ_SOURCE_SHA` — the build-arg `Dockerfile` already declares
//!    (`ARG`/`ENV`, consumed here at `cargo build` time) and every build path
//!    already threads through: `.github/workflows/docker.yml` for the public
//!    image, and `deploy/autodeploy/autodeploy` for hive/lightyear. This is
//!    the case `git` cannot answer: the relay's own `.dockerignore` excludes
//!    `.git/`, and `deploy/autodeploy/autodeploy` builds from a `git archive`
//!    export, which never had one.
//! 3. `unknown` — a legal, disclosed value, never invented.
//!
//! `BUZZ_SOURCE_SHA` is checked first, not the checkout: it is what the
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
    println!("cargo:rerun-if-env-changed=BUZZ_SOURCE_SHA");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    println!("cargo:rerun-if-changed={manifest_dir}/src");
    watch_git_refs(&manifest_dir);

    let commit = env_override("BUZZ_SOURCE_SHA")
        .or_else(|| git_full_sha(&manifest_dir))
        .unwrap_or_else(|| UNKNOWN_STAMP.to_string());
    println!("cargo:rustc-env=BUZZ_RELAY_SOURCE_SHA={commit}");
    println!(
        "cargo:rustc-env=BUZZ_RELAY_BUILD_TIME={}",
        rfc3339_utc_now()
    );
}

/// `BUZZ_SOURCE_SHA` from the build environment, when it is plausibly a
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
    // The same seven variables every other git spawn in this repository
    // clears (`buzz-cli`'s `git_command`, the desktop's
    // `GIT_REPO_SELECTION_VARS`): git exports `GIT_DIR` and friends into
    // every hook it runs, and a `cargo build` under one — the pre-push gate
    // is exactly that — would otherwise stamp this binary with the *hook's*
    // repository HEAD while `-C` was pointing somewhere else. A relay that
    // names the wrong commit is worse than one that says `unknown`.
    let mut command = Command::new("git");
    command.args(["-C", manifest_dir, "rev-parse", "HEAD"]);
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
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    is_full_sha(&sha).then_some(sha)
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
