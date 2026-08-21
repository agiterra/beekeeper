use std::sync::{Mutex, OnceLock};

use serde_json::Value;

const BOOTSTRAP_SCHEMA: &str = "buzz-coding-session-popout-bootstrap/v1";
const MAX_BOOTSTRAPS: usize = 32;
const MAX_BOOTSTRAP_BYTES: usize = 64 * 1024 * 1024;
const MAX_RELAY_EVENTS: usize = 20_000;

#[derive(Default)]
struct BootstrapStore {
    entries: Vec<(String, Value)>,
}

impl BootstrapStore {
    fn stage(&mut self, label: String, bootstrap: Value) -> Result<(), String> {
        validate_label(&label)?;
        validate_bootstrap(&bootstrap)?;
        self.entries
            .retain(|(entry_label, _)| entry_label != &label);
        self.entries.push((label, bootstrap));
        if self.entries.len() > MAX_BOOTSTRAPS {
            self.entries.remove(0);
        }
        Ok(())
    }

    fn get(
        &self,
        label: &str,
        channel_id: &str,
        generation_id: &str,
    ) -> Result<Option<Value>, String> {
        validate_label(label)?;
        let Some((_, bootstrap)) = self
            .entries
            .iter()
            .find(|(entry_label, _)| entry_label == label)
        else {
            return Ok(None);
        };
        if bootstrap.get("channelId").and_then(Value::as_str) != Some(channel_id)
            || bootstrap.get("generationId").and_then(Value::as_str) != Some(generation_id)
        {
            return Ok(None);
        }
        Ok(Some(bootstrap.clone()))
    }
}

fn store() -> &'static Mutex<BootstrapStore> {
    static STORE: OnceLock<Mutex<BootstrapStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(BootstrapStore::default()))
}

fn validate_label(label: &str) -> Result<(), String> {
    if label.starts_with("coding-session-")
        && label.len() <= 80
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Ok(());
    }
    Err("Invalid coding-session pop-out window label.".to_string())
}

fn validate_bootstrap(bootstrap: &Value) -> Result<(), String> {
    let object = bootstrap
        .as_object()
        .ok_or_else(|| "Invalid coding-session pop-out snapshot.".to_string())?;
    if object.get("schema").and_then(Value::as_str) != Some(BOOTSTRAP_SCHEMA)
        || object
            .get("channelId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || object
            .get("generationId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || object
            .get("authorityIdentity")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err("Invalid coding-session pop-out snapshot coordinates.".to_string());
    }
    let relay_events = object
        .get("relayEvents")
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid coding-session pop-out relay snapshot.".to_string())?;
    if relay_events.is_empty() || relay_events.len() > MAX_RELAY_EVENTS {
        return Err("Coding-session pop-out relay snapshot is outside safe limits.".to_string());
    }
    let encoded_bytes = serde_json::to_vec(bootstrap)
        .map_err(|_| "Unable to encode coding-session pop-out snapshot.".to_string())?
        .len();
    if encoded_bytes > MAX_BOOTSTRAP_BYTES {
        return Err("Coding-session pop-out snapshot exceeds the safe size limit.".to_string());
    }
    Ok(())
}

/// Stage one exact accepted signed session snapshot for a fresh native window.
#[tauri::command]
pub fn stage_coding_session_popout_bootstrap(
    label: String,
    bootstrap: Value,
) -> Result<(), String> {
    store()
        .lock()
        .map_err(|_| "Coding-session pop-out snapshot store is unavailable.".to_string())?
        .stage(label, bootstrap)
}

/// Read the snapshot only when both route coordinates match exactly.
#[tauri::command]
pub fn get_coding_session_popout_bootstrap(
    label: String,
    channel_id: String,
    generation_id: String,
) -> Result<Option<Value>, String> {
    store()
        .lock()
        .map_err(|_| "Coding-session pop-out snapshot store is unavailable.".to_string())?
        .get(&label, &channel_id, &generation_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(channel_id: &str, generation_id: &str) -> Value {
        serde_json::json!({
            "schema": BOOTSTRAP_SCHEMA,
            "channelId": channel_id,
            "generationId": generation_id,
            "authorityIdentity": "trusted-authority",
            "relayEvents": [{"id": "signed-event"}],
        })
    }

    #[test]
    fn returns_only_the_exact_staged_route() {
        let mut store = BootstrapStore::default();
        store
            .stage(
                "coding-session-1234abcd".to_string(),
                snapshot("channel-1", "generation-2"),
            )
            .expect("valid snapshot");

        assert!(store
            .get("coding-session-1234abcd", "channel-1", "generation-2")
            .expect("read")
            .is_some());
        assert!(store
            .get("coding-session-1234abcd", "channel-1", "generation-3")
            .expect("read")
            .is_none());
        assert!(store
            .get("coding-session-1234abcd", "channel-2", "generation-2")
            .expect("read")
            .is_none());
    }

    #[test]
    fn replacement_and_capacity_are_bounded() {
        let mut store = BootstrapStore::default();
        for index in 0..=MAX_BOOTSTRAPS {
            store
                .stage(
                    format!("coding-session-{index:08x}"),
                    snapshot("channel-1", &format!("generation-{index}")),
                )
                .expect("valid snapshot");
        }
        assert_eq!(store.entries.len(), MAX_BOOTSTRAPS);
        assert!(store
            .get("coding-session-00000000", "channel-1", "generation-0")
            .expect("read")
            .is_none());
    }
}
