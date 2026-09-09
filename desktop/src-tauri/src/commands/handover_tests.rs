//! Tests for the handover checkout, against real git repositories.
//!
//! Every case builds a throwaway origin and a throwaway checkout under
//! `target/handover-fixtures/` and runs the real command against them. Nothing
//! here runs inside this repository's own worktree: a git-tooling test that
//! fetches, checks out branches and resets hard inside the checkout it is
//! being developed in is one bad argument away from destroying somebody's
//! work, and this crate has made that mistake before.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::commands::project_git_exec::build_test_git_auth_config;

static FIXTURE_SEQ: AtomicUsize = AtomicUsize::new(0);

const SESSION: &str = "683d55b3-d34e-410d-874a-55f9082d2631";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixture_dir(name: &str) -> PathBuf {
    let seq = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root()
        .join("target/handover-fixtures")
        .join(format!("{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

/// A hermetic git invocation for the fixtures themselves.
fn fixture_git(cwd: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Handover Test")
        .env("GIT_AUTHOR_EMAIL", "handover@example.invalid")
        .env("GIT_COMMITTER_NAME", "Handover Test")
        .env("GIT_COMMITTER_EMAIL", "handover@example.invalid")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout(output: std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

struct Fixture {
    origin: PathBuf,
    checkout: PathBuf,
    home: PathBuf,
    wip_sha: String,
}

/// One origin holding `refs/heads/wip/builder/abc12345`, and a clone of it.
fn fixture(name: &str) -> Fixture {
    let root = fixture_dir(name);
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let origin = root.join("origin");
    std::fs::create_dir_all(&origin).expect("origin");
    fixture_git(&origin, &home, &["init", "--initial-branch=main"]);
    std::fs::write(origin.join("README.md"), "base\n").expect("write");
    std::fs::write(origin.join("keep.txt"), "one\n").expect("write");
    fixture_git(&origin, &home, &["add", "."]);
    fixture_git(&origin, &home, &["commit", "-m", "base"]);
    fixture_git(&origin, &home, &["checkout", "-b", "wip/builder/abc12345"]);
    std::fs::write(origin.join("keep.txt"), "one\ntwo\n").expect("write");
    fixture_git(&origin, &home, &["add", "."]);
    fixture_git(&origin, &home, &["commit", "-m", "wip"]);
    let wip_sha = stdout(fixture_git(&origin, &home, &["rev-parse", "HEAD"]));
    fixture_git(&origin, &home, &["checkout", "main"]);

    let checkout = root.join("checkout");
    fixture_git(
        &root,
        &home,
        &[
            "-c",
            "protocol.file.allow=always",
            "clone",
            origin.to_str().expect("origin path"),
            checkout.to_str().expect("checkout path"),
        ],
    );
    Fixture {
        origin,
        checkout,
        home,
        wip_sha,
    }
}

fn request(
    fixture: &Fixture,
    overrides: impl FnOnce(&mut HandoverPrepareCheckoutRequest),
) -> HandoverPrepareCheckoutRequest {
    let mut request = HandoverPrepareCheckoutRequest {
        cwd: fixture.checkout.to_string_lossy().to_string(),
        repo_remote: Some(fixture.origin.to_string_lossy().to_string()),
        repo_ref: None,
        relay_origin: None,
        wip_ref: "refs/heads/wip/builder/abc12345".to_string(),
        sha: fixture.wip_sha.clone(),
        session_ref: SESSION.to_string(),
        patch_text: None,
        base_sha: None,
    };
    overrides(&mut request);
    request
}

fn prepare(
    request: &HandoverPrepareCheckoutRequest,
) -> Result<HandoverPrepareCheckoutReport, String> {
    let auth = build_test_git_auth_config().expect("test git auth");
    prepare_checkout(request, &auth)
}

#[test]
fn the_wip_ref_is_fetched_and_checked_out_on_its_own_branch() {
    let fixture = fixture("checkout");
    let report = prepare(&request(&fixture, |_| {})).expect("prepare");
    assert_eq!(report.branch, "handover/683d55b3");
    assert_eq!(report.checked_out_sha, fixture.wip_sha);
    assert_eq!(
        report.recovered,
        vec![format!(
            "wip-ref refs/heads/wip/builder/abc12345 at {}",
            fixture.wip_sha
        )]
    );
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    assert_eq!(
        stdout(fixture_git(
            &fixture.checkout,
            &fixture.home,
            &["rev-parse", "HEAD"]
        )),
        fixture.wip_sha
    );
    assert_eq!(
        std::fs::read_to_string(fixture.checkout.join("keep.txt")).expect("read"),
        "one\ntwo\n"
    );
}

#[test]
fn a_ref_that_moved_since_the_checkpoint_is_refused_rather_than_reconstructed() {
    let fixture = fixture("moved");
    let error = prepare(&request(&fixture, |request| {
        request.sha = "0".repeat(40);
    }))
    .expect_err("a moved ref must refuse");
    assert!(error.contains("not the"), "{error}");
    assert!(error.contains("nothing was checked out"), "{error}");
}

#[test]
fn a_dirty_checkout_is_never_checked_out_over() {
    let fixture = fixture("dirty");
    std::fs::write(fixture.checkout.join("README.md"), "mine\n").expect("write");
    let error = prepare(&request(&fixture, |_| {})).expect_err("dirty must refuse");
    assert!(error.contains("uncommitted changes"), "{error}");
    assert_eq!(
        std::fs::read_to_string(fixture.checkout.join("README.md")).expect("read"),
        "mine\n",
        "the person's own edit survives the refusal"
    );
}

/// A patch carrying a binary file and a brand-new untracked file.
fn binary_and_untracked_patch(fixture: &Fixture) -> (String, String) {
    // Build the patch in the origin, against the wip commit, then reset it.
    fixture_git(
        &fixture.origin,
        &fixture.home,
        &["checkout", "wip/builder/abc12345"],
    );
    let base = stdout(fixture_git(
        &fixture.origin,
        &fixture.home,
        &["rev-parse", "HEAD"],
    ));
    std::fs::write(
        fixture.origin.join("logo.bin"),
        [0u8, 159, 146, 150, 0, 255],
    )
    .expect("write");
    std::fs::write(fixture.origin.join("notes.md"), "a new file\n").expect("write");
    fixture_git(&fixture.origin, &fixture.home, &["add", "-A"]);
    let patch = stdout(fixture_git(
        &fixture.origin,
        &fixture.home,
        &["diff", "--binary", "--cached"],
    ));
    fixture_git(&fixture.origin, &fixture.home, &["reset", "--hard", "HEAD"]);
    fixture_git(&fixture.origin, &fixture.home, &["clean", "-fd"]);
    fixture_git(&fixture.origin, &fixture.home, &["checkout", "main"]);
    (patch, base)
}

#[test]
fn a_binary_and_untracked_patch_lands_whole() {
    let fixture = fixture("patch");
    let (patch, base) = binary_and_untracked_patch(&fixture);
    let report = prepare(&request(&fixture, |request| {
        request.patch_text = Some(patch.clone());
        request.base_sha = Some(base.clone());
    }))
    .expect("prepare");
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    assert!(
        report
            .recovered
            .iter()
            .any(|line| line.starts_with("patch applied")),
        "{:?}",
        report.recovered
    );
    assert_eq!(
        std::fs::read(fixture.checkout.join("logo.bin")).expect("binary file"),
        vec![0u8, 159, 146, 150, 0, 255],
        "the binary bytes are the author's, byte for byte"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.checkout.join("notes.md")).expect("new file"),
        "a new file\n"
    );
}

#[test]
fn a_patch_that_does_not_apply_leaves_no_half_applied_tree() {
    let fixture = fixture("refuse");
    // Built line by line rather than with `\`-continuations: a continuation
    // eats the leading space of a context line, and git would then refuse the
    // patch as *corrupt* instead of as *not applying* — a different code path
    // from the one this test is about.
    let unappliable = [
        "diff --git a/keep.txt b/keep.txt",
        "index 1111111..2222222 100644",
        "--- a/keep.txt",
        "+++ b/keep.txt",
        "@@ -1,2 +1,2 @@",
        "-something that is not there",
        " two",
        "+three",
        "",
    ]
    .join("\n");
    let report = prepare(&request(&fixture, |request| {
        request.patch_text = Some(unappliable.clone());
    }))
    .expect("the checkout still succeeds");
    assert_eq!(report.checked_out_sha, fixture.wip_sha);
    assert_eq!(report.missing.len(), 1, "{:?}", report.missing);
    assert!(
        report.missing[0].contains("keep.txt"),
        "the refusal names the file: {:?}",
        report.missing
    );
    assert_eq!(
        stdout(fixture_git(
            &fixture.checkout,
            &fixture.home,
            &["status", "--porcelain"]
        )),
        "",
        "a refused patch leaves the tree exactly as the checkout left it"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.checkout.join("keep.txt")).expect("read"),
        "one\ntwo\n"
    );
}

#[test]
fn a_patch_whose_base_is_absent_is_disclosed_rather_than_guessed() {
    let fixture = fixture("base");
    let report = prepare(&request(&fixture, |request| {
        request.patch_text = Some("diff --git a/x b/x\n".to_string());
        request.base_sha = Some("a".repeat(40));
    }))
    .expect("the checkout still succeeds");
    assert_eq!(report.missing.len(), 1);
    assert!(
        report.missing[0].contains("does not have"),
        "{:?}",
        report.missing
    );
}

#[test]
fn flag_shaped_refs_and_shas_never_reach_git_as_options() {
    let fixture = fixture("flags");
    let error = prepare(&request(&fixture, |request| {
        request.wip_ref = "--upload-pack=touch /tmp/pwned".to_string();
    }))
    .expect_err("a flag-shaped ref must refuse");
    assert!(
        error.contains("not a plain refs/heads/… branch ref"),
        "{error}"
    );

    let error = prepare(&request(&fixture, |request| {
        request.repo_remote = Some("--upload-pack=touch /tmp/pwned".to_string());
    }))
    .expect_err("a flag-shaped remote must refuse");
    assert!(error.contains("not a usable git remote"), "{error}");

    let error = prepare(&request(&fixture, |request| {
        request.sha = "--hard".to_string();
    }))
    .expect_err("a flag-shaped sha must refuse");
    assert!(error.contains("not a commit sha"), "{error}");
}

#[test]
fn a_refspec_that_would_write_a_local_branch_is_refused_before_the_fetch() {
    let fixture = fixture("refspec");
    let main_before = stdout(fixture_git(
        &fixture.checkout,
        &fixture.home,
        &["rev-parse", "refs/heads/main"],
    ));
    for hostile in [
        "+refs/heads/wip/builder/abc12345:refs/heads/main",
        "refs/heads/wip/builder/abc12345:main",
        "refs/heads/wip/builder/abc12345^{commit}",
        "refs/heads/wip/*",
        "wip/builder/abc12345",
        "refs/heads/../../etc/passwd",
    ] {
        let error = prepare(&request(&fixture, |request| {
            request.wip_ref = hostile.to_string();
        }))
        .expect_err("a ref that is not a plain branch ref must refuse");
        assert!(
            error.contains("not a plain refs/heads/… branch ref"),
            "{hostile}: {error}"
        );
    }
    // The load-bearing part: the person's own `main` was never written. A
    // refusal that arrived *after* the fetch would already have moved it.
    assert_eq!(
        stdout(fixture_git(
            &fixture.checkout,
            &fixture.home,
            &["rev-parse", "refs/heads/main"]
        )),
        main_before
    );
}

#[test]
fn an_earlier_reconstructions_branch_is_never_reset_over() {
    let fixture = fixture("reuse");
    let report = prepare(&request(&fixture, |_| {})).expect("first reconstruction");
    // A commit made on the reconstruction branch, as a continuation would.
    std::fs::write(fixture.checkout.join("keep.txt"), "one\ntwo\nthree\n").expect("write");
    fixture_git(&fixture.checkout, &fixture.home, &["add", "."]);
    fixture_git(
        &fixture.checkout,
        &fixture.home,
        &["commit", "-m", "continued"],
    );
    let continued = stdout(fixture_git(
        &fixture.checkout,
        &fixture.home,
        &["rev-parse", "HEAD"],
    ));
    assert_ne!(continued, report.checked_out_sha);
    fixture_git(&fixture.checkout, &fixture.home, &["checkout", "main"]);

    let error = prepare(&request(&fixture, |_| {})).expect_err("a second run must refuse");
    assert!(error.contains(&report.branch), "{error}");
    assert!(error.contains(&continued), "{error}");
    assert!(error.contains("nothing was checked out"), "{error}");
    assert_eq!(
        stdout(fixture_git(
            &fixture.checkout,
            &fixture.home,
            &["rev-parse", &format!("refs/heads/{}", report.branch)]
        )),
        continued,
        "the earlier reconstruction's commit is still on its branch"
    );
}

#[test]
fn a_branch_left_exactly_at_the_target_sha_is_reused() {
    let fixture = fixture("reuse-clean");
    prepare(&request(&fixture, |_| {})).expect("first reconstruction");
    fixture_git(&fixture.checkout, &fixture.home, &["checkout", "main"]);
    let report = prepare(&request(&fixture, |_| {})).expect("a repeat run is idempotent");
    assert_eq!(report.checked_out_sha, fixture.wip_sha);
}

const OWNER: &str = "1f2e3d4c5b6a798807162534435261708f9e0d1c2b3a49586776859403a2b1c0";
const RELAY_ORIGIN: &str = "https://hive.example.test";

/// Give the fixture's checkout a remote whose URL is the relay-hosted one.
fn add_relay_remote(fixture: &Fixture, name: &str, id: &str) {
    fixture_git(
        &fixture.checkout,
        &fixture.home,
        &[
            "remote",
            "add",
            name,
            &format!("{RELAY_ORIGIN}/git/{OWNER}/{id}"),
        ],
    );
}

/// A request that resolves its remote from the checkout, as production does.
fn resolving_request(fixture: &Fixture, id: &str) -> HandoverPrepareCheckoutRequest {
    request(fixture, |request| {
        request.repo_remote = None;
        request.repo_ref = Some(format!("30617:{OWNER}:{id}"));
        request.relay_origin = Some(RELAY_ORIGIN.to_string());
    })
}

#[test]
fn the_remote_is_read_from_the_checkout_whatever_it_is_called() {
    let fixture = fixture("remote-hive");
    // The only remote is called `hive`, and the clone's own `origin` was
    // removed: a tool that assumed a name would find nothing here.
    fixture_git(
        &fixture.checkout,
        &fixture.home,
        &["remote", "remove", "origin"],
    );
    add_relay_remote(&fixture, "hive", "beekeeper");
    // Point it at the fixture's real origin so the fetch can succeed, while
    // keeping the relay URL as the *matching* fetch URL would be circular —
    // instead the resolution is asserted on its own below, and this case
    // proves the name is taken from the checkout rather than assumed.
    let error = prepare(&resolving_request(&fixture, "beekeeper")).expect_err("no such host");
    assert!(
        error.contains("could not fetch") || error.contains("hive"),
        "the resolved remote is the one it tried: {error}"
    );
    assert!(!error.contains("origin"), "{error}");
}

#[test]
fn two_remotes_at_the_same_repository_are_a_refusal_that_names_both() {
    let fixture = fixture("remote-two");
    add_relay_remote(&fixture, "hive", "beekeeper");
    add_relay_remote(&fixture, "upstream", "beekeeper");
    let error = prepare(&resolving_request(&fixture, "beekeeper"))
        .expect_err("an ambiguous remote must refuse");
    assert!(error.contains("hive"), "{error}");
    assert!(error.contains("upstream"), "{error}");
    assert!(error.contains("not this app's guess to make"), "{error}");
}

#[test]
fn a_checkout_with_no_matching_remote_is_told_what_was_looked_for() {
    let fixture = fixture("remote-none");
    let error = prepare(&resolving_request(&fixture, "beekeeper"))
        .expect_err("no matching remote must refuse");
    assert!(
        error.contains(&format!("{RELAY_ORIGIN}/git/{OWNER}/beekeeper")),
        "{error}"
    );
    assert!(error.contains("does not guess a remote name"), "{error}");
}

#[test]
fn an_origin_pointing_somewhere_else_is_never_chosen() {
    let fixture = fixture("remote-other-origin");
    // The clone's `origin` points at the fixture's own origin directory, and a
    // differently named remote points at the repository the checkpoint names.
    add_relay_remote(&fixture, "hive", "beekeeper");
    let error =
        prepare(&resolving_request(&fixture, "beekeeper")).expect_err("hive is unreachable");
    assert!(
        !error.contains("no remote in this checkout"),
        "a matching remote was found: {error}"
    );
    assert!(
        error.contains("hive"),
        "the chosen remote is named: {error}"
    );
}

#[test]
fn nothing_is_fetched_when_the_coordinate_is_absent() {
    let fixture = fixture("remote-unknown");
    let error = prepare(&request(&fixture, |request| {
        request.repo_remote = None;
    }))
    .expect_err("with nothing to match against, this refuses");
    assert!(error.contains("will not guess a remote name"), "{error}");
}

#[test]
fn the_branch_name_is_the_session_slug_the_contract_names() {
    assert_eq!(
        handover_branch(SESSION).expect("branch"),
        "handover/683d55b3"
    );
    assert!(handover_branch("short").is_err());
}
