//! Tests for creating a project's repositories. Every git operation runs
//! against throwaway directories under this crate's own `target/`, and the
//! relay is a localhost stub — never the real one.

use super::*;
use std::path::PathBuf;

fn scratch_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("agents-repo-scratch")
        .join(format!(
            "run-{}-{}",
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
    let (commit, roles) =
        seed_agents_checkout(&checkout, &catalog, "demo", &auth).expect("seed commits");
    assert_eq!(commit.len(), 40);
    assert_eq!(roles, vec!["builder", "lead"]);
    for rel in [
        "team.yml",
        "actions.yml",
        "README.md",
        "roles/lead.md",
        "roles/archive/.gitkeep",
        "plans/archive/.gitkeep",
    ] {
        assert!(checkout.join(rel).is_file(), "{rel}");
    }
    let lead = std::fs::read_to_string(checkout.join("roles/lead.md")).expect("lead");
    assert!(lead.contains("![[beekeeper/lead@^1.0.0]]"));
    assert!(!lead.contains("You are the lead"), "referenced, not copied");
    // A retry clears and reseeds rather than layering a second copy.
    let (again, _) = seed_agents_checkout(&checkout, &catalog, "demo", &auth).expect("reseed");
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

// Gated off Windows for the same reason `packs_repo_tests` is: the stub
// state pulls native DLLs unavailable on the Windows CI runner.
#[cfg(not(target_os = "windows"))]
mod against_a_stub_relay {
    use super::*;
    use crate::app_state::build_app_state;
    use std::sync::{Arc, Mutex};

    /// Every event the stub relay was asked to store, in order.
    type Stored = Arc<Mutex<Vec<serde_json::Value>>>;

    /// `POST /events` stores everything (or refuses `reject_kind`);
    /// `POST /query` answers from `seeded` plus what was stored, filtering on
    /// `kinds` and `#d`. No `/git/...` route, so a push always fails fast.
    async fn spawn_stub_relay(
        seeded: Vec<serde_json::Value>,
        reject_kind: Option<u64>,
    ) -> (String, Stored) {
        use axum::{http::StatusCode, routing::post, Router};
        let stored: Stored = Arc::new(Mutex::new(Vec::new()));
        let events_store = stored.clone();
        let query_store = stored.clone();
        let app = Router::new()
            .route(
                "/events",
                post(move |body: String| {
                    let store = events_store.clone();
                    async move {
                        let event: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        if Some(
                            event
                                .get("kind")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or_default(),
                        ) == reject_kind
                        {
                            return (StatusCode::INTERNAL_SERVER_ERROR, String::new());
                        }
                        let id = event
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        store.lock().unwrap().push(event);
                        (
                            StatusCode::OK,
                            serde_json::json!({ "event_id": id, "accepted": true, "message": "" })
                                .to_string(),
                        )
                    }
                }),
            )
            .route(
                "/query",
                post(move |body: String| {
                    let seeded = seeded.clone();
                    let store = query_store.clone();
                    async move {
                        let filters: Vec<serde_json::Value> =
                            serde_json::from_str(&body).unwrap_or_default();
                        let mut all = seeded.clone();
                        all.extend(store.lock().unwrap().iter().cloned());
                        let matching: Vec<serde_json::Value> = all
                            .into_iter()
                            .filter(|event| {
                                filters.iter().any(|filter| {
                                    let kind_ok = filter
                                        .get("kinds")
                                        .and_then(serde_json::Value::as_array)
                                        .is_some_and(|kinds| {
                                            kinds.iter().any(|k| {
                                                k == event
                                                    .get("kind")
                                                    .unwrap_or(&serde_json::Value::Null)
                                            })
                                        });
                                    let d_ok = match filter
                                        .get("#d")
                                        .and_then(serde_json::Value::as_array)
                                    {
                                        None => true,
                                        Some(wanted) => event
                                            .get("tags")
                                            .and_then(serde_json::Value::as_array)
                                            .is_some_and(|tags| {
                                                tags.iter().any(|tag| {
                                                    tag.get(0).and_then(serde_json::Value::as_str)
                                                        == Some("d")
                                                        && wanted.contains(
                                                            tag.get(1).unwrap_or(
                                                                &serde_json::Value::Null,
                                                            ),
                                                        )
                                                })
                                            }),
                                    };
                                    kind_ok && d_ok
                                })
                            })
                            .collect();
                        (
                            StatusCode::OK,
                            serde_json::Value::Array(matching).to_string(),
                        )
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stub relay");
        let addr = listener.local_addr().expect("stub relay addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        (format!("http://{addr}"), stored)
    }

    async fn stubbed_state(relay_url: String, keys: Keys) -> AppState {
        let state = build_app_state();
        *state.keys.lock().unwrap() = keys;
        *state.relay_url_override.lock().unwrap() = Some(relay_url);
        state
    }

    fn announcement_json(keys: &Keys, repo_id: &str) -> serde_json::Value {
        use nostr::JsonUtil;
        let event = EventBuilder::new(Kind::Custom(KIND_REPO_ANNOUNCEMENT), "")
            .tags(vec![
                Tag::parse(vec!["d".to_string(), repo_id.to_string()]).unwrap()
            ])
            .sign_with_keys(keys)
            .expect("sign");
        serde_json::from_str(&event.as_json()).expect("json")
    }

    fn kinds_stored(stored: &Stored) -> Vec<u64> {
        stored
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| event.get("kind").and_then(serde_json::Value::as_u64))
            .collect()
    }

    /// The push cannot land against a git-less stub: both announcements
    /// land, the seed commits, the push fails, this run's agents
    /// announcement is withdrawn, the code announcement stands, no 30624 is
    /// published, and `gap` says what is missing.
    #[tokio::test]
    async fn a_push_that_never_lands_withdraws_the_agents_announcement_and_names_the_gap() {
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let (relay_url, stored) = spawn_stub_relay(Vec::new(), None).await;
        let state = stubbed_state(relay_url, keys).await;

        let result = project_agents_init_with_paths(
            &state,
            format!("30621:{viewer}:demo"),
            catalog,
            root.join("cache"),
        )
        .await
        .expect("runs");

        assert_eq!(result.code_repo_id, "demo");
        assert_eq!(result.agents_repo_id, "demo-beekeeper-agents");
        assert!(result.code_announcement_event_id.is_some());
        assert!(result.seed_commit_sha.is_some(), "{result:?}");
        assert_eq!(result.roles, vec!["builder", "lead"]);
        assert!(!result.pushed);
        assert!(result.push_error.is_some());
        assert!(result.agents_announcement_withdrawn_event_id.is_some());
        assert!(
            result.agents_announcement_event_id.is_some(),
            "the announcement landed; its withdrawal is reported beside it"
        );
        assert!(result.source_event_id.is_none());
        assert!(!result.complete);
        assert!(
            result
                .gap
                .as_deref()
                .is_some_and(|gap| gap.contains("not seeded")),
            "{:?}",
            result.gap
        );
        // 30617 (code), 30617 (agents), 5 (withdrawal) — and nothing else.
        assert_eq!(kinds_stored(&stored), vec![30617, 30617, 5]);
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
        )
        .await;
        let state = stubbed_state(relay_url, keys).await;

        let error = project_agents_init_with_paths(
            &state,
            format!("30621:{viewer}:demo"),
            catalog,
            root.join("cache"),
        )
        .await
        .expect_err("refused");
        assert!(error.contains("demo-beekeeper-agents"), "{error}");
        assert!(error.contains(&other.public_key().to_hex()[..8]), "{error}");
        assert!(error.contains("nothing was changed"), "{error}");
        assert!(kinds_stored(&stored).is_empty(), "nothing was signed");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Finish setup: what the viewer already announced is reused, not
    /// re-announced; a push record on the relay skips the seed; the only
    /// thing published is the missing 30624.
    #[tokio::test]
    async fn a_rerun_reuses_what_exists_and_publishes_only_what_is_missing() {
        use nostr::JsonUtil;
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        // Both announcements by the viewer, and the relay's push record for
        // the agents repository.
        let push_record = {
            let event = EventBuilder::new(Kind::Custom(30618), "")
                .tags(vec![Tag::parse(vec![
                    "d".to_string(),
                    "demo-beekeeper-agents".to_string(),
                ])
                .unwrap()])
                .sign_with_keys(&Keys::generate())
                .expect("sign");
            serde_json::from_str::<serde_json::Value>(&event.as_json()).expect("json")
        };
        let (relay_url, stored) = spawn_stub_relay(
            vec![
                announcement_json(&keys, "demo"),
                announcement_json(&keys, "demo-beekeeper-agents"),
                push_record,
            ],
            None,
        )
        .await;
        let state = stubbed_state(relay_url, keys).await;

        let result = project_agents_init_with_paths(
            &state,
            format!("30621:{viewer}:demo"),
            catalog,
            root.join("cache"),
        )
        .await
        .expect("runs");

        assert!(result.code_repo_existed && result.agents_repo_existed);
        assert!(result.code_announcement_event_id.is_none());
        assert!(result.agents_announcement_event_id.is_none());
        assert!(result.seed_skipped);
        assert!(result.pushed);
        assert!(result.push_record_event_id.is_some());
        assert!(result.source_event_id.is_some());
        assert!(result.complete, "{result:?}");
        assert!(result.gap.is_none());
        assert_eq!(kinds_stored(&stored), vec![30624]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A project whose pack source already names another repository is
    /// refused: re-pointing is a deliberate `set-source`.
    #[tokio::test]
    async fn a_project_pointed_elsewhere_is_refused() {
        use nostr::JsonUtil;
        let root = scratch_root();
        let catalog = catalog(&root);
        let keys = Keys::generate();
        let viewer = keys.public_key().to_hex();
        let project = format!("30621:{viewer}:demo");
        let elsewhere =
            build_agents_pack_source(&keys, &project, &format!("30617:{viewer}:shared-packs"))
                .expect("source");
        let (relay_url, stored) = spawn_stub_relay(
            vec![serde_json::from_str(&elsewhere.as_json()).expect("json")],
            None,
        )
        .await;
        let state = stubbed_state(relay_url, keys).await;
        let error = project_agents_init_with_paths(&state, project, catalog, root.join("cache"))
            .await
            .expect_err("refused");
        assert!(error.contains("shared-packs"), "{error}");
        assert!(error.contains("set-source"), "{error}");
        assert!(kinds_stored(&stored).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
