//! Durable repository-announcement publication for the initial project pack.

use std::io::Write;
use std::path::Path;

use nostr::{Event, EventBuilder, Kind, Tag};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;

const JOURNAL_LIMIT: u64 = 96 * 1024;
const KIND: u16 = 30617;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    version: u32,
    publication_id: String,
    repo_ref: String,
    relay_url: String,
    owner_pubkey: String,
    event: Event,
    #[serde(default)]
    submission_started: bool,
}

fn path(draft: &ProjectTeamSetupDraft) -> Result<std::path::PathBuf, SetupError> {
    Ok(Path::new(&draft.draft_directory)
        .parent()
        .ok_or_else(|| invalid("The setup draft has no storage directory."))?
        .join("publication-announcement.json"))
}

fn validate(
    value: &Journal,
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
) -> Result<(), SetupError> {
    if value.version != 1
        || value.publication_id != journal.publication_id
        || value.repo_ref != journal.request.destination.repo_ref
        || value.relay_url != draft.relay_url
        || value.owner_pubkey != draft.owner_pubkey
        || value.event.kind != Kind::Custom(KIND)
        || value.event.pubkey.to_hex() != draft.owner_pubkey
        || value.event.verify().is_err()
    {
        return Err(invalid(
            "The saved repository announcement does not belong to this publication scope.",
        ));
    }
    Ok(())
}

fn load(
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
) -> Result<Option<Journal>, SetupError> {
    let path = path(draft)?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => tree::check_regular_file(&path, JOURNAL_LIMIT)?,
    }
    let value = serde_json::from_slice(&std::fs::read(path)?).map_err(|error| {
        invalid(format!(
            "Could not read the repository announcement: {error}"
        ))
    })?;
    validate(&value, draft, journal)?;
    Ok(Some(value))
}

fn save(
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
    value: &Journal,
) -> Result<(), SetupError> {
    validate(value, draft, journal)?;
    let path = path(draft)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Missing announcement storage directory."))?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| invalid(error.to_string()))?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut file = temporary.reopen()?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    temporary.persist(&path).map_err(|error| error.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn build(
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
    owner: &nostr::Keys,
) -> Result<Journal, SetupError> {
    let announcement = journal
        .request
        .destination
        .create_announcement
        .as_ref()
        .ok_or_else(|| invalid("The publication lost its required repository announcement."))?;
    let (repo_owner, repo_id) =
        packs_cache::parse_repo_coordinate(&journal.request.destination.repo_ref)
            .map_err(invalid)?;
    if repo_owner != draft.owner_pubkey {
        return Err(invalid(
            "A new project packs repository must be owned by the scoped active identity.",
        ));
    }
    let clone_url = packs_cache::packs_clone_url(
        &crate::relay::relay_http_base_url(&draft.relay_url),
        &repo_owner,
        &repo_id,
    );
    let tags = [
        vec!["d".to_string(), repo_id],
        vec!["name".to_string(), announcement.name.clone()],
        vec!["description".to_string(), announcement.description.clone()],
        vec!["clone".to_string(), clone_url],
        vec!["project".to_string(), draft.project_ref.clone()],
    ]
    .into_iter()
    .map(Tag::parse)
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| invalid(error.to_string()))?;
    let event = EventBuilder::new(Kind::Custom(KIND), String::new())
        .tags(tags)
        .sign_with_keys(owner)
        .map_err(|error| {
            external(format!(
                "Could not sign the repository announcement: {error}"
            ))
        })?;
    Ok(Journal {
        version: 1,
        publication_id: journal.publication_id.clone(),
        repo_ref: journal.request.destination.repo_ref.clone(),
        relay_url: draft.relay_url.clone(),
        owner_pubkey: draft.owner_pubkey.clone(),
        event,
        submission_started: false,
    })
}

async fn observation(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
    saved: &Journal,
) -> Result<Option<bool>, SetupError> {
    let (_, repo_id) = packs_cache::parse_repo_coordinate(&journal.request.destination.repo_ref)
        .map_err(invalid)?;
    let events = crate::relay::query_relay(
        state,
        &[json!({"kinds":[KIND],"authors":[draft.owner_pubkey],"#d":[repo_id],"limit":8})],
    )
    .await
    .map_err(|error| {
        external(format!(
            "The repository announcement could not be read: {error}"
        ))
    })?;
    if events.len() >= 8 {
        return Err(external(
            "The repository announcement query reached its safety bound.",
        ));
    }
    let mut other = false;
    for event in events {
        if event.kind != Kind::Custom(KIND) || event.verify().is_err() {
            continue;
        }
        if event.id == saved.event.id {
            return Ok(Some(true));
        }
        other = true;
    }
    Ok(other.then_some(false))
}

/// Persist the exact signed 30617 before sending it. A collision never
/// replaces a different announcement; an uncertain response remains retryable
/// with the saved bytes.
pub(super) async fn ensure(
    draft: &ProjectTeamSetupDraft,
    state: &AppState,
    journal: &mut PublicationJournal,
    owner: &nostr::Keys,
) -> Result<bool, SetupError> {
    if journal.request.destination.create_announcement.is_none() {
        return Ok(true);
    }
    let mut saved = match load(draft, journal)? {
        Some(value) => value,
        None => {
            let value = build(draft, journal, owner)?;
            save(draft, journal, &value)?;
            value
        }
    };
    match observation(state, draft, journal, &saved).await? {
        Some(true) => return Ok(true),
        Some(false) => {
            journal.status = PublicationStatus::Refused;
            journal.message = Some("A different repository announcement already occupies this host-derived destination.".to_string());
            save_journal(draft, journal)?;
            return Ok(false);
        }
        None => {}
    }
    saved.submission_started = true;
    save(draft, journal, &saved)?;
    match crate::relay::submit_signed_event_with_keys(&saved.event, state, owner, None).await {
        Ok(_) => match observation(state, draft, journal, &saved).await? {
            Some(true) => Ok(true),
            Some(false) => {
                journal.status = PublicationStatus::Refused;
                journal.message = Some(
                    "A different repository announcement won the host-derived destination."
                        .to_string(),
                );
                save_journal(draft, journal)?;
                Ok(false)
            }
            None => {
                journal.message = Some("The repository announcement was submitted but is not yet observable; retry uses its saved signed bytes.".to_string());
                save_journal(draft, journal)?;
                Ok(false)
            }
        },
        Err(error) => {
            journal.message = Some(format!(
                "The repository announcement outcome is unknown: {error}"
            ));
            save_journal(draft, journal)?;
            Ok(false)
        }
    }
}
