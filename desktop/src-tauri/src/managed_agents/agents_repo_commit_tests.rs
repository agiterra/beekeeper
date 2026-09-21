//! The Files tab's commit and reads against a throwaway bare repository:
//! the tree is built from the tip, validated before the push, pushed under
//! a lease, and every refusal names its path.

use std::path::{Path, PathBuf};

use super::*;
use crate::commands::project_git_exec::build_test_git_auth_config;
use crate::managed_agents::agents_repo_read::{list_tip, read_tip, AgentsRepoCheckout};
use buzz_persona_pkg::template::TemplateCatalog;

const PROJECT: &str =
    "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:tank-loop";
const ALICE: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn templates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/templates")
}

fn git(
    auth: &crate::commands::project_git_exec::GitAuthConfig,
    cwd: &Path,
    args: &[&str],
) -> String {
    run_git(args, Some(cwd), auth).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
}

/// A bare remote seeded with the shipped layout, a "packs cache" clone of
/// it with `refs/remotes/origin/main`, and the resolved checkout the
/// commands work from.
struct Fixture {
    root: PathBuf,
    remote_url: String,
    cache: PathBuf,
    catalog: TemplateCatalog,
    auth: crate::commands::project_git_exec::GitAuthConfig,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "beekeeper-agents-repo-commit-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let auth = build_test_git_auth_config().expect("auth");
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        let cache = root.join("cache");
        std::fs::create_dir_all(&seed).expect("mkdir");
        std::fs::create_dir_all(&remote).expect("mkdir");
        git(
            &auth,
            &remote,
            &["init", "--bare", "--quiet", "--initial-branch=main"],
        );
        let catalog = TemplateCatalog::load(&templates(), "test").expect("catalog");
        buzz_persona_pkg::seed::write_agents_repo_seed(&seed, &catalog, "tank-loop").expect("seed");
        git(&auth, &seed, &["init", "--quiet", "--initial-branch=main"]);
        git(&auth, &seed, &["add", "--all"]);
        git(&auth, &seed, &["commit", "--quiet", "-m", "seed"]);
        let remote_url = format!("file://{}", remote.display());
        git(
            &auth,
            &seed,
            &["push", "--quiet", &remote_url, "HEAD:refs/heads/main"],
        );
        git(
            &auth,
            &root,
            &[
                "clone",
                "--quiet",
                "--",
                &remote_url,
                cache.to_str().unwrap(),
            ],
        );
        Self {
            root,
            remote_url,
            cache,
            catalog,
            auth,
        }
    }

    fn tip(&self) -> String {
        git(
            &self.auth,
            &self.cache,
            &["ls-remote", "--", &self.remote_url, "refs/heads/main"],
        )
        .split_whitespace()
        .next()
        .expect("tip")
        .to_owned()
    }

    fn checkout(&self) -> AgentsRepoCheckout {
        git(
            &self.auth,
            &self.cache,
            &[
                "fetch",
                "--quiet",
                "origin",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
        );
        let tip = git(
            &self.auth,
            &self.cache,
            &["rev-parse", "refs/remotes/origin/main"],
        );
        AgentsRepoCheckout {
            repo: format!("30617:{}:tank-loop-beekeeper-agents", "a".repeat(64)),
            branch: "main".into(),
            checkout: self.cache.clone(),
            clone_url: self.remote_url.clone(),
            tip: tip.trim().to_owned(),
            synced_at: None,
            auth: self.auth.clone(),
        }
    }

    fn blob(&self, path: &str) -> Option<String> {
        blob_at_tip(&self.checkout(), path)
    }

    fn commit(&self, request: &AgentsRepoCommitRequest) -> AgentsRepoCommitResult {
        let repo = self.checkout();
        let scratch = self
            .root
            .join(format!("scratch-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&scratch).expect("scratch");
        let ids = request.drafts.iter().map(|d| d.id.clone()).collect();
        commit_in(
            &scratch,
            &repo,
            request,
            &("Andy".to_owned(), "11111111@beekeeper.local".to_owned()),
            &repo.tip,
            ids,
            &self.catalog,
        )
        .expect("commit runs")
    }

    fn file_on_main(&self, path: &str) -> Option<String> {
        read_tip(&self.checkout(), path).expect("read").text
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn put(id: &str, path: &str, text: &str, base: Option<String>) -> AgentsRepoDraftChange {
    AgentsRepoDraftChange {
        id: id.repeat(64),
        author: ALICE.into(),
        op: "file.put".into(),
        path: path.into(),
        to: None,
        text: Some(text.into()),
        base,
        message: Some("because".into()),
    }
}

fn request(
    drafts: Vec<AgentsRepoDraftChange>,
    expected_tip: Option<String>,
) -> AgentsRepoCommitRequest {
    AgentsRepoCommitRequest {
        project_ref: PROJECT.into(),
        expected_tip,
        message: "docs(agents): test".into(),
        drafts,
        authors: vec![AgentsRepoAuthor {
            pubkey: ALICE.into(),
            name: Some("Alice".into()),
        }],
    }
}

#[test]
fn the_listing_and_a_read_come_from_the_fetched_tip() {
    let fx = Fixture::new();
    let repo = fx.checkout();
    let listing = list_tip(&repo).expect("list");
    assert_eq!(listing.commit, repo.tip);
    let paths: Vec<&str> = listing.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"team.yml"), "{paths:?}");
    assert!(paths.contains(&"roles/lead.md"), "{paths:?}");
    let kinds: std::collections::BTreeSet<&str> =
        listing.entries.iter().map(|e| e.kind.as_str()).collect();
    assert!(
        kinds.contains("role") && kinds.contains("manifest") && kinds.contains("gitkeep"),
        "{kinds:?}"
    );

    let lead = read_tip(&repo, "roles/lead.md").expect("read");
    assert_eq!(lead.state, "on-main");
    assert!(lead
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("beekeeper/lead@"));
    assert_eq!(lead.blob, fx.blob("roles/lead.md"));
    let absent = read_tip(&repo, "plans/rpg.md").expect("read");
    assert_eq!(absent.state, "not-on-main");
    assert!(absent.text.is_none() && absent.blob.is_none());
    assert!(
        read_tip(&repo, "../x").is_err(),
        "the path grammar is enforced on reads"
    );
}

#[test]
fn a_commit_lands_with_trailers_and_the_tree_is_validated_first() {
    let fx = Fixture::new();
    let tip_before = fx.tip();
    let mut drafts = vec![
        put("1", "plans/rpg.md", "# RPG\n", None),
        AgentsRepoDraftChange {
            id: "2".repeat(64),
            author: ALICE.into(),
            op: "file.move".into(),
            path: "roles/poker.md".into(),
            to: Some("roles/archive/poker.md".into()),
            text: None,
            base: fx.blob("roles/poker.md"),
            message: None,
        },
    ];
    let result = fx.commit(&request(drafts.clone(), Some(tip_before.clone())));
    assert_eq!(result.pushed, "no");
    assert!(
        result
            .refusals
            .iter()
            .any(|r| r.code == "invalid-tree" && r.path.as_deref() == Some("team.yml")),
        "{result:?}"
    );
    assert_eq!(fx.tip(), tip_before, "nothing pushed");

    let team = fx.file_on_main("team.yml").expect("team.yml");
    let team: String = team
        .lines()
        .filter(|l| !l.contains("poker"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    drafts.push(put("3", "team.yml", &team, fx.blob("team.yml")));
    let result = fx.commit(&request(drafts, Some(tip_before.clone())));
    assert_eq!(result.pushed, "yes", "{result:?}");
    let commit = result.commit.clone().expect("commit");
    assert_eq!(fx.tip(), commit);
    assert_eq!(result.tip_before.as_deref(), Some(tip_before.as_str()));
    assert!(result
        .actions
        .as_deref()
        .unwrap_or_default()
        .starts_with("checked"));
    let mut touched: Vec<&str> = result.paths.iter().map(|p| p.path.as_str()).collect();
    touched.sort();
    assert_eq!(
        touched,
        [
            "plans/rpg.md",
            "roles/archive/poker.md",
            "roles/poker.md",
            "team.yml"
        ]
    );
    assert_eq!(fx.file_on_main("plans/rpg.md").as_deref(), Some("# RPG\n"));
    assert!(fx.file_on_main("roles/poker.md").is_none());

    let body = git(&fx.auth, &fx.cache, &["log", "-1", "--format=%B", &commit]);
    assert!(
        body.contains("Co-authored-by: Alice <22222222@beekeeper.local>"),
        "{body}"
    );
    assert!(
        body.contains("Signed-off-by: Andy <11111111@beekeeper.local>"),
        "{body}"
    );
    assert!(body.contains("Beekeeper-Drafts: "), "{body}");
    assert!(body.contains("- because"), "{body}");
    let author = git(
        &fx.auth,
        &fx.cache,
        &["log", "-1", "--format=%an <%ae>", &commit],
    );
    assert_eq!(author.trim(), "Andy <11111111@beekeeper.local>");
}

#[test]
fn a_stale_base_and_a_moved_main_refuse_without_pushing() {
    let fx = Fixture::new();
    let tip_before = fx.tip();
    let result = fx.commit(&request(
        vec![put("1", "roles/lead.md", "changed\n", Some("0".repeat(40)))],
        None,
    ));
    assert_eq!(result.pushed, "no");
    assert_eq!(result.refusals.len(), 1);
    assert_eq!(result.refusals[0].code, "stale-base");
    assert!(
        result.refusals[0].message.contains("22222222"),
        "{}",
        result.refusals[0].message
    );
    assert_eq!(fx.tip(), tip_before);

    let result = fx.commit(&request(
        vec![put("1", "plans/rpg.md", "x\n", None)],
        Some("f".repeat(40)),
    ));
    assert_eq!(result.pushed, "no");
    assert_eq!(result.refusals[0].code, "main-moved");
    assert_eq!(fx.tip(), tip_before);
}

#[test]
fn a_push_that_loses_the_lease_reports_no_and_leaves_the_remote_alone() {
    let fx = Fixture::new();
    let repo = fx.checkout();
    let tip_before = repo.tip.clone();
    // Someone else lands a commit after this run read its tip.
    let other = fx.root.join("other");
    git(
        &fx.auth,
        &fx.root,
        &[
            "clone",
            "--quiet",
            "--",
            &fx.remote_url,
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("plans/other.md"), "other\n").expect("write");
    git(&fx.auth, &other, &["add", "--all"]);
    git(&fx.auth, &other, &["commit", "--quiet", "-m", "other"]);
    git(
        &fx.auth,
        &other,
        &["push", "--quiet", "origin", "HEAD:refs/heads/main"],
    );
    let moved = fx.tip();
    assert_ne!(moved, tip_before);

    let scratch = fx.root.join("scratch-lease");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let req = request(vec![put("1", "plans/rpg.md", "x\n", None)], None);
    let result = commit_in(
        &scratch,
        &repo,
        &req,
        &("Andy".to_owned(), "11111111@beekeeper.local".to_owned()),
        &tip_before,
        vec!["1".repeat(64)],
        &fx.catalog,
    )
    .expect("runs");
    assert_eq!(result.pushed, "no", "{result:?}");
    assert_eq!(result.refusals[0].code, "lease-rejected");
    assert_eq!(fx.tip(), moved, "the other commit stands");
}
