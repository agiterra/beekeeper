//! The rules that govern a repository, read from every founder's record as
//! well as the announcement's own rows.
//!
//! [`beekeeper_core::repository_protection`] holds the layering itself and has no
//! I/O. This module supplies it with layers: the announcement, plus the
//! newest kind:30625 per **current** founder of the repository.
//!
//! # Why the read filters by founder too
//!
//! `handlers::repo_protection` admits a record only from a founder, so every
//! stored record was written by one. But the founder set is not frozen — a
//! `maintainers` entry can be dropped, a roster Owner demoted — and a record
//! written by yesterday's founder is still on the wire. Trusting the write
//! gate to have been the only door would leave a removed maintainer holding a
//! rule nobody can see them holding. The read resolves the founder set and
//! keeps only records whose author is in it *now*.
//!
//! # Cost
//!
//! One indexed query per push: kind 30625 by `(community, kind, d_tag)`,
//! bounded by [`PROTECTION_MAX_RECORDS`]. When it comes back empty — every
//! repository that has never had a record, which is all of them until someone
//! writes one — the announcement is the only layer and **nothing else is
//! read**: no roster lookup, no founder resolution. A repository that does
//! have records pays one further roster query.
//!
//! That one query is the honest price of the rule. It is not conditional on
//! `require-verdict`, because the rules themselves are what decide whether
//! anything is gated, and reading them after deciding would be circular.
//!
//! # Failure is closed
//!
//! A storage error refuses the push. A rule record that could not be read is
//! not evidence that the repository is ungoverned — that is the direction
//! that admits a push the operator meant to gate.

use std::sync::Arc;

use beekeeper_core::git_perms::{ProtectionRule, RuleParseError};
use beekeeper_core::kind::KIND_GIT_REPO_PROTECTION;
use beekeeper_core::repository_founders::RepositoryFounders;
use beekeeper_core::repository_protection::{
    decode_repository_protection, repository_protection_d_tag, resolve_protection_layers,
    ProtectionLayer, ResolvedProtection,
};
use beekeeper_db::EventQuery;

use crate::state::AppState;

/// How many rule records one repository's resolution reads.
///
/// A record is addressable per `(kind, author, d)`, so this is a bound on
/// **distinct founders holding rules**, not on writes: a founder republishing
/// theirs replaces it. Sixty-four is far above any real founder set and keeps
/// a hostile flood of stale-founder records off the page.
pub const PROTECTION_MAX_RECORDS: usize = 64;

/// The rules that govern `announcement`'s repository, and the founder set
/// that was resolved to get them (`None` when no record existed, so no
/// founder resolution was needed).
pub struct RepositoryProtection {
    /// The resolved rules, with provenance.
    pub resolved: ResolvedProtection,
    /// The founder set, resolved only when a record forced it. The push gate
    /// reuses it rather than resolving twice.
    pub founders: Option<RepositoryFounders>,
}

impl RepositoryProtection {
    /// The flat rule list every existing caller expects.
    pub fn rules(&self) -> &[ProtectionRule] {
        self.resolved.rules()
    }
}

/// Why the rules could not be resolved.
#[derive(Debug)]
pub enum ProtectionReadError {
    /// A `buzz-protect` tag on the announcement is structurally malformed —
    /// the long-standing fail-closed case, unchanged.
    MalformedAnnouncement(RuleParseError),
    /// Storage could not answer. Fail closed.
    Unavailable,
}

/// Resolve a repository's rules from its announcement and every founder's
/// rule record.
///
/// # Errors
/// [`ProtectionReadError::MalformedAnnouncement`] when the announcement's own
/// tags do not parse (the pre-existing denial), [`ProtectionReadError::Unavailable`]
/// when storage failed.
pub async fn resolve_repository_protection(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    announcement: &nostr::Event,
    repo_id: &str,
) -> Result<RepositoryProtection, ProtectionReadError> {
    let tags: Vec<Vec<String>> = announcement
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let announcement_layer = ProtectionLayer::from_announcement_tags(
        announcement.created_at.as_secs(),
        announcement.id.to_hex(),
        &tags,
    )
    .map_err(ProtectionReadError::MalformedAnnouncement)?;

    let owner_hex = announcement.pubkey.to_hex();
    let query = EventQuery {
        kinds: Some(vec![KIND_GIT_REPO_PROTECTION as i32]),
        d_tag: Some(repository_protection_d_tag(&owner_hex, repo_id)),
        global_only: true,
        limit: Some(PROTECTION_MAX_RECORDS as i64),
        ..EventQuery::for_community(community)
    };
    let stored = match state.db.query_events(&query).await {
        Ok(stored) => stored,
        Err(error) => {
            tracing::error!(error = %error, "protection layers: rule record query failed");
            return Err(ProtectionReadError::Unavailable);
        }
    };

    // The overwhelmingly common case, and the one that must cost nothing
    // beyond the query above: no record, so the announcement is the whole of
    // the rules and there is no founder set to resolve.
    if stored.is_empty() {
        return Ok(RepositoryProtection {
            resolved: resolve_protection_layers(&[announcement_layer]),
            founders: None,
        });
    }

    let founders = match crate::api::git::verdict_admission::resolve_repository_founders(
        state,
        community,
        announcement,
    )
    .await
    {
        Ok(founders) => founders,
        // Fail closed: a roster we could not read is not evidence that these
        // records were written by strangers.
        Err(()) => return Err(ProtectionReadError::Unavailable),
    };

    let mut layers = vec![announcement_layer];
    for stored in stored {
        let event = stored.event;
        let Ok(record) = decode_repository_protection(&event) else {
            // A record this build cannot read is skipped, not fatal: refusing
            // every push on one malformed record would let any founder brick
            // the repository. It contributes no rules, which is the
            // subtracting direction.
            continue;
        };
        if !record.addresses(&owner_hex, repo_id) {
            continue;
        }
        if !founders.contains(record.author()) {
            continue;
        }
        layers.push(ProtectionLayer::from_record(
            &record,
            event.created_at.as_secs(),
        ));
    }

    Ok(RepositoryProtection {
        resolved: resolve_protection_layers(&layers),
        founders: Some(founders),
    })
}
