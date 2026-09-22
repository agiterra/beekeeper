//! The stub relay the `agents_repo` tests run against: `POST /events`,
//! `POST /query`, and — when given a git root — git smart HTTP over
//! `git http-backend`, so a seed's push and the checkout's clone actually
//! land, and the relay's kind:30618 push record is synthesized the way the
//! real relay does it. Plus the event and run helpers the tests share.
//! Split from `agents_repo_tests.rs` for the file-size gate.

use super::*;
use crate::app_state::build_app_state;
use crate::commands::project_git_exec::build_test_git_auth_config;
use crate::managed_agents::packs_repo::KIND_REPO_REF_STATE;
use nostr::JsonUtil;
use std::sync::{Arc, Mutex};

/// Every event the stub relay was asked to store, in order — including
/// the kind:30618 push records the stub's git server synthesizes, as
/// the real relay does after a push.
pub(super) type Stored = Arc<Mutex<Vec<serde_json::Value>>>;

/// `POST /events` stores everything (or refuses `reject_kind`);
/// `POST /query` answers from `seeded` plus what was stored, filtering on
/// `kinds` and `#d`. With `git_root`, `/git/<owner>/<id>/...` is git smart
/// HTTP over `git http-backend` (bare repositories created on first
/// touch, anonymous push allowed), and a push that lands stores a
/// kind:30618 for the id. Without it there is no `/git/...` route, so a
/// push always fails fast.
pub(super) async fn spawn_stub_relay(
    seeded: Vec<serde_json::Value>,
    reject_kind: Option<u64>,
    git_root: Option<PathBuf>,
) -> (String, Stored) {
    use axum::{
        http::StatusCode,
        routing::{any, post},
        Router,
    };
    let stored: Stored = Arc::new(Mutex::new(Vec::new()));
    let events_store = stored.clone();
    let query_store = stored.clone();
    let mut app = Router::new()
        .route(
            EVENTS_ROUTE,
            post(move |body: String| {
                let store = events_store.clone();
                async move {
                    let event: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
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
                    // The real relay writes a `HEAD`-only kind:30618 the
                    // moment a repository is announced (ledger 176); so
                    // does this stub, so a seed that trusts any record
                    // with the right `d` is caught here.
                    let creation = (event.get("kind").and_then(serde_json::Value::as_u64)
                        == Some(u64::from(KIND_REPO_ANNOUNCEMENT)))
                    .then(|| {
                        event
                            .get("tags")
                            .and_then(serde_json::Value::as_array)
                            .and_then(|tags| {
                                tags.iter().find_map(|tag| {
                                    (tag.get(0).and_then(serde_json::Value::as_str) == Some("d"))
                                        .then(|| tag.get(1).and_then(serde_json::Value::as_str))
                                        .flatten()
                                })
                            })
                            .map(creation_record_json)
                    })
                    .flatten();
                    let mut store = store.lock().unwrap();
                    store.push(event);
                    store.extend(creation);
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
                                let d_ok =
                                    match filter.get("#d").and_then(serde_json::Value::as_array) {
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
    if let Some(root) = git_root {
        let git_store = stored.clone();
        app = app.route(
            "/git/{owner}/{id}/{*rest}",
            any(
                move |path: axum::extract::Path<(String, String, String)>,
                      query: axum::extract::RawQuery,
                      method: axum::http::Method,
                      headers: axum::http::HeaderMap,
                      body: axum::body::Bytes| {
                    let root = root.clone();
                    let store = git_store.clone();
                    async move {
                        let (owner, id, rest) = path.0;
                        let (status, headers, body) = tokio::task::spawn_blocking(move || {
                            git_smart_http(
                                &root, &store, &owner, &id, &rest, &query.0, &method, &headers,
                                &body,
                            )
                        })
                        .await
                        .unwrap_or_else(|_| {
                            (StatusCode::INTERNAL_SERVER_ERROR, Vec::new(), Vec::new())
                        });
                        let mut response = axum::response::Response::builder().status(status);
                        for (name, value) in headers {
                            response = response.header(name, value);
                        }
                        response
                            .body(axum::body::Body::from(body))
                            .unwrap_or_else(|_| {
                                axum::response::Response::new(axum::body::Body::empty())
                            })
                    }
                },
            ),
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub relay");
    let addr = listener.local_addr().expect("stub relay addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    (format!("http://{addr}"), stored)
}

pub(super) fn hygienic_git() -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

/// One git smart-HTTP request answered by `git http-backend` as CGI. The
/// bare repository is created on first touch, with anonymous push
/// allowed; a receive-pack that leaves `refs/heads/main` behind stores
/// a kind:30618 for the id (once), the relay's push record.
#[allow(clippy::too_many_arguments)]
pub(super) fn git_smart_http(
    root: &Path,
    store: &Stored,
    owner: &str,
    id: &str,
    rest: &str,
    query: &Option<String>,
    method: &axum::http::Method,
    headers: &axum::http::HeaderMap,
    body: &[u8],
) -> (axum::http::StatusCode, Vec<(String, String)>, Vec<u8>) {
    use axum::http::StatusCode;
    use std::io::{Read, Write};
    let bare = root.join(owner).join(id);
    if !bare.exists() {
        std::fs::create_dir_all(&bare).expect("bare dir");
        let init = hygienic_git()
            .args(["init", "--quiet", "--bare", "--initial-branch", SEED_BRANCH])
            .current_dir(&bare)
            .status()
            .expect("git init --bare");
        assert!(init.success(), "bare init");
        let allow = hygienic_git()
            .args(["config", "http.receivepack", "true"])
            .current_dir(&bare)
            .status()
            .expect("git config");
        assert!(allow.success(), "receivepack config");
    }
    let mut command = hygienic_git();
    command
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", format!("/{owner}/{id}/{rest}"))
        .env("REQUEST_METHOD", method.as_str())
        .env("QUERY_STRING", query.clone().unwrap_or_default())
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("REMOTE_USER", "stub")
        .env("REMOTE_ADDR", "127.0.0.1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(content_type) = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        command.env("CONTENT_TYPE", content_type);
    }
    if let Some(encoding) = headers
        .get(axum::http::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
    {
        command.env("HTTP_CONTENT_ENCODING", encoding);
    }
    let mut child = command.spawn().expect("git http-backend");
    let mut stdin = child.stdin.take().expect("stdin");
    let body = body.to_vec();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&body).ok();
    });
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_end(&mut output)
        .expect("read cgi output");
    writer.join().ok();
    child.wait().ok();
    let split = output
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| (at, at + 4))
        .or_else(|| {
            output
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|at| (at, at + 2))
        });
    let Some((headers_end, body_start)) = split else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Vec::new(), output);
    };
    let header_text = String::from_utf8_lossy(&output[..headers_end]).to_string();
    let mut status = StatusCode::OK;
    let mut response_headers = Vec::new();
    for line in header_text.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("status") {
            status = value
                .split_whitespace()
                .next()
                .and_then(|code| code.parse::<u16>().ok())
                .and_then(|code| StatusCode::from_u16(code).ok())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        } else {
            response_headers.push((name.trim().to_string(), value.to_string()));
        }
    }
    if rest == "git-receive-pack" && status.is_success() {
        let landed = hygienic_git()
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/heads/{SEED_BRANCH}"),
            ])
            .env("GIT_DIR", &bare)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
        if let Some(sha) = landed {
            let mut stored = store.lock().unwrap();
            // A creation record (`HEAD` only) does not count as the push.
            let already = stored.iter().any(|event| {
                event.get("kind").and_then(serde_json::Value::as_u64)
                    == Some(u64::from(KIND_REPO_REF_STATE))
                    && event
                        .get("tags")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|tags| {
                            tags.iter().any(|tag| {
                                tag.get(0).and_then(serde_json::Value::as_str) == Some("d")
                                    && tag.get(1).and_then(serde_json::Value::as_str) == Some(id)
                            }) && tags.iter().any(|tag| {
                                tag.get(0).and_then(serde_json::Value::as_str)
                                    == Some(&format!("refs/heads/{SEED_BRANCH}"))
                            })
                        })
            });
            if !already {
                stored.push(push_record_json(id, &sha));
            }
        }
    }
    (status, response_headers, output[body_start..].to_vec())
}

pub(super) async fn stubbed_state(relay_url: String, keys: Keys) -> AppState {
    let state = build_app_state();
    *state.keys.lock().unwrap() = keys;
    *state.relay_url_override.lock().unwrap() = Some(relay_url);
    state
}

pub(super) fn event_json(event: &nostr::Event) -> serde_json::Value {
    serde_json::from_str(&event.as_json()).expect("json")
}

pub(super) fn announcement_json(keys: &Keys, repo_id: &str) -> serde_json::Value {
    let event = EventBuilder::new(Kind::Custom(KIND_REPO_ANNOUNCEMENT), "")
        .tags(vec![
            Tag::parse(vec!["d".to_string(), repo_id.to_string()]).unwrap()
        ])
        .sign_with_keys(keys)
        .expect("sign");
    event_json(&event)
}

/// The relay-signed kind:30618 for `repo_id`, as the stub's git server
/// stores it after a push: `d`, the pushed `main`, and `HEAD`.
pub(super) fn push_record_json(repo_id: &str, sha: &str) -> serde_json::Value {
    let event = EventBuilder::new(Kind::Custom(KIND_REPO_REF_STATE), "")
        .tags(vec![
            Tag::parse(vec!["d".to_string(), repo_id.to_string()]).unwrap(),
            Tag::parse(vec![format!("refs/heads/{SEED_BRANCH}"), sha.to_string()]).unwrap(),
            Tag::parse(vec![
                "HEAD".to_string(),
                format!("ref: refs/heads/{SEED_BRANCH}"),
            ])
            .unwrap(),
        ])
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    event_json(&event)
}

/// The relay-signed kind:30618 the relay writes when a repository is
/// announced, before any push: `d` and `HEAD` only, no branch (ledger 176).
pub(super) fn creation_record_json(repo_id: &str) -> serde_json::Value {
    let event = EventBuilder::new(Kind::Custom(KIND_REPO_REF_STATE), "")
        .tags(vec![
            Tag::parse(vec!["d".to_string(), repo_id.to_string()]).unwrap(),
            Tag::parse(vec![
                "HEAD".to_string(),
                format!("ref: refs/heads/{SEED_BRANCH}"),
            ])
            .unwrap(),
        ])
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    event_json(&event)
}

/// The kind:30618 records in `stored` that name a pushed `main`.
pub(super) fn pushed_records(stored: &Stored) -> Vec<serde_json::Value> {
    stored_of_kind(stored, u64::from(KIND_REPO_REF_STATE))
        .into_iter()
        .filter(|event| {
            event
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tags| {
                    tags.iter().any(|tag| {
                        tag.get(0).and_then(serde_json::Value::as_str)
                            == Some(&format!("refs/heads/{SEED_BRANCH}"))
                    })
                })
        })
        .collect()
}

/// The project's kind:30621 head by `keys`, named.
pub(super) fn project_head_json(keys: &Keys, slug: &str, name: &str) -> serde_json::Value {
    let event = EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), "")
        .tags(vec![
            Tag::parse(vec!["d".to_string(), slug.to_string()]).unwrap(),
            Tag::parse(vec!["name".to_string(), name.to_string()]).unwrap(),
        ])
        .sign_with_keys(keys)
        .expect("sign");
    event_json(&event)
}

/// A project head naming `repo_id` as one of its repositories, the shape a
/// project created before the pivot has.
pub(super) fn project_head_with_repo_json(
    keys: &Keys,
    slug: &str,
    name: &str,
    repo_id: &str,
) -> serde_json::Value {
    let viewer = keys.public_key().to_hex();
    let event = EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), "")
        .tags(vec![
            Tag::parse(vec!["d".to_string(), slug.to_string()]).unwrap(),
            Tag::parse(vec!["name".to_string(), name.to_string()]).unwrap(),
            Tag::parse(vec!["a".to_string(), format!("30617:{viewer}:{repo_id}")]).unwrap(),
        ])
        .sign_with_keys(keys)
        .expect("sign");
    event_json(&event)
}

/// [`push_pack_layout_repo`], off the test's runtime thread.
///
/// The stub relay serves git from an axum handler on the *same* runtime a
/// `#[tokio::test]` gives the test body — a current-thread one. Pushing
/// straight from the body therefore blocks the only thread that could
/// answer the push, and the test hangs forever rather than failing.
pub(super) async fn push_legacy_repo(
    work: PathBuf,
    relay_url: String,
    owner: String,
    repo_id: &str,
    path: &str,
    roles: &[&str],
) {
    let repo_id = repo_id.to_owned();
    let path = path.to_owned();
    let roles: Vec<String> = roles.iter().map(|role| (*role).to_owned()).collect();
    tokio::task::spawn_blocking(move || {
        let roles: Vec<&str> = roles.iter().map(String::as_str).collect();
        push_pack_layout_repo(&work, &relay_url, &owner, &repo_id, &path, &roles);
    })
    .await
    .expect("seeding the legacy repository did not finish");
}

/// Build a pack-layout role source in a scratch directory and push it to
/// the stub relay's git server as `repo_id` — what a project created before
/// the pivot points at.
pub(super) fn push_pack_layout_repo(
    work: &Path,
    relay_url: &str,
    owner: &str,
    repo_id: &str,
    path: &str,
    roles: &[&str],
) {
    let auth = {
        let mut auth = build_test_git_auth_config().expect("auth");
        auth.set_commit_identity("Test".to_string(), "test@beekeeper.local".to_string());
        auth
    };
    for role in roles {
        let dir = work.join(path).join(role);
        std::fs::create_dir_all(dir.join(".plugin")).expect("plugin dir");
        std::fs::write(
            dir.join(".plugin/plugin.json"),
            format!(
                "{{\n  \"id\": \"com.beekeeper.crew.{role}\",\n  \"name\": \"{role}\",\n  \"version\": \"0.4.0\",\n  \"personas\": [\"personas/{role}.persona.md\"]\n}}\n"
            ),
        )
        .expect("manifest");
        std::fs::create_dir_all(dir.join("personas")).expect("personas dir");
        std::fs::write(
            dir.join(format!("personas/{role}.persona.md")),
            format!(
                "---\nname: {role}\nrole: {role}\ndisplay_name: \"{role}\"\ndescription: \"The project's own {role}.\"\nskills:\n  - \"./skills/{role}-skill/\"\n---\nThis is the project's own {role}, written long before the pivot.\n"
            ),
        )
        .expect("persona");
        let skill = dir.join("skills").join(format!("{role}-skill"));
        std::fs::create_dir_all(&skill).expect("skill dir");
        std::fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {role}-skill\ndescription: the {role}'s own skill\n---\nDo the {role} thing.\n"),
        )
        .expect("skill");
    }
    run_git(
        &["init", "--quiet", "--initial-branch", "main"],
        Some(work),
        &auth,
    )
    .expect("init");
    run_git(&["add", "--all"], Some(work), &auth).expect("add");
    run_git(
        &["commit", "--quiet", "-m", "the project's roles"],
        Some(work),
        &auth,
    )
    .expect("commit");
    run_git(
        &[
            "push",
            "--quiet",
            "--",
            &format!("{relay_url}/git/{owner}/{repo_id}"),
            "HEAD:refs/heads/main",
        ],
        Some(work),
        &auth,
    )
    .expect("push the legacy repository");
}

/// A relay-signed kind:39010 roster projection for `coordinate`.
pub(super) fn roster_projection_json(
    coordinate: &str,
    members: &[(&str, &str)],
) -> serde_json::Value {
    let mut tags = vec![Tag::parse(vec!["d".to_string(), coordinate.to_string()]).unwrap()];
    for (pubkey, role) in members {
        tags.push(
            Tag::parse(vec![
                "p".to_string(),
                (*pubkey).to_string(),
                String::new(),
                (*role).to_string(),
            ])
            .unwrap(),
        );
    }
    let event = EventBuilder::new(Kind::Custom(39010), "")
        .tags(tags)
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    event_json(&event)
}

pub(super) fn kinds_stored(stored: &Stored) -> Vec<u64> {
    stored
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| event.get("kind").and_then(serde_json::Value::as_u64))
        .collect()
}

pub(super) fn stored_of_kind(stored: &Stored, kind: u64) -> Vec<serde_json::Value> {
    stored
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.get("kind").and_then(serde_json::Value::as_u64) == Some(kind))
        .cloned()
        .collect()
}

pub(super) fn p_tags(event: &serde_json::Value) -> Vec<Vec<String>> {
    event
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter(|tag| tag.get(0).and_then(serde_json::Value::as_str) == Some("p"))
                .map(|tag| {
                    tag.as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Run the init with the test git config, capturing what it records.
pub(super) async fn run_init(
    state: &AppState,
    project: &str,
    catalog: TemplateCatalog,
    packs_root: PathBuf,
    checkout_parent: PathBuf,
    recorded_checkout: Option<PathBuf>,
) -> (ProjectAgentsInit, Vec<PathBuf>) {
    run_init_migrating(
        state,
        project,
        catalog,
        packs_root,
        checkout_parent,
        recorded_checkout,
        None,
    )
    .await
}

/// [`run_init`], asking for a migration off the named source event.
pub(super) async fn run_init_migrating(
    state: &AppState,
    project: &str,
    catalog: TemplateCatalog,
    packs_root: PathBuf,
    checkout_parent: PathBuf,
    recorded_checkout: Option<PathBuf>,
    migrate: Option<MigrateFromSource>,
) -> (ProjectAgentsInit, Vec<PathBuf>) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let sink = recorded.clone();
    let mut record = move |path: &Path| -> Result<(), String> {
        sink.lock().unwrap().push(path.to_path_buf());
        Ok(())
    };
    let result = project_agents_init_with_paths(
        state,
        project.to_string(),
        catalog,
        packs_root,
        ProjectAgentsInitOptions {
            verify_command: buzz_persona_pkg::seed::default_verify_command(),
            checkout_parent,
            recorded_checkout,
            git_auth: |_: &Keys| build_test_git_auth_config(),
            migrate,
        },
        &mut record,
    )
    .await
    .expect("runs");
    let recorded = recorded.lock().unwrap().clone();
    (result, recorded)
}
