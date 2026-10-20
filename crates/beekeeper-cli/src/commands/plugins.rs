//! `bee plugins` — any plugin the relay runs, through the same route the
//! desktop uses, and the draft executor every plugin write goes through.
//!
//! A plugin write returns unsigned drafts. This client signs each with the
//! caller's own key and publishes it through ordinary ingest, in order, under
//! the plugin's stated refusal policy (`docs/plugins.md` § Drafts):
//!
//! - `abort` — stop. Nothing before it saved: the relay's refusal is the
//!   error. Something did: the write is partial, and the error says what
//!   landed (the draft's `partialSummary`, or a generic line).
//! - `skip-group` — record the group as failed, skip the rest of that group,
//!   carry on with the next.
//! - a submit with no answer (`DeliveryUnknown`, a timeout) stops the run as
//!   unconfirmed, whatever the policy: the event may still land.
//!
//! Before signing anything the client checks each draft is a kind it is
//! willing to sign for a plugin and that its only `a` tag is the project the
//! call named — the relay checked too; a client never signs on trust alone.

use serde_json::{json, Value};

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The kinds this client will sign on a plugin's behalf.
const SIGNABLE_KINDS: &[u64] = &[beekeeper_core::kind::KIND_PROJECT_TODO_OP as u64];

/// `GET /plugins`.
pub async fn list(client: &BeekeeperClient) -> Result<Value, CliError> {
    let raw = client.get_authed("/plugins").await?;
    serde_json::from_str(&raw).map_err(|e| CliError::Other(format!("bad /plugins reply: {e}")))
}

/// The plugin that implements `api`: the one named, or the only one. Two
/// with no name is an error that lists them — never a silent pick.
pub async fn provider_for(
    client: &BeekeeperClient,
    api: &str,
    named: Option<&str>,
) -> Result<String, CliError> {
    let listing = list(client).await?;
    let providers: Vec<String> = listing["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["api"]
                .as_array()
                .is_some_and(|apis| apis.iter().any(|a| a == api))
        })
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();
    match (named, providers.as_slice()) {
        (Some(name), _) if providers.iter().any(|p| p == name) => Ok(name.to_owned()),
        (Some(name), _) => Err(CliError::Usage(format!(
            "this relay has no plugin {name:?} implementing {api}; it has: {}",
            if providers.is_empty() {
                "none".to_owned()
            } else {
                providers.join(", ")
            }
        ))),
        (None, [only]) => Ok(only.clone()),
        (None, []) => Err(CliError::Other(format!(
            "this relay runs no plugin implementing {api}"
        ))),
        (None, many) => Err(CliError::Usage(format!(
            "this relay has {} providers of {api} ({}); name one with --provider",
            many.len(),
            many.join(", ")
        ))),
    }
}

/// `POST /plugins/{name}/invoke`.
pub async fn invoke(
    client: &BeekeeperClient,
    plugin: &str,
    operation: &str,
    project: &str,
    input: Value,
) -> Result<Value, CliError> {
    let raw = client
        .post_authed(
            &format!("/plugins/{plugin}/invoke"),
            &json!({ "operation": operation, "scope": { "project": project }, "input": input }),
        )
        .await?;
    serde_json::from_str(&raw).map_err(|e| CliError::Other(format!("bad invoke reply: {e}")))
}

fn str_tags(draft: &Value) -> Result<Vec<Vec<String>>, CliError> {
    draft["tags"]
        .as_array()
        .ok_or_else(|| CliError::Other("draft has no tags".into()))?
        .iter()
        .map(|tag| {
            tag.as_array()
                .ok_or_else(|| CliError::Other("draft tag is not an array".into()))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| CliError::Other("draft tag value is not a string".into()))
                })
                .collect()
        })
        .collect()
}

/// Refuse to sign anything that is not a signable kind scoped to `project`.
fn check_draft(draft: &Value, project: &str) -> Result<(), CliError> {
    let kind = draft["kind"].as_u64().unwrap_or(0);
    if !SIGNABLE_KINDS.contains(&kind) {
        return Err(CliError::Other(format!(
            "the plugin returned a kind {kind} draft; this client signs only kinds {SIGNABLE_KINDS:?} for plugins"
        )));
    }
    let tags = str_tags(draft)?;
    let scopes: Vec<&Vec<String>> = tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("a"))
        .collect();
    if scopes.len() != 1 || scopes[0].get(1).map(String::as_str) != Some(project) {
        return Err(CliError::Other(
            "the plugin returned a draft outside the project this call named; nothing was signed"
                .into(),
        ));
    }
    Ok(())
}

/// Sign and publish `drafts` in order as the caller. Returns what was
/// published and which groups failed; an `abort` refusal is an error.
pub async fn execute_drafts(
    client: &BeekeeperClient,
    project: &str,
    drafts: &Value,
) -> Result<Value, CliError> {
    let drafts = drafts.as_array().cloned().unwrap_or_default();
    for draft in &drafts {
        check_draft(draft, project)?;
    }
    let mut published = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut saved_groups = std::collections::BTreeSet::new();
    let mut skipped = std::collections::BTreeSet::new();
    for draft in &drafts {
        let group = draft["group"].as_str().unwrap_or("").to_owned();
        if skipped.contains(&group) {
            continue;
        }
        let tags = str_tags(draft)?
            .into_iter()
            .map(|t| nostr::Tag::parse(t).map_err(|e| CliError::Other(format!("draft tag: {e}"))))
            .collect::<Result<Vec<_>, _>>()?;
        let builder = nostr::EventBuilder::new(
            nostr::Kind::Custom(draft["kind"].as_u64().unwrap_or(0) as u16),
            draft["content"].as_str().unwrap_or(""),
        )
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(
            draft["createdAt"].as_u64().unwrap_or(0),
        ));
        // Signed verbatim (see `todos::publish`): the draft's tag set is
        // closed, so the auth tag `sign_event` injects would be refused.
        let event = client.sign_event_unchecked(builder)?;
        let event_id = event.id.to_hex();
        let outcome = match client.submit_event(event).await {
            Ok(raw) => super::parse_write_response(&raw, "plugin draft was superseded"),
            Err(error) => Err(error),
        };
        match outcome {
            Ok(_) => {
                published.push(json!({ "event_id": event_id, "group": group }));
                saved_groups.insert(group);
            }
            Err(error) => {
                let reason = error.to_string();
                // No answer: the event may still land. Stop and say so —
                // reporting it as refused would invite a retry that could
                // mint a duplicate.
                if matches!(error, CliError::DeliveryUnknown(_))
                    || matches!(&error, CliError::Network(e) if e.is_timeout())
                {
                    return Err(CliError::DeliveryUnknown(format!(
                        "the relay did not confirm the {group} change ({reason}); it may still \
                         land — read before retrying. published: {}",
                        Value::Array(published)
                    )));
                }
                let summary = draft["partialSummary"].as_str();
                if draft["ifRefused"] == "skip-group" {
                    let message = match summary {
                        Some(summary) if saved_groups.contains(&group) => {
                            format!("{summary} ({reason})")
                        }
                        _ => reason,
                    };
                    failed.push(json!({ "group": group, "message": message }));
                    skipped.insert(group);
                    continue;
                }
                if published.is_empty() {
                    return Err(error);
                }
                let summary = summary.map(str::to_owned).unwrap_or_else(|| {
                    format!(
                        "Only part of the change was confirmed: {} of {} signed changes landed",
                        published.len(),
                        drafts.len()
                    )
                });
                return Err(CliError::Other(format!(
                    "{summary} ({reason}); published: {}",
                    Value::Array(published)
                )));
            }
        }
    }
    Ok(json!({ "published": published, "failed": failed }))
}

/// Dispatch `bee plugins`.
pub async fn dispatch(cmd: crate::PluginsCmd, client: &BeekeeperClient) -> Result<(), CliError> {
    match cmd {
        crate::PluginsCmd::List => {
            println!("{}", list(client).await?);
            Ok(())
        }
        crate::PluginsCmd::Invoke {
            plugin,
            operation,
            project,
            input,
            publish,
        } => {
            let project = super::pulse::resolve_project(client, project.as_deref()).await?;
            let input: Value = match input {
                Some(text) => serde_json::from_str(&text)
                    .map_err(|e| CliError::Usage(format!("--input is not JSON: {e}")))?,
                None => json!({}),
            };
            let mut reply = invoke(client, &plugin, &operation, &project, input).await?;
            if publish {
                let ran = execute_drafts(client, &project, &reply["drafts"]).await?;
                reply["published"] = ran["published"].clone();
                reply["failed"] = ran["failed"].clone();
            }
            println!("{reply}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:demo";

    #[test]
    fn only_signable_kinds_in_the_named_project_pass() {
        let good = json!({ "kind": 44248, "tags": [["a", PROJECT], ["td-v", "td1-1"]] });
        assert!(check_draft(&good, PROJECT).is_ok());
        let other_kind = json!({ "kind": 1, "tags": [["a", PROJECT]] });
        assert!(check_draft(&other_kind, PROJECT).is_err());
        let other_scope = json!({ "kind": 44248, "tags": [["a", "30621:22:x"]] });
        assert!(check_draft(&other_scope, PROJECT).is_err());
        let two_scopes = json!({ "kind": 44248, "tags": [["a", PROJECT], ["a", PROJECT]] });
        assert!(check_draft(&two_scopes, PROJECT).is_err());
    }
}
