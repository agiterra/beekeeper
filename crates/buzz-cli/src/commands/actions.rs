//! `bee actions` — a project's `actions.yml` (the agents repository's root,
//! spec § 4.11), published one
//! kind:30620 per entry (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 5.1).
//!
//! The file is parsed with [`buzz_workflow::parse_actions_yml`], the same
//! function a host runs when it recompiles an entry to check a kind:46013's
//! `definitionHash`. What this command publishes is the **bound** definition
//! (`project` set) re-serialized as YAML, so the relay's own `parse_yaml`
//! yields the identical canonical JSON and hash.
//!
//! The workflow id is deterministic — UUID v5 of `<project>\n<name>` under
//! [`ACTIONS_NAMESPACE`] — so republishing the file upserts each entry's
//! kind:30620 in place instead of minting a new workflow every time.

use buzz_workflow::{parse_actions_yml, ActionEntry, ACTIONS_YML};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{parse_uuid, read_file_or_stdin, sdk_err};

/// UUID v5 namespace for action workflow ids: `uuid5(ACTIONS_NAMESPACE,
/// "<project coordinate>\n<action name>")`.
pub const ACTIONS_NAMESPACE: Uuid = Uuid::from_u128(0x6b3a1c2e_9d4f_4e7a_8b1c_2f5d9e0a7c31);

/// The workflow id one action of one project always publishes under.
pub fn action_workflow_id(project: &str, name: &str) -> Uuid {
    Uuid::new_v5(&ACTIONS_NAMESPACE, format!("{project}\n{name}").as_bytes())
}

/// The YAML the relay receives for one entry: its definition with `project`
/// bound, so the relay's `parse_yaml` hashes exactly what the host recompiles.
pub fn bound_definition_yaml(entry: &ActionEntry) -> Result<String, CliError> {
    serde_yaml::to_string(&entry.def)
        .map_err(|error| CliError::Other(format!("serialize action {:?}: {error}", entry.name)))
}

fn parse_file(file: &str, project: &str) -> Result<Vec<ActionEntry>, CliError> {
    let text = read_file_or_stdin(file)?;
    parse_actions_yml(&text, project).map_err(|error| CliError::Usage(format!("{file}: {error}")))
}

fn trigger_name(entry: &ActionEntry) -> String {
    serde_json::to_value(&entry.def.trigger)
        .ok()
        .and_then(|value| value.get("on")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn runs_on_host(entry: &ActionEntry) -> bool {
    entry
        .def
        .steps
        .iter()
        .any(|step| matches!(step.action, buzz_workflow::ActionDef::RunOnHost { .. }))
}

/// `bee actions publish`: one kind:30620 per entry, into `channel`.
///
/// Every entry is attempted; the command fails after the loop when any was
/// refused, so one bad entry does not hide the others' outcome.
pub async fn cmd_publish(
    client: &BuzzClient,
    project: &str,
    channel: &str,
    file: &str,
) -> Result<(), CliError> {
    let channel_uuid = parse_uuid(channel)?;
    let entries = parse_file(file, project)?;
    let mut refused = 0usize;
    for entry in &entries {
        let workflow_id = action_workflow_id(project, &entry.name);
        let yaml = bound_definition_yaml(entry)?;
        let builder =
            buzz_sdk::build_project_workflow_def(channel_uuid, workflow_id, project, &yaml)
                .map_err(sdk_err)?;
        let event = client.sign_event(builder)?;
        let (accepted, event_id, message) = match client.submit_event(event).await {
            Ok(raw) => {
                let response: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                (
                    response
                        .get("accepted")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    response
                        .get("event_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    response
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                )
            }
            Err(error) => (false, None, error.to_string()),
        };
        if !accepted {
            refused += 1;
        }
        let line = serde_json::json!({
            "name": entry.name,
            "workflow_id": workflow_id,
            "hash": entry.hash,
            "event_id": event_id,
            "accepted": accepted,
            "message": message,
        });
        println!("{line}");
    }
    if refused > 0 {
        return Err(CliError::Other(format!(
            "{refused} of {} actions refused",
            entries.len()
        )));
    }
    Ok(())
}

/// `bee actions status`: what the file would publish — name, hash, trigger,
/// and whether any step runs on a host. Reads nothing from the relay.
pub fn cmd_status(project: &str, file: &str) -> Result<(), CliError> {
    let entries = parse_file(file, project)?;
    let lines: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.name,
                "workflow_id": action_workflow_id(project, &entry.name),
                "hash": entry.hash,
                "trigger": trigger_name(entry),
                "run_on_host": runs_on_host(entry),
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string(&lines).unwrap_or_else(|_| "[]".to_owned())
    );
    Ok(())
}

pub async fn dispatch(cmd: crate::ActionsCmd, client: &BuzzClient) -> Result<(), CliError> {
    use crate::ActionsCmd;
    match cmd {
        ActionsCmd::Publish {
            project,
            channel,
            file,
        } => {
            cmd_publish(
                client,
                &project,
                &channel,
                file.as_deref().unwrap_or(ACTIONS_YML),
            )
            .await
        }
        ActionsCmd::Status { project, file } => {
            cmd_status(&project, file.as_deref().unwrap_or(ACTIONS_YML))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_workflow::{hash_definition_value, WorkflowEngine};

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse";

    const FILE: &str = "schema: buzz-project-actions/v1\nactions:\n  - name: nightly-build\n    description: Build it\n    trigger: { on: schedule, cron: '0 2 * * *' }\n    steps:\n      - id: build\n        action: run_on_host\n        command: [\"just\", \"ci\"]\n        timeout: 30m\n        env: { CI: '1' }\n  - name: hello\n    trigger: { on: webhook }\n    steps:\n      - id: say\n        action: send_message\n        text: hi\n";

    #[test]
    fn published_yaml_hashes_as_the_host_recompiles_it() {
        let entries = parse_actions_yml(FILE, PROJECT).expect("parse");
        for entry in &entries {
            let yaml = bound_definition_yaml(entry).expect("yaml");
            // The relay's chain: YAML → WorkflowDef → canonical JSON → Value → sha256.
            let (_, canonical) = WorkflowEngine::parse_yaml(&yaml).expect("relay parse");
            let value: serde_json::Value = serde_json::from_str(&canonical).expect("value");
            let relay_hash = hex::encode(hash_definition_value(&value).expect("hash"));
            assert_eq!(relay_hash, entry.hash, "entry {}", entry.name);
            assert_eq!(canonical, entry.canonical_json, "entry {}", entry.name);
            assert!(yaml.contains(PROJECT), "the bound project is published");
        }
    }

    #[test]
    fn workflow_ids_are_deterministic_per_project_and_name() {
        let a = action_workflow_id(PROJECT, "nightly-build");
        assert_eq!(a, action_workflow_id(PROJECT, "nightly-build"));
        assert_ne!(a, action_workflow_id(PROJECT, "hello"));
        let other = PROJECT.replace("pulse", "other");
        assert_ne!(a, action_workflow_id(&other, "nightly-build"));
        assert_eq!(a.get_version_num(), 5);
    }

    #[test]
    fn status_reads_trigger_and_host_flag() {
        let entries = parse_actions_yml(FILE, PROJECT).expect("parse");
        assert_eq!(trigger_name(&entries[0]), "schedule");
        assert!(runs_on_host(&entries[0]));
        assert_eq!(trigger_name(&entries[1]), "webhook");
        assert!(!runs_on_host(&entries[1]));
    }
}
