//! Tests for the packs cache: syncing, locating, composing and staging.
//!
//! Split out of `packs_cache.rs` to keep both files under the repository's
//! 1000-line ceiling; `use super::*` keeps every claim against the same
//! module it was written for.

use super::*;
use crate::commands::project_git_exec::build_test_git_auth_config;

/// A throwaway git repository under this worktree's scratch directory.
///
/// Never a worktree of this repository: a test that runs `git` inside the
/// checkout it was launched from will one day write to it.
struct ScratchRepo {
    dir: PathBuf,
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn scratch_root() -> PathBuf {
    // Two tests starting in the same instant collided on pid + nanoseconds
    // once enough of them ran in parallel; a counter makes the name unique.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("l23b-scratch")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&root).expect("scratch root");
    root
}

fn git(args: &[&str], cwd: &Path) -> String {
    let auth = build_test_git_auth_config().expect("git auth");
    run_git(args, Some(cwd), &auth).unwrap_or_else(|error| panic!("git {args:?}: {error}"))
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, contents).expect("write");
}

/// Write one role pack — `.plugin/plugin.json` plus a persona declaring
/// the role — under `<root>/personas/roles/<role>`.
fn write_role_pack(root: &Path, role: &str, body: &str) {
    let pack = root.join(DEFAULT_PACK_PATH).join(role);
    write(
        &pack.join(".plugin/plugin.json"),
        &format!(
            r#"{{"id":"com.test.{role}","name":"{role}","version":"0.1.0","personas":["personas/{role}.persona.md"]}}"#
        ),
    );
    write(
        &pack.join(format!("personas/{role}.persona.md")),
        &format!(
            "---\nname: {role}\ndisplay_name: {role}\ndescription: The {role}.\nrole: {role}\n---\n{body}\n"
        ),
    );
}

/// A packs repository with two roles on `main`, plus a second commit that
/// changes the builder and adds a runner.
fn packs_repo(root: &Path) -> (ScratchRepo, String, String) {
    let dir = root.join("packs-origin");
    std::fs::create_dir_all(&dir).expect("origin dir");
    git(&["init", "--quiet", "--initial-branch", "main", "."], &dir);
    assert!(
        dir.join(".git").is_dir(),
        "the throwaway repository must own its own .git before anything is added"
    );
    git(&["config", "user.email", "l23b@test.invalid"], &dir);
    git(&["config", "user.name", "L23B"], &dir);
    write_role_pack(&dir, "builder", "You build. v1");
    write_role_pack(&dir, "architect", "You design. v1");
    git(&["add", "--all"], &dir);
    git(&["commit", "--quiet", "-m", "roles v1"], &dir);
    let first = git(&["rev-parse", "HEAD"], &dir).trim().to_string();
    write_role_pack(&dir, "builder", "You build. v2");
    write_role_pack(&dir, "runner", "You run. v2");
    git(&["add", "--all"], &dir);
    git(&["commit", "--quiet", "-m", "roles v2"], &dir);
    let second = git(&["rev-parse", "HEAD"], &dir).trim().to_string();
    (ScratchRepo { dir }, first, second)
}

fn persona_body(checkout: &Path, role: &str) -> String {
    std::fs::read_to_string(
        checkout
            .join(DEFAULT_PACK_PATH)
            .join(role)
            .join(format!("personas/{role}.persona.md")),
    )
    .expect("persona file")
}

fn source(repo: &str, git_ref: Option<&str>, sha: Option<&str>) -> ProjectPackSource {
    ProjectPackSource {
        repo: repo.to_string(),
        git_ref: git_ref.map(str::to_owned),
        sha: sha.map(str::to_owned),
        path: DEFAULT_PACK_PATH.to_string(),
    }
}

const REPO: &str = "30617:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:packs";

#[test]
fn a_ref_following_source_lands_on_the_branch_tip_and_records_its_sha() {
    let root = scratch_root();
    let (origin, _first, second) = packs_repo(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-packs");
    let url = origin.dir.to_string_lossy().to_string();

    let sha = sync_packs_checkout(
        &checkout,
        &url,
        &source(REPO, Some("refs/heads/main"), None),
        &auth,
    )
    .expect("sync");
    assert_eq!(sha, second, "a ref records the commit it resolved to");
    assert!(persona_body(&checkout, "builder").contains("You build. v2"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_pinned_sha_stages_that_commit_and_not_the_tip() {
    let root = scratch_root();
    let (origin, first, second) = packs_repo(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-packs");
    let url = origin.dir.to_string_lossy().to_string();

    let sha = sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&first)), &auth)
        .expect("sync");
    assert_eq!(sha, first);
    assert_ne!(first, second);
    assert!(
        persona_body(&checkout, "builder").contains("You build. v1"),
        "a pinned commit stages that commit's pack"
    );
    // A role added after the pin is not in the checkout — and no file from
    // a later checkout of the same cache is left behind either.
    assert!(!checkout.join("personas/roles/runner").exists());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_second_sync_moves_an_existing_cache_between_commits() {
    let root = scratch_root();
    let (origin, first, second) = packs_repo(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-packs");
    let url = origin.dir.to_string_lossy().to_string();

    sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&second)), &auth)
        .expect("sync to tip");
    assert!(checkout.join("personas/roles/runner").is_dir());
    sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&first)), &auth)
        .expect("sync back");
    assert!(
        !checkout.join("personas/roles/runner").exists(),
        "a role the pinned commit does not have must not survive the move"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_commit_the_repository_does_not_have_refuses() {
    let root = scratch_root();
    let (origin, _first, _second) = packs_repo(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-packs");
    let url = origin.dir.to_string_lossy().to_string();
    let error = sync_packs_checkout(
        &checkout,
        &url,
        &source(REPO, None, Some(&"9".repeat(40))),
        &auth,
    )
    .expect_err("a commit that is not there is not a pack");
    assert!(error.contains("does not contain commit"), "{error}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_branch_the_repository_does_not_have_refuses() {
    let root = scratch_root();
    let (origin, _first, _second) = packs_repo(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-packs");
    let url = origin.dir.to_string_lossy().to_string();
    let error = sync_packs_checkout(
        &checkout,
        &url,
        &source(REPO, Some("refs/heads/nope"), None),
        &auth,
    )
    .expect_err("a branch that is not there is not a pack");
    assert!(error.contains("no branch nope"), "{error}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_role_the_repository_does_not_hold_is_not_a_pack() {
    let root = scratch_root();
    let (origin, first, _second) = packs_repo(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-packs");
    let url = origin.dir.to_string_lossy().to_string();
    sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&first)), &auth).expect("sync");
    assert!(role_pack_in_checkout(&checkout, DEFAULT_PACK_PATH, "builder").is_some());
    assert!(
        role_pack_in_checkout(&checkout, DEFAULT_PACK_PATH, "runner").is_none(),
        "the role is not in this commit"
    );
    assert!(
        role_pack_in_checkout(&checkout, DEFAULT_PACK_PATH, "../../etc").is_none(),
        "a traversing role is not a role slug"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// The addendum's last fallback, checked against the packs this build
/// actually ships rather than against a fixture: every shipped
/// role directories in `personas/roles` must resolve as that role's pack,
/// or a fresh install hires an architect and gets a bare persona.
#[test]
fn every_shipped_role_pack_resolves_as_its_own_role() {
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate has a repository above it")
        .join(DEFAULT_PACK_PATH);
    assert!(
        shipped.is_dir(),
        "this build ships no {DEFAULT_PACK_PATH}: {}",
        shipped.display()
    );
    let mut found: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&shipped).expect("read shipped packs") {
        let entry = entry.expect("entry");
        if !entry.path().is_dir() {
            continue;
        }
        let role = entry.file_name().to_string_lossy().into_owned();
        assert!(
            role_pack_in_checkout(&shipped, "", &role).is_some(),
            "{role} is a directory in the shipped packs but not a pack declaring that role"
        );
        found.push(role);
    }
    found.sort();
    assert_eq!(
        found,
        vec![
            "architect",
            "builder",
            "designer",
            "lead",
            "poker",
            "project-setup",
            "runner",
            "verifier",
        ],
        "the eight roles the app ships"
    );
}

/// The middle fallback: a project that keeps its packs in the repository
/// being worked on gets them without announcing anything.
#[test]
fn a_session_checkout_can_hold_the_role_pack_itself() {
    let root = scratch_root();
    let checkout = root.join("checkout");
    write_role_pack(&checkout, "builder", "You build, from the checkout.");
    assert_eq!(
        locate_role_source(&checkout, DEFAULT_PACK_PATH, "builder"),
        Some(RoleSource::Pack {
            dir: checkout.join(DEFAULT_PACK_PATH).join("builder"),
            role: "builder".to_owned(),
            persona: Some("builder".to_owned()),
        })
    );
    // A role the checkout does not hold is not the checkout's answer.
    assert!(locate_role_source(&checkout, DEFAULT_PACK_PATH, "runner").is_none());
    // And a directory that is not a pack is not one either.
    std::fs::create_dir_all(checkout.join(DEFAULT_PACK_PATH).join("runner"))
        .expect("empty role dir");
    assert!(locate_role_source(&checkout, DEFAULT_PACK_PATH, "runner").is_none());
    std::fs::remove_dir_all(&root).ok();
}

/// The flat layout (spec § 4.1): `<path>/roles/<role>.md` answers when
/// no pack directory does, and a pack directory outranks it.
#[test]
fn a_flat_role_file_is_located_after_a_pack_directory() {
    let root = scratch_root();
    let checkout = root.join("checkout");
    write(
        &checkout.join(DEFAULT_FLAT_PATH).join("roles/builder.md"),
        "You build, flat.\n",
    );
    assert_eq!(
        locate_role_source(&checkout, DEFAULT_FLAT_PATH, "builder"),
        Some(RoleSource::Flat {
            root: checkout.join(DEFAULT_FLAT_PATH),
            role: "builder".to_owned(),
        })
    );
    assert!(locate_role_source(&checkout, DEFAULT_FLAT_PATH, "runner").is_none());
    assert!(
        locate_role_source(&checkout, DEFAULT_FLAT_PATH, "../builder").is_none(),
        "a traversing role is not a role slug"
    );
    // A pack directory beside the flat file wins, byte for byte the old rule.
    let pack = checkout.join(DEFAULT_FLAT_PATH).join("builder");
    write(
        &pack.join(".plugin/plugin.json"),
        r#"{"id":"com.test.builder","name":"builder","version":"0.1.0","personas":["personas/builder.persona.md"]}"#,
    );
    write(
        &pack.join("personas/builder.persona.md"),
        "---\nname: builder\ndisplay_name: builder\ndescription: The builder.\nrole: builder\n---\nPacked.\n",
    );
    assert!(matches!(
        locate_role_source(&checkout, DEFAULT_FLAT_PATH, "builder"),
        Some(RoleSource::Pack { .. })
    ));
    assert_eq!(
        pack_ref_path(
            &RoleSource::Flat {
                root: checkout.clone(),
                role: "builder".into()
            },
            DEFAULT_FLAT_PATH
        ),
        "beekeeper/roles/builder"
    );
    assert_eq!(
        pack_ref_path(
            &RoleSource::Pack {
                dir: pack,
                role: "builder".into(),
                persona: None
            },
            DEFAULT_PACK_PATH
        ),
        "personas/roles/builder"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// Staging is keyed by the composition's digest: the same inputs land in
/// the same immutable directory and write nothing the second time; a
/// change lands beside it. The staged directory is an ordinary pack the
/// resolver reads, never the source itself.
#[test]
fn staging_a_composed_pack_is_digest_keyed_and_idempotent() {
    let root = scratch_root();
    let packs_root = root.join("packs");
    let checkout = root.join("checkout");
    write_role_pack(&checkout, "builder", "You build.");
    let source = locate_role_source(&checkout, DEFAULT_PACK_PATH, "builder").expect("located");
    let catalog = TemplateCatalog::empty("0.5.16");
    let key = local_source_key(&checkout.join(DEFAULT_PACK_PATH));
    let provenance = SourceProvenance::local("personas/roles/builder");

    let first = stage_composed_pack(&packs_root, &key, &source, &catalog, provenance.clone())
        .expect("staged");
    assert!(first
        .dir
        .starts_with(staged_packs_root(&packs_root).join(&key)));
    assert_eq!(first.persona, "builder");
    assert!(first.digest.starts_with("sha256:"));
    assert!(first.warnings.is_empty());
    assert_ne!(first.dir, checkout.join(DEFAULT_PACK_PATH).join("builder"));
    let resolved = buzz_persona_pkg::resolve::resolve_persona_by_name(&first.dir, "builder")
        .expect("the staged pack is a pack");
    assert_eq!(resolved.system_prompt, "You build.\n");
    assert_eq!(resolved.role.as_deref(), Some("builder"));
    let stamp = std::fs::metadata(first.dir.join("compose.json"))
        .and_then(|m| m.modified())
        .expect("mtime");

    let again = stage_composed_pack(&packs_root, &key, &source, &catalog, provenance.clone())
        .expect("staged again");
    assert_eq!(again.dir, first.dir);
    assert_eq!(again.digest, first.digest);
    assert_eq!(
        std::fs::metadata(first.dir.join("compose.json"))
            .and_then(|m| m.modified())
            .expect("mtime"),
        stamp,
        "an identical composition writes nothing"
    );

    write_role_pack(&checkout, "builder", "You build, revised.");
    let changed = stage_composed_pack(&packs_root, &key, &source, &catalog, provenance)
        .expect("staged changed");
    assert_ne!(
        changed.dir, first.dir,
        "a changed source lands beside the old one"
    );
    assert!(
        first.dir.join("personas/builder.persona.md").is_file(),
        "the old staged copy is untouched"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// A pack that cannot be composed — here, an include of a template this
/// build does not ship — refuses with the composer's reason, and stages
/// nothing.
#[test]
fn an_uncomposable_pack_refuses_and_stages_nothing() {
    let root = scratch_root();
    let packs_root = root.join("packs");
    let checkout = root.join("checkout");
    write_role_pack(
        &checkout,
        "builder",
        "![[beekeeper/memory@^1.0.0]]\nYou build.",
    );
    let source = locate_role_source(&checkout, DEFAULT_PACK_PATH, "builder").expect("located");
    let error = stage_composed_pack(
        &packs_root,
        "k",
        &source,
        &TemplateCatalog::empty("0.5.16"),
        SourceProvenance::local("personas/roles/builder"),
    )
    .expect_err("refused");
    assert!(error.contains("no template catalog"), "{error}");
    assert!(!staged_packs_root(&packs_root).join("k").exists());
    std::fs::remove_dir_all(&root).ok();
}

/// The project rung end to end: a synced repository whose role is a flat
/// file stages a composed pack and stamps the flat `packRef.path`.
#[test]
fn a_project_source_with_a_flat_role_stages_it_and_names_its_path() {
    let root = scratch_root();
    let (origin, _first, _second) = packs_repo(&root);
    write(
        &origin.dir.join("beekeeper/roles/verifier.md"),
        "---\ndescription: Verifies.\n---\nYou verify, flat.\n",
    );
    git(&["add", "--all"], &origin.dir);
    git(&["commit", "--quiet", "-m", "flat verifier"], &origin.dir);
    let third = git(&["rev-parse", "HEAD"], &origin.dir).trim().to_string();
    let auth = build_test_git_auth_config().expect("git auth");
    let packs_root = root.join("cache");
    let url = origin.dir.to_string_lossy().to_string();
    let mut project = source(REPO, None, Some(&third));
    project.path = DEFAULT_FLAT_PATH.to_string();
    // `stage_project_role_pack` builds the clone URL from the relay base;
    // hand it the origin's parent so `<base>/git/<owner>/<id>` is not
    // what is cloned — instead exercise the pieces it composes.
    let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
    let checkout = packs_checkout_dir(&packs_root, &owner, &id);
    let sha = sync_packs_checkout(&checkout, &url, &project, &auth).expect("sync");
    assert_eq!(sha, third);
    let located =
        locate_role_source(&checkout, DEFAULT_FLAT_PATH, "verifier").expect("flat verifier");
    assert!(matches!(located, RoleSource::Flat { .. }));
    assert_eq!(
        pack_ref_path(&located, DEFAULT_FLAT_PATH),
        "beekeeper/roles/verifier"
    );
    let staged = stage_composed_pack(
        &packs_root,
        &format!("{}-{sha}", pack_cache_dir_name(&owner, &id)),
        &located,
        &TemplateCatalog::empty("0.5.16"),
        SourceProvenance {
            kind: "repository".into(),
            repo: Some(REPO.into()),
            sha: Some(sha.clone()),
            path: "beekeeper/roles/verifier".into(),
        },
    )
    .expect("staged");
    let resolved = buzz_persona_pkg::resolve::resolve_persona_by_name(&staged.dir, "verifier")
        .expect("staged pack resolves");
    assert_eq!(resolved.system_prompt, "You verify, flat.\n");
    assert_eq!(resolved.description, "Verifies.");
    let provenance: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(staged.dir.join("compose.json")).expect("compose.json"),
    )
    .expect("json");
    assert_eq!(provenance["source"]["kind"], "repository");
    assert_eq!(provenance["source"]["sha"], sha);
    assert_eq!(provenance["source"]["path"], "beekeeper/roles/verifier");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_source_names_exactly_one_of_a_ref_and_a_sha() {
    assert!(source(REPO, Some("refs/heads/main"), None).target().is_ok());
    assert!(source(REPO, None, Some(&"a".repeat(40))).target().is_ok());
    let both = source(REPO, Some("refs/heads/main"), Some(&"a".repeat(40)));
    assert!(both.target().unwrap_err().contains("exactly one"));
    assert!(source(REPO, None, None)
        .target()
        .unwrap_err()
        .contains("neither"));
    assert!(source(REPO, None, Some("nothex")).target().is_err());
    assert!(source(REPO, Some("refs/heads/../evil"), None)
        .target()
        .is_err());
}

#[test]
fn a_repository_coordinate_is_validated_before_it_becomes_a_path() {
    let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
    assert_eq!(id, "packs");
    assert_eq!(
        packs_checkout_dir(Path::new("/cache"), &owner, &id),
        PathBuf::from("/cache/aa11bb22-packs")
    );
    assert_eq!(
        packs_clone_url("https://hive.example/", &owner, &id),
        format!("https://hive.example/git/{owner}/packs")
    );
    for bad in [
        "30621:aa:packs",
        "30617:not-hex:packs",
        &format!("30617:{owner}:../escape"),
        &format!("30617:{owner}:"),
        &format!("30617:{owner}:-flag"),
    ] {
        assert!(
            parse_repo_coordinate(bad).is_err(),
            "{bad} must not become a path"
        );
    }
}

#[test]
fn a_pack_path_may_only_name_a_place_inside_the_checkout() {
    assert_eq!(validate_pack_path("").expect("default"), DEFAULT_PACK_PATH);
    assert_eq!(
        validate_pack_path("/personas/roles/").expect("trimmed"),
        "personas/roles"
    );
    for bad in ["../etc", "personas/../../etc", "personas/-flag", "a//b"] {
        assert!(validate_pack_path(bad).is_err(), "{bad} must be refused");
    }
}

#[test]
fn the_host_and_the_cli_name_the_same_cache_directory() {
    // `bee packs status` derives the directory with
    // `buzz_core::project_pack_source::pack_cache_dir_name` from the whole
    // coordinate; this host derives it from the two halves. If they ever
    // disagreed the CLI would report `cache_present: false` over a cache
    // the host had just filled — a wrong answer that looks like a fact.
    let owner = "a".repeat(64);
    for id in ["packs", "beekeeper-packs", &"z".repeat(64)] {
        let coordinate = format!("30617:{owner}:{id}");
        assert_eq!(
            Some(pack_cache_dir_name(&owner, id)),
            buzz_core_pkg::project_pack_source::pack_cache_dir_name(&coordinate),
            "{coordinate}"
        );
    }
}

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
