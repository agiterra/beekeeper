//! Tests for creating a project's packs repository.
//!
//! Every git operation here runs against **throwaway repositories under this
//! crate's own `target/` scratch directory**, never inside a worktree of this
//! repository: a test that shells `git` in the checkout it was launched from
//! will one day write to it.

use super::*;
use crate::commands::project_git_exec::build_test_git_auth_config;
use std::path::PathBuf;

fn scratch_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("l23b-scratch")
        .join(format!(
            "repo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
    std::fs::create_dir_all(&root).expect("scratch root");
    root
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, contents).expect("write");
}

/// A stand-in for the packs this build ships: two real role packs.
fn shipped_packs(root: &Path) -> PathBuf {
    let dir = root.join("shipped");
    for role in ["builder", "architect"] {
        write(
            &dir.join(role).join(".plugin/plugin.json"),
            &format!(
                r#"{{"id":"com.test.{role}","name":"{role}","version":"0.1.0","personas":["personas/{role}.persona.md"]}}"#
            ),
        );
        write(
            &dir.join(role).join(format!("personas/{role}.persona.md")),
            &format!(
                "---\nname: {role}\ndisplay_name: {role}\ndescription: The {role}.\nrole: {role}\n---\nYou are the {role}.\n"
            ),
        );
    }
    // Not a pack, and not a role: must not be reported as one.
    write(&dir.join("notes/README.md"), "# not a pack\n");
    dir
}

#[test]
fn a_project_slug_becomes_a_packs_repository_id() {
    assert_eq!(
        default_packs_repo_id("beekeeper").expect("id"),
        "beekeeper-packs"
    );
    assert_eq!(
        default_packs_repo_id("  My Project  ").expect("id"),
        "my-project-packs"
    );
    // The suffix survives the length bound: a repository called `<slug>`
    // instead of `<slug>-packs` would collide with the project's own code.
    let long = default_packs_repo_id(&"a".repeat(120)).expect("id");
    assert!(long.ends_with(PACKS_REPO_SUFFIX), "{long}");
    assert!(long.len() <= 64, "{} chars", long.len());
    assert!(default_packs_repo_id("///").is_err());
}

#[test]
fn a_project_coordinate_is_validated_before_it_names_anything() {
    let owner = "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";
    assert_eq!(
        parse_project_coordinate(&format!("30621:{owner}:beekeeper")).expect("coordinate"),
        (owner.to_string(), "beekeeper".to_string())
    );
    for bad in [
        "30617:aa:beekeeper",
        "30621:not-hex:beekeeper",
        &format!("30621:{owner}:"),
    ] {
        assert!(parse_project_coordinate(bad).is_err(), "{bad}");
    }
}

#[test]
fn seeding_writes_one_commit_holding_every_shipped_role() {
    let root = scratch_root();
    let shipped = shipped_packs(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-beekeeper-packs");

    let (commit, roles) = seed_packs_checkout(&checkout, &shipped, &auth).expect("seed");
    assert_eq!(commit.len(), 40, "{commit}");
    assert_eq!(
        roles,
        vec!["architect".to_string(), "builder".to_string()],
        "a directory that is not a role pack is not reported as a role"
    );
    // The tree is where the staging rule will look for it.
    assert!(checkout
        .join(packs_cache::DEFAULT_PACK_PATH)
        .join("builder/.plugin/plugin.json")
        .is_file());
    assert!(packs_cache::role_pack_in_checkout(
        &checkout,
        packs_cache::DEFAULT_PACK_PATH,
        "architect"
    )
    .is_some());
    // Exactly one commit, on the branch the 30624 will name.
    let log = run_git(&["log", "--oneline"], Some(&checkout), &auth).expect("log");
    assert_eq!(log.lines().count(), 1, "{log}");
    let branch = run_git(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        Some(&checkout),
        &auth,
    )
    .expect("branch");
    assert_eq!(branch.trim(), SEED_BRANCH);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_second_seed_replaces_the_tree_rather_than_layering_on_it() {
    let root = scratch_root();
    let shipped = shipped_packs(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-beekeeper-packs");
    seed_packs_checkout(&checkout, &shipped, &auth).expect("first seed");
    // A role that only the first attempt had must not survive the retry.
    write(
        &checkout
            .join(packs_cache::DEFAULT_PACK_PATH)
            .join("stale/marker.txt"),
        "left over\n",
    );
    let (_commit, roles) = seed_packs_checkout(&checkout, &shipped, &auth).expect("second seed");
    assert_eq!(roles, vec!["architect".to_string(), "builder".to_string()]);
    assert!(!checkout
        .join(packs_cache::DEFAULT_PACK_PATH)
        .join("stale")
        .exists());
    let log = run_git(&["log", "--oneline"], Some(&checkout), &auth).expect("log");
    assert_eq!(log.lines().count(), 1, "a retry is still one commit: {log}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_seed_pushes_to_the_repository_the_announcement_names() {
    let root = scratch_root();
    let shipped = shipped_packs(&root);
    let auth = build_test_git_auth_config().expect("git auth");
    // A stand-in for the relay's git server: a bare repository this test owns.
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&remote).expect("remote dir");
    run_git(&["init", "--quiet", "--bare"], Some(&remote), &auth).expect("bare init");

    let checkout = root.join("cache/aa11bb22-beekeeper-packs");
    let (commit, _roles) = seed_packs_checkout(&checkout, &shipped, &auth).expect("seed");
    run_git(
        &[
            "push",
            "--quiet",
            "--",
            remote.to_str().expect("utf-8"),
            &format!("HEAD:refs/heads/{SEED_BRANCH}"),
        ],
        Some(&checkout),
        &auth,
    )
    .expect("push");

    // The commit the caller is told about is the one the remote now has.
    let remote_head = run_git(
        &["rev-parse", &format!("refs/heads/{SEED_BRANCH}")],
        Some(&remote),
        &auth,
    )
    .expect("remote head");
    assert_eq!(remote_head.trim(), commit);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_symlink_in_the_shipped_packs_is_never_seeded() {
    // A symlink would be published pointing at a path on the machine that
    // seeded it, for everyone who later clones the repository.
    let root = scratch_root();
    let shipped = shipped_packs(&root);
    let outside = root.join("outside.txt");
    std::fs::write(&outside, "not yours\n").expect("write");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, shipped.join("builder/escape.txt")).expect("symlink");
    let auth = build_test_git_auth_config().expect("git auth");
    let checkout = root.join("cache/aa11bb22-beekeeper-packs");
    seed_packs_checkout(&checkout, &shipped, &auth).expect("seed");
    assert!(!checkout
        .join(packs_cache::DEFAULT_PACK_PATH)
        .join("builder/escape.txt")
        .exists());
    std::fs::remove_dir_all(&root).ok();
}

// --- LANE-L30: a caller-chosen repository id and name ---

#[test]
fn a_typed_name_is_kept_verbatim_but_trimmed() {
    assert_eq!(
        resolved_repo_name(Some("Agiterra Shared Packs"), "agiterra-packs"),
        "Agiterra Shared Packs"
    );
    assert_eq!(
        resolved_repo_name(Some("  Agiterra Shared Packs  "), "agiterra-packs"),
        "Agiterra Shared Packs"
    );
}

#[test]
fn an_absent_or_blank_name_falls_back_to_the_repo_id() {
    assert_eq!(resolved_repo_name(None, "agiterra-packs"), "agiterra-packs");
    assert_eq!(
        resolved_repo_name(Some(""), "agiterra-packs"),
        "agiterra-packs"
    );
    assert_eq!(
        resolved_repo_name(Some("   "), "agiterra-packs"),
        "agiterra-packs"
    );
}

#[test]
fn the_requests_name_reaches_the_announcements_name_tag() {
    // The Tauri boundary between "what the form sent" and "what the relay
    // receives" — a request naming a repository "Agiterra Shared Packs" must
    // not silently arrive as "<project-slug> role packs" (the old, fixed
    // default) or any other name the caller did not choose.
    let keys = Keys::generate();
    let event = build_announcement(
        &keys,
        "agiterra-packs",
        "30621:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:beekeeper",
        "Agiterra Shared Packs",
        "https://hive.example/git/aa11bb22/agiterra-packs",
    )
    .expect("announcement");

    fn tag_value<'a>(event: &'a nostr::Event, name: &str) -> Option<&'a str> {
        event.tags.iter().find_map(|t| {
            let values: Vec<&str> = t.as_slice().iter().map(|s| s.as_str()).collect();
            (values.first() == Some(&name))
                .then(|| values.get(1).copied())
                .flatten()
        })
    }

    assert_eq!(tag_value(&event, "name"), Some("Agiterra Shared Packs"));
    assert_eq!(tag_value(&event, "d"), Some("agiterra-packs"));
}

#[test]
fn a_custom_repo_id_bypasses_the_project_slug_default() {
    // One packs repository for many projects (LANE-L30) means the id the
    // announcement carries need not derive from this project's slug at all.
    let keys = Keys::generate();
    let event = build_announcement(
        &keys,
        "shared-org-packs",
        "30621:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:unrelated-slug",
        "shared-org-packs",
        "https://hive.example/git/aa11bb22/shared-org-packs",
    )
    .expect("announcement");
    let d = event
        .tags
        .iter()
        .find_map(|t| {
            let values: Vec<&str> = t.as_slice().iter().map(|s| s.as_str()).collect();
            (values.first() == Some(&"d"))
                .then(|| values.get(1).map(|s| s.to_string()))
                .flatten()
        })
        .expect("d tag");
    assert_eq!(d, "shared-org-packs");
    assert!(!d.contains("unrelated-slug"));
}
