//! Disposable, read-only Memory Explorer snapshot boundary.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use super::agents_repo_read::{list_tip, AgentsRepoCheckout, AgentsRepoFile, AgentsRepoListing};
use crate::commands::project_git_exec::{run_git, run_git_bytes};

/// Independent text-read ceiling; draft and asset limits remain unchanged.
pub const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
static SNAPSHOTS: OnceLock<Mutex<HashMap<String, (String, AgentsRepoCheckout)>>> = OnceLock::new();

/// Authorized tree captured at one immutable commit.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerSnapshot {
    /// Opaque, process-local capability, additionally checked against current access.
    pub token: String,
    /// Tree and disclosed provenance.
    pub listing: AgentsRepoListing,
}

pub(crate) fn capture(scope: String, repo: AgentsRepoCheckout) -> Result<ExplorerSnapshot, String> {
    let listing = list_tip(&repo)?;
    let token = uuid::Uuid::new_v4().to_string();
    let mut snapshots = SNAPSHOTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    if snapshots.len() >= 32 {
        // No persistence: an evicted reader gets an explicit expired-snapshot error.
        snapshots.clear();
    }
    snapshots.insert(token.clone(), (scope, repo));
    Ok(ExplorerSnapshot { token, listing })
}

pub(crate) fn snapshot_repo(token: &str, scope: &str) -> Result<AgentsRepoCheckout, String> {
    let snapshots = SNAPSHOTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    let (authorized, repo) = snapshots
        .get(token)
        .ok_or("Snapshot expired; refresh Explore")?;
    if authorized != scope {
        return Err("Snapshot access or project source changed; refresh Explore".into());
    }
    Ok(repo.clone())
}

pub(crate) fn release(token: &str) {
    if let Ok(mut snapshots) = SNAPSHOTS.get_or_init(Default::default).lock() {
        snapshots.remove(token);
    }
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', '\0', ':'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Read only a regular blob in the captured tree, never a caller-selected object.
pub(crate) fn read(repo: &AgentsRepoCheckout, path: &str) -> Result<AgentsRepoFile, String> {
    if !valid_path(path) {
        return Err("Invalid repository-relative document path".into());
    }
    let record = run_git_bytes(
        &["ls-tree", "-z", &repo.tip, "--", path],
        Some(&repo.checkout),
        &repo.auth,
        &[],
    )?;
    let meta = String::from_utf8(record).map_err(|_| "Invalid tree entry")?;
    let fields: Vec<&str> = meta
        .split_once('\t')
        .map(|(meta, _)| meta.split_whitespace().collect())
        .unwrap_or_default();
    let mut file = AgentsRepoFile {
        path: path.into(),
        text: None,
        state: "not-on-main".into(),
        blob: None,
        commit: repo.tip.clone(),
        size: None,
        synced_at: repo.synced_at.clone(),
    };
    let [mode, kind, blob] = fields.as_slice() else {
        return Ok(file);
    };
    if *kind != "blob" || !matches!(*mode, "100644" | "100755") {
        file.state = "not-regular-file".into();
        return Ok(file);
    }
    let size: u64 = run_git(&["cat-file", "-s", blob], Some(&repo.checkout), &repo.auth)?
        .trim()
        .parse()
        .map_err(|_| "Invalid blob size")?;
    file.blob = Some((*blob).into());
    file.size = Some(size);
    if size > MAX_TEXT_BYTES {
        file.state = "too-large".into();
        return Ok(file);
    }
    let bytes = run_git_bytes(
        &["cat-file", "blob", blob],
        Some(&repo.checkout),
        &repo.auth,
        &[],
    )?;
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => {
            file.text = Some(text);
            file.state = "on-main".into();
        }
        _ => file.state = "not-text".into(),
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_paths_are_bounded() {
        assert!(valid_path("notes/nested file.md"));
        for path in [
            "../secret",
            "/tmp/a",
            "a/../b",
            "a\\b",
            "HEAD:a",
            "a\0b",
            "a//b",
        ] {
            assert!(!valid_path(path), "{path}");
        }
        assert_eq!(MAX_TEXT_BYTES, 4_194_304);
        assert_eq!(super::super::agents_repo_read::MAX_READ_BYTES, 60_000);
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::commands::project_git_exec::build_test_git_auth_config;

    #[test]
    fn immutable_regular_blobs_and_limits() {
        let checkout =
            std::env::temp_dir().join(format!("memory-explorer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&checkout).expect("fixture directory");
        let mut auth = build_test_git_auth_config().expect("test git");
        auth.set_commit_identity("Explorer Test", "explorer@example.invalid");
        run_git(&["init"], Some(&checkout), &auth).expect("init");
        std::fs::write(checkout.join("README.md"), "# Original\n").expect("readme");
        std::fs::write(
            checkout.join("boundary.md"),
            vec![b'x'; MAX_TEXT_BYTES as usize],
        )
        .expect("boundary");
        std::fs::write(
            checkout.join("over.md"),
            vec![b'x'; MAX_TEXT_BYTES as usize + 1],
        )
        .expect("over");
        std::fs::write(checkout.join("binary.md"), [0, 255]).expect("binary");
        #[cfg(unix)]
        std::os::unix::fs::symlink("README.md", checkout.join("link.md")).expect("symlink");
        run_git(&["add", "."], Some(&checkout), &auth).expect("add");
        run_git(&["commit", "-m", "fixture"], Some(&checkout), &auth).expect("commit");
        let tip = run_git(&["rev-parse", "HEAD"], Some(&checkout), &auth)
            .expect("tip")
            .trim()
            .to_owned();
        let repo = AgentsRepoCheckout {
            repo: "test".into(),
            branch: "main".into(),
            checkout: checkout.clone(),
            clone_url: "test".into(),
            tip,
            synced_at: None,
            auth,
        };
        let snapshot = capture("scope A".into(), repo.clone()).expect("capture");
        assert!(snapshot_repo(&snapshot.token, "scope B").is_err());
        std::fs::write(checkout.join("README.md"), "# Changed\n").expect("change");
        run_git(&["add", "."], Some(&checkout), &repo.auth).expect("add");
        run_git(&["commit", "-m", "moved"], Some(&checkout), &repo.auth).expect("commit");
        assert_eq!(
            read(&repo, "README.md").expect("read").text.as_deref(),
            Some("# Original\n")
        );
        assert_eq!(
            read(&repo, "boundary.md")
                .expect("boundary")
                .text
                .map(|s| s.len()),
            Some(MAX_TEXT_BYTES as usize)
        );
        assert_eq!(read(&repo, "over.md").expect("over").state, "too-large");
        assert_eq!(read(&repo, "binary.md").expect("binary").state, "not-text");
        assert_eq!(
            read(&repo, "missing.md").expect("missing").state,
            "not-on-main"
        );
        #[cfg(unix)]
        assert_eq!(
            read(&repo, "link.md").expect("link").state,
            "not-regular-file"
        );
        release(&snapshot.token);
        assert!(snapshot_repo(&snapshot.token, "scope A").is_err());
        let mut missing = repo.clone();
        missing.tip = "0000000000000000000000000000000000000000".into();
        assert!(read(&missing, "README.md").is_err());
        std::fs::remove_dir_all(checkout).expect("remove fixture");
    }
}
