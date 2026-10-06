//! Who may set a repository's rules — the kind:30625 write gate.
//!
//! A rule record carries `buzz-protect` rows for a repository at an address
//! keyed to its **author**, so the shape confers no authority at all and this
//! gate is the whole of it. Like the pack-source gate next door it is
//! **closed by default**: a record is admitted only from a founder of the
//! repository its `d` tag names ([`RepositoryFounders`] — the announcement's
//! signer, its NIP-34 `maintainers`, and the roster Owners of the project it
//! back-references).
//!
//! # Why this is not "the signer, as before"
//!
//! Finding 33's residual R2. Rules lived on the announcement, addressable by
//! `(kind, author, d)`, so a co-founder's `bee repos protect set` published a
//! second repository rather than changing the rules of the one they
//! co-founded. Every other authority question in batch 3 — which missions may
//! rule, who may land, who names a project's packs — became plural; this one
//! stayed singular and was merely disclosed. Admitting the founder set here
//! is what closes it.
//!
//! # Failure is closed
//!
//! An unannounced repository has no founders and therefore nobody who may
//! write rules for it; the refusal says the announcement was missing rather
//! than reporting "not a founder", which would send the author looking for
//! the wrong problem. A storage error refuses as an internal error: narrowing
//! the founder set on a Postgres blip is exactly the bug that would hand one
//! co-founder silent control of the repository's rules.

use std::sync::Arc;

use beekeeper_core::channel::ProjectRole;
use beekeeper_core::kind::{repo_project_ref, KIND_GIT_REPO_ANNOUNCEMENT};
use beekeeper_core::repository_founders::RepositoryFounders;
use beekeeper_core::repository_protection::decode_repository_protection;
use beekeeper_db::EventQuery;

use crate::state::AppState;

/// Why a kind:30625 was admitted — the clause that admitted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepoProtectionAdmission {
    /// The author signed the repository's own announcement. The pre-30625
    /// behaviour, reached through the new kind.
    AnnouncementSigner,
    /// The author founds the repository some other way — a NIP-34
    /// `maintainers` entry, or an Owner on the roster of the project the
    /// announcement back-references.
    RepositoryFounder,
}

/// Why a kind:30625 was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoProtectionRefusal {
    /// The repository coordinate the record's `d` tag named.
    pub coordinate: String,
    /// Whether that repository's announcement was found at all.
    pub announcement_found: bool,
    /// Whether the project roster half of the founder set could be read.
    pub roster_read: bool,
    /// How many founders were resolved, when the announcement was found.
    pub founders: usize,
}

impl RepoProtectionRefusal {
    /// The sentence the author sees, which says what was checked.
    pub(crate) fn sentence(&self) -> String {
        if !self.announcement_found {
            return format!(
                "restricted: no repository is announced at {}, so it has no founders and no key \
                 may write rules for it",
                self.coordinate
            );
        }
        let roster = if self.roster_read {
            "the project roster was read"
        } else {
            "this repository names no project, so no roster was read"
        };
        format!(
            "restricted: a protection rule for {} may only be published by a founder of that \
             repository ({}; {} founder(s) resolved)",
            self.coordinate, roster, self.founders
        )
    }
}

/// Decide admission from resolved inputs. Pure, so the rule is testable
/// without Postgres and cannot be re-stated differently by a second caller.
///
/// `announcement` is the repository's own kind:30617, `None` when none is
/// stored. `roster` carries every roster row of the project it
/// back-references, including the creator's implicit Owner.
pub(crate) fn decide_repo_protection_admission(
    author_hex: &str,
    repo_owner_hex: &str,
    repo_id: &str,
    announcement: Option<&nostr::Event>,
    roster: &[(String, ProjectRole)],
    roster_read: bool,
) -> Result<RepoProtectionAdmission, RepoProtectionRefusal> {
    let author = author_hex.trim().to_ascii_lowercase();
    let coordinate = format!(
        "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{repo_id}",
        repo_owner_hex.trim().to_ascii_lowercase()
    );

    let Some(announcement) = announcement else {
        return Err(RepoProtectionRefusal {
            coordinate,
            announcement_found: false,
            roster_read,
            founders: 0,
        });
    };

    let founders =
        RepositoryFounders::from_announcement(announcement).with_roster_roles(roster.to_vec());
    if founders.signer().eq_ignore_ascii_case(&author) {
        return Ok(RepoProtectionAdmission::AnnouncementSigner);
    }
    if founders.contains(&author) {
        return Ok(RepoProtectionAdmission::RepositoryFounder);
    }
    Err(RepoProtectionRefusal {
        coordinate,
        announcement_found: true,
        roster_read,
        founders: founders.len(),
    })
}

/// The production gate: validate the record's shape, then resolve the founder
/// set from storage and decide.
///
/// # Errors
/// `Err(Some(refusal))` when the author may not write this record;
/// `Err(None)` when storage could not answer, which the caller must map to an
/// internal error rather than a refusal.
pub(crate) async fn repo_protection_write_admitted(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    event: &nostr::Event,
) -> Result<Result<RepoProtectionAdmission, RepoProtectionRefusal>, ()> {
    let record = match decode_repository_protection(event) {
        Ok(record) => record,
        // Shape is validated before this function is reached; a decode failure
        // here would be a caller bug, and admitting on it would be worse than
        // failing closed.
        Err(_) => return Err(()),
    };

    let announcement = match repository_announcement(
        state,
        community,
        record.repo_owner(),
        record.repo_id(),
    )
    .await
    {
        Ok(announcement) => announcement,
        Err(()) => return Err(()),
    };

    let Some(announcement) = announcement else {
        return Ok(decide_repo_protection_admission(
            &event.pubkey.to_hex(),
            record.repo_owner(),
            record.repo_id(),
            None,
            &[],
            false,
        ));
    };

    let (roster, roster_read) = match project_roster_rows(state, community, &announcement).await {
        Ok(rows) => rows,
        Err(()) => return Err(()),
    };

    Ok(decide_repo_protection_admission(
        &event.pubkey.to_hex(),
        record.repo_owner(),
        record.repo_id(),
        Some(&announcement),
        &roster,
        roster_read,
    ))
}

/// The repository's own kind:30617, by `(community, kind, pubkey, d)` — the
/// same spoof-proof lookup the push policy uses, never a scan.
pub(crate) async fn repository_announcement(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    repo_owner_hex: &str,
    repo_id: &str,
) -> Result<Option<nostr::Event>, ()> {
    let owner_bytes = match hex::decode(repo_owner_hex) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        // An address whose owner is not a pubkey names no repository; the
        // decoder already refuses these, so this is defence in depth.
        _ => return Ok(None),
    };
    let query = EventQuery {
        kinds: Some(vec![KIND_GIT_REPO_ANNOUNCEMENT as i32]),
        pubkey: Some(owner_bytes),
        d_tag: Some(repo_id.to_string()),
        global_only: true,
        limit: Some(1),
        ..EventQuery::for_community(community)
    };
    match state.db.query_events(&query).await {
        Ok(mut events) => Ok(events.pop().map(|stored| stored.event)),
        Err(error) => {
            tracing::error!(error = %error, "repo protection: announcement lookup failed");
            Err(())
        }
    }
}

/// The roster of the project an announcement back-references, and whether it
/// was read.
///
/// A repository with no `project` tag has no roster to read, so the set is
/// read-and-empty: there is nothing missing from it. Only a storage failure
/// is an error.
pub(crate) async fn project_roster_rows(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    announcement: &nostr::Event,
) -> Result<(Vec<(String, ProjectRole)>, bool), ()> {
    let Some(coordinate) = repo_project_ref(announcement) else {
        return Ok((Vec::new(), false));
    };
    let roster = match state.db.get_project_roster(community, &coordinate).await {
        Ok(roster) => roster,
        Err(error) => {
            tracing::error!(error = %error, "repo protection: project roster lookup failed");
            return Err(());
        }
    };
    let Some(roster) = roster else {
        return Ok((Vec::new(), true));
    };
    // The creator holds no `project_acl_members` row and is an implicit Owner
    // everywhere else this roster is read; omitting them here would drop the
    // one owner the coordinate itself names.
    let mut rows = vec![(hex::encode(&roster.owner), ProjectRole::Owner)];
    rows.extend(
        roster
            .members
            .into_iter()
            .map(|(pubkey, role)| (hex::encode(pubkey), role)),
    );
    Ok((rows, true))
}

#[cfg(test)]
#[path = "repo_protection_tests.rs"]
mod tests;
