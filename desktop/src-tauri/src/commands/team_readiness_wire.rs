#![allow(dead_code)] // R2 consumes this pure seam when frontend wire truth lands.

use super::{
    TeamReadinessCatalog, TeamReadinessFact, TeamReadinessFactState, TeamReadinessRelay,
    TeamReadinessResponse, TeamReadinessScope, TeamReadinessStatus,
};
use crate::session_provider::runtimes::StrictRuntimeAuthState;

/// A catalog snapshot whose signature was checked by the wire-owning frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedTeamCatalogSnapshot {
    pub revision: u64,
    pub targets: Vec<String>,
}

/// Fold already-trusted wire observations over local readiness without I/O.
pub fn fold_trusted_team_wire(
    mut local: TeamReadinessResponse,
    snapshot: Option<TrustedTeamCatalogSnapshot>,
    relay_reachable: Option<bool>,
) -> TeamReadinessResponse {
    local.facts.retain(|fact| {
        fact.code != "CATALOG_AWAITING_FIRST_SESSION" && fact.code != "RELAY_UNOBSERVED"
    });
    let configured = local.registry.pending_targets.clone();
    let signed_targets = snapshot
        .as_ref()
        .map(|snapshot| &snapshot.targets)
        .cloned()
        .unwrap_or_default();
    local.registry.covered_targets = configured
        .iter()
        .filter(|target| signed_targets.contains(target))
        .cloned()
        .collect();
    local.registry.uncovered_targets = configured
        .iter()
        .filter(|target| !signed_targets.contains(target))
        .cloned()
        .collect();
    local.registry.pending_targets.clear();
    let covered = !local.registry.covered_targets.is_empty();
    for runtime in &mut local.runtimes {
        if signed_targets.iter().any(|target| {
            target
                .split_once(':')
                .is_some_and(|(provider, _)| provider == runtime.instance_ref)
        }) {
            runtime.auth = StrictRuntimeAuthState::Ready;
            runtime.model_probe = "trusted_provider_catalog".into();
        }
    }
    local.catalog = TeamReadinessCatalog {
        state: if snapshot.is_some() && covered {
            TeamReadinessFactState::Ready
        } else {
            TeamReadinessFactState::Unknown
        },
        revision: snapshot.as_ref().map(|snapshot| snapshot.revision),
        targets: snapshot
            .as_ref()
            .map(|snapshot| snapshot.targets.clone())
            .unwrap_or_default(),
        source: TeamReadinessScope::Wire,
    };
    local.relay = TeamReadinessRelay {
        state: match relay_reachable {
            Some(true) => TeamReadinessFactState::Ready,
            Some(false) => TeamReadinessFactState::Blocked,
            None => TeamReadinessFactState::Unknown,
        },
        reachable: relay_reachable,
        source: TeamReadinessScope::Wire,
    };
    if snapshot.is_none() {
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_TRUST_UNKNOWN",
            TeamReadinessFactState::Unknown,
            "No signature-verified catalog snapshot was supplied",
        ));
    } else if !covered {
        local.facts.push(wire_fact(
            "catalog",
            "CATALOG_TARGETS_UNCOVERED",
            TeamReadinessFactState::Unknown,
            "Trusted catalog has no target covered by a ready local runtime",
        ));
    }
    if snapshot.is_some() {
        let providers = signed_targets
            .iter()
            .filter_map(|target| target.split_once(':').map(|(provider, _)| provider))
            .collect::<std::collections::BTreeSet<_>>();
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
                "Trusted catalog exposes multiple provider targets"
            } else {
                "Trusted catalog exposes one provider target; launch remains supported"
            },
        ));
    }
    match relay_reachable {
        Some(true) => {}
        Some(false) => local.facts.push(wire_fact(
            "relay",
            "RELAY_UNREACHABLE",
            TeamReadinessFactState::Blocked,
            "The relay is unreachable",
        )),
        None => local.facts.push(wire_fact(
            "relay",
            "RELAY_REACHABILITY_UNKNOWN",
            TeamReadinessFactState::Unknown,
            "Relay reachability is unknown",
        )),
    }
    local.blocking_codes = codes(&local, TeamReadinessFactState::Blocked);
    local.unknown_codes = codes(&local, TeamReadinessFactState::Unknown);
    local.awaiting_codes.clear();
    local.limited_codes = codes(&local, TeamReadinessFactState::Limited);
    local.status = if !local.blocking_codes.is_empty() {
        TeamReadinessStatus::Blocked
    } else if !local.unknown_codes.is_empty() {
        TeamReadinessStatus::Unknown
    } else {
        TeamReadinessStatus::Ready
    };
    local.ready = local.ready_for_first_session && local.status == TeamReadinessStatus::Ready;
    local
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
