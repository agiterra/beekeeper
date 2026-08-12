//! Resolution of local project checkouts under the configured repos roots.
//!
//! Shared by the project git commands (snapshots, sync status, push) and the
//! project terminal launcher. Explicitly imported/linked checkouts outside the
//! repos roots resolve through the persisted registry
//! (`project_repo_registry`) before the scan runs.

use super::project_repo_registry::{
    load_repo_registry, resolve_registered_checkout, LocalRepoCheckout, RepoCheckoutRegistry,
};
use crate::managed_agents::nest_dir;
use url::Url;

fn local_repo_name_candidate(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches(".git");
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn clone_url_repo_name(clone_url: &str) -> Option<String> {
    let parsed = Url::parse(clone_url).ok()?;
    let last_segment = parsed.path_segments()?.rfind(|part| !part.is_empty())?;
    local_repo_name_candidate(last_segment)
}

fn clone_url_owner_repo_name(clone_url: &str) -> Option<String> {
    let parsed = Url::parse(clone_url).ok()?;
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let [.., owner, repo] = parts.as_slice() else {
        return None;
    };
    local_repo_name_candidate(&format!(
        "{}--{}",
        local_repo_name_candidate(owner)?,
        local_repo_name_candidate(repo)?
    ))
}

pub(crate) fn normalized_clone_url(value: &str) -> &str {
    value.trim().trim_end_matches('/').trim_end_matches(".git")
}

/// Resolve a checkout's git config file, following a `gitdir:` pointer file
/// (linked worktrees). No containment check — callers that need one apply it
/// to the returned path.
fn resolve_git_config_path(repo_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let dot_git = repo_dir.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let pointer = std::fs::read_to_string(dot_git).ok()?;
        let git_dir = std::path::PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
        if git_dir.is_absolute() {
            git_dir
        } else {
            repo_dir.join(git_dir)
        }
    };
    let git_dir = git_dir.canonicalize().ok()?;
    git_dir.join("config").canonicalize().ok()
}

fn checkout_git_config(
    repo_dir: &std::path::Path,
    repos_root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let config = resolve_git_config_path(repo_dir)?;
    // The config must not resolve outside the scanned root (hostile symlink /
    // gitdir pointer); registry-resolved checkouts skip this by design.
    config.starts_with(repos_root).then_some(config)
}

/// Read the configured URL of `remote_name` from a checkout's git config.
pub(crate) fn checkout_remote_url(repo_dir: &std::path::Path, remote_name: &str) -> Option<String> {
    let config_path = resolve_git_config_path(repo_dir)?;
    remote_url_from_config(&std::fs::read_to_string(config_path).ok()?, remote_name)
}

fn remote_url_from_config(config: &str, remote_name: &str) -> Option<String> {
    let section = format!("[remote \"{remote_name}\"]");
    let mut in_remote = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_remote = line == section;
            continue;
        }
        if in_remote {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

fn checkout_origin_matches(
    repo_dir: &std::path::Path,
    repos_root: &std::path::Path,
    clone_url: &str,
) -> bool {
    let Some(config_path) = checkout_git_config(repo_dir, repos_root) else {
        return false;
    };
    let Ok(config) = std::fs::read_to_string(config_path) else {
        return false;
    };
    remote_url_from_config(&config, "origin")
        .is_some_and(|url| normalized_clone_url(&url) == normalized_clone_url(clone_url))
}

/// Does the checkout's `remote_name` point at `clone_url` (normalized)?
/// Registry variant of [`checkout_origin_matches`] — no containment check,
/// the user explicitly registered the path.
pub(crate) fn checkout_remote_matches(
    repo_dir: &std::path::Path,
    remote_name: &str,
    clone_url: &str,
) -> bool {
    checkout_remote_url(repo_dir, remote_name)
        .is_some_and(|url| normalized_clone_url(&url) == normalized_clone_url(clone_url))
}

pub(crate) fn local_repo_candidates(project_dtag: &str, clone_url: Option<&str>) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(candidate) = clone_url.and_then(clone_url_owner_repo_name) {
        candidates.push(candidate);
    }
    if let Some(candidate) = local_repo_name_candidate(project_dtag) {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    if let Some(candidate) = clone_url.and_then(clone_url_repo_name) {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

pub(crate) fn find_local_repo_dir(
    repos_dir: Option<&str>,
    project_dtag: &str,
    clone_url: Option<&str>,
) -> Result<Option<LocalRepoCheckout>, String> {
    find_local_repo_dir_with_registry(&load_repo_registry(), repos_dir, project_dtag, clone_url)
}

pub(crate) fn find_local_repo_dir_with_registry(
    registry: &RepoCheckoutRegistry,
    repos_dir: Option<&str>,
    project_dtag: &str,
    clone_url: Option<&str>,
) -> Result<Option<LocalRepoCheckout>, String> {
    // Explicit registrations win — they work even when reposDir is missing or
    // inaccessible, so resolve them before touching the roots.
    if let Some(checkout) =
        resolve_registered_checkout(registry, project_dtag, clone_url, checkout_remote_matches)
    {
        return Ok(Some(checkout));
    }

    let repos_roots = canonical_repos_roots(repos_dir)?;

    for repos_root in repos_roots {
        for candidate in local_repo_candidates(project_dtag, clone_url) {
            let candidate_path = repos_root.join(candidate);
            let Ok(candidate_path) = candidate_path.canonicalize() else {
                continue;
            };
            if !candidate_path.starts_with(&repos_root) || !candidate_path.is_dir() {
                continue;
            }
            if candidate_path.join(".git").exists()
                && clone_url
                    .map(|url| checkout_origin_matches(&candidate_path, &repos_root, url))
                    .unwrap_or(true)
            {
                return Ok(Some(LocalRepoCheckout {
                    path: candidate_path,
                    remote: "origin".to_string(),
                }));
            }
        }
    }
    Ok(None)
}

pub(crate) fn default_repos_root_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    candidates.extend(nest_dir().map(|path| path.join("REPOS")));
    candidates.extend(
        dirs::home_dir()
            .map(|home| home.join(".buzz").join("REPOS"))
            .filter(|path| !candidates.iter().any(|candidate| candidate == path)),
    );
    candidates
}

pub(crate) fn canonicalize_repos_root(
    repos_root: std::path::PathBuf,
) -> Result<std::path::PathBuf, String> {
    if !repos_root.is_absolute() {
        return Err("reposDir must be an absolute path".to_string());
    }
    let repos_root = repos_root
        .canonicalize()
        .map_err(|error| format!("reposDir is not accessible: {error}"))?;
    if !repos_root.is_dir() {
        return Err("reposDir is not a directory".to_string());
    }
    Ok(repos_root)
}

pub(crate) fn canonical_repos_roots(
    repos_dir: Option<&str>,
) -> Result<Vec<std::path::PathBuf>, String> {
    if let Some(repos_root) = repos_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
    {
        return canonicalize_repos_root(repos_root).map(|root| vec![root]);
    }

    let roots = default_repos_root_candidates()
        .into_iter()
        .filter_map(|root| canonicalize_repos_root(root).ok())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err("reposDir is not accessible".to_string());
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::project_repo_registry::{registry_key, RepoCheckoutEntry};

    const OWNER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn write_git_checkout(dir: &std::path::Path, origin_url: Option<&str>) {
        let git_dir = dir.join(".git");
        std::fs::create_dir_all(&git_dir).expect("create .git");
        let config = origin_url
            .map(|url| format!("[remote \"origin\"]\n\turl = {url}\n"))
            .unwrap_or_default();
        std::fs::write(git_dir.join("config"), config).expect("write config");
    }

    fn registry_with(owner: &str, dtag: &str, entry: RepoCheckoutEntry) -> RepoCheckoutRegistry {
        let mut registry = RepoCheckoutRegistry::default();
        registry.checkouts.insert(registry_key(owner, dtag), entry);
        registry
    }

    #[test]
    fn registry_hit_resolves_outside_the_repos_roots() {
        let repos = tempfile::tempdir().expect("repos root");
        let elsewhere = tempfile::tempdir().expect("external checkout");
        let checkout = elsewhere.path().join("widget");
        let clone_url = format!("https://relay.test/git/{OWNER}/widget");
        std::fs::create_dir_all(&checkout).expect("create checkout");
        write_git_checkout(&checkout, Some("https://github.com/example/widget.git"));
        // The buzz remote carries the relay URL; origin stays foreign.
        let git_config = checkout.join(".git").join("config");
        let mut config = std::fs::read_to_string(&git_config).expect("read config");
        config.push_str(&format!("[remote \"buzz\"]\n\turl = {clone_url}\n"));
        std::fs::write(&git_config, config).expect("extend config");

        let registry = registry_with(
            OWNER,
            "widget",
            RepoCheckoutEntry {
                path: checkout.clone(),
                remote: "buzz".to_string(),
                clone_url: clone_url.clone(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        );
        let hit = find_local_repo_dir_with_registry(
            &registry,
            Some(repos.path().to_str().expect("utf-8 root")),
            "widget",
            Some(&clone_url),
        )
        .expect("resolution succeeds")
        .expect("registry hit");
        assert_eq!(hit.remote, "buzz");
        assert_eq!(hit.path, checkout.canonicalize().expect("canonical"));
    }

    #[test]
    fn stale_registry_entry_falls_back_to_the_scan() {
        let repos = tempfile::tempdir().expect("repos root");
        let clone_url = format!("https://relay.test/git/{OWNER}/widget");
        // Scannable checkout inside the root, origin matching the clone URL.
        let scanned = repos.path().join("widget");
        std::fs::create_dir_all(&scanned).expect("create scanned checkout");
        write_git_checkout(&scanned, Some(clone_url.as_str()));

        // Registry entry pointing at a deleted directory.
        let registry = registry_with(
            OWNER,
            "widget",
            RepoCheckoutEntry {
                path: repos.path().join("deleted"),
                remote: "origin".to_string(),
                clone_url: clone_url.clone(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        );
        let hit = find_local_repo_dir_with_registry(
            &registry,
            Some(repos.path().to_str().expect("utf-8 root")),
            "widget",
            Some(&clone_url),
        )
        .expect("resolution succeeds")
        .expect("scan fallback hit");
        assert_eq!(hit.remote, "origin");
        assert_eq!(hit.path, scanned.canonicalize().expect("canonical"));
    }

    #[test]
    fn scan_hits_keep_using_the_origin_remote() {
        let repos = tempfile::tempdir().expect("repos root");
        let clone_url = format!("https://relay.test/git/{OWNER}/widget");
        let scanned = repos.path().join("widget");
        std::fs::create_dir_all(&scanned).expect("create scanned checkout");
        write_git_checkout(&scanned, Some(clone_url.as_str()));

        let hit = find_local_repo_dir_with_registry(
            &RepoCheckoutRegistry::default(),
            Some(repos.path().to_str().expect("utf-8 root")),
            "widget",
            Some(&clone_url),
        )
        .expect("resolution succeeds")
        .expect("scan hit");
        assert_eq!(hit.remote, "origin");
    }
}
