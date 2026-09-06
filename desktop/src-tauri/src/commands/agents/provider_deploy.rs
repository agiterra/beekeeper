use std::sync::Arc;

use tauri::AppHandle;

use crate::{
    app_state::AppState,
    managed_agents::{
        discover_provider_candidates, load_managed_agents, provider_deploy,
        resolve_provider_binary, save_managed_agents, BackendKind, REPLAY_FLOOR_ENV_VAR,
    },
    util::now_iso,
};

use super::build_deploy_payload;

/// Caller-captured invocation state for a provider wake.
///
/// An ordinary deploy uses [`Self::default`]. Publish-first detached wakes pass
/// the relay and signer that owned the message plus its replay floor, allowing
/// the provider boundary to validate and amend the exact rebuilt payload.
#[derive(Debug, Default)]
pub(crate) struct ProviderDeployOptions<'a> {
    pub(crate) expected_relay_url: Option<&'a str>,
    pub(crate) expected_signer_pubkey: Option<&'a str>,
    pub(crate) replay_floor_unix: Option<u64>,
}

/// Deploy an agent to a provider backend. Resolves the binary, calls deploy via
/// spawn_blocking, and persists the result (backend_agent_id or last_error).
///
/// Idempotency: calling deploy on an already-deployed agent sends the same payload
/// again. Providers are expected to handle this as an update-in-place or no-op.
/// The protocol has no explicit `undeploy` operation or acknowledgement that an
/// existing process stopped, so a successful redeploy delegates access-policy
/// revocation semantics to the provider implementation (deferred to v2).
/// Returns Ok(()) on success, Err(message) on failure. Either way the record is
/// updated and saved before returning.
pub(crate) async fn deploy_to_provider(
    app: &AppHandle,
    state: &AppState,
    pubkey: &str,
    options: ProviderDeployOptions<'_>,
) -> Result<(), String> {
    let deploy_lock = {
        let mut locks = state
            .provider_deploy_locks
            .lock()
            .map_err(|error| error.to_string())?;
        Arc::clone(
            locks
                .entry(pubkey.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    };
    let _deploy_guard = deploy_lock.lock().await;
    // The payload may have waited behind another deployment. Rebuild it from
    // the current record so the final provider invocation always carries the
    // newest saved policy rather than the stale snapshot captured by its caller.
    let (provider_id, config, cached_binary_path, mut agent_json) = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = load_managed_agents(app)?;
        let record = records
            .iter()
            .find(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} not found"))?;
        let (provider_id, config) = match &record.backend {
            BackendKind::Provider { id, config } => (id.clone(), config.clone()),
            BackendKind::Local => return Err(format!("agent {pubkey} is not provider-backed")),
        };
        (
            provider_id,
            config,
            record.provider_binary_path.clone(),
            build_deploy_payload(app, state, record)?,
        )
    };
    // Rebuild after the deploy lock, then assert the caller's captured
    // tenant and signer against the exact payload about to be invoked.
    assert_payload_scope(
        &agent_json,
        options.expected_relay_url,
        options.expected_signer_pubkey,
    )?;
    apply_replay_floor(&mut agent_json, options.replay_floor_unix);

    // Resolve via discovered candidates only. Cached path must match BOTH
    // "is a discovered candidate" AND "belongs to this provider_id". A tampered
    // record cannot redirect deploys to a different provider's binary.
    let bin_path = cached_binary_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists())
        .map(|p| p.canonicalize().unwrap_or(p))
        .filter(|canonical| {
            discover_provider_candidates().iter().any(|(id, cp)| {
                id == &provider_id && cp.canonicalize().ok().as_ref() == Some(canonical)
            })
        })
        .map_or_else(|| resolve_provider_binary(&provider_id), Ok)?;

    let deployed_agent_json = agent_json.clone();
    let config_clone = config.clone();
    let deploy_result =
        tokio::task::spawn_blocking(move || provider_deploy(&bin_path, &agent_json, &config_clone))
            .await
            .map_err(|e| format!("spawn_blocking failed: {e}"))?;

    // Persist result under lock.
    let _store_guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let mut records = load_managed_agents(app)?;
    let rec = records
        .iter_mut()
        .find(|r| r.pubkey == pubkey)
        .ok_or_else(|| format!("agent {pubkey} not found"))?;

    let result = apply_deploy_result(rec, deploy_result, &deployed_agent_json);
    save_managed_agents(app, &records)?;
    result
}

/// Check caller-captured relay and signer against the rebuilt provider payload.
/// Scoped callers fail closed when either payload field is unavailable.
fn assert_payload_scope(
    agent_json: &serde_json::Value,
    expected_relay_url: Option<&str>,
    expected_signer_pubkey: Option<&str>,
) -> Result<(), String> {
    let has_expectation = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
    };
    match agent_json
        .get("relay_url")
        .and_then(serde_json::Value::as_str)
    {
        Some(relay) => crate::relay::assert_expected_relay_scope(
            expected_relay_url,
            &crate::relay::relay_http_base_url(relay),
        )?,
        None if has_expectation(expected_relay_url) => {
            return Err("deploy payload carries no relay; not deployed".to_string());
        }
        None => {}
    }
    match agent_json
        .get("launch")
        .and_then(|launch| launch.get("owner_pubkey"))
        .and_then(serde_json::Value::as_str)
    {
        Some(owner) => crate::relay::assert_expected_signer(expected_signer_pubkey, owner)?,
        None if has_expectation(expected_signer_pubkey) => {
            return Err("deploy payload carries no owner identity; not deployed".to_string());
        }
        None => {}
    }
    Ok(())
}

/// Inject a one-shot publish timestamp into the exact provider launch payload.
/// A persisted `launch.env` floor is removed because it would override
/// `policy_env` in provider launch precedence.
fn apply_replay_floor(agent_json: &mut serde_json::Value, replay_floor_unix: Option<u64>) {
    let Some(floor) = replay_floor_unix else {
        return;
    };
    let Some(launch) = agent_json
        .get_mut("launch")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    if let Some(env) = launch
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
    {
        let keys: Vec<String> = env
            .keys()
            .filter(|key| key.eq_ignore_ascii_case(REPLAY_FLOOR_ENV_VAR))
            .cloned()
            .collect();
        for key in keys {
            env.remove(&key);
        }
    }
    match launch
        .get_mut("policy_env")
        .and_then(serde_json::Value::as_object_mut)
    {
        Some(policy_env) => {
            policy_env.insert(
                REPLAY_FLOOR_ENV_VAR.to_string(),
                serde_json::Value::String(floor.to_string()),
            );
        }
        None => {
            launch.insert(
                "policy_env".to_string(),
                serde_json::json!({ (REPLAY_FLOOR_ENV_VAR): floor.to_string() }),
            );
        }
    }
}

fn policy_matches_payload(
    record: &crate::managed_agents::ManagedAgentRecord,
    deployed_agent_json: &serde_json::Value,
) -> bool {
    deployed_agent_json
        .get("respond_to")
        .and_then(serde_json::Value::as_str)
        == Some(record.respond_to.as_str())
        && deployed_agent_json.get("respond_to_allowlist")
            == Some(&serde_json::json!(record.respond_to_allowlist))
}

fn apply_deploy_result(
    record: &mut crate::managed_agents::ManagedAgentRecord,
    deploy_result: Result<String, String>,
    deployed_agent_json: &serde_json::Value,
) -> Result<(), String> {
    match deploy_result {
        Ok(backend_agent_id) => {
            record.backend_agent_id = Some(backend_agent_id);
            if policy_matches_payload(record, deployed_agent_json) {
                record.provider_policy_pending = false;
            }
            record.last_started_at = Some(now_iso());
            record.updated_at = now_iso();
            record.last_error = None;
            Ok(())
        }
        Err(error) => {
            record.last_error = Some(error.clone());
            record.updated_at = now_iso();
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> crate::managed_agents::ManagedAgentRecord {
        serde_json::from_value(serde_json::json!({
            "pubkey": "agent", "name": "Agent", "relay_url": "", "acp_command": "",
            "agent_command": "", "agent_args": [], "mcp_command": "",
            "turn_timeout_seconds": 0, "system_prompt": null, "created_at": "",
            "updated_at": "", "last_started_at": null, "last_stopped_at": null,
            "last_exit_code": null, "last_error": null,
            "provider_policy_pending": true
        }))
        .unwrap()
    }

    fn policy_payload(respond_to: &str) -> serde_json::Value {
        serde_json::json!({"respond_to": respond_to, "respond_to_allowlist": []})
    }

    fn scoped_payload(relay: &str, owner: &str) -> serde_json::Value {
        serde_json::json!({
            "relay_url": relay,
            "launch": { "owner_pubkey": owner, "env": {}, "policy_env": {} },
        })
    }

    #[test]
    fn rebuilt_provider_payload_rejects_a_stale_relay_or_signer() {
        let relay_error = assert_payload_scope(
            &scoped_payload("wss://tenant-b.example", "aa11"),
            Some("wss://tenant-a.example"),
            Some("aa11"),
        )
        .expect_err("a deploy must not cross a captured tenant boundary");
        assert!(relay_error.contains("active community changed"));

        let signer_error = assert_payload_scope(
            &scoped_payload("wss://tenant-a.example", "bb22"),
            Some("wss://tenant-a.example"),
            Some("aa11"),
        )
        .expect_err("a deploy must not use a replacement identity");
        assert!(signer_error.contains("active identity changed"));
    }

    #[test]
    fn replay_floor_overrides_a_persisted_provider_environment_value() {
        let mut payload = scoped_payload("wss://tenant-a.example", "aa11");
        payload["launch"]["env"][REPLAY_FLOOR_ENV_VAR] = "1".into();

        apply_replay_floor(&mut payload, Some(42));

        assert_eq!(payload["launch"]["policy_env"][REPLAY_FLOOR_ENV_VAR], "42");
        assert!(payload["launch"]["env"][REPLAY_FLOOR_ENV_VAR].is_null());
    }

    #[test]
    fn successful_deploy_acknowledges_pending_policy() {
        let mut record = record();

        apply_deploy_result(
            &mut record,
            Ok("provider-agent".into()),
            &policy_payload("owner-only"),
        )
        .unwrap();

        assert!(!record.provider_policy_pending);
        assert_eq!(record.backend_agent_id.as_deref(), Some("provider-agent"));
        assert_eq!(record.last_error, None);
    }

    #[test]
    fn successful_stale_deploy_preserves_newer_pending_policy() {
        let mut record = record();
        record.respond_to = crate::managed_agents::RespondTo::Anyone;

        apply_deploy_result(
            &mut record,
            Ok("provider-agent".into()),
            &policy_payload("owner-only"),
        )
        .unwrap();

        assert!(record.provider_policy_pending);
    }

    #[test]
    fn failed_deploy_preserves_pending_policy() {
        let mut record = record();

        let error = apply_deploy_result(
            &mut record,
            Err("provider unavailable".into()),
            &policy_payload("owner-only"),
        )
        .expect_err("deployment should fail");

        assert_eq!(error, "provider unavailable");
        assert!(record.provider_policy_pending);
        assert_eq!(record.last_error.as_deref(), Some("provider unavailable"));
    }
}
