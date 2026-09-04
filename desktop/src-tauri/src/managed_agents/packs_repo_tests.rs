//! Tests for creating a project's packs repository.
//!
//! Every git operation here runs against **throwaway repositories under this
//! crate's own `target/` scratch directory**, never inside a worktree of this
//! repository: a test that shells `git` in the checkout it was launched from
//! will one day write to it.

use super::*;
use crate::commands::project_git_exec::{
    build_git_auth_config_for_keys, build_test_git_auth_config,
};
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

// --- Finding 66: the packs seed commit has no identity in production ---
//
// Red, reproduced outside this suite (a throwaway repo, the exact env this
// crate's own `configure_git_auth` sets — `GIT_CONFIG_GLOBAL=/dev/null`,
// `GIT_CONFIG_NOSYSTEM=1`, no author env — and this host's own hostname,
// which has no dot):
//
//   $ git init --quiet --initial-branch main && git add -A
//   $ env -u GIT_AUTHOR_NAME -u GIT_AUTHOR_EMAIL -u GIT_COMMITTER_NAME \
//       -u GIT_COMMITTER_EMAIL GIT_CONFIG_GLOBAL=/dev/null \
//       GIT_CONFIG_NOSYSTEM=1 git commit --quiet -m seed
//   Author identity unknown
//
//   *** Please tell me who you are.
//   ...
//   fatal: unable to auto-detect email address (got 'brian@MacBookPro.(none)')
//
// verbatim what Brian's screen printed at 21:09. Production's
// `project_packs_init` ran exactly this shape (`build_git_auth_config_for_keys`,
// `commit_identity: None`, same as every other production caller before this
// lane) with no way to name an identity of its own. Green below: the same
// checkout, the same cleared config, with `set_commit_identity` — no ambient
// identity of any kind, and the commit still lands, authored as the app.

#[test]
fn the_display_name_resolves_from_the_kind_0_and_falls_back_to_the_pubkey() {
    let pubkey = "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";
    assert_eq!(
        app_commit_identity_from_profile(pubkey, Some(r#"{"display_name":"Waggle Bot"}"#)),
        (
            "Waggle Bot".to_string(),
            "aa11bb22@beekeeper.local".to_string()
        ),
        "display_name wins when present"
    );
    assert_eq!(
        app_commit_identity_from_profile(pubkey, Some(r#"{"name":"waggle"}"#)),
        ("waggle".to_string(), "aa11bb22@beekeeper.local".to_string()),
        "name is the fallback, same order nostr_convert::profile_info_from_event uses"
    );
    assert_eq!(
        app_commit_identity_from_profile(pubkey, Some(r#"{"display_name":"   "}"#)),
        (
            "Beekeeper aa11bb22".to_string(),
            "aa11bb22@beekeeper.local".to_string()
        ),
        "a blank display_name is not a name"
    );
    assert_eq!(
        app_commit_identity_from_profile(pubkey, None),
        (
            "Beekeeper aa11bb22".to_string(),
            "aa11bb22@beekeeper.local".to_string()
        ),
        "no profile at all falls back the same way"
    );
    assert_eq!(
        app_commit_identity_from_profile(pubkey, Some("not json")),
        (
            "Beekeeper aa11bb22".to_string(),
            "aa11bb22@beekeeper.local".to_string()
        ),
        "malformed content must not panic or block the seed"
    );
}

#[test]
fn the_seed_commit_is_authored_as_the_app_identity_with_no_ambient_config_at_all() {
    let root = scratch_root();
    let shipped = shipped_packs(&root);
    let keys = Keys::generate();
    let mut auth = build_git_auth_config_for_keys(&keys).expect("git auth");
    auth.set_commit_identity("Waggle Bot", "aa11bb22@beekeeper.local");
    let checkout = root.join("cache/aa11bb22-beekeeper-packs");

    // The exact conditions the live failure ran under (see the module doc
    // above): global and system git config cleared, this host's own
    // hostname (no dot). Only `set_commit_identity` differs.
    let (commit, _roles) =
        seed_packs_checkout(&checkout, &shipped, &auth).expect("seed must succeed");
    assert_eq!(commit.len(), 40, "{commit}");
    let author =
        run_git(&["log", "-1", "--format=%an <%ae>"], Some(&checkout), &auth).expect("log");
    assert_eq!(author.trim(), "Waggle Bot <aa11bb22@beekeeper.local>");
    std::fs::remove_dir_all(&root).ok();
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

// --- Finding 66: rollback — a seed or push failure withdraws the 30617 ---
//
// Gated off Windows for the same reason `persona_events::tests::flush_barrier`
// is: `build_app_state()` pulls native DLLs unavailable on the Windows CI
// runner. Hermetic otherwise — a localhost axum stand-in for the relay, never
// the real one.
#[cfg(not(target_os = "windows"))]
mod rollback_and_identity {
    use super::*;
    use crate::app_state::build_app_state;

    /// Stub relay: `POST /events` accepts everything except a kind this test
    /// tells it to reject (used to make the tombstone publish itself fail);
    /// `POST /query` answers a kind:0 lookup with `profile_content` when
    /// given, and an empty array otherwise (covering both the identity read
    /// and the kind:30618 push-record read this command also makes). No
    /// `/git/...` route exists, so a `git push` against this stub always
    /// fails fast — exactly the "push does not land" half of the rollback
    /// rule, without a real git smart-HTTP server to stand up.
    async fn spawn_stub_relay(
        profile_event_json: Option<String>,
        reject_kind: Option<u64>,
    ) -> String {
        use axum::{http::StatusCode, routing::post, Router};

        let app = Router::new()
            .route(
                "/events",
                post(move |body: String| async move {
                    let event: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    if Some(event.get("kind").and_then(serde_json::Value::as_u64).unwrap_or_default())
                        == reject_kind
                    {
                        return (StatusCode::INTERNAL_SERVER_ERROR, String::new());
                    }
                    (
                        StatusCode::OK,
                        serde_json::json!({
                            "event_id": event.get("id").and_then(serde_json::Value::as_str).unwrap_or(""),
                            "accepted": true,
                            "message": ""
                        })
                        .to_string(),
                    )
                }),
            )
            .route(
                "/query",
                post(move |body: String| {
                    let profile_event_json = profile_event_json.clone();
                    async move {
                        let filters: Vec<serde_json::Value> =
                            serde_json::from_str(&body).unwrap_or_default();
                        let wants_profile = filters.iter().any(|filter| {
                            filter
                                .get("kinds")
                                .and_then(serde_json::Value::as_array)
                                .is_some_and(|kinds| {
                                    kinds.iter().any(|k| k.as_u64() == Some(0))
                                })
                        });
                        let body = match (wants_profile, &profile_event_json) {
                            (true, Some(event_json)) => format!("[{event_json}]"),
                            _ => "[]".to_string(),
                        };
                        (StatusCode::OK, body)
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
        format!("http://{addr}")
    }

    fn signed_profile_json(keys: &Keys, display_name: &str) -> String {
        use nostr::JsonUtil;
        EventBuilder::new(
            Kind::Metadata,
            serde_json::json!({ "display_name": display_name }).to_string(),
        )
        .sign_with_keys(keys)
        .expect("sign profile")
        .as_json()
    }

    async fn stubbed_state(relay_url: String, keys: Keys) -> AppState {
        let state = build_app_state();
        *state.keys.lock().unwrap() = keys;
        *state.relay_url_override.lock().unwrap() = Some(relay_url);
        state
    }

    /// The seed commit is authored as the display name this host's own
    /// kind:0 carries — read off the stub relay, never fabricated by the
    /// test and never git config.
    #[tokio::test]
    async fn the_seed_commit_carries_the_identity_this_host_read_from_its_own_profile() {
        let root = scratch_root();
        let shipped = shipped_packs(&root);
        let keys = Keys::generate();
        let profile = signed_profile_json(&keys, "Waggle Bot");
        let relay_url = spawn_stub_relay(Some(profile), None).await;
        let state = stubbed_state(relay_url, keys).await;

        let result = project_packs_init_with_paths(
            &state,
            "30621:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:beekeeper"
                .to_string(),
            Some("beekeeper-packs".to_string()),
            None,
            shipped,
            root.join("cache"),
        )
        .await
        .expect("project_packs_init_with_paths");

        assert_eq!(result.commit_identity_name, "Waggle Bot");
        assert!(result.commit_identity_email.ends_with("@beekeeper.local"));
        assert_eq!(result.seed_commit_sha.as_deref().map(str::len), Some(40));
        std::fs::remove_dir_all(&root).ok();
    }

    /// The push cannot land against a stub with no git route — the rollback
    /// case: the 30617 already landed, so its withdrawal (kind:5) must be
    /// published, and reported.
    #[tokio::test]
    async fn a_push_that_never_lands_withdraws_the_announcement_it_already_published() {
        let root = scratch_root();
        let shipped = shipped_packs(&root);
        let keys = Keys::generate();
        let relay_url = spawn_stub_relay(None, None).await;
        let state = stubbed_state(relay_url, keys).await;

        let result = project_packs_init_with_paths(
            &state,
            "30621:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:beekeeper"
                .to_string(),
            Some("beekeeper-packs".to_string()),
            None,
            shipped,
            root.join("cache"),
        )
        .await
        .expect("project_packs_init_with_paths");

        assert!(
            !result.pushed,
            "no git route on the stub, so the push fails"
        );
        assert!(result.push_error.is_some());
        assert!(
            result.source_event_id.is_none(),
            "no 30624 is published for a repository the push never filled"
        );
        assert!(
            result.announcement_withdrawn_event_id.is_some(),
            "the 30617 that already landed must be withdrawn: {result:?}"
        );
        assert!(result.announcement_withdrawal_error.is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    /// A seed that never produces a commit at all (no role packs to seed
    /// from) is the same rollback rule, reached the other way in.
    #[tokio::test]
    async fn a_seed_that_never_produces_a_commit_also_withdraws_the_announcement() {
        let root = scratch_root();
        // No role packs in here at all — `seed_packs_checkout` refuses
        // before ever reaching git.
        let empty_shipped = root.join("empty-shipped");
        std::fs::create_dir_all(&empty_shipped).expect("empty shipped dir");
        let keys = Keys::generate();
        let relay_url = spawn_stub_relay(None, None).await;
        let state = stubbed_state(relay_url, keys).await;

        let result = project_packs_init_with_paths(
            &state,
            "30621:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:beekeeper"
                .to_string(),
            Some("beekeeper-packs".to_string()),
            None,
            empty_shipped,
            root.join("cache"),
        )
        .await
        .expect("project_packs_init_with_paths");

        assert!(result.seed_commit_sha.is_none());
        assert!(result.seed_error.is_some(), "{result:?}");
        assert!(!result.pushed);
        assert!(result.announcement_withdrawn_event_id.is_some());
        std::fs::remove_dir_all(&root).ok();
    }

    /// The tombstone publish can itself fail (relay down, refused, …). That
    /// must be reported — never silently swallowed — and the coordinate the
    /// function already returns (`repo_ref`) is what a founder deletes by
    /// hand, per the module docs.
    #[tokio::test]
    async fn a_failed_withdrawal_is_reported_with_the_coordinate_still_in_hand() {
        let root = scratch_root();
        let shipped = shipped_packs(&root);
        let keys = Keys::generate();
        // Reject kind:5 (the tombstone) — the announcement (30617) still
        // lands, and the push still fails against the git-less stub.
        let relay_url = spawn_stub_relay(None, Some(5)).await;
        let state = stubbed_state(relay_url, keys).await;

        let result = project_packs_init_with_paths(
            &state,
            "30621:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:beekeeper"
                .to_string(),
            Some("beekeeper-packs".to_string()),
            None,
            shipped,
            root.join("cache"),
        )
        .await
        .expect("project_packs_init_with_paths");

        assert!(!result.pushed);
        assert!(
            result.announcement_withdrawn_event_id.is_none(),
            "the relay refused the tombstone"
        );
        assert!(result.announcement_withdrawal_error.is_some());
        // The coordinate a founder needs to delete it by hand is still
        // right here, regardless of the tombstone's own failure.
        assert!(
            result.repo_ref.starts_with("30617:") && result.repo_ref.ends_with(":beekeeper-packs"),
            "{}",
            result.repo_ref
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
