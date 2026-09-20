//! Where the model registry is looked for, in order, and which copy answered.
//!
//! # Why this module exists
//!
//! The registry (`team/model-registry.yaml`, Brian's ruling of 2026-08-30) is
//! what `bee sessions route` and the desktop's hire host both read to choose
//! an execution target. Until 2026-09-20 every reader looked in exactly one
//! place: `team/model-registry.yaml` inside the project's **code** checkout.
//! Only this repository has that file, so on the live Pivot Test run a routed
//! hire was refused `HIRE_NO_ROUTE — registry not readable on this host`, the
//! retry ten seconds later was unrouted, and the seat ran the identity's own
//! pin — the most expensive target on offer (ledger 178(a), 179(b)).
//!
//! Under the agents-repository pivot a project's team lives in
//! `<slug>-beekeeper-agents` beside `team.yml` (spec § 4.11), so the registry
//! must travel with the project: the agents repository seeds
//! [`AGENTS_REPO_REGISTRY_FILE`] at its root
//! (`buzz_persona::seed::write_agents_repo_seed`).
//!
//! # The lookup order, and why a refusal names every place it looked
//!
//! 1. The project's **agents repository** — the snapshot the seat's role pack
//!    was staged from, at the commit it was staged at.
//! 2. `team/model-registry.yaml` in the project's **code checkout** — what
//!    this repository has, kept so Beekeeper's own routing is unchanged.
//! 3. Nothing. That is [`MissingModelRegistry`], whose sentence names both
//!    paths, because a reader told only "no registry" cannot tell an absent
//!    file from a registry that gated every candidate out — the two have
//!    opposite remedies, and conflating them is what sent the live lead
//!    looking for a routing bug that was not there.
//!
//! Both halves of the order live here so the CLI and the desktop host cannot
//! drift: a host that read the agents repository while `bee sessions route`
//! read only the checkout would print one answer and seat another.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::coding_session_routing::DEFAULT_REGISTRY_RELATIVE_PATH;

/// The registry's file name at an agents repository's root.
///
/// Beside `team.yml`, not under `team/`: the agents repository *is* the
/// project's team, so a `team/` directory inside it would be a second,
/// competing home for the same thing.
pub const AGENTS_REPO_REGISTRY_FILE: &str = "model-registry.yaml";

/// The suffix a seat's clone of the agents repository carries beside its
/// worktree (`<worktree>-agents`).
///
/// Defined here because two readers compose it: the desktop host, which cuts
/// the clone, and `bee`, which runs *inside* the seat's worktree and finds the
/// registry by looking at that sibling.
pub const SEAT_AGENTS_CLONE_SUFFIX: &str = "-agents";

/// Which copy of the registry a reader used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelRegistryOrigin {
    /// The project's agents repository (spec § 4.11) — its snapshot on this
    /// computer, at the commit the seat's role pack was staged from.
    AgentsRepo,
    /// `team/model-registry.yaml` in the project's code checkout.
    Checkout,
    /// A path the caller named outright (`--registry <path>`).
    Explicit,
}

impl ModelRegistryOrigin {
    /// The stable token this origin is disclosed as.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentsRepo => "agents-repo",
            Self::Checkout => "checkout",
            Self::Explicit => "explicit",
        }
    }

    /// One clause naming where the bytes came from, for a disclosure line.
    pub fn describe(self) -> &'static str {
        match self {
            Self::AgentsRepo => "the project's agents repository",
            Self::Checkout => "the project's code checkout",
            Self::Explicit => "the path this run was given",
        }
    }
}

impl fmt::Display for ModelRegistryOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One place a reader will look for the registry.
///
/// `display` is what a refusal prints. It is carried rather than derived so an
/// agents repository can be named the way a person recognizes it —
/// `<repo>:model-registry.yaml` — while the filesystem path stays exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRegistryCandidate {
    /// Which copy this candidate would be.
    pub origin: ModelRegistryOrigin,
    /// How this place is named in a disclosure or a refusal.
    pub display: String,
    /// The file to read.
    pub path: PathBuf,
}

impl ModelRegistryCandidate {
    /// `<root>/model-registry.yaml` in an agents repository.
    ///
    /// `label` names the repository as a person would recognize it (its
    /// coordinate, or its directory); with `None` the path is used.
    pub fn agents_repo(root: &Path, label: Option<&str>) -> Self {
        let path = root.join(AGENTS_REPO_REGISTRY_FILE);
        let display = match label {
            Some(label) => format!("{label}:{AGENTS_REPO_REGISTRY_FILE}"),
            None => path.display().to_string(),
        };
        Self {
            origin: ModelRegistryOrigin::AgentsRepo,
            display,
            path,
        }
    }

    /// `<root>/team/model-registry.yaml` in a code checkout.
    pub fn checkout(root: &Path) -> Self {
        let path = root.join(DEFAULT_REGISTRY_RELATIVE_PATH);
        Self {
            origin: ModelRegistryOrigin::Checkout,
            display: path.display().to_string(),
            path,
        }
    }

    /// A path a caller named outright.
    pub fn explicit(path: &Path) -> Self {
        Self {
            origin: ModelRegistryOrigin::Explicit,
            display: path.display().to_string(),
            path: path.to_path_buf(),
        }
    }
}

/// The registry a reader ended up with, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelRegistry {
    /// Which copy answered.
    pub origin: ModelRegistryOrigin,
    /// How that place is named in a disclosure.
    pub display: String,
    /// The file the bytes were read from.
    pub path: PathBuf,
    /// The file's contents, unparsed: this module resolves a *location*, and
    /// a parse refusal belongs to the parser, which names the file.
    pub text: String,
    /// Every place that was looked at, in order, up to and including the one
    /// that answered — so a disclosure can say what was skipped.
    pub looked_in: Vec<String>,
}

/// No copy of the registry was found.
///
/// Its sentence is deliberately *not* "nothing offered clears that class at
/// that risk tier": that sentence describes a registry that exists and gated
/// every candidate out, and the remedy for it is a different class or a
/// different risk. The remedy for this one is a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingModelRegistry {
    /// Every place that was looked at, in order.
    pub looked_in: Vec<String>,
}

impl fmt::Display for MissingModelRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.looked_in.is_empty() {
            return f.write_str(
                "no model registry: this computer knows no place to look for one — \
                 no agents repository and no checkout is recorded for the project",
            );
        }
        write!(
            f,
            "no model registry: looked in {}",
            join_and(&self.looked_in)
        )
    }
}

impl std::error::Error for MissingModelRegistry {}

/// `a`, `a and b`, `a, b and c` — the way a sentence lists places.
pub fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Read the first candidate that holds a readable file, or say where it looked.
///
/// A candidate whose file cannot be read — absent, a directory, a broken
/// symlink, not UTF-8 — is skipped rather than fatal: the next rung is the
/// point of an order. Every candidate that was considered is reported either
/// way.
///
/// # Errors
///
/// [`MissingModelRegistry`] naming every place that was looked at, in order.
pub fn resolve_model_registry(
    candidates: &[ModelRegistryCandidate],
) -> Result<ResolvedModelRegistry, MissingModelRegistry> {
    resolve_model_registry_with(candidates, |path| std::fs::read_to_string(path).ok())
}

/// [`resolve_model_registry`] with the read injected, so the order is testable
/// without a filesystem.
///
/// # Errors
///
/// [`MissingModelRegistry`] naming every place that was looked at, in order.
pub fn resolve_model_registry_with(
    candidates: &[ModelRegistryCandidate],
    mut read: impl FnMut(&Path) -> Option<String>,
) -> Result<ResolvedModelRegistry, MissingModelRegistry> {
    let mut looked_in = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        looked_in.push(candidate.display.clone());
        if let Some(text) = read(&candidate.path) {
            return Ok(ResolvedModelRegistry {
                origin: candidate.origin,
                display: candidate.display.clone(),
                path: candidate.path.clone(),
                text,
                looked_in,
            });
        }
    }
    Err(MissingModelRegistry { looked_in })
}

/// Where a seat's clone of the agents repository sits beside its worktree.
///
/// `None` when the worktree path has no final component to put a sibling
/// beside — a root directory is not a seat's tree.
pub fn seat_agents_clone_path(worktree: &Path) -> Option<PathBuf> {
    let name = worktree.file_name()?.to_str()?;
    Some(worktree.with_file_name(format!("{name}{SEAT_AGENTS_CLONE_SUFFIX}")))
}

/// The ordered candidates a reader standing in a directory should try.
///
/// For each ancestor of `start`, in order:
///
/// 1. `<ancestor>/model-registry.yaml` — the ancestor *is* an agents
///    repository (a seat's agents clone, or a clone opened by hand).
/// 2. `<ancestor>-agents/model-registry.yaml` — the sibling clone the host
///    cuts beside a seat's worktree.
/// 3. `<ancestor>/team/model-registry.yaml` — a code checkout.
///
/// The ancestor's own root is tried before its sibling so a person standing
/// inside an agents repository reads the file they are looking at.
pub fn ancestor_model_registry_candidates(start: &Path) -> Vec<ModelRegistryCandidate> {
    let mut candidates = Vec::new();
    for ancestor in start.ancestors() {
        candidates.push(ModelRegistryCandidate::agents_repo(ancestor, None));
        if let Some(sibling) = seat_agents_clone_path(ancestor) {
            candidates.push(ModelRegistryCandidate::agents_repo(&sibling, None));
        }
        candidates.push(ModelRegistryCandidate::checkout(ancestor));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<ModelRegistryCandidate> {
        vec![
            ModelRegistryCandidate::agents_repo(
                Path::new("/cache/abc-demo-beekeeper-agents"),
                Some("30617:abc:demo-beekeeper-agents"),
            ),
            ModelRegistryCandidate::checkout(Path::new("/src/demo")),
        ]
    }

    #[test]
    fn the_agents_repository_answers_before_the_checkout() {
        let resolved = resolve_model_registry_with(&candidates(), |path| {
            (path == Path::new("/cache/abc-demo-beekeeper-agents/model-registry.yaml"))
                .then(|| "version: 1\n".to_owned())
        })
        .expect("resolved");
        assert_eq!(resolved.origin, ModelRegistryOrigin::AgentsRepo);
        assert_eq!(resolved.origin.as_str(), "agents-repo");
        assert_eq!(
            resolved.display,
            "30617:abc:demo-beekeeper-agents:model-registry.yaml"
        );
        assert_eq!(resolved.text, "version: 1\n");
        // Only the place that answered was looked at: the checkout was never
        // opened, and the disclosure says exactly that.
        assert_eq!(
            resolved.looked_in,
            vec!["30617:abc:demo-beekeeper-agents:model-registry.yaml".to_owned()]
        );
    }

    #[test]
    fn the_checkout_still_answers_when_the_agents_repository_has_none() {
        let resolved = resolve_model_registry_with(&candidates(), |path| {
            (path == Path::new("/src/demo/team/model-registry.yaml"))
                .then(|| "version: 1\n".to_owned())
        })
        .expect("resolved");
        assert_eq!(resolved.origin, ModelRegistryOrigin::Checkout);
        assert_eq!(
            resolved.path,
            Path::new("/src/demo/team/model-registry.yaml")
        );
        assert_eq!(resolved.looked_in.len(), 2);
    }

    /// Ledger 178(a): the refusal must name the files, and must not be the
    /// sentence about a class nothing clears.
    #[test]
    fn no_registry_anywhere_names_both_places_and_blames_neither_the_class_nor_the_tier() {
        let missing = resolve_model_registry_with(&candidates(), |_| None).expect_err("missing");
        assert_eq!(
            missing.to_string(),
            "no model registry: looked in \
             30617:abc:demo-beekeeper-agents:model-registry.yaml and \
             /src/demo/team/model-registry.yaml"
        );
        assert!(!missing.to_string().contains("risk tier"));
    }

    #[test]
    fn with_nowhere_to_look_the_refusal_says_so_rather_than_naming_nothing() {
        let missing = resolve_model_registry_with(&[], |_| None).expect_err("missing");
        assert!(
            missing.to_string().contains("no place to look"),
            "{missing}"
        );
    }

    #[test]
    fn a_seats_agents_clone_is_its_worktrees_sibling() {
        assert_eq!(
            seat_agents_clone_path(Path::new("/w/trees/seat-1")),
            Some(PathBuf::from("/w/trees/seat-1-agents"))
        );
        assert_eq!(seat_agents_clone_path(Path::new("/")), None);
    }

    #[test]
    fn the_ancestor_walk_tries_the_agents_repository_the_sibling_clone_then_the_checkout() {
        let candidates = ancestor_model_registry_candidates(Path::new("/w/seat-1/crates"));
        let displays: Vec<&str> = candidates.iter().map(|c| c.display.as_str()).collect();
        assert_eq!(
            displays,
            vec![
                "/w/seat-1/crates/model-registry.yaml",
                "/w/seat-1/crates-agents/model-registry.yaml",
                "/w/seat-1/crates/team/model-registry.yaml",
                "/w/seat-1/model-registry.yaml",
                "/w/seat-1-agents/model-registry.yaml",
                "/w/seat-1/team/model-registry.yaml",
                "/w/model-registry.yaml",
                "/w-agents/model-registry.yaml",
                "/w/team/model-registry.yaml",
                "/model-registry.yaml",
                "/team/model-registry.yaml",
            ]
        );
    }

    #[test]
    fn places_are_listed_the_way_a_sentence_lists_them() {
        assert_eq!(join_and(&[]), "");
        assert_eq!(join_and(&["a".to_owned()]), "a");
        assert_eq!(join_and(&["a".to_owned(), "b".to_owned()]), "a and b");
        assert_eq!(
            join_and(&["a".to_owned(), "b".to_owned(), "c".to_owned()]),
            "a, b and c"
        );
    }
}
