//! Protocol adapters supporting the single management implementation.
use crate::{Error, RelayTransport};
use nostr::Event;
use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn validate_hex64(value: &str) -> Result<(), Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Usage(
            "expected 64 lowercase hexadecimal characters".into(),
        ));
    }
    nostr::PublicKey::from_hex(value)
        .map(|_| ())
        .map_err(|_| Error::Usage("invalid public identity or event ID".into()))
}
pub(crate) fn to_ws_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.into()
    }
}
pub(crate) fn parse_write_response(value: &Value, conflict: &str) -> Result<(), Error> {
    if value.get("accepted").and_then(Value::as_bool) != Some(true) {
        return Err(Error::Refused(
            value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("relay refused operation")
                .into(),
        ));
    }
    if value
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(|v| v == "duplicate" || v.starts_with("duplicate:"))
    {
        return Err(Error::Conflict(conflict.into()));
    }
    Ok(())
}
pub(crate) fn bind_pending_directory(
    path: &Path,
    operation: &str,
    cwd: &Path,
    project: Option<&str>,
) -> Result<(), Error> {
    if operation.is_empty()
        || !operation
            .chars()
            .all(|v| v.is_ascii_alphanumeric() || v == '-')
    {
        return Err(Error::Usage("invalid pending operation".into()));
    }
    let cwd = cwd
        .canonicalize()
        .map_err(|_| Error::Usage("workspace unavailable".into()))?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Usage("projects path has no parent".into()))?;
    let hints = parent.join(beekeeper_host_core::project::PENDING_HINTS_DIR);
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&hints).map_err(Error::Other)?;
    let hint = json!({"commandId":operation,"path":cwd,"projectRef":project,"writtenAt":chrono::Utc::now().timestamp()});
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &hints.join(format!("{operation}.json")),
        &serde_json::to_vec(&hint).map_err(|e| Error::Other(e.to_string()))?,
    )
    .map_err(Error::Other)
}
pub(crate) fn head_is_private(event: &Value) -> bool {
    event
        .get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| {
            tags.iter().any(|tag| {
                tag.as_array().is_some_and(|v| {
                    v.first().and_then(Value::as_str)
                        == Some(beekeeper_core::kind::PROJECT_ACCESS_TAG)
                        && v.get(1).and_then(Value::as_str)
                            == Some(beekeeper_core::kind::PROJECT_ACCESS_PRIVATE)
                })
            })
        })
}
pub(crate) async fn project_roster(
    client: &impl RelayTransport,
    coordinate: &str,
) -> Result<Vec<(String, String)>, Error> {
    let creator = coordinate
        .split(':')
        .nth(1)
        .ok_or_else(|| Error::Usage("invalid project".into()))?;
    let mut rows=client.query_all(json!({"kinds":[beekeeper_core::kind::KIND_PROJECT_MEMBERS],"#d":[coordinate],"limit":1})).await?;
    if rows.is_empty() {
        let slug = coordinate
            .splitn(3, ':')
            .nth(2)
            .ok_or_else(|| Error::Usage("invalid project".into()))?;
        rows = client
            .query_all(json!({"kinds":[30621],"authors":[creator],"#d":[slug],"limit":1}))
            .await?;
    }
    let mut events: Vec<Event> = rows
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .filter(|v| beekeeper_core::verify_event(v).is_ok())
        .collect();
    events.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let head = events
        .first()
        .ok_or_else(|| Error::Refused("project membership unavailable".into()))?;
    let mut roster = vec![(creator.into(), "owner".into())];
    for tag in head.tags.iter() {
        let t = tag.as_slice();
        if t.first().is_some_and(|v| v == "p") {
            if let (Some(key), Some(role)) = (t.get(1), t.get(3)) {
                roster.push((key.clone(), role.clone()))
            }
        }
    }
    Ok(roster)
}
