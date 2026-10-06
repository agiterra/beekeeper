use super::*;

fn p(path: &str) -> PathBuf {
    PathBuf::from(path)
}

const REPO: &str = "/Users/andy/Code/beekeeper";

#[test]
fn a_common_dir_named_dot_git_names_the_folder_around_it() {
    assert_eq!(
        repo_root_from_common_dir(&p("/Users/andy/Code/beekeeper/.git")),
        p("/Users/andy/Code/beekeeper")
    );
}

#[test]
fn a_conventional_bare_clone_is_its_own_root() {
    // `proj.git` is a directory name, not the `.git` component, so stripping
    // it would name a sibling that does not exist.
    assert_eq!(
        repo_root_from_common_dir(&p("/Users/andy/Code/proj.git")),
        p("/Users/andy/Code/proj.git")
    );
}

#[test]
fn a_stem_drops_a_dot_git_suffix_so_siblings_stay_readable() {
    assert_eq!(repo_stem(&p(REPO)), Some("beekeeper"));
    assert_eq!(repo_stem(&p("/Users/andy/Code/proj.git")), Some("proj"));
    assert_eq!(repo_stem(&p("/")), None);
}

#[test]
fn a_sibling_is_named_stem_wt_slug_beside_the_repository() {
    let parent = default_worktree_parent(&p(REPO), false).expect("a repository with a parent");
    assert_eq!(parent.kind(), "sibling");
    assert_eq!(
        parent.path_for("fix-the-timeout"),
        p("/Users/andy/Code/beekeeper-wt-fix-the-timeout")
    );
}

#[test]
fn an_ignored_holder_wins_over_the_sibling() {
    let parent = default_worktree_parent(&p(REPO), true).expect("a holder");
    assert_eq!(parent.kind(), "in-repo-holder");
    assert_eq!(
        parent.path_for("fix-the-timeout"),
        p("/Users/andy/Code/beekeeper/.worktrees/fix-the-timeout")
    );
}

#[test]
fn a_repository_at_the_filesystem_root_has_nowhere_to_put_a_sibling() {
    assert_eq!(default_worktree_parent(&p("/"), false), None);
}

#[test]
fn the_in_repo_holder_admits_what_lies_under_it_but_not_itself() {
    let repo = p(REPO);
    assert!(is_managed_worktree_path(
        &repo,
        &p("/Users/andy/Code/beekeeper/.worktrees/one"),
        &[]
    ));
    assert!(
        !is_managed_worktree_path(&repo, &p("/Users/andy/Code/beekeeper/.worktrees"), &[]),
        "the holder itself is not a worktree"
    );
    assert!(
        is_managed_worktree_path(
            &repo,
            &p("/Users/andy/Code/beekeeper/.worktrees/one/deeper"),
            &[]
        ),
        "worktree names contain slashes here (lane/batch3-…), so a tree may sit \
         below the holder — see worktree_prune_tests.rs"
    );
}

#[test]
fn the_legacy_holder_stays_admissible_but_is_never_chosen() {
    let repo = p(REPO);
    // Trees cut before this module existed must stay recordable and
    // removable, or the work inside them becomes unmanageable.
    assert!(is_managed_worktree_path(
        &repo,
        &p("/Users/andy/Code/beekeeper.worktrees/old-one"),
        &[]
    ));
    // ...and yet nothing ever plans there again.
    for ignored in [true, false] {
        let chosen = default_worktree_parent(&repo, ignored).expect("a parent");
        assert_ne!(
            chosen.path_for("x").parent(),
            Some(p("/Users/andy/Code/beekeeper.worktrees").as_path()),
            "the legacy holder must never be planned into (ignored={ignored})"
        );
    }
}

#[test]
fn a_sibling_decoy_without_the_separator_is_refused() {
    let repo = p(REPO);
    assert!(is_managed_worktree_path(
        &repo,
        &p("/Users/andy/Code/beekeeper-wt-one"),
        &[]
    ));
    for decoy in [
        "/Users/andy/Code/beekeeper-wtf",      // no separator
        "/Users/andy/Code/beekeeper-wt-",      // empty tail
        "/Users/andy/Code/beekeeperwt-one",    // no hyphen
        "/Users/andy/Code/other-wt-one",       // another repository's sibling
        "/Users/andy/beekeeper-wt-one",        // right name, wrong parent
        "/Users/andy/Code/beekeeper-wt-a/sub", // a child of a sibling
    ] {
        assert!(
            !is_managed_worktree_path(&repo, &p(decoy), &[]),
            "must be refused: {decoy}"
        );
    }
}

#[test]
fn the_repository_itself_is_never_a_managed_worktree() {
    let repo = p(REPO);
    assert!(!is_managed_worktree_path(&repo, &repo, &[]));
    assert!(!is_managed_worktree_path(
        &repo,
        &p("/Users/andy/Code"),
        &[]
    ));
}

#[test]
fn a_relative_path_is_refused_rather_than_resolved() {
    // Resolution needs a cwd this module deliberately does not have; guessing
    // one would grant a licence over somewhere unintended.
    assert!(!is_managed_worktree_path(
        &p(REPO),
        &p("beekeeper-wt-one"),
        &[]
    ));
    assert!(!is_managed_worktree_path(
        &p("Code/beekeeper"),
        &p("/Users/andy/Code/beekeeper-wt-one"),
        &[]
    ));
}

#[test]
fn a_chosen_folder_admits_what_lies_under_it() {
    let repo = p(REPO);
    let chosen = vec![p("/Volumes/scratch/trees")];
    assert!(is_managed_worktree_path(
        &repo,
        &p("/Volumes/scratch/trees/one"),
        &chosen
    ));
    assert!(is_managed_worktree_path(
        &repo,
        &p("/Volumes/scratch/trees/one/deeper"),
        &chosen
    ));
    assert!(!is_managed_worktree_path(
        &repo,
        &p("/Volumes/scratch/elsewhere/one"),
        &chosen
    ));
}

#[test]
fn a_chosen_folder_containing_the_repository_is_refused() {
    let repo = p(REPO);
    for bad in ["/Users/andy/Code", "/Users/andy", "/", REPO] {
        assert!(
            chosen_parent_refusal(&repo, &p(bad), None, None, None).is_some(),
            "must be refused: {bad}"
        );
    }
}

#[test]
fn a_chosen_folder_inside_an_unignored_repository_is_refused() {
    let repo = p(REPO);
    let inside = p("/Users/andy/Code/beekeeper/trees");
    assert!(
        chosen_parent_refusal(&repo, &inside, Some(false), None, None)
            .is_some_and(|why| why.contains("git status")),
        "an unignored folder inside the repository would dirty every status"
    );
    assert_eq!(
        chosen_parent_refusal(&repo, &inside, Some(true), None, None),
        None,
        "ignored, it is fine"
    );
}

#[test]
fn the_home_directory_itself_is_refused_but_its_children_are_not() {
    let repo = p(REPO);
    let home = p("/Users/andy");
    assert!(chosen_parent_refusal(&repo, &home, None, Some(&home), None).is_some());
    assert_eq!(
        chosen_parent_refusal(&repo, &p("/Users/andy/trees"), None, Some(&home), None),
        None
    );
}

#[test]
fn a_folder_containing_this_apps_own_cwd_is_refused() {
    let repo = p(REPO);
    let cwd = p("/Users/andy/Code/beekeeper-relayver");
    assert!(chosen_parent_refusal(&repo, &p("/Users/andy/Code"), None, None, Some(&cwd)).is_some());
    assert_eq!(
        chosen_parent_refusal(&repo, &p("/Volumes/scratch"), None, None, Some(&cwd)),
        None
    );
}

#[test]
fn a_relative_chosen_folder_is_refused_before_anything_else() {
    assert!(
        chosen_parent_refusal(&p(REPO), &p("trees"), None, None, None)
            .is_some_and(|why| why.contains("absolute"))
    );
}
