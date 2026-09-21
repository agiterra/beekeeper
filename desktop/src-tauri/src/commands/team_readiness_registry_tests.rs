//! Ledger 207(1): readiness reads the registry the hire host reads.
//!
//! The live defect these pin: "Kettle Smoke" was told
//! `This project pins no provider or model targets (file-missing:
//! …/kettle-smoke/team/model-registry.yaml)` with the remedy "Add
//! team/model-registry.yaml to the checkout", while its registry sat at the
//! root of `kettle-smoke-beekeeper-agents` and routed a hire minutes later.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::*;
use crate::coding_sessions::workdir_store::{
    CodingSessionAgentsRepo, CodingSessionWorkdirScope, CodingSessionWorkdirStore,
};

const PROJECT: &str = "30621:aaaa:kettle-smoke";

/// A store that records only the code checkout — the pre-agents-repository
/// layout the ledger-137 cases below were written against. Shared with the
/// packs tests, which need the same fixture.
pub(crate) fn checkout_only_store(project_ref: &str, path: &Path) -> CodingSessionWorkdirStore {
    let mut store = CodingSessionWorkdirStore::default();
    store.set(
        CodingSessionWorkdirScope::Project,
        project_ref,
        path.to_path_buf(),
    );
    store
}

fn registry_text(marker: &str) -> String {
    format!("version: 1\nupdatedAt: \"2026-09-20\"\n# {marker}\n")
}

fn store(agents: Option<&Path>, checkout: Option<&Path>) -> CodingSessionWorkdirStore {
    let mut store = CodingSessionWorkdirStore::default();
    if let Some(path) = agents {
        store.agents_repos = BTreeMap::from([(
            PROJECT.to_owned(),
            CodingSessionAgentsRepo {
                path: path.to_path_buf(),
                ref_name: "refs/heads/main".to_owned(),
                updated_at: "2026-09-20T00:00:00Z".to_owned(),
            },
        )]);
    }
    if let Some(path) = checkout {
        store.set(
            CodingSessionWorkdirScope::Project,
            PROJECT,
            path.to_path_buf(),
        );
    }
    store
}

/// The defect itself: an agents-repository registry is found, not missed.
#[test]
fn the_agents_repository_registry_is_read_and_named() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("kettle-smoke-beekeeper-agents");
    let checkout = tmp.path().join("kettle-smoke");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(&checkout).expect("checkout dir");
    fs::write(agents.join("model-registry.yaml"), registry_text("agents")).expect("write");

    let store = store(Some(&agents), Some(&checkout));
    let mut gathered = Gathered::default();
    let text = resolve_registry_text(Some(&store), PROJECT, true, &mut gathered)
        .expect("the agents repository answers");

    assert!(text.contains("# agents"), "{text}");
    assert_eq!(gathered.registry.origin.as_deref(), Some("agents-repo"));
    assert_eq!(
        gathered.registry.origin_label.as_deref(),
        Some("the project's agents repository")
    );
    assert!(
        gathered
            .facts
            .iter()
            .all(|fact| fact.code != "REGISTRY_UNREADABLE"),
        "no REGISTRY_UNREADABLE over a registry that exists: {:?}",
        gathered.facts
    );
    assert!(
        registry_origin_clause(&gathered).contains("the project's agents repository"),
        "{}",
        registry_origin_clause(&gathered)
    );
}

/// The checkout is still the second rung, and still says which copy answered.
#[test]
fn the_checkout_answers_when_the_agents_repository_holds_none() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("kettle-smoke-beekeeper-agents");
    let checkout = tmp.path().join("kettle-smoke");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(checkout.join("team")).expect("checkout dir");
    fs::write(
        checkout.join("team/model-registry.yaml"),
        registry_text("checkout"),
    )
    .expect("write");

    let store = store(Some(&agents), Some(&checkout));
    let mut gathered = Gathered::default();
    let text =
        resolve_registry_text(Some(&store), PROJECT, true, &mut gathered).expect("the checkout");
    assert!(text.contains("# checkout"), "{text}");
    assert_eq!(gathered.registry.origin.as_deref(), Some("checkout"));
    assert_eq!(
        gathered.registry.looked_in.len(),
        2,
        "the skipped agents repository is still disclosed: {:?}",
        gathered.registry.looked_in
    );
}

/// With neither copy, the fact names both places and the remedy matches the
/// layout this project actually has.
#[test]
fn an_absent_registry_names_both_places_and_the_agents_repository_remedy() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("kettle-smoke-beekeeper-agents");
    let checkout = tmp.path().join("kettle-smoke");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(&checkout).expect("checkout dir");

    let store = store(Some(&agents), Some(&checkout));
    let mut gathered = Gathered::default();
    assert!(resolve_registry_text(Some(&store), PROJECT, true, &mut gathered).is_none());

    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("the absence is disclosed");
    assert!(
        fact.summary.contains("kettle-smoke-beekeeper-agents")
            && fact.summary.contains("team/model-registry.yaml"),
        "both places are named: {}",
        fact.summary
    );
    let remedy = fact.remedy.clone().expect("a remedy");
    assert!(
        remedy.contains("agents repository") && !remedy.contains("to the checkout"),
        "a project with an agents repository is not told to edit its checkout: {remedy}"
    );
}

/// No agents repository recorded: the old checkout advice is still the right
/// advice, and is still given.
#[test]
fn a_checkout_only_project_keeps_the_checkout_remedy() {
    let tmp = tempfile::tempdir().expect("tmp");
    let checkout = tmp.path().join("beekeeper");
    fs::create_dir_all(&checkout).expect("checkout dir");

    let store = store(None, Some(&checkout));
    let mut gathered = Gathered::default();
    assert!(resolve_registry_text(Some(&store), PROJECT, false, &mut gathered).is_none());
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("the absence is disclosed");
    assert_eq!(fact.state, TeamReadinessFactState::Blocked);
    assert!(fact
        .remedy
        .as_deref()
        .expect("a remedy")
        .contains("team/model-registry.yaml to the checkout"));
}

/// Nothing recorded at all: the remedy is to create the repositories, not to
/// edit a file in a folder this computer does not have.
#[test]
fn a_project_with_no_records_is_told_to_create_its_repositories() {
    let store = CodingSessionWorkdirStore::default();
    let mut gathered = Gathered::default();
    assert!(resolve_registry_text(Some(&store), PROJECT, true, &mut gathered).is_none());
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("the absence is disclosed");
    assert!(fact
        .remedy
        .as_deref()
        .expect("a remedy")
        .contains("Finish repository setup"));
}

/// Ledger 137: a project whose roles are staged from a kind:30624 packs
/// repository never reads a registry at all, so its absence is a limit on
/// what readiness can say, not a reason to refuse the session.
#[test]
fn registry_unreadable_is_limited_when_packs_come_from_a_project_source() {
    let temp = tempfile::tempdir().expect("tempdir");
    let host = crate::commands::team_readiness::tests::CountingHost::default();
    let store = checkout_only_store(PROJECT, temp.path());
    let mut gathered = Gathered::default();
    crate::commands::team_readiness::collect_runtimes_and_registry(
        &host,
        Some(&store),
        PROJECT,
        true,
        &mut gathered,
    );
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("registry fact is present for an absent registry file");
    assert_eq!(fact.state, TeamReadinessFactState::Limited);
    assert!(
        fact.summary
            .contains("each role pack names its own runtime and model instead"),
        "{}",
        fact.summary
    );
}

/// The counterpart: a project with no packs repository still routes every
/// session through a registry, so its absence stays a hard blocker.
#[test]
fn registry_unreadable_still_blocks_without_a_project_pack_source() {
    let temp = tempfile::tempdir().expect("tempdir");
    let host = crate::commands::team_readiness::tests::CountingHost::default();
    let store = checkout_only_store(PROJECT, temp.path());
    let mut gathered = Gathered::default();
    crate::commands::team_readiness::collect_runtimes_and_registry(
        &host,
        Some(&store),
        PROJECT,
        false,
        &mut gathered,
    );
    assert_eq!(
        gathered
            .facts
            .iter()
            .find(|fact| fact.code == "REGISTRY_UNREADABLE")
            .map(|fact| fact.state),
        Some(TeamReadinessFactState::Blocked)
    );
}
