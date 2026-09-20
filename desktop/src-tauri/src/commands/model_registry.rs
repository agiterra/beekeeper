//! Finding a project's model registry on this computer, in order, and saying
//! which copy answered.
//!
//! # Why the order exists
//!
//! The hire host routes a `session.hire` against the shared registry. It used
//! to read exactly one file — `team/model-registry.yaml` in the project's
//! **code** checkout ([`super::project_files`]) — and only Beekeeper's own
//! repository has that file. So on the live Pivot Test run of 2026-09-20 a
//! routed hire (`class builder`, `risk 2,2,2`) was refused `HIRE_NO_ROUTE —
//! registry not readable on this host`, the retry ten seconds later was
//! unrouted, and the seat ran the identity's own pin, `opus[1m]` — the most
//! expensive target on offer. Andy's eight-seat run the day before did the
//! same thing seven times over (ledger 178(a), 179(b)).
//!
//! Under the agents-repository pivot the project's team lives in
//! `<slug>-beekeeper-agents` (spec § 4.11) and its seed now writes
//! `model-registry.yaml` at that repository's root. This module is the host
//! side of that: it looks in the agents repository first, then in the code
//! checkout, and when neither holds one it refuses with a sentence naming
//! both files.
//!
//! # What it will not do
//!
//! - **It will not route on a copy compiled into the app.** A registry the
//!   operator cannot open is a hardcoded opinion wearing a registry's name.
//!   Every answer here names an absolute path that exists on this disk.
//! - **It will not reach the network.** The agents repository is read from
//!   the snapshot this host already has — the clone the seat's role pack was
//!   staged from — so reading the registry cannot make a hire wait on the
//!   relay, and cannot answer from a commit no seat is running.
//! - **It will not guess.** A project with no recorded agents repository and
//!   no recorded checkout is told exactly that, with the project coordinate
//!   in the sentence.
//!
//! The file reads go through [`super::project_files`]'s guards —
//! allowlisted name, containment after symlink resolution, a size ceiling,
//! UTF-8 or nothing — because a checkout is a person's disk.

use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use buzz_core_pkg::model_registry_source::{
    MissingModelRegistry, ModelRegistryCandidate, AGENTS_REPO_REGISTRY_FILE,
};

use super::project_files::{
    read_allowlisted_project_file, ProjectFileRefusal, MODEL_REGISTRY_RELATIVE_PATH,
};
use crate::coding_sessions::workdir_store::{load_workdir_store, CodingSessionWorkdirStore};

/// The refusal code a project with no registry anywhere earns.
///
/// Distinct from every `read_project_file` code on purpose: those are facts
/// about one path, and this is the answer to "where is the registry" — the
/// only one whose remedy is to seed or write a file.
pub const NO_MODEL_REGISTRY_CODE: &str = "no-model-registry";

/// One place this host will look, and the root it is under.
///
/// The root travels with the candidate because the read is guarded relative
/// to it: [`read_allowlisted_project_file`] canonicalizes the root and
/// refuses anything that resolves outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostModelRegistryCandidate {
    /// The place, as every reader names it (`buzz_core`).
    pub candidate: ModelRegistryCandidate,
    /// The directory the file is read relative to.
    pub root: PathBuf,
    /// The allowlisted relative path under [`Self::root`].
    pub relative: &'static str,
}

/// The registry this host found, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRegistryRead {
    /// The absolute, symlink-resolved path the bytes came from.
    pub path: String,
    /// The file's contents, as UTF-8. Unparsed: the parse refusal belongs to
    /// the router's own parser, which names the file.
    pub text: String,
    /// `agents-repo` or `checkout` — which copy answered.
    pub origin: String,
    /// That origin as a clause a sentence can carry ("the project's agents
    /// repository").
    pub origin_label: String,
    /// Every place that was looked at, in order, up to and including the one
    /// that answered — so a disclosure can say what was skipped rather than
    /// implying there was only ever one place.
    pub looked_in: Vec<String>,
}

/// Where this host will look for `project_ref`'s registry, in order.
///
/// Both rungs come from this computer's own records, never from a caller:
///
/// 1. `agents_repos[project_ref]` — the clone of `<slug>-beekeeper-agents`
///    this host keeps, recorded when the repository is created here, when a
///    seat is staged from it, or when the Actions tab reads the project's
///    source.
/// 2. `by_project[project_ref]` — the code checkout, the rung that existed
///    before this change and the one Beekeeper's own project still answers
///    from.
///
/// A rung with no record contributes no candidate: a path this host does not
/// have is not a place it looked.
pub(crate) fn host_model_registry_candidates(
    store: &CodingSessionWorkdirStore,
    project_ref: &str,
) -> Vec<HostModelRegistryCandidate> {
    let mut candidates = Vec::with_capacity(2);
    if let Some(agents) = store.agents_repos.get(project_ref) {
        let label = format!("{} ({})", agents.path.display(), agents.ref_name);
        candidates.push(HostModelRegistryCandidate {
            candidate: ModelRegistryCandidate::agents_repo(&agents.path, Some(&label)),
            root: agents.path.clone(),
            relative: AGENTS_REPO_REGISTRY_FILE,
        });
    }
    if let Some(checkout) = store.by_project.get(project_ref) {
        candidates.push(HostModelRegistryCandidate {
            candidate: ModelRegistryCandidate::checkout(&checkout.path),
            root: checkout.path.clone(),
            relative: MODEL_REGISTRY_RELATIVE_PATH,
        });
    }
    candidates
}

/// Read the first candidate that holds a registry, or say where it looked.
///
/// A candidate whose file cannot be read — absent, a directory, outside its
/// root, too large, not UTF-8 — is skipped rather than fatal: the next rung
/// is the whole point of an order. Its own sentence is not lost, though: it
/// rides along in `looked_in`, so a reader can tell "there is no file there"
/// from "there is a file there and this host would not read it".
///
/// # Errors
///
/// [`MissingModelRegistry`], whose `Display` names every place that was
/// looked at, in order.
pub(crate) fn resolve_host_model_registry(
    candidates: &[HostModelRegistryCandidate],
) -> Result<ModelRegistryRead, MissingModelRegistry> {
    let mut looked_in = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match read_allowlisted_project_file(&candidate.root, candidate.relative) {
            Ok(read) => {
                looked_in.push(candidate.candidate.display.clone());
                return Ok(ModelRegistryRead {
                    path: read.path,
                    text: read.text,
                    origin: candidate.candidate.origin.as_str().to_owned(),
                    origin_label: candidate.candidate.origin.describe().to_owned(),
                    looked_in,
                });
            }
            Err(refusal) => looked_in.push(format!(
                "{} ({})",
                candidate.candidate.display, refusal.code
            )),
        }
    }
    Err(MissingModelRegistry { looked_in })
}

/// Read a project's model registry from this computer, in the order above.
///
/// # Errors
///
/// A [`ProjectFileRefusal`] coded [`NO_MODEL_REGISTRY_CODE`] when no copy was
/// found — its message names every place that was tried — or when no project
/// was named at all.
#[tauri::command]
pub fn read_model_registry(
    app: AppHandle,
    project_ref: String,
) -> Result<ModelRegistryRead, ProjectFileRefusal> {
    let project_ref = project_ref.trim().to_owned();
    if project_ref.is_empty() {
        return Err(ProjectFileRefusal {
            code: NO_MODEL_REGISTRY_CODE,
            message: "no project was named, so there is nowhere to look for a model registry"
                .to_owned(),
        });
    }
    let store = load_workdir_store(&app).map_err(|error| ProjectFileRefusal {
        code: NO_MODEL_REGISTRY_CODE,
        message: format!("this computer's project directory record could not be read: {error}"),
    })?;
    let candidates = host_model_registry_candidates(&store, &project_ref);
    resolve_host_model_registry(&candidates).map_err(|missing| ProjectFileRefusal {
        code: NO_MODEL_REGISTRY_CODE,
        message: if missing.looked_in.is_empty() {
            format!(
                "no model registry: this computer has recorded neither an agents repository nor \
                 a checkout for project {project_ref}, so there is nowhere to look. Finish the \
                 project's repository setup, or choose its folder."
            )
        } else {
            format!(
                "{missing}. Seed the project's agents repository with \
                 {AGENTS_REPO_REGISTRY_FILE}, or hire without routing."
            )
        },
    })
}

#[cfg(test)]
#[path = "model_registry_tests.rs"]
mod tests;
