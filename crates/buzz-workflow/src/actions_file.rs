//! `actions.yml` at the root of the project's agents repository (spec
//! § 4.11; `beekeeper/actions.yml` in the code repository before 2026-09-18):
//! a project's actions, each an ordinary workflow
//! definition (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 5.1).
//!
//! The file is parsed in two places that must agree: `bee actions publish`
//! turns each entry into one kind:30620 definition, and a host that receives a
//! kind:46013 request recompiles the named entry from its own checkout and
//! compares [`crate::hash::definition_hash_hex`] against the request. Both go
//! through [`parse_actions_yml`], which binds every entry to the project the
//! caller names, so the stored definition and the recompiled one carry the
//! same `project` and hash the same.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::hash::definition_hash_hex;
use crate::schema::{TriggerDef, WorkflowDef};

/// Exact schema the file must name.
pub const ACTIONS_SCHEMA: &str = "buzz-project-actions/v1";
/// Where the file lives, relative to the project checkout.
pub const ACTIONS_YML: &str = "actions.yml";
/// Ceiling on entries in one file.
pub const MAX_ACTIONS: usize = 64;

/// The file as authored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionsFile {
    /// Must equal [`ACTIONS_SCHEMA`].
    pub schema: String,
    /// IANA timezone every `schedule` below is written in, unless the
    /// schedule names its own. Applied to each entry before hashing, so the
    /// stored definition carries the zone explicitly.
    #[serde(default)]
    pub timezone: Option<String>,
    /// The actions, each a workflow definition.
    pub actions: Vec<WorkflowDef>,
}

/// One compiled entry: the definition bound to its project, and its hash.
#[derive(Debug, Clone)]
pub struct ActionEntry {
    /// The definition's name, unique within the file.
    pub name: String,
    /// The definition with `project` bound.
    pub def: WorkflowDef,
    /// The definition's canonical JSON, as the relay stores it.
    pub canonical_json: String,
    /// Lowercase hex SHA-256 of that JSON — what a kind:46013 carries.
    pub hash: String,
}

/// Parse and validate `actions.yml`, binding every entry to `project`.
///
/// An entry that names a different `project` is refused: the file belongs to
/// one project's repository and cannot act for another.
pub fn parse_actions_yml(text: &str, project: &str) -> Result<Vec<ActionEntry>, WorkflowError> {
    let normalized = buzz_core::kind::normalize_project_coordinate(project).ok_or_else(|| {
        WorkflowError::InvalidDefinition(
            "actions.yml project must be a full 30621:<64-hex>:<id> coordinate".into(),
        )
    })?;
    if normalized != project {
        return Err(WorkflowError::InvalidDefinition(
            "actions.yml project must use a lowercase canonical owner key".into(),
        ));
    }
    let mut document: serde_yaml::Value = serde_yaml::from_str(text)?;
    expand_wake_brief_sugar(&mut document)?;
    let file: ActionsFile = serde_yaml::from_value(document)?;
    if file.schema != ACTIONS_SCHEMA {
        return Err(WorkflowError::InvalidDefinition(format!(
            "actions.yml schema must be {ACTIONS_SCHEMA:?} (got {:?})",
            file.schema
        )));
    }
    let default_timezone = match &file.timezone {
        Some(timezone) => {
            crate::schema::parse_timezone(timezone).map_err(|error| {
                WorkflowError::InvalidDefinition(format!("actions.yml: {error}"))
            })?;
            Some(timezone.trim().to_owned())
        }
        None => None,
    };
    if file.actions.is_empty() {
        return Err(WorkflowError::InvalidDefinition(
            "actions.yml must list at least one action".into(),
        ));
    }
    if file.actions.len() > MAX_ACTIONS {
        return Err(WorkflowError::InvalidDefinition(format!(
            "actions.yml lists {} actions; at most {MAX_ACTIONS} are allowed",
            file.actions.len()
        )));
    }
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut entries = Vec::with_capacity(file.actions.len());
    for mut def in file.actions {
        let name = def.name.trim().to_owned();
        if name.is_empty() {
            return Err(WorkflowError::InvalidDefinition(
                "every action needs a non-empty name".into(),
            ));
        }
        if !names.insert(name.clone()) {
            return Err(WorkflowError::InvalidDefinition(format!(
                "action name {name:?} appears twice"
            )));
        }
        def.name = name.clone();
        if let (
            Some(default_timezone),
            TriggerDef::Schedule {
                timezone: timezone @ None,
                ..
            },
        ) = (&default_timezone, &mut def.trigger)
        {
            *timezone = Some(default_timezone.clone());
        }
        match &def.project {
            Some(declared) if declared != project => {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "action {name:?} names project {declared:?} but this file belongs to {project:?}"
                )));
            }
            _ => def.project = Some(project.to_owned()),
        }
        def.validate().map_err(|error| {
            WorkflowError::InvalidDefinition(format!("action {name:?}: {error}"))
        })?;
        let canonical_json = serde_json::to_string(&def)
            .map_err(|error| WorkflowError::InvalidDefinition(error.to_string()))?;
        let hash = definition_hash_hex(&def)?;
        entries.push(ActionEntry {
            name,
            def,
            canonical_json,
            hash,
        });
    }
    Ok(entries)
}

/// Spec § 5.1 mode (2): a `wake_agent` step whose `brief` is
/// `{on_success, on_failure}` becomes two steps, `<id>_success` and
/// `<id>_failure`, with opposite `if:` conditions on the nearest preceding
/// `run_on_host` step's exit code. Expanded before hashing, so the relay's
/// stored definition and the host's recompilation see the same two steps.
fn expand_wake_brief_sugar(document: &mut serde_yaml::Value) -> Result<(), WorkflowError> {
    use serde_yaml::Value;
    let Some(actions) = document.get_mut("actions").and_then(Value::as_sequence_mut) else {
        return Ok(());
    };
    for action in actions.iter_mut() {
        let name = action
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let Some(steps) = action.get_mut("steps").and_then(Value::as_sequence_mut) else {
            continue;
        };
        let mut expanded: Vec<Value> = Vec::with_capacity(steps.len());
        let mut last_host_step: Option<String> = None;
        for step in steps.drain(..) {
            let action_kind = step.get("action").and_then(Value::as_str).unwrap_or("");
            if action_kind == "run_on_host" {
                last_host_step = step.get("id").and_then(Value::as_str).map(str::to_owned);
            }
            let is_sugar =
                action_kind == "wake_agent" && step.get("brief").is_some_and(Value::is_mapping);
            if !is_sugar {
                expanded.push(step);
                continue;
            }
            let id = step
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let Some(previous) = last_host_step.clone() else {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "action {name:?} step {id:?}: brief.on_success/on_failure needs a run_on_host \
                     step before it"
                )));
            };
            let brief = step
                .get("brief")
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            let on_success = brief.get("on_success").and_then(Value::as_str);
            let on_failure = brief.get("on_failure").and_then(Value::as_str);
            let (Some(on_success), Some(on_failure)) = (on_success, on_failure) else {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "action {name:?} step {id:?}: brief must be a string or exactly \
                     {{on_success, on_failure}}"
                )));
            };
            if brief.len() != 2 {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "action {name:?} step {id:?}: brief must be a string or exactly \
                     {{on_success, on_failure}}"
                )));
            }
            if step.get("if").is_some() {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "action {name:?} step {id:?}: a brief with on_success/on_failure already \
                     decides its own `if`"
                )));
            }
            for (suffix, text, condition) in
                [("success", on_success, "=="), ("failure", on_failure, "!=")]
            {
                let mut clone = step.clone();
                if let Some(map) = clone.as_mapping_mut() {
                    map.insert(
                        Value::String("id".into()),
                        Value::String(format!("{id}_{suffix}")),
                    );
                    map.insert(
                        Value::String("if".into()),
                        Value::String(format!("steps_{previous}_output_exit_code {condition} 0")),
                    );
                    map.insert(
                        Value::String("brief".into()),
                        Value::String(text.to_owned()),
                    );
                }
                expanded.push(clone);
            }
        }
        *steps = expanded;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse";

    fn file(extra: &str) -> String {
        format!(
            "schema: buzz-project-actions/v1\n{extra}actions:\n  - name: nightly-build\n    trigger: {{ on: manual }}\n    steps:\n      - id: build\n        action: run_on_host\n        command: [\"just\", \"ci\"]\n        timeout: 30m\n  - name: hello\n    trigger: {{ on: webhook }}\n    steps:\n      - id: say\n        action: send_message\n        text: hi\n"
        )
    }

    #[test]
    fn entries_are_bound_to_the_project_and_hashed() {
        let entries = parse_actions_yml(&file(""), PROJECT).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "nightly-build");
        assert_eq!(entries[0].def.project.as_deref(), Some(PROJECT));
        assert_eq!(entries[0].hash.len(), 64);
        assert!(entries[0].canonical_json.contains("\"project\""));
        // Recompiling the same text yields the same hash: the host's check.
        let again = parse_actions_yml(&file(""), PROJECT).expect("parse");
        assert_eq!(again[0].hash, entries[0].hash);
    }

    /// The file's `timezone` becomes every schedule's explicit zone before
    /// hashing, so the stored definition and the host's recompilation agree
    /// without either knowing the file-level default.
    #[test]
    fn file_timezone_is_applied_to_schedules_without_their_own() {
        let text = "schema: buzz-project-actions/v1\ntimezone: Europe/London\nactions:\n  - name: nightly\n    trigger: { on: schedule, cron: '0 17 * * FRI' }\n    steps:\n      - id: build\n        action: run_on_host\n        command: [\"true\"]\n  - name: tokyo\n    trigger: { on: schedule, cron: '0 9 * * *', timezone: Asia/Tokyo }\n    steps:\n      - id: say\n        action: send_message\n        text: hi\n";
        let entries = parse_actions_yml(text, PROJECT).expect("parse");
        let zone = |entry: &ActionEntry| match &entry.def.trigger {
            TriggerDef::Schedule { timezone, .. } => timezone.clone(),
            _ => None,
        };
        assert_eq!(zone(&entries[0]).as_deref(), Some("Europe/London"));
        assert_eq!(zone(&entries[1]).as_deref(), Some("Asia/Tokyo"));
        assert!(entries[0].canonical_json.contains("Europe/London"));
    }

    /// The verify-style action of spec § 5.1: a manual trigger whose host
    /// step declares `checkout: required`, so the run must name its commit
    /// (ledger 178(g)).
    #[test]
    fn a_verify_action_declares_its_required_checkout_through_the_file() {
        let text = "schema: buzz-project-actions/v1\nactions:\n  - name: verify\n    trigger: { on: manual }\n    steps:\n      - id: verify\n        action: run_on_host\n        command: [\"python3\", \"-m\", \"unittest\", \"discover\", \"-s\", \"tests\"]\n        working_directory: '.'\n        checkout: required\n        timeout: 300s\n";
        let entries = parse_actions_yml(text, PROJECT).expect("parse");
        assert_eq!(
            entries[0].def.step_requiring_bound_checkout(),
            Some("verify")
        );
        assert!(entries[0]
            .canonical_json
            .contains("\"checkout\":\"required\""));
    }

    #[test]
    fn refuses_wrong_schema_timezone_duplicates_and_foreign_project() {
        let bad_schema = file("").replace("buzz-project-actions/v1", "nope/v9");
        assert!(parse_actions_yml(&bad_schema, PROJECT)
            .unwrap_err()
            .to_string()
            .contains("schema"));
        assert!(
            parse_actions_yml(&file("timezone: Mars/Olympus\n"), PROJECT)
                .unwrap_err()
                .to_string()
                .contains("unknown timezone")
        );
        let dup = file("").replace("name: hello", "name: nightly-build");
        assert!(parse_actions_yml(&dup, PROJECT)
            .unwrap_err()
            .to_string()
            .contains("twice"));
        let foreign = file("").replace(
            "  - name: hello\n",
            "  - name: hello\n    project: '30621:2222222222222222222222222222222222222222222222222222222222222222:x'\n",
        );
        assert!(parse_actions_yml(&foreign, PROJECT)
            .unwrap_err()
            .to_string()
            .contains("belongs to"));
        assert!(parse_actions_yml(&file(""), "not-a-coordinate").is_err());
        let hooked = file("").replace("trigger: { on: manual }", "trigger: { on: webhook }");
        assert!(parse_actions_yml(&hooked, PROJECT)
            .unwrap_err()
            .to_string()
            .contains("webhook trigger"));
        let padded = file("").replace("name: nightly-build", "name: 'nightly-build '");
        let entries = parse_actions_yml(&padded, PROJECT).expect("parse");
        assert_eq!(
            entries[0].def.name, "nightly-build",
            "the bound name is trimmed too"
        );
    }

    /// Spec § 5.1 mode (2): the on_success/on_failure sugar becomes two
    /// `wake_agent` steps gated on the preceding host step's exit code, and
    /// the expansion happens before hashing so relay and host agree.
    #[test]
    fn a_two_way_brief_expands_into_two_gated_wake_steps() {
        let text = "schema: buzz-project-actions/v1\nactions:\n  - name: on-push\n    trigger: { on: manual }\n    steps:\n      - id: build\n        action: run_on_host\n        command: [\"cargo\", \"build\"]\n      - id: review\n        action: wake_agent\n        to: { agent: Keystone }\n        brief:\n          on_success: Review the warnings.\n          on_failure: Fix the build.\n";
        let entries = parse_actions_yml(text, PROJECT).expect("parse");
        let steps = &entries[0].def.steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[1].id, "review_success");
        assert_eq!(
            steps[1].if_expr.as_deref(),
            Some("steps_build_output_exit_code == 0")
        );
        assert_eq!(steps[2].id, "review_failure");
        assert_eq!(
            steps[2].if_expr.as_deref(),
            Some("steps_build_output_exit_code != 0")
        );
        match &steps[2].action {
            crate::schema::ActionDef::WakeAgent { to, brief } => {
                assert_eq!(to.agent, "Keystone");
                assert_eq!(brief, "Fix the build.");
            }
            other => panic!("expected wake_agent, got {other:?}"),
        }
        let orphan = text.replace("      - id: build\n        action: run_on_host\n        command: [\"cargo\", \"build\"]\n", "");
        assert!(parse_actions_yml(&orphan, PROJECT)
            .unwrap_err()
            .to_string()
            .contains("needs a run_on_host"));
    }
}
