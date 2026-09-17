//! `beekeeper/actions.yml`: a project's actions, each an ordinary workflow
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
use crate::schema::WorkflowDef;

/// Exact schema the file must name.
pub const ACTIONS_SCHEMA: &str = "buzz-project-actions/v1";
/// Where the file lives, relative to the project checkout.
pub const ACTIONS_YML: &str = "beekeeper/actions.yml";
/// Ceiling on entries in one file.
pub const MAX_ACTIONS: usize = 64;

/// The file as authored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionsFile {
    /// Must equal [`ACTIONS_SCHEMA`].
    pub schema: String,
    /// IANA timezone for every schedule below. Parsed, not yet honoured:
    /// slice C2 wires it, so a file that sets it is refused until then rather
    /// than silently run in UTC.
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
    let file: ActionsFile = serde_yaml::from_str(text)?;
    if file.schema != ACTIONS_SCHEMA {
        return Err(WorkflowError::InvalidDefinition(format!(
            "actions.yml schema must be {ACTIONS_SCHEMA:?} (got {:?})",
            file.schema
        )));
    }
    if let Some(timezone) = &file.timezone {
        return Err(WorkflowError::InvalidDefinition(format!(
            "actions.yml timezone {timezone:?} is not supported yet; schedules run in UTC until \
             project-scoped triggers land (spec § 5.3), so remove the key for now"
        )));
    }
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

    #[test]
    fn refuses_wrong_schema_timezone_duplicates_and_foreign_project() {
        let bad_schema = file("").replace("buzz-project-actions/v1", "nope/v9");
        assert!(parse_actions_yml(&bad_schema, PROJECT)
            .unwrap_err()
            .to_string()
            .contains("schema"));
        assert!(
            parse_actions_yml(&file("timezone: Europe/London\n"), PROJECT)
                .unwrap_err()
                .to_string()
                .contains("timezone")
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
}
