use super::project_git_exec::{
    build_git_auth_config, clean_branch, run_git, validate_workspace_clone_url, GitAuthConfig,
};
use super::project_repo_paths::find_local_repo_dir;
use crate::app_state::AppState;
use serde::Serialize;
use tauri::State;

/// Per-file cap on rendered patch lines. One regenerated lockfile or
/// minified bundle would otherwise produce tens of thousands of DOM nodes
/// in the diff view and freeze the webview.
const MAX_PATCH_LINES: usize = 2_000;

#[derive(Serialize)]
pub struct ProjectRepoDiffFileInfo {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub patch: String,
    pub truncated: bool,
}

#[derive(Serialize)]
pub struct ProjectRepoDiffInfo {
    pub files: Vec<ProjectRepoDiffFileInfo>,
    pub additions: usize,
    pub deletions: usize,
    pub commit_body: Option<String>,
}

fn clean_target_ref(value: Option<String>) -> Option<String> {
    value.filter(|value| {
        value.starts_with("refs/")
            && !value.contains("..")
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
    })
}

pub(crate) fn clean_commit(value: Option<String>) -> Option<String> {
    value
        .filter(|value| matches!(value.len(), 40 | 64))
        .filter(|value| value.chars().all(|c| c.is_ascii_hexdigit()))
}

fn fetch_target(
    repo_dir: &std::path::Path,
    auth: &GitAuthConfig,
    branch: Option<&str>,
    target_ref: Option<&str>,
    target_commit: Option<&str>,
) -> Result<(), String> {
    if let Some(target_ref) = target_ref {
        if run_git(
            &["fetch", "--depth=100", "origin", target_ref],
            Some(repo_dir),
            auth,
        )
        .is_ok()
        {
            run_git(
                &["checkout", "--detach", "FETCH_HEAD"],
                Some(repo_dir),
                auth,
            )?;
            return Ok(());
        }
    } else if let Some(target_commit) = target_commit {
        if run_git(
            &["fetch", "--depth=100", "origin", target_commit],
            Some(repo_dir),
            auth,
        )
        .is_ok()
        {
            run_git(
                &["checkout", "--detach", "FETCH_HEAD"],
                Some(repo_dir),
                auth,
            )?;
            return Ok(());
        }
    }

    if let Some(target_commit) = target_commit {
        if run_git(
            &["fetch", "--depth=100", "origin", target_commit],
            Some(repo_dir),
            auth,
        )
        .is_ok()
        {
            run_git(
                &["checkout", "--detach", "FETCH_HEAD"],
                Some(repo_dir),
                auth,
            )?;
            return Ok(());
        }
    }

    if let Some(branch) = branch {
        let refspec = format!("refs/heads/{branch}:refs/remotes/origin/{branch}");
        run_git(
            &["fetch", "--depth=100", "origin", &refspec],
            Some(repo_dir),
            auth,
        )?;
        run_git(
            &["checkout", "--detach", &format!("origin/{branch}")],
            Some(repo_dir),
            auth,
        )?;
        return Ok(());
    }

    run_git(
        &["fetch", "--depth=100", "origin", "HEAD"],
        Some(repo_dir),
        auth,
    )?;
    run_git(
        &["checkout", "--detach", "FETCH_HEAD"],
        Some(repo_dir),
        auth,
    )?;
    Ok(())
}

fn diff_base_ref(
    repo_dir: &std::path::Path,
    auth: &GitAuthConfig,
    base_branch: Option<&str>,
) -> Option<String> {
    let base_branch = base_branch?;
    let refspec = format!("refs/heads/{base_branch}:refs/remotes/origin/{base_branch}");
    run_git(
        &["fetch", "--depth=100", "origin", &refspec],
        Some(repo_dir),
        auth,
    )
    .ok()?;
    Some(format!("origin/{base_branch}"))
}

fn parse_count(value: &str) -> usize {
    value.parse::<usize>().unwrap_or_default()
}

fn parse_numstat(output: &str) -> Vec<(String, usize, usize)> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let additions = parse_count(parts.next()?);
            let deletions = parse_count(parts.next()?);
            let path = parts.next()?.to_string();
            Some((path, additions, deletions))
        })
        .take(250)
        .collect()
}

fn empty_tree_ref(repo_dir: &std::path::Path, auth: &GitAuthConfig) -> Result<String, String> {
    run_git(
        &["hash-object", "-t", "tree", "/dev/null"],
        Some(repo_dir),
        auth,
    )
    .map(|output| output.trim().to_string())
}

fn diff_range(
    repo_dir: &std::path::Path,
    auth: &GitAuthConfig,
    base_ref: Option<String>,
) -> String {
    if let Some(base_ref) = base_ref {
        return if run_git(&["merge-base", &base_ref, "HEAD"], Some(repo_dir), auth).is_ok() {
            format!("{base_ref}...HEAD")
        } else {
            format!("{base_ref}..HEAD")
        };
    }

    empty_tree_ref(repo_dir, auth)
        .map(|empty_tree| format!("{empty_tree}..HEAD"))
        .unwrap_or_else(|_| "HEAD^..HEAD".to_string())
}

/// Range for a single commit against its parent, used by the commit detail
/// view. Root commits fall back to the empty tree so the whole initial tree
/// renders as additions. Errors when the commit is not reachable in the
/// available history — diffing an unrelated ref instead would be misleading.
fn commit_parent_range(
    repo_dir: &std::path::Path,
    auth: &GitAuthConfig,
    commit: &str,
) -> Result<String, String> {
    run_git(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{commit}^{{commit}}"),
        ],
        Some(repo_dir),
        auth,
    )
    .map_err(|_| format!("commit {commit} was not found in the repository history"))?;
    let parent = format!("{commit}^");
    if run_git(
        &["rev-parse", "--verify", "--quiet", &parent],
        Some(repo_dir),
        auth,
    )
    .is_ok()
    {
        return Ok(format!("{parent}..{commit}"));
    }
    let empty_tree = empty_tree_ref(repo_dir, auth)?;
    Ok(format!("{empty_tree}..{commit}"))
}

fn local_ref_exists(repo_dir: &std::path::Path, auth: &GitAuthConfig, ref_name: &str) -> bool {
    run_git(
        &["rev-parse", "--verify", "--quiet", ref_name],
        Some(repo_dir),
        auth,
    )
    .is_ok()
}

fn local_target_ref(
    repo_dir: &std::path::Path,
    remote: &str,
    auth: &GitAuthConfig,
    branch: Option<&str>,
    target_commit: Option<&str>,
) -> String {
    if let Some(target_commit) = target_commit {
        if local_ref_exists(repo_dir, auth, target_commit) {
            return target_commit.to_string();
        }
    }
    if let Some(branch) = branch {
        if local_ref_exists(repo_dir, auth, branch) {
            return branch.to_string();
        }
        let remote_branch = format!("{remote}/{branch}");
        if local_ref_exists(repo_dir, auth, &remote_branch) {
            return remote_branch;
        }
    }
    "HEAD".to_string()
}

fn local_base_ref(
    repo_dir: &std::path::Path,
    remote: &str,
    auth: &GitAuthConfig,
    branch: Option<&str>,
    target_branch: Option<&str>,
) -> Option<String> {
    let branch = branch?;
    let remote_branch = format!("{remote}/{branch}");
    if local_ref_exists(repo_dir, auth, &remote_branch) {
        return Some(remote_branch);
    }
    if target_branch == Some(branch) {
        return None;
    }
    local_ref_exists(repo_dir, auth, branch).then_some(branch.to_string())
}

fn local_diff_range(
    repo_dir: &std::path::Path,
    remote: &str,
    auth: &GitAuthConfig,
    base_branch: Option<&str>,
    target_branch: Option<&str>,
    base_commit: Option<&str>,
    target_commit: Option<&str>,
) -> String {
    let target_ref = local_target_ref(repo_dir, remote, auth, target_branch, target_commit);
    if let Some(base_commit) = base_commit {
        if base_commit != target_ref && local_ref_exists(repo_dir, auth, base_commit) {
            return if run_git(
                &["merge-base", base_commit, &target_ref],
                Some(repo_dir),
                auth,
            )
            .is_ok()
            {
                format!("{base_commit}...{target_ref}")
            } else {
                format!("{base_commit}..{target_ref}")
            };
        }
    }
    if let Some(base_ref) = local_base_ref(repo_dir, remote, auth, base_branch, target_branch) {
        return if run_git(
            &["merge-base", &base_ref, &target_ref],
            Some(repo_dir),
            auth,
        )
        .is_ok()
        {
            format!("{base_ref}...{target_ref}")
        } else {
            format!("{base_ref}..{target_ref}")
        };
    }
    // With no base at all, a bare commit means "diff against its parent"
    // (commit detail view) rather than against the whole tree.
    if base_commit.is_none() && base_branch.is_none() {
        if let Some(target_commit) = target_commit {
            if local_ref_exists(repo_dir, auth, target_commit) {
                if let Ok(range) = commit_parent_range(repo_dir, auth, target_commit) {
                    return range;
                }
            }
        }
    }
    empty_tree_ref(repo_dir, auth)
        .map(|empty_tree| format!("{empty_tree}..{target_ref}"))
        .unwrap_or_else(|_| format!("{target_ref}^..{target_ref}"))
}

/// Caps a patch at [`MAX_PATCH_LINES`], reporting whether it was cut.
pub(super) fn truncate_patch(patch: String) -> (String, bool) {
    let mut line_starts = patch
        .char_indices()
        .filter(|(_, c)| *c == '\n')
        .map(|(index, _)| index);
    match line_starts.nth(MAX_PATCH_LINES - 1) {
        Some(cut_at) => (patch[..cut_at].to_string(), true),
        None => (patch, false),
    }
}

/// Where a diff's Git runs. A diff runs the repository's own `textconv`
/// programs, so a recorded local workspace — writable by the operator and by
/// the project's sessions — diffs inside its project boundary. The host's own
/// fresh scratch clone holds no configuration but what the host wrote, and
/// stays hardened host Git.
enum DiffGit<'a> {
    Scratch(&'a GitAuthConfig),
    Workspace(&'a buzz_session_provider_pkg::execution_scope_host::HostLaunchPlan),
}

impl DiffGit<'_> {
    fn run(&self, args: &[&str], repo_dir: &std::path::Path) -> Result<String, String> {
        match self {
            Self::Scratch(auth) => run_git(args, Some(repo_dir), auth),
            Self::Workspace(plan) => crate::coding_sessions::host_git::run(plan, repo_dir, args),
        }
    }
}

fn diff_from_repo(
    repo_dir: &std::path::Path,
    git: &DiffGit<'_>,
    range: &str,
    target_commit: Option<&str>,
) -> Result<ProjectRepoDiffInfo, String> {
    let commit_body = target_commit
        .map(|commit| {
            git.run(
                &[
                    "show",
                    "--no-patch",
                    "--format=%b",
                    "--end-of-options",
                    commit,
                ],
                repo_dir,
            )
            .map(|body| body.trim_end().to_string())
        })
        .transpose()?
        .filter(|body| !body.is_empty());
    let numstat = git.run(&["diff", "--numstat", range], repo_dir)?;
    let files = parse_numstat(&numstat)
        .into_iter()
        .map(|(path, additions, deletions)| {
            // A patch that could not be produced — a converter the repository
            // names failed, or was refused by the boundary — is an error
            // naming the file, never an empty patch that reads as no change.
            let patch = git
                .run(
                    &[
                        "diff",
                        "--no-ext-diff",
                        "--find-renames",
                        "--find-copies",
                        "--unified=80",
                        "--src-prefix=a/",
                        "--dst-prefix=b/",
                        range,
                        "--",
                        &path,
                    ],
                    repo_dir,
                )
                .map_err(|error| format!("could not produce the diff of {path}: {error}"))?;
            let (patch, truncated) = truncate_patch(patch);
            Ok(ProjectRepoDiffFileInfo {
                path,
                additions,
                deletions,
                patch,
                truncated,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ProjectRepoDiffInfo {
        additions: files.iter().map(|file| file.additions).sum(),
        deletions: files.iter().map(|file| file.deletions).sum(),
        commit_body,
        files,
    })
}

#[tauri::command]
pub async fn get_project_repo_diff(
    clone_url: String,
    default_branch: Option<String>,
    base_branch: Option<String>,
    target_ref: Option<String>,
    target_commit: Option<String>,
    state: State<'_, AppState>,
) -> Result<ProjectRepoDiffInfo, String> {
    validate_workspace_clone_url(&clone_url, &state)?;
    let auth = build_git_auth_config(&state)?;
    let branch = clean_branch(default_branch);
    let base_branch = clean_branch(base_branch);
    let target_ref = clean_target_ref(target_ref);
    let target_commit = clean_commit(target_commit);

    tauri::async_runtime::spawn_blocking(move || {
        let temp_dir = tempfile::tempdir().map_err(|error| format!("create temp dir: {error}"))?;
        let repo_dir = temp_dir.path().join("repo");
        let repo_path = repo_dir
            .to_str()
            .ok_or_else(|| "temporary repository path is not UTF-8".to_string())?;
        run_git(
            &[
                "clone",
                "--filter=blob:none",
                "--no-checkout",
                &clone_url,
                repo_path,
            ],
            None,
            &auth,
        )?;
        fetch_target(
            &repo_dir,
            &auth,
            branch.as_deref(),
            target_ref.as_deref(),
            target_commit.as_deref(),
        )?;
        // A commit with no base branch or target ref means "diff this commit
        // against its parent" (commit detail view), not "diff HEAD against a
        // base".
        let range = match (&target_ref, &base_branch, &target_commit) {
            (None, None, Some(commit)) => commit_parent_range(&repo_dir, &auth, commit)?,
            _ => diff_range(
                &repo_dir,
                &auth,
                diff_base_ref(&repo_dir, &auth, base_branch.as_deref()),
            ),
        };
        let commit_body_ref = if target_ref.is_none() && base_branch.is_none() {
            target_commit.as_deref()
        } else {
            None
        };
        diff_from_repo(&repo_dir, &DiffGit::Scratch(&auth), &range, commit_body_ref)
    })
    .await
    .map_err(|error| format!("repo diff task failed: {error}"))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_project_local_repo_diff(
    repos_dir: Option<String>,
    project_dtag: String,
    clone_url: Option<String>,
    default_branch: Option<String>,
    base_branch: Option<String>,
    base_commit: Option<String>,
    target_commit: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<ProjectRepoDiffInfo>, String> {
    let auth = build_git_auth_config(&state)?;
    let branch = clean_branch(default_branch);
    let base_branch = clean_branch(base_branch);
    let base_commit = clean_commit(base_commit);
    let target_commit = clean_commit(target_commit);

    tauri::async_runtime::spawn_blocking(move || {
        let Some(checkout) =
            find_local_repo_dir(repos_dir.as_deref(), &project_dtag, clone_url.as_deref())?
        else {
            return Ok(None);
        };
        local_checkout_diff(
            &checkout,
            &auth,
            branch.as_deref(),
            base_branch.as_deref(),
            base_commit.as_deref(),
            target_commit.as_deref(),
        )
        .map(Some)
    })
    .await
    .map_err(|error| format!("local repo diff task failed: {error}"))?
}

/// The diff of a recorded local checkout, run inside its project boundary
/// (see [`DiffGit`]). Resolving the range reads refs only; the diff itself
/// runs the repository's own `textconv`.
///
/// # Errors
/// The checkout could not be bounded (nothing ran), or Git failed.
pub(crate) fn local_checkout_diff(
    checkout: &super::project_repo_registry::LocalRepoCheckout,
    auth: &GitAuthConfig,
    branch: Option<&str>,
    base_branch: Option<&str>,
    base_commit: Option<&str>,
    target_commit: Option<&str>,
) -> Result<ProjectRepoDiffInfo, String> {
    let range = local_diff_range(
        &checkout.path,
        &checkout.remote,
        auth,
        base_branch,
        branch,
        base_commit,
        target_commit,
    );
    let commit_body_ref = if base_commit.is_none() && base_branch.is_none() {
        target_commit
    } else {
        None
    };
    let plan =
        crate::coding_sessions::host_git::prepare(&crate::coding_sessions::host_git::Workspace {
            tree: &checkout.path,
            repo_root: None,
            name: "diff",
            host_branch: None,
            host_read: &[],
        })?;
    diff_from_repo(
        &checkout.path,
        &DiffGit::Workspace(&plan),
        &range,
        commit_body_ref,
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{diff_from_repo, local_checkout_diff, DiffGit};
    use crate::commands::project_git_exec::{build_test_git_auth_config, run_git};
    use crate::commands::project_repo_registry::LocalRepoCheckout;

    /// A diff of the recorded local checkout runs the repository's own
    /// `textconv` — its transformation is in the patch — inside the
    /// checkout's boundary. The control, the same diff through hardened host
    /// Git, shows the same program reading a neighbour's file.
    #[test]
    fn a_local_checkout_diff_runs_its_textconv_inside_the_boundary() {
        let auth = build_test_git_auth_config().expect("auth");
        let root = tempfile::tempdir().expect("tempdir");
        let root = root.path().canonicalize().expect("canonical");
        let foreign = root.join("elsewhere/plan.md");
        std::fs::create_dir_all(foreign.parent().expect("parent")).expect("elsewhere");
        std::fs::write(&foreign, "FOREIGN_PLAN_CANARY\n").expect("foreign");
        let checkout = root.join("checkout");
        run_git(
            &[
                "init",
                "-q",
                "--initial-branch=main",
                "--",
                checkout.to_str().expect("path"),
            ],
            None,
            &auth,
        )
        .expect("init");
        let commit = |message: &str| {
            run_git(&["add", "."], Some(&checkout), &auth).expect("add");
            run_git(
                &[
                    "-c",
                    "user.name=T",
                    "-c",
                    "user.email=t@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    message,
                ],
                Some(&checkout),
                &auth,
            )
            .expect("commit");
            run_git(&["rev-parse", "HEAD"], Some(&checkout), &auth)
                .expect("head")
                .trim()
                .to_owned()
        };
        std::fs::write(checkout.join(".gitattributes"), "*.txt diff=fixture\n")
            .expect("attributes");
        std::fs::write(checkout.join("notes.txt"), "one\n").expect("one");
        let first = commit("first");
        std::fs::write(checkout.join("notes.txt"), "two\n").expect("two");
        let second = commit("second");
        let textconv = format!(
            "sh -c 'cat \"{}\" 2>/dev/null; sed s/^/conv:/ \"$1\"' --",
            foreign.display()
        );
        run_git(
            &["config", "diff.fixture.textconv", &textconv],
            Some(&checkout),
            &auth,
        )
        .expect("textconv");
        let range = format!("{first}..{second}");

        let control =
            diff_from_repo(&checkout, &DiffGit::Scratch(&auth), &range, None).expect("control");
        let control_patch: String = control
            .files
            .iter()
            .map(|file| file.patch.as_str())
            .collect();
        assert!(
            control_patch.contains("FOREIGN_PLAN_CANARY") && control_patch.contains("+conv:two"),
            "the fixture's textconv must be live and able to read outside, or this test proves nothing: {control_patch}"
        );

        let bounded = local_checkout_diff(
            &LocalRepoCheckout {
                path: checkout.clone(),
                remote: "origin".to_owned(),
            },
            &auth,
            None,
            None,
            Some(&first),
            Some(&second),
        )
        .expect("local diff");
        let patch: String = bounded
            .files
            .iter()
            .map(|file| file.patch.as_str())
            .collect();
        assert!(
            patch.contains("-conv:one") && patch.contains("+conv:two"),
            "the project's textconv applied: {patch}"
        );
        assert!(
            !patch.contains("FOREIGN_PLAN_CANARY"),
            "the diff read a neighbour: {patch}"
        );

        // A converter that fails is an error naming the file, not an empty
        // patch that reads as "no change".
        run_git(
            &["config", "diff.fixture.textconv", "sh -c 'exit 3' --"],
            Some(&checkout),
            &auth,
        )
        .expect("failing textconv");
        let error = local_checkout_diff(
            &LocalRepoCheckout {
                path: checkout.clone(),
                remote: "origin".to_owned(),
            },
            &auth,
            None,
            None,
            Some(&first),
            Some(&second),
        )
        .err()
        .expect("a failed converter is disclosed");
        assert!(error.contains("notes.txt"), "{error}");
    }
}
