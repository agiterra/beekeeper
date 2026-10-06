//! Persisted registry of explicitly linked local checkouts.
//!
//! The repos-root scan (`project_repo_paths`) only discovers checkouts living
//! inside the configured `reposDir`. Importing or linking a repository from an
//! arbitrary folder records it here, keyed by the announcement coordinate, so
//! every git command resolves it without moving the checkout. Entries also
//! carry the remote name to use (`origin` for imported repos, `buzz` when the
//! user kept a foreign `origin` untouched).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::managed_agents::nest_dir;
use crate::managed_agents::storage::atomic_write_json;

pub(crate) const REPO_REGISTRY_VERSION: u32 = 1;
const REPO_REGISTRY_FILE: &str = "repo-checkouts.json";

/// A registered checkout: where it lives and which remote points at the relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoCheckoutEntry {
    /// Absolute worktree path chosen by the user.
    pub path: PathBuf,
    /// Git remote name whose URL points at the relay ("origin" or "buzz").
    pub remote: String,
    /// Relay clone URL at registration time — disambiguates same-dtag repos
    /// across relays, since lookups compare it against the requested URL.
    pub clone_url: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepoCheckoutRegistry {
    pub version: u32,
    /// Keyed by `<owner64hex-lowercase>:<dtag>`.
    #[serde(default)]
    pub checkouts: BTreeMap<String, RepoCheckoutEntry>,
}

impl Default for RepoCheckoutRegistry {
    fn default() -> Self {
        Self {
            version: REPO_REGISTRY_VERSION,
            checkouts: BTreeMap::new(),
        }
    }
}

/// A resolved local checkout: the worktree path plus the remote name the git
/// tooling must address (scan hits always use `origin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalRepoCheckout {
    pub path: PathBuf,
    pub remote: String,
}

pub(crate) fn registry_key(owner: &str, dtag: &str) -> String {
    format!("{}:{dtag}", owner.trim().to_lowercase())
}

/// Remote names are handed to `git` as positional arguments — keep them to a
/// conservative allowlist so a corrupted registry can never smuggle a flag.
pub(crate) fn validate_remote_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let valid_first = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let valid_rest = name
        .chars()
        .skip(1)
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid_first && valid_rest {
        Ok(())
    } else {
        Err(format!("invalid git remote name: {name:?}"))
    }
}

fn registry_path() -> Option<PathBuf> {
    nest_dir().map(|dir| dir.join(REPO_REGISTRY_FILE))
}

/// Load the registry. Missing or corrupt files yield an empty registry — a
/// bad registry must degrade to the repos-root scan, never break every git
/// command.
pub(crate) fn load_repo_registry() -> RepoCheckoutRegistry {
    load_repo_registry_at(registry_path().as_deref())
}

fn load_repo_registry_at(path: Option<&Path>) -> RepoCheckoutRegistry {
    let Some(path) = path else {
        return RepoCheckoutRegistry::default();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return RepoCheckoutRegistry::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_repo_registry_at(path: &Path, registry: &RepoCheckoutRegistry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|error| format!("failed to serialize repo registry: {error}"))?;
    atomic_write_json(path, &payload)
}

/// Insert (or replace) a checkout registration and persist the registry.
pub(crate) fn register_checkout(
    owner: &str,
    dtag: &str,
    path: &Path,
    remote: &str,
    clone_url: &str,
) -> Result<(), String> {
    validate_remote_name(remote)?;
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("checkout path is not accessible: {error}"))?;
    if !canonical.is_dir() {
        return Err("checkout path is not a directory".to_string());
    }
    let registry_file =
        registry_path().ok_or_else(|| "cannot resolve the Beekeeper nest directory".to_string())?;
    let mut registry = load_repo_registry_at(Some(&registry_file));
    registry.version = REPO_REGISTRY_VERSION;
    registry.checkouts.insert(
        registry_key(owner, dtag),
        RepoCheckoutEntry {
            path: canonical,
            remote: remote.to_string(),
            clone_url: clone_url.to_string(),
            updated_at: crate::util::now_iso(),
        },
    );
    save_repo_registry_at(&registry_file, &registry)
}

/// Resolve a checkout from the registry. Every failure is a silent `None` so
/// stale entries fall through to the repos-root scan.
///
/// With a relay-shaped clone URL the entry must match it (normalized) both in
/// the stored registration and in the checkout's actual remote config. With
/// an external clone URL (a fork-style announcement naming its upstream) or
/// none at all (snapshot/diff/terminal callers), a dtag resolves only when
/// exactly one entry for that dtag passes — validated against the entry's own
/// stored relay URL; ambiguity is a miss.
pub(crate) fn resolve_registered_checkout(
    registry: &RepoCheckoutRegistry,
    project_dtag: &str,
    clone_url: Option<&str>,
    remote_url_matches: impl Fn(&Path, &str, &str) -> bool,
) -> Option<LocalRepoCheckout> {
    let entry_checkout = |entry: &RepoCheckoutEntry| -> Option<LocalRepoCheckout> {
        validate_remote_name(&entry.remote).ok()?;
        let path = entry.path.canonicalize().ok()?;
        if !path.is_dir() || !path.join(".git").exists() {
            return None;
        }
        Some(LocalRepoCheckout {
            path,
            remote: entry.remote.clone(),
        })
    };
    // Fallback used whenever no relay URL keys the lookup: the checkout's
    // registered remote must still point at the URL it was registered with.
    let unique_dtag_match = || {
        let suffix = format!(":{project_dtag}");
        let mut matches = registry
            .checkouts
            .iter()
            .filter(|(key, _)| key.ends_with(&suffix))
            .filter_map(|(_, entry)| {
                let checkout = entry_checkout(entry)?;
                remote_url_matches(&checkout.path, &checkout.remote, &entry.clone_url)
                    .then_some(checkout)
            });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    };

    match clone_url {
        Some(clone_url) => {
            match super::project_git_exec::clone_url_owner(clone_url) {
                Some(owner) => {
                    let entry = registry
                        .checkouts
                        .get(&registry_key(&owner, project_dtag))?;
                    if normalized(&entry.clone_url) != normalized(clone_url) {
                        return None;
                    }
                    let checkout = entry_checkout(entry)?;
                    remote_url_matches(&checkout.path, &checkout.remote, clone_url)
                        .then_some(checkout)
                }
                // The caller passed the announcement's clone tag verbatim and
                // it names an external upstream (e.g. the GitHub repo this
                // was forked from) — linked checkouts are keyed by the relay
                // URL, so match by dtag instead.
                None => unique_dtag_match(),
            }
        }
        None => unique_dtag_match(),
    }
}

fn normalized(value: &str) -> &str {
    value.trim().trim_end_matches('/').trim_end_matches(".git")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry(entries: &[(&str, &str, RepoCheckoutEntry)]) -> RepoCheckoutRegistry {
        let mut registry = RepoCheckoutRegistry::default();
        for (owner, dtag, entry) in entries {
            registry
                .checkouts
                .insert(registry_key(owner, dtag), entry.clone());
        }
        registry
    }

    fn git_checkout_in(dir: &Path) -> PathBuf {
        let repo = dir.join("checkout");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        repo
    }

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn clone_url(dtag: &str) -> String {
        format!("https://relay.test/git/{OWNER}/{dtag}")
    }

    fn entry(path: &Path, remote: &str, url: &str) -> RepoCheckoutEntry {
        RepoCheckoutEntry {
            path: path.to_path_buf(),
            remote: remote.to_string(),
            clone_url: url.to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn validate_remote_name_rejects_flag_shaped_values() {
        assert!(validate_remote_name("origin").is_ok());
        assert!(validate_remote_name("buzz").is_ok());
        assert!(validate_remote_name("up-stream_2.0").is_ok());
        assert!(validate_remote_name("--upload-pack=/bin/sh").is_err());
        assert!(validate_remote_name("").is_err());
        assert!(validate_remote_name("has space").is_err());
        assert!(validate_remote_name(".dot").is_err());
    }

    #[test]
    fn resolve_matches_clone_url_and_remote() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git_checkout_in(dir.path());
        let url = clone_url("widget");
        let registry = make_registry(&[(OWNER, "widget", entry(&repo, "buzz", &url))]);

        let hit = resolve_registered_checkout(&registry, "widget", Some(&url), |_, _, _| true)
            .expect("registry hit");
        assert_eq!(hit.remote, "buzz");
        assert_eq!(hit.path, repo.canonicalize().unwrap());

        // Stored clone URL pointing at another relay never matches.
        let other = format!("https://other.test/git/{OWNER}/widget");
        assert!(
            resolve_registered_checkout(&registry, "widget", Some(&other), |_, _, _| true)
                .is_none()
        );

        // Remote-config mismatch is a miss.
        assert!(
            resolve_registered_checkout(&registry, "widget", Some(&url), |_, _, _| false).is_none()
        );
    }

    #[test]
    fn resolve_rejects_stale_paths_and_bad_remotes() {
        let dir = tempfile::tempdir().unwrap();
        let url = clone_url("widget");
        // Path without a .git directory.
        let bare = dir.path().join("not-a-repo");
        std::fs::create_dir_all(&bare).unwrap();
        let registry = make_registry(&[(OWNER, "widget", entry(&bare, "origin", &url))]);
        assert!(
            resolve_registered_checkout(&registry, "widget", Some(&url), |_, _, _| true).is_none()
        );

        // Missing path.
        let gone = dir.path().join("deleted");
        let registry = make_registry(&[(OWNER, "widget", entry(&gone, "origin", &url))]);
        assert!(
            resolve_registered_checkout(&registry, "widget", Some(&url), |_, _, _| true).is_none()
        );

        // Flag-shaped remote name recorded by a corrupt registry.
        let repo = git_checkout_in(dir.path());
        let registry = make_registry(&[(OWNER, "widget", entry(&repo, "--evil", &url))]);
        assert!(
            resolve_registered_checkout(&registry, "widget", Some(&url), |_, _, _| true).is_none()
        );
    }

    #[test]
    fn external_clone_url_falls_back_to_the_dtag_match() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git_checkout_in(dir.path());
        let relay_url = clone_url("widget");
        let registry = make_registry(&[(OWNER, "widget", entry(&repo, "buzz", &relay_url))]);

        // A fork-style announcement passes its GitHub upstream verbatim; the
        // stored relay URL is what the registered remote must match.
        let compared_against = std::cell::RefCell::new(Vec::new());
        let hit = resolve_registered_checkout(
            &registry,
            "widget",
            Some("https://github.com/upstream/widget.git"),
            |_, _, url| {
                compared_against.borrow_mut().push(url.to_string());
                true
            },
        )
        .expect("dtag fallback hit");
        assert_eq!(hit.remote, "buzz");
        assert_eq!(*compared_against.borrow(), std::slice::from_ref(&relay_url));

        // A stale registered remote is still a miss.
        assert!(resolve_registered_checkout(
            &registry,
            "widget",
            Some("https://github.com/upstream/widget.git"),
            |_, _, _| false,
        )
        .is_none());
    }

    #[test]
    fn dtag_only_lookup_requires_a_unique_match() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git_checkout_in(dir.path());
        let url = clone_url("widget");
        let registry = make_registry(&[(OWNER, "widget", entry(&repo, "origin", &url))]);
        let hit = resolve_registered_checkout(&registry, "widget", None, |_, _, _| true)
            .expect("unique dtag hit");
        assert_eq!(hit.remote, "origin");

        // A second owner registering the same dtag makes the lookup ambiguous.
        let other_owner = "b".repeat(64);
        let dir2 = tempfile::tempdir().unwrap();
        let repo2 = git_checkout_in(dir2.path());
        let registry = make_registry(&[
            (OWNER, "widget", entry(&repo, "origin", &url)),
            (
                &other_owner,
                "widget",
                entry(
                    &repo2,
                    "origin",
                    &format!("https://relay.test/git/{other_owner}/widget"),
                ),
            ),
        ]);
        assert!(resolve_registered_checkout(&registry, "widget", None, |_, _, _| true).is_none());
    }

    #[test]
    fn registry_roundtrips_and_tolerates_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("repo-checkouts.json");
        let repo = git_checkout_in(dir.path());
        let url = clone_url("widget");
        let mut registry = RepoCheckoutRegistry::default();
        registry
            .checkouts
            .insert(registry_key(OWNER, "widget"), entry(&repo, "origin", &url));
        save_repo_registry_at(&file, &registry).unwrap();
        let loaded = load_repo_registry_at(Some(&file));
        assert_eq!(loaded.checkouts.len(), 1);
        assert_eq!(
            loaded.checkouts[&registry_key(OWNER, "widget")].remote,
            "origin"
        );

        std::fs::write(&file, b"{not json").unwrap();
        let recovered = load_repo_registry_at(Some(&file));
        assert!(recovered.checkouts.is_empty());
    }
}
