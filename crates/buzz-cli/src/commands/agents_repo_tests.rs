//! `bee agents-repo` tests: the git half against a throwaway bare
//! repository, and the pure helpers.

use std::path::{Path, PathBuf};

use buzz_persona::template::TemplateCatalog;

use crate::commands::agents_repo_git::{
    commit_drafts, AssetBytes, CommitOutcome, CommitRequest, DraftChange, Identity,
};

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
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn templates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/templates")
}

/// A bare remote seeded with the shipped agents-repository layout on `main`,
/// plus a working clone for the test to push competing commits from.
struct Fixture {
    root: PathBuf,
    remote: String,
    work: PathBuf,
    catalog: TemplateCatalog,
    /// The bytes an `asset.put` would have been handed; empty unless a test
    /// puts some there.
    assets: AssetBytes,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("bee-agents-repo-{}", uuid::Uuid::new_v4().simple()));
        let remote_dir = root.join("remote.git");
        let work = root.join("work");
        std::fs::create_dir_all(&work).expect("mkdir");
        git(
            &root,
            &[
                "init",
                "--bare",
                "--quiet",
                "--initial-branch=main",
                remote_dir.to_str().unwrap(),
            ],
        );
        let catalog = TemplateCatalog::load(&templates(), "test").expect("catalog");
        buzz_persona::seed::write_agents_repo_seed(&work, &catalog, "tank-loop").expect("seed");
        git(&work, &["init", "--quiet", "--initial-branch=main"]);
        git(&work, &["add", "--all"]);
        git(&work, &["commit", "--quiet", "-m", "seed"]);
        let remote = format!("file://{}", remote_dir.display());
        git(&work, &["push", "--quiet", &remote, "HEAD:refs/heads/main"]);
        Self {
            root,
            remote,
            work,
            catalog,
            assets: AssetBytes::new(),
        }
    }

    fn tip(&self) -> String {
        git(&self.work, &["ls-remote", &self.remote, "refs/heads/main"])
            .split_whitespace()
            .next()
            .expect("tip")
            .to_owned()
    }

    fn blob(&self, path: &str) -> Option<String> {
        let tip = self.tip();
        git(
            &self.work,
            &["fetch", "--quiet", &self.remote, "refs/heads/main"],
        );
        let output = crate::commands::sessions::worktree::git_command(&self.work)
            .args(["rev-parse", "--verify", "--quiet", &format!("{tip}:{path}")])
            .output()
            .expect("rev-parse");
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn file_on_main(&self, path: &str) -> Option<String> {
        let tip = self.tip();
        git(
            &self.work,
            &["fetch", "--quiet", &self.remote, "refs/heads/main"],
        );
        let output = crate::commands::sessions::worktree::git_command(&self.work)
            .args(["show", &format!("{tip}:{path}")])
            .output()
            .expect("show");
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn committer() -> Identity {
        Identity {
            name: "Andy".into(),
            email: "11111111@beekeeper.local".into(),
        }
    }

    fn request<'a>(
        &'a self,
        changes: &'a [DraftChange],
        coauthors: &'a [Identity],
        committer: &'a Identity,
    ) -> CommitRequest<'a> {
        CommitRequest {
            remote: &self.remote,
            expected_tip: None,
            changes,
            message: "docs(agents): test",
            committer,
            coauthors,
            catalog: &self.catalog,
            project: PROJECT,
            assets: &self.assets,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

const PROJECT: &str =
    "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:tank-loop";
const ALICE: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn put(id: &str, path: &str, text: &str, base: Option<String>) -> DraftChange {
    DraftChange {
        id: id.repeat(64),
        author: ALICE.into(),
        op: "file.put".into(),
        path: path.into(),
        to: None,
        text: Some(text.into()),
        sha256: None,
        base,
        message: Some("because".into()),
    }
}

#[test]
fn commit_builds_from_tip_and_pushes_one_signed_commit_with_coauthors() {
    let fx = Fixture::new();
    let tip_before = fx.tip();
    let lead_blob = fx.blob("roles/lead.md").expect("lead on main");
    let changes = vec![
        put("1", "plans/rpg.md", "# RPG\n\nA plan.\n", None),
        put("2", "roles/lead.md", "---\ndescription: \"Leads.\"\n---\n![[beekeeper/lead@^1.0.0]]\n![[beekeeper/working-contract@^1.0.0]]\n", Some(lead_blob)),
        DraftChange {
            id: "3".repeat(64),
            author: ALICE.into(),
            op: "file.move".into(),
            path: "roles/poker.md".into(),
            to: Some("roles/archive/poker.md".into()),
            text: None,
            sha256: None,
            base: fx.blob("roles/poker.md"),
            message: None,
        },
    ];
    // team.yml still names poker, so the tree refuses until the manifest
    // drops it — prove the refusal names team.yml, then fix and land.
    let coauthors = vec![Identity {
        name: "Alice".into(),
        email: "22222222@beekeeper.local".into(),
    }];
    let committer = Fixture::committer();
    let outcome = commit_drafts(&fx.request(&changes, &coauthors, &committer)).expect("runs");
    match &outcome {
        CommitOutcome::No {
            refusals,
            tip_before: Some(t),
        } => {
            assert_eq!(t, &tip_before);
            assert!(
                refusals.iter().all(|r| r.code == "invalid-tree"),
                "{refusals:?}"
            );
            assert!(
                refusals
                    .iter()
                    .any(|r| r.path.as_deref() == Some("team.yml")),
                "{refusals:?}"
            );
        }
        other => panic!("expected an invalid-tree refusal, got {other:?}"),
    }
    assert_eq!(fx.tip(), tip_before, "nothing pushed");

    let team = fx.file_on_main("team.yml").expect("team.yml");
    let team: serde_yaml::Value = serde_yaml::from_str(&team).expect("yaml");
    let mut team = team;
    team["roles"]
        .as_mapping_mut()
        .expect("roles")
        .remove("poker");
    team["agents"]
        .as_sequence_mut()
        .expect("agents")
        .retain(|a| a["role"].as_str() != Some("poker"));
    let mut changes = changes;
    changes.push(put(
        "4",
        "team.yml",
        &serde_yaml::to_string(&team).expect("yaml"),
        fx.blob("team.yml"),
    ));

    let outcome = commit_drafts(&fx.request(&changes, &coauthors, &committer)).expect("runs");
    let CommitOutcome::Yes {
        commit,
        paths,
        tip_before: t,
        actions,
        ..
    } = &outcome
    else {
        panic!("expected a push, got {outcome:?}");
    };
    assert_eq!(t, &tip_before);
    assert_eq!(fx.tip(), *commit, "main is the new commit");
    assert!(actions.starts_with("checked"), "{actions}");
    let mut touched: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();
    touched.sort();
    assert_eq!(
        touched,
        [
            "plans/rpg.md",
            "roles/archive/poker.md",
            "roles/lead.md",
            "roles/poker.md",
            "team.yml"
        ]
    );
    assert_eq!(
        fx.file_on_main("plans/rpg.md").as_deref(),
        Some("# RPG\n\nA plan.\n")
    );
    assert!(fx.file_on_main("roles/poker.md").is_none());
    assert!(fx.file_on_main("roles/archive/poker.md").is_some());

    let body = git(&fx.work, &["log", "-1", "--format=%B", commit]);
    assert!(body.starts_with("docs(agents): test"), "{body}");
    assert!(body.contains("- because"), "{body}");
    assert!(
        body.contains("Co-authored-by: Alice <22222222@beekeeper.local>"),
        "{body}"
    );
    assert!(
        body.contains("Signed-off-by: Andy <11111111@beekeeper.local>"),
        "{body}"
    );
    assert!(
        body.contains(&format!("Beekeeper-Drafts: {}", "1".repeat(64))),
        "{body}"
    );
    let author = git(&fx.work, &["log", "-1", "--format=%an <%ae>", commit]);
    assert_eq!(author, "Andy <11111111@beekeeper.local>");
    // One parent: the tip before.
    let parents = git(&fx.work, &["log", "-1", "--format=%P", commit]);
    assert_eq!(parents, tip_before);
}

#[test]
fn a_stale_base_refuses_naming_path_and_author_and_pushes_nothing() {
    let fx = Fixture::new();
    let tip_before = fx.tip();
    let stale = "0".repeat(40);
    let changes = vec![
        put("1", "plans/rpg.md", "new\n", None),
        put("2", "roles/lead.md", "changed\n", Some(stale)),
        put("3", "README.md", "readme\n", None), // exists on main, drafted as new
    ];
    let committer = Fixture::committer();
    let outcome = commit_drafts(&fx.request(&changes, &[], &committer)).expect("runs");
    let CommitOutcome::No { refusals, .. } = &outcome else {
        panic!("expected refusal, got {outcome:?}");
    };
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    let lead = refusals
        .iter()
        .find(|r| r.path.as_deref() == Some("roles/lead.md"))
        .expect("lead");
    assert_eq!(lead.code, "stale-base");
    assert!(
        lead.message.contains("22222222"),
        "names the author: {}",
        lead.message
    );
    assert!(
        lead.message.contains("nothing was pushed"),
        "{}",
        lead.message
    );
    let readme = refusals
        .iter()
        .find(|r| r.path.as_deref() == Some("README.md"))
        .expect("readme");
    assert!(
        readme.message.contains("exists on main now"),
        "{}",
        readme.message
    );
    assert_eq!(fx.tip(), tip_before);
}

#[test]
fn a_tree_the_composer_refuses_is_named_and_not_pushed() {
    let fx = Fixture::new();
    let tip_before = fx.tip();
    let changes = vec![put(
        "1",
        "roles/lead.md",
        "---\ndescription: lead\n---\n![[beekeeper/no-such-template@^1.0.0]]\n",
        fx.blob("roles/lead.md"),
    )];
    let committer = Fixture::committer();
    let outcome = commit_drafts(&fx.request(&changes, &[], &committer)).expect("runs");
    let CommitOutcome::No { refusals, .. } = &outcome else {
        panic!("expected refusal, got {outcome:?}");
    };
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0].code, "invalid-tree");
    assert_eq!(refusals[0].path.as_deref(), Some("roles/lead.md"));
    assert_eq!(fx.tip(), tip_before);
}

#[test]
fn main_moving_under_the_expected_tip_refuses_before_building() {
    let fx = Fixture::new();
    let tip_before = fx.tip();
    // Someone else lands a commit first.
    std::fs::write(fx.work.join("plans/other.md"), "other\n").expect("write");
    git(&fx.work, &["add", "--all"]);
    git(&fx.work, &["commit", "--quiet", "-m", "other"]);
    git(
        &fx.work,
        &["push", "--quiet", &fx.remote, "HEAD:refs/heads/main"],
    );
    let moved = fx.tip();
    assert_ne!(moved, tip_before);

    let changes = vec![put("1", "plans/rpg.md", "new\n", None)];
    let committer = Fixture::committer();
    let mut request = fx.request(&changes, &[], &committer);
    request.expected_tip = Some(&tip_before);
    let outcome = commit_drafts(&request).expect("runs");
    let CommitOutcome::No {
        refusals,
        tip_before: Some(t),
    } = &outcome
    else {
        panic!("expected refusal, got {outcome:?}");
    };
    assert_eq!(t, &moved);
    assert_eq!(refusals[0].code, "main-moved");
    assert_eq!(fx.tip(), moved);

    // Without an expectation the same drafts land on the moved tip.
    request.expected_tip = None;
    let outcome = commit_drafts(&request).expect("runs");
    assert!(matches!(outcome, CommitOutcome::Yes { .. }), "{outcome:?}");
    assert!(
        fx.file_on_main("plans/other.md").is_some(),
        "the other commit is kept"
    );
}

#[test]
fn archive_counterpart_and_the_persona_archive_dir_agree() {
    assert_eq!(
        buzz_core::agents_repo_draft::ARCHIVE_SEGMENT,
        buzz_persona::team::ARCHIVE_DIR
    );
    assert_eq!(
        buzz_core::agents_repo_draft::archive_counterpart("roles/lead.md").as_deref(),
        Some(format!("roles/{}/lead.md", buzz_persona::team::ARCHIVE_DIR).as_str())
    );
}

#[test]
fn plan_names_resolve_to_plan_paths() {
    assert_eq!(super::plan_path("rpg").unwrap(), "plans/rpg.md");
    assert_eq!(super::plan_path("rpg.md").unwrap(), "plans/rpg.md");
    assert_eq!(super::plan_path("plans/rpg.md").unwrap(), "plans/rpg.md");
    assert_eq!(
        super::plan_path("plans/archive/rpg.md").unwrap(),
        "plans/archive/rpg.md"
    );
    assert!(super::plan_path("roles/lead.md").is_err());
    assert!(super::plan_path("../x").is_err());
    assert!(super::plan_path("Archive").is_err());
}

/// An image's bytes never travel in a draft op, so `asset.put` is the one op
/// whose content the committer is handed rather than reading. The host
/// (Tauri) path and this one read the same ops; when only the host learned
/// `asset.put`, `bee agents-repo commit --all` answered "unknown draft op
/// \"asset.put\"" on a draft the relay had already accepted. These two tests
/// pin the arm and its refusal; the exhaustive match on
/// `AgentsRepoDraftOpKind` in `agents_repo_git.rs` is what keeps the next op
/// from diverging silently.
#[test]
fn an_asset_put_lands_the_exact_bytes_it_was_handed() {
    use sha2::{Digest, Sha256};

    let mut fx = Fixture::new();
    // Binary, with a NUL and a high byte, so a path that treated the blob as
    // text would corrupt it visibly.
    let bytes: Vec<u8> = vec![
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 1, 2, 0xff,
    ];
    let sha = hex::encode(Sha256::digest(&bytes));
    fx.assets.insert(sha.clone(), bytes.clone());
    let path = "docs/mockups/img/shot.png";
    let changes = vec![DraftChange {
        id: "4".repeat(64),
        author: ALICE.into(),
        op: "asset.put".into(),
        path: path.into(),
        to: None,
        text: None,
        sha256: Some(sha),
        base: None,
        message: Some("the shot".into()),
    }];
    let committer = Fixture::committer();
    let outcome = commit_drafts(&fx.request(&changes, &[], &committer)).expect("runs");
    let CommitOutcome::Yes { paths, .. } = &outcome else {
        panic!("expected a push, got {outcome:?}");
    };
    assert!(
        paths.iter().any(|p| p.path == path && p.status == "A"),
        "{paths:?}"
    );
    // The blob in the tree is git's hash of exactly these bytes.
    let file = fx.root.join("shot.png");
    std::fs::write(&file, &bytes).expect("write");
    let expected = git(&fx.work, &["hash-object", file.to_str().unwrap()]);
    assert_eq!(fx.blob(path).as_deref(), Some(expected.as_str()));
}

#[test]
fn an_asset_put_whose_bytes_were_never_fetched_refuses_by_name() {
    let fx = Fixture::new();
    let sha = "ab".repeat(32);
    let changes = vec![DraftChange {
        id: "5".repeat(64),
        author: ALICE.into(),
        op: "asset.put".into(),
        path: "docs/mockups/img/shot.png".into(),
        to: None,
        text: None,
        sha256: Some(sha),
        base: None,
        message: None,
    }];
    let committer = Fixture::committer();
    let tip_before = fx.tip();
    let error = commit_drafts(&fx.request(&changes, &[], &committer)).expect_err("refuses");
    let message = error.to_string();
    assert!(message.contains("abababab"), "names the blob: {message}");
    assert!(message.contains("nothing was pushed"), "{message}");
    assert_eq!(fx.tip(), tip_before);
}
