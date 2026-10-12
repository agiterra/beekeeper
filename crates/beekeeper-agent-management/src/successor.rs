//! Resume host ownership after a terminal responsibility using verified relay facts.
use super::*;
use beekeeper_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, ReceiptStatus,
};
use beekeeper_host_core::ownership::{self, Phase, Transfer};
use std::collections::BTreeMap;

pub(super) fn prepare(
    home: &Path,
    config: &HostConfig,
    actor: Option<&str>,
    operation: Uuid,
) -> Result<Option<Transfer>, CliError> {
    let Some(actor) = actor else { return Ok(None) };
    let Some(current) =
        ownership::for_pair(home, config.instance, actor, &config.relay_url).map_err(fail)?
    else {
        return Ok(None);
    };
    match current.phase {
        Phase::Adopted => Ok(None),
        Phase::HostStopped if current.operation_id != operation.to_string() => Ok(Some(current)),
        _ => Err(CliError::Refused("existing participant ownership is not available for a new responsibility; original transfer preserved".into())),
    }
}

#[derive(Deserialize)]
struct Snapshot {
    sessions: BTreeMap<String, beekeeper_session_provider::state::SessionRecord>,
}

fn stopped_receipt(
    rows: &[Value],
    channel: &str,
    provider: &str,
    target: &CodingSessionTarget,
) -> bool {
    rows.iter()
        .filter_map(|row| row.get("content").and_then(Value::as_str))
        .filter_map(|content| decode_coding_session_lifecycle_receipt(content).ok())
        .any(|candidate| {
            crate::receipts::verified_command_receipts(
                rows,
                channel,
                &candidate.command_id,
                provider,
            )
            .into_iter()
            .any(|(_, held)| {
                held.status == ReceiptStatus::Stopped && held.session.as_ref() == Some(target)
            })
        })
}

pub(super) async fn require_prior_stops(
    config: &HostConfig,
    provider: &record::CodingSessionProviderRecord,
    prior: &Transfer,
    operation: Uuid,
    accepted: Option<&CodingSessionTarget>,
    client: &impl RelayTransport,
) -> Result<(), CliError> {
    if prior.target.is_none() {
        let value: Value = serde_json::from_slice(
            &std::fs::read(
                config
                    .provider_state_dir
                    .join("actors")
                    .join(&prior.actor_pubkey)
                    .join("enrollment.json"),
            )
            .map_err(fail)?,
        )
        .map_err(fail)?;
        if value.get("actorPubkey").and_then(Value::as_str) != Some(&prior.actor_pubkey) {
            return Err(CliError::Auth(
                "configured enrollment scope mismatch".into(),
            ));
        }
        let channel = value
            .get("channelId")
            .and_then(Value::as_str)
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or_else(|| fail("configured enrollment channel missing"))?;
        return crate::lifecycle::require_configured_stops(
            &prior.actor_pubkey,
            channel,
            accepted,
            client,
        )
        .await;
    }
    let snapshot: Snapshot = serde_json::from_slice(
        &std::fs::read(config.provider_state_dir.join("state.json")).map_err(fail)?,
    )
    .map_err(|_| {
        fail("provider responsibility inventory is malformed; successor was not adopted")
    })?;
    let old = prior
        .target
        .as_ref()
        .ok_or_else(|| fail("stopped ownership has no exact target"))?;
    let actor_records: Vec<_> = snapshot
        .sessions
        .values()
        .filter(|record| record.actor.as_deref() == Some(prior.actor_pubkey.as_str()))
        .collect();
    let prior_record = actor_records.iter().find(|record| record.target(&provider.instance_id) == *old)
        .ok_or_else(|| CliError::Refused("stopped ownership target is absent from provider inventory; reconcile its exact generation before restarting".into()))?;
    let mut targets = vec![(prior_record.channel_id.to_string(), old.clone())];
    for record in actor_records {
        let target = record.target(&provider.instance_id);
        if record.command_id == operation.to_string() {
            if accepted.is_some_and(|accepted| accepted != &target) {
                return Err(CliError::Conflict(
                    "accepted successor differs from persisted provider generation".into(),
                ));
            }
            continue;
        }
        if target != *old {
            targets.push((record.channel_id.to_string(), target));
        }
    }
    let mut channels = BTreeMap::new();
    for (channel, target) in targets {
        if !channels.contains_key(&channel) {
            let rows = client
                .query_all(
                    json!({"kinds":[44224],"authors":[config.provider_pubkey],"#h":[channel]}),
                )
                .await?;
            channels.insert(channel.clone(), rows);
        }
        if !stopped_receipt(
            &channels[&channel],
            &channel,
            &config.provider_pubkey,
            &target,
        ) {
            return Err(CliError::Refused(format!("responsibility {} generation {} lacks a verified stopped receipt; stop every responsibility before restarting", target.session_id, target.generation)));
        }
    }
    Ok(())
}

pub(super) async fn reconcile(
    home: &Path,
    config: &HostConfig,
    provider: &record::CodingSessionProviderRecord,
    operation: &Operation,
    operation_id: Uuid,
    boundary: Option<&Transfer>,
    client: &impl RelayTransport,
) -> Result<(), CliError> {
    // The client already holds management; worker startup has released its pair
    // lock before publishing the accepted create receipt. Never reverse this order.
    let _admission = ownership::admission_lock(
        home,
        config.instance,
        &operation.actor_pubkey,
        &config.relay_url,
    )
    .map_err(fail)?;
    let current = ownership::for_pair(
        home,
        config.instance,
        &operation.actor_pubkey,
        &config.relay_url,
    )
    .map_err(fail)?;
    let target = operation
        .target
        .as_ref()
        .ok_or_else(|| fail("successor has no verified accepted target"))?;
    let revision = operation
        .events
        .first()
        .ok_or_else(|| fail("successor has no signed definition"))?
        .id
        .to_hex();
    match current {
        None if boundary.is_none() => Ok(()),
        Some(current) if current.phase == Phase::Adopted && current.operation_id == operation_id.to_string() => {
            if current.target.as_ref() != Some(target) || current.definition_event_id != revision {
                return Err(CliError::Conflict("successor retry conflicts with adopted target or revision".into()));
            }
            Ok(())
        }
        Some(current) if current.phase == Phase::Adopted && boundary.is_none() => Ok(()),
        Some(current) if current.phase == Phase::HostStopped && boundary == Some(&current) => {
            require_prior_stops(config, provider, &current, operation_id, Some(target), client).await?;
            ownership::successor(home, config.instance, &operation.actor_pubkey, &config.relay_url, &operation_id.to_string(), &revision, target.clone()).map_err(fail)
        }
        _ => Err(CliError::Conflict("ownership changed during successor admission; accepted operation retained for reconciliation".into())),
    }
}

#[cfg(test)]
#[path = "successor_tests.rs"]
mod tests;
