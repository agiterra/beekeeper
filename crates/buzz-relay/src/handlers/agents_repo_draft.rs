//! NIP-AD: the two relay-side checks a kind:44250 draft op gets beyond the
//! project-scoped write admission it shares with Pulse and to-dos.
//!
//! 1. **The draft names the project's agents repository.** `ad-repo` must be
//!    the repository the project's newest kind:30624 pins. Otherwise a draft
//!    could be written against a repository the project does not use, and a
//!    re-pointed project would keep accumulating drafts for the old one.
//! 2. **A `commit.record` names a commit on `main`.** The relay reads the
//!    repository's manifest chain (no pack is hydrated) and requires the
//!    commit to be `refs/heads/main` now, or to have been within the last
//!    [`crate::api::git::hydrate::MAIN_TIP_HISTORY`] pushes. A record is
//!    then a relay-checked fact that every reader may close drafts on. What
//!    stays unchecked, and NIP-AD says so: that the named drafts' text is
//!    what landed in that commit.
//!
//! Both are refusals of the *event's shape against the world*, not of the
//! author's authority, so they are `Rejected` (HTTP 400, CLI exit 2), not
//! `AuthFailed`. A storage failure is `Internal`, never a silent accept.

use buzz_core::agents_repo_draft::{AgentsRepoDraftOp, AgentsRepoDraftOpValue};
use buzz_core::kind::KIND_PROJECT_PACK_SOURCE;
use buzz_core::project_pack_source::decode_project_pack_source;
use buzz_core::tenant::TenantContext;
use buzz_db::EventQuery;
use nostr::Event;

use super::ingest::IngestError;
use crate::api::git::hydrate::{commit_was_main_tip, MainTipCheck};
use crate::state::AppState;

/// The repository the project's newest kind:30624 pins, as a canonical
/// `30617:<hex>:<id>` coordinate; `None` when the project has no source.
pub(crate) async fn project_agents_repository(
    state: &AppState,
    tenant: &TenantContext,
    coordinate: &str,
) -> Result<Option<String>, IngestError> {
    let query = EventQuery {
        kinds: Some(vec![KIND_PROJECT_PACK_SOURCE as i32]),
        d_tag: Some(coordinate.to_owned()),
        limit: Some(1),
        ..EventQuery::for_community(tenant.community())
    };
    let stored = state
        .db
        .query_events(&query)
        .await
        .map_err(|e| IngestError::Internal(format!("error: pack source lookup failed: {e}")))?;
    let Some(head) = stored.into_iter().next() else {
        return Ok(None);
    };
    let record = decode_project_pack_source(&head.event).map_err(|e| {
        IngestError::Internal(format!("error: stored pack source undecodable: {e}"))
    })?;
    Ok(Some(record.repo().to_owned()))
}

/// Split a canonical repository coordinate into `(owner hex, id)`.
fn repository_parts(coordinate: &str) -> Option<(&str, &str)> {
    let mut parts = coordinate.splitn(3, ':');
    let _kind = parts.next()?;
    let owner = parts.next()?;
    let id = parts.next()?;
    Some((owner, id))
}

/// Refuse a draft op that names a repository other than the project's, and
/// a `commit.record` whose commit is not on the repository's `main`.
pub(crate) async fn admit_draft_repository(
    state: &AppState,
    tenant: &TenantContext,
    event: &Event,
    op: &AgentsRepoDraftOp,
) -> Result<(), IngestError> {
    let coordinate = buzz_core::kind::project_a_scoped_coordinate(event)
        .ok_or_else(|| IngestError::Rejected("invalid: draft op requires one a tag".into()))?;
    let pinned = project_agents_repository(state, tenant, &coordinate).await?;
    match pinned.as_deref() {
        Some(repo) if repo == op.repo => {}
        Some(repo) => {
            return Err(IngestError::Rejected(format!(
                "invalid: {} is not this project's agents repository (its source pins {repo})",
                op.repo
            )));
        }
        None => {
            return Err(IngestError::Rejected(
                "invalid: this project has no agents repository (no kind:30624 source); \
                 create one with Finish repository setup or `bee packs init`"
                    .into(),
            ));
        }
    }
    let AgentsRepoDraftOpValue::CommitRecord { commit, .. } = &op.value else {
        return Ok(());
    };
    let (owner, id) = repository_parts(&op.repo)
        .ok_or_else(|| IngestError::Rejected("invalid: draft op ad-repo is malformed".into()))?;
    let check = commit_was_main_tip(&state.git_store, tenant, owner, id, commit)
        .await
        .map_err(|e| {
            IngestError::Internal(format!("error: repository state lookup failed: {e}"))
        })?;
    match check {
        MainTipCheck::Current | MainTipCheck::Recent { .. } => Ok(()),
        MainTipCheck::NotFound { main_now } => Err(IngestError::Rejected(format!(
            "invalid: commit {commit} is not refs/heads/main of {id} ({})",
            match main_now {
                Some(sha) => format!("main is at {sha}"),
                None => "the repository has no main".to_owned(),
            }
        ))),
        MainTipCheck::NoRepository => Err(IngestError::Rejected(format!(
            "invalid: commit {commit} cannot be on main: repository {id} has never been pushed"
        ))),
    }
}
