//! The caller's own agents clone after `bee agents-repo commit`, end to end:
//! a real commit through `commit_drafts` to a throwaway relay-shaped bare
//! repository, a seat clone cut the way the host cuts one, and the real
//! `bee sessions work validate` reading the landed commit from that clone.

use std::path::{Path, PathBuf};

use beekeeper_persona::template::TemplateCatalog;

use super::{
    canonical_clone_url, clone_refresh_report, find_own_clone, redact_credentials, redact_url,
    OwnClone,
};
use crate::commands::agents_repo_git::{
    commit_drafts, AssetBytes, CommitOutcome, CommitRequest, DraftChange, Identity,
};

const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID: &str = "demo-beekeeper-agents";
const PROJECT: &str = "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:demo";
const PLAN: &str = "plans/example-plan.md";
/// A harmless synthetic value standing where a credential could be.
const SENTINEL: &str = "SYNTHETIC-SENTINEL-NOT-A-SECRET";

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = crate::commands::sessions::worktree::git_command(cwd)
        .args(args)
        .env("GIT_AUTHOR_NAME", "seed")
        .env("GIT_AUTHOR_EMAIL", "seed@test")
        .env("GIT_COMMITTER_NAME", "seed")
        .env("GIT_COMMITTER_EMAIL", "seed@test")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn has_object(dir: &Path, commit: &str) -> bool {
    crate::commands::sessions::worktree::git_command(dir)
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// A relay-shaped bare repository at `<root>/relay/git/<owner>/<id>`, a host
/// packs cache cloned from it, and a seat worktree whose `-agents` sibling is
/// cut from the cache and re-pointed at the relay's clone URL — as
/// `seat_agents_clone::cut_seat_agents_clone` does.
struct Seat {
    root: PathBuf,
    bare: PathBuf,
    relay: String,
    cache: PathBuf,
    worktree: PathBuf,
    clone: PathBuf,
    catalog: TemplateCatalog,
}

impl Seat {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bee-agents-clone-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("mkdir");
        let root = root.canonicalize().expect("canonical");
        let bare = root.join("relay/git").join(OWNER).join(ID);
        std::fs::create_dir_all(&bare).expect("mkdir");
        git(
            &bare,
            &["init", "--bare", "--quiet", "--initial-branch=main"],
        );
        let relay = format!("file://{}", bare.display());
        let catalog = TemplateCatalog::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/templates"),
            "test",
        )
        .expect("catalog");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).expect("mkdir");
        beekeeper_persona::seed::write_agents_repo_seed(&seed, &catalog, "demo").expect("seed");
        git(&seed, &["init", "--quiet", "--initial-branch=main"]);
        git(&seed, &["add", "--all"]);
        git(&seed, &["commit", "--quiet", "-m", "seed"]);
        git(&seed, &["push", "--quiet", &relay, "HEAD:refs/heads/main"]);

        let cache = root.join("host/packs/aaaaaaaa-demo-beekeeper-agents");
        std::fs::create_dir_all(cache.parent().unwrap()).expect("mkdir");
        git(
            &root,
            &["clone", "--quiet", &relay, cache.to_str().unwrap()],
        );

        let worktree = root.join("seats/lead-wt");
        std::fs::create_dir_all(&worktree).expect("mkdir");
        git(&worktree, &["init", "--quiet", "--initial-branch=main"]);
        git(
            &worktree,
            &[
                "remote",
                "add",
                "origin",
                "https://relay.invalid/git/bb/demo",
            ],
        );
        let clone = root.join("seats/lead-wt-agents");
        let tip = git(&cache, &["rev-parse", "HEAD"]);
        git(
            &root,
            &[
                "clone",
                "--quiet",
                "--",
                cache.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        git(&clone, &["checkout", "--quiet", "-B", "main", &tip]);
        git(&clone, &["remote", "set-url", "origin", "--", &relay]);
        Self {
            root,
            bare,
            relay,
            cache,
            worktree,
            clone,
            catalog,
        }
    }

    /// Land the example plan on the relay's main through the real committer.
    fn land_example_plan(&self) -> String {
        let changes = [DraftChange {
            id: "1".repeat(64),
            author: "2".repeat(64),
            op: "file.put".into(),
            path: PLAN.into(),
            to: None,
            text: Some(crate::commands::plans_example::EXAMPLE.into()),
            sha256: None,
            base: None,
            message: Some("the plan".into()),
        }];
        let committer = Identity {
            name: "Lead".into(),
            email: "11111111@beekeeper.local".into(),
        };
        let assets = AssetBytes::new();
        let outcome = commit_drafts(&CommitRequest {
            remote: &self.relay,
            expected_tip: None,
            changes: &changes,
            assets: &assets,
            message: "docs(agents): the plan",
            committer: &committer,
            coauthors: &[],
            catalog: &self.catalog,
            project: PROJECT,
        })
        .expect("runs");
        let CommitOutcome::Yes { commit, .. } = outcome else {
            panic!("expected a push, got {outcome:?}");
        };
        commit
    }

    fn report(&self, commit: &str) -> serde_json::Value {
        clone_refresh_report(Some(&self.worktree), &self.relay, commit)
    }

    fn reason(report: &serde_json::Value) -> &str {
        report["reason"].as_str().unwrap_or_default()
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// The seat's own clone receives the commit it just landed, fast-forwards,
/// and the real validator reads the committed plan from it at that commit.
#[test]
fn a_landed_commit_reaches_the_seats_own_clone_and_validates_there() {
    let seat = Seat::new();
    let commit = seat.land_example_plan();
    assert!(
        !has_object(&seat.clone, &commit),
        "stale before the refresh"
    );

    let report = seat.report(&commit);
    assert_eq!(report["path"], seat.clone.display().to_string());
    assert_eq!(report["remote"], "origin");
    assert_eq!(report["commit"], commit);
    assert_eq!(report["fetched"], true, "{report}");
    assert_eq!(report["working_tree"], "fast-forwarded", "{report}");
    assert_eq!(git(&seat.clone, &["rev-parse", "HEAD"]), commit);
    assert_eq!(
        git(&seat.clone, &["rev-parse", "refs/remotes/origin/main"]),
        commit
    );

    use crate::commands::sessions::work::{cmd_validate, WorkValidateArgs};
    cmd_validate(&WorkValidateArgs {
        plan: PLAN.into(),
        agents_repo: Some(seat.clone.display().to_string()),
        commit: Some(commit.clone()),
    })
    .expect("the committed plan validates from the seat's own clone");

    let again = seat.report(&commit);
    assert_eq!(again["working_tree"], "current", "{again}");
    // Standing inside the clone itself binds it too.
    assert_eq!(
        find_own_clone(&seat.clone, &seat.relay),
        OwnClone::Bound {
            path: seat.clone.clone(),
            remote: "origin".into()
        }
    );
}

/// The remote's name is read from the clone: a clone whose relay remote is
/// not called `origin` is bound through that name and its tracking ref.
#[test]
fn a_renamed_relay_remote_is_used_by_its_own_name() {
    let seat = Seat::new();
    git(&seat.clone, &["remote", "rename", "origin", "hive"]);
    git(
        &seat.clone,
        &[
            "remote",
            "add",
            "origin",
            "https://mirror.invalid/elsewhere",
        ],
    );
    let commit = seat.land_example_plan();
    let report = seat.report(&commit);
    assert_eq!(report["remote"], "hive", "{report}");
    assert_eq!(report["working_tree"], "fast-forwarded", "{report}");
    assert_eq!(
        git(&seat.clone, &["rev-parse", "refs/remotes/hive/main"]),
        commit
    );
    assert!(
        git(&seat.clone, &["for-each-ref", "refs/remotes/origin"]).is_empty(),
        "nothing was written under a remote that is not the relay"
    );
}

/// Uncommitted edits, untracked and ignored files the commit would write,
/// local commits and a detached checkout are never overwritten or reset; the
/// commit is still fetched and the answer names what stood in the way.
#[test]
fn local_work_in_the_clone_is_left_and_disclosed() {
    let seat = Seat::new();
    let head = git(&seat.clone, &["rev-parse", "HEAD"]);
    let commit = seat.land_example_plan();

    // An ignored file at exactly the path the commit adds: `git merge` alone
    // would overwrite it without a word.
    std::fs::write(seat.clone.join(".git/info/exclude"), format!("{PLAN}\n")).expect("exclude");
    std::fs::write(seat.clone.join(PLAN), "the seat's ignored notes\n").expect("write");
    let report = seat.report(&commit);
    assert_eq!(report["fetched"], true, "{report}");
    assert_eq!(report["working_tree"], "left");
    assert!(Seat::reason(&report).contains(PLAN), "{report}");
    assert_eq!(
        std::fs::read_to_string(seat.clone.join(PLAN)).unwrap(),
        "the seat's ignored notes\n"
    );
    assert_eq!(git(&seat.clone, &["rev-parse", "HEAD"]), head);
    assert!(has_object(&seat.clone, &commit));

    // The same file untracked but not ignored.
    std::fs::write(seat.clone.join(".git/info/exclude"), "").expect("exclude");
    let report = seat.report(&commit);
    assert_eq!(report["working_tree"], "left", "{report}");
    assert!(Seat::reason(&report).contains(PLAN), "{report}");
    std::fs::remove_file(seat.clone.join(PLAN)).expect("rm");

    // Uncommitted change to a tracked file.
    std::fs::write(seat.clone.join("README.md"), "the seat's own edit\n").expect("write");
    let report = seat.report(&commit);
    assert_eq!(report["working_tree"], "left", "{report}");
    assert!(
        Seat::reason(&report).contains("uncommitted changes"),
        "{report}"
    );
    assert_eq!(
        std::fs::read_to_string(seat.clone.join("README.md")).unwrap(),
        "the seat's own edit\n"
    );
    git(&seat.clone, &["checkout", "--quiet", "--", "README.md"]);

    // A local commit the relay does not have: main is not reset.
    std::fs::write(seat.clone.join("plans/local.md"), "local\n").expect("write");
    git(&seat.clone, &["add", "--all"]);
    git(&seat.clone, &["commit", "--quiet", "-m", "local, unpushed"]);
    let local = git(&seat.clone, &["rev-parse", "HEAD"]);
    let report = seat.report(&commit);
    assert_eq!(report["working_tree"], "left", "{report}");
    assert!(
        Seat::reason(&report).contains("cannot fast-forward"),
        "{report}"
    );
    assert_eq!(git(&seat.clone, &["rev-parse", "HEAD"]), local);

    // A detached checkout.
    git(&seat.clone, &["checkout", "--quiet", "--detach", &head]);
    let report = seat.report(&commit);
    assert_eq!(report["working_tree"], "left", "{report}");
    assert!(Seat::reason(&report).contains("detached"), "{report}");
    assert_eq!(git(&seat.clone, &["rev-parse", "HEAD"]), head);
}

/// A clone whose remote is the host's packs cache, the same owner and id on
/// another relay, or another project's repository is not bound: nothing is
/// fetched into it, the cache is never read through it, and no credential in
/// a remote URL is printed.
#[test]
fn a_clone_without_this_projects_relay_remote_is_not_touched() {
    let seat = Seat::new();
    let commit = seat.land_example_plan();

    git(
        &seat.clone,
        &[
            "remote",
            "set-url",
            "origin",
            "--",
            seat.cache.to_str().unwrap(),
        ],
    );
    // The cache is gone: had anything followed that remote it would fail
    // rather than silently read another tree.
    std::fs::remove_dir_all(&seat.cache).expect("rm cache");
    let report = seat.report(&commit);
    assert_eq!(report["fetched"], false, "{report}");
    assert!(
        Seat::reason(&report).contains("was not fetched into"),
        "{report}"
    );
    assert!(!has_object(&seat.clone, &commit));

    // Same owner and repository id, another relay, a synthetic credential.
    let other_relay = format!("https://user:{SENTINEL}@other-relay.invalid/git/{OWNER}/{ID}");
    git(
        &seat.clone,
        &["remote", "set-url", "origin", "--", &other_relay],
    );
    let found = find_own_clone(&seat.worktree, &seat.relay);
    assert!(
        matches!(&found, OwnClone::Unbound { remotes, .. }
            if remotes == &[format!("origin https://other-relay.invalid/git/{OWNER}/{ID}")]),
        "{found:?}"
    );
    let report = seat.report(&commit);
    assert_eq!(report["fetched"], false);
    assert!(!report.to_string().contains(SENTINEL), "{report}");
    assert!(!has_object(&seat.clone, &commit));

    // Another project's agents repository on the same relay.
    git(
        &seat.clone,
        &[
            "remote",
            "set-url",
            "origin",
            "--",
            &seat.relay.replace(ID, "other-beekeeper-agents"),
        ],
    );
    assert!(matches!(
        find_own_clone(&seat.worktree, &seat.relay),
        OwnClone::Unbound { .. }
    ));

    std::fs::remove_dir_all(&seat.clone).expect("rm clone");
    let report = seat.report(&commit);
    assert_eq!(report["path"], serde_json::Value::Null, "{report}");
    assert_eq!(
        report["commit"], commit,
        "the landed commit is still reported"
    );
}

/// A relay that cannot be reached after the push is disclosed with the
/// landed commit, not hidden.
#[test]
fn a_failed_fetch_is_disclosed_with_the_commit() {
    let seat = Seat::new();
    let commit = seat.land_example_plan();
    std::fs::remove_dir_all(&seat.bare).expect("rm relay");
    let report = seat.report(&commit);
    assert_eq!(report["fetched"], false, "{report}");
    assert_eq!(report["commit"], commit);
    assert!(
        Seat::reason(&report).contains("could not fetch"),
        "{report}"
    );
    assert!(!has_object(&seat.clone, &commit));
}

#[test]
fn canonical_clone_urls_name_one_relay_repository() {
    let canonical = canonical_clone_url(&format!("https://hive.example/git/{OWNER}/{ID}"));
    for same in [
        format!("https://hive.example/git/{OWNER}/{ID}/"),
        format!("https://hive.example/git/{OWNER}/{ID}.git"),
        format!("HTTPS://Hive.Example/git/{OWNER}/{ID}"),
        format!("https://hive.example:443/git/{OWNER}/{ID}"),
        format!("wss://hive.example/git/{OWNER}/{ID}"),
        format!("https://user:{SENTINEL}@hive.example/git/{OWNER}/{ID}"),
    ] {
        assert_eq!(canonical_clone_url(&same), canonical, "{same}");
    }
    for different in [
        format!("https://other-relay.example/git/{OWNER}/{ID}"),
        format!("http://hive.example/git/{OWNER}/{ID}"),
        format!("https://hive.example:8443/git/{OWNER}/{ID}"),
        format!("https://hive.example/git/{OWNER}/other-beekeeper-agents"),
        format!("https://hive.example/git/bbbb/{ID}"),
    ] {
        assert_ne!(canonical_clone_url(&different), canonical, "{different}");
    }
    assert_eq!(
        canonical_clone_url("/Users/someone/Library/Application Support/Beekeeper/packs/x"),
        None,
        "a local path is never a relay"
    );
}

#[test]
fn printed_urls_carry_no_credentials() {
    assert_eq!(
        redact_url(&format!(
            "https://user:{SENTINEL}@hive.example/git/o/r?token={SENTINEL}"
        )),
        "https://hive.example/git/o/r"
    );
    let text =
        format!("fatal: unable to access 'https://user:{SENTINEL}@hive.example/git/o/r/': refused");
    let redacted = redact_credentials(&text);
    assert!(!redacted.contains(SENTINEL), "{redacted}");
    assert!(
        redacted.contains("https://hive.example/git/o/r/"),
        "{redacted}"
    );
}
