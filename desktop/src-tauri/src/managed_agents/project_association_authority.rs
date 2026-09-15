//! Proof that the active identity may associate agents with a project, read
//! from signed project state (`docs/PROJECT_AGENT_HIRING_IMPL.md`).
//!
//! The renderer's owner/collaborator check is presentation only. The native
//! association command calls [`read_association_authority`] before it writes,
//! and refuses whenever the project head or roster cannot be read and
//! verified: an unreadable project is never treated as permission.
//!
//! The same verified head answers whether a project is public, which decides
//! whether its agents' association digest may be announced on their
//! world-readable kind:30177 ([`verify_project_visibility`]).

use std::collections::{BTreeMap, BTreeSet};

use nostr::Event;
use serde_json::json;
use tauri::{AppHandle, Manager};

use super::project_agent_association::{normalize_project_ref, ASSOCIATION_MALFORMED_PROJECT};
use super::ManagedAgentRecord;
use crate::app_state::AppState;
use buzz_core_pkg::kind::{
    is_valid_project_role, KIND_PROJECT, KIND_PROJECT_MEMBERS, PROJECT_ACCESS_PRIVATE,
    PROJECT_ACCESS_TAG, PROJECT_ROLE_COLLABORATOR, PROJECT_ROLE_OWNER,
};

/// Refusal for an identity that is not the project's creator, owner or
/// collaborator.
pub(crate) const ASSOCIATION_NOT_PROJECT_WRITER: &str =
    "Only this project's creator, owners and collaborators can associate agents with it.";

/// Refusal when the signing identity changed between the authority read and
/// the write.
pub(crate) const ASSOCIATION_IDENTITY_CHANGED: &str =
    "The signing identity changed while this computer was checking project authority. Nothing was changed.";

/// Refusal when the project head, the community's roster signer, or the
/// roster could not be read and verified.
pub(crate) fn association_project_unreadable(coordinate: &str) -> String {
    let project = coordinate.splitn(3, ':').nth(2).unwrap_or(coordinate);
    format!(
        "This computer could not read {project} from the relay, so it cannot confirm you may associate agents with it. Nothing was changed."
    )
}

/// Whether a verified project head is community-readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectVisibility {
    Public,
    Private,
}

impl ProjectVisibility {
    pub(crate) fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }
}

/// `(owner-hex, dtag)` of a normalized coordinate.
fn split_coordinate(normalized: &str) -> Option<(&str, &str)> {
    let mut parts = normalized.splitn(3, ':');
    parts.next()?;
    Some((parts.next()?, parts.next()?))
}

fn is_lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Exactly one `name` tag, and it is `[name, value]`.
fn single_tag(event: &Event, name: &str, value: &str) -> bool {
    let mut tags = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some(name));
    matches!((tags.next(), tags.next()), (Some(tag), None) if tag.as_slice() == [name, value])
}

/// The newest event of `kind` by `author` with the exact `d` tag whose
/// signature verifies. Anything else in `events` is ignored.
fn newest<'a>(events: &'a [Event], kind: u32, author: &str, d: &str) -> Option<&'a Event> {
    events
        .iter()
        .filter(|event| {
            u32::from(event.kind.as_u16()) == kind
                && event.pubkey.to_hex() == author
                && single_tag(event, "d", d)
                && event.verify().is_ok()
        })
        .max_by_key(|event| (event.created_at, event.id))
}

/// The project's verified current head: kind:30621 signed by the coordinate's
/// owner with its exact `d` tag, newest first.
pub(crate) fn verified_project_head<'a>(
    coordinate: &str,
    events: &'a [Event],
) -> Option<&'a Event> {
    let normalized = normalize_project_ref(coordinate)?;
    let (owner, dtag) = split_coordinate(&normalized)?;
    newest(events, KIND_PROJECT, owner, dtag)
}

/// A head carrying `["buzz-access","private"]` is private; otherwise public.
pub(crate) fn head_visibility(head: &Event) -> ProjectVisibility {
    let private = head.tags.iter().any(|tag| {
        let parts = tag.as_slice();
        parts.first().map(String::as_str) == Some(PROJECT_ACCESS_TAG)
            && parts.get(1).map(String::as_str) == Some(PROJECT_ACCESS_PRIVATE)
    });
    if private {
        ProjectVisibility::Private
    } else {
        ProjectVisibility::Public
    }
}

/// `(pubkey, role)` rows from an event's `p` tags. A missing or unknown role
/// element is a legacy collaborator, exactly as `bee projects members` reads
/// it (`roster_from_event_json`).
fn roster_rows(event: &Event) -> Vec<(String, &str)> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let parts = tag.as_slice();
            if parts.first().map(String::as_str) != Some("p") {
                return None;
            }
            let role = parts
                .get(3)
                .map(String::as_str)
                .filter(|role| is_valid_project_role(role))
                .unwrap_or(PROJECT_ROLE_COLLABORATOR);
            Some((parts.get(1)?.to_ascii_lowercase(), role))
        })
        .collect()
}

/// Whether `active_pubkey` may associate agents with the project at
/// `coordinate`, and the project's visibility when it may.
///
/// - The head is the newest verified kind:30621 by the coordinate's owner
///   with its exact `d`; without one this refuses as unreadable.
/// - The creator is authorized.
/// - Otherwise the roster is the newest verified kind:39010 signed by
///   `relay_signer` whose `d` is the normalized coordinate; only when none
///   exists do the head's own `p` tags stand in. The identity must hold only
///   owner or collaborator rows, and at least one. A malformed
///   `relay_signer` refuses as unreadable, because whether a roster exists
///   cannot then be known.
pub(crate) fn association_authority(
    coordinate: &str,
    active_pubkey: &str,
    head_events: &[Event],
    roster_events: &[Event],
    relay_signer: &str,
) -> Result<ProjectVisibility, String> {
    let normalized = normalize_project_ref(coordinate)
        .ok_or_else(|| ASSOCIATION_MALFORMED_PROJECT.to_string())?;
    let (owner, _) =
        split_coordinate(&normalized).ok_or_else(|| ASSOCIATION_MALFORMED_PROJECT.to_string())?;
    let head = verified_project_head(&normalized, head_events)
        .ok_or_else(|| association_project_unreadable(&normalized))?;
    let visibility = head_visibility(head);
    let active = active_pubkey.trim().to_ascii_lowercase();
    if active == owner {
        return Ok(visibility);
    }
    let signer = relay_signer.trim().to_ascii_lowercase();
    if !is_lower_hex64(&signer) {
        return Err(association_project_unreadable(&normalized));
    }
    let roster = newest(roster_events, KIND_PROJECT_MEMBERS, &signer, &normalized).unwrap_or(head);
    let roles: Vec<&str> = roster_rows(roster)
        .into_iter()
        .filter(|(pubkey, _)| *pubkey == active)
        .map(|(_, role)| role)
        .collect();
    let writer = !roles.is_empty()
        && roles
            .iter()
            .all(|role| *role == PROJECT_ROLE_OWNER || *role == PROJECT_ROLE_COLLABORATOR);
    if writer {
        Ok(visibility)
    } else {
        Err(ASSOCIATION_NOT_PROJECT_WRITER.to_string())
    }
}

/// The community's metadata signing identity (NIP-11 `self`).
async fn relay_signer(state: &AppState, base: &str) -> Result<String, String> {
    let response = state
        .http_client
        .get(base)
        .header("Accept", "application/nostr+json")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("relay metadata answered {}", response.status()));
    }
    let value: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
    let signer = value
        .get("self")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "relay metadata names no signing identity".to_string())?;
    nostr::PublicKey::from_hex(signer)
        .map(|key| key.to_hex())
        .map_err(|_| "relay metadata signing identity is malformed".to_string())
}

async fn read_project_heads(
    state: &AppState,
    base: &str,
    keys: &nostr::Keys,
    owner: &str,
    dtag: &str,
) -> Result<Vec<Event>, String> {
    let filter = json!({"kinds": [KIND_PROJECT], "authors": [owner], "#d": [dtag], "limit": 8});
    crate::relay::query_relay_at_with_keys(state, base, &[filter], keys, None).await
}

/// Read the project's signed head (and, for a non-creator, the community's
/// roster signer and roster) as `keys`, then decide with
/// [`association_authority`]. Every read failure refuses.
pub(crate) async fn read_association_authority(
    state: &AppState,
    project_ref: &str,
    keys: &nostr::Keys,
) -> Result<ProjectVisibility, String> {
    let normalized = normalize_project_ref(project_ref)
        .ok_or_else(|| ASSOCIATION_MALFORMED_PROJECT.to_string())?;
    let (owner, dtag) =
        split_coordinate(&normalized).ok_or_else(|| ASSOCIATION_MALFORMED_PROJECT.to_string())?;
    let unreadable = |step: &str, error: String| {
        tracing::warn!(project = %normalized, step, %error, "project association authority read failed");
        association_project_unreadable(&normalized)
    };
    let base = crate::relay::relay_api_base_url_with_override(state);
    let heads = read_project_heads(state, &base, keys, owner, dtag)
        .await
        .map_err(|error| unreadable("head", error))?;
    let active = keys.public_key().to_hex();
    if active == owner {
        return association_authority(&normalized, &active, &heads, &[], "");
    }
    let signer = relay_signer(state, &base)
        .await
        .map_err(|error| unreadable("relay signer", error))?;
    let filter = json!({
        "kinds": [KIND_PROJECT_MEMBERS],
        "authors": [signer],
        "#d": [normalized],
        "limit": 8,
    });
    let rosters = crate::relay::query_relay_at_with_keys(state, &base, &[filter], keys, None)
        .await
        .map_err(|error| unreadable("roster", error))?;
    association_authority(&normalized, &active, &heads, &rosters, &signer)
}

/// The distinct normalized projects of associated agents whose visibility has
/// not been read yet.
pub(crate) fn projects_needing_visibility(records: &[ManagedAgentRecord]) -> Vec<String> {
    records
        .iter()
        .filter(|record| record.project_public.is_none())
        .filter_map(|record| {
            record
                .project_ref
                .as_deref()
                .and_then(normalize_project_ref)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Record each read visibility on the agents of that project still lacking
/// one, returning the pubkeys changed. A known visibility is never replaced
/// here, and a project without a result is left unknown.
pub(crate) fn apply_visibility_results(
    records: &mut [ManagedAgentRecord],
    results: &BTreeMap<String, ProjectVisibility>,
) -> Vec<String> {
    let mut changed = Vec::new();
    for record in records
        .iter_mut()
        .filter(|record| record.project_public.is_none())
    {
        let Some(project) = record
            .project_ref
            .as_deref()
            .and_then(normalize_project_ref)
        else {
            continue;
        };
        if let Some(visibility) = results.get(&project) {
            record.project_public = Some(visibility.is_public());
            changed.push(record.pubkey.clone());
        }
    }
    changed
}

/// Read the verified head of every project whose agents have no recorded
/// visibility (a journal backfill, a setup installation, an older build) and
/// record it. Unreadable heads stay unknown, so nothing is announced for
/// them. Saves under the store lock only when something changed, and queues
/// a kind:30177 republish for exactly those agents. Returns how many changed.
pub(crate) async fn verify_project_visibility(app: &AppHandle) -> Result<usize, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "app state is unavailable".to_string())?;
    let keys = state.signing_keys()?;
    let load_app = app.clone();
    let records = tauri::async_runtime::spawn_blocking(move || {
        let state = load_app.state::<AppState>();
        let _guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        super::load_managed_agents(&load_app)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))??;
    let projects = projects_needing_visibility(&records);
    if projects.is_empty() {
        return Ok(0);
    }
    let base = crate::relay::relay_api_base_url_with_override(&state);
    let mut results = BTreeMap::new();
    for project in projects {
        let Some((owner, dtag)) = split_coordinate(&project) else {
            continue;
        };
        match read_project_heads(&state, &base, &keys, owner, dtag).await {
            Ok(events) => {
                if let Some(head) = verified_project_head(&project, &events) {
                    results.insert(project.clone(), head_visibility(head));
                }
            }
            Err(error) => {
                tracing::warn!(%project, %error, "project visibility read failed; left unknown")
            }
        }
    }
    if results.is_empty() {
        return Ok(0);
    }
    let save_app = app.clone();
    let reader = keys.public_key();
    tauri::async_runtime::spawn_blocking(move || {
        let state = save_app.state::<AppState>();
        let _guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        // Facts read from another community or as another identity are not
        // applied to this one.
        if crate::relay::relay_api_base_url_with_override(&state) != base
            || state.signing_keys()?.public_key() != reader
        {
            return Ok(0);
        }
        let mut records = super::load_managed_agents(&save_app)?;
        let changed = apply_visibility_results(&mut records, &results);
        if changed.is_empty() {
            return Ok(0);
        }
        super::save_managed_agents(&save_app, &records)?;
        for record in records
            .iter()
            .filter(|record| changed.contains(&record.pubkey))
        {
            crate::commands::retain_managed_agent_pending(&save_app, &state, record);
        }
        Ok(changed.len())
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// [`verify_project_visibility`] off the caller's path, logging its outcome.
pub(crate) fn spawn_project_visibility_verification(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match verify_project_visibility(&app).await {
            Ok(0) => {}
            Ok(count) => tracing::info!("recorded project visibility for {count} agents"),
            Err(error) => tracing::warn!("project visibility verification skipped: {error}"),
        }
    });
}

#[cfg(test)]
#[path = "project_association_authority_tests.rs"]
mod tests;
