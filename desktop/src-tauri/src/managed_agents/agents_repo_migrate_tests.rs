//! Migrating a project created before the pivot (ledger 243).
//!
//! The same stub relay the sibling file uses, with one addition: a
//! pack-layout repository pushed to it first, standing in for the packs
//! repository a pre-pivot project points at. Split from
//! `agents_repo_tests.rs` for the file-size gate.

use super::stub::*;
use super::*;
use crate::commands::project_git_exec::build_test_git_auth_config;

/// A legacy pack-layout source, announced by the viewer, plus the head
/// that names the project's own code repository.
fn legacy_source(keys: &Keys, project: &str, repo_id: &str, path: &str) -> nostr::Event {
    let viewer = keys.public_key().to_hex();
    let content = serde_json::json!({ "schema": PROJECT_PACK_SOURCE_SCHEMA }).to_string();
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE), content)
        .tags(vec![
            Tag::parse(vec!["d".to_string(), project.to_string()]).unwrap(),
            Tag::parse(vec![
                "repo".to_string(),
                format!("30617:{viewer}:{repo_id}"),
            ])
            .unwrap(),
            Tag::parse(vec!["ref".to_string(), "refs/heads/main".to_string()]).unwrap(),
            Tag::parse(vec!["path".to_string(), path.to_string()]).unwrap(),
        ])
        .sign_with_keys(keys)
        .expect("sign")
}

/// The whole point of the migration arm: a project pointed at a
/// pack-layout repository gets an agents repository holding its own
/// roles, and the re-point is conditional on the source the caller saw.
#[tokio::test]
async fn a_migration_converts_the_projects_own_roles_and_re_points_conditionally() {
    let root = scratch_root();
    let catalog = catalog(&root);
    let keys = Keys::generate();
    let viewer = keys.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let source = legacy_source(&keys, &project, "shared-packs", "personas/roles");
    let (relay_url, stored) = spawn_stub_relay(
        vec![
            event_json(&source),
            project_head_with_repo_json(&keys, "demo", "Demo Project", "demo-code"),
        ],
        None,
        Some(root.join("git")),
    )
    .await;
    push_legacy_repo(
        root.join("legacy"),
        relay_url.clone(),
        viewer.clone(),
        "shared-packs",
        "personas/roles",
        &["lead", "runner"],
    )
    .await;
    let state = stubbed_state(relay_url.clone(), keys.clone()).await;

    let (result, _recorded) = run_init_migrating(
        &state,
        &project,
        catalog,
        root.join("cache"),
        root.join("repos"),
        None,
        Some(MigrateFromSource {
            expected_source_id: source.id.to_hex(),
            convert: true,
        }),
    )
    .await;

    assert!(result.pushed, "{result:?}");
    assert_eq!(
        result.migrated_from.as_deref(),
        Some(format!("30617:{viewer}:shared-packs").as_str()),
        "{result:?}"
    );
    assert_eq!(result.migrated_roles, vec!["lead", "runner"], "{result:?}");
    assert!(
        result.migration_notes.iter().any(|n| n.contains("lead")),
        "the dropped frontmatter is disclosed: {:?}",
        result.migration_notes
    );
    assert!(!result.source_conflict, "{result:?}");
    assert!(result.source_event_id.is_some(), "{result:?}");

    // The code repository is the one the head already names, not the
    // slug, so no second empty repository was announced.
    assert_eq!(result.code_repo_id, "demo-code");
    assert!(result.code_repo_adopted, "{result:?}");

    // The published 30624 is conditional on the source the caller saw.
    let published = stored
        .lock()
        .unwrap()
        .iter()
        .filter(|event| {
            event.get("kind").and_then(serde_json::Value::as_u64)
                == Some(u64::from(KIND_PROJECT_PACK_SOURCE))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(published.len(), 1, "{published:?}");
    let content = published[0]
        .get("content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        content.contains(&source.id.to_hex()),
        "the re-point names the source it replaces: {content}"
    );

    // The seeded repository holds the project's own text, not the
    // shipped templates'.
    let checkout =
        packs_cache::packs_checkout_dir(&root.join("cache"), &viewer, &result.agents_repo_id);
    let lead = std::fs::read_to_string(checkout.join("roles/lead.md")).expect("lead.md");
    assert!(
        lead.contains("written long before the pivot"),
        "the project's own words survive: {lead}"
    );
    assert!(
        checkout
            .join("roles/lead/skills/lead-skill/SKILL.md")
            .is_file(),
        "the role's skill came across"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// A migration is a decision about one record: if the source moved
/// since the caller read it, nothing is signed.
#[tokio::test]
async fn a_migration_naming_a_source_that_moved_refuses_before_anything_is_signed() {
    let root = scratch_root();
    let catalog = catalog(&root);
    let keys = Keys::generate();
    let viewer = keys.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let source = legacy_source(&keys, &project, "shared-packs", "personas/roles");
    let (relay_url, stored) =
        spawn_stub_relay(vec![event_json(&source)], None, Some(root.join("git"))).await;
    let state = stubbed_state(relay_url, keys).await;
    let mut record = |_: &Path| -> Result<(), String> { Ok(()) };
    let error = project_agents_init_with_paths(
        &state,
        project,
        catalog,
        root.join("cache"),
        ProjectAgentsInitOptions {
            verify_command: beekeeper_persona_pkg::seed::default_verify_command(),
            checkout_parent: root.join("repos"),
            recorded_checkout: None,
            git_auth: |_: &Keys| build_test_git_auth_config(),
            migrate: Some(MigrateFromSource {
                expected_source_id: "0".repeat(64),
                convert: true,
            }),
        },
        &mut record,
    )
    .await
    .expect_err("refused");
    assert!(error.contains(&source.id.to_hex()), "{error}");
    assert!(error.contains("read it again"), "{error}");
    assert!(kinds_stored(&stored).is_empty(), "nothing signed");
    std::fs::remove_dir_all(&root).ok();
}

/// A relay that refuses the conditional re-point leaves the new
/// repository standing and says the source moved, in those words.
#[tokio::test]
async fn a_refused_re_point_leaves_the_new_repository_standing_and_says_so() {
    let root = scratch_root();
    let catalog = catalog(&root);
    let keys = Keys::generate();
    let viewer = keys.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let source = legacy_source(&keys, &project, "shared-packs", "personas/roles");
    let (relay_url, _stored) = spawn_stub_relay(
        vec![event_json(&source)],
        Some(u64::from(KIND_PROJECT_PACK_SOURCE)),
        Some(root.join("git")),
    )
    .await;
    push_legacy_repo(
        root.join("legacy"),
        relay_url.clone(),
        viewer.clone(),
        "shared-packs",
        "personas/roles",
        &["lead"],
    )
    .await;
    let state = stubbed_state(relay_url, keys).await;

    let (result, _recorded) = run_init_migrating(
        &state,
        &project,
        catalog,
        root.join("cache"),
        root.join("repos"),
        None,
        Some(MigrateFromSource {
            expected_source_id: source.id.to_hex(),
            convert: true,
        }),
    )
    .await;

    assert!(result.pushed, "the repository was seeded: {result:?}");
    assert!(result.source_event_id.is_none(), "{result:?}");
    assert!(result.publication_error.is_some(), "{result:?}");
    assert!(!result.complete);
    assert!(
        result.agents_announcement_withdrawn_event_id.is_none(),
        "a refused re-point does not withdraw a repository that seeded: {result:?}"
    );
    std::fs::remove_dir_all(&root).ok();
}
