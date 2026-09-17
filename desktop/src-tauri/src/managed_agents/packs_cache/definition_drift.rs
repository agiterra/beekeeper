//! Spec § 4.9, second half: has a running seat's role definition drifted from
//! what this host would stage for it now?
//!
//! A running execution keeps the instructions it was staged with (the vision:
//! "an active execution does not silently change instructions"). This module
//! answers, for one seat, whether the *current* resolution of its role — the
//! project's pinned `main`, or the seat's own branch when that overrides —
//! composes to different bytes than the commit the seat's `packRef` names.
//! The card built on it says **Definition changed** with the cause, and
//! offers a restart; it never restarts anything itself.
//!
//! Three honest outcomes, never a guess:
//!
//! - `Current`: the seat runs what would be staged now (same digest).
//! - `Changed`: the digests differ; `cause` says what moved.
//! - `Unknown`: this host cannot compose one side — the packs cache does not
//!   hold the seat's commit, the source cannot be synced, the branch check
//!   cannot run — and `reason` says which. Unknown is not "current".

use std::path::Path;

use serde::Serialize;

use crate::commands::project_git_exec::GitAuthConfig;

use super::branch_override::materialize_commit_tree;
use super::{
    branch_role_override, locate_role_source, pack_cache_dir_name, pack_ref_path,
    packs_checkout_dir, parse_repo_coordinate, stage_composed_pack, stage_project_role_pack,
    validate_pack_path, BranchOverride, ProjectPackSource, SourceProvenance, TemplateCatalog,
    UNCOMMITTED_ROLE_EDITS,
};

/// The directory under the packs cache holding materialized trees of commits
/// a seat was staged from: `<packs root>/seat/<owner8>-<id>-<sha>/`.
pub const SEAT_TREES_DIR: &str = "seat";

/// What the comparison found. The wire vocabulary of the Agents tab card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionDriftState {
    /// The seat runs the bytes this host would stage now.
    Current,
    /// The current resolution composes differently; a restart would change
    /// the seat's instructions.
    Changed,
    /// One side could not be composed here; `reason` says which.
    Unknown,
}

/// The answer for one seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionDrift {
    pub state: DefinitionDriftState,
    /// The commit the seat's instructions came from (its `packRef.sha`).
    pub seat_sha: String,
    /// The commit the current resolution comes from: `main`'s pin, or the
    /// seat's branch when that overrides. `null` when unknown.
    pub current_sha: Option<String>,
    /// Where the current resolution comes from: `repository` or
    /// `branch-override`. `null` when unknown.
    pub current_source_kind: Option<String>,
    /// The current composition's digest, `null` when unknown.
    pub current_digest: Option<String>,
    /// The seat's composition's digest, `null` when unknown.
    pub seat_digest: Option<String>,
    /// One sentence naming what moved, for the card. Empty when `Current`.
    pub cause: String,
    /// Why the answer is `Unknown`, verbatim. `null` otherwise.
    pub reason: Option<String>,
    /// Facts worth saying beside the state: uncommitted role edits in the
    /// seat's worktree, a warning from either composition.
    pub warnings: Vec<String>,
}

impl DefinitionDrift {
    fn unknown(seat_sha: &str, reason: String, warnings: Vec<String>) -> Self {
        Self {
            state: DefinitionDriftState::Unknown,
            seat_sha: seat_sha.to_string(),
            current_sha: None,
            current_source_kind: None,
            current_digest: None,
            seat_digest: None,
            cause: String::new(),
            reason: Some(reason),
            warnings,
        }
    }
}

/// Compare a seat's staged definition (`seat_sha`, from its `packRef`) with
/// what this host would stage for `role` now.
///
/// `seat_worktree` is the seat's own tree when the host knows it; with it the
/// § 4.9 branch override is part of "now". Without it, "now" is `main`'s pin
/// — which is what a provider-restart restage stages too (ledger 145).
///
/// Never errors: every failure is an `Unknown` with its reason, because the
/// card must render something truthful for a seat whose history this
/// computer cannot see.
#[allow(clippy::too_many_arguments)] // Every argument is a fact the comparison reads.
pub fn definition_drift(
    packs_root: &Path,
    relay_http_base: &str,
    source: &ProjectPackSource,
    role: &str,
    seat_sha: &str,
    seat_worktree: Option<&Path>,
    auth: &GitAuthConfig,
    catalog: &TemplateCatalog,
) -> DefinitionDrift {
    let seat_sha = seat_sha.trim().to_ascii_lowercase();
    if seat_sha.len() != 40 || !seat_sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return DefinitionDrift::unknown(
            &seat_sha,
            format!("the seat's packRef sha is not a commit: {seat_sha:?}"),
            Vec::new(),
        );
    }
    // 1. What would be staged now from main's pin.
    let current =
        match stage_project_role_pack(packs_root, relay_http_base, source, role, auth, catalog) {
            Ok(staged) => staged,
            Err(reason) => {
                return DefinitionDrift::unknown(
                    &seat_sha,
                    format!("the project's current pack source could not be staged here: {reason}"),
                    Vec::new(),
                )
            }
        };
    definition_drift_against(
        packs_root,
        relay_http_base,
        source,
        role,
        &seat_sha,
        seat_worktree,
        &current,
        auth,
        catalog,
    )
}

/// The comparison half of [`definition_drift`], given `main`'s current
/// composition already staged. Split out so a test can supply a composition
/// from a scratch repository, which the staging half's relay-URL check
/// would refuse.
#[allow(clippy::too_many_arguments)] // Every argument is a fact the comparison reads.
pub fn definition_drift_against(
    packs_root: &Path,
    relay_http_base: &str,
    source: &ProjectPackSource,
    role: &str,
    seat_sha: &str,
    seat_worktree: Option<&Path>,
    current: &super::StagedProjectPack,
    auth: &GitAuthConfig,
    catalog: &TemplateCatalog,
) -> DefinitionDrift {
    let seat_sha = seat_sha.to_string();
    let mut warnings = current.warnings.clone();
    let mut current_sha = current.pack_ref.sha.clone();
    let mut current_digest = current.digest.clone();
    let mut current_kind = "repository".to_string();

    // 2. The seat's own branch, when the host knows its tree.
    if let Some(worktree) = seat_worktree {
        match branch_role_override(
            packs_root,
            relay_http_base,
            source,
            role,
            worktree,
            &current.pack_ref.sha,
            &current.digest,
            auth,
            catalog,
        ) {
            Ok(check) => {
                if check.dirty {
                    warnings.push(UNCOMMITTED_ROLE_EDITS.to_string());
                }
                if let BranchOverride::Overridden { sha, staged, .. } = check.decision {
                    current_sha = sha;
                    current_digest = staged.digest;
                    current_kind = super::BRANCH_OVERRIDE_KIND.to_string();
                    warnings.extend(staged.warnings);
                }
            }
            Err(reason) => warnings.push(format!(
                "could not check the seat's worktree for a role override; comparing against main: {reason}"
            )),
        }
    }

    // 3. The seat's own composition: the same commit means the same bytes.
    if seat_sha == current_sha {
        return DefinitionDrift {
            state: DefinitionDriftState::Current,
            seat_sha,
            current_sha: Some(current_sha),
            current_source_kind: Some(current_kind),
            current_digest: Some(current_digest.clone()),
            seat_digest: Some(current_digest),
            cause: String::new(),
            reason: None,
            warnings,
        };
    }
    let seat_digest = match compose_at_commit(packs_root, source, role, &seat_sha, auth, catalog) {
        Ok((digest, more)) => {
            warnings.extend(more);
            digest
        }
        Err(reason) => {
            return DefinitionDrift::unknown(
                &seat_sha,
                format!("the seat's own commit could not be composed here: {reason}"),
                warnings,
            )
        }
    };
    let short = |sha: &str| sha.chars().take(8).collect::<String>();
    if seat_digest == current_digest {
        return DefinitionDrift {
            state: DefinitionDriftState::Current,
            seat_sha: seat_sha.clone(),
            current_sha: Some(current_sha.clone()),
            current_source_kind: Some(current_kind),
            current_digest: Some(current_digest),
            seat_digest: Some(seat_digest),
            // The source moved but this role's bytes did not: say so, because
            // "current" over a moved commit is otherwise surprising.
            cause: format!(
                "the source moved {} → {}, and this role's definition is unchanged",
                short(&seat_sha),
                short(&current_sha)
            ),
            reason: None,
            warnings,
        };
    }
    let cause = if current_kind == super::BRANCH_OVERRIDE_KIND {
        format!(
            "this seat's branch changed the {role} definition ({} → {})",
            short(&seat_sha),
            short(&current_sha)
        )
    } else {
        format!(
            "main moved {} → {} and changed the {role} definition",
            short(&seat_sha),
            short(&current_sha)
        )
    };
    DefinitionDrift {
        state: DefinitionDriftState::Changed,
        seat_sha,
        current_sha: Some(current_sha),
        current_source_kind: Some(current_kind),
        current_digest: Some(current_digest),
        seat_digest: Some(seat_digest),
        cause,
        reason: None,
        warnings,
    }
}

/// Compose `role` at `sha` from the packs cache's own objects, staged under
/// the same digest-keyed layout every seat uses, and answer its digest.
fn compose_at_commit(
    packs_root: &Path,
    source: &ProjectPackSource,
    role: &str,
    sha: &str,
    auth: &GitAuthConfig,
    catalog: &TemplateCatalog,
) -> Result<(String, Vec<String>), String> {
    let (owner, id) = parse_repo_coordinate(&source.repo)?;
    let path = validate_pack_path(&source.path)?;
    let checkout = packs_checkout_dir(packs_root, &owner, &id);
    if !checkout.join(".git").is_dir() {
        return Err("this computer has no packs cache for the project's repository".to_string());
    }
    let tree_dir = packs_root
        .join(SEAT_TREES_DIR)
        .join(format!("{}-{sha}", pack_cache_dir_name(&owner, &id)));
    materialize_commit_tree(&checkout, sha, &path, &tree_dir, auth).map_err(|error| {
        format!("this computer's packs cache does not hold commit {sha}: {error}")
    })?;
    let role_source = locate_role_source(&tree_dir, &path, role).ok_or_else(|| {
        format!("commit {sha} holds no {path}/{role} pack and no {path}/roles/{role}.md")
    })?;
    let ref_path = pack_ref_path(&role_source, &path);
    let staged = stage_composed_pack(
        packs_root,
        &format!("{}-{sha}", pack_cache_dir_name(&owner, &id)),
        &role_source,
        catalog,
        SourceProvenance {
            kind: "repository".to_string(),
            repo: Some(source.repo.clone()),
            sha: Some(sha.to_string()),
            path: ref_path,
        },
    )?;
    Ok((staged.digest, staged.warnings))
}
