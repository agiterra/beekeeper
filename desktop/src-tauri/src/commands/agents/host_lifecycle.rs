//! Thin lifecycle clients; shared service owns admission, quiescence and retirement.
use super::*;
use beekeeper_agent_management::{DeleteRequest, LifecycleRequest};
use std::collections::HashSet;

fn lifecycle(actor: &str) -> LifecycleRequest {
    LifecycleRequest {
        operation_id: uuid::Uuid::new_v4(),
        agent_pubkey: actor.into(),
        channels: Vec::new(),
        wait_secs: 60,
    }
}

pub(crate) async fn start_host_pair(
    app: &AppHandle,
    actor: &str,
    relay: &str,
) -> Result<(), String> {
    let record = record_for_pair(app, actor, relay)?;
    if record.backend != BackendKind::Local {
        return Err("remote lifecycle uses its authorized provider boundary".into());
    }
    enrollment::start_existing(app, &record, relay).await
}

pub(crate) async fn start_host_pair_with_floor(
    app: &AppHandle,
    actor: &str,
    relay: &str,
    floor: Option<u64>,
) -> Result<(), String> {
    if floor.is_some() {
        return Err("explicit replay floor requires a new CLI responsibility".into());
    }
    let record = record_for_pair(app, actor, relay)?;
    if record.backend != BackendKind::Local {
        return Err("remote lifecycle uses its provider boundary".into());
    }
    enrollment::start_existing(app, &record, relay).await
}

pub(crate) async fn stop_host_pair(
    app: &AppHandle,
    actor: &str,
    relay: &str,
) -> Result<(), String> {
    let record = record_for_pair(app, actor, relay)?;
    if record.backend != BackendKind::Local {
        return Err(
            "remote stop requires an observed relay shutdown; provider has no stop acknowledgment"
                .into(),
        );
    }
    let _guard = client_guard(app, actor, relay).await?;
    let ctx = context()?;
    let intent = stop_intent::pending(&ctx, actor, relay)?;
    let mut request = lifecycle(actor);
    request.operation_id = intent.operation_id;
    let outcome = beekeeper_agent_management::stop(&request, &Transport::owner(app, relay)?, &ctx)
        .await
        .map_err(|e| e.to_string())?;
    if outcome.get("terminal").and_then(Value::as_bool) != Some(true) {
        return Err("shared stop remains pending; retry preserves its operation and cumulative exact targets. No terminal success or custody removal claimed".into());
    }
    stop_intent::complete(&ctx, actor, relay, intent.operation_id)?;
    Ok(())
}

pub(crate) async fn delete_host_agent_snapshot(
    app: &AppHandle,
    _state: &AppState,
    expected: &ManagedAgentRecord,
    relay: &str,
) -> Result<Value, String> {
    let ctx = context()?;
    let _guard = if expected.backend == BackendKind::Local {
        let guard = client_guard(app, &expected.pubkey, relay).await?;
        stop_intent::require_host_ownership(&ctx, &expected.pubkey, relay)?;
        Some(guard)
    } else {
        None
    };
    if load_managed_agents(app)?
        .into_iter()
        .find(|r| r.pubkey == expected.pubkey)
        .is_some_and(|current| current != *expected)
    {
        return Err("agent changed before deletion; intended custody retained".into());
    }
    let intent = deletion::deletion_intent(app, expected, relay).await?;
    let request = DeleteRequest {
        operation_id: intent.operation_id,
        agent_pubkey: expected.pubkey.clone(),
        expected_revision: intent.revision,
        channels: Vec::new(),
        wait_secs: 60,
        remote_deployment: expected.backend != BackendKind::Local
            && expected.backend_agent_id.is_some(),
    };
    let outcome =
        beekeeper_agent_management::delete(&request, &Transport::owner(app, relay)?, &ctx)
            .await
            .map_err(|e| e.to_string())?;
    if outcome.get("terminal").and_then(Value::as_bool) != Some(true) {
        return Err("retirement accepted or requested but execution termination is pending; source custody and cache retained".into());
    }
    Ok(outcome)
}

pub(crate) async fn active_host_agent_pubkeys(
    app: &AppHandle,
    state: &AppState,
) -> Result<HashSet<String>, String> {
    let relay = crate::relay::relay_ws_url_with_override(state);
    let transport = Transport::owner(app, &relay)?;
    let mut active = HashSet::new();
    for record in load_managed_agents(app)? {
        if record.backend != BackendKind::Local {
            continue;
        }
        let effective = crate::relay::effective_agent_relay_url(&record.relay_url, &relay);
        if beekeeper_host_core::record::canonical_relay_key(&effective)
            != beekeeper_host_core::record::canonical_relay_key(&relay)
        {
            continue;
        }
        let outcome = beekeeper_agent_management::status(&lifecycle(&record.pubkey), &transport)
            .await
            .map_err(|e| e.to_string())?;
        if outcome
            .get("responsibilities")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| row.get("runtime_available").and_then(Value::as_bool) == Some(true))
            })
        {
            active.insert(record.pubkey);
        }
    }
    Ok(active)
}
