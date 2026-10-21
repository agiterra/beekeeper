//! The relay's plugin route (`docs/plugins.md`), for any UI surface that
//! drives a plugin — Planning first.
//!
//! - `relay_plugins_list` — `GET /plugins`.
//! - `relay_plugin_invoke` — `POST /plugins/{name}/invoke` for one operation
//!   on one project.
//!
//! Both sign NIP-98 with the user's keys. Neither signs anything else: a
//! write's unsigned drafts go back to the webview, which signs each one
//! through `sign_event` only after checking it (`shared/plugins`).
//!
//! A relay refusal is returned as data (`ok: false`, the HTTP status and the
//! relay's JSON body), not an error string, because the reason matters to
//! the UI: "this relay runs no Planning plugin" and "you may only read this
//! project" are honest states to draw, not failures to toast.

use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::app_state::AppState;
use crate::relay::{
    build_nip98_auth_header, classify_request_error, relay_api_base_url_with_override,
};

/// One reply from the plugin route.
#[derive(Debug, Serialize)]
pub struct PluginReply {
    pub ok: bool,
    pub status: u16,
    pub body: Value,
}

/// `[a-z0-9-]{1,63}`, the relay's plugin-name rule, so a name can never
/// steer the request to another path.
fn valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

async fn send(
    state: &AppState,
    method: Method,
    path: &str,
    body: Option<Vec<u8>>,
) -> Result<PluginReply, String> {
    crate::relay_admission::wait_for_rate_limit().await;
    let url = format!("{}{path}", relay_api_base_url_with_override(state));
    let bytes = body.unwrap_or_default();
    let auth = build_nip98_auth_header(&method, &url, &bytes, state)?;
    let mut request = state
        .http_client
        .request(method.clone(), &url)
        .header("Authorization", auth);
    if method == Method::POST {
        request = request
            .header("Content-Type", "application/json")
            .body(bytes);
    }
    let response = request
        .send()
        .await
        .map_err(|error| classify_request_error(&error))?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("relay rate-limited: plugin call".to_string());
    }
    let text = response
        .text()
        .await
        .map_err(|error| format!("relay reply unreadable: {error}"))?;
    let body = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "error": text }));
    Ok(PluginReply {
        ok: status.is_success(),
        status: status.as_u16(),
        body,
    })
}

/// `GET /plugins` on the active community's relay.
#[tauri::command]
pub async fn relay_plugins_list(state: State<'_, AppState>) -> Result<PluginReply, String> {
    send(state.inner(), Method::GET, "/plugins", None).await
}

/// `POST /plugins/{plugin}/invoke`.
#[tauri::command]
pub async fn relay_plugin_invoke(
    state: State<'_, AppState>,
    plugin: String,
    operation: String,
    project: String,
    input: Value,
) -> Result<PluginReply, String> {
    if !valid_plugin_name(&plugin) {
        return Err(format!("{plugin:?} is not a plugin name"));
    }
    let body = serde_json::to_vec(&json!({
        "operation": operation,
        "scope": { "project": project },
        "input": input,
    }))
    .map_err(|error| error.to_string())?;
    send(
        state.inner(),
        Method::POST,
        &format!("/plugins/{plugin}/invoke"),
        Some(body),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_names_cannot_carry_a_path() {
        assert!(valid_plugin_name("planning"));
        assert!(valid_plugin_name("planning-fixture"));
        assert!(!valid_plugin_name("../admin"));
        assert!(!valid_plugin_name("a/b"));
        assert!(!valid_plugin_name(""));
    }
}
