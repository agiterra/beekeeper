//! Moving a project created before the pivot into its own agents
//! repository (spec § 4.11, ledger 233).
//!
//! A project made before 2026-09-18 points its kind:30624 at a packs
//! repository laid out one directory per role. [`super::agents_repo`]
//! refuses such a project by default, because re-pointing it re-points
//! every seat on it; this module is what that refusal turns into when a
//! caller asks for the move and names the exact source event it read.
//!
//! Three pieces: the code repository a pre-pivot project already names
//! ([`adopted_code_repo_id`]), the conversion of its roles into the flat
//! layout ([`convert_agents_checkout`], over `buzz_persona::migrate`), and
//! the conditional kind:30624 that re-points it
//! ([`build_migrated_pack_source`]) — conditional on the event the caller
//! decided against, so a source someone moved meanwhile is the relay's
//! conflict to refuse rather than a race this host wins. The repository
//! being migrated off is only ever read.

use std::path::Path;

use nostr::{Event, EventBuilder, Keys, Kind, Tag};

use crate::commands::project_git_exec::GitAuthConfig;
use crate::managed_agents::packs_cache;
use buzz_core_pkg::project_pack_source::{
    build_conditional_project_pack_source, PackPin, PACK_PATH_ROOT,
};

use super::agents_repo::{commit_seed, KIND_PROJECT_PACK_SOURCE, MIGRATE_COMMIT_MESSAGE};
use crate::managed_agents::packs_repo::SEED_BRANCH;

/// The role source a migration is moving the project off: enough to sync
/// that repository and find its roles, plus the event the re-point is
/// conditional on.
pub(crate) struct LegacySource {
    pub repo: String,
    pub git_ref: Option<String>,
    pub sha: Option<String>,
    pub path: String,
    pub event_id: String,
    pub convert: bool,
}

/// What a screen or the CLI sends to ask for a migration.
///
/// Separate from [`MigrateFromSource`] because this crosses the IPC
/// boundary: it is unvalidated until [`MigrateFromSource::try_from`] has
/// looked at it.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateRequest {
    /// The kind:30624 the caller read and decided against.
    pub expected_source_id: String,
    /// Convert the legacy roles rather than seeding fresh ones. Defaults to
    /// `true`: a migration that silently replaced a project's roles with
    /// this build's templates would be the worst possible default.
    #[serde(default = "default_convert")]
    pub convert: bool,
}

pub(crate) const fn default_convert() -> bool {
    true
}

impl TryFrom<MigrateRequest> for MigrateFromSource {
    type Error = String;

    fn try_from(request: MigrateRequest) -> Result<Self, Self::Error> {
        let expected_source_id = request.expected_source_id.trim().to_owned();
        if expected_source_id.len() != 64
            || !expected_source_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "the source event to replace must be 64 lowercase hex characters, got {:?}",
                request.expected_source_id
            ));
        }
        Ok(Self {
            expected_source_id,
            convert: request.convert,
        })
    }
}

/// A caller's deliberate instruction to re-point a project's role source at
/// its own agents repository (spec § 4.11, legacy projects).
///
/// The expected event id is the one the caller *observed*: a migration is a
/// decision about a specific record, so a source that changed underneath is
/// a conflict the relay refuses rather than a race this host wins.
#[derive(Clone, Debug)]
pub(crate) struct MigrateFromSource {
    /// The kind:30624 event this migration replaces, 64 lowercase hex.
    pub expected_source_id: String,
    /// Convert the legacy source's roles into the new repository instead of
    /// seeding it from the shipped templates. `false` starts the project's
    /// team over from this build's templates, which is a different decision
    /// and is never the default.
    pub convert: bool,
}

/// The code repository the project's own head names, when it names one
/// under the viewer's key that is not the agents repository.
///
/// A project created before the pivot has a code repository whose id is not
/// its slug — Beekeeper's own project `bee-keeper` carries
/// `agiterra-beekeeper`, Tank Loop carries `tankloop`. Deriving the id from
/// the slug there would announce and seed a second, empty repository and
/// then refuse the folder every seat is cut from as "not a checkout of
/// `<slug>`" (ledger 175). `None` when the head names none, names only the
/// agents repository, or names one under another key — this host can only
/// push to its own.
pub(crate) fn adopted_code_repo_id(
    head: Option<&Event>,
    viewer: &str,
    agents_repo_id: &str,
) -> Option<String> {
    let prefix = format!("30617:{viewer}:");
    head?.tags.iter().find_map(|tag| {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("a") {
            return None;
        }
        let id = parts.get(1)?.strip_prefix(&prefix)?;
        (!id.is_empty() && id != agents_repo_id).then(|| id.to_owned())
    })
}

/// Sync the repository a legacy project points at, convert its roles into
/// the flat layout in a fresh `checkout`, and commit that once.
///
/// Returns `(commit, roles, notes)`. The legacy repository is only read:
/// the conversion writes into this host's cache directory for the *new*
/// repository, and the old one keeps every byte it had.
pub(crate) fn convert_agents_checkout(
    checkout: &Path,
    legacy: &packs_cache::ProjectPackSource,
    relay_http: &str,
    packs_root: &Path,
    slug: &str,
    auth: &GitAuthConfig,
) -> Result<(String, Vec<String>, Vec<String>), String> {
    let (owner, id) = packs_cache::parse_repo_coordinate(&legacy.repo)?;
    let legacy_checkout = packs_cache::packs_checkout_dir(packs_root, &owner, &id);
    let clone_url = packs_cache::packs_clone_url(relay_http, &owner, &id);
    crate::commands::project_git_exec::validate_clone_url(&clone_url)?;
    packs_cache::sync_packs_checkout(&legacy_checkout, &clone_url, legacy, auth).map_err(
        |error| {
            format!(
                "could not read the roles this project points at ({}): {error}",
                legacy.repo
            )
        },
    )?;
    let roles_dir = if buzz_core_pkg::project_pack_source::is_root_pack_path(&legacy.path) {
        legacy_checkout.clone()
    } else {
        legacy_checkout.join(&legacy.path)
    };

    if checkout.exists() {
        std::fs::remove_dir_all(checkout)
            .map_err(|error| format!("clear {}: {error}", checkout.display()))?;
    }
    std::fs::create_dir_all(checkout)
        .map_err(|error| format!("create {}: {error}", checkout.display()))?;
    let report = buzz_persona_pkg::migrate::convert_pack_tree(&roles_dir, checkout, slug)
        .map_err(|error| error.to_string())?;
    let notes = report
        .roles
        .iter()
        .filter(|role| !role.dropped_keys.is_empty())
        .map(|role| {
            format!(
                "{}: the frontmatter's {} did not survive the layout change; every skill was \
                 copied and still reaches the seat",
                role.role,
                role.dropped_keys.join(" and ")
            )
        })
        .collect();
    let commit = commit_seed(checkout, MIGRATE_COMMIT_MESSAGE, auth)?;
    Ok((commit, report.role_slugs(), notes))
}

/// The kind:30624 a migration publishes: the same record
/// [`build_agents_pack_source`] builds, conditional on the source event the
/// caller decided against, so the relay refuses it if that moved.
pub(crate) fn build_migrated_pack_source(
    keys: &Keys,
    project: &str,
    repo: &str,
    expected_source_id: &str,
) -> Result<nostr::Event, String> {
    let draft = build_conditional_project_pack_source(
        project,
        repo,
        &PackPin::Ref(format!("refs/heads/{SEED_BRANCH}")),
        Some(PACK_PATH_ROOT),
        None,
        Some(expected_source_id),
    )?;
    let mut tags = Vec::with_capacity(draft.tags.len());
    for tag in draft.tags {
        tags.push(Tag::parse(tag).map_err(|error| format!("invalid pack source tag: {error}"))?);
    }
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE), draft.content)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| format!("sign the pack source: {error}"))
}
