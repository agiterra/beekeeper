//! NIP-AD read routes: a tree listing and a raw blob from a relay-hosted
//! repository, for clients that hold no git — Mobile, and `bee` reading the
//! agents repository's tip before drafting against it.
//!
//! ```text
//! GET /git/{owner}/{repo}/tree/{ref}[/{path}]  → JSON {commit, path, entries[]}
//! GET /git/{owner}/{repo}/raw/{ref}/{path}     → the blob's bytes
//! ```
//!
//! These are git content read-side — the same exception `AGENTS.md`
//! makes for smart HTTP — not a new JSON API over a Nostr concern. They
//! share the smart-HTTP routes' NIP-98 [`super::transport::GitAuth`]
//! extractor (one repo-root token serves every read) and their
//! [`super::transport::authorize_git_read`] gate, and answer a denial with
//! the same generic 404. `ref` is `refs/heads/<branch>` or a 40-hex commit;
//! nothing else resolves, so no client can read a stash, a note or an
//! arbitrary revision expression through here. A raw read is capped at
//! [`MAX_RAW_BYTES`] and answered `413` beyond it. Responses are
//! `Cache-Control: no-store`: `main` moves.
//!
//! Every request hydrates the repository from the object store
//! ([`hydrate_for_read`]), as `info/refs` does. That is the cost of having
//! no persistent checkout on the relay; a client polling these routes pays
//! it on every call, and a hydration cache is the follow-up if one starts to.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::hydrate::{hydrate_for_read, run_git_stdout, HydrationOptions};
use super::manifest::{is_hex_oid, is_safe_refname};
use super::transport::{
    acquire_git_permit, authorize_git_read, hydrate_error_to_response, validate_repo_id, GitAuth,
};
use crate::state::AppState;

/// The largest blob `raw` serves. A role, plan or manifest is kilobytes;
/// anything past this is not a document a phone edits.
pub const MAX_RAW_BYTES: u64 = 1024 * 1024;

/// Path parameters: `{owner}/{repo}` plus the `{ref}[/{path}]` tail.
#[derive(Deserialize)]
pub struct ReadParams {
    owner: String,
    repo: String,
    rest: String,
}

/// One entry of a tree listing.
#[derive(Serialize)]
pub struct TreeEntry {
    /// Path relative to the repository root.
    pub path: String,
    /// `blob`, `tree` or `commit` (a submodule).
    pub kind: String,
    /// Object id (40 hex).
    pub oid: String,
    /// Blob size in bytes; null for a tree or submodule.
    pub size: Option<u64>,
}

/// A tree listing.
#[derive(Serialize)]
pub struct TreeListing {
    /// The commit the ref resolved to.
    pub commit: String,
    /// The subtree listed (`""` for the root).
    pub path: String,
    /// Every blob and tree under it, recursively, in `git ls-tree` order.
    pub entries: Vec<TreeEntry>,
}

/// Split `{ref}[/{path}]`: a ref is `refs/heads/<branch>` (three segments)
/// or a 40-hex commit (one); the rest is the path.
#[allow(clippy::result_large_err)] // Response is the natural error type for axum handlers
fn split_ref_and_path(rest: &str) -> Result<(String, String), Response> {
    let bad = || {
        (
            StatusCode::BAD_REQUEST,
            "ref must be refs/heads/<branch> or a 40-hex commit",
        )
            .into_response()
    };
    let segments: Vec<&str> = rest.split('/').collect();
    let (reference, path_segments): (String, &[&str]) = match segments.as_slice() {
        ["refs", "heads", branch, tail @ ..] => {
            let reference = format!("refs/heads/{branch}");
            if !is_safe_refname(&reference) {
                return Err(bad());
            }
            (reference, tail)
        }
        [oid, tail @ ..] if is_hex_oid(oid) => ((*oid).to_owned(), tail),
        _ => return Err(bad()),
    };
    if path_segments.iter().any(|segment| {
        segment.is_empty() || *segment == "." || *segment == ".." || segment.contains('\0')
    }) {
        return Err((StatusCode::BAD_REQUEST, "path has an empty or dot segment").into_response());
    }
    Ok((reference, path_segments.join("/")))
}

struct ReadContext {
    repo: super::hydrate::HydratedRepo,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

/// Authorize, then hydrate the repository for reading.
#[allow(clippy::result_large_err)] // Response is the natural error type for axum handlers
async fn open_for_read(
    state: &Arc<AppState>,
    auth: &GitAuth,
    owner: &str,
    repo: &str,
) -> Result<ReadContext, Response> {
    let repo_name = validate_repo_id(owner, repo)?;
    authorize_git_read(
        &state.db,
        auth.tenant.community(),
        &auth.pubkey,
        auth.attested_owner.as_ref(),
        owner,
        repo_name,
    )
    .await?;
    let permit = acquire_git_permit(state, "read_routes")?;
    let hydrated = match hydrate_for_read(
        &state.git_store,
        &auth.tenant,
        owner,
        repo,
        HydrationOptions {
            pack_cache: &state.git_pack_cache,
            scratch_dir: &state.config.git_repo_path,
            max_pack_bytes: state.config.git_max_pack_bytes,
            max_repo_bytes: state.config.git_max_repo_bytes,
        },
    )
    .await
    {
        Ok(Some(repo)) => repo,
        Ok(None) => return Err((StatusCode::NOT_FOUND, "repository not found").into_response()),
        Err(e) => return Err(hydrate_error_to_response(owner, repo, e)),
    };
    Ok(ReadContext {
        repo: hydrated,
        _permit: permit,
    })
}

/// Resolve `reference` to a commit in the hydrated repository; a ref the
/// repository does not have is a 404.
#[allow(clippy::result_large_err)] // Response is the natural error type for axum handlers
async fn resolve_commit(repo_path: &std::path::Path, reference: &str) -> Result<String, Response> {
    let spec = format!("{reference}^{{commit}}");
    match run_git_stdout(repo_path, &["rev-parse", "--verify", "--quiet", &spec]).await {
        Ok(out) => {
            let sha = String::from_utf8_lossy(&out).trim().to_owned();
            if is_hex_oid(&sha) {
                Ok(sha)
            } else {
                Err((StatusCode::NOT_FOUND, "ref not found").into_response())
            }
        }
        Err(_) => Err((StatusCode::NOT_FOUND, "ref not found").into_response()),
    }
}

/// `GET /git/{owner}/{repo}/tree/{ref}[/{path}]`.
pub async fn tree(
    State(state): State<Arc<AppState>>,
    auth: GitAuth,
    AxumPath(params): AxumPath<ReadParams>,
) -> Result<Response, Response> {
    let (reference, path) = split_ref_and_path(&params.rest)?;
    let ctx = open_for_read(&state, &auth, &params.owner, &params.repo).await?;
    let commit = resolve_commit(ctx.repo.path(), &reference).await?;
    let spec = if path.is_empty() {
        commit.clone()
    } else {
        format!("{commit}:{path}")
    };
    let out =
        match run_git_stdout(ctx.repo.path(), &["ls-tree", "-r", "-t", "-l", "-z", &spec]).await {
            Ok(out) => out,
            Err(_) => return Err((StatusCode::NOT_FOUND, "path not found").into_response()),
        };
    let mut entries = Vec::new();
    for record in out
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        // `<mode> <type> <oid> <size>\t<path>`; size is `-` for a tree.
        let record = String::from_utf8_lossy(record);
        let Some((meta, name)) = record.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = meta.split_whitespace().collect();
        let [_mode, kind, oid, size] = fields.as_slice() else {
            continue;
        };
        entries.push(TreeEntry {
            path: if path.is_empty() {
                name.to_owned()
            } else {
                format!("{path}/{name}")
            },
            kind: (*kind).to_owned(),
            oid: (*oid).to_owned(),
            size: size.parse().ok(),
        });
    }
    let listing = TreeListing {
        commit,
        path,
        entries,
    };
    let body = serde_json::to_vec(&listing).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "listing did not serialize",
        )
            .into_response()
    })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "response").into_response())
}

/// `GET /git/{owner}/{repo}/raw/{ref}/{path}`.
pub async fn raw(
    State(state): State<Arc<AppState>>,
    auth: GitAuth,
    AxumPath(params): AxumPath<ReadParams>,
) -> Result<Response, Response> {
    let (reference, path) = split_ref_and_path(&params.rest)?;
    if path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "raw needs a file path").into_response());
    }
    let ctx = open_for_read(&state, &auth, &params.owner, &params.repo).await?;
    let commit = resolve_commit(ctx.repo.path(), &reference).await?;
    let spec = format!("{commit}:{path}");
    let oid = match run_git_stdout(
        ctx.repo.path(),
        &["rev-parse", "--verify", "--quiet", &spec],
    )
    .await
    {
        Ok(out) => String::from_utf8_lossy(&out).trim().to_owned(),
        Err(_) => return Err((StatusCode::NOT_FOUND, "path not found").into_response()),
    };
    let kind = match run_git_stdout(ctx.repo.path(), &["cat-file", "-t", &oid]).await {
        Ok(out) => String::from_utf8_lossy(&out).trim().to_owned(),
        Err(_) => return Err((StatusCode::NOT_FOUND, "path not found").into_response()),
    };
    if kind != "blob" {
        return Err((StatusCode::BAD_REQUEST, "path is not a file").into_response());
    }
    let size: u64 = match run_git_stdout(ctx.repo.path(), &["cat-file", "-s", &oid]).await {
        Ok(out) => String::from_utf8_lossy(&out)
            .trim()
            .parse()
            .unwrap_or(u64::MAX),
        Err(_) => return Err((StatusCode::NOT_FOUND, "path not found").into_response()),
    };
    if size > MAX_RAW_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("file is {size} bytes; raw serves at most {MAX_RAW_BYTES}"),
        )
            .into_response());
    }
    let bytes = match run_git_stdout(ctx.repo.path(), &["cat-file", "blob", &oid]).await {
        Ok(out) => out,
        Err(_) => return Err((StatusCode::NOT_FOUND, "path not found").into_response()),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Git-Commit", commit)
        .header("X-Git-Blob", oid)
        .body(Body::from(bytes))
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "response").into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_branches_or_commits_and_paths_are_clean() {
        let (reference, path) = split_ref_and_path("refs/heads/main").expect("branch");
        assert_eq!((reference.as_str(), path.as_str()), ("refs/heads/main", ""));
        let (reference, path) =
            split_ref_and_path("refs/heads/main/plans/rpg.md").expect("branch+path");
        assert_eq!(
            (reference.as_str(), path.as_str()),
            ("refs/heads/main", "plans/rpg.md")
        );
        let sha = "a".repeat(40);
        let (reference, path) = split_ref_and_path(&format!("{sha}/team.yml")).expect("sha+path");
        assert_eq!(
            (reference.as_str(), path.as_str()),
            (sha.as_str(), "team.yml")
        );
        for bad in [
            "main",
            "refs/tags/v1",
            "refs/heads/main/../x",
            "refs/heads/main//x",
            "HEAD",
            "refs/heads/main/./x",
            "ABCDEF",
        ] {
            assert!(split_ref_and_path(bad).is_err(), "{bad}");
        }
    }
}
