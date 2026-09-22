//! The lookup order, and the refusal that names every place it looked.
//!
//! Every case here is one of the two live findings of 2026-09-20: a project
//! whose registry lives in its agents repository must route (ledger 178(a),
//! 179(b)), and a project with no registry at all must be told that in those
//! words rather than being told nothing cleared its class.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::*;
use crate::coding_sessions::workdir_store::{
    CodingSessionAgentsRepo, CodingSessionWorkdirScope, CodingSessionWorkdirStore,
};

const PROJECT: &str = "30621:aaaa:pivot-test";

/// A registry that parses: `version` and one row is all these tests need.
fn registry_text(marker: &str) -> String {
    format!("version: 1\nupdatedAt: \"2026-09-20\"\n# {marker}\n")
}

fn store(agents: Option<&Path>, checkout: Option<&Path>) -> CodingSessionWorkdirStore {
    let mut store = CodingSessionWorkdirStore::default();
    if let Some(path) = agents {
        store.agents_repos = BTreeMap::from([(
            PROJECT.to_owned(),
            CodingSessionAgentsRepo {
                url: None,
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

/// The whole of ledger 178(a): a project whose registry is in its agents
/// repository has a registry.
#[test]
fn the_agents_repository_answers_first_and_says_so() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("pivot-test-beekeeper-agents");
    let checkout = tmp.path().join("pivot-test");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(checkout.join("team")).expect("checkout dir");
    fs::write(agents.join("model-registry.yaml"), registry_text("agents")).expect("write");
    fs::write(
        checkout.join("team/model-registry.yaml"),
        registry_text("checkout"),
    )
    .expect("write");

    let store = store(Some(&agents), Some(&checkout));
    let read = resolve_host_model_registry(&host_model_registry_candidates(&store, PROJECT))
        .expect("a registry");
    assert_eq!(read.origin, "agents-repo");
    assert_eq!(read.origin_label, "the project's agents repository");
    assert!(read.text.contains("# agents"), "{}", read.text);
    assert!(
        read.path
            .ends_with("pivot-test-beekeeper-agents/model-registry.yaml"),
        "{}",
        read.path
    );
    // The pinned ref rides along in the disclosure, so a reader can tell
    // which snapshot answered.
    assert_eq!(read.looked_in.len(), 1);
    assert!(
        read.looked_in[0].contains("refs/heads/main"),
        "{:?}",
        read.looked_in
    );
}

/// Beekeeper's own project keeps working: the checkout rung still answers.
#[test]
fn the_checkout_answers_when_the_agents_repository_holds_none() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("beekeeper-beekeeper-agents");
    let checkout = tmp.path().join("beekeeper");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(checkout.join("team")).expect("checkout dir");
    fs::write(
        checkout.join("team/model-registry.yaml"),
        registry_text("checkout"),
    )
    .expect("write");

    let store = store(Some(&agents), Some(&checkout));
    let read = resolve_host_model_registry(&host_model_registry_candidates(&store, PROJECT))
        .expect("a registry");
    assert_eq!(read.origin, "checkout");
    assert!(read.text.contains("# checkout"), "{}", read.text);
    // Both places are disclosed, and the one that had nothing says why.
    assert_eq!(read.looked_in.len(), 2);
    assert!(
        read.looked_in[0].contains("file-missing"),
        "{:?}",
        read.looked_in
    );
}

/// Ledger 178(a) again, from the other side: the refusal must name the files
/// and must not be the sentence about a class nothing clears.
#[test]
fn no_registry_anywhere_names_both_files() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("pivot-test-beekeeper-agents");
    let checkout = tmp.path().join("pivot-test");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(&checkout).expect("checkout dir");

    let store = store(Some(&agents), Some(&checkout));
    let missing = resolve_host_model_registry(&host_model_registry_candidates(&store, PROJECT))
        .expect_err("no registry");
    let sentence = missing.to_string();
    assert!(
        sentence.starts_with("no model registry: looked in "),
        "{sentence}"
    );
    assert!(
        sentence.contains("pivot-test-beekeeper-agents"),
        "{sentence}"
    );
    assert!(sentence.contains("team/model-registry.yaml"), "{sentence}");
    assert!(!sentence.contains("risk tier"), "{sentence}");
    assert!(!sentence.contains("nothing offered clears"), "{sentence}");
}

/// A project this computer has no record of is told that, not told there is
/// no registry in a place it never had.
#[test]
fn a_project_with_no_recorded_directory_has_nowhere_to_look() {
    let store = CodingSessionWorkdirStore::default();
    let candidates = host_model_registry_candidates(&store, PROJECT);
    assert!(candidates.is_empty());
    let missing = resolve_host_model_registry(&candidates).expect_err("nowhere");
    assert!(missing.looked_in.is_empty());
    assert!(
        missing.to_string().contains("no place to look"),
        "{missing}"
    );
}

/// A registry planted behind a symlink out of the repository is not read:
/// the guards in `project_files` still apply, and the next rung still gets
/// its turn.
#[cfg(unix)]
#[test]
fn a_symlinked_registry_is_skipped_and_the_next_rung_answers() {
    use std::os::unix::fs as unix_fs;

    let tmp = tempfile::tempdir().expect("tmp");
    let secret = tmp.path().join("secrets.yaml");
    fs::write(&secret, registry_text("secret")).expect("write");
    let agents = tmp.path().join("pivot-test-beekeeper-agents");
    let checkout = tmp.path().join("pivot-test");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::create_dir_all(checkout.join("team")).expect("checkout dir");
    unix_fs::symlink(&secret, agents.join("model-registry.yaml")).expect("symlink");
    fs::write(
        checkout.join("team/model-registry.yaml"),
        registry_text("checkout"),
    )
    .expect("write");

    let store = store(Some(&agents), Some(&checkout));
    let read = resolve_host_model_registry(&host_model_registry_candidates(&store, PROJECT))
        .expect("a registry");
    assert_eq!(read.origin, "checkout");
    assert!(read.text.contains("# checkout"), "{}", read.text);
    assert!(
        read.looked_in[0].contains("outside-checkout"),
        "{:?}",
        read.looked_in
    );
}

/// The name the seed writes and the name every reader composes are the same
/// string. This crate is the only place that can see both.
#[test]
fn the_seeded_file_name_is_the_name_readers_look_for() {
    assert_eq!(
        buzz_persona_pkg::seed::MODEL_REGISTRY_YML,
        buzz_core_pkg::model_registry_source::AGENTS_REPO_REGISTRY_FILE
    );
}

/// The hints an unrouted hire runs on, read from the same snapshot as the
/// registry (ledger 179(b), 180).
#[test]
fn team_yml_hints_are_read_from_the_agents_repository_and_list_every_role() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join("pivot-test-beekeeper-agents");
    fs::create_dir_all(&agents).expect("agents dir");
    fs::write(
        agents.join("team.yml"),
        concat!(
            "schema: beekeeper-team/v1\n",
            "version: 0.1.0\n",
            "lead: lead\n",
            "roles:\n",
            "  lead: {}\n",
            "  builder:\n",
            "    runtime: claude\n",
            "    model: anthropic:claude-sonnet-5\n",
        ),
    )
    .expect("write team.yml");

    let hints = super::team_role_hints(
        &fs::read_to_string(agents.join("team.yml")).expect("read"),
        &agents.join("team.yml"),
    );
    // Every role the manifest lists, hint or not: "the team says nothing
    // about this role" and "this host never read the team" are different
    // facts, and only the second one may fall back silently.
    assert_eq!(
        hints.keys().cloned().collect::<Vec<_>>(),
        vec!["builder".to_owned(), "lead".to_owned()]
    );
    let builder = &hints["builder"];
    assert_eq!(builder.runtime.as_deref(), Some("claude"));
    assert_eq!(builder.model.as_deref(), Some("anthropic:claude-sonnet-5"));
    assert_eq!(hints["lead"].runtime, None);
    assert_eq!(hints["lead"].model, None);
}

/// A `team.yml` the parser refuses is no hints, not a refused hire: the file
/// is the project's to edit and the hire never mentioned it.
#[test]
fn a_team_manifest_that_does_not_parse_is_no_hints_rather_than_a_refusal() {
    let hints = super::team_role_hints("schema: something-else/v9\n", Path::new("/x/team.yml"));
    assert!(hints.is_empty());
}
