//! The person's grant of full access to this computer, per coding session.
//!
//! The provider reads `full-access.json` in its own state directory at every
//! session preparation and starts a listed session's agent without the project
//! boundary (`beekeeper_session_provider::full_access`, ledger 303). These commands
//! are the only writer: the person flips the switch here, and the session's
//! next start — a create, or the restart the UI sends after a change — runs
//! under the new answer.
//!
//! Locality first, as for the redaction vault: the grant is this computer's,
//! so a session whose provider is not this desktop's is never read or written
//! (`None` from the read, an error from the write).

use beekeeper_session_provider_pkg::full_access::FULL_ACCESS_FILE;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::relay::relay_ws_url_with_override;

use super::store::load_provider_store;

/// Longest session id accepted: a UUID is 36 bytes.
const MAX_SESSION_ID_BYTES: usize = 128;

/// Whether `session_id` has full access on this computer; `None` when the
/// session's provider is not this computer's, so there is nothing to say.
#[tauri::command]
pub async fn coding_session_full_access(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_pubkey: String,
    session_id: String,
) -> Result<Option<bool>, String> {
    let Some(dir) = local_state_dir(&app, &state, &provider_pubkey)? else {
        return Ok(None);
    };
    check_session_id(&session_id)?;
    Ok(Some(read(&dir).iter().any(|id| id == &session_id)))
}

/// Grant or withdraw full access for `session_id` on this computer.
///
/// Takes effect at the session's next start; the caller restarts a running
/// session so the answer is in force at once.
#[tauri::command]
pub async fn set_coding_session_full_access(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_pubkey: String,
    session_id: String,
    granted: bool,
) -> Result<(), String> {
    let dir = local_state_dir(&app, &state, &provider_pubkey)?
        .ok_or("full access can only be granted to a session running on this computer")?;
    check_session_id(&session_id)?;
    let mut sessions = read(&dir);
    sessions.retain(|id| id != &session_id);
    if granted {
        sessions.push(session_id);
    }
    write(&dir, &sessions)
}

/// This desktop's provider state directory, when `provider_pubkey` is the
/// provider it provisioned for the active relay.
fn local_state_dir(
    app: &AppHandle,
    state: &State<'_, AppState>,
    provider_pubkey: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    let relay_url = relay_ws_url_with_override(state);
    let store = load_provider_store(app)?;
    let local = store
        .get(&relay_url)
        .is_some_and(|record| record.provider_pubkey == provider_pubkey);
    if !local {
        return Ok(None);
    }
    crate::session_provider::provider_state_dir(app, provider_pubkey).map(Some)
}

fn check_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id.len() > MAX_SESSION_ID_BYTES
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("not a coding session id".into());
    }
    Ok(())
}

/// The listed sessions; an absent or unreadable file lists none, which is
/// also how the provider reads it.
fn read(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(FULL_ACCESS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|value| value.get("version").and_then(serde_json::Value::as_u64) == Some(1))
        .and_then(|value| {
            value.get("sessions").and_then(|sessions| {
                sessions.as_array().map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.as_str().map(str::to_owned))
                        .collect()
                })
            })
        })
        .unwrap_or_default()
}

/// Replace the file whole, so the provider never reads half of it.
fn write(dir: &std::path::Path, sessions: &[String]) -> Result<(), String> {
    let body = serde_json::json!({ "version": 1, "sessions": sessions }).to_string();
    let target = dir.join(FULL_ACCESS_FILE);
    let staged = dir.join(format!("{FULL_ACCESS_FILE}.tmp"));
    std::fs::write(&staged, body)
        .map_err(|error| format!("could not write the full-access grants: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&staged, &target)
        .map_err(|error| format!("could not save the full-access grants: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_round_trip_and_the_provider_reads_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path();
        assert!(read(state).is_empty());
        write(state, &["s-1".into(), "s-2".into()]).expect("write");
        assert_eq!(read(state), vec!["s-1".to_string(), "s-2".to_string()]);
        // The file this writes is the file the provider grants from.
        assert!(beekeeper_session_provider_pkg::full_access::granted(
            state, "s-1"
        ));
        assert!(!beekeeper_session_provider_pkg::full_access::granted(
            state, "s-3"
        ));
        write(state, &["s-2".into()]).expect("withdraw");
        assert!(!beekeeper_session_provider_pkg::full_access::granted(
            state, "s-1"
        ));
    }

    #[test]
    fn only_a_plain_session_id_is_accepted() {
        assert!(check_session_id("8f0c1d2e-aaaa-4bbb-8ccc-0123456789ab").is_ok());
        for bad in ["", "../x", "a b", "a/b", &"x".repeat(200)] {
            assert!(check_session_id(bad).is_err(), "{bad}");
        }
    }
}
