//! Tests for creating a project's repositories. Every git operation runs
//! against throwaway directories under this crate's own `target/`, and the
//! relay is a localhost stub — never the real one.

use super::*;
use std::path::PathBuf;

/// A fresh directory per call. The counter is what makes it fresh: the
/// clock alone has microsecond resolution on macOS, and two of the stub
/// tests starting in the same microsecond once shared a root — and one
/// test's cleanup removed the other's working directory mid-push.
fn scratch_root() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("agents-repo-scratch")
        .join(format!(
            "run-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

/// A stand-in for this build's catalog: two role templates and the three
/// shared fragments the seed includes.
fn catalog(root: &Path) -> TemplateCatalog {
    let dir = root.join("templates");
    for role in ["builder", "lead"] {
        write(
            &dir.join(role).join("1.0.0/TEMPLATE.md"),
            &format!("---\nname: {role}\nversion: 1.0.0\ndescription: The {role}.\nkind: role\nskills:\n  - ./skills/{role}-skill/\n---\nYou are the {role}.\n"),
        );
        write(
            &dir.join(role)
                .join(format!("1.0.0/skills/{role}-skill/SKILL.md")),
            &format!("---\nname: {role}-skill\ndescription: s\n---\ns\n"),
        );
    }
    for fragment in buzz_persona_pkg::seed::SHARED_FRAGMENTS {
        write(
            &dir.join(fragment).join("1.0.0/TEMPLATE.md"),
            &format!("---\nname: {fragment}\nversion: 1.0.0\ndescription: {fragment}\n---\n{fragment}.\n"),
        );
    }
    TemplateCatalog::load(&dir, "0.6.0").expect("catalog")
}

#[test]
fn the_agents_repo_id_keeps_its_suffix_whole() {
    assert_eq!(
        default_agents_repo_id("tank-loop").unwrap(),
        "tank-loop-beekeeper-agents"
    );
    let long = default_agents_repo_id(&"a".repeat(80)).unwrap();
    assert_eq!(long.len(), 64);
    assert!(long.ends_with(AGENTS_REPO_SUFFIX));
    assert!(default_agents_repo_id("---").is_err());
}

#[test]
fn the_seed_commit_holds_the_layout_by_reference() {
    let root = scratch_root();
    let catalog = catalog(&root);
    let checkout = root.join("checkout");
    let mut auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("auth");
    auth.set_commit_identity("Test".to_string(), "test@beekeeper.local".to_string());
    let (commit, roles) = seed_agents_checkout(
        &checkout,
        &catalog,
        "demo",
        &buzz_persona_pkg::seed::default_verify_command(),
        &auth,
    )
    .expect("seed commits");
    assert_eq!(commit.len(), 40);
    assert_eq!(roles, vec!["builder", "lead"]);
    for rel in [
        "team.yml",
        "actions.yml",
        // The registry travels with the project, in the seed commit, or a
        // routed hire on this project has nothing to route against (ledger
        // 178(a), 180).
        "model-registry.yaml",
        "README.md",
        "roles/lead.md",
        "roles/archive/.gitkeep",
        "plans/archive/.gitkeep",
    ] {
        assert!(checkout.join(rel).is_file(), "{rel}");
    }
    assert_eq!(
        std::fs::read_to_string(checkout.join("model-registry.yaml")).expect("registry"),
        buzz_persona_pkg::seed::SEEDED_MODEL_REGISTRY
    );
    let lead = std::fs::read_to_string(checkout.join("roles/lead.md")).expect("lead");
    assert!(lead.contains("![[beekeeper/lead@^1.0.0]]"));
    assert!(!lead.contains("You are the lead"), "referenced, not copied");
    // A retry clears and reseeds rather than layering a second copy.
    let (again, _) = seed_agents_checkout(
        &checkout,
        &catalog,
        "demo",
        &buzz_persona_pkg::seed::default_verify_command(),
        &auth,
    )
    .expect("reseed");
    assert_eq!(again.len(), 40);
    std::fs::remove_dir_all(&root).ok();
}

/// The code seed: one `README.md` — the project's name, then where its
/// roles and plans live — committed once on `main`.
#[test]
fn the_code_seed_readme_names_the_project_and_the_agents_repository() {
    assert_eq!(
        code_seed_readme(" RPG Test ", "rpg-test-beekeeper-agents"),
        "# RPG Test\n\nThis project's code. Roles and plans live in `rpg-test-beekeeper-agents`.\n"
    );
    let root = scratch_root();
    let checkout = root.join("code");
    let mut auth = crate::commands::project_git_exec::build_test_git_auth_config().expect("auth");
    auth.set_commit_identity("Test".to_string(), "test@beekeeper.local".to_string());
    let commit = seed_code_checkout(&checkout, "RPG Test", "rpg-test-beekeeper-agents", &auth)
        .expect("seed");
    assert_eq!(commit.len(), 40);
    let readme = std::fs::read_to_string(checkout.join("README.md")).expect("readme");
    assert_eq!(readme.lines().next(), Some("# RPG Test"));
    assert!(readme.contains("Roles and plans live in `rpg-test-beekeeper-agents`."));
    let branch = run_git(&["branch", "--show-current"], Some(&checkout), &auth).expect("branch");
    assert_eq!(branch.trim(), SEED_BRANCH);
    let files = run_git(&["ls-tree", "--name-only", "HEAD"], Some(&checkout), &auth).expect("tree");
    assert_eq!(files.trim(), "README.md", "one file, nothing else");
    // A retry clears and reseeds rather than layering.
    let again = seed_code_checkout(&checkout, "RPG Test", "rpg-test-beekeeper-agents", &auth)
        .expect("reseed");
    assert_eq!(again.len(), 40);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_source_names_the_branch_at_the_root() {
    let keys = Keys::generate();
    let owner = keys.public_key().to_hex();
    let project = format!("30621:{owner}:demo");
    let repo = format!("30617:{owner}:demo-beekeeper-agents");
    let event = build_agents_pack_source(&keys, &project, &repo).expect("source");
    let decoded = decode_project_pack_source(&event).expect("decodes");
    assert_eq!(decoded.repo(), repo);
    assert_eq!(decoded.path(), ".");
    assert_eq!(decoded.pin().value(), "refs/heads/main");
    assert_eq!(decoded.role_path("lead").as_deref(), Some("lead"));
}

#[cfg(not(target_os = "windows"))]
#[path = "agents_repo_tests_stub.rs"]
mod stub;

// Migrating a pre-pivot project, in its own file for the size gate. A
// sibling of `stub` rather than a child of `against_a_stub_relay`, because
// a `#[path]` inside an inline module resolves against a directory named
// after that module, which does not exist.
#[cfg(not(target_os = "windows"))]
#[path = "agents_repo_migrate_tests.rs"]
mod migrating;

// Project-service admission of this computer's host key (ledger 266),
// against the same stub relay; its own file for the size gate.
#[cfg(not(target_os = "windows"))]
#[path = "project_admission_tests.rs"]
mod admission;

/// The stub relay's submit route. The literal stays in this file rather
/// than the stub because `egress_guard_tests::EVENTS_INVENTORY` counts
/// `/events` URL sites per file and pins this test file's one site.
#[cfg(not(target_os = "windows"))]
const EVENTS_ROUTE: &str = "/events";

// Gated off Windows for the same reason `packs_repo_tests` is: the stub
// state pulls native DLLs unavailable on the Windows CI runner.
#[cfg(not(target_os = "windows"))]
mod against_a_stub_relay {
    use super::stub::*;
    use super::*;

    use crate::commands::project_git_exec::build_test_git_auth_config;
    use crate::managed_agents::project_roster::ensure_project_agents_on_roster;

    /// The push cannot land against a git-less stub: both announcements
    /// land, the code seed commits but its push fails (disclosed, the code
    /// announcement stands), the agents seed commits, its push fails, this
    /// run's agents announcement is withdrawn, no 30624 is published, no
    /// clone is attempted, and `gap` says what is missing first.
    #[tokio::test]
    async fn a_push_that_never_lands_withdraws_the_agents_announcement_and_names_the_gap() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let (relay_url, stored) = spawn_stub_relay(Vec::new(), None, None).await;
        let state = stubbed_state(relay_url, keys).await;

        let (result, recorded) = run_init(
            &state,
            &format!("30621:{viewer}:demo"),
            catalog,
            root.join("cache"),
            root.join("repos"),
            None,
        )
        .await;

        assert_eq!(result.code_repo_id, "demo");
        assert_eq!(result.agents_repo_id, "demo-beekeeper-agents");
        assert!(result.code_announcement_event_id.is_some());
        assert!(result.code_seed_commit_sha.is_some(), "{result:?}");
        assert!(result.code_seed_error.is_some(), "the code push failed");
        assert!(!result.code_seed_skipped);
        assert!(result.seed_commit_sha.is_some(), "{result:?}");
        assert_eq!(result.roles, vec!["builder", "lead"]);
        assert!(!result.pushed);
        assert!(result.push_error.is_some());
        assert_eq!(
            result.seeded_actions_yml, None,
            "an unpushed seed offers nothing to publish"
        );
        assert!(result.agents_announcement_withdrawn_event_id.is_some());
        assert!(
            result.agents_announcement_event_id.is_some(),
            "the announcement landed; its withdrawal is reported beside it"
        );
        assert!(result.source_event_id.is_none());
        assert!(
            result.checkout_path.is_none(),
            "no clone over an unseeded repository"
        );
        assert!(recorded.is_empty());
        assert!(!result.complete);
        assert!(
            result
                .gap
                .as_deref()
                .is_some_and(|gap| gap.contains("code repository demo not seeded")),
            "{:?}",
            result.gap
        );
        // 30617 (code) and its creation ref state, 30617 (agents) and its,
        // 5 (withdrawal) — and nothing else.
        assert_eq!(kinds_stored(&stored), vec![30617, 30618, 30617, 30618, 5]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Against a stub that serves git: both repositories are seeded and
    /// pushed (the relay's push records are there afterwards), the source
    /// is set, the code repository is cloned under the given parent as
    /// `<slug>` with the README on `main`, the clone is recorded, and the
    /// roster step publishes exactly one 9010 naming every project agent.
    #[tokio::test]
    async fn a_run_seeds_both_repositories_clones_the_code_checkout_and_records_it() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let (relay_url, stored) = spawn_stub_relay(
            vec![project_head_json(&keys, "demo", "Demo Project")],
            None,
            Some(root.join("git")),
        )
        .await;
        let state = stubbed_state(relay_url.clone(), keys.clone()).await;

        let (result, recorded) = run_init(
            &state,
            &project,
            catalog,
            root.join("cache"),
            root.join("repos"),
            None,
        )
        .await;

        assert!(result.code_announcement_event_id.is_some(), "{result:?}");
        assert!(result.code_seed_commit_sha.is_some(), "{result:?}");
        assert_eq!(result.code_seed_error, None, "{result:?}");
        assert!(!result.code_seed_skipped);
        assert!(result.pushed, "{result:?}");
        assert!(result.push_record_event_id.is_some());
        assert!(result.source_event_id.is_some());
        // Ledger 248: the pushed seed's actions.yml comes back byte for byte,
        // an active verify that setup publishes next.
        assert_eq!(
            result.seeded_actions_yml.as_deref(),
            Some(buzz_persona_pkg::seed::seeded_actions_yml().as_str())
        );
        let expected = root.join("repos").join("demo");
        assert_eq!(
            result.checkout_path.as_deref(),
            Some(expected.display().to_string().as_str())
        );
        assert!(result.checkout_cloned);
        assert_eq!(result.checkout_error, None);
        assert_eq!(
            recorded,
            vec![expected.clone()],
            "recorded as the project's folder"
        );
        // A normal clone: origin is the relay URL, `main` is checked out,
        // and the README names the project and the agents repository.
        let auth = build_test_git_auth_config().expect("auth");
        let origin =
            run_git(&["remote", "get-url", "origin"], Some(&expected), &auth).expect("origin");
        assert_eq!(origin.trim(), format!("{relay_url}/git/{viewer}/demo"));
        let branch =
            run_git(&["branch", "--show-current"], Some(&expected), &auth).expect("branch");
        assert_eq!(branch.trim(), "main");
        let head = run_git(&["rev-parse", "HEAD"], Some(&expected), &auth).expect("head");
        assert_eq!(Some(head.trim()), result.code_seed_commit_sha.as_deref());
        let readme = std::fs::read_to_string(expected.join("README.md")).expect("readme");
        assert_eq!(
            readme,
            "# Demo Project\n\nThis project's code. Roles and plans live in `demo-beekeeper-agents`.\n"
        );
        // The relay holds a push record for each repository, beside the
        // `HEAD`-only record each announcement produced.
        let records = pushed_records(&stored);
        assert_eq!(records.len(), 2, "{records:?}");
        assert!(result.complete, "{result:?}");
        assert_eq!(result.gap, None);
        assert_eq!(
            kinds_stored(&stored),
            vec![30617, 30618, 30618, 30617, 30618, 30618, 30624]
        );

        // Step 7 — the roster: every project agent, one 9010, as collaborators.
        let agents = vec!["a".repeat(64), "b".repeat(64)];
        let outcome = ensure_project_agents_on_roster(&state, &keys, &project, &agents).await;
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.added, agents);
        assert!(outcome.event_id.is_some());
        let puts = stored_of_kind(&stored, 9010);
        assert_eq!(puts.len(), 1, "exactly one 9010");
        assert_eq!(
            p_tags(&puts[0]),
            vec![
                vec![
                    "p".to_string(),
                    "a".repeat(64),
                    String::new(),
                    "collaborator".to_string()
                ],
                vec![
                    "p".to_string(),
                    "b".repeat(64),
                    String::new(),
                    "collaborator".to_string()
                ],
            ]
        );
        assert_eq!(
            puts[0]["tags"][0],
            serde_json::json!(["a", project]),
            "scoped to the project"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// Finish setup after a run that landed: what exists is reused — the
    /// push records skip both seeds, the recorded checkout is kept
    /// (`checkoutCloned=false`), nothing is recorded again, and a roster
    /// that already names the agents gets no second 9010.
    #[tokio::test]
    async fn a_rerun_reuses_the_checkout_skips_both_seeds_and_publishes_no_second_9010() {
        let root = scratch_root();
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let (relay_url, stored) = spawn_stub_relay(
            vec![project_head_json(&keys, "demo", "Demo Project")],
            None,
            Some(root.join("git")),
        )
        .await;
        let state = stubbed_state(relay_url, keys.clone()).await;
        let (first, recorded_first) = run_init(
            &state,
            &project,
            catalog(&root),
            root.join("cache"),
            root.join("repos"),
            None,
        )
        .await;
        assert!(first.complete, "{first:?}");
        let checkout = PathBuf::from(first.checkout_path.clone().expect("cloned"));
        assert_eq!(recorded_first, vec![checkout.clone()]);
        let agents = vec!["a".repeat(64), "b".repeat(64)];
        let outcome = ensure_project_agents_on_roster(&state, &keys, &project, &agents).await;
        assert_eq!(outcome.added, agents);
        let stored_after_first = stored.lock().unwrap().len();
        // The relay projects the accepted op into a kind:39010.
        stored.lock().unwrap().push(roster_projection_json(
            &project,
            &[
                (&"a".repeat(64), "collaborator"),
                (&"b".repeat(64), "collaborator"),
            ],
        ));

        let (second, recorded_second) = run_init(
            &state,
            &project,
            catalog(&root),
            root.join("cache"),
            root.join("repos"),
            Some(checkout.clone()),
        )
        .await;

        assert!(second.code_repo_existed && second.agents_repo_existed);
        assert!(second.code_announcement_event_id.is_none());
        assert!(second.agents_announcement_event_id.is_none());
        assert!(second.code_seed_skipped, "{second:?}");
        assert_eq!(second.code_seed_commit_sha, None);
        assert!(second.seed_skipped);
        assert!(second.pushed);
        assert!(second.source_existed);
        assert!(second.source_event_id.is_none());
        assert_eq!(
            second.checkout_path.as_deref(),
            Some(checkout.display().to_string().as_str())
        );
        assert!(!second.checkout_cloned, "reused, not cloned");
        assert_eq!(second.checkout_error, None);
        assert!(recorded_second.is_empty(), "already recorded");
        assert!(second.complete, "{second:?}");
        let outcome = ensure_project_agents_on_roster(&state, &keys, &project, &agents).await;
        assert_eq!(outcome.error, None);
        assert!(outcome.added.is_empty(), "{outcome:?}");
        assert_eq!(outcome.event_id, None);
        assert_eq!(stored_of_kind(&stored, 9010).len(), 1, "no second 9010");
        assert_eq!(
            stored.lock().unwrap().len(),
            stored_after_first + 1,
            "the rerun published nothing but the projection this test added"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// A clone that cannot land — the parent is a file — leaves the project
    /// created: both seeds pushed, the source set, `checkoutError` in the
    /// clone's own words, nothing recorded, `gap` naming the checkout.
    #[tokio::test]
    async fn a_clone_that_fails_leaves_the_project_created_with_the_checkout_error() {
        let root = scratch_root();
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let (relay_url, stored) = spawn_stub_relay(Vec::new(), None, Some(root.join("git"))).await;
        let state = stubbed_state(relay_url, keys).await;
        let parent = root.join("not-a-folder");
        std::fs::write(&parent, "a file where the repos folder should be").expect("file");

        let (result, recorded) = run_init(
            &state,
            &project,
            catalog(&root),
            root.join("cache"),
            parent.clone(),
            None,
        )
        .await;

        assert!(
            result.code_seed_commit_sha.is_some() && result.code_seed_error.is_none(),
            "{result:?}"
        );
        assert!(result.pushed, "{result:?}");
        assert!(result.source_event_id.is_some(), "{result:?}");
        assert_eq!(result.checkout_path, None, "{result:?}");
        assert!(!result.checkout_cloned);
        let error = result
            .checkout_error
            .clone()
            .expect("the clone's own words");
        assert!(error.contains(&parent.display().to_string()), "{error}");
        assert!(recorded.is_empty());
        assert!(!result.complete);
        assert!(
            result
                .gap
                .as_deref()
                .is_some_and(|gap| gap.contains("not checked out as the project's folder")),
            "{:?}",
            result.gap
        );
        assert_eq!(
            kinds_stored(&stored),
            vec![30617, 30618, 30618, 30617, 30618, 30618, 30624]
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// A recorded folder that is not a checkout of `<slug>` is refused by
    /// name — both paths — and left untouched; nothing else is held back.
    #[tokio::test]
    async fn a_recorded_folder_that_is_not_a_checkout_is_refused_by_name() {
        let root = scratch_root();
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let (relay_url, stored) = spawn_stub_relay(Vec::new(), None, Some(root.join("git"))).await;
        let state = stubbed_state(relay_url, keys).await;
        let elsewhere = root.join("tankloop");
        std::fs::create_dir_all(&elsewhere).expect("dir");
        std::fs::write(elsewhere.join("keep.txt"), "someone's work").expect("file");

        let (result, recorded) = run_init(
            &state,
            &project,
            catalog(&root),
            root.join("cache"),
            root.join("repos"),
            Some(elsewhere.clone()),
        )
        .await;

        assert!(
            result.pushed && result.source_event_id.is_some(),
            "{result:?}"
        );
        assert_eq!(result.checkout_path, None, "{result:?}");
        assert!(!result.checkout_cloned);
        let error = result.checkout_error.clone().expect("refused");
        assert!(error.contains(&elsewhere.display().to_string()), "{error}");
        assert!(
            error.contains(&root.join("repos").join("demo").display().to_string()),
            "{error}"
        );
        assert!(error.contains("nothing was overwritten"), "{error}");
        assert!(recorded.is_empty(), "the record is not changed");
        assert!(
            !root.join("repos").join("demo").exists(),
            "no clone beside it"
        );
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("keep.txt")).unwrap(),
            "someone's work"
        );
        assert!(!result.complete);
        assert_eq!(stored_of_kind(&stored, 30624).len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    /// An id another key already announced refuses the whole command before
    /// anything is signed.
    #[tokio::test]
    async fn an_id_taken_by_another_owner_refuses_before_anything_is_signed() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let other = Keys::generate();
        let (relay_url, stored) = spawn_stub_relay(
            vec![announcement_json(&other, "demo-beekeeper-agents")],
            None,
            None,
        )
        .await;
        let state = stubbed_state(relay_url, keys).await;

        let mut record = |_: &Path| -> Result<(), String> { Ok(()) };
        let error = project_agents_init_with_paths(
            &state,
            format!("30621:{viewer}:demo"),
            catalog,
            root.join("cache"),
            ProjectAgentsInitOptions {
                verify_command: buzz_persona_pkg::seed::default_verify_command(),
                checkout_parent: root.join("repos"),
                recorded_checkout: None,
                git_auth: |_: &Keys| build_test_git_auth_config(),
                migrate: None,
            },
            &mut record,
        )
        .await
        .expect_err("refused");
        assert!(error.contains("demo-beekeeper-agents"), "{error}");
        assert!(error.contains(&other.public_key().to_hex()[..8]), "{error}");
        assert!(error.contains("nothing was changed"), "{error}");
        assert!(kinds_stored(&stored).is_empty(), "nothing was signed");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Finish setup with nothing on disk: what the viewer already announced
    /// is reused, not re-announced; push records on the relay skip both
    /// seeds; the only thing published is the missing 30624; the checkout
    /// is cloned from the relay's copy.
    #[tokio::test]
    async fn a_rerun_reuses_what_exists_and_publishes_only_what_is_missing() {
        let root = scratch_root();
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        // Both announcements by the viewer and both push records, the code
        // repository already holding a commit on the stub's git server.
        let (relay_url, stored) = spawn_stub_relay(
            vec![
                announcement_json(&keys, "demo"),
                announcement_json(&keys, "demo-beekeeper-agents"),
                push_record_json("demo", &"1".repeat(40)),
                push_record_json("demo-beekeeper-agents", &"2".repeat(40)),
            ],
            None,
            Some(root.join("git")),
        )
        .await;
        // Off the runtime thread: the stub answers on it, so a blocking push
        // here would wait on itself.
        let upstream = root.join("upstream");
        let push_url = format!("{relay_url}/git/{viewer}/demo");
        let push_from = upstream.clone();
        tokio::task::spawn_blocking(move || {
            let mut auth = build_test_git_auth_config().expect("auth");
            auth.set_commit_identity("Test".to_string(), "test@beekeeper.local".to_string());
            seed_code_checkout(&push_from, "Demo", "demo-beekeeper-agents", &auth).expect("seed");
            run_git(
                &["push", "--quiet", "--", &push_url, "HEAD:refs/heads/main"],
                Some(&push_from),
                &auth,
            )
            .expect("push");
        })
        .await
        .expect("push task");
        let state = stubbed_state(relay_url, keys).await;

        let (result, recorded) = run_init(
            &state,
            &project,
            catalog(&root),
            root.join("cache"),
            root.join("repos"),
            None,
        )
        .await;

        assert!(result.code_repo_existed && result.agents_repo_existed);
        assert!(result.code_announcement_event_id.is_none());
        assert!(result.agents_announcement_event_id.is_none());
        assert!(result.code_seed_skipped, "{result:?}");
        assert!(result.seed_skipped, "{result:?}");
        assert!(result.pushed, "{result:?}");
        assert!(result.push_record_event_id.is_some(), "{result:?}");
        assert!(result.source_event_id.is_some(), "{result:?}");
        let expected = root.join("repos").join("demo");
        assert_eq!(
            result.checkout_path.as_deref(),
            Some(expected.display().to_string().as_str())
        );
        assert!(result.checkout_cloned);
        assert_eq!(recorded, vec![expected]);
        assert!(result.complete, "{result:?}");
        assert!(result.gap.is_none());
        // The test's own push stored one 30618; the run published only the 30624.
        assert_eq!(kinds_stored(&stored), vec![30618, 30624]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Ledger 176: the relay writes a `HEAD`-only kind:30618 when a
    /// repository is announced, before any push. A code repository that
    /// exists with only that record has no commits, so the seed must run —
    /// the first Finish repository setup on RPG Test skipped it as "already
    /// had commits" and recorded an unborn clone.
    #[tokio::test]
    async fn a_creation_only_ref_state_does_not_skip_the_code_seed() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let (relay_url, stored) = spawn_stub_relay(
            vec![
                project_head_json(&keys, "demo", "Demo Project"),
                announcement_json(&keys, "demo"),
                creation_record_json("demo"),
            ],
            None,
            Some(root.join("git")),
        )
        .await;
        let state = stubbed_state(relay_url, keys).await;

        let (result, recorded) = run_init(
            &state,
            &project,
            catalog,
            root.join("cache"),
            root.join("repos"),
            None,
        )
        .await;

        assert!(result.code_repo_existed);
        assert!(!result.code_seed_skipped, "{result:?}");
        assert!(result.code_seed_commit_sha.is_some(), "{result:?}");
        assert_eq!(result.code_seed_error, None);
        let expected = root.join("repos").join("demo");
        assert_eq!(recorded, vec![expected.clone()]);
        let auth = build_test_git_auth_config().expect("auth");
        let head = run_git(&["rev-parse", "HEAD"], Some(&expected), &auth).expect("head");
        assert_eq!(
            Some(head.trim()),
            result.code_seed_commit_sha.as_deref(),
            "the clone is born on the seed"
        );
        assert_eq!(pushed_records(&stored).len(), 2);
        assert!(result.complete, "{result:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Ledger 177: a recorded folder that is a checkout of `<slug>` cut
    /// while the repository was still empty (item 176's run left exactly
    /// that) is not "already checked out": the run must fetch the seeded
    /// `main` into it, so the next `git worktree add` has a commit.
    #[tokio::test]
    async fn a_recorded_unborn_clone_is_brought_to_main_by_the_seed() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let (relay_url, _stored) = spawn_stub_relay(
            vec![
                project_head_json(&keys, "demo", "Demo Project"),
                announcement_json(&keys, "demo"),
                creation_record_json("demo"),
            ],
            None,
            Some(root.join("git")),
        )
        .await;
        let state = stubbed_state(relay_url.clone(), keys).await;
        // The clone the first run left: `origin` on the relay, no commit.
        let unborn = root.join("repos").join("demo");
        std::fs::create_dir_all(&unborn).expect("dir");
        let auth = build_test_git_auth_config().expect("auth");
        run_git(
            &["init", "--quiet", "--initial-branch", SEED_BRANCH],
            Some(&unborn),
            &auth,
        )
        .expect("init");
        run_git(
            &[
                "remote",
                "add",
                "origin",
                &format!("{relay_url}/git/{viewer}/demo"),
            ],
            Some(&unborn),
            &auth,
        )
        .expect("remote");
        assert!(run_git(&["rev-parse", "--verify", "HEAD"], Some(&unborn), &auth).is_err());

        let (result, recorded) = run_init(
            &state,
            &project,
            catalog,
            root.join("cache"),
            root.join("repos"),
            Some(unborn.clone()),
        )
        .await;

        assert!(!result.code_seed_skipped, "{result:?}");
        assert!(result.code_seed_commit_sha.is_some(), "{result:?}");
        assert_eq!(result.checkout_error, None, "{result:?}");
        assert!(!result.checkout_cloned, "reused, not cloned");
        assert_eq!(
            result.checkout_path.as_deref(),
            Some(unborn.display().to_string().as_str())
        );
        assert!(recorded.is_empty(), "already recorded; not re-recorded");
        let head = run_git(&["rev-parse", "HEAD"], Some(&unborn), &auth).expect("head");
        assert_eq!(
            Some(head.trim()),
            result.code_seed_commit_sha.as_deref(),
            "the recorded clone now holds the seed"
        );
        let branch = run_git(&["branch", "--show-current"], Some(&unborn), &auth).expect("branch");
        assert_eq!(branch.trim(), SEED_BRANCH);
        assert!(result.complete, "{result:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// A project whose pack source already names another repository is
    /// refused: re-pointing is a deliberate act, and the refusal now names
    /// the control that performs one (ledger 243).
    #[tokio::test]
    async fn a_project_pointed_elsewhere_is_refused() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let elsewhere =
            build_agents_pack_source(&keys, &project, &format!("30617:{viewer}:shared-packs"))
                .expect("source");
        let (relay_url, stored) = spawn_stub_relay(vec![event_json(&elsewhere)], None, None).await;
        let state = stubbed_state(relay_url, keys).await;
        let mut record = |_: &Path| -> Result<(), String> { Ok(()) };
        let error = project_agents_init_with_paths(
            &state,
            project,
            catalog,
            root.join("cache"),
            ProjectAgentsInitOptions {
                verify_command: buzz_persona_pkg::seed::default_verify_command(),
                checkout_parent: root.join("repos"),
                recorded_checkout: None,
                git_auth: |_: &Keys| build_test_git_auth_config(),
                migrate: None,
            },
            &mut record,
        )
        .await
        .expect_err("refused");
        assert!(error.contains("shared-packs"), "{error}");
        assert!(error.contains("set-source"), "{error}");
        assert!(kinds_stored(&stored).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
