//! `bee planning` — a project's issues, through whichever plugin on the relay
//! implements `beekeeper.planning/1`. The same operations the desktop's
//! Planning tab calls; no planning logic lives here. Writes are signed by the
//! caller and published through ingest (`plugins::execute_drafts`).
//!
//! `bee todos` is separate and unchanged: it edits the NIP-TD lists directly.

use serde_json::{json, Value};

use super::plugins;
use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The operation API this command speaks.
pub const PLANNING_API: &str = "beekeeper.planning/1";

/// Beekeeper's own Planning plugin, the built-in a relay ships: the provider
/// used when the caller names none, whatever else sorts before it.
pub const NATIVE_PLANNING_PLUGIN: &str = "planning";

/// `<containerId>/<issueId>` → the two ids.
fn split_key(key: &str) -> Result<(String, String), CliError> {
    match key.split_once('/') {
        Some((container, issue)) if !container.is_empty() && !issue.is_empty() => {
            Ok((container.to_owned(), issue.to_owned()))
        }
        _ => Err(CliError::Usage(
            "--issue takes the issue key `<containerId>/<issueId>` (see `bee planning read`)"
                .to_owned(),
        )),
    }
}

fn nullable(value: Option<String>, none_word: &str) -> Value {
    match value {
        Some(v) if v.eq_ignore_ascii_case(none_word) => Value::Null,
        Some(v) => json!(v),
        None => Value::Null,
    }
}

struct Call<'a> {
    client: &'a BeekeeperClient,
    plugin: String,
    project: String,
}

impl Call<'_> {
    async fn read(&self, operation: &str, input: Value) -> Result<Value, CliError> {
        plugins::invoke(self.client, &self.plugin, operation, &self.project, input).await
    }

    async fn write(&self, operation: &str, input: Value) -> Result<Value, CliError> {
        let reply = self.read(operation, input).await?;
        let ran = plugins::execute_drafts(self.client, &self.project, &reply["drafts"])
            .await
            .map_err(|error| plugins::with_result(error, &reply["result"]))?;
        Ok(json!({
            "plugin": reply["plugin"],
            "operation": operation,
            "result": reply["result"],
            "published": ran["published"],
            "failed": ran["failed"],
        }))
    }
}

/// Dispatch `bee planning`.
pub async fn dispatch(cmd: crate::PlanningCmd, client: &BeekeeperClient) -> Result<(), CliError> {
    use crate::PlanningCmd as P;
    let (project, provider) = match &cmd {
        P::Providers { project }
        | P::Read { project, .. }
        | P::Activity { project, .. }
        | P::Create { project, .. }
        | P::Title { project, .. }
        | P::Describe { project, .. }
        | P::Status { project, .. }
        | P::Priority { project, .. }
        | P::Assign { project, .. } => (project.clone(), cmd.provider()),
    };
    if let P::Providers { .. } = cmd {
        let listing = plugins::list(client).await?;
        let providers: Vec<&Value> = listing["plugins"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| {
                p["api"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x == PLANNING_API))
            })
            .collect();
        println!("{}", json!({ "api": PLANNING_API, "providers": providers }));
        return Ok(());
    }
    let call = Call {
        client,
        plugin: plugins::provider_for(
            client,
            PLANNING_API,
            provider.as_deref(),
            Some(NATIVE_PLANNING_PLUGIN),
        )
        .await?,
        project: super::pulse::resolve_project(client, project.as_deref()).await?,
    };
    let out = match cmd {
        P::Providers { .. } => unreachable!("handled above"),
        P::Read { .. } => call.read("read", json!({})).await?,
        P::Activity { issue, .. } => {
            let (container, id) = split_key(&issue)?;
            call.read(
                "activity",
                json!({ "containerId": container, "issueId": id }),
            )
            .await?
        }
        P::Create {
            title,
            description,
            status,
            priority,
            assignee,
            container,
            ..
        } => {
            call.write(
                "create-issue",
                json!({
                    "title": title,
                    "description": description,
                    "statusId": status,
                    "priorityId": priority,
                    "assignee": nullable(assignee, "none"),
                    "containerId": container,
                }),
            )
            .await?
        }
        P::Title { issue, title, .. } => {
            let (container, id) = split_key(&issue)?;
            call.write(
                "set-title",
                json!({ "containerId": container, "issueId": id, "title": title }),
            )
            .await?
        }
        P::Describe {
            issue,
            description,
            clear,
            ..
        } => {
            let (container, id) = split_key(&issue)?;
            let description = match (description, clear) {
                (Some(text), false) => json!(text),
                (None, true) => Value::Null,
                _ => {
                    return Err(CliError::Usage(
                        "give exactly one of --description or --clear".to_owned(),
                    ))
                }
            };
            call.write(
                "set-description",
                json!({ "containerId": container, "issueId": id, "description": description }),
            )
            .await?
        }
        P::Status { issue, status, .. } => {
            let (container, id) = split_key(&issue)?;
            call.write(
                "set-status",
                json!({ "containerId": container, "issueId": id, "statusId": status }),
            )
            .await?
        }
        P::Priority {
            issue, priority, ..
        } => {
            let (container, id) = split_key(&issue)?;
            call.write(
                "set-priority",
                json!({ "containerId": container, "issueId": id, "priorityId": priority }),
            )
            .await?
        }
        P::Assign {
            issue, assignee, ..
        } => {
            let (container, id) = split_key(&issue)?;
            call.write(
                "set-assignee",
                json!({ "containerId": container, "issueId": id,
                        "assignee": nullable(Some(assignee), "none") }),
            )
            .await?
        }
    };
    println!("{out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_keys_split_into_container_and_issue() {
        assert_eq!(
            split_key("abc/def").unwrap(),
            ("abc".to_owned(), "def".to_owned())
        );
        assert!(split_key("abc").is_err());
        assert!(split_key("/def").is_err());
    }

    #[test]
    fn none_means_null() {
        assert_eq!(nullable(Some("NONE".into()), "none"), Value::Null);
        assert_eq!(nullable(Some("ab".into()), "none"), json!("ab"));
        assert_eq!(nullable(None, "none"), Value::Null);
    }
}
