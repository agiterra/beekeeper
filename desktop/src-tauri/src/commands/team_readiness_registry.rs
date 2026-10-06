//! Where readiness looks for a project's model registry, and which copy
//! answered.
//!
//! # The finding this module exists for
//!
//! Ledger 207(1). Founding the first team session of "Kettle Smoke" on
//! 2026-09-20, the founder's readiness panel printed
//! `REGISTRY_UNREADABLE · This project pins no provider or model targets
//! (file-missing: /Users/brian/Projects/Kettle Smoke/kettle-smoke/team/model-registry.yaml …)
//! Remedy: Add team/model-registry.yaml to the checkout`. The project had a
//! registry: lane 180's seed had written `model-registry.yaml` at the root of
//! `kettle-smoke-beekeeper-agents`, and minutes later a routed hire resolved
//! from it. Readiness was reading the code checkout and nothing else — the
//! one place lane 180 had already established is not the only place a
//! registry lives (ledger 180).
//!
//! So readiness now asks the *same* resolver the hire host asks
//! ([`crate::commands::model_registry`], over [`beekeeper_core_pkg::model_registry_source`]):
//! the agents repository first, then the code checkout. The answer names
//! which copy it came from, and a project with none is told both places that
//! were looked in and the remedy that matches the layout it actually has.

use super::{Gathered, TeamReadinessFact, TeamReadinessFactState};
use crate::coding_sessions::workdir_store::CodingSessionWorkdirStore;
use crate::commands::model_registry::{
    host_model_registry_candidates, resolve_host_model_registry, HostModelRegistryCandidate,
};

/// The remedy sentence for a project with no registry in either place.
///
/// It follows the layout this computer records, because the checkout-only
/// advice is wrong for every project created under spec § 4.11: its registry
/// belongs in the agents repository, which is the copy the hire reads first.
pub(super) fn missing_registry_remedy(candidates: &[HostModelRegistryCandidate]) -> String {
    let agents_repo = candidates
        .iter()
        .find(|candidate| candidate.candidate.origin.as_str() == "agents-repo");
    if let Some(agents_repo) = agents_repo {
        return format!(
            "Add model-registry.yaml to this project's agents repository ({}) and commit it — \
             that is the copy a hire reads first.",
            agents_repo.root.display()
        );
    }
    if candidates.is_empty() {
        return "Create this project's repositories (Project settings → Packs → Finish \
                repository setup); the agents-repository seed writes model-registry.yaml at \
                its root."
            .to_owned();
    }
    "Add team/model-registry.yaml to the checkout to pin the provider and model targets this \
     project routes to."
        .to_owned()
}

/// Read the project's registry through the shared resolver, recording where
/// it came from, or push the `REGISTRY_UNREADABLE` fact that names both
/// places.
///
/// Returns the registry text when one was found.
pub(super) fn resolve_registry_text(
    store: Option<&CodingSessionWorkdirStore>,
    project_ref: &str,
    packs_from_project: bool,
    gathered: &mut Gathered,
) -> Option<String> {
    let candidates = store
        .map(|store| host_model_registry_candidates(store, project_ref))
        .unwrap_or_default();
    match resolve_host_model_registry(&candidates) {
        Ok(read) => {
            gathered.registry.origin = Some(read.origin);
            gathered.registry.origin_label = Some(read.origin_label);
            gathered.registry.path = Some(read.path);
            gathered.registry.looked_in = read.looked_in;
            Some(read.text)
        }
        Err(missing) => {
            gathered.registry.looked_in.clone_from(&missing.looked_in);
            // A project whose roles come from a packs repository does not need
            // a registry — every pack names its own runtime and model, and the
            // hire honours that — so its absence limits what readiness can
            // say rather than refusing a session (ledger 135(c)).
            let mut fact = if packs_from_project {
                TeamReadinessFact::local(
                    "routing",
                    "REGISTRY_UNREADABLE",
                    TeamReadinessFactState::Limited,
                    format!(
                        "This project pins no provider or model targets ({missing}); each role \
                         pack names its own runtime and model instead"
                    ),
                )
            } else {
                TeamReadinessFact::local(
                    "routing",
                    "REGISTRY_UNREADABLE",
                    TeamReadinessFactState::Blocked,
                    format!(
                        "This project's model registry could not be read, so nothing says which \
                         provider and model its sessions route to ({missing})"
                    ),
                )
            };
            fact.remedy = Some(missing_registry_remedy(&candidates));
            gathered.facts.push(fact);
            None
        }
    }
}

/// The clause the coverage fact carries so the panel says which copy answered.
pub(super) fn registry_origin_clause(gathered: &Gathered) -> String {
    match (
        gathered.registry.origin_label.as_deref(),
        gathered.registry.path.as_deref(),
    ) {
        (Some(label), Some(path)) => format!(" (read from {label}: {path})"),
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "team_readiness_registry_tests.rs"]
pub(super) mod tests;
