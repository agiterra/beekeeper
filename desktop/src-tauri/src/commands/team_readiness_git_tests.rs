//! Tests for the git side of team readiness — the read-only checkout probe in
//! `team_readiness_git.rs`.
//!
//! Split out of `team_readiness_tests.rs`, which sits at the repository's
//! 1000-line ceiling. These two are the only cases that shell out to `git`, so
//! they are also the only ones carrying the hook-environment hazard documented
//! on `GIT_REPO_SELECTION_VARS`.

use std::fs;
use std::process::Command;

use super::*;

#[test]
fn checkout_probe_detects_untracked_files_without_git_locks() {
    let temp = tempfile::tempdir().expect("tempdir");
    // `-C` does not win against an inherited `GIT_DIR`, and git exports one
    // into every hook it runs — so under the pre-push gate this fixture used
    // to `init`, `add` and `commit` straight into the developer's own
    // repository, leaving a stray "seed" commit on their branch and failing
    // the clean-source assertion below against their untracked files.
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_NAMESPACE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .arg("-C")
            .arg(temp.path())
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    };
    run(&["init", "-q"]);
    fs::write(temp.path().join("tracked"), "one").expect("tracked");
    run(&["add", "tracked"]);
    run(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-q",
        "--no-gpg-sign",
        "-m",
        "seed",
    ]);
    let (_, dirty) = checkout_source(temp.path()).expect("clean source");
    assert!(!dirty);

    fs::write(temp.path().join(".untracked-hidden"), "two").expect("untracked");
    let (_, dirty) = checkout_source(temp.path()).expect("dirty source");
    assert!(dirty);
    assert!(!temp.path().join(".git/index.lock").exists());
}

#[test]
fn checkout_probe_rejects_unexpected_git_exit_codes() {
    assert_eq!(validate_dirty(0, 1), Ok(false));
    assert_eq!(validate_dirty(1, 1), Ok(true));
    assert!(validate_dirty(128, 1).is_err());
    assert!(validate_dirty(0, 128).is_err());
}
