//! Import and link existing local checkouts.
//!
//! Importing announces a repo elsewhere (the frontend publishes the
//! kind:30617), then this module wires the chosen folder to the relay: remote
//! setup, first push, and a registry entry so every git command resolves the
//! arbitrary path (`project_repo_registry`). Linking is the no-push sibling
//! for repos that already exist — typically announced by another user — where
//! a `ls-remote` connectivity check stands in for the push.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use super::project_git_exec::{
    build_git_auth_config, clean_branch, clone_url_owner, run_git, validate_workspace_clone_url,
    GitAuthConfig,
};
use super::project_repo_paths::normalized_clone_url;
use super::project_repo_registry::register_checkout;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ImportRepoFolderInfo {
    pub path: String,
    /// Folder basename — the dialog's default repository name.
    pub name: String,
    pub is_git_repo: bool,
    pub current_branch: Option<String>,
    pub origin_url: Option<String>,
    pub has_commits: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProjectRepoInput {
    pub path: String,
    pub clone_url: String,
    /// Announcement owner (64-hex pubkey) — the importer for imports, the
    /// original announcer for links.
    pub owner: String,
    pub dtag: String,
    /// "set-origin" (rename a foreign origin to `upstream`, point `origin` at
    /// the relay) or "add-buzz-remote" (leave `origin` untouched).
    pub remote_strategy: String,
}

#[derive(Debug, Serialize)]
pub struct ImportProjectRepoResult {
    pub path: String,
    pub remote: String,
    pub branch: String,
}

#[derive(Debug, Serialize)]
pub struct LinkProjectRepoResult {
    pub path: String,
    pub remote: String,
}

enum RemoteStrategy {
    SetOrigin,
    AddBuzzRemote,
}

impl RemoteStrategy {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "set-origin" => Ok(Self::SetOrigin),
            "add-buzz-remote" => Ok(Self::AddBuzzRemote),
            other => Err(format!("unknown remote strategy: {other:?}")),
        }
    }
}

/// Native folder picker plus a git inspection of the chosen folder, feeding
/// the import/link dialogs' validation and prefill.
#[tauri::command]
pub async fn pick_project_import_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ImportRepoFolderInfo>, String> {
    use tauri_plugin_dialog::DialogExt;

    let auth = build_git_auth_config(&state)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a git repository")
        .pick_folder(move |picked| {
            let _ = sender.send(picked);
        });
    let Some(picked) = receiver
        .await
        .map_err(|_| "folder picker closed unexpectedly".to_string())?
    else {
        return Ok(None);
    };
    let path = picked
        .as_path()
        .ok_or("the selected folder path is invalid")?
        .to_path_buf();

    tauri::async_runtime::spawn_blocking(move || Ok(Some(inspect_import_folder(&path, &auth))))
        .await
        .map_err(|error| format!("folder inspection task failed: {error}"))?
}

fn inspect_import_folder(path: &std::path::Path, auth: &GitAuthConfig) -> ImportRepoFolderInfo {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let is_git_repo = path.join(".git").exists();
    let (current_branch, origin_url, has_commits) = if is_git_repo {
        let current_branch = run_git(&["branch", "--show-current"], Some(path), auth)
            .ok()
            .and_then(|output| clean_branch(Some(output.trim().to_string())));
        let origin_url = run_git(&["remote", "get-url", "origin"], Some(path), auth)
            .ok()
            .map(|output| output.trim().to_string())
            .filter(|url| !url.is_empty());
        let has_commits = run_git(&["rev-parse", "--verify", "HEAD"], Some(path), auth).is_ok();
        (current_branch, origin_url, has_commits)
    } else {
        (None, None, false)
    };
    ImportRepoFolderInfo {
        path: path.display().to_string(),
        name,
        is_git_repo,
        current_branch,
        origin_url,
        has_commits,
    }
}

/// Wire an existing local checkout to a freshly announced relay-hosted repo:
/// remote setup, first push, registry entry.
#[tauri::command]
pub async fn import_project_local_repository(
    input: ImportProjectRepoInput,
    state: State<'_, AppState>,
) -> Result<ImportProjectRepoResult, String> {
    validate_import_input(&input, &state)?;
    let auth = build_git_auth_config(&state)?;
    tauri::async_runtime::spawn_blocking(move || import_repo_blocking(&input, &auth))
        .await
        .map_err(|error| format!("repository import task failed: {error}"))?
}

/// Register an existing local checkout for an already-announced repo without
/// pushing — the linker may only have read access.
#[tauri::command]
pub async fn link_project_local_repository(
    input: ImportProjectRepoInput,
    state: State<'_, AppState>,
) -> Result<LinkProjectRepoResult, String> {
    validate_import_input(&input, &state)?;
    let auth = build_git_auth_config(&state)?;
    tauri::async_runtime::spawn_blocking(move || link_repo_blocking(&input, &auth))
        .await
        .map_err(|error| format!("repository link task failed: {error}"))?
}

/// Defense in depth around the frontend-supplied coordinate: the clone URL
/// must belong to the active relay and actually address `<owner>/<dtag>`.
fn validate_import_input(input: &ImportProjectRepoInput, state: &AppState) -> Result<(), String> {
    validate_workspace_clone_url(&input.clone_url, state)?;
    let url_owner = clone_url_owner(&input.clone_url)
        .ok_or_else(|| "clone URL does not carry an owner pubkey".to_string())?;
    if url_owner != input.owner.trim().to_lowercase() {
        return Err("clone URL owner does not match the repository owner".to_string());
    }
    let url_repo = input
        .clone_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .map(|segment| segment.trim_end_matches(".git"))
        .unwrap_or_default();
    if url_repo != input.dtag {
        return Err("clone URL does not match the repository id".to_string());
    }
    RemoteStrategy::parse(&input.remote_strategy).map(|_| ())
}

fn validate_checkout_path(path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(path);
    if !path.is_absolute() {
        return Err("Choose a git repository.".to_string());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "Choose a git repository.".to_string())?;
    if !path.is_dir() || !path.join(".git").exists() {
        return Err("Choose a git repository.".to_string());
    }
    Ok(path)
}

/// Idempotent remote setup per the user's strategy; returns the remote name
/// the registry must record. Reads before mutating so a retry after a failed
/// push changes nothing.
fn ensure_relay_remote(
    repo_dir: &std::path::Path,
    clone_url: &str,
    strategy: &RemoteStrategy,
    auth: &GitAuthConfig,
) -> Result<String, String> {
    let remote_url = |remote: &str| {
        run_git(&["remote", "get-url", remote], Some(repo_dir), auth)
            .ok()
            .map(|output| output.trim().to_string())
            .filter(|url| !url.is_empty())
    };
    match strategy {
        RemoteStrategy::SetOrigin => {
            match remote_url("origin") {
                None => {
                    run_git(
                        &["remote", "add", "origin", clone_url],
                        Some(repo_dir),
                        auth,
                    )?;
                }
                Some(current)
                    if normalized_clone_url(&current) == normalized_clone_url(clone_url) => {}
                Some(_) => {
                    if remote_url("upstream").is_some() {
                        return Err(
                            "This repository already has an 'upstream' remote — rename it first, \
                             or keep origin unchanged and use a separate 'buzz' remote."
                                .to_string(),
                        );
                    }
                    run_git(
                        &["remote", "rename", "origin", "upstream"],
                        Some(repo_dir),
                        auth,
                    )?;
                    run_git(
                        &["remote", "add", "origin", clone_url],
                        Some(repo_dir),
                        auth,
                    )?;
                }
            }
            Ok("origin".to_string())
        }
        RemoteStrategy::AddBuzzRemote => {
            match remote_url("buzz") {
                None => {
                    run_git(&["remote", "add", "buzz", clone_url], Some(repo_dir), auth)?;
                }
                Some(current)
                    if normalized_clone_url(&current) == normalized_clone_url(clone_url) => {}
                Some(_) => {
                    run_git(
                        &["remote", "set-url", "buzz", clone_url],
                        Some(repo_dir),
                        auth,
                    )?;
                }
            }
            Ok("buzz".to_string())
        }
    }
}

fn import_repo_blocking(
    input: &ImportProjectRepoInput,
    auth: &GitAuthConfig,
) -> Result<ImportProjectRepoResult, String> {
    let strategy = RemoteStrategy::parse(&input.remote_strategy)?;
    let repo_dir = validate_checkout_path(&input.path)?;
    if run_git(&["rev-parse", "--verify", "HEAD"], Some(&repo_dir), auth).is_err() {
        return Err("The repository has no commits yet — commit before importing.".to_string());
    }
    let branch = run_git(&["branch", "--show-current"], Some(&repo_dir), auth)
        .ok()
        .and_then(|output| clean_branch(Some(output.trim().to_string())))
        .ok_or_else(|| "Check out a branch before importing (detached HEAD).".to_string())?;

    let remote = ensure_relay_remote(&repo_dir, &input.clone_url, &strategy, auth)?;
    run_git(
        &[
            "push",
            "-u",
            "--end-of-options",
            remote.as_str(),
            format!("HEAD:{branch}").as_str(),
        ],
        Some(&repo_dir),
        auth,
    )?;
    register_checkout(
        &input.owner,
        &input.dtag,
        &repo_dir,
        &remote,
        &input.clone_url,
    )?;

    Ok(ImportProjectRepoResult {
        path: repo_dir.display().to_string(),
        remote,
        branch,
    })
}

fn link_repo_blocking(
    input: &ImportProjectRepoInput,
    auth: &GitAuthConfig,
) -> Result<LinkProjectRepoResult, String> {
    let strategy = RemoteStrategy::parse(&input.remote_strategy)?;
    let repo_dir = validate_checkout_path(&input.path)?;
    let remote = ensure_relay_remote(&repo_dir, &input.clone_url, &strategy, auth)?;
    // No push happens for a link — a listing proves the repo is reachable and
    // the caller may read it before anything is registered.
    run_git(
        &["ls-remote", "--heads", "--end-of-options", remote.as_str()],
        Some(&repo_dir),
        auth,
    )
    .map_err(|error| format!("Cannot reach the repository (check your access): {error}"))?;
    register_checkout(
        &input.owner,
        &input.dtag,
        &repo_dir,
        &remote,
        &input.clone_url,
    )?;

    Ok(LinkProjectRepoResult {
        path: repo_dir.display().to_string(),
        remote,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::project_git_exec::build_test_git_auth_config;

    fn make_input(
        path: &std::path::Path,
        clone_url: &str,
        strategy: &str,
    ) -> ImportProjectRepoInput {
        ImportProjectRepoInput {
            path: path.display().to_string(),
            clone_url: clone_url.to_string(),
            owner: "a".repeat(64),
            dtag: "widget".to_string(),
            remote_strategy: strategy.to_string(),
        }
    }

    fn init_commit(checkout: &std::path::Path, auth: &GitAuthConfig) {
        run_git(
            &[
                "init",
                "-b",
                "main",
                "--",
                checkout.to_str().expect("checkout path"),
            ],
            None,
            auth,
        )
        .expect("initialize checkout");
        std::fs::write(checkout.join("README.md"), "first commit\n").expect("write fixture");
        run_git(&["add", "README.md"], Some(checkout), auth).expect("stage fixture");
        run_git(
            &[
                "-c",
                "user.name=Beekeeper Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "Initial commit",
            ],
            Some(checkout),
            auth,
        )
        .expect("commit fixture");
    }

    #[test]
    fn set_origin_renames_a_foreign_origin_to_upstream_and_pushes() {
        let auth = build_test_git_auth_config().expect("test auth");
        let root = tempfile::tempdir().expect("tempdir");
        let remote = root.path().join("relay.git");
        let checkout = root.path().join("checkout");
        let remote_path = remote.to_str().expect("remote path").to_string();
        run_git(&["init", "--bare", "--", &remote_path], None, &auth).expect("init remote");
        init_commit(&checkout, &auth);
        let foreign = "https://github.com/example/widget.git";
        run_git(
            &["remote", "add", "origin", foreign],
            Some(&checkout),
            &auth,
        )
        .expect("add foreign origin");

        let strategy = RemoteStrategy::parse("set-origin").expect("strategy");
        let remote_name =
            ensure_relay_remote(&checkout, &remote_path, &strategy, &auth).expect("remote setup");
        assert_eq!(remote_name, "origin");
        assert_eq!(
            run_git(&["remote", "get-url", "upstream"], Some(&checkout), &auth)
                .expect("upstream url")
                .trim(),
            foreign
        );
        assert_eq!(
            run_git(&["remote", "get-url", "origin"], Some(&checkout), &auth)
                .expect("origin url")
                .trim(),
            remote_path
        );

        // Rerunning is a no-op.
        let again =
            ensure_relay_remote(&checkout, &remote_path, &strategy, &auth).expect("idempotent");
        assert_eq!(again, "origin");

        run_git(
            &["push", "-u", "--end-of-options", "origin", "HEAD:main"],
            Some(&checkout),
            &auth,
        )
        .expect("push to relay remote");
        assert!(run_git(
            &[
                format!("--git-dir={remote_path}").as_str(),
                "show-ref",
                "--verify",
                "refs/heads/main",
            ],
            None,
            &auth,
        )
        .is_ok());
    }

    #[test]
    fn add_buzz_remote_leaves_origin_untouched() {
        let auth = build_test_git_auth_config().expect("test auth");
        let root = tempfile::tempdir().expect("tempdir");
        let remote = root.path().join("relay.git");
        let checkout = root.path().join("checkout");
        let remote_path = remote.to_str().expect("remote path").to_string();
        run_git(&["init", "--bare", "--", &remote_path], None, &auth).expect("init remote");
        init_commit(&checkout, &auth);
        let foreign = "https://github.com/example/widget.git";
        run_git(
            &["remote", "add", "origin", foreign],
            Some(&checkout),
            &auth,
        )
        .expect("add foreign origin");

        let strategy = RemoteStrategy::parse("add-buzz-remote").expect("strategy");
        let remote_name =
            ensure_relay_remote(&checkout, &remote_path, &strategy, &auth).expect("remote setup");
        assert_eq!(remote_name, "buzz");
        assert_eq!(
            run_git(&["remote", "get-url", "origin"], Some(&checkout), &auth)
                .expect("origin url")
                .trim(),
            foreign
        );
        assert_eq!(
            run_git(&["remote", "get-url", "buzz"], Some(&checkout), &auth)
                .expect("buzz url")
                .trim(),
            remote_path
        );
    }

    #[test]
    fn set_origin_refuses_when_upstream_already_exists() {
        let auth = build_test_git_auth_config().expect("test auth");
        let root = tempfile::tempdir().expect("tempdir");
        let checkout = root.path().join("checkout");
        init_commit(&checkout, &auth);
        run_git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/widget.git",
            ],
            Some(&checkout),
            &auth,
        )
        .expect("add origin");
        run_git(
            &[
                "remote",
                "add",
                "upstream",
                "https://github.com/upstream/widget.git",
            ],
            Some(&checkout),
            &auth,
        )
        .expect("add upstream");

        let strategy = RemoteStrategy::parse("set-origin").expect("strategy");
        let error = ensure_relay_remote(
            &checkout,
            "https://relay.test/git/x/widget",
            &strategy,
            &auth,
        )
        .expect_err("must refuse");
        assert!(error.contains("upstream"), "{error}");
    }

    #[test]
    fn import_rejects_unborn_and_non_git_folders() {
        let auth = build_test_git_auth_config().expect("test auth");
        let root = tempfile::tempdir().expect("tempdir");

        let plain = root.path().join("plain");
        std::fs::create_dir_all(&plain).expect("create dir");
        let error = import_repo_blocking(&make_input(&plain, "file:///tmp/x", "set-origin"), &auth)
            .expect_err("non-git folder");
        assert!(error.contains("Choose a git repository"), "{error}");

        let unborn = root.path().join("unborn");
        run_git(
            &[
                "init",
                "-b",
                "main",
                "--",
                unborn.to_str().expect("unborn path"),
            ],
            None,
            &auth,
        )
        .expect("init unborn");
        let error =
            import_repo_blocking(&make_input(&unborn, "file:///tmp/x", "set-origin"), &auth)
                .expect_err("unborn repo");
        assert!(error.contains("no commits"), "{error}");
    }

    #[test]
    fn link_checks_reachability_without_pushing() {
        let auth = build_test_git_auth_config().expect("test auth");
        let root = tempfile::tempdir().expect("tempdir");
        let remote = root.path().join("relay.git");
        let checkout = root.path().join("checkout");
        let remote_path = remote.to_str().expect("remote path").to_string();
        run_git(&["init", "--bare", "--", &remote_path], None, &auth).expect("init remote");
        init_commit(&checkout, &auth);

        let strategy = RemoteStrategy::parse("set-origin").expect("strategy");
        let remote_name =
            ensure_relay_remote(&checkout, &remote_path, &strategy, &auth).expect("remote setup");
        // Reachable empty remote lists fine...
        run_git(
            &[
                "ls-remote",
                "--heads",
                "--end-of-options",
                remote_name.as_str(),
            ],
            Some(&checkout),
            &auth,
        )
        .expect("ls-remote reachable");
        // ...and no branch was pushed by the link path.
        assert!(run_git(
            &[
                format!("--git-dir={remote_path}").as_str(),
                "show-ref",
                "--verify",
                "refs/heads/main",
            ],
            None,
            &auth,
        )
        .is_err());

        // Unreachable remote fails the check.
        run_git(
            &[
                "remote",
                "set-url",
                "origin",
                root.path().join("missing.git").to_str().expect("path"),
            ],
            Some(&checkout),
            &auth,
        )
        .expect("point at missing remote");
        assert!(run_git(
            &["ls-remote", "--heads", "--end-of-options", "origin"],
            Some(&checkout),
            &auth,
        )
        .is_err());
    }
}

#[derive(Serialize)]
pub struct ProjectLocalRepoInfo {
    pub name: String,
    pub path: String,
}

/// Local checkouts visible to the frontend: every registered checkout
/// (imported/linked from arbitrary paths, reported under its announcement
/// dtag so name-set matching lights "Local" badges) plus the repos-root scan.
#[tauri::command]
pub async fn list_project_local_repositories(
    repos_dir: Option<String>,
) -> Result<Vec<ProjectLocalRepoInfo>, String> {
    use super::project_repo_paths::canonical_repos_roots;

    tauri::async_runtime::spawn_blocking(move || {
        let repos_roots = canonical_repos_roots(repos_dir.as_deref())?;
        let mut seen_paths = std::collections::HashSet::new();
        let mut repos = Vec::new();
        for (key, entry) in super::project_repo_registry::load_repo_registry().checkouts {
            let Some((_, dtag)) = key.split_once(':') else {
                continue;
            };
            let Ok(path) = entry.path.canonicalize() else {
                continue;
            };
            if !path.is_dir() || !path.join(".git").exists() {
                continue;
            }
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            repos.push(ProjectLocalRepoInfo {
                name: dtag.to_string(),
                path: path.display().to_string(),
            });
        }
        for repos_root in repos_roots {
            let entries = std::fs::read_dir(&repos_root)
                .map_err(|error| format!("read reposDir: {error}"))?;
            for entry in entries.filter_map(Result::ok) {
                let Some(file_type) = entry.file_type().ok() else {
                    continue;
                };
                if !file_type.is_dir() && !file_type.is_symlink() {
                    continue;
                }
                let Ok(path) = entry.path().canonicalize() else {
                    continue;
                };
                if !path.starts_with(&repos_root) || !path.is_dir() || !path.join(".git").exists() {
                    continue;
                }
                if !seen_paths.insert(path.clone()) {
                    continue;
                }
                repos.push(ProjectLocalRepoInfo {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: path.display().to_string(),
                });
            }
        }
        repos.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(repos)
    })
    .await
    .map_err(|error| format!("local repo list task failed: {error}"))?
}
