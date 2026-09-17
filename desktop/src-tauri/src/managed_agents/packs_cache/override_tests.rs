//! Tests for spec § 4.9: the branch override and definition drift.
//!
//! Split out of `packs_cache/tests.rs` to keep both files under the
//! repository's 1000-line ceiling; the scratch-repository helpers stay in
//! `tests.rs` and are shared through `pub(super)`.

use std::path::{Path, PathBuf};

use super::definition_drift::definition_drift_against;
use super::tests::{
    git, packs_repo, persona_body, scratch_root, source, write, write_role_pack, ScratchRepo, REPO,
};
use super::*;
use crate::commands::project_git_exec::build_test_git_auth_config;

// ── Spec § 4.9: the branch override ──────────────────────────────────────────

/// A packs repository on `main` plus a worktree of a clone of it, parked on
/// a topic branch. Returns the origin, the worktree, the `main` sha and the
/// relay base whose `packs_clone_url` equals the clone's origin URL.
fn repo_and_worktree(root: &Path) -> (ScratchRepo, PathBuf, String, String) {
    let (origin, _first, second) = packs_repo(root);
    let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
    // The clone's origin URL must be exactly what `packs_clone_url` derives
    // from the relay base, so the check recognises the same repository.
    let relay_base = origin
        .dir
        .parent()
        .expect("parent")
        .to_string_lossy()
        .to_string();
    let clone_url = packs_clone_url(&relay_base, &owner, &id);
    let git_dir = root.join("git");
    std::fs::create_dir_all(git_dir.join(&owner)).expect("git dir");
    // `packs_clone_url` is `<base>/git/<owner>/<id>`; make that path exist as
    // the origin by moving the repository there.
    let target = git_dir.join(&owner).join(&id);
    std::fs::rename(&origin.dir, &target).expect("relocate origin");
    let origin = ScratchRepo { dir: target };
    assert_eq!(clone_url, format!("{relay_base}/git/{owner}/{id}"));
    let worktree = root.join("seat-tree");
    git(
        &[
            "clone",
            "--quiet",
            &origin.dir.to_string_lossy(),
            &worktree.to_string_lossy(),
        ],
        root,
    );
    git(&["config", "user.email", "l23b@test.invalid"], &worktree);
    git(&["config", "user.name", "L23B"], &worktree);
    git(&["checkout", "--quiet", "-b", "topic"], &worktree);
    (origin, worktree, second, relay_base)
}

/// `main`'s composition of the builder, the way `stage_project_role_pack`
/// produces it — assembled from its pieces here because that function
/// validates the clone URL as a relay URL, and a scratch origin is a path.
fn main_composition(
    packs_root: &Path,
    relay_base: &str,
    main_sha: &str,
) -> (ProjectPackSource, StagedProjectPack) {
    let auth = build_test_git_auth_config().expect("git auth");
    let source = source(REPO, None, Some(main_sha));
    let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
    let checkout = packs_checkout_dir(packs_root, &owner, &id);
    let clone_url = packs_clone_url(relay_base, &owner, &id);
    let sha = sync_packs_checkout(&checkout, &clone_url, &source, &auth).expect("sync main");
    let role_source =
        locate_role_source(&checkout, DEFAULT_PACK_PATH, "builder").expect("builder on main");
    let ref_path = pack_ref_path(&role_source, DEFAULT_PACK_PATH);
    let staged = stage_composed_pack(
        packs_root,
        &format!("{}-{sha}", pack_cache_dir_name(&owner, &id)),
        &role_source,
        &TemplateCatalog::empty("0.5.16"),
        SourceProvenance {
            kind: "repository".into(),
            repo: Some(REPO.into()),
            sha: Some(sha.clone()),
            path: ref_path.clone(),
        },
    )
    .expect("main composition");
    (
        source,
        StagedProjectPack {
            pack_ref: PackRef {
                repo: REPO.into(),
                sha,
                role: "builder".into(),
                path: ref_path,
            },
            dir: staged.dir,
            persona: staged.persona,
            digest: staged.digest,
            warnings: staged.warnings,
            roles_visible: staged.roles_visible,
        },
    )
}

#[test]
fn a_worktree_on_mains_commit_or_with_an_unrelated_change_does_not_override() {
    let root = scratch_root();
    let (_origin, worktree, main_sha, relay_base) = repo_and_worktree(&root);
    let packs_root = root.join("cache");
    let auth = build_test_git_auth_config().expect("git auth");
    let catalog = TemplateCatalog::empty("0.5.16");
    let (source, main) = main_composition(&packs_root, &relay_base, &main_sha);

    let same = branch_role_override(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &worktree,
        &main.pack_ref.sha,
        &main.digest,
        &auth,
        &catalog,
    )
    .expect("check");
    assert_eq!(same.decision, BranchOverride::SameCommit);
    assert!(!same.dirty);

    // A commit that changes something the builder does not read: the branch
    // composes to the same bytes, so main's definition stays in effect.
    write(&worktree.join("README.md"), "unrelated\n");
    git(&["add", "--all"], &worktree);
    git(&["commit", "--quiet", "-m", "unrelated"], &worktree);
    let unchanged = branch_role_override(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &worktree,
        &main.pack_ref.sha,
        &main.digest,
        &auth,
        &catalog,
    )
    .expect("check");
    assert!(
        matches!(unchanged.decision, BranchOverride::Unchanged { ref sha } if sha != &main_sha),
        "{:?}",
        unchanged.decision
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_committed_change_to_the_role_overrides_and_an_uncommitted_one_is_only_disclosed() {
    let root = scratch_root();
    let (_origin, worktree, main_sha, relay_base) = repo_and_worktree(&root);
    let packs_root = root.join("cache");
    let auth = build_test_git_auth_config().expect("git auth");
    let catalog = TemplateCatalog::empty("0.5.16");
    let (source, main) = main_composition(&packs_root, &relay_base, &main_sha);

    // Uncommitted: disclosed, not in effect.
    write_role_pack(&worktree, "builder", "You build, on the topic branch.");
    let dirty = branch_role_override(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &worktree,
        &main.pack_ref.sha,
        &main.digest,
        &auth,
        &catalog,
    )
    .expect("check");
    assert!(dirty.dirty, "uncommitted edits under the path are reported");
    assert_eq!(
        dirty.decision,
        BranchOverride::SameCommit,
        "…and never in effect"
    );

    // Committed: the branch's definition is the seat's.
    git(&["add", "--all"], &worktree);
    git(
        &["commit", "--quiet", "-m", "builder evolves on topic"],
        &worktree,
    );
    let topic_sha = git(&["rev-parse", "HEAD"], &worktree).trim().to_string();
    let overridden = branch_role_override(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &worktree,
        &main.pack_ref.sha,
        &main.digest,
        &auth,
        &catalog,
    )
    .expect("check");
    assert!(!overridden.dirty);
    let BranchOverride::Overridden {
        sha,
        staged,
        pack_ref_path,
    } = overridden.decision
    else {
        panic!("expected an override, got {:?}", overridden.decision);
    };
    assert_eq!(sha, topic_sha);
    assert_eq!(pack_ref_path, "personas/roles/builder");
    assert_ne!(staged.digest, main.digest);
    assert!(staged.dir.starts_with(staged_packs_root(&packs_root)));
    let resolved = buzz_persona_pkg::resolve::resolve_persona_by_name(&staged.dir, "builder")
        .expect("the branch composition is a pack");
    assert_eq!(resolved.system_prompt, "You build, on the topic branch.\n");
    let provenance: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(staged.dir.join("compose.json")).expect("compose.json"),
    )
    .expect("json");
    assert_eq!(provenance["source"]["kind"], BRANCH_OVERRIDE_KIND);
    assert_eq!(provenance["source"]["sha"], topic_sha);
    // The branch tree was read from git objects, not the working copy: the
    // materialized copy carries the committed bytes and a marker.
    let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
    let tree = super::branch_override::branch_tree_dir(&packs_root, &owner, &id, &topic_sha);
    assert!(tree.join(".materialized").is_file());
    assert!(persona_body(&tree, "builder").contains("on the topic branch"));

    // A role the branch does not hold is said so, not guessed (`runner`
    // exists on the fixture\'s second commit; `poker` never does).
    let absent = branch_role_override(
        &packs_root,
        &relay_base,
        &source,
        "poker",
        &worktree,
        &main.pack_ref.sha,
        &main.digest,
        &auth,
        &catalog,
    )
    .expect("check");
    assert!(matches!(absent.decision, BranchOverride::RoleAbsent { .. }));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_worktree_of_another_repository_never_overrides() {
    let root = scratch_root();
    let (_origin, _worktree, main_sha, relay_base) = repo_and_worktree(&root);
    let packs_root = root.join("cache");
    let auth = build_test_git_auth_config().expect("git auth");
    let (source, main) = main_composition(&packs_root, &relay_base, &main_sha);
    // Some other repository, cloned from somewhere else entirely.
    let other_origin = root.join("elsewhere");
    std::fs::create_dir_all(&other_origin).expect("dir");
    git(
        &["init", "--quiet", "--initial-branch", "main", "."],
        &other_origin,
    );
    git(
        &["config", "user.email", "l23b@test.invalid"],
        &other_origin,
    );
    git(&["config", "user.name", "L23B"], &other_origin);
    write_role_pack(&other_origin, "builder", "Somebody else's builder.");
    git(&["add", "--all"], &other_origin);
    git(&["commit", "--quiet", "-m", "other"], &other_origin);
    let other_tree = root.join("other-tree");
    git(
        &[
            "clone",
            "--quiet",
            &other_origin.to_string_lossy(),
            &other_tree.to_string_lossy(),
        ],
        &root,
    );
    let check = branch_role_override(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &other_tree,
        &main.pack_ref.sha,
        &main.digest,
        &auth,
        &TemplateCatalog::empty("0.5.16"),
    )
    .expect("check");
    assert!(
        matches!(check.decision, BranchOverride::OtherRepository { ref origin } if origin.contains("elsewhere")),
        "{:?}",
        check.decision
    );
    std::fs::remove_dir_all(&root).ok();
}

// ── Spec § 4.9: definition drift ─────────────────────────────────────────────

#[test]
fn drift_is_current_when_the_seat_runs_the_current_commit_or_an_unchanged_role() {
    let root = scratch_root();
    let (origin, _worktree, second, relay_base) = repo_and_worktree(&root);
    let packs_root = root.join("cache");
    let auth = build_test_git_auth_config().expect("git auth");
    let catalog = TemplateCatalog::empty("0.5.16");
    let (source, current) = main_composition(&packs_root, &relay_base, &second);

    let same = definition_drift_against(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &second,
        None,
        &current,
        &auth,
        &catalog,
    );
    assert_eq!(same.state, DefinitionDriftState::Current);
    assert_eq!(same.current_source_kind.as_deref(), Some("repository"));
    assert_eq!(same.seat_digest, same.current_digest);
    assert!(same.reason.is_none());

    // main moves by a commit that does not touch the architect: a seat staged
    // from the older commit is still current, and the cause says the source
    // moved while the role did not.
    let first = git(&["rev-parse", "HEAD~1"], &origin.dir)
        .trim()
        .to_string();
    let (source_arch, current_arch) = {
        let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
        let checkout = packs_checkout_dir(&packs_root, &owner, &id);
        let role_source =
            locate_role_source(&checkout, DEFAULT_PACK_PATH, "architect").expect("architect");
        let staged = stage_composed_pack(
            &packs_root,
            &format!("{}-{second}", pack_cache_dir_name(&owner, &id)),
            &role_source,
            &catalog,
            SourceProvenance::local("personas/roles/architect"),
        )
        .expect("architect now");
        (
            source.clone(),
            StagedProjectPack {
                pack_ref: PackRef {
                    repo: REPO.into(),
                    sha: second.clone(),
                    role: "architect".into(),
                    path: "personas/roles/architect".into(),
                },
                dir: staged.dir,
                persona: staged.persona,
                digest: staged.digest,
                warnings: staged.warnings,
                roles_visible: staged.roles_visible,
            },
        )
    };
    let unchanged = definition_drift_against(
        &packs_root,
        &relay_base,
        &source_arch,
        "architect",
        &first,
        None,
        &current_arch,
        &auth,
        &catalog,
    );
    assert_eq!(
        unchanged.state,
        DefinitionDriftState::Current,
        "{unchanged:?}"
    );
    assert!(unchanged.cause.contains("unchanged"), "{}", unchanged.cause);
    assert_ne!(unchanged.seat_sha, unchanged.current_sha.clone().unwrap());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn drift_is_changed_when_main_changed_the_role_and_unknown_when_the_commit_is_not_here() {
    let root = scratch_root();
    let (origin, _worktree, second, relay_base) = repo_and_worktree(&root);
    let packs_root = root.join("cache");
    let auth = build_test_git_auth_config().expect("git auth");
    let catalog = TemplateCatalog::empty("0.5.16");
    let (source, current) = main_composition(&packs_root, &relay_base, &second);
    // The fixture's first commit has builder v1; the second changed it.
    let first = git(&["rev-parse", "HEAD~1"], &origin.dir)
        .trim()
        .to_string();
    let changed = definition_drift_against(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &first,
        None,
        &current,
        &auth,
        &catalog,
    );
    assert_eq!(changed.state, DefinitionDriftState::Changed, "{changed:?}");
    assert!(changed.cause.contains("main moved"), "{}", changed.cause);
    assert!(
        changed.cause.contains("changed the builder definition"),
        "{}",
        changed.cause
    );
    assert_ne!(changed.seat_digest, changed.current_digest);
    assert_eq!(changed.current_sha.as_deref(), Some(second.as_str()));

    let unknown = definition_drift_against(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &"f".repeat(40),
        None,
        &current,
        &auth,
        &catalog,
    );
    assert_eq!(unknown.state, DefinitionDriftState::Unknown);
    assert!(
        unknown
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("does not hold commit")),
        "{unknown:?}"
    );
    assert!(unknown.current_sha.is_none(), "unknown claims nothing");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn drift_counts_the_seats_branch_override_as_now() {
    let root = scratch_root();
    let (_origin, worktree, second, relay_base) = repo_and_worktree(&root);
    let packs_root = root.join("cache");
    let auth = build_test_git_auth_config().expect("git auth");
    let catalog = TemplateCatalog::empty("0.5.16");
    let (source, current) = main_composition(&packs_root, &relay_base, &second);
    write_role_pack(&worktree, "builder", "You build, on the topic branch.");
    git(&["add", "--all"], &worktree);
    git(
        &["commit", "--quiet", "-m", "builder evolves on topic"],
        &worktree,
    );
    let topic = git(&["rev-parse", "HEAD"], &worktree).trim().to_string();

    // A seat staged from main, whose tree has since committed a change to its
    // role: "now" is the branch, and the seat has drifted from it.
    let drifted = definition_drift_against(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &second,
        Some(&worktree),
        &current,
        &auth,
        &catalog,
    );
    assert_eq!(drifted.state, DefinitionDriftState::Changed, "{drifted:?}");
    assert_eq!(
        drifted.current_source_kind.as_deref(),
        Some(BRANCH_OVERRIDE_KIND)
    );
    assert_eq!(drifted.current_sha.as_deref(), Some(topic.as_str()));
    assert!(
        drifted.cause.contains("this seat's branch changed"),
        "{}",
        drifted.cause
    );

    // A seat staged from that branch commit is current.
    let current_on_branch = definition_drift_against(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &topic,
        Some(&worktree),
        &current,
        &auth,
        &catalog,
    );
    assert_eq!(
        current_on_branch.state,
        DefinitionDriftState::Current,
        "{current_on_branch:?}"
    );

    // Uncommitted edits on top are disclosed, not counted.
    write_role_pack(&worktree, "builder", "You build, uncommitted.");
    let dirty = definition_drift_against(
        &packs_root,
        &relay_base,
        &source,
        "builder",
        &topic,
        Some(&worktree),
        &current,
        &auth,
        &catalog,
    );
    assert_eq!(dirty.state, DefinitionDriftState::Current);
    assert!(
        dirty.warnings.iter().any(|w| w == UNCOMMITTED_ROLE_EDITS),
        "{:?}",
        dirty.warnings
    );
    std::fs::remove_dir_all(&root).ok();
}
