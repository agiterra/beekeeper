//! File-ref resolution and its refusals, against real directories in a
//! `TempDir`. Every directory here is created by the test that uses it.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{
    file_refs_from_loaded, git_toplevel, parse_file_ref_candidate, run_file_ref_action,
    CodingSessionFileRefActionRequest, CodingSessionFileRefScope, CodingSessionFileRefWhere,
    CodingSessionFileRefsRequest, FOLDER_GONE_REASON, MAX_FILE_REF_CANDIDATES, NOT_LOCAL_REASON,
};
use crate::coding_sessions::session_tree::CodingSessionTreeSource;
use crate::coding_sessions::workdir_store::{
    CodingSessionSeatWorktree, CodingSessionWorkdirEntry, CodingSessionWorkdirStore,
};

const SESSION_ID: &str = "2b0e6a52-6a8e-4f0a-9d1b-3f3c1c0d9a01";
const CHANNEL_ID: &str = "8f4d0a8e-0000-4000-8000-000000000001";
const PROJECT_REF: &str = "30621:abc:proj";
const CHECKED_AT: &str = "2026-10-07T00:00:00+00:00";

fn mkdir(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).expect("create dir");
    std::fs::canonicalize(&path).expect("canonicalize")
}

fn write(root: &Path, rel: &str) -> PathBuf {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(&path, b"x").expect("write file");
    std::fs::canonicalize(&path).expect("canonicalize")
}

fn worktree(path: &Path, session_id: &str) -> CodingSessionSeatWorktree {
    CodingSessionSeatWorktree {
        path: path.to_path_buf(),
        branch: "seat".to_string(),
        repo_root: path.to_path_buf(),
        created_at: "2026-10-07T00:00:00Z".to_string(),
        session_id: Some(session_id.to_string()),
        agents_clone: None,
        commit_identity: None,
        actor_pubkey: None,
        seeding: None,
    }
}

fn entry(path: &Path) -> CodingSessionWorkdirEntry {
    CodingSessionWorkdirEntry {
        path: path.to_path_buf(),
        updated_at: "2026-10-07T00:00:00Z".to_string(),
    }
}

fn scope(is_local_provider: bool, is_hired_seat: bool) -> CodingSessionFileRefScope {
    CodingSessionFileRefScope {
        channel_id: CHANNEL_ID.to_string(),
        provider_session_id: Some(SESSION_ID.to_string()),
        project_ref: Some(PROJECT_REF.to_string()),
        is_hired_seat,
        is_local_provider,
    }
}

fn lookup(
    store: &CodingSessionWorkdirStore,
    scope: CodingSessionFileRefScope,
    candidates: &[&str],
) -> super::CodingSessionFileRefsAnswer {
    let request = CodingSessionFileRefsRequest {
        scope,
        candidates: candidates.iter().map(|c| c.to_string()).collect(),
    };
    file_refs_from_loaded(Ok(store.clone()), &request, CHECKED_AT.to_string())
        .expect("lookup answers")
}

/// A repository with a session worktree at its root, and a project checkout.
struct Fixture {
    _temp: TempDir,
    session: PathBuf,
    project: PathBuf,
    store: CodingSessionWorkdirStore,
}

fn fixture() -> Fixture {
    let temp = TempDir::new().expect("tempdir");
    let session = mkdir(temp.path(), "session-tree");
    std::fs::write(session.join(".git"), b"gitdir: elsewhere").expect("worktree .git file");
    write(&session, "desktop/src/app/App.tsx");
    let project = mkdir(temp.path(), "project-checkout");
    write(&project, "README.md");
    let mut store = CodingSessionWorkdirStore::default();
    store
        .worktrees
        .insert("ref/seat".to_string(), worktree(&session, SESSION_ID));
    store
        .by_project
        .insert(PROJECT_REF.to_string(), entry(&project));
    Fixture {
        _temp: temp,
        session,
        project,
        store,
    }
}

#[test]
fn file_refs_session_worktree_is_this_computer_with_relative_path_and_position() {
    let fx = fixture();
    let answer = lookup(
        &fx.store,
        scope(false, true),
        &["desktop/src/app/App.tsx:42:7"],
    );
    assert_eq!(answer.where_, CodingSessionFileRefWhere::ThisComputer);
    assert_eq!(answer.source, Some(CodingSessionTreeSource::Session));
    assert_eq!(answer.reason, None);
    assert_eq!(answer.checked_at, CHECKED_AT);
    let found = &answer.refs["desktop/src/app/App.tsx:42:7"];
    assert!(found.exists);
    assert!(!found.is_dir);
    assert_eq!(
        found.relative_path.as_deref(),
        Some("desktop/src/app/App.tsx")
    );
    let expected = fx.session.join("desktop/src/app/App.tsx");
    assert_eq!(found.full_path.as_deref(), expected.to_str());
    assert_eq!((found.line, found.column), (Some(42), Some(7)));
}

#[test]
fn file_refs_folder_and_missing_file_are_told_apart() {
    let fx = fixture();
    let answer = lookup(
        &fx.store,
        scope(true, false),
        &["desktop/src/", "nope/gone.ts"],
    );
    let folder = &answer.refs["desktop/src/"];
    assert!(folder.exists && folder.is_dir);
    assert_eq!(folder.relative_path.as_deref(), Some("desktop/src"));
    let missing = &answer.refs["nope/gone.ts"];
    assert!(!missing.exists);
    assert_eq!(missing.full_path, None);
    assert_eq!(missing.relative_path, None);
}

#[test]
fn file_refs_project_default_only_when_local_and_not_hired() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    let local = lookup(&fx.store, scope(true, false), &["README.md"]);
    assert_eq!(local.where_, CodingSessionFileRefWhere::ThisComputer);
    assert_eq!(local.source, Some(CodingSessionTreeSource::Project));
    let readme = &local.refs["README.md"];
    assert_eq!(
        readme.full_path.as_deref(),
        fx.project.join("README.md").to_str()
    );

    let hired = lookup(&fx.store, scope(true, true), &["README.md"]);
    assert_eq!(hired.where_, CodingSessionFileRefWhere::NotRecorded);
    assert!(hired.refs.is_empty());
    assert!(hired.reason.as_deref().unwrap_or("").contains("hired seat"));

    let remote = lookup(&fx.store, scope(false, false), &["README.md"]);
    assert_eq!(remote.where_, CodingSessionFileRefWhere::NotLocal);
    assert_eq!(remote.reason.as_deref(), Some(NOT_LOCAL_REASON));
    assert!(
        remote.refs.is_empty(),
        "no path of any kind for another computer"
    );
}

#[test]
fn file_refs_channel_default_answers_after_the_project() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    fx.store.by_project.clear();
    let temp = TempDir::new().expect("tempdir");
    let channel = mkdir(temp.path(), "channel-folder");
    write(&channel, "notes/plan.md");
    fx.store
        .by_channel
        .insert(CHANNEL_ID.to_string(), entry(&channel));
    let answer = lookup(&fx.store, scope(true, false), &["notes/plan.md"]);
    assert_eq!(answer.source, Some(CodingSessionTreeSource::Channel));
    assert!(answer.refs["notes/plan.md"].exists);
}

#[test]
fn file_refs_not_local_gives_no_refs_and_the_true_reason() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    let answer = lookup(&fx.store, scope(false, false), &["desktop/src/app/App.tsx"]);
    assert_eq!(answer.where_, CodingSessionFileRefWhere::NotLocal);
    assert_eq!(answer.reason.as_deref(), Some(NOT_LOCAL_REASON));
    assert!(answer.refs.is_empty());
    assert_eq!(answer.source, None);
}

#[test]
fn file_refs_a_worktree_whose_folder_is_gone_says_so() {
    let fx = fixture();
    std::fs::remove_dir_all(&fx.session).expect("remove session tree");
    let answer = lookup(&fx.store, scope(false, true), &["desktop/src/app/App.tsx"]);
    assert_eq!(answer.where_, CodingSessionFileRefWhere::FolderGone);
    assert_eq!(answer.reason.as_deref(), Some(FOLDER_GONE_REASON));
    assert!(answer.refs.is_empty());
}

#[test]
fn file_refs_unreadable_store_keeps_the_resolver_reason() {
    let request = CodingSessionFileRefsRequest {
        scope: scope(true, false),
        candidates: vec!["a/b.ts".to_string()],
    };
    let answer = file_refs_from_loaded(
        Err("invalid value: \"/Users/someone/secret\"".to_string()),
        &request,
        CHECKED_AT.to_string(),
    )
    .expect("answers");
    assert_eq!(answer.where_, CodingSessionFileRefWhere::StoreUnreadable);
    let reason = answer.reason.unwrap_or_default();
    assert!(reason.contains("could not be read"));
    assert!(
        !reason.contains("/Users/"),
        "the store error never reaches the renderer"
    );
}

#[test]
fn file_refs_refuses_more_than_256_candidates() {
    let fx = fixture();
    let ok = CodingSessionFileRefsRequest {
        scope: scope(false, true),
        candidates: (0..MAX_FILE_REF_CANDIDATES)
            .map(|i| format!("f{i}.ts"))
            .collect(),
    };
    assert!(file_refs_from_loaded(Ok(fx.store.clone()), &ok, CHECKED_AT.to_string()).is_ok());
    let too_many = CodingSessionFileRefsRequest {
        scope: scope(false, true),
        candidates: (0..=MAX_FILE_REF_CANDIDATES)
            .map(|i| format!("f{i}.ts"))
            .collect(),
    };
    assert_eq!(too_many.candidates.len(), 257);
    let error = file_refs_from_loaded(Ok(fx.store), &too_many, CHECKED_AT.to_string())
        .expect_err("257 is refused");
    assert!(error.contains("256"));
}

#[test]
fn file_refs_parent_climb_resolves_with_null_relative_path() {
    let fx = fixture();
    let outside = fx.session.parent().expect("parent").join("outside.md");
    std::fs::write(&outside, b"x").expect("write outside");
    let answer = lookup(&fx.store, scope(false, true), &["../outside.md"]);
    let found = &answer.refs["../outside.md"];
    assert!(found.exists);
    assert_eq!(found.relative_path, None);
    assert!(found.full_path.is_some());
}

#[test]
fn file_refs_resolves_against_the_git_toplevel_when_the_root_is_a_subfolder() {
    let temp = TempDir::new().expect("tempdir");
    let repo = mkdir(temp.path(), "repo");
    std::fs::create_dir(repo.join(".git")).expect(".git dir");
    write(&repo, "docs/guide.md");
    let sub = mkdir(&repo, "crates/app");
    write(&sub, "src/lib.rs");
    assert_eq!(git_toplevel(&sub).as_deref(), Some(repo.as_path()));
    let mut store = CodingSessionWorkdirStore::default();
    store
        .worktrees
        .insert("ref/seat".to_string(), worktree(&sub, SESSION_ID));
    let answer = lookup(&store, scope(false, true), &["src/lib.rs", "docs/guide.md"]);
    assert_eq!(
        answer.refs["src/lib.rs"].relative_path.as_deref(),
        Some("src/lib.rs")
    );
    let guide = &answer.refs["docs/guide.md"];
    assert!(guide.exists);
    assert_eq!(guide.relative_path.as_deref(), Some("docs/guide.md"));
}

#[test]
fn file_refs_absolute_and_malformed_candidates_are_absent() {
    let fx = fixture();
    let answer = lookup(
        &fx.store,
        scope(false, true),
        &["/etc/hosts", "~/x.md", "C:/a.ts", "", "https://a.b/c.ts"],
    );
    assert!(answer.refs.is_empty(), "{:?}", answer.refs.keys());
}

#[test]
fn file_refs_candidate_position_parsing() {
    let parsed = parse_file_ref_candidate("src/main.ts:12:5").expect("parses");
    assert_eq!(
        (parsed.path.as_str(), parsed.line, parsed.column),
        ("src/main.ts", Some(12), Some(5))
    );
    let parsed = parse_file_ref_candidate("src/main.ts:0").expect("parses");
    assert_eq!((parsed.path.as_str(), parsed.line), ("src/main.ts", None));
    let parsed = parse_file_ref_candidate("src\\main.ts").expect("parses");
    assert_eq!(parsed.path, "src/main.ts");
    assert!(parse_file_ref_candidate("a.ts:x").is_none());
    assert!(parse_file_ref_candidate(&"a/".repeat(600)).is_none());
}

fn action(candidate: &str, scope: CodingSessionFileRefScope) -> CodingSessionFileRefActionRequest {
    CodingSessionFileRefActionRequest {
        scope,
        candidate: candidate.to_string(),
    }
}

#[test]
fn file_refs_open_launches_the_re_resolved_canonical_path() {
    let fx = fixture();
    let launched = std::cell::RefCell::new(None::<PathBuf>);
    run_file_ref_action(
        Ok(fx.store.clone()),
        &action("desktop/src/app/App.tsx:42", scope(false, true)),
        |path| {
            *launched.borrow_mut() = Some(path.to_path_buf());
            Ok(())
        },
    )
    .expect("opens");
    assert_eq!(
        launched.into_inner(),
        Some(fx.session.join("desktop/src/app/App.tsx"))
    );
}

#[test]
fn file_refs_open_and_reveal_of_a_deleted_file_err_without_launch() {
    let fx = fixture();
    std::fs::remove_file(fx.session.join("desktop/src/app/App.tsx")).expect("delete file");
    let launches = Cell::new(0);
    for _ in ["open", "reveal"] {
        let result = run_file_ref_action(
            Ok(fx.store.clone()),
            &action("desktop/src/app/App.tsx", scope(false, true)),
            |_| {
                launches.set(launches.get() + 1);
                Ok(())
            },
        );
        let error = result.expect_err("a deleted file is refused");
        assert!(error.contains("no longer"));
        assert!(!error.contains('/'), "no path in the sentence: {error}");
    }
    assert_eq!(launches.get(), 0);
}

#[test]
fn file_refs_action_refuses_another_computer_and_absolute_paths() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    let launches = Cell::new(0);
    let launch = |_: &Path| {
        launches.set(launches.get() + 1);
        Ok(())
    };
    let remote = run_file_ref_action(
        Ok(fx.store.clone()),
        &action("README.md", scope(false, false)),
        launch,
    );
    assert_eq!(remote.expect_err("remote"), NOT_LOCAL_REASON);
    let absolute = fx.project.join("README.md");
    let absolute = run_file_ref_action(
        Ok(fx.store),
        &action(absolute.to_str().expect("utf-8"), scope(true, false)),
        |_| {
            launches.set(launches.get() + 1);
            Ok(())
        },
    );
    assert!(
        absolute.is_err(),
        "a renderer-computed absolute path never opens"
    );
    assert_eq!(launches.get(), 0);
}

#[test]
fn file_refs_request_decodes_the_renderer_shape() {
    let request: CodingSessionFileRefsRequest = serde_json::from_value(serde_json::json!({
        "channelId": CHANNEL_ID,
        "providerSessionId": SESSION_ID,
        "projectRef": null,
        "isHiredSeat": false,
        "isLocalProvider": true,
        "candidates": ["a/b.ts"],
    }))
    .expect("decodes");
    assert_eq!(
        request.scope.provider_session_id.as_deref(),
        Some(SESSION_ID)
    );
    assert!(request.scope.is_local_provider);
    assert_eq!(request.candidates, vec!["a/b.ts".to_string()]);
    let answer = file_refs_from_loaded(
        Ok(CodingSessionWorkdirStore::default()),
        &request,
        CHECKED_AT.to_string(),
    )
    .expect("answers");
    let json = serde_json::to_value(&answer).expect("encodes");
    assert_eq!(json["where"], "notRecorded");
    assert!(json["checkedAt"].is_string());
}

#[test]
fn file_refs_open_and_reveal_refuse_a_path_outside_the_folder() {
    let fx = fixture();
    let outside = fx.session.parent().expect("parent").join("outside.md");
    std::fs::write(&outside, b"x").expect("write outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, fx.session.join("link-out.md")).expect("symlink");
    let mut candidates = vec!["../outside.md"];
    if cfg!(unix) {
        candidates.push("link-out.md");
    }
    for candidate in candidates {
        let result = run_file_ref_action(
            Ok(fx.store.clone()),
            &action(candidate, scope(false, true)),
            |path| panic!("launched {}", path.display()),
        );
        assert!(result.is_err(), "{candidate} must not launch");
    }
}
