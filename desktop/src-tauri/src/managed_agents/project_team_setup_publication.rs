//! Publish a selected, already-checked setup snapshot without reopening the
//! editable draft.  The journal records observations separately: a failed or
//! interrupted push/source submission is never reported as adoption.

use std::io::Write;
use std::path::{Path, PathBuf};

use buzz_core_pkg::project_pack_source::{
    build_conditional_project_pack_source, PackPin, DEFAULT_PACK_PATH,
};
use nostr::{Event, EventBuilder, Kind, Tag};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};

use super::{
    context, read_draft, snapshot, tree, verify_context, ProjectTeamSetupDraft, SetupError,
    SetupScope,
};
use crate::app_state::AppState;
use crate::commands::project_git_exec::{build_git_auth_config_for_keys, run_git};
use crate::managed_agents::packs_cache;
use crate::managed_agents::{
    crew_roles, load_managed_agents, load_personas, load_teams, save_managed_agents, save_personas,
    save_teams,
};
use crate::session_provider::{
    commands as provider_commands, store as provider_store, supervisor, CodingSessionProviderState,
};
use buzz_core_pkg::coding_session_genesis::CodingSessionGenesisPayload;
use buzz_core_pkg::coding_session_identity::ProviderInstanceAlias;
use buzz_core_pkg::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core_pkg::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, ReceiptStatus,
};
use buzz_core_pkg::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use serde_json::json;

#[path = "project_team_setup_activation.rs"]
pub(crate) mod activation;
use activation::{InstallationJournal, ProjectTeamActivationSource, ProjectTeamLeadStatus};
#[path = "project_team_setup_publication_announcement.rs"]
mod announcement;
#[cfg(test)]
#[path = "project_team_setup_publication_blocker_tests.rs"]
mod blocker_tests;
#[path = "project_team_setup_publication_git.rs"]
mod git;
#[cfg(test)]
#[path = "project_team_setup_publication_tests.rs"]
mod tests;

const JOURNAL_LIMIT: u64 = 96 * 1024;
static PUBLICATION_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationDestination {
    pub repo_ref: String,
    pub pack_path: String,
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_announcement: Option<PublicationAnnouncement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationAnnouncement {
    pub name: String,
    pub description: String,
}

/// Relay-enforced expectation; it is checked locally before work and encoded
/// into the v2 event for the relay to compare atomically at adoption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PublicationSourceExpectation {
    IfUnset,
    Expected {
        #[serde(rename = "eventId", alias = "event_id")]
        event_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PublicationOutput {
    Snapshot {
        #[serde(rename = "snapshotId", alias = "snapshot_id")]
        snapshot_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationRequest {
    pub destination: PublicationDestination,
    pub source_expectation: PublicationSourceExpectation,
    pub output: PublicationOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    Checking,
    CandidatePrepared,
    PushUnknown,
    Pushed,
    SourceUnknown,
    Adopted,
    /// The exact signed source was observed, but a later source now wins.
    /// Keep this distinct from a rejected CAS: the candidate remains real,
    /// while this publication is no longer the project's effective source.
    Superseded,
    Conflict,
    Refused,
}

/// The public, durable publication journal projection.  A `null` event id is
/// intentionally ambiguous only where `status` says the operation is still
/// unknown; callers must offer retry instead of inventing completion.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamSetupPublication {
    pub publication_id: String,
    pub setup_id: String,
    pub snapshot_id: String,
    pub destination: PublicationDestination,
    pub source_expectation: PublicationSourceExpectation,
    pub candidate_ref: String,
    pub candidate_commit: Option<String>,
    pub source_event_id: Option<String>,
    pub status: PublicationStatus,
    pub message: Option<String>,
}

/// Host-read facts a renderer may use to fill a publication request. The host
/// derives this destination from the scoped project and active owner; the
/// webview never selects a repository, path, clone URL, or base revision.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamSetupPublicationOptions {
    pub current_source_event_id: Option<String>,
    pub suggested_destination: Option<PublicationDestination>,
    pub source_expectation: PublicationSourceExpectation,
    pub publication: Option<ProjectTeamSetupPublication>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PublicationJournal {
    version: u32,
    setup_id: String,
    project_ref: String,
    owner_pubkey: String,
    relay_url: String,
    publication_id: String,
    request: PublicationRequest,
    candidate_ref: String,
    candidate_commit: Option<String>,
    source_event: Option<Event>,
    #[serde(default)]
    pub(super) installation: Option<InstallationJournal>,
    #[serde(default)]
    pub(super) lead: Option<LeadJournal>,
    status: PublicationStatus,
    message: Option<String>,
}

/// The channel reservation and exact signed launch records are retained with
/// the publication. A repeated click must replay these bytes, never create a
/// second project lead session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LeadJournal {
    pub(super) channel_id: String,
    pub(super) lead_pubkey: String,
    #[serde(default)]
    pub(super) session_ref: Option<String>,
    #[serde(default)]
    pub(super) create_command_id: Option<String>,
    #[serde(default)]
    pub(super) provider_pubkey: Option<String>,
    #[serde(default)]
    pub(super) provider_instance_ref: Option<String>,
    #[serde(default)]
    pub(super) runtime: Option<String>,
    #[serde(default)]
    pub(super) driver: Option<String>,
    #[serde(default)]
    pub(super) provider_host_instance_id: Option<String>,
    #[serde(default)]
    pub(super) model: Option<String>,
    #[serde(default)]
    pub(super) genesis_event: Option<Event>,
    pub(super) create_event: Option<Event>,
    pub(super) status: ProjectTeamLeadStatus,
    pub(super) message: Option<String>,
}

fn invalid(message: impl Into<String>) -> SetupError {
    SetupError::new("invalid_publication", message)
}

fn external(message: impl Into<String>) -> SetupError {
    SetupError::new("publication_unavailable", message)
}

fn journal_path(draft: &ProjectTeamSetupDraft) -> Result<PathBuf, SetupError> {
    let parent = Path::new(&draft.draft_directory)
        .parent()
        .ok_or_else(|| invalid("The setup draft has no storage directory."))?;
    Ok(parent.join("publication.json"))
}

fn candidate_dir(
    draft: &ProjectTeamSetupDraft,
    publication_id: &str,
) -> Result<PathBuf, SetupError> {
    let parent = Path::new(&draft.draft_directory)
        .parent()
        .ok_or_else(|| invalid("The setup draft has no storage directory."))?;
    Ok(parent
        .join("publications")
        .join(publication_id)
        .join("candidate"))
}

fn bound_draft(
    root: &Path,
    scope: &SetupScope,
    setup_id: &str,
) -> Result<ProjectTeamSetupDraft, SetupError> {
    let draft =
        read_draft(root, scope)?.ok_or_else(|| invalid("Prepare the setup draft first."))?;
    if draft.setup_id != setup_id {
        return Err(invalid("The setup ID does not match the preserved draft."));
    }
    Ok(draft)
}

fn validate_request(request: &PublicationRequest) -> Result<(), SetupError> {
    packs_cache::parse_repo_coordinate(&request.destination.repo_ref).map_err(invalid)?;
    packs_cache::validate_pack_path(&request.destination.pack_path).map_err(invalid)?;
    if request
        .destination
        .base_commit
        .as_deref()
        .is_some_and(|sha| {
            sha.len() != 40
                || !sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return Err(invalid(
            "The destination base commit must be a lowercase 40-hex SHA.",
        ));
    }
    if let Some(announcement) = &request.destination.create_announcement {
        if announcement.name.trim().is_empty()
            || announcement.name.len() > 256
            || announcement.description.len() > 4096
        {
            return Err(invalid(
                "The repository announcement has an invalid name or description.",
            ));
        }
    }
    if let PublicationSourceExpectation::Expected { event_id } = &request.source_expectation {
        if event_id.len() != 64
            || !event_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "The expected pack-source event ID must be lowercase 64-hex.",
            ));
        }
        if request.destination.base_commit.is_none() {
            return Err(invalid(
                "Replacing a project source requires its immutable base commit.",
            ));
        }
    }
    let PublicationOutput::Snapshot { snapshot_id } = &request.output;
    if snapshot_id.len() != 64
        || !snapshot_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid(
            "A selected snapshot ID must be a lowercase SHA-256 digest.",
        ));
    }
    Ok(())
}

fn validate_journal(
    journal: &PublicationJournal,
    draft: &ProjectTeamSetupDraft,
) -> Result<(), SetupError> {
    if journal.version != 1
        || journal.setup_id != draft.setup_id
        || journal.project_ref != draft.project_ref
        || journal.owner_pubkey != draft.owner_pubkey
        || journal.relay_url != draft.relay_url
        || uuid::Uuid::parse_str(&journal.publication_id).is_err()
        || journal.candidate_ref != format!("refs/heads/setup/{}", journal.publication_id)
    {
        return Err(invalid(
            "The publication journal does not belong to this setup scope.",
        ));
    }
    validate_request(&journal.request)
}

fn load_journal(draft: &ProjectTeamSetupDraft) -> Result<Option<PublicationJournal>, SetupError> {
    let path = journal_path(draft)?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => tree::check_regular_file(&path, JOURNAL_LIMIT)?,
    }
    let journal = serde_json::from_slice(&std::fs::read(&path)?)
        .map_err(|error| invalid(format!("Could not read the publication journal: {error}")))?;
    validate_journal(&journal, draft)?;
    Ok(Some(journal))
}

fn save_journal(
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
) -> Result<(), SetupError> {
    validate_journal(journal, draft)?;
    let path = journal_path(draft)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Missing publication storage directory."))?;
    let bytes = serde_json::to_vec_pretty(journal).map_err(|error| invalid(error.to_string()))?;
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

fn project(journal: &PublicationJournal) -> ProjectTeamSetupPublication {
    let PublicationOutput::Snapshot { snapshot_id } = &journal.request.output;
    ProjectTeamSetupPublication {
        publication_id: journal.publication_id.clone(),
        setup_id: journal.setup_id.clone(),
        snapshot_id: snapshot_id.clone(),
        destination: journal.request.destination.clone(),
        source_expectation: journal.request.source_expectation.clone(),
        candidate_ref: journal.candidate_ref.clone(),
        candidate_commit: journal.candidate_commit.clone(),
        source_event_id: journal.source_event.as_ref().map(|event| event.id.to_hex()),
        status: journal.status,
        message: journal.message.clone(),
    }
}

fn adopted_source(journal: &PublicationJournal) -> Result<ProjectTeamActivationSource, SetupError> {
    if journal.status != PublicationStatus::Adopted {
        return Err(invalid(
            "Install project roles only after the exact source is adopted.",
        ));
    }
    let commit = journal
        .candidate_commit
        .clone()
        .ok_or_else(|| invalid("The adopted source has no immutable commit."))?;
    Ok(ProjectTeamActivationSource {
        repo_ref: journal.request.destination.repo_ref.clone(),
        commit,
        pack_path: journal.request.destination.pack_path.clone(),
    })
}

#[derive(Clone)]
struct CurrentSource {
    event_id: String,
    destination: PublicationDestination,
}

fn host_destination(
    draft: &ProjectTeamSetupDraft,
    source: Option<&CurrentSource>,
) -> Result<PublicationDestination, SetupError> {
    if let Some(source) = source {
        return Ok(source.destination.clone());
    }
    let (_, slug) = crate::managed_agents::packs_repo::parse_project_coordinate(&draft.project_ref)
        .map_err(invalid)?;
    let readable =
        crate::managed_agents::packs_repo::default_packs_repo_id(&slug).map_err(invalid)?;
    // Repository ownership is the active host identity, so a slug alone would
    // collide when that identity works with two founders' projects named alike.
    // Bind the id to the complete canonical coordinate while retaining a
    // readable stem for repository discovery.
    let coordinate_digest = hex::encode(Sha256::digest(draft.project_ref.as_bytes()));
    let stem: String = readable.chars().take(64 - 1 - 12).collect();
    let repo_id = format!(
        "{}-{}",
        stem.trim_end_matches('-'),
        &coordinate_digest[..12]
    );
    let repo_ref = format!("30617:{}:{repo_id}", draft.owner_pubkey);
    packs_cache::parse_repo_coordinate(&repo_ref).map_err(invalid)?;
    Ok(PublicationDestination {
        repo_ref,
        pack_path: DEFAULT_PACK_PATH.to_string(),
        base_commit: None,
        create_announcement: Some(PublicationAnnouncement {
            name: format!("{slug} role packs"),
            description: format!("Role packs for the {slug} project team."),
        }),
    })
}

fn expected_source_expectation(source: Option<&CurrentSource>) -> PublicationSourceExpectation {
    match source {
        Some(source) => PublicationSourceExpectation::Expected {
            event_id: source.event_id.clone(),
        },
        None => PublicationSourceExpectation::IfUnset,
    }
}

async fn current_source(
    state: &AppState,
    project_ref: &str,
) -> Result<Option<CurrentSource>, SetupError> {
    let events = crate::relay::query_relay(
        state,
        &[serde_json::json!({
            "kinds": [buzz_core_pkg::kind::KIND_PROJECT_PACK_SOURCE],
            "#d": [project_ref], "limit": 8,
        })],
    )
    .await
    .map_err(|error| {
        external(format!(
            "The current project pack source could not be read: {error}"
        ))
    })?;
    let mut newest: Option<((u64, String), CurrentSource)> = None;
    for event in events {
        let Ok(decoded) = buzz_core_pkg::project_pack_source::decode_project_pack_source(&event)
        else {
            continue;
        };
        let ordering = (event.created_at.as_secs(), event.id.to_hex());
        if event.verify().is_ok()
            && newest
                .as_ref()
                .is_none_or(|(current, _)| ordering > *current)
        {
            newest = Some((
                ordering,
                CurrentSource {
                    event_id: event.id.to_hex(),
                    destination: PublicationDestination {
                        repo_ref: decoded.repo().to_string(),
                        pack_path: decoded.path().to_string(),
                        base_commit: decoded.pin().as_sha().map(str::to_string),
                        create_announcement: None,
                    },
                },
            ));
        }
    }
    Ok(newest.map(|(_, source)| source))
}

/// Reconcile the effective addressable source after a submission.  A POST
/// acknowledgement only proves the relay received the bytes; it cannot prove
/// this event remains the source an installer will resolve.
async fn reconcile_source_adoption(
    state: &AppState,
    journal: &mut PublicationJournal,
) -> Result<(), SetupError> {
    reconcile_source_observation(journal, current_source(state, &journal.project_ref).await?)
}

fn reconcile_source_observation(
    journal: &mut PublicationJournal,
    current_source: Option<CurrentSource>,
) -> Result<(), SetupError> {
    let event = journal
        .source_event
        .as_ref()
        .ok_or_else(|| invalid("A source reconciliation needs the exact saved source event."))?;
    match current_source {
        Some(current) if current.event_id == event.id.to_hex() => {
            journal.status = PublicationStatus::Adopted;
            journal.message = None;
        }
        Some(current) => {
            journal.status = PublicationStatus::Superseded;
            journal.message = Some(format!(
                "The saved source was not the effective project source; {} now wins.",
                current.event_id
            ));
        }
        None => {
            journal.status = PublicationStatus::SourceUnknown;
            journal.message = Some(
                "The conditional source was submitted, but its effective source could not yet be observed."
                    .to_string(),
            );
        }
    }
    Ok(())
}

// Kept as a narrow test seam while candidate construction itself lives in the
// byte-oriented helper. Production calls that helper through `prepare_candidate`.
#[cfg(test)]
fn create_candidate(
    draft: &ProjectTeamSetupDraft,
    publication_id: &str,
    request: &PublicationRequest,
    files: Vec<(String, Vec<u8>)>,
    owner: &nostr::Keys,
) -> Result<String, SetupError> {
    git::create_candidate(draft, publication_id, &request.destination, &files, owner)
}

fn push_candidate(
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
    owner: &nostr::Keys,
) -> Result<(), SetupError> {
    let candidate = candidate_dir(draft, &journal.publication_id)?;
    let (repo_owner, repo_id) =
        packs_cache::parse_repo_coordinate(&journal.request.destination.repo_ref)
            .map_err(invalid)?;
    let relay_http = crate::relay::relay_http_base_url(&journal.relay_url);
    let remote = packs_cache::packs_clone_url(&relay_http, &repo_owner, &repo_id);
    let auth = build_git_auth_config_for_keys(owner).map_err(external)?;
    let expected = journal
        .candidate_commit
        .as_deref()
        .ok_or_else(|| invalid("A candidate push needs its journaled commit."))?;
    if let Ok(existing) = run_git(
        &["ls-remote", "--", &remote, &journal.candidate_ref],
        None,
        &auth,
    ) {
        if existing
            .split_whitespace()
            .next()
            .is_some_and(|sha| sha == expected)
        {
            return Ok(());
        }
    }
    let lease = format!("--force-with-lease={}:", journal.candidate_ref);
    let refspec = format!("HEAD:{}", journal.candidate_ref);
    run_git(
        &["push", "--porcelain", &lease, "--", &remote, &refspec],
        Some(&candidate),
        &auth,
    )
    .map_err(external)?;
    let observed = run_git(
        &["ls-remote", "--", &remote, &journal.candidate_ref],
        None,
        &auth,
    )
    .map_err(external)?;
    if observed
        .split_whitespace()
        .next()
        .is_some_and(|sha| sha == expected)
    {
        Ok(())
    } else {
        Err(external(
            "The candidate push completed without the recorded immutable ref being observable.",
        ))
    }
}

fn source_event(journal: &PublicationJournal, owner: &nostr::Keys) -> Result<Event, SetupError> {
    let sha = journal
        .candidate_commit
        .as_deref()
        .ok_or_else(|| invalid("A source cannot be adopted without a candidate commit."))?;
    let expected = match &journal.request.source_expectation {
        PublicationSourceExpectation::IfUnset => None,
        PublicationSourceExpectation::Expected { event_id } => Some(event_id.as_str()),
    };
    let draft = build_conditional_project_pack_source(
        &journal.project_ref,
        &journal.request.destination.repo_ref,
        &PackPin::Sha(sha.to_string()),
        Some(&journal.request.destination.pack_path),
        Some("project-team setup snapshot"),
        expected,
    )
    .map_err(invalid)?;
    let tags = draft
        .tags
        .into_iter()
        .map(Tag::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| invalid(error.to_string()))?;
    EventBuilder::new(
        Kind::Custom(buzz_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16),
        draft.content,
    )
    .tags(tags)
    .sign_with_keys(owner)
    .map_err(|error| {
        external(format!(
            "Could not sign the conditional pack source: {error}"
        ))
    })
}

async fn adopt_source(
    draft: &ProjectTeamSetupDraft,
    state: &AppState,
    journal: &mut PublicationJournal,
    owner: &nostr::Keys,
) -> Result<(), SetupError> {
    if journal.source_event.is_some() {
        // A lost response is not permission to replay a conditional write.
        // Reconcile the exact retained event first: our own adoption changes
        // the expected head, so a blind retry would manufacture a false CAS
        // conflict. Absence is still ambiguous (the event may have been
        // accepted and later deleted), so preserve SourceUnknown rather than
        // resurrecting it.
        reconcile_source_adoption(state, journal).await?;
        if matches!(
            journal.status,
            PublicationStatus::Adopted | PublicationStatus::Superseded
        ) {
            return Ok(());
        }
        journal.status = PublicationStatus::SourceUnknown;
        journal.message = Some(
            "The saved source is not currently observable; its retained signed bytes will not be replayed blindly."
                .to_string(),
        );
        return Ok(());
    }
    if journal.source_event.is_none() {
        match source_event(journal, owner) {
            Ok(event) => journal.source_event = Some(event),
            Err(error) => {
                journal.status = PublicationStatus::Refused;
                journal.message = Some(error.message);
                save_journal(draft, journal)?;
                return Ok(());
            }
        }
    }
    journal.status = PublicationStatus::SourceUnknown;
    journal.message = Some(
        "The conditional source request was saved; relay outcome is not yet known.".to_string(),
    );
    // Persist the exact signed event and ambiguity before the network write.
    // A retry must resend these bytes, never sign a replacement timestamp.
    save_journal(draft, journal)?;
    let Some(event) = journal.source_event.as_ref() else {
        return Ok(());
    };
    match crate::relay::submit_signed_event_with_keys(event, state, owner, None).await {
        Ok(_) => {
            reconcile_source_adoption(state, journal).await?;
        }
        Err(error) if error.contains("PACK_SOURCE_CONFLICT") || error.contains("conflict") => {
            journal.status = PublicationStatus::Conflict;
            journal.message = Some(error);
        }
        Err(error) => journal.message = Some(format!("The relay outcome is unknown: {error}")),
    }
    Ok(())
}

async fn prepare_candidate(
    root: &Path,
    scope: &SetupScope,
    draft: &ProjectTeamSetupDraft,
    state: &AppState,
    journal: &mut PublicationJournal,
) -> Result<(), SetupError> {
    if journal.status != PublicationStatus::Checking || journal.candidate_commit.is_some() {
        return Ok(());
    }
    let PublicationOutput::Snapshot { snapshot_id } = &journal.request.output;
    let root = root.to_path_buf();
    let scope = SetupScope::new(&scope.project, &scope.owner, &scope.relay)?;
    let setup_id = draft.setup_id.clone();
    let snapshot_id = snapshot_id.clone();
    let expected_snapshot_id = snapshot_id.clone();
    let verified = tokio::task::spawn_blocking(move || {
        snapshot::run_verified_contents(&root, &scope, &setup_id, &snapshot_id)
    })
    .await
    .map_err(|error| external(error.to_string()))??;
    if verified.snapshot.snapshot_id != expected_snapshot_id {
        return Err(invalid(
            "The reverified snapshot did not retain the selected content address.",
        ));
    }
    let keys = state.signing_keys().map_err(external)?;
    let candidate_draft = draft.clone();
    let destination = journal.request.destination.clone();
    let publication_id = journal.publication_id.clone();
    let files = verified.files;
    let recovered = tokio::task::spawn_blocking(move || {
        match git::recover_candidate(
            &candidate_draft,
            &publication_id,
            &destination,
            &files,
            &keys,
        )? {
            Some(sha) => Ok(sha),
            None => git::create_candidate(
                &candidate_draft,
                &publication_id,
                &destination,
                &files,
                &keys,
            ),
        }
    })
    .await
    .map_err(|error| external(format!("Candidate construction did not finish: {error}")))?;
    match recovered {
        Ok(sha) => {
            journal.candidate_commit = Some(sha);
            journal.status = PublicationStatus::CandidatePrepared;
            journal.message = None;
            save_journal(draft, journal)?;
        }
        Err(error) => {
            journal.status = PublicationStatus::Refused;
            journal.message = Some(error.message);
            save_journal(draft, journal)?;
        }
    }
    Ok(())
}

/// Start (or return) the one durable selected-snapshot publication for this
/// setup.  The current source is read before candidate work, then the signed
/// v2 event repeats that expectation at the relay's atomic compare-and-store.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn project_team_setup_start_publication(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    destination: PublicationDestination,
    source_expectation: PublicationSourceExpectation,
    output: PublicationOutput,
) -> Result<ProjectTeamSetupPublication, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let request = PublicationRequest {
        destination,
        source_expectation,
        output,
    };
    validate_request(&request)?;
    let _guard = PUBLICATION_LOCK.lock().await;
    if let Some(existing) = load_journal(&draft)? {
        if existing.request != request {
            return Err(invalid(
                "This setup already has a different durable publication request.",
            ));
        }
        return Ok(project(&existing));
    }
    let current = current_source(&state, &draft.project_ref).await?;
    if current.is_some() {
        return Err(SetupError::new(
            "source_changed",
            "This neutral setup draft has no sealed source provenance, so it cannot replace existing project procedures. Start a source-seeded maintenance setup instead.",
        ));
    }
    let expected_destination = host_destination(&draft, current.as_ref())?;
    let expected_expectation = expected_source_expectation(current.as_ref());
    if request.destination != expected_destination
        || request.source_expectation != expected_expectation
    {
        return Err(SetupError::new(
            "source_changed",
            "The host-derived project publication destination or source expectation changed; refresh setup before publishing.",
        ));
    }
    let publication_id = uuid::Uuid::new_v4().to_string();
    let mut journal = PublicationJournal {
        version: 1,
        setup_id: draft.setup_id.clone(),
        project_ref: draft.project_ref.clone(),
        owner_pubkey: draft.owner_pubkey.clone(),
        relay_url: draft.relay_url.clone(),
        publication_id: publication_id.clone(),
        request,
        candidate_ref: format!("refs/heads/setup/{publication_id}"),
        candidate_commit: None,
        source_event: None,
        installation: None,
        lead: None,
        status: PublicationStatus::Checking,
        message: None,
    };
    save_journal(&draft, &journal)?;
    continue_publication(&root, &scope, &draft, &state, &mut journal).await?;
    verify_context(&state, &scope)?;
    Ok(project(&journal))
}

/// Read the live source and any retained publication without creating a
/// repository, candidate, source event, or install.  This is the only safe
/// renderer input for the CAS expectation and existing-source destination.
#[tauri::command]
pub async fn project_team_setup_get_publication_options(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
) -> Result<ProjectTeamSetupPublicationOptions, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let source = current_source(&state, &draft.project_ref).await?;
    if let Some(mut journal) = load_journal(&draft)? {
        if journal.source_event.is_some() {
            reconcile_source_observation(&mut journal, source.clone())?;
            save_journal(&draft, &journal)?;
        }
        verify_context(&state, &scope)?;
        return Ok(ProjectTeamSetupPublicationOptions {
            current_source_event_id: source.as_ref().map(|source| source.event_id.clone()),
            suggested_destination: Some(journal.request.destination.clone()),
            source_expectation: journal.request.source_expectation.clone(),
            publication: Some(project(&journal)),
        });
    }
    let source_expectation = expected_source_expectation(source.as_ref());
    verify_context(&state, &scope)?;
    Ok(ProjectTeamSetupPublicationOptions {
        current_source_event_id: source.as_ref().map(|source| source.event_id.clone()),
        // Preparation currently seeds the neutral foundation only. Do not
        // present an enabled replace action until maintenance records the
        // existing source's sealed seed provenance.
        suggested_destination: source
            .is_none()
            .then(|| host_destination(&draft, None))
            .transpose()?,
        source_expectation,
        publication: None,
    })
}

async fn continue_publication(
    root: &Path,
    scope: &SetupScope,
    draft: &ProjectTeamSetupDraft,
    state: &AppState,
    journal: &mut PublicationJournal,
) -> Result<(), SetupError> {
    let keys = state.signing_keys().map_err(external)?;
    prepare_candidate(root, scope, draft, state, journal).await?;
    if journal.status == PublicationStatus::Refused {
        return Ok(());
    }
    if !announcement::ensure(draft, state, journal, &keys).await? {
        return Ok(());
    }
    if matches!(
        journal.status,
        PublicationStatus::CandidatePrepared | PublicationStatus::PushUnknown
    ) {
        journal.status = PublicationStatus::PushUnknown;
        journal.message =
            Some("Candidate push started; its remote outcome is not yet known.".to_string());
        save_journal(draft, journal)?;
        let candidate_draft = draft.clone();
        let retry = journal.clone();
        let keys_for_push = keys.clone();
        match tokio::task::spawn_blocking(move || {
            push_candidate(&candidate_draft, &retry, &keys_for_push)
        })
        .await
        {
            Ok(Ok(())) => {
                journal.status = PublicationStatus::Pushed;
                journal.message = None;
            }
            Ok(Err(error)) => {
                journal.message = Some(format!(
                    "Candidate push outcome is unknown: {}",
                    error.message
                ))
            }
            Err(error) => {
                journal.message = Some(format!("Candidate push outcome is unknown: {error}"))
            }
        }
        save_journal(draft, journal)?;
    }
    if matches!(
        journal.status,
        PublicationStatus::Pushed | PublicationStatus::SourceUnknown
    ) {
        adopt_source(draft, state, journal, &keys).await?;
        save_journal(draft, journal)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn project_team_setup_get_publication(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
) -> Result<Option<ProjectTeamSetupPublication>, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let source = current_source(&state, &draft.project_ref).await?;
    let publication = match load_journal(&draft)? {
        Some(mut journal) => {
            if journal.source_event.is_some() {
                reconcile_source_observation(&mut journal, source)?;
                save_journal(&draft, &journal)?;
            }
            Some(project(&journal))
        }
        None => None,
    };
    verify_context(&state, &scope)?;
    Ok(publication)
}

#[tauri::command]
pub async fn project_team_setup_continue_publication(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    publication_id: String,
) -> Result<ProjectTeamSetupPublication, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let _guard = PUBLICATION_LOCK.lock().await;
    let mut journal =
        load_journal(&draft)?.ok_or_else(|| invalid("Start publication before retrying it."))?;
    if journal.publication_id != publication_id {
        return Err(invalid(
            "The publication ID does not match this setup's journal.",
        ));
    }
    continue_publication(&root, &scope, &draft, &state, &mut journal).await?;
    verify_context(&state, &scope)?;
    Ok(project(&journal))
}
