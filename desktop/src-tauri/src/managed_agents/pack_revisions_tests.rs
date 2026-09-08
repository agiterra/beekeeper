//! Tests for the reported-revision comparison.
//!
//! **The two-host proof simulates two isolated hosts on one machine.** One
//! throwaway packs repository plays the relay's; two checkout directories,
//! `host-a` and `host-b`, play two computers' packs caches, each synced by
//! [`packs_cache::sync_packs_checkout`] exactly as the Roles view syncs the
//! real one. Nothing here spans two machines — no second machine was
//! available — but every fact the comparison reads is per-checkout (the
//! object store and `HEAD` of one directory), so two directories exercise the
//! same code paths two computers would. What it does *not* prove is anything
//! about the relay, the network, or clock skew between hosts.
//!
//! Split into its own file the way `role_packs_view_tests.rs` is, to keep both
//! under the repository's 1000-line ceiling.

use super::*;

use crate::commands::project_git_exec::build_test_git_auth_config;
use crate::managed_agents::packs_cache::{ProjectPackSource, DEFAULT_PACK_PATH};

/// The packs repository coordinate every fixture here names. No relay is
/// asked about it: the comparison only ever echoes it back on the wire.
const REPO: &str = "30617:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:packs";

/// A fixed clock, so `comparedAt` is asserted rather than tolerated.
const NOW_MS: u64 = 1_757_000_000_000;

/// A commit no fixture ever writes: 40 lowercase hex, and not in any object
/// store here.
const STRANGER: &str = "0123456789abcdef0123456789abcdef01234567";

/// A throwaway directory tree, removed when the test ends.
///
/// Never a worktree of this repository: every `git` invocation below writes
/// commits, and one aimed at the checkout the tests were launched from would
/// eventually land in it.
struct Scratch {
    dir: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn scratch() -> Scratch {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("pack-revisions-scratch")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
    std::fs::create_dir_all(&dir).expect("scratch root");
    Scratch { dir }
}

fn auth() -> GitAuthConfig {
    build_test_git_auth_config().expect("git auth")
}

fn git(args: &[&str], cwd: &Path) -> String {
    run_git(args, Some(cwd), &auth()).unwrap_or_else(|error| panic!("git {args:?}: {error}"))
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, contents).expect("write");
}

/// One role pack under `<root>/personas/roles/<role>`, so the fixture repo is
/// shaped like a real packs repository rather than an empty commit chain.
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

/// A packs repository on `main` holding one role pack, and the commit it is
/// on. The identity the commits are authored with comes from
/// [`build_test_git_auth_config`], never from the machine running the test.
fn packs_origin(root: &Path) -> (PathBuf, String) {
    let dir = root.join("packs-origin");
    std::fs::create_dir_all(&dir).expect("origin dir");
    git(&["init", "--quiet", "--initial-branch", "main", "."], &dir);
    assert!(
        dir.join(".git").is_dir(),
        "the throwaway repository must own its own .git before anything is committed"
    );
    let sha = commit_pack(&dir, "builder", "You build. v1", "roles v1");
    (dir, sha)
}

/// Rewrite `role`'s pack, commit it, and return the new commit.
fn commit_pack(dir: &Path, role: &str, body: &str, message: &str) -> String {
    write_role_pack(dir, role, body);
    git(&["add", "--all"], dir);
    git(&["commit", "--quiet", "-m", message], dir);
    git(&["rev-parse", "HEAD"], dir).trim().to_string()
}

fn source(git_ref: Option<&str>, sha: Option<&str>) -> ProjectPackSource {
    ProjectPackSource {
        repo: REPO.to_string(),
        git_ref: git_ref.map(str::to_owned),
        sha: sha.map(str::to_owned),
        path: DEFAULT_PACK_PATH.to_string(),
    }
}

/// Sync one host's checkout from the fixture origin, the way the Roles view
/// syncs the real one, and return the commit it landed on.
fn sync(checkout: &Path, origin: &Path, source: &ProjectPackSource) -> String {
    packs_cache::sync_packs_checkout(
        checkout,
        &origin.to_string_lossy(),
        source,
        &build_test_git_auth_config().expect("git auth"),
    )
    .expect("sync")
}

fn compare(checkout: &Path, shas: &[&str]) -> ProjectPackRevisionComparison {
    let shas: Vec<String> = shas.iter().map(|sha| (*sha).to_string()).collect();
    compare_pack_revisions(Some(checkout), Some(REPO), &shas, &auth(), NOW_MS).expect("compare")
}

fn relation_for<'a>(
    comparison: &'a ProjectPackRevisionComparison,
    sha: &str,
) -> &'a PackRevisionRelation {
    comparison
        .relations
        .iter()
        .find(|relation| relation.sha == sha)
        .unwrap_or_else(|| panic!("no row for {sha}"))
}

fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn the_wire_shape_is_camel_case_with_exactly_these_keys() {
    let comparison = ProjectPackRevisionComparison {
        repo: Some(REPO.to_string()),
        current_sha: Some("a".repeat(40)),
        compared_at: NOW_MS,
        reason: None,
        relations: vec![PackRevisionRelation {
            sha: "b".repeat(40),
            relation: PackRevisionKind::Earlier,
            behind: Some(2),
            ahead: None,
        }],
    };
    let json = serde_json::to_value(&comparison).expect("serializes");
    assert_eq!(
        keys(&json),
        ["comparedAt", "currentSha", "reason", "relations", "repo"]
    );
    assert_eq!(json["comparedAt"], NOW_MS);
    assert_eq!(json["reason"], serde_json::Value::Null);
    assert_eq!(
        keys(&json["relations"][0]),
        ["ahead", "behind", "relation", "sha"]
    );
    assert_eq!(json["relations"][0]["relation"], "earlier");
    assert_eq!(json["relations"][0]["behind"], 2);
    assert_eq!(json["relations"][0]["ahead"], serde_json::Value::Null);

    let spellings: Vec<serde_json::Value> = [
        PackRevisionKind::Current,
        PackRevisionKind::Earlier,
        PackRevisionKind::Later,
        PackRevisionKind::Unrelated,
        PackRevisionKind::UnknownHere,
    ]
    .iter()
    .map(|kind| serde_json::to_value(kind).expect("serializes"))
    .collect();
    assert_eq!(
        spellings,
        ["current", "earlier", "later", "unrelated", "unknown-here"],
        "the five words the renderer switches on"
    );
}

#[test]
fn a_reported_revision_that_is_not_a_commit_refuses_the_whole_call() {
    let short = "a".repeat(39);
    for bad in [
        "not-a-sha",
        "ABCDEF0123456789ABCDEF0123456789ABCDEF01",
        short.as_str(),
        "",
    ] {
        let shas = vec!["c".repeat(40), bad.to_string()];
        let error = compare_pack_revisions(None, Some(REPO), &shas, &auth(), NOW_MS)
            .expect_err("a malformed revision refuses the call");
        assert!(
            error.contains(&format!("{bad:?}")),
            "the refusal names the offending value: {error}"
        );
    }
}

#[test]
fn a_computer_with_no_packs_checkout_says_so_and_claims_nothing() {
    let scratch = scratch();
    let unheld = "d".repeat(40);
    let shas = vec![unheld.clone(), STRANGER.to_string()];

    let no_source =
        compare_pack_revisions(None, None, &shas, &auth(), NOW_MS).expect("still an answer");
    assert_eq!(no_source.repo, None);
    assert_eq!(no_source.current_sha, None);
    assert_eq!(no_source.compared_at, NOW_MS);
    assert!(
        no_source
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("names no packs repository")),
        "{:?}",
        no_source.reason
    );
    assert!(no_source
        .relations
        .iter()
        .all(|relation| relation.relation == PackRevisionKind::UnknownHere));

    // A directory that exists but was never cloned into is the same answer
    // with the other reason: this computer has not resolved the source yet.
    let empty = scratch.dir.join("host-never-synced");
    std::fs::create_dir_all(&empty).expect("empty checkout dir");
    let unresolved = compare(&empty, &[unheld.as_str(), STRANGER]);
    assert_eq!(unresolved.repo.as_deref(), Some(REPO));
    assert_eq!(unresolved.current_sha, None);
    assert!(
        unresolved
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("has not resolved")),
        "{:?}",
        unresolved.reason
    );
    assert!(unresolved
        .relations
        .iter()
        .all(|relation| relation.relation == PackRevisionKind::UnknownHere));
}

#[test]
fn two_hosts_on_one_advancing_source_each_report_their_own_revision() {
    let scratch = scratch();
    let (origin, first) = packs_origin(&scratch.dir);
    let host_a = scratch.dir.join("host-a/packs/aa11bb22-packs");
    let host_b = scratch.dir.join("host-b/packs/aa11bb22-packs");
    let follow_main = source(Some("refs/heads/main"), None);

    // Both computers resolve the source while it points at the first commit.
    assert_eq!(sync(&host_a, &origin, &follow_main), first);
    assert_eq!(sync(&host_b, &origin, &follow_main), first);

    // The source advances, and only host A resolves it again — host B is the
    // machine that was not available.
    let second = commit_pack(&origin, "builder", "You build. v2", "roles v2");
    assert_ne!(first, second);
    assert_eq!(sync(&host_a, &origin, &follow_main), second);

    let a = compare(&host_a, &[first.as_str(), second.as_str(), STRANGER]);
    assert_eq!(a.current_sha.as_deref(), Some(second.as_str()));
    assert_eq!(a.reason, None);
    assert_eq!(a.compared_at, NOW_MS);
    let a_first = relation_for(&a, &first);
    assert_eq!(a_first.relation, PackRevisionKind::Earlier);
    assert_eq!(a_first.behind, Some(1), "one commit behind this machine");
    assert_eq!(a_first.ahead, None);
    let a_second = relation_for(&a, &second);
    assert_eq!(a_second.relation, PackRevisionKind::Current);
    assert_eq!((a_second.behind, a_second.ahead), (None, None));

    let b = compare(&host_b, &[first.as_str(), second.as_str(), STRANGER]);
    assert_eq!(b.current_sha.as_deref(), Some(first.as_str()));
    assert_eq!(b.reason, None);
    assert_eq!(relation_for(&b, &first).relation, PackRevisionKind::Current);
    assert_eq!(
        relation_for(&b, &second).relation,
        PackRevisionKind::UnknownHere,
        "the commit host B never fetched is not something host B can rank"
    );

    // A revision neither machine has ever held is unknown on both, never
    // "current" and never "earlier".
    for comparison in [&a, &b] {
        assert_eq!(
            relation_for(comparison, STRANGER).relation,
            PackRevisionKind::UnknownHere
        );
    }
}

#[test]
fn a_commit_from_another_branch_is_unrelated_not_older() {
    let scratch = scratch();
    let (origin, first) = packs_origin(&scratch.dir);
    let second = commit_pack(&origin, "builder", "You build. v2", "roles v2");

    // A commit on a branch that is not an ancestor of, and not reachable
    // from, `main`'s tip.
    git(
        &["checkout", "--quiet", "-b", "side", first.as_str()],
        &origin,
    );
    let side = commit_pack(&origin, "builder", "You build. sideways", "roles sideways");
    git(&["checkout", "--quiet", "main"], &origin);

    let host = scratch.dir.join("host-a/packs/aa11bb22-packs");
    // Resolving the side branch once puts its commit in this host's object
    // store; resolving main again leaves the checkout back on main's tip.
    assert_eq!(
        sync(&host, &origin, &source(Some("refs/heads/side"), None)),
        side
    );
    assert_eq!(
        sync(&host, &origin, &source(Some("refs/heads/main"), None)),
        second
    );

    let comparison = compare(&host, &[side.as_str(), first.as_str()]);
    assert_eq!(comparison.current_sha.as_deref(), Some(second.as_str()));
    let unrelated = relation_for(&comparison, &side);
    assert_eq!(
        unrelated.relation,
        PackRevisionKind::Unrelated,
        "present here, but neither an ancestor nor a descendant"
    );
    assert_eq!((unrelated.behind, unrelated.ahead), (None, None));
    assert_eq!(
        relation_for(&comparison, &first).relation,
        PackRevisionKind::Earlier,
        "the shared ancestor is still plainly older"
    );
}

#[test]
fn a_host_that_has_not_refreshed_reports_the_newer_revision_as_later() {
    let scratch = scratch();
    let (origin, first) = packs_origin(&scratch.dir);
    let second = commit_pack(&origin, "builder", "You build. v2", "roles v2");

    // This host fetched the newer commit and then went back to the pinned
    // older one, so it holds both objects and sits on the older.
    let host = scratch.dir.join("host-b/packs/aa11bb22-packs");
    assert_eq!(sync(&host, &origin, &source(None, Some(&second))), second);
    assert_eq!(sync(&host, &origin, &source(None, Some(&first))), first);

    let comparison = compare(&host, &[second.as_str()]);
    assert_eq!(comparison.current_sha.as_deref(), Some(first.as_str()));
    let later = relation_for(&comparison, &second);
    assert_eq!(
        later.relation,
        PackRevisionKind::Later,
        "an execution on a commit this machine has not moved to is newer, not unknown"
    );
    assert_eq!(later.ahead, Some(1), "one commit ahead of this machine");
    assert_eq!(later.behind, None);
}
