//! Exact Git-object construction for a checked setup snapshot.
//!
//! The snapshot verifier hands this module owned bytes.  It never stages a
//! mutable worktree: blobs are written with `hash-object --stdin`, the index is
//! assembled from those object IDs, and the resulting tree is read back before
//! its commit is accepted.

use std::collections::BTreeMap;
use std::path::Path;

use super::{
    candidate_dir, external, invalid, ProjectTeamSetupDraft, PublicationDestination, SetupError,
};
use crate::commands::project_git_exec::{
    build_git_auth_config_for_keys, run_git, run_git_bytes, GitAuthConfig,
};
use crate::managed_agents::packs_cache;

const COMMIT_MESSAGE: &str = "publish checked project-team snapshot";

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn pack_file(pack_path: &str, relative: &str) -> String {
    format!("{pack_path}/{relative}")
}

fn object_id(checkout: &Path, auth: &GitAuthConfig, bytes: &[u8]) -> Result<String, SetupError> {
    let value = run_git_bytes(
        &["hash-object", "-w", "--stdin"],
        Some(checkout),
        auth,
        bytes,
    )
    .map_err(external)?;
    let value = std::str::from_utf8(&value)
        .map_err(|_| external("Git returned a non-UTF-8 object ID."))?
        .trim();
    if !valid_sha(value) {
        return Err(external("Git did not return a valid blob object ID."));
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeEntry {
    mode: String,
    kind: String,
    object: String,
}

fn tree_entries(
    checkout: &Path,
    auth: &GitAuthConfig,
    treeish: &str,
) -> Result<BTreeMap<String, TreeEntry>, SetupError> {
    let bytes = run_git_bytes(&["ls-tree", "-r", "-z", treeish], Some(checkout), auth, &[])
        .map_err(external)?;
    let mut entries = BTreeMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| external("Git returned a malformed tree entry."))?;
        let (metadata, path_with_separator) = record.split_at(separator);
        let path = &path_with_separator[1..];
        let metadata = std::str::from_utf8(metadata)
            .map_err(|_| external("Git returned non-UTF-8 tree metadata."))?;
        let mut fields = metadata.split_whitespace();
        let (Some(mode), Some(kind), Some(object), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(external("Git returned a malformed tree entry."));
        };
        let path = std::str::from_utf8(path)
            .map_err(|_| external("Git returned a non-UTF-8 snapshot path."))?;
        if entries
            .insert(
                path.to_string(),
                TreeEntry {
                    mode: mode.to_string(),
                    kind: kind.to_string(),
                    object: object.to_string(),
                },
            )
            .is_some()
        {
            return Err(external("Git returned a duplicate tree path."));
        }
    }
    Ok(entries)
}

fn verify_tree(
    checkout: &Path,
    auth: &GitAuthConfig,
    treeish: &str,
    destination: &PublicationDestination,
    files: &[(String, Vec<u8>)],
) -> Result<(), SetupError> {
    let pack_path = packs_cache::validate_pack_path(&destination.pack_path).map_err(invalid)?;
    let tree = tree_entries(checkout, auth, treeish)?;
    let expected = files
        .iter()
        .map(|(relative, bytes)| (pack_file(&pack_path, relative), bytes))
        .collect::<BTreeMap<_, _>>();
    let actual_pack = tree
        .iter()
        .filter(|(path, _)| *path == &pack_path || path.starts_with(&format!("{pack_path}/")))
        .map(|(path, entry)| (path.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    if actual_pack.len() != expected.len() || actual_pack.keys().ne(expected.keys()) {
        return Err(invalid(
            "The candidate tree contains pack paths other than the verified snapshot.",
        ));
    }
    for (path, bytes) in expected {
        let entry = tree
            .get(&path)
            .ok_or_else(|| invalid("The candidate omitted a verified snapshot file."))?;
        if entry.mode != "100644" || entry.kind != "blob" {
            return Err(invalid(
                "A candidate snapshot entry is not a regular Git blob.",
            ));
        }
        let actual = run_git_bytes(
            &["cat-file", "blob", &entry.object],
            Some(checkout),
            auth,
            &[],
        )
        .map_err(external)?;
        if actual != *bytes {
            return Err(invalid(
                "A Git filter or mutable checkout changed a verified snapshot file.",
            ));
        }
    }
    match destination.base_commit.as_deref() {
        Some(base) => {
            let base_tree = tree_entries(checkout, auth, base)?;
            for (path, entry) in &base_tree {
                if path == &pack_path || path.starts_with(&format!("{pack_path}/")) {
                    continue;
                }
                if tree.get(path) != Some(entry) {
                    return Err(invalid(
                        "The candidate changed a non-pack path from the captured base.",
                    ));
                }
            }
            if tree.iter().any(|(path, _)| {
                !(path == &pack_path
                    || path.starts_with(&format!("{pack_path}/"))
                    || base_tree.contains_key(path))
            }) {
                return Err(invalid(
                    "The candidate added a non-pack path outside the verified snapshot.",
                ));
            }
        }
        None if tree.iter().any(|(path, _)| {
            !(path == &pack_path || path.starts_with(&format!("{pack_path}/")))
        }) =>
        {
            return Err(invalid(
                "A new packs repository may contain only the verified pack snapshot.",
            ));
        }
        None => {}
    }
    Ok(())
}

fn prepare_index(
    checkout: &Path,
    auth: &GitAuthConfig,
    destination: &PublicationDestination,
    files: &[(String, Vec<u8>)],
) -> Result<(), SetupError> {
    let pack_path = packs_cache::validate_pack_path(&destination.pack_path).map_err(invalid)?;
    if let Some(base) = destination.base_commit.as_deref() {
        run_git(&["read-tree", base], Some(checkout), auth).map_err(external)?;
        run_git(
            &["rm", "--cached", "-r", "--ignore-unmatch", "--", &pack_path],
            Some(checkout),
            auth,
        )
        .map_err(external)?;
    }
    for (relative, bytes) in files {
        let blob = object_id(checkout, auth, bytes)?;
        let cache_info = format!("100644,{blob},{}", pack_file(&pack_path, relative));
        run_git(
            &["update-index", "--add", "--cacheinfo", &cache_info],
            Some(checkout),
            auth,
        )
        .map_err(external)?;
    }
    Ok(())
}

/// Construct and prove one immutable candidate from the snapshot's owned
/// buffers.  The directory is only a Git object database; no pack bytes are
/// copied to its worktree.
pub(super) fn create_candidate(
    draft: &ProjectTeamSetupDraft,
    publication_id: &str,
    destination: &PublicationDestination,
    files: &[(String, Vec<u8>)],
    owner: &nostr::Keys,
) -> Result<String, SetupError> {
    let candidate = candidate_dir(draft, publication_id)?;
    if candidate.exists() {
        return Err(invalid(
            "The persisted candidate directory already exists without a journaled commit.",
        ));
    }
    let parent = candidate
        .parent()
        .ok_or_else(|| invalid("Missing candidate parent."))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".candidate-")
        .tempdir_in(parent)?;
    let checkout = temporary.path();
    let mut auth = build_git_auth_config_for_keys(owner).map_err(external)?;
    let owner_short: String = draft.owner_pubkey.chars().take(8).collect();
    auth.set_commit_identity(
        format!("Beekeeper {owner_short}"),
        format!("{owner_short}@beekeeper.local"),
    );
    if let Some(base) = destination.base_commit.as_deref() {
        let (repo_owner, repo_id) =
            packs_cache::parse_repo_coordinate(&destination.repo_ref).map_err(invalid)?;
        let remote = packs_cache::packs_clone_url(
            &crate::relay::relay_http_base_url(&draft.relay_url),
            &repo_owner,
            &repo_id,
        );
        run_git(
            &["clone", "--quiet", "--no-checkout", "--", &remote, "."],
            Some(checkout),
            &auth,
        )
        .map_err(external)?;
        // Resolve the captured SHA from the repository object database before
        // changing its index. No checkout or clean filter is involved.
        run_git(
            &["cat-file", "-e", &format!("{base}^{{commit}}")],
            Some(checkout),
            &auth,
        )
        .map_err(external)?;
    } else {
        run_git(&["init", "--quiet"], Some(checkout), &auth).map_err(external)?;
    }
    prepare_index(checkout, &auth, destination, files)?;
    let tree = run_git(&["write-tree"], Some(checkout), &auth).map_err(external)?;
    let tree = tree.trim();
    let mut commit_args = vec!["commit-tree", tree, "-m", COMMIT_MESSAGE];
    if let Some(base) = destination.base_commit.as_deref() {
        commit_args.extend(["-p", base]);
    }
    let sha = run_git(&commit_args, Some(checkout), &auth).map_err(external)?;
    let sha = sha.trim().to_string();
    if !valid_sha(&sha) {
        return Err(external(
            "Git did not return an immutable candidate commit.",
        ));
    }
    run_git(&["update-ref", "HEAD", &sha], Some(checkout), &auth).map_err(external)?;
    verify_tree(checkout, &auth, &sha, destination, files)?;
    // Materialize only after the immutable tree has been constructed and
    // proven. This is a diagnostic/recovery checkout, never an input to Git
    // staging, so a base `.gitattributes` filter cannot alter the commit.
    run_git(
        &["checkout", "--quiet", "--detach", &sha],
        Some(checkout),
        &auth,
    )
    .map_err(external)?;
    // Keep a diagnostic checkout for recovery and human inspection, but write
    // the owned snapshot buffers after checkout so its attributes cannot make
    // even that copy disagree with the committed blobs. It is never staged.
    let pack_path = packs_cache::validate_pack_path(&destination.pack_path).map_err(invalid)?;
    for (relative, bytes) in files {
        let path = checkout.join(&pack_path).join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| invalid("Missing diagnostic snapshot parent."))?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(path, bytes)?;
    }
    std::fs::rename(checkout, &candidate)?;
    temporary.disable_cleanup(true);
    Ok(sha)
}

/// Recover the one crash window between finalizing the candidate directory and
/// recording its commit. The recovered HEAD must still prove byte-for-byte
/// equality with the reverified snapshot before it is journaled.
pub(super) fn recover_candidate(
    draft: &ProjectTeamSetupDraft,
    publication_id: &str,
    destination: &PublicationDestination,
    files: &[(String, Vec<u8>)],
    owner: &nostr::Keys,
) -> Result<Option<String>, SetupError> {
    let candidate = candidate_dir(draft, publication_id)?;
    if !candidate.exists() {
        return Ok(None);
    }
    let auth = build_git_auth_config_for_keys(owner).map_err(external)?;
    let sha = run_git(&["rev-parse", "HEAD"], Some(&candidate), &auth).map_err(external)?;
    let sha = sha.trim().to_string();
    if !valid_sha(&sha) {
        return Err(invalid("The recovered candidate has no immutable HEAD."));
    }
    verify_tree(&candidate, &auth, &sha, destination, files)?;
    Ok(Some(sha))
}
