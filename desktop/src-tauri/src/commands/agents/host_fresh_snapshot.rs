//! Fresh snapshot-mint custody callbacks, never legacy identity conversion.
use super::*;

/// Reserve a newly minted snapshot identity before writing its record.
pub(crate) fn reserve_fresh_snapshot(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let relay = crate::relay::effective_agent_relay_url(
        &record.relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
    );
    reserve_fresh_host_agent(app, record, &relay, false)
}
/// Materialize stopped host custody and disclose partial setup alongside profile errors.
pub(crate) async fn enroll_fresh_snapshot(
    app: &AppHandle,
    state: &AppState,
    record: &ManagedAgentRecord,
    profile_error: Option<String>,
) -> Option<String> {
    let relay = crate::relay::effective_agent_relay_url(
        &record.relay_url,
        &crate::relay::relay_ws_url_with_override(state),
    );
    match enroll_new_host_agent(app, state, &record.pubkey, &relay, false).await {
        Ok(()) => profile_error,
        Err(error) => Some(format!(
            "Fresh host enrollment unconfirmed; identity retained: {error}{}",
            profile_error
                .as_ref()
                .map(|e| format!("; profile: {e}"))
                .unwrap_or_default()
        )),
    }
}

/// Refuse tracked old-owner conflicts before a new snapshot identity is minted.
pub(crate) fn preflight_fresh_snapshot(app: &AppHandle) -> Result<(), String> {
    let relay = crate::relay::relay_ws_url_with_override(&app.state::<AppState>());
    require_clean_setup(app, &relay)
}
