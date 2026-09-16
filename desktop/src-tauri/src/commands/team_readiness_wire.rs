//! Signed wire observations for Team Readiness.
//!
//! Trust stays behind the native boundary: this module loads the configured
//! provider signer allowlist, queries the active relay with the user's
//! authenticated key, re-verifies every event, and only then folds catalog
//! target coverage over the side-effect-free local inventory.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use buzz_core_pkg::coding_session_catalog::{parse_catalog, Catalog, CatalogProvider};
use buzz_core_pkg::kind::KIND_CODING_SESSION_PROVIDER_CATALOG;
use nostr::Event;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::{
    TeamReadinessCatalog, TeamReadinessFact, TeamReadinessFactState, TeamReadinessRelay,
    TeamReadinessResponse, TeamReadinessScope, TeamReadinessStatus,
};
use crate::app_state::AppState;

// One beyond the supported history: seeing the sentinel means the relay may
// have truncated a newer revision or same-revision conflict, so fail closed.
const CATALOG_HISTORY_LIMIT: usize = 1001;
const CATALOG_TAG_VERSION: &str = "cspc1-1";

/// Provenance for one exact signed catalog used by readiness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessCatalogProvenance {
    /// Exact signed event id used by the fold.
    pub event_id: String,
    /// Channel whose catalog was used.
    pub channel_id: String,
    /// Trusted provider bridge signer.
    pub signer_pubkey: String,
    /// Canonical catalog revision selected for this source.
    pub revision: u64,
}

/// Native result of validating the active relay's catalogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedTeamCatalogSnapshot {
    /// Signed catalog heads selected by source.
    pub provenance: Vec<TeamReadinessCatalogProvenance>,
    /// Canonical `providerInstanceRef:model` targets covering the project.
    pub targets: Vec<String>,
    /// Signed events excluded by structural, signature, or scope validation.
    pub invalid_event_count: usize,
    /// Ambiguous source heads or provider declarations.
    pub conflict_count: usize,
}

/// Wire query outcome, keeping reachability distinct from malformed truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamReadinessWireObservation {
    /// Relay query succeeded and produced a verified snapshot (possibly empty).
    Reached(TrustedTeamCatalogSnapshot),
    /// The authenticated relay seam classified the relay as unreachable.
    Unreachable,
    /// No target channel exists yet; only local first-session truth can gate.
    AwaitingChannel,
    /// Trust or query truth could not be established without guessing.
    Unknown {
        /// Stable machine-readable blocker code.
        code: String,
        /// Bounded operator-facing explanation.
        summary: String,
        /// Concrete recovery action, when one is known.
        remedy: Option<String>,
    },
}

#[derive(Clone)]
struct ParsedCatalogEvent {
    event_id: String,
    channel_id: String,
    signer_pubkey: String,
    created_at: u64,
    content: String,
    catalog: Catalog,
}

#[derive(Default)]
struct SourceHead {
    revision: u64,
    candidates: Vec<ParsedCatalogEvent>,
}

/// Query the exact active relay/channel scope and validate provider catalogs.
pub async fn observe_team_wire(
    app: &AppHandle,
    project_ref: &str,
    expected_relay_url: &str,
    channel_ids: &[String],
) -> Result<TeamReadinessWireObservation, String> {
    let state = app.state::<AppState>();
    ensure_active_relay_scope(
        expected_relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
    )?;
    let channels = normalize_channel_scope(channel_ids)?;
    if channels.is_empty() {
        return Ok(TeamReadinessWireObservation::AwaitingChannel);
    }

    let app_for_config = app.clone();
    let trust_result = tauri::async_runtime::spawn_blocking(move || {
        super::host::load_allowed_bridge_pubkeys_readonly(&app_for_config)
    })
    .await;
    let trust_observation = match trust_result {
        Ok(result) => match classify_trusted_signers(result) {
            Ok(signers) => Ok(signers),
            Err(observation) => Err(observation),
        },
        Err(_) => Err(unknown_observation(
            "TRUST_CONFIG_UNAVAILABLE",
            "Trusted provider signer metadata could not be read",
            Some("Restore access to the app metadata directory, then re-read readiness"),
        )),
    };

    // Loading trust crosses an await boundary. The active community may have
    // changed while the file was read. Recheck before returning either success
    // or structured Unknown; stale failures must not be attributed to the new
    // community either.
    let trusted_signers = match finish_trust_observation_for_scope(
        expected_relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
        trust_observation,
    )? {
        Ok(signers) => signers,
        Err(observation) => return Ok(observation),
    };

    let filter = provider_catalog_filter(&trusted_signers, &channels);
    // Pin the request itself to the caller's already-validated relay. Using
    // `query_relay` here would resolve the mutable workspace override again and
    // could query community B after validating community A.
    let relay_http_url = crate::relay::relay_http_base_url(expected_relay_url);
    let query = crate::relay::query_relay_at(&state, &relay_http_url, &[filter]).await;
    // The query is another await boundary. Even though its URL was pinned, its
    // result no longer belongs to the active UI scope after a community change.
    ensure_active_relay_scope(
        expected_relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
    )?;
    match query {
        Ok(events) => Ok(classify_catalog_query(
            &events,
            &trusted_signers,
            &channels,
            project_ref,
        )),
        Err(error) if error.starts_with("relay unreachable:") => {
            Ok(TeamReadinessWireObservation::Unreachable)
        }
        Err(_) => Ok(unknown_observation(
            "RELAY_QUERY_UNKNOWN",
            "The active relay query did not return verifiable catalog truth",
            Some("Re-authenticate to the active community, then re-read readiness"),
        )),
    }
}

pub(super) fn ensure_active_relay_scope(expected: &str, active: &str) -> Result<(), String> {
    if normalize_relay_scope(expected).is_empty()
        || normalize_relay_scope(expected) != normalize_relay_scope(active)
    {
        return Err("the active community changed during Team Readiness; re-read it".into());
    }
    Ok(())
}

pub(super) fn finish_trust_observation_for_scope(
    expected: &str,
    active: &str,
    observation: Result<Vec<String>, TeamReadinessWireObservation>,
) -> Result<Result<Vec<String>, TeamReadinessWireObservation>, String> {
    ensure_active_relay_scope(expected, active)?;
    Ok(observation)
}

pub(super) fn classify_catalog_query(
    events: &[Event],
    trusted_signers: &[String],
    channels: &[String],
    project_ref: &str,
) -> TeamReadinessWireObservation {
    if events.len() >= CATALOG_HISTORY_LIMIT {
        return unknown_observation(
            "CATALOG_HISTORY_OVERFLOW",
            "Provider catalog history exceeds the bounded readiness window",
            Some("Resolve stale or conflicting provider catalogs, then re-read readiness"),
        );
    }
    TeamReadinessWireObservation::Reached(validate_catalog_events(
        events,
        trusted_signers,
        channels,
        project_ref,
    ))
}

pub(super) fn classify_trusted_signers(
    result: Result<Vec<String>, String>,
) -> Result<Vec<String>, TeamReadinessWireObservation> {
    match result {
        Ok(signers) if !signers.is_empty() => Ok(signers),
        Ok(_) => Err(unknown_observation(
            "TRUST_SIGNERS_MISSING",
            "No trusted provider bridge signer is configured",
            Some("Configure an allowed bridge public key, then re-read readiness"),
        )),
        Err(_) => Err(unknown_observation(
            "TRUST_CONFIG_INVALID",
            "Trusted provider signer metadata is malformed or unreadable",
            Some("Repair global-agent-config.json without changing key custody, then re-read readiness"),
        )),
    }
}

fn unknown_observation(
    code: &str,
    summary: &str,
    remedy: Option<&str>,
) -> TeamReadinessWireObservation {
    TeamReadinessWireObservation::Unknown {
        code: code.into(),
        summary: summary.into(),
        remedy: remedy.map(str::to_string),
    }
}

fn normalize_relay_scope(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn normalize_channel_scope(channel_ids: &[String]) -> Result<Vec<String>, String> {
    let mut channels = channel_ids
        .iter()
        .map(|channel| channel.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    channels.sort();
    channels.dedup();
    if channels
        .iter()
        .any(|channel| uuid::Uuid::parse_str(channel).is_err())
    {
        return Err("Team Readiness channel scope must contain only channel UUIDs".into());
    }
    Ok(channels)
}

pub(super) fn provider_catalog_filter(
    trusted_signers: &[String],
    channel_ids: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "kinds": [KIND_CODING_SESSION_PROVIDER_CATALOG],
        "authors": trusted_signers,
        "#h": channel_ids,
        "limit": CATALOG_HISTORY_LIMIT,
    })
}

pub(super) fn validate_catalog_events(
    events: &[Event],
    trusted_signers: &[String],
    channel_ids: &[String],
    project_ref: &str,
) -> TrustedTeamCatalogSnapshot {
    let trusted = trusted_signers.iter().cloned().collect::<HashSet<_>>();
    let channels = channel_ids.iter().cloned().collect::<HashSet<_>>();
    let mut invalid_event_count = 0;
    let mut heads = BTreeMap::<(String, String), SourceHead>::new();
    let mut seen_event_ids = HashSet::new();
    for event in events {
        if !seen_event_ids.insert(event.id.to_hex()) {
            continue;
        }
        let Some(parsed) = parse_catalog_event(event, &trusted, &channels) else {
            invalid_event_count += 1;
            continue;
        };
        let key = (parsed.channel_id.clone(), parsed.signer_pubkey.clone());
        let head = heads.entry(key).or_default();
        if parsed.catalog.revision > head.revision {
            head.revision = parsed.catalog.revision;
            head.candidates = vec![parsed];
        } else if parsed.catalog.revision == head.revision {
            head.candidates.push(parsed);
        }
    }

    let mut conflict_count = 0;
    let mut selected = Vec::new();
    for head in heads.into_values() {
        let contents = head
            .candidates
            .iter()
            .map(|candidate| candidate.content.as_str())
            .collect::<BTreeSet<_>>();
        if contents.len() != 1 {
            conflict_count += 1;
            continue;
        }
        if let Some(winner) = head.candidates.into_iter().max_by(|left, right| {
            (left.created_at, left.event_id.as_str())
                .cmp(&(right.created_at, right.event_id.as_str()))
        }) {
            selected.push(winner);
        }
    }

    let mut declarations = BTreeMap::<String, Vec<(&ParsedCatalogEvent, &CatalogProvider)>>::new();
    for entry in &selected {
        for provider in providers_for_project(&entry.catalog, project_ref) {
            declarations
                .entry(provider.provider_instance_ref.clone())
                .or_default()
                .push((entry, provider));
        }
    }
    let mut targets = BTreeSet::new();
    for declarations in declarations.into_values() {
        let provider = declarations[0].1;
        if declarations
            .iter()
            .any(|(_, contender)| *contender != provider)
        {
            conflict_count += 1;
            continue;
        }
        for model in &provider.allowed_models {
            targets.insert(format!("{}:{model}", provider.provider_instance_ref));
        }
    }
    let mut provenance = selected
        .into_iter()
        .map(|entry| TeamReadinessCatalogProvenance {
            event_id: entry.event_id,
            channel_id: entry.channel_id,
            signer_pubkey: entry.signer_pubkey,
            revision: entry.catalog.revision,
        })
        .collect::<Vec<_>>();
    provenance.sort_by(|left, right| {
        (
            left.channel_id.as_str(),
            left.signer_pubkey.as_str(),
            left.revision,
            left.event_id.as_str(),
        )
            .cmp(&(
                right.channel_id.as_str(),
                right.signer_pubkey.as_str(),
                right.revision,
                right.event_id.as_str(),
            ))
    });
    TrustedTeamCatalogSnapshot {
        provenance,
        targets: targets.into_iter().collect(),
        invalid_event_count,
        conflict_count,
    }
}

fn parse_catalog_event(
    event: &Event,
    trusted: &HashSet<String>,
    channels: &HashSet<String>,
) -> Option<ParsedCatalogEvent> {
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_PROVIDER_CATALOG || event.verify().is_err()
    {
        return None;
    }
    let signer_pubkey = event.pubkey.to_hex();
    if !trusted.contains(&signer_pubkey) {
        return None;
    }
    let tags = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    if tags.len() != 4 || tags.iter().any(|tag| tag.len() != 2) {
        return None;
    }
    if tags[0][0] != "h" || !channels.contains(&tags[0][1]) {
        return None;
    }
    if tags[1] != ["cspc-v", CATALOG_TAG_VERSION] {
        return None;
    }
    let catalog = parse_catalog(&event.content).ok()?;
    if tags[2] != ["cspc-revision", &catalog.revision.to_string()] {
        return None;
    }
    let expected_key = buzz_sdk_pkg::coding_session::coding_session_provider_catalog_semantic_key(
        &tags[0][1],
        catalog.revision,
        &event.content,
    );
    if tags[3] != ["cspc-key", expected_key.as_str()] {
        return None;
    }
    Some(ParsedCatalogEvent {
        event_id: event.id.to_hex(),
        channel_id: tags[0][1].clone(),
        signer_pubkey,
        created_at: event.created_at.as_secs(),
        content: event.content.clone(),
        catalog,
    })
}

fn providers_for_project<'a>(catalog: &'a Catalog, project_ref: &str) -> Vec<&'a CatalogProvider> {
    if catalog.projects.is_empty() {
        return catalog.providers.iter().collect();
    }
    let Some(project) = catalog
        .projects
        .iter()
        .find(|project| project.project_ref == project_ref && project.repo_ref.is_none())
    else {
        return Vec::new();
    };
    catalog
        .providers
        .iter()
        .filter(|provider| project.providers.contains(&provider.provider_instance_ref))
        .collect()
}

/// Fold native wire truth over the already-computed local readiness report.
pub fn fold_trusted_team_wire(
    mut local: TeamReadinessResponse,
    observation: TeamReadinessWireObservation,
) -> TeamReadinessResponse {
    local.facts.retain(|fact| {
        fact.code != "CATALOG_AWAITING_FIRST_SESSION" && fact.code != "RELAY_UNOBSERVED"
    });
    match observation {
        TeamReadinessWireObservation::Reached(snapshot) => {
            local.relay = TeamReadinessRelay {
                state: TeamReadinessFactState::Ready,
                reachable: Some(true),
                source: TeamReadinessScope::Wire,
            };
            apply_reached_catalog(&mut local, snapshot);
        }
        TeamReadinessWireObservation::Unreachable => {
            local.relay = TeamReadinessRelay {
                state: TeamReadinessFactState::Blocked,
                reachable: Some(false),
                source: TeamReadinessScope::Wire,
            };
            local.catalog.state = TeamReadinessFactState::Unknown;
            local.facts.push(wire_fact(
                "relay",
                "RELAY_UNREACHABLE",
                TeamReadinessFactState::Blocked,
                "The active community relay is unreachable",
            ));
        }
        TeamReadinessWireObservation::AwaitingChannel => {
            local.relay = TeamReadinessRelay {
                state: TeamReadinessFactState::AwaitingFirstSession,
                reachable: None,
                source: TeamReadinessScope::Wire,
            };
            local.catalog = TeamReadinessCatalog {
                state: TeamReadinessFactState::AwaitingFirstSession,
                revision: None,
                targets: Vec::new(),
                source: TeamReadinessScope::Wire,
                provenance: Vec::new(),
            };
            local.facts.push(TeamReadinessFact {
                category: "catalog".into(),
                code: "CATALOG_AWAITING_FIRST_SESSION".into(),
                scope: TeamReadinessScope::Wire,
                state: TeamReadinessFactState::AwaitingFirstSession,
                summary: "No target channel exists for a signed provider catalog yet".into(),
                remedy: Some(
                    "Launch the first session; Prepare does not create its channel".into(),
                ),
            });
            local.facts.push(TeamReadinessFact {
                category: "relay".into(),
                code: "RELAY_UNOBSERVED".into(),
                scope: TeamReadinessScope::Wire,
                state: TeamReadinessFactState::AwaitingFirstSession,
                summary: "Relay truth is awaiting the first session's target channel".into(),
                remedy: Some("Launch the first session to establish the channel".into()),
            });
        }
        TeamReadinessWireObservation::Unknown {
            code,
            summary,
            remedy,
        } => {
            local.relay = TeamReadinessRelay {
                state: TeamReadinessFactState::Unknown,
                reachable: None,
                source: TeamReadinessScope::Wire,
            };
            local.catalog.state = TeamReadinessFactState::Unknown;
            local.facts.push(wire_fact(
                "relay",
                &code,
                TeamReadinessFactState::Unknown,
                &summary,
            ));
            if let Some(fact) = local.facts.last_mut() {
                fact.remedy = remedy;
            }
        }
    }
    summarize(&mut local);
    local
}

fn apply_reached_catalog(local: &mut TeamReadinessResponse, snapshot: TrustedTeamCatalogSnapshot) {
    local.catalog = TeamReadinessCatalog {
        state: TeamReadinessFactState::Unknown,
        revision: (snapshot.provenance.len() == 1).then(|| snapshot.provenance[0].revision),
        targets: snapshot.targets.clone(),
        source: TeamReadinessScope::Wire,
        provenance: snapshot.provenance.clone(),
    };
    if snapshot.invalid_event_count > 0 {
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_EVENTS_INVALID",
            TeamReadinessFactState::Unknown,
            "The active relay returned a provider catalog that failed signature or canonical validation",
        ));
    }
    if snapshot.conflict_count > 0 {
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_CONFLICT",
            TeamReadinessFactState::Unknown,
            "Trusted provider catalogs conflict on a source revision or provider target",
        ));
    }
    if snapshot.provenance.is_empty()
        && snapshot.invalid_event_count == 0
        && snapshot.conflict_count == 0
    {
        local.catalog.state = TeamReadinessFactState::AwaitingFirstSession;
        local.facts.push(TeamReadinessFact {
            category: "catalog".into(),
            code: "CATALOG_AWAITING_FIRST_SESSION".into(),
            scope: TeamReadinessScope::Wire,
            state: TeamReadinessFactState::AwaitingFirstSession,
            summary: "No trusted provider-signed catalog exists on this relay yet".into(),
            remedy: Some(
                "Launch the first session; no catalog is fabricated during Prepare.".into(),
            ),
        });
        return;
    }
    let configured = local.registry.pending_targets.clone();
    local.registry.covered_targets = configured
        .iter()
        .filter(|target| snapshot.targets.contains(target))
        .cloned()
        .collect();
    local.registry.uncovered_targets = configured
        .iter()
        .filter(|target| !snapshot.targets.contains(target))
        .cloned()
        .collect();
    local.registry.pending_targets.clear();
    if configured.is_empty() {
        // No local registry pinned a provider or model target — absent,
        // unreadable, or an empty version-1 registry — so there is nothing
        // here for the signed catalog to cover. Emitting
        // CATALOG_TARGETS_UNCOVERED anyway asserted a fact this project's own
        // REGISTRY_UNREADABLE=Limited fact already disclosed does not apply:
        // a project whose packs come from a project source names its own
        // runtime and model per role, and coverage for that is judged per
        // role pack at hire time, not against a registry that does not exist
        // (ledger 137 left this contradiction standing; ledger 140 closes
        // it). This is Limited, not Unknown: nothing here is unverifiable,
        // there is simply nothing local to check coverage against.
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_COVERAGE_NOT_APPLICABLE",
            TeamReadinessFactState::Limited,
            "No local registry pins a provider or model target; each role pack's runtime \
             and model coverage is judged at hire time instead",
        ));
    } else if local.registry.covered_targets.is_empty() {
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_TARGETS_UNCOVERED",
            TeamReadinessFactState::Unknown,
            "No signed provider target covers this project's ready local registry",
        ));
    } else if snapshot.invalid_event_count == 0 && snapshot.conflict_count == 0 {
        local.catalog.state = TeamReadinessFactState::Ready;
    }
    if !local.registry.uncovered_targets.is_empty() {
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_TARGETS_PARTIAL",
            TeamReadinessFactState::Limited,
            "Some local registry targets are not present in the signed provider catalog",
        ));
    }
    let providers = snapshot
        .targets
        .iter()
        .filter_map(|target| target.split_once(':').map(|(provider, _)| provider))
        .collect::<BTreeSet<_>>();
    local.facts.push(wire_fact(
        "runtime",
        if providers.len() > 1 {
            "PROVIDER_DIVERSITY_AVAILABLE"
        } else {
            "PROVIDER_DIVERSITY_LIMITED"
        },
        if providers.len() > 1 {
            TeamReadinessFactState::Ready
        } else {
            TeamReadinessFactState::Limited
        },
        if providers.len() > 1 {
            "Signed catalogs expose multiple provider targets"
        } else {
            "Signed catalogs expose one provider target; launch remains supported"
        },
    ));
}

fn summarize(local: &mut TeamReadinessResponse) {
    local.blocking_codes = codes(local, TeamReadinessFactState::Blocked);
    local.unknown_codes = codes(local, TeamReadinessFactState::Unknown);
    local.awaiting_codes = codes(local, TeamReadinessFactState::AwaitingFirstSession);
    local.limited_codes = codes(local, TeamReadinessFactState::Limited);
    local.status = if !local.blocking_codes.is_empty() {
        TeamReadinessStatus::Blocked
    } else if !local.unknown_codes.is_empty() {
        TeamReadinessStatus::Unknown
    } else if !local.awaiting_codes.is_empty() {
        TeamReadinessStatus::AwaitingFirstSession
    } else {
        TeamReadinessStatus::Ready
    };
    local.ready = local.ready_for_first_session && local.status == TeamReadinessStatus::Ready;
}

fn wire_fact(
    category: &str,
    code: &str,
    state: TeamReadinessFactState,
    summary: &str,
) -> TeamReadinessFact {
    TeamReadinessFact {
        category: category.into(),
        code: code.into(),
        scope: TeamReadinessScope::Wire,
        state,
        summary: summary.into(),
        remedy: None,
    }
}

fn codes(response: &TeamReadinessResponse, state: TeamReadinessFactState) -> Vec<String> {
    response
        .facts
        .iter()
        .filter(|fact| fact.state == state)
        .map(|fact| fact.code.clone())
        .collect()
}
