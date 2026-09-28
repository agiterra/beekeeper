//! The branch authority's storage contract (ledger 275 A1, 277): it holds, or
//! it refuses — never a silent fall back to the child-writable `HEAD`, and
//! never "first seen" again because a source went away or failed.

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

/// A test-owned root: provider state, a repository and a seat worktree on
/// `seat-branch`.
struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
    seat: PathBuf,
}

fn fx() -> Fx {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let state = root.join("state");
    std::fs::create_dir_all(&state).expect("state");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let seat = root.join("seat");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "seat-branch",
            seat.to_str().expect("utf8"),
        ],
    );
    Fx {
        _dir: dir,
        root,
        state,
        seat,
    }
}

impl Fx {
    /// The host's store, in the app directory beside (not inside) the
    /// provider state, as the desktop lays it out.
    fn store(&self) -> PathBuf {
        self.root.join("coding-session-workdirs.json")
    }

    fn pointer(&self) -> PathBuf {
        self.state
            .join(crate::assignment_inputs::HOST_STORE_POINTER_FILE)
    }

    /// Declare the host and record the seat's tree on `branch`.
    fn host_records(&self, branch: &str) {
        self.write_store(serde_json::json!({ "session/builder": {
            "path": self.seat,
            "branch": branch,
        }}));
        std::fs::write(
            self.pointer(),
            serde_json::json!({
                "version": crate::assignment_inputs::HOST_STORE_POINTER_VERSION,
                "path": self.store(),
            })
            .to_string(),
        )
        .expect("pointer");
    }

    fn write_store(&self, worktrees: serde_json::Value) {
        std::fs::write(
            self.store(),
            serde_json::json!({ "version": 2, "worktrees": worktrees }).to_string(),
        )
        .expect("store");
    }

    fn authority(&self, bound: Option<&str>) -> Result<Option<String>, AuthorityError> {
        branch_authority(&self.state, &self.seat, bound)
    }

    fn child_repoints_head(&self) {
        git(&self.seat, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    }
}

#[test]
fn the_first_preparation_pins_head_and_later_ones_keep_the_pin() {
    let fx = fx();
    assert_eq!(
        branch_authority(&fx.state, &fx.seat, None),
        Ok(Some("seat-branch".to_owned()))
    );
    // A child re-points HEAD; the pin decides.
    git(&fx.seat, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert_eq!(
        branch_authority(&fx.state, &fx.seat, None),
        Ok(Some("seat-branch".to_owned()))
    );
}

/// Astra's A1 reproduction: a file where the pins directory must be. The
/// preparation refuses instead of granting from HEAD.
#[test]
fn a_pin_that_cannot_be_stored_refuses() {
    let fx = fx();
    std::fs::write(fx.state.join(BRANCH_PINS_DIR), "blocks the pin directory").expect("block");
    let error = branch_authority(&fx.state, &fx.seat, None).expect_err("refused");
    assert!(error.0.contains("branch pin"), "{error:?}");
}

#[test]
fn an_empty_or_corrupt_pin_refuses() {
    for content in [
        "",
        "\n",
        "../main",
        "a b",
        "main.lock",
        "seat-branch",
        r#"{"version":1,"branch":"../main","source":"first-head"}"#,
        r#"{"version":9,"branch":"seat-branch","source":"first-head"}"#,
        r#"{"version":1,"branch":"seat-branch","source":"child"}"#,
    ] {
        let fx = fx();
        let pin = pin_path(&fx.state, &fx.seat);
        std::fs::create_dir_all(pin.parent().expect("dir")).expect("dir");
        std::fs::write(&pin, content).expect("pin");
        git(&fx.seat, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        let error = branch_authority(&fx.state, &fx.seat, None).expect_err(content);
        assert!(error.0.contains("branch pin"), "{content:?}: {error:?}");
    }
}

#[test]
fn an_unreadable_pin_refuses() {
    let fx = fx();
    let pin = pin_path(&fx.state, &fx.seat);
    // A directory where the pin file must be: reading it fails, not "absent".
    std::fs::create_dir_all(&pin).expect("dir");
    assert!(branch_authority(&fx.state, &fx.seat, None).is_err());
}

#[test]
fn concurrent_first_preparations_agree_on_one_pin() {
    let fx = fx();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| branch_authority(&fx.state, &fx.seat, None)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });
    for result in results {
        assert_eq!(result, Ok(Some("seat-branch".to_owned())));
    }
    let leftovers: Vec<_> = std::fs::read_dir(fx.state.join(BRANCH_PINS_DIR))
        .expect("pins")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temporary files remain: {leftovers:?}"
    );
}

/// The host's own worktree record decides over HEAD and over the binding.
#[test]
fn the_hosts_worktree_record_is_the_authority() {
    let fx = fx();
    fx.host_records("seat-branch");
    fx.child_repoints_head();
    assert_eq!(
        fx.authority(Some("main")),
        Ok(Some("seat-branch".to_owned()))
    );
    // An unreadable host record refuses rather than falling through.
    std::fs::write(fx.store(), "{ not json").expect("corrupt");
    assert!(fx.authority(None).is_err());
}

/// Ledger 277, Astra's retained sequence: authority from the host record,
/// a child re-points HEAD, then the host pointer is corrupted, removed, or
/// otherwise unusable. Each later preparation — with no session binding —
/// either keeps `seat-branch` or refuses; none grants `main`.
/// One change a case makes to the fixture's host records.
type Change = fn(&Fx);

#[test]
fn a_used_host_record_that_fails_or_disappears_never_restores_head_authority() {
    let unusable_pointers: [(&str, Change); 5] = [
        ("corrupt pointer", |fx| {
            std::fs::write(fx.pointer(), "{ bad json").expect("corrupt")
        }),
        ("missing pointer", |fx| {
            std::fs::remove_file(fx.pointer()).expect("remove")
        }),
        ("unsupported pointer version", |fx| {
            std::fs::write(
                fx.pointer(),
                serde_json::json!({ "version": 99, "path": fx.store() }).to_string(),
            )
            .expect("future")
        }),
        ("relative pointer path", |fx| {
            std::fs::write(
                fx.pointer(),
                serde_json::json!({ "version": 1, "path": "coding-session-workdirs.json" })
                    .to_string(),
            )
            .expect("relative")
        }),
        ("pointer to another store", |fx| {
            let other = fx.root.join("elsewhere.json");
            std::fs::write(&other, r#"{"version":2,"worktrees":{}}"#).expect("other");
            std::fs::write(
                fx.pointer(),
                serde_json::json!({ "version": 1, "path": other }).to_string(),
            )
            .expect("moved")
        }),
    ];
    for (case, break_it) in unusable_pointers {
        let fx = fx();
        fx.host_records("seat-branch");
        assert_eq!(
            fx.authority(None),
            Ok(Some("seat-branch".to_owned())),
            "{case}"
        );
        fx.child_repoints_head();
        break_it(&fx);
        let error = fx.authority(None).expect_err(case);
        assert!(error.0.contains("host"), "{case}: {error:?}");
    }

    // Sources that legitimately stop naming the tree: the established pin
    // holds, still with no session binding.
    let silent_sources: [(&str, Change); 3] = [
        ("record removed", |fx| fx.write_store(serde_json::json!({}))),
        ("store file removed", |fx| {
            std::fs::remove_file(fx.store()).expect("remove")
        }),
        ("no worktrees key", |fx| {
            std::fs::write(fx.store(), r#"{"version":2}"#).expect("store")
        }),
    ];
    for (case, silence) in silent_sources {
        let fx = fx();
        fx.host_records("seat-branch");
        assert_eq!(
            fx.authority(None),
            Ok(Some("seat-branch".to_owned())),
            "{case}"
        );
        fx.child_repoints_head();
        silence(&fx);
        assert_eq!(
            fx.authority(None),
            Ok(Some("seat-branch".to_owned())),
            "{case}"
        );
    }
}

/// A malformed record is declared authority that failed, not "no record".
#[test]
fn a_malformed_host_record_refuses() {
    for worktrees in [
        serde_json::json!(["not", "a", "map"]),
        serde_json::json!({ "other/seat": { "branch": "x" } }),
    ] {
        let fx = fx();
        fx.host_records("seat-branch");
        fx.write_store(worktrees.clone());
        assert!(fx.authority(None).is_err(), "{worktrees}");
    }
}

/// The host re-cutting a tree on another branch is an explicit allocation:
/// it replaces the pin. The child's HEAD never does.
#[test]
fn only_the_host_reallocates_a_pinned_tree() {
    let fx = fx();
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
    fx.child_repoints_head();
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
    git(&fx.seat, &["branch", "-q", "next"]);
    fx.host_records("next");
    assert_eq!(fx.authority(None), Ok(Some("next".to_owned())));
    std::fs::remove_file(fx.store()).expect("remove");
    assert_eq!(fx.authority(None), Ok(Some("next".to_owned())));
    // So does a host caller holding the allocation (establishment).
    record_allocation(&fx.state, &fx.seat, "seat-branch").expect("allocated");
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
}

/// A session binding that established the tree is pinned too, and a later
/// preparation without the binding keeps it; a binding that disagrees with
/// the pin refuses instead of picking one.
#[test]
fn a_session_binding_is_made_durable_and_a_disagreeing_one_refuses() {
    let fx = fx();
    fx.child_repoints_head();
    assert_eq!(
        fx.authority(Some("seat-branch")),
        Ok(Some("seat-branch".to_owned()))
    );
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
    assert!(fx.authority(Some("main")).is_err());
}

/// Every caller reaches one pin, however it spells the tree: a subdirectory
/// or a non-canonical path is the same tree.
#[test]
fn every_spelling_of_a_tree_reaches_one_pin() {
    let fx = fx();
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
    fx.child_repoints_head();
    let sub = fx.seat.join("deep/er");
    std::fs::create_dir_all(&sub).expect("sub");
    assert_eq!(
        branch_authority(&fx.state, &sub, None),
        Ok(Some("seat-branch".to_owned()))
    );
    let dotted = fx.seat.join("deep/..");
    assert_eq!(
        branch_authority(&fx.state, &dotted, None),
        Ok(Some("seat-branch".to_owned()))
    );
}

/// Two provider identities (and the desktop's host commands) on one
/// computer share the pins beside the host's store: a fresh state directory
/// with the same pointer still finds the tree established.
#[test]
fn every_state_directory_on_one_host_shares_its_pins() {
    let fx = fx();
    fx.host_records("seat-branch");
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
    fx.child_repoints_head();
    fx.write_store(serde_json::json!({}));
    let other = fx.root.join("other-identity");
    std::fs::create_dir_all(&other).expect("other");
    std::fs::copy(
        fx.pointer(),
        other.join(crate::assignment_inputs::HOST_STORE_POINTER_FILE),
    )
    .expect("pointer");
    assert_eq!(
        branch_authority(&other, &fx.seat, None),
        Ok(Some("seat-branch".to_owned()))
    );
}

#[test]
fn a_detached_tree_nobody_records_gets_no_branch() {
    let fx = fx();
    git(&fx.seat, &["checkout", "-q", "--detach"]);
    assert_eq!(fx.authority(None), Ok(None));
    // Established on no branch: a child pointing HEAD at main gains nothing.
    fx.child_repoints_head();
    assert_eq!(fx.authority(None), Ok(None));
}

/// Ledger 280(4): a provider that established trees while standalone, then
/// gains a host, carries those decisions into the host's pin store — a
/// branch and a "no branch" alike. The tree is never first-seen again.
#[test]
fn the_first_host_declaration_carries_standalone_decisions_over() {
    for detached in [false, true] {
        let fx = fx();
        if detached {
            git(&fx.seat, &["checkout", "-q", "--detach"]);
        }
        let established = fx.authority(None).expect("standalone");
        fx.child_repoints_head();
        // The host appears; its store records nothing for this tree.
        fx.host_records("unrelated");
        fx.write_store(serde_json::json!({}));
        assert_eq!(
            fx.authority(None),
            Ok(established.clone()),
            "detached={detached}"
        );
        assert!(
            pin_path(&fx.root, &fx.seat).exists(),
            "the decision lives in the host's pin store now"
        );
        // And stays there on later preparations.
        assert_eq!(fx.authority(None), Ok(established), "detached={detached}");
    }
}

/// A tree this provider established one way while standalone, which the
/// host's store already established another way, refuses the declaration:
/// neither record is picked, and the declaration is not noted, so every
/// later preparation refuses too — even with a host record for the tree —
/// until one of the two pins is removed.
#[test]
fn a_standalone_decision_the_host_contradicts_refuses_the_declaration() {
    let fx = fx();
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
    let host_pin = pin_path(&fx.root, &fx.seat);
    std::fs::create_dir_all(host_pin.parent().expect("dir")).expect("dir");
    std::fs::write(
        &host_pin,
        r#"{"version":1,"branch":"main","source":"first-head"}"#,
    )
    .expect("other identity's pin");
    fx.host_records("unrelated");
    fx.write_store(serde_json::json!({}));
    for _ in 0..2 {
        let error = fx.authority(None).expect_err("contradiction");
        assert!(error.0.contains("neither is picked"), "{error:?}");
    }
    assert!(!fx.state.join(HOST_DECLARED_FILE).exists());
    fx.host_records("seat-branch");
    assert!(fx.authority(None).is_err(), "still refused");
    // Removing the contradicting pin lets the standalone decision carry over.
    std::fs::remove_file(&host_pin).expect("remove");
    fx.child_repoints_head();
    assert_eq!(fx.authority(None), Ok(Some("seat-branch".to_owned())));
}
