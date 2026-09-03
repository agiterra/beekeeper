//! Who may say where a project's packs live — the kind:30624 write gate.
//!
//! A pack source decides which prompt bytes every seat on a project runs. That
//! makes it the most privileged small record on this wire: a community member
//! who could publish one could re-point an entire team's agents at a
//! repository they control. So unlike the soft `project` back-references
//! elsewhere in ingest, this gate is **closed by default** — a 30624 is
//! admitted only from a key that already speaks for the project's code.
//!
//! # The rule
//!
//! The author must be one of:
//!
//! 1. the project coordinate's own pubkey — the creator, an implicit Owner
//!    everywhere else the roster is read;
//! 2. a roster **Owner** of that project (the same rows
//!    `get_project_role_by_coordinate` authorizes pushes against);
//! 3. a **founder** of a repository that belongs to the project — L18's
//!    [`RepositoryFounders`]: an announcement's signer, its NIP-34
//!    `maintainers`, and the project's roster Owners.
//!
//! (3) is the clause finding 33 bought: `agiterra-beekeeper` is signed by one
//! human and co-owned by two, and a rule keyed to the signer alone would let
//! Andy set the pack source and refuse Brian.
//!
//! # Bounds, disclosed
//!
//! (3) reads the newest [`PACK_SOURCE_MAX_REPOSITORIES`] repository
//! announcements in the community and keeps the ones whose `project`
//! back-reference names this project. A community holding more than that many
//! repositories could push an older announcement off the page, and the refusal
//! says how many were searched rather than reporting "not a founder" as though
//! the question had been fully asked.
//!
//! # Failure is closed
//!
//! A storage error refuses the write as an internal error. Narrowing the
//! founder set to the signer on a Postgres blip is exactly the shape of bug
//! that would hand one co-founder silent control of the team's packs.

use std::sync::Arc;

use buzz_core::channel::ProjectRole;
use buzz_core::kind::{repo_project_ref, KIND_GIT_REPO_ANNOUNCEMENT};
use buzz_core::project_pack_source::decode_project_pack_source;
use buzz_core::repository_founders::RepositoryFounders;
use buzz_db::EventQuery;

use crate::state::AppState;

/// How many repository announcements the founder clause reads.
pub(crate) const PACK_SOURCE_MAX_REPOSITORIES: usize = 500;

/// Why a kind:30624 was admitted — the clause that admitted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PackSourceAdmission {
    /// The author is the project coordinate's own pubkey.
    ProjectCreator,
    /// The author holds Owner on the project roster.
    ProjectOwner,
    /// The author founds one of the project's repositories.
    RepositoryFounder {
        /// The repository coordinate whose founder set admitted them.
        repo_coordinate: String,
    },
}

/// Why a kind:30624 was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackSourceRefusal {
    /// The project coordinate the record named.
    pub project: String,
    /// How many of the project's repositories were searched.
    pub repositories_searched: usize,
    /// Whether the project roster could be read at all.
    pub roster_read: bool,
}

impl PackSourceRefusal {
    /// The sentence the author sees, which says what was checked.
    pub(crate) fn sentence(&self) -> String {
        let roster = if self.roster_read {
            "the project roster was read"
        } else {
            "no project by that coordinate is stored here, so no roster was read"
        };
        format!(
            "restricted: a pack source for {} may only be published by an Owner of that project \
             or a founder of one of its repositories ({}; {} repositor(y|ies) of this project \
             searched, newest {} announcements)",
            self.project, roster, self.repositories_searched, PACK_SOURCE_MAX_REPOSITORIES
        )
    }
}

/// Decide admission from resolved inputs. Pure, so the rule is testable
/// without Postgres and cannot be re-stated differently by a second caller.
///
/// `roster` carries every roster row including the creator's implicit Owner;
/// `repositories` are the project's repository announcements, already filtered
/// to those whose `project` back-reference names `project_coordinate`.
pub(crate) fn decide_pack_source_admission(
    author_hex: &str,
    project_coordinate: &str,
    project_creator_hex: &str,
    roster: &[(String, ProjectRole)],
    repositories: &[nostr::Event],
    roster_read: bool,
) -> Result<PackSourceAdmission, PackSourceRefusal> {
    let author = author_hex.trim().to_ascii_lowercase();

    if author == project_creator_hex.trim().to_ascii_lowercase() {
        return Ok(PackSourceAdmission::ProjectCreator);
    }

    if roster.iter().any(|(pubkey, role)| {
        matches!(role, ProjectRole::Owner) && pubkey.trim().eq_ignore_ascii_case(&author)
    }) {
        return Ok(PackSourceAdmission::ProjectOwner);
    }

    for announcement in repositories {
        let founders =
            RepositoryFounders::from_announcement(announcement).with_roster_roles(roster.to_vec());
        if founders.contains(&author) {
            let coordinate =
                repository_coordinate(announcement).unwrap_or_else(|| announcement.id.to_hex());
            return Ok(PackSourceAdmission::RepositoryFounder {
                repo_coordinate: coordinate,
            });
        }
    }

    Err(PackSourceRefusal {
        project: project_coordinate.to_string(),
        repositories_searched: repositories.len(),
        roster_read,
    })
}

/// The production gate: validate the record's shape, then resolve the founder
/// set from storage and decide.
///
/// # Errors
/// `Err(Some(refusal))` when the author may not write this record;
/// `Err(None)` when storage could not answer, which the caller must map to an
/// internal error rather than a refusal.
pub(crate) async fn pack_source_write_admitted(
    state: &Arc<AppState>,
    community: buzz_core::CommunityId,
    event: &nostr::Event,
) -> Result<Result<PackSourceAdmission, PackSourceRefusal>, ()> {
    let record = match decode_project_pack_source(event) {
        Ok(record) => record,
        // Shape is validated before this function is reached; a decode failure
        // here would be a caller bug, and admitting on it would be worse than
        // failing closed.
        Err(_) => return Err(()),
    };
    let coordinate = record.project().to_string();
    let creator = project_coordinate_owner(&coordinate).unwrap_or_default();

    let roster = match state.db.get_project_roster(community, &coordinate).await {
        Ok(roster) => roster,
        Err(error) => {
            tracing::error!(error = %error, "pack source: project roster lookup failed");
            return Err(());
        }
    };
    let roster_read = roster.is_some();
    let mut rows: Vec<(String, ProjectRole)> = Vec::new();
    if let Some(roster) = roster {
        // The creator holds no `project_acl_members` row and is an implicit
        // Owner everywhere else this roster is read; omitting them here would
        // drop the one owner the coordinate itself names.
        rows.push((hex::encode(&roster.owner), ProjectRole::Owner));
        rows.extend(
            roster
                .members
                .into_iter()
                .map(|(pubkey, role)| (hex::encode(pubkey), role)),
        );
    }

    let announcements = match project_repository_announcements(state, community, &coordinate).await
    {
        Ok(announcements) => announcements,
        Err(()) => return Err(()),
    };

    Ok(decide_pack_source_admission(
        &event.pubkey.to_hex(),
        &coordinate,
        &creator,
        &rows,
        &announcements,
        roster_read,
    ))
}

/// The repository announcements whose `project` back-reference names
/// `coordinate`, newest first, bounded by [`PACK_SOURCE_MAX_REPOSITORIES`].
async fn project_repository_announcements(
    state: &Arc<AppState>,
    community: buzz_core::CommunityId,
    coordinate: &str,
) -> Result<Vec<nostr::Event>, ()> {
    let query = EventQuery {
        kinds: Some(vec![KIND_GIT_REPO_ANNOUNCEMENT as i32]),
        limit: Some(PACK_SOURCE_MAX_REPOSITORIES as i64),
        ..EventQuery::for_community(community)
    };
    let stored = match state.db.query_events(&query).await {
        Ok(stored) => stored,
        Err(error) => {
            tracing::error!(error = %error, "pack source: repository announcement query failed");
            return Err(());
        }
    };
    Ok(stored
        .into_iter()
        .map(|stored| stored.event)
        .filter(|event| repo_project_ref(event).as_deref() == Some(coordinate))
        .collect())
}

/// The `30617:<owner>:<d>` coordinate an announcement addresses.
fn repository_coordinate(event: &nostr::Event) -> Option<String> {
    let d = nostr::SingleLetterTag::lowercase(nostr::Alphabet::D);
    let dtag = event
        .tags
        .filter(nostr::TagKind::SingleLetter(d))
        .find_map(|tag| tag.content())?;
    Some(format!(
        "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{dtag}",
        event.pubkey.to_hex()
    ))
}

/// The owner pubkey a `30621:<owner>:<d>` coordinate names.
fn project_coordinate_owner(coordinate: &str) -> Option<String> {
    let mut parts = coordinate.splitn(3, ':');
    let _kind = parts.next()?;
    let owner = parts.next()?;
    let _dtag = parts.next()?;
    Some(owner.to_ascii_lowercase())
}

#[cfg(test)]
#[path = "pack_source_tests.rs"]
mod tests;
