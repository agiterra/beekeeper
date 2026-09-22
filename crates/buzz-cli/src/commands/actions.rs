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

use crate::client::BuzzClient;
use crate::commands::actions_authority::{
    decide_project_action_authority, project_action_grant_remedy, read_project_action_authority,
};
use crate::commands::actions_example::explain_actions_error;
use crate::error::CliError;
use crate::validate::{parse_uuid, read_file_or_stdin, sdk_err};

pub use buzz_workflow::actions_file::action_workflow_id;

/// The YAML the relay receives for one entry: its definition with `project`
/// bound, so the relay's `parse_yaml` hashes exactly what the host recompiles.
pub fn bound_definition_yaml(entry: &ActionEntry) -> Result<String, CliError> {
    buzz_workflow::actions_file::bound_definition_yaml(entry)
        .map_err(|error| CliError::Other(format!("serialize action {:?}: {error}", entry.name)))
}

/// Read and parse the actions file, answering for its shape when it fails.
///
/// The parser's own words are preserved verbatim; what is added is the thing
/// no error message could supply — which object the missing key belongs to,
/// the whole of that object's required keys, and the command that prints a
/// working file (ledger 206 A). The old behaviour handed serde's one-field-
/// at-a-time message straight on, which is what made a lead learn this
/// schema by probing.
fn parse_file(file: &str, project: &str) -> Result<Vec<ActionEntry>, CliError> {
    let text = read_file_or_stdin(file)?;
    parse_actions_yml(&text, project)
        .map_err(|error| explain_actions_error(file, &text, &error.to_string()))
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
    let caller_pubkey = client.keys().public_key().to_hex().to_ascii_lowercase();
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
        // The relay's own words stay in `message` verbatim — a refusal must
        // never be paraphrased — and the remedy is added beside it only for
        // the refusals the delegation can actually fix (ledger 186, finding
        // 178(f)). A transport error or an `invalid:` refusal gets none: a
        // grant would not help, and offering one would send the caller to ask
        // an owner for something they already have.
        let remedy = (!accepted && is_authority_refusal(&message))
            .then(|| project_action_grant_remedy(project, &caller_pubkey));
        let line = serde_json::json!({
            "name": entry.name,
            "workflow_id": workflow_id,
            "hash": entry.hash,
            "event_id": event_id,
            "accepted": accepted,
            "message": message,
            "remedy": remedy,
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

/// Whether a relay refusal is one a project-actions delegation could fix.
///
/// The relay prefixes its authority refusals with `forbidden:` and its shape
/// refusals with `invalid:` (`crates/buzz-relay/src/handlers/command_executor.rs`),
/// so the prefix is the discriminator rather than a match on wording that
/// would rot the first time a message is reworded.
fn is_authority_refusal(message: &str) -> bool {
    message.trim_start().starts_with("forbidden:")
}

/// Whether publishing this entry additionally needs the channel owner/admin
/// role — a `run_on_host` step, or a `call_webhook` that can carry channel
/// content outward (`command_executor.rs:789-818`).
fn needs_channel_elevation(entry: &ActionEntry) -> bool {
    entry.def.has_host_steps() || entry.def.requires_elevated_authority()
}

/// `bee actions status`: what the file would publish — name, hash, trigger,
/// and whether any step runs on a host — plus whether this key may actually
/// publish and trigger it.
///
/// The authority half is a prediction of the relay's answer, read once for the
/// project and narrowed per entry; it exists because the file half alone let a
/// team session accept a goal it had no standing to finish (ledger 186,
/// finding 178(f)). `channel` is the channel whose kind:44228 authority chain
/// would carry a delegation; without it the two predictions are `null`, with
/// the missing read named, because a `false` nobody checked is a lie the same
/// size as a `true`.
pub async fn cmd_status(
    client: &BuzzClient,
    project: &str,
    channel: Option<&str>,
    file: &str,
) -> Result<(), CliError> {
    let entries = parse_file(file, project)?;
    let channel = match channel {
        Some(channel) => Some(parse_uuid(channel)?.to_string()),
        None => None,
    };
    let inputs = read_project_action_authority(client, project, channel.as_deref()).await;
    let authority = decide_project_action_authority(&inputs);
    let lines: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            let entry_authority = authority
                .clone()
                .narrowed_by_channel_elevation(needs_channel_elevation(entry));
            serde_json::json!({
                "name": entry.name,
                "workflow_id": action_workflow_id(project, &entry.name),
                "hash": entry.hash,
                "trigger": trigger_name(entry),
                "run_on_host": runs_on_host(entry),
                "may_publish": entry_authority.may_publish,
                "may_trigger": entry_authority.may_trigger,
                "authority": entry_authority.to_json(),
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
        // Dispatched ahead of the key gate in `lib.rs` too, so it answers
        // offline; this arm is the one a caller with a key reaches.
        ActionsCmd::Example { kind } => crate::commands::actions_example::cmd_example(kind),
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
        ActionsCmd::Status {
            project,
            channel,
            file,
        } => {
            cmd_status(
                client,
                &project,
                channel.as_deref(),
                file.as_deref().unwrap_or(ACTIONS_YML),
            )
            .await
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
    fn only_an_authority_refusal_gets_the_grant_remedy() {
        // The relay's two prefixes, verbatim (`command_executor.rs`).
        assert!(is_authority_refusal(
            "forbidden: a project action must be saved by the project creator, a roster owner or \
             a repository founder"
        ));
        assert!(is_authority_refusal(
            "forbidden: not authorized to trigger this workflow"
        ));
        // A shape refusal, and a transport error, are not fixed by a grant.
        assert!(!is_authority_refusal(
            "invalid: a tag names a project the definition does not declare"
        ));
        assert!(!is_authority_refusal("error sending request for url"));
    }

    #[test]
    fn a_host_step_entry_is_flagged_as_needing_channel_elevation() {
        let entries = parse_actions_yml(FILE, PROJECT).expect("parse");
        // The first entry runs a command on a host; the second sends a message.
        assert!(needs_channel_elevation(&entries[0]));
        assert!(!needs_channel_elevation(&entries[1]));
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
