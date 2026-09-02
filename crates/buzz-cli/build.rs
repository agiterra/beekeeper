//! Stamp the `bee` binary with the git commit it was built from.
//!
//! A seat runs whichever `bee` its `PATH` reaches first, and on 2026-09-01
//! that was the desktop app's bundled sidecar (`desktop/src-tauri/tauri.conf.json`
//! lists `binaries/bee`), not `~/.local/bin/bee` — so a seat ran an old CLI all
//! night while every fix sat in the checkout (`docs/SESSION_STATE.md` item 103,
//! finding 1). `bee --version` has to answer "which build is this", and the
//! only honest answer is the commit, not the crate version, which moves once a
//! release.
//!
//! The value is `BUZZ_CLI_GIT_SHA` when the build environment sets one and it
//! is commit-shaped (so a packaging pipeline that builds from an exported tree
//! can supply it), then the checkout's own short commit, and finally the
//! literal `unknown`. It is never invented, and a build from a tree with
//! uncommitted changes to tracked files is stamped `<sha>-dirty`, because that
//! binary is not the commit it names.
//!
//! The resolution — which gitdir, which ref file makes the stamp stale, and
//! what an acceptable override looks like — lives in `src/build_provenance.rs`
//! and is included verbatim, because `cargo test` never runs a build script
//! and this code had two defects a test would have caught (REVIEW-A1 F1, F12).

use std::process::Command;

// Brings `Path`/`PathBuf` with it, along with the resolution itself.
include!("src/build_provenance.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=BUZZ_CLI_GIT_SHA");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    // A source edit is exactly the case `-dirty` exists to disclose, so the
    // stamp is retaken when this crate's own sources move.
    println!("cargo:rerun-if-changed={manifest_dir}/src");
    watch_git_refs(&manifest_dir);

    let stamped = match std::env::var("BUZZ_CLI_GIT_SHA") {
        Ok(value) if is_plausible_stamp(value.trim()) => value.trim().to_owned(),
        Ok(value) if !value.trim().is_empty() => {
            println!(
                "cargo:warning=BUZZ_CLI_GIT_SHA={:?} does not name a commit; falling back to the \
                 checkout",
                value.trim()
            );
            stamp_from_checkout(&manifest_dir)
        }
        _ => stamp_from_checkout(&manifest_dir),
    };
    println!("cargo:rustc-env=BUZZ_CLI_GIT_SHA={stamped}");
}

/// The checkout's own answer: its short commit, marked dirty when the tree it
/// was built from carried uncommitted changes to tracked files.
fn stamp_from_checkout(manifest_dir: &str) -> String {
    let sha = git_short_sha(manifest_dir);
    stamp(
        sha.as_deref(),
        sha.is_some() && has_tracked_changes(manifest_dir),
    )
}

/// The checkout's short commit, or `None` when there is no readable one.
fn git_short_sha(manifest_dir: &str) -> Option<String> {
    let output = git(manifest_dir, &["rev-parse", "--short=9", "HEAD"])?;
    (!output.is_empty()).then_some(output)
}

/// Whether the checkout has uncommitted changes to tracked files.
///
/// Tracked only: an untracked scratch file beside the crate does not change
/// the code the binary contains, and treating it as if it did would make every
/// build of a working checkout read `-dirty` for no reason.
fn has_tracked_changes(manifest_dir: &str) -> bool {
    git(
        manifest_dir,
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .is_some_and(|output| !output.is_empty())
}

/// Rebuild the stamp when the checkout moves to another commit.
///
/// Watches `HEAD`, the ref `HEAD` resolves to — the file a commit actually
/// rewrites, which for a worktree lives in the common gitdir — and
/// `packed-refs`. Only paths that exist are handed to cargo: a watch on a
/// missing file re-runs the build script on every build.
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

/// Run one git command in the checkout, returning its trimmed stdout.
fn git(manifest_dir: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.args(["-C", manifest_dir]).args(args);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}
