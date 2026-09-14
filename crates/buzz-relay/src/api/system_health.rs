//! `GET /health/system` — the relay's own machine health for the people who
//! run it: CPU, memory and disk from [`crate::system_health`].
//!
//! Authenticated like every other bridge read (NIP-98, host-bound tenant,
//! admission, replay, membership) and then restricted to the community's
//! stewards: an `owner` or `admin` on the roster. An open relay with no
//! steward at all admits any member, the same rule kind:9033 (workspace
//! profile) uses, so a single-person dev relay is not locked out of its own
//! numbers. Everyone else gets a 403 that says so, and a relay whose sampler
//! has not produced a sample yet answers 503 rather than an invented one.

use std::sync::Arc;

use axum::{extract::State, http::HeaderMap, http::StatusCode, Json};
use serde_json::Value;

use crate::api::{api_error, bridge, internal_error};
use crate::state::AppState;
use crate::system_health;

/// The path this handler is mounted on; the NIP-98 `u` tag must name it.
pub const SYSTEM_HEALTH_PATH: &str = "/health/system";

/// Whether `sender_role` may read the machine health. Mirrors the
/// workspace-profile rule: stewards always; anyone on an open relay that has
/// no steward yet.
pub fn may_read_system_health(
    sender_role: &str,
    membership_enforced: bool,
    community_has_steward: bool,
) -> bool {
    if !membership_enforced && !community_has_steward {
        return true;
    }
    sender_role == "admin" || sender_role == "owner"
}

/// A sample plus its age, the shape the wire carries.
pub fn system_health_response(
    snapshot: &system_health::SystemHealthSnapshot,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(snapshot)?;
    if let Value::Object(map) = &mut value {
        map.insert(
            "age_seconds".to_string(),
            Value::from(system_health::age_seconds(snapshot.sampled_at, now)),
        );
    }
    Ok(value)
}

/// `GET /health/system`.
pub async fn get_system_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::NOT_FOUND,
                "relay: no community is configured for this host",
            )
        })?;

    let url = bridge::nip98_expected_url(&state.config.relay_url, &tenant, SYSTEM_HEALTH_PATH);
    let bridge::VerifiedBridgeAuth {
        pubkey,
        event_id_bytes,
        signed_created_at,
    } = bridge::verify_bridge_auth(&headers, "GET", &url, None, state.config.require_auth_token)?;
    bridge::enforce_http_admission(&state, &tenant, &pubkey).await?;
    bridge::check_nip98_replay(&state, &tenant, event_id_bytes).await?;

    let pubkey_bytes = pubkey.to_bytes().to_vec();
    let auth_tag = super::relay_members::extract_auth_tag_header(&headers);
    super::relay_members::enforce_relay_membership(
        &state,
        tenant.community(),
        &pubkey_bytes,
        auth_tag,
        signed_created_at,
    )
    .await?;

    let pubkey_hex = pubkey.to_hex();
    let sender_role = state
        .db
        .get_relay_member(tenant.community(), &pubkey_hex)
        .await
        .map_err(|error| internal_error(&format!("system health role lookup: {error}")))?
        .map(|member| member.role)
        .unwrap_or_default();
    let community_has_steward = if state.config.require_relay_membership {
        true
    } else {
        state
            .db
            .has_admin_or_owner(tenant.community())
            .await
            .map_err(|error| internal_error(&format!("system health steward lookup: {error}")))?
    };
    if !may_read_system_health(
        &sender_role,
        state.config.require_relay_membership,
        community_has_steward,
    ) {
        tracing::info!(pubkey = %pubkey_hex, route = SYSTEM_HEALTH_PATH, status = 403u16, "system health refused");
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "restricted: machine health is for this community's owners and admins",
        ));
    }

    let Some(snapshot) = system_health::latest() else {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "machine health has not been sampled yet",
        ));
    };
    let body = system_health_response(&snapshot, chrono::Utc::now())
        .map_err(|error| internal_error(&format!("system health serialize: {error}")))?;
    tracing::info!(pubkey = %pubkey_hex, route = SYSTEM_HEALTH_PATH, status = 200u16, "system health read");
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_health::*;

    #[test]
    fn stewards_read_and_members_do_not_once_a_steward_exists() {
        assert!(may_read_system_health("owner", true, true));
        assert!(may_read_system_health("admin", true, true));
        assert!(!may_read_system_health("member", true, true));
        assert!(!may_read_system_health("", true, true));
        // Open relay, but somebody holds a steward role: still stewards only.
        assert!(!may_read_system_health("member", false, true));
        assert!(may_read_system_health("admin", false, true));
    }

    #[test]
    fn an_open_relay_with_no_steward_admits_anyone() {
        assert!(may_read_system_health("", false, false));
        assert!(may_read_system_health("member", false, false));
    }

    #[test]
    fn a_closed_relay_never_uses_the_no_steward_exception() {
        assert!(!may_read_system_health("member", true, false));
    }

    #[test]
    fn the_response_is_the_snapshot_plus_its_age() {
        let sampled = chrono::DateTime::parse_from_rfc3339("2026-09-14T20:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snapshot = SystemHealthSnapshot {
            sampled_at: sampled,
            interval_seconds: 10,
            host: HostHealth {
                name: None,
                os: None,
                uptime_seconds: 1,
                relay_uptime_seconds: 1,
            },
            cpu: CpuHealth {
                cores: 1,
                machine_percent: 0.0,
                process_percent: 0.0,
                load_average: None,
            },
            memory: MemoryHealth {
                machine_total_bytes: 1,
                machine_used_bytes: 1,
                machine_available_bytes: 0,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
                process_rss_bytes: 1,
                container: None,
            },
            disks: vec![],
        };
        let body =
            system_health_response(&snapshot, sampled + chrono::Duration::seconds(7)).unwrap();
        assert_eq!(body["age_seconds"], 7);
        assert_eq!(body["sampled_at"], "2026-09-14T20:00:00Z");
        assert_eq!(body["interval_seconds"], 10);
    }
}
