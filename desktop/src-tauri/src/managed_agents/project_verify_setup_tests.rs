//! Ledger 248: what project setup publishes for the seeded `verify` action.

use super::project_verify_setup::{plan_verify_publication, run_id_from_trigger_message};

const PROJECT: &str =
    "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

/// The publication lands on the id `bee actions publish` uses and carries the
/// hash the relay will store, so the grant collected at setup binds the
/// definition every later run of the unchanged action names.
#[test]
fn setup_publishes_the_seeded_verify_under_the_shared_id_and_relay_hash() {
    let command: Vec<String> = ["cargo", "test", "--workspace"].map(str::to_owned).to_vec();
    let yml = buzz_persona_pkg::seed::seeded_actions_yml_with_verify(&command);
    let plan = plan_verify_publication(PROJECT, &yml).expect("plan");
    assert_eq!(
        plan.workflow_id,
        buzz_workflow_pkg::actions_file::action_workflow_id(PROJECT, "verify")
    );
    assert_eq!(plan.command, command);
    let (def, _) = buzz_workflow_pkg::WorkflowEngine::parse_yaml(&plan.yaml).expect("relay parse");
    assert_eq!(
        buzz_workflow_pkg::hash::definition_hash_hex(&def).expect("hash"),
        plan.definition_hash,
        "the relay hashes the published YAML to the hash setup shows"
    );
    assert_eq!(def.project.as_deref(), Some(PROJECT));
}

/// A file with no `verify`, or one whose `verify` runs nothing on a host, is
/// refused with words rather than publishing something no grant can cover.
#[test]
fn setup_refuses_a_file_without_a_host_verify() {
    let yml = "schema: buzz-project-actions/v1\nactions:\n  - name: other\n    trigger:\n      on: manual\n    steps:\n      - id: a\n        action: run_on_host\n        command: [\"true\"]\n";
    let error = plan_verify_publication(PROJECT, yml).expect_err("no verify");
    assert!(error.contains("verify"), "{error}");
}

#[test]
fn the_run_id_is_read_from_the_trigger_response() {
    let id = "5d6f1f7e-1c1b-4a53-9a8e-3a6f2d2b7c10";
    assert_eq!(
        run_id_from_trigger_message(&format!("response:{{\"run_id\":\"{id}\"}}")),
        Some(id.to_string())
    );
    assert_eq!(run_id_from_trigger_message("accepted"), None);
}
