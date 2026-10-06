//! `get_relay_system_health`: the active relay's own machine health (CPU,
//! memory, disk) from `GET /health/system`, for the Dashboard's relay card.
//!
//! The relay answers only its community's stewards (owner or admin; any
//! member on an open relay with no steward yet) and says 403 to everyone
//! else — the TS side reads the `relay returned 403` prefix from
//! [`crate::relay::relay_error_message`] and hides the card. The body is
//! passed through untyped: the relay's `system_health` module owns the
//! shape and the webview validates it (`features/dashboard/lib/relaySystemHealth.ts`),
//! so a field the relay adds later reaches the UI without a Rust change.

use serde_json::Value;
use tauri::State;

use crate::app_state::AppState;
use crate::relay::get_relay_json;

/// The path on the relay; kept beside the relay's own constant of the same
/// name (`beekeeper_relay::api::system_health::SYSTEM_HEALTH_PATH`).
pub const RELAY_SYSTEM_HEALTH_PATH: &str = "/health/system";

/// Read the active relay's machine health as the signed-in identity.
#[tauri::command]
pub async fn get_relay_system_health(state: State<'_, AppState>) -> Result<Value, String> {
    get_relay_json::<Value>(&state, RELAY_SYSTEM_HEALTH_PATH).await
}
