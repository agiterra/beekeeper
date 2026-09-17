//! Spec § 4.9: a seat runs `main`'s roles by default, and a worktree's
//! branch overrides a role only when a **committed** change on that branch
//! alters what the role's composition reads.
//!
//! The rule is decided by composing, not by guessing at paths: the role is
//! composed from the branch commit's `<path>` tree — read out of git objects
//! with `git show <sha>:<file>`, never from the working copy — and staged
//! beside `main`'s composition. Equal digests mean the branch changed
//! nothing the role reads, however many other files moved; different digests
//! mean an override, stamped with the branch commit as `packRef.sha` and
//! `branch-override` as its provenance kind.
//!
//! Two things never happen here:
//!
//! - **Uncommitted edits are never in effect.** A dirty `<path>` is reported
//!   as a fact for the caller to disclose ("commit them, then restart"); the
//!   composition still comes from a commit the wire can name.
//! - **A worktree of some other repository never overrides.** The seat's
//!   `origin` must be the packs repository the project's 30624 names; a tree
//!   cut from a different repository has nothing to say about these roles.

use std::path::Path;

use crate::commands::project_git_exec::{run_git, run_git_bytes, GitAuthConfig};

use super::{
    locate_role_source, pack_cache_dir_name, pack_ref_path, packs_clone_url, parse_repo_coordinate,
    stage_composed_pack, validate_pack_path, ProjectPackSource, SourceProvenance,
    StagedComposedPack, TemplateCatalog,
};

/// The provenance kind a branch override is stamped with.
pub const BRANCH_OVERRIDE_KIND: &str = "branch-override";

/// The directory under the packs cache holding branch commits' role trees:
/// `<packs root>/branch/<owner8>-<id>-<sha>/<path>/…`.
pub const BRANCH_TREES_DIR: &str = "branch";

/// The sentence disclosed when a worktree has uncommitted role edits.
pub const UNCOMMITTED_ROLE_EDITS: &str =
    "uncommitted role edits in this worktree are not in effect; commit them, then restart";

/// What checking a seat's worktree against `main`'s composition found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchOverride {
    /// The worktree's `origin` is not the project's packs repository, so its
    /// branch has no say. Carries the origin it does have.
    OtherRepository { origin: String },
    /// The worktree is on the very commit `main`'s composition came from.
    SameCommit,
    /// The branch commit composes to the same bytes as `main`'s.
    Unchanged { sha: String },
    /// The branch commit holds no source for the role under `<path>`.
    RoleAbsent { sha: String },
    /// The branch commit composes differently: this is the seat's pack.
    Overridden {
        sha: String,
        staged: StagedComposedPack,
        /// The repository-relative `packRef.path` of the branch's source.
        pack_ref_path: String,
    },
}

/// The override decision plus the one fact reported alongside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchOverrideCheck {
    pub decision: BranchOverride,
    /// `true` when `<path>` in the worktree has uncommitted changes — never in
    /// effect, always disclosed.
    pub dirty: bool,
}

/// Check `worktree` against the composition `main_sha`/`main_digest` produced
/// for `role` from `source`, composing the branch commit's tree when the
/// worktree belongs to the same repository.
///
/// # Errors
/// A sentence when git cannot answer about the worktree (not a repository,
/// no `origin`, no `HEAD`), when the branch tree cannot be read, or when the
/// branch composition refuses — the caller discloses it as a warning and
/// keeps `main`'s composition, because a check that could not run is not an
/// override.
#[allow(clippy::too_many_arguments)] // Every argument is a fact the check compares; none is optional.
pub fn branch_role_override(
    packs_root: &Path,
    relay_http_base: &str,
    source: &ProjectPackSource,
    role: &str,
    worktree: &Path,
    main_sha: &str,
    main_digest: &str,
    auth: &GitAuthConfig,
    catalog: &TemplateCatalog,
) -> Result<BranchOverrideCheck, String> {
    let (owner, id) = parse_repo_coordinate(&source.repo)?;
    let path = validate_pack_path(&source.path)?;
    let expected = normalize_git_url(&packs_clone_url(relay_http_base, &owner, &id));
    let origin = run_git(&["remote", "get-url", "origin"], Some(worktree), auth)
        .map_err(|error| format!("the seat's worktree has no origin remote: {error}"))?;
    let origin = origin.trim().to_string();
    let dirty = !run_git(
        &["status", "--porcelain", "--", &path],
        Some(worktree),
        auth,
    )?
    .trim()
    .is_empty();
    if normalize_git_url(&origin) != expected {
        return Ok(BranchOverrideCheck {
            decision: BranchOverride::OtherRepository { origin },
            dirty,
        });
    }
    let sha = run_git(&["rev-parse", "HEAD"], Some(worktree), auth)?
        .trim()
        .to_string();
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("the seat's worktree HEAD is not a commit: {sha:?}"));
    }
    if sha == main_sha {
        return Ok(BranchOverrideCheck {
            decision: BranchOverride::SameCommit,
            dirty,
        });
    }
    let tree_dir = packs_root
        .join(BRANCH_TREES_DIR)
        .join(format!("{}-{sha}", pack_cache_dir_name(&owner, &id)));
    materialize_commit_tree(worktree, &sha, &path, &tree_dir, auth)?;
    compose_commit_tree(
        packs_root,
        source,
        &owner,
        &id,
        &sha,
        &path,
        role,
        &tree_dir,
        catalog,
        main_digest,
        dirty,
    )
}

/// Compose `role` from a materialized commit tree and compare it with
/// `main`'s digest.
#[allow(clippy::too_many_arguments)] // Every argument is a fact the comparison reads.
fn compose_commit_tree(
    packs_root: &Path,
    source: &ProjectPackSource,
    owner: &str,
    id: &str,
    sha: &str,
    path: &str,
    role: &str,
    tree_dir: &Path,
    catalog: &TemplateCatalog,
    main_digest: &str,
    dirty: bool,
) -> Result<BranchOverrideCheck, String> {
    let sha = sha.to_string();
    let Some(role_source) = locate_role_source(tree_dir, path, role) else {
        return Ok(BranchOverrideCheck {
            decision: BranchOverride::RoleAbsent { sha },
            dirty,
        });
    };
    let ref_path = pack_ref_path(&role_source, path);
    let staged = stage_composed_pack(
        packs_root,
        &format!("{}-{sha}", pack_cache_dir_name(owner, id)),
        &role_source,
        catalog,
        SourceProvenance {
            kind: BRANCH_OVERRIDE_KIND.to_string(),
            repo: Some(source.repo.clone()),
            sha: Some(sha.clone()),
            path: ref_path.clone(),
        },
    )?;
    if staged.digest == main_digest {
        return Ok(BranchOverrideCheck {
            decision: BranchOverride::Unchanged { sha },
            dirty,
        });
    }
    Ok(BranchOverrideCheck {
        decision: BranchOverride::Overridden {
            sha,
            staged,
            pack_ref_path: ref_path,
        },
        dirty,
    })
}

/// Write every file under `<sha>:<path>` into `<dest>/<path>/…`, byte for
/// byte, from the objects of the repository at `worktree` (any checkout or
/// worktree of it; the packs cache included).
///
/// Idempotent: a destination that already holds the commit's tree is left
/// alone (a commit's tree cannot change), so the second seat cut on the same
/// branch commit reads what the first one wrote.
///
/// # Errors
/// A sentence when the repository does not hold `sha` (`ls-tree` refuses),
/// when a listed path would escape `dest`, or on a filesystem failure.
pub(super) fn materialize_commit_tree(
    worktree: &Path,
    sha: &str,
    path: &str,
    dest: &Path,
    auth: &GitAuthConfig,
) -> Result<(), String> {
    let marker = dest.join(".materialized");
    if marker.is_file() {
        return Ok(());
    }
    let listing = run_git(
        &["ls-tree", "-r", "--name-only", "-z", sha, "--", path],
        Some(worktree),
        auth,
    )?;
    let files: Vec<&str> = listing
        .split('\0')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    for file in &files {
        // Every name came from git's own listing of this commit, but the path
        // is still confined: nothing outside `dest` may be written.
        if Path::new(file)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "refusing to materialize a traversing path from git: {file:?}"
            ));
        }
        let bytes = run_git_bytes(
            &["show", &format!("{sha}:{file}")],
            Some(worktree),
            auth,
            &[],
        )?;
        let target = dest.join(file);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        std::fs::write(&target, bytes)
            .map_err(|error| format!("write {}: {error}", target.display()))?;
    }
    std::fs::create_dir_all(dest).map_err(|error| format!("create {}: {error}", dest.display()))?;
    std::fs::write(&marker, format!("{sha}\n"))
        .map_err(|error| format!("write {}: {error}", marker.display()))?;
    Ok(())
}

/// Two spellings of one repository URL compare equal: trailing slashes and
/// a `.git` suffix are not identity.
fn normalize_git_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix(".git")
        .unwrap_or(trimmed)
        .to_ascii_lowercase()
}

/// The directory a branch commit's role tree is materialized under.
#[cfg(test)]
pub(super) fn branch_tree_dir(
    packs_root: &Path,
    owner: &str,
    id: &str,
    sha: &str,
) -> std::path::PathBuf {
    packs_root
        .join(BRANCH_TREES_DIR)
        .join(format!("{}-{sha}", pack_cache_dir_name(owner, id)))
}
