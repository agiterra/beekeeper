use super::*;

#[test]
fn slug_lowercases_and_hyphenates_words() {
    assert_eq!(
        worktree_slug("Improve Coding Session Creation").as_deref(),
        Some("improve-coding-session-creation")
    );
}

#[test]
fn slug_collapses_runs_of_separators() {
    assert_eq!(
        worktree_slug("fix   the --- push, timeout!").as_deref(),
        Some("fix-the-push-timeout")
    );
}

#[test]
fn slug_drops_leading_and_trailing_separators() {
    assert_eq!(worktree_slug("  -- hello --  ").as_deref(), Some("hello"));
}

#[test]
fn slug_rejects_a_name_with_nothing_to_slug() {
    assert_eq!(worktree_slug(""), None);
    assert_eq!(worktree_slug("   "), None);
    assert_eq!(worktree_slug("!!! ---"), None);
    // Non-ASCII collapses to separators, leaving nothing addressable.
    assert_eq!(worktree_slug("日本語"), None);
}

#[test]
fn slug_truncates_without_leaving_a_trailing_hyphen() {
    let long = "a".repeat(MAX_WORKTREE_SLUG_LEN + 10);
    assert_eq!(worktree_slug(&long).unwrap().len(), MAX_WORKTREE_SLUG_LEN);

    // A cut landing exactly on a separator must not produce "…-".
    let name = format!("{} tail", "b".repeat(MAX_WORKTREE_SLUG_LEN - 1));
    let slug = worktree_slug(&name).unwrap();
    assert!(!slug.ends_with('-'), "{slug} ends with a hyphen");
}

#[test]
fn slug_never_starts_with_a_hyphen() {
    // A leading hyphen would let a name reach git as an option.
    for name in ["--force", "-b other", " - dash"] {
        let slug = worktree_slug(name).expect("slug");
        assert!(!slug.starts_with('-'), "{slug} starts with a hyphen");
    }
}

/// Placement itself now lives in `buzz_core::worktree_placement` and is
/// tested there against every admissible shape. What belongs here is the one
/// fact this side establishes: whether the in-repo holder is usable. Both
/// halves matter, so both are pinned.
#[test]
fn an_absent_holder_is_not_usable() {
    let root = tempfile::tempdir().expect("tempdir");
    assert!(
        !holder_exists_and_is_ignored(root.path()),
        "a repository without a .worktrees directory has not opted in"
    );
}

#[test]
fn a_holder_that_git_does_not_ignore_is_not_usable() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(checkout.join(".worktrees")).expect("create holder");
    if scratch_repo(&checkout).is_err() {
        return;
    }
    assert!(
        !holder_exists_and_is_ignored(&checkout),
        "an unignored holder would put every live worktree in git status"
    );

    // The trailing slash on the probe is what makes a directory pattern
    // match; without it this would answer "not ignored" and the rule would
    // never fire for anyone.
    std::fs::write(checkout.join(".gitignore"), ".worktrees/\n").expect("write gitignore");
    assert!(
        holder_exists_and_is_ignored(&checkout),
        "an existing, ignored holder is the one case that opts in"
    );
}

#[test]
fn disambiguator_is_four_hex_characters() {
    let value = disambiguator();
    assert_eq!(value.len(), 4);
    assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn plan_reports_a_missing_working_directory_rather_than_failing() {
    let planned = plan("/definitely/not/a/directory/here", "some name", None, None).expect("plan");
    assert!(planned.path.is_none());
    assert_eq!(
        planned.problem.as_deref(),
        Some("Choose an existing working directory first.")
    );
}

#[test]
fn plan_reports_an_unnameable_worktree() {
    let planned = plan("/tmp", "!!!", None, None).expect("plan");
    assert!(planned.path.is_none());
    assert_eq!(
        planned.problem.as_deref(),
        Some("Give the worktree a name — letters and numbers.")
    );
}

/// A repository with one commit, so `HEAD` resolves.
fn scratch_repo(dir: &Path) -> Result<(), String> {
    let auth = build_local_git_auth_config()?;
    run_git(&["init", "--initial-branch=main", "."], Some(dir), &auth)?;
    run_git(
        &[
            "-c",
            "user.name=Beekeeper Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "root",
        ],
        Some(dir),
        &auth,
    )?;
    Ok(())
}

/// The whole point of the feature, against a real repository: a plan that
/// names a directory, a create that produces it, and a second create of the
/// same name that lands somewhere else instead of failing or colliding.
#[test]
fn a_worktree_is_planned_created_and_then_disambiguated() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(&checkout).expect("create checkout");
    if scratch_repo(&checkout).is_err() {
        // No usable git on this machine. The pure-function tests above still
        // cover the naming rules; skip rather than fail on a missing tool.
        return;
    }
    let checkout_str = checkout.to_string_lossy().into_owned();

    let planned = plan(&checkout_str, "Improve Coding Session Creation", None, None).expect("plan");
    assert_eq!(planned.problem, None);
    // The scratch repository's one branch is `main`, so the default start
    // point is `main` — even though `HEAD` would answer the same commit here.
    assert_eq!(planned.source.as_deref(), Some("main"));
    assert_eq!(
        planned.slug.as_deref(),
        Some("improve-coding-session-creation")
    );
    assert!(!planned.disambiguated);
    // Anchored at git's own answer for the repository root, which on macOS
    // resolves the symlinked temp dir (`/var` → `/private/var`). One sibling
    // folder per worktree, not a container: this repository has no ignored
    // `.worktrees` holder, so the sibling rule applies.
    let expected_parent = root.path().canonicalize().expect("canonical root");
    assert_eq!(planned.placement.as_deref(), Some("sibling"));
    assert_eq!(
        planned.path.as_deref().map(Path::new),
        Some(
            expected_parent
                .join("beekeeper-wt-improve-coding-session-creation")
                .as_path()
        )
    );

    // Create it the way the command does, minus the Tauri wrapper.
    let auth = build_local_git_auth_config().expect("git");
    std::fs::create_dir_all(&expected_parent).expect("create worktree folder");
    let first = planned.path.clone().expect("path");
    let branch = planned.branch.clone().expect("branch");
    run_git(
        &["worktree", "add", "-b", &branch, &first, "HEAD"],
        Some(&checkout),
        &auth,
    )
    .expect("worktree add");
    assert!(Path::new(&first).is_dir(), "worktree directory exists");
    assert!(
        branch_exists(&checkout, &branch).expect("branch probe"),
        "worktree branch exists"
    );

    // The same name again: same request, different answer, and it says so.
    let again = plan(&checkout_str, "Improve Coding Session Creation", None, None).expect("plan");
    assert_eq!(again.problem, None);
    assert!(again.disambiguated, "a taken name must be disambiguated");
    assert_ne!(again.path, planned.path);
    assert_ne!(again.branch, planned.branch);
    assert!(again
        .slug
        .as_deref()
        .expect("slug")
        .starts_with("improve-coding-session-creation-"));
}

/// An empty commit on whatever branch `dir` has checked out.
fn scratch_commit(dir: &Path, message: &str) -> Result<(), String> {
    let auth = build_local_git_auth_config()?;
    run_git(
        &[
            "-c",
            "user.name=Beekeeper Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            message,
        ],
        Some(dir),
        &auth,
    )?;
    Ok(())
}

/// The commit a ref points at.
fn rev_parse(dir: &Path, rev: &str) -> String {
    let auth = build_local_git_auth_config().expect("git");
    run_git(&["rev-parse", rev], Some(dir), &auth)
        .expect("rev-parse")
        .trim()
        .to_string()
}

/// Spec § 4.10: a seat's worktree does not materialise the project's team
/// definitions, the person's checkout keeps them, the objects stay readable
/// through git, and a worktree cut without the flag keeps everything.
#[test]
fn a_worktree_starts_from_main_even_when_the_checkout_is_parked_elsewhere() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(&checkout).expect("create checkout");
    if scratch_repo(&checkout).is_err() {
        return;
    }
    let auth = build_local_git_auth_config().expect("git");
    // A topic branch at the root commit, then `main` moves past it, then the
    // checkout parks on the topic branch — exactly a stale session worktree.
    run_git(&["branch", "old-topic"], Some(&checkout), &auth).expect("branch");
    scratch_commit(&checkout, "main moved on").expect("commit");
    run_git(&["checkout", "old-topic"], Some(&checkout), &auth).expect("checkout");
    let checkout_str = checkout.to_string_lossy().into_owned();

    let listed = list_branches(&checkout_str).expect("list");
    assert!(listed.branches.iter().any(|branch| branch == "main"));
    assert!(listed.branches.iter().any(|branch| branch == "old-topic"));
    assert_eq!(listed.default_branch.as_deref(), Some("main"));
    assert_eq!(listed.head_branch.as_deref(), Some("old-topic"));

    // Create with no explicit source, the way the dialog's default submits.
    let created = create(&checkout_str, "fresh session", None, None).expect("create");
    assert_eq!(
        rev_parse(Path::new(&created.path), "HEAD"),
        rev_parse(&checkout, "main"),
        "the worktree must start at main's tip, not the parked topic branch"
    );

    // An explicit source is honored too.
    let from_topic = create(&checkout_str, "topic followup", None, Some("old-topic"))
        .expect("create from topic");
    assert_eq!(
        rev_parse(Path::new(&from_topic.path), "HEAD"),
        rev_parse(&checkout, "old-topic"),
    );

    // A source that does not exist is one sentence, not a git error.
    let missing = plan(&checkout_str, "ghost", None, Some("no-such-branch")).expect("plan");
    assert_eq!(
        missing.problem.as_deref(),
        Some("That repository has no branch named \"no-such-branch\".")
    );
}

/// A repository with neither `main` nor `master` keeps the old behavior:
/// the worktree branches from the checkout's `HEAD`.
#[test]
fn a_repository_without_a_trunk_falls_back_to_head() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(&checkout).expect("create checkout");
    let auth = match build_local_git_auth_config() {
        Ok(auth) => auth,
        Err(_) => return,
    };
    if run_git(
        &["init", "--initial-branch=trunk", "."],
        Some(&checkout),
        &auth,
    )
    .is_err()
    {
        return;
    }
    scratch_commit(&checkout, "root").expect("commit");
    let checkout_str = checkout.to_string_lossy().into_owned();

    let listed = list_branches(&checkout_str).expect("list");
    assert_eq!(listed.branches, vec!["trunk".to_string()]);
    assert_eq!(listed.default_branch, None);

    let planned = plan(&checkout_str, "no trunk name", None, None).expect("plan");
    assert_eq!(planned.problem, None);
    assert_eq!(planned.source, None, "no trunk means HEAD, and it says so");
}

/// Somewhere that is not a git checkout has no branches and no default —
/// an empty answer, not an error.
#[test]
fn listing_branches_outside_a_repository_is_empty() {
    let root = tempfile::tempdir().expect("tempdir");
    let listed = list_branches(&root.path().to_string_lossy()).expect("list");
    assert!(listed.branches.is_empty());
    assert_eq!(listed.default_branch, None);
    assert_eq!(listed.head_branch, None);
}

/// A folder a person names wins over both defaults — but only after being
/// checked, because recording a worktree there hands the prune path a licence
/// over that folder's contents.
#[test]
fn a_chosen_folder_is_used_when_it_is_allowed_and_refused_with_a_sentence_when_not() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(&checkout).expect("create checkout");
    if scratch_repo(&checkout).is_err() {
        return;
    }
    let checkout_str = checkout.to_string_lossy().into_owned();

    // Canonicalized on the expectation side too: git answers in canonical
    // paths (`/private/var` on macOS) and so, deliberately, does the plan.
    let elsewhere = root.path().join("trees");
    std::fs::create_dir_all(&elsewhere).expect("create chosen folder");
    let elsewhere_canonical = elsewhere.canonicalize().expect("canonicalize");
    let planned = plan(
        &checkout_str,
        "fix the timeout",
        Some(&elsewhere.to_string_lossy()),
        None,
    )
    .expect("plan");
    assert_eq!(planned.parent_problem, None);
    assert_eq!(planned.placement.as_deref(), Some("chosen"));
    assert_eq!(
        planned.path.as_deref().map(Path::new),
        Some(elsewhere_canonical.join("fix-the-timeout").as_path())
    );

    // Inside the repository and not ignored: every session would then show up
    // in `git status`, which is the reason placement is outside the repo.
    let inside = checkout.join("trees");
    let refused = plan(
        &checkout_str,
        "fix the timeout",
        Some(&inside.to_string_lossy()),
        None,
    )
    .expect("plan");
    assert!(
        refused
            .parent_problem
            .as_deref()
            .is_some_and(|why| why.contains("git status")),
        "expected a sentence about git status, got {:?}",
        refused.parent_problem
    );
    assert_eq!(refused.path, None, "a refused folder plans nothing");

    // A folder containing the repository would swallow the checkout.
    let swallowing = plan(
        &checkout_str,
        "fix the timeout",
        Some(&root.path().to_string_lossy()),
        None,
    )
    .expect("plan");
    assert!(swallowing.parent_problem.is_some());
}

/// The predicate the late record relies on, exercised over the two shapes it
/// must separate: a directory this host cut, and an ordinary checkout that
/// merely happens to be a repository. Recording the latter would hand the
/// prune path a licence over somebody's actual working copy.
#[test]
fn a_late_record_admits_a_cut_worktree_and_refuses_a_plain_checkout() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(&checkout).expect("create checkout");
    if scratch_repo(&checkout).is_err() {
        return;
    }
    let created = create(&checkout.to_string_lossy(), "fix the timeout", None, None)
        .expect("create the worktree");

    let repo_root = Path::new(&created.repo_root);
    assert!(
        buzz_core_pkg::worktree_placement::is_managed_worktree_path(
            repo_root,
            Path::new(&created.path),
            &[]
        ),
        "a tree this host just cut must be recordable"
    );
    assert!(
        !buzz_core_pkg::worktree_placement::is_managed_worktree_path(repo_root, repo_root, &[]),
        "the checkout the worktree came from must never be"
    );
}

/// A repository that has opted in — an existing `.worktrees` directory that
/// git ignores — keeps its worktrees inside itself rather than scattering
/// siblings across its parent.
#[test]
fn an_existing_ignored_holder_wins_over_the_sibling() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(checkout.join(".worktrees")).expect("create holder");
    if scratch_repo(&checkout).is_err() {
        return;
    }
    std::fs::write(checkout.join(".gitignore"), ".worktrees/\n").expect("write gitignore");

    let planned = plan(&checkout.to_string_lossy(), "fix the timeout", None, None).expect("plan");
    assert_eq!(planned.problem, None);
    assert_eq!(planned.placement.as_deref(), Some("in-repo-holder"));
    assert_eq!(
        planned.path.as_deref().map(Path::new),
        Some(
            checkout
                .canonicalize()
                .expect("canonicalize")
                .join(".worktrees")
                .join("fix-the-timeout")
                .as_path()
        )
    );
}

/// The bug this resolution fix exists for: a *linked worktree* is not its own
/// repository. Asking `--show-toplevel` from inside one names the worktree,
/// so a second worktree home gets built beside it — which is exactly what
/// happened on the machine this was found on, leaving two `.worktrees`
/// directories for one repository. The sibling of
/// `a_directory_inside_a_repository_plans_against_the_repository_root`, which
/// only ever covered a subdirectory.
#[test]
fn a_linked_worktree_plans_against_its_main_worktree() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    std::fs::create_dir_all(&checkout).expect("create checkout");
    if scratch_repo(&checkout).is_err() {
        return;
    }
    let auth = build_local_git_auth_config().expect("auth");
    let linked = root.path().join("beekeeper-relayver");
    run_git(
        &[
            "worktree",
            "add",
            "-b",
            "relayver",
            &linked.to_string_lossy(),
            "main",
        ],
        Some(&checkout),
        &auth,
    )
    .expect("git proved usable above, so a failure here is this test being wrong");

    let planned = plan(&linked.to_string_lossy(), "fix the timeout", None, None).expect("plan");
    assert_eq!(planned.problem, None);
    let repo_root = planned.repo_root.as_deref().map(Path::new);
    assert_eq!(
        repo_root,
        Some(checkout.canonicalize().expect("canonicalize").as_path()),
        "a linked worktree must resolve to the repository, never to itself"
    );
    let path = planned.path.as_deref().unwrap_or_default();
    assert!(
        !path.contains("beekeeper-relayver."),
        "must not build a worktree home beside the worktree: {path}"
    );
}

/// A bare repository is a perfectly good worktree host — arguably the best
/// one, since it has no checkout to collide with. It used to be refused with
/// "not a git checkout", because `--show-toplevel` fails where there is no
/// work tree.
///
/// Asserts the resolution itself rather than a whole plan: building a bare
/// repository by cloning would need the file transport this auth config
/// deliberately forbids, and `git init --bare` leaves no branch to plan from.
/// Resolution is the thing the fix changed, so it is the thing to pin.
#[test]
fn a_bare_repository_resolves_to_itself_rather_than_being_refused() {
    let root = tempfile::tempdir().expect("tempdir");
    let bare = root.path().join("beekeeper.git");
    std::fs::create_dir_all(&bare).expect("create bare dir");
    let auth = build_local_git_auth_config().expect("auth");
    if run_git(&["init", "--bare", "."], Some(&bare), &auth).is_err() {
        return;
    }

    let resolved = resolve_repo(&bare)
        .expect("resolution must not error")
        .expect("a bare repository is a repository");
    assert_eq!(
        resolved.root.canonicalize().expect("canonicalize"),
        bare.canonicalize().expect("canonicalize"),
        "a bare repo's root is the folder holding its git dir"
    );
    assert!(resolved.bare, "and it must know that it is bare");
}

#[test]
fn a_directory_inside_a_repository_plans_against_the_repository_root() {
    let root = tempfile::tempdir().expect("tempdir");
    let checkout = root.path().join("beekeeper");
    let nested = checkout.join("crates").join("buzz-core");
    std::fs::create_dir_all(&nested).expect("create nested");
    if scratch_repo(&checkout).is_err() {
        return;
    }

    let planned = plan(&nested.to_string_lossy(), "nested start", None, None).expect("plan");
    assert_eq!(planned.problem, None);
    // Not `crates/buzz-core-wt-…` — the worktree belongs to the repository,
    // so it is named and placed from the repository's own folder.
    assert_eq!(
        planned.path.as_deref().map(Path::new),
        Some(
            root.path()
                .canonicalize()
                .expect("canonical root")
                .join("beekeeper-wt-nested-start")
                .as_path()
        )
    );
}
