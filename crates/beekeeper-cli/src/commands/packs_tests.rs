//! `bee packs` argument handling — the parts that decide without a relay —
//! plus, at the bottom, LANE-L31's rollback: `withdraw_repo_announcement` and
//! `cmd_init`'s use of it, exercised against a localhost stub relay (never a
//! real one).

use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

/// `status --role` composes the cached role the way the host stages it and
/// says so without writing: a pack directory, then a flat file, then a
/// disclosed absence; an include this catalog cannot resolve is a refusal
/// with the composer's reason, never a silent bare persona.
#[test]
fn status_composes_the_cached_role_and_discloses_what_it_cannot() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cache = tmp.path().join("cache");
    let write = |rel: &str, body: &str| {
        let path = cache.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, body).expect("write");
    };
    // A pack directory.
    write(
        "personas/roles/builder/.plugin/plugin.json",
        r#"{"id":"t.builder","name":"builder","version":"0.1.0","personas":["personas/builder.persona.md"]}"#,
    );
    write(
        "personas/roles/builder/personas/builder.persona.md",
        "---\nname: builder\ndisplay_name: Builder\ndescription: Builds.\nrole: builder\n---\nYou build.\n",
    );
    // A flat file that includes a shipped template.
    write(
        "beekeeper/roles/lead.md",
        "![[beekeeper/memory@latest]]\nYou lead.\n",
    );
    // A catalog that ships it.
    let templates = tmp.path().join("templates");
    std::fs::create_dir_all(templates.join("memory/1.0.0")).expect("mkdir");
    std::fs::write(
        templates.join("memory/1.0.0/TEMPLATE.md"),
        "---\nname: memory\nversion: 1.0.0\ndescription: m\n---\nRemember.\n",
    )
    .expect("template");

    let pack = compose_status(&cache, "personas/roles", "builder", None);
    assert_eq!(pack["ok"], true, "{pack}");
    assert_eq!(pack["layout"], "pack");
    assert!(pack["digest"]
        .as_str()
        .is_some_and(|d| d.starts_with("sha256:")));
    assert!(pack["note"]
        .as_str()
        .is_some_and(|n| n.contains("BEEKEEPER_TEMPLATES_DIR")));

    let flat = compose_status(&cache, "beekeeper", "lead", Some(&templates));
    assert_eq!(flat["ok"], true, "{flat}");
    assert_eq!(flat["layout"], "flat");
    assert_eq!(flat["includes"][0]["ref"], "beekeeper/memory@latest");
    assert_eq!(flat["includes"][0]["resolved"], "1.0.0");
    assert_eq!(flat["note"], "");

    let refused = compose_status(&cache, "beekeeper", "lead", None);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(refused["reason"]
        .as_str()
        .is_some_and(|r| r.contains("no template catalog")));

    let absent = compose_status(&cache, "beekeeper", "runner", None);
    assert_eq!(absent["ok"], false);
    assert!(absent["layout"].is_null());
    assert!(absent["reason"]
        .as_str()
        .is_some_and(|r| r.contains("beekeeper/roles/runner.md")));
    assert!(
        !tmp.path().join("cache/staged").exists(),
        "status writes nothing"
    );
}

/// Exactly one pin, and the refusal says which flags to use.
#[test]
fn set_source_requires_exactly_one_pin() {
    let both = PackSourcePin {
        ref_name: Some("refs/heads/main"),
        sha: Some(&"a".repeat(40)),
    };
    let error = both.resolve().expect_err("both pins refused");
    assert!(format!("{error}").contains("exactly one"), "{error}");

    let neither = PackSourcePin {
        ref_name: None,
        sha: None,
    };
    let error = neither.resolve().expect_err("no pin refused");
    assert!(format!("{error}").contains("exactly one"), "{error}");

    assert_eq!(
        PackSourcePin {
            ref_name: Some("refs/heads/main"),
            sha: None,
        }
        .resolve()
        .expect("a ref"),
        PackPin::Ref("refs/heads/main".to_string())
    );
    assert_eq!(
        PackSourcePin {
            ref_name: None,
            sha: Some(&"A".repeat(40)),
        }
        .resolve()
        .expect("a sha"),
        PackPin::Sha("A".repeat(40)),
        "the pin is normalized by the builder, not here"
    );
}

/// A `--project` that is not a project coordinate is refused at exit 1, with
/// the shape it wanted printed back.
#[test]
fn a_project_argument_must_be_a_project_coordinate() {
    let error = normalize_project("agiterra").expect_err("refused");
    assert!(
        format!("{error}").contains("30621:<64-hex>:<slug>"),
        "{error}"
    );
    let error = normalize_project(&format!("30617:{OWNER}:repo")).expect_err("refused");
    assert!(format!("{error}").contains("30621"), "{error}");
    assert_eq!(
        normalize_project(&format!("  30621:{}:agiterra ", OWNER.to_ascii_uppercase()))
            .expect("normalizes"),
        format!("30621:{OWNER}:agiterra"),
        "the coordinate is case-folded and trimmed like every other reader's"
    );
}

/// The role directory probe reads a real directory and stays empty — never
/// erroring — for a path this machine has never fetched.
#[test]
fn role_directories_reads_what_is_there_and_nothing_when_it_is_not() {
    let root = std::env::temp_dir().join(format!("bee-packs-{}", uuid::Uuid::new_v4().simple()));
    let packs = root.join("personas/roles");
    std::fs::create_dir_all(packs.join("builder")).expect("mkdir");
    std::fs::create_dir_all(packs.join("lead")).expect("mkdir");
    std::fs::write(packs.join("README.md"), "not a role").expect("write");

    assert_eq!(
        role_directories(&packs),
        vec!["builder".to_string(), "lead".to_string()],
        "only directories count, and they are sorted"
    );
    assert!(
        role_directories(&root.join("never/fetched")).is_empty(),
        "a path this machine never fetched is empty, not an error"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// `--from` must name a real directory, and without it the nearest
/// `personas/roles` is found by walking up — never invented.
#[test]
fn the_seed_directory_is_resolved_or_refused_by_name() {
    let missing = std::env::temp_dir().join(format!("bee-nope-{}", uuid::Uuid::new_v4().simple()));
    let error = resolve_seed_dir(Some(&missing)).expect_err("refused");
    assert!(format!("{error}").contains("is not a directory"), "{error}");

    let root = std::env::temp_dir().join(format!("bee-seed-{}", uuid::Uuid::new_v4().simple()));
    let roles = root.join("personas/roles/builder");
    std::fs::create_dir_all(&roles).expect("mkdir");
    assert_eq!(
        resolve_seed_dir(Some(&root.join("personas/roles"))).expect("a directory"),
        root.join("personas/roles")
    );
    std::fs::remove_dir_all(&root).ok();
}

/// The seed step builds one signed commit holding the packs under the record's
/// path and pushes it — exercised against a throwaway bare repository, never a
/// worktree of this repository.
#[test]
fn seeding_writes_the_packs_under_the_path_and_pushes_one_signed_commit() {
    let root = std::env::temp_dir().join(format!("bee-seedrun-{}", uuid::Uuid::new_v4().simple()));
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("builder")).expect("mkdir");
    std::fs::create_dir_all(seed.join("lead")).expect("mkdir");
    std::fs::write(seed.join("builder/PERSONA.md"), "you build\n").expect("write");
    std::fs::write(seed.join("lead/PERSONA.md"), "you lead\n").expect("write");

    let remote = root.join("remote.git");
    let init = crate::commands::sessions::worktree::git_command(&root)
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(&remote)
        .output()
        .expect("git init --bare");
    assert!(init.status.success(), "bare init: {init:?}");

    let seeded = seed_packs_repository(
        &seed,
        "personas/roles",
        remote.to_str().expect("utf8 remote path"),
    )
    .expect("seeding succeeds against a throwaway bare repository");
    assert_eq!(seeded.pushed_ref, "refs/heads/main");
    assert_eq!(seeded.commit.len(), 40, "a full commit id, never a prefix");

    let listing = crate::commands::sessions::worktree::git_command(&remote)
        .args(["ls-tree", "-r", "--name-only", "refs/heads/main"])
        .output()
        .expect("ls-tree");
    let files = String::from_utf8_lossy(&listing.stdout);
    assert!(
        files.contains("personas/roles/builder/PERSONA.md")
            && files.contains("personas/roles/lead/PERSONA.md"),
        "the packs must land under the record's path: {files}"
    );

    let message = crate::commands::sessions::worktree::git_command(&remote)
        .args(["log", "-1", "--format=%B", "refs/heads/main"])
        .output()
        .expect("git log");
    let body = String::from_utf8_lossy(&message.stdout);
    assert!(
        body.contains("Signed-off-by:"),
        "the seed commit must carry the DCO trailer: {body}"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// The flat layout seeds at the repository root (`path: "."`): the files
/// land at the top of the tree, not under a `./` directory. Found live on
/// 2026-09-21: `create_dir_all("<tmp>/.")` failed before the work directory
/// existed, so every `bee projects create` and flat `bee packs init` died
/// at the seed step.
#[test]
fn seeding_at_the_root_path_lands_the_files_at_the_top_of_the_tree() {
    let root = std::env::temp_dir().join(format!("bee-seedroot-{}", uuid::Uuid::new_v4().simple()));
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("roles")).expect("mkdir");
    std::fs::write(
        seed.join("team.yml"),
        "schema: beekeeper-team/v1\nversion: '1'\n",
    )
    .expect("write");
    std::fs::write(seed.join("roles/lead.md"), "you lead\n").expect("write");

    let remote = root.join("remote.git");
    let init = crate::commands::sessions::worktree::git_command(&root)
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(&remote)
        .output()
        .expect("git init --bare");
    assert!(init.status.success(), "bare init: {init:?}");

    seed_packs_repository(&seed, ".", remote.to_str().expect("utf8 remote path"))
        .expect("seeding at the root succeeds");

    let listing = crate::commands::sessions::worktree::git_command(&remote)
        .args(["ls-tree", "-r", "--name-only", "refs/heads/main"])
        .output()
        .expect("ls-tree");
    let files = String::from_utf8_lossy(&listing.stdout);
    let mut names: Vec<&str> = files.lines().collect();
    names.sort();
    assert_eq!(names, ["roles/lead.md", "team.yml"], "{files}");
    std::fs::remove_dir_all(&root).ok();
}

// --- finding 135(e): the packs cache must name which fact resolved it ---

/// Serializes tests that mutate `BEEKEEPER_MANAGED_AGENT` — `std::env::set_var`
/// races across threads otherwise (this suite runs tests in parallel by
/// default), and this env var is not touched by any other test in the crate.
static MANAGED_AGENT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// No `BEEKEEPER_MANAGED_AGENT`: the identifier is the hard-coded release guess,
/// and the source is `Default` — never asserted silently as a confirmed path.
#[test]
fn no_env_marker_falls_back_to_the_release_identifier_and_says_so() {
    let _guard = MANAGED_AGENT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("BEEKEEPER_MANAGED_AGENT");
    let (identifier, source) = resolve_app_identifier();
    assert_eq!(identifier, APP_IDENTIFIER);
    assert_eq!(source, CacheDirSource::Default);
    assert_eq!(source.as_str(), "default");
}

/// `BEEKEEPER_MANAGED_AGENT` is the fact the desktop host stamps on every process
/// it spawns for a seat (`current_instance_id`/`buzz_marker_entry` in
/// `desktop/src-tauri/src/managed_agents/runtime/process.rs`); a dev bundle's
/// value must be used verbatim, not folded into the release identifier.
#[test]
fn env_marker_names_the_running_instance_and_is_preferred_over_the_guess() {
    let _guard = MANAGED_AGENT_ENV_LOCK.lock().unwrap();
    std::env::set_var("BEEKEEPER_MANAGED_AGENT", "io.agiterra.beekeeper.app.dev");
    let (identifier, source) = resolve_app_identifier();
    std::env::remove_var("BEEKEEPER_MANAGED_AGENT");
    assert_eq!(identifier, "io.agiterra.beekeeper.app.dev");
    assert_eq!(source, CacheDirSource::Env);
    assert_eq!(source.as_str(), "env");
}

/// A blank `BEEKEEPER_MANAGED_AGENT` (unset-but-exported, or explicitly cleared to
/// empty) is not a fact either — treat it the same as absent rather than
/// deriving a cache path from an empty directory name.
#[test]
fn a_blank_env_marker_is_not_treated_as_a_fact() {
    let _guard = MANAGED_AGENT_ENV_LOCK.lock().unwrap();
    std::env::set_var("BEEKEEPER_MANAGED_AGENT", "   ");
    let (identifier, source) = resolve_app_identifier();
    std::env::remove_var("BEEKEEPER_MANAGED_AGENT");
    assert_eq!(identifier, APP_IDENTIFIER);
    assert_eq!(source, CacheDirSource::Default);
}

/// `default_packs_dir` composes the identifier it resolved into the same
/// `<platform data dir>/<identifier>/packs` shape the desktop host writes to,
/// and carries the source forward so `cmd_status` can disclose it.
#[test]
fn default_packs_dir_names_the_dev_cache_when_the_env_marker_says_so() {
    let _guard = MANAGED_AGENT_ENV_LOCK.lock().unwrap();
    std::env::set_var("BEEKEEPER_MANAGED_AGENT", "io.agiterra.beekeeper.app.dev");
    let (dir, source) = default_packs_dir().expect("HOME is set in this environment");
    std::env::remove_var("BEEKEEPER_MANAGED_AGENT");
    assert_eq!(source, CacheDirSource::Env);
    let dir_str = dir.display().to_string();
    assert!(
        dir_str.contains("io.agiterra.beekeeper.app.dev") && dir_str.ends_with("packs"),
        "{dir_str}"
    );
}

// --- LANE-L31: `bee packs init` rolls back a seed/push that never lands ---

mod rollback {
    use super::*;
    use axum::{http::StatusCode, routing::post, Router};
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    /// A stub relay recording every event kind it was handed. `reject_kind`
    /// answers that one kind with HTTP 500 (used to make the tombstone
    /// publish itself fail); every other kind is accepted. No `/git/...`
    /// route exists, so any push against this stub fails fast.
    async fn spawn_stub_relay(reject_kind: Option<u64>) -> (String, Arc<Mutex<Vec<u64>>>) {
        let received: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let received_for_route = received.clone();
        let app = Router::new()
            .route(
                "/events",
                post(move |body: String| {
                    let received = received_for_route.clone();
                    async move {
                        let event: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        let kind = event
                            .get("kind")
                            .and_then(Value::as_u64)
                            .unwrap_or_default();
                        received.lock().unwrap().push(kind);
                        if Some(kind) == reject_kind {
                            return (StatusCode::INTERNAL_SERVER_ERROR, String::new());
                        }
                        (
                            StatusCode::OK,
                            serde_json::json!({
                                "event_id": event.get("id").and_then(Value::as_str).unwrap_or(""),
                                "accepted": true,
                                "message": ""
                            })
                            .to_string(),
                        )
                    }
                }),
            )
            // `cmd_init` reads any existing pack source before announcing —
            // this stub always answers "none", so the create path proceeds.
            .route(
                "/query",
                post(|| async { (StatusCode::OK, "[]".to_string()) }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), received)
    }

    fn test_client(relay_url: String) -> BeekeeperClient {
        let keys = nostr::Keys::generate();
        BeekeeperClient::new(relay_url, keys, None, None).expect("client construction")
    }

    #[tokio::test]
    async fn withdraw_repo_announcement_publishes_a_kind_5_naming_the_coordinate() {
        let (relay_url, received) = spawn_stub_relay(None).await;
        let client = test_client(relay_url);
        let owner = client.keys().public_key().to_hex();

        let (withdrawn, error) =
            withdraw_repo_announcement(&client, &owner, "beekeeper-packs").await;
        assert!(error.is_none(), "{error:?}");
        let withdrawn = withdrawn.expect("a tombstone event id");
        assert_eq!(withdrawn.len(), 64, "a full event id, never a prefix");
        assert_eq!(*received.lock().unwrap(), vec![5]);
    }

    #[tokio::test]
    async fn a_relays_refusal_of_the_tombstone_is_reported_not_swallowed() {
        let (relay_url, _received) = spawn_stub_relay(Some(5)).await;
        let client = test_client(relay_url);
        let owner = client.keys().public_key().to_hex();

        let (withdrawn, error) =
            withdraw_repo_announcement(&client, &owner, "beekeeper-packs").await;
        assert!(withdrawn.is_none(), "the relay refused it");
        assert!(
            error.is_some(),
            "the refusal must be reported, not silently dropped"
        );
    }

    // A third test drove `cmd_init` itself end to end (announce against the
    // stub, then a push with nowhere to land) to prove the 30617→kind:5
    // sequence at the command level, not just in `withdraw_repo_announcement`
    // directly. Removed: `seed_packs_repository` shells a bare `git push`
    // (`commands::sessions::worktree::git_command`) with none of
    // `project_git_exec::configure_git_auth`'s hardening — no
    // `GIT_TERMINAL_PROMPT=0`, no credential-helper isolation, no timeout —
    // so on this development machine (which has a real
    // `credential.helper` wired for Nostr pushes, per CONTRIBUTING.md) the
    // push against the stub triggered the *real* credential helper and hung
    // past 60s; killed rather than let run. `withdraw_repo_announcement`'s
    // own two tests above cover the new logic without shelling git against a
    // relay this suite does not control. Flagged separately (not this
    // lane's scope): `buzz-cli`'s git shelling should get the same
    // `GIT_TERMINAL_PROMPT=0` / timeout hardening `project_git_exec.rs`
    // already gives the desktop host.
}
