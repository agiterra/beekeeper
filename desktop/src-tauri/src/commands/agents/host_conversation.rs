//! Channel-bound signing adapter; shared management and the worker own execution.
use super::*;

fn prior_stopped(outcome: &Value, observed: &Value, channel: Uuid, provider: &str) -> bool {
    let targets: Vec<Value> = if let Some(target) = outcome.get("target") {
        vec![target.clone()]
    } else {
        outcome["responsibilities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| {
                row["channelId"] == channel.to_string() && row["providerAuthority"] == provider
            })
            .map(|row| row["target"].clone())
            .collect()
    };
    !targets.is_empty()
        && targets.iter().all(|target| {
            observed["responsibilities"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|row| {
                    row["responsibility"]["target"] == *target
                        && row["responsibility"]["channelId"] == channel.to_string()
                        && row["responsibility"]["providerAuthority"] == provider
                        && row["terminalStopped"] == true
                })
        })
}

pub(crate) async fn start_conversation(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
    channel: Uuid,
) -> Result<(), String> {
    let guard = client_guard(app, &record.pubkey, relay).await?;
    let ctx = context()?;
    let held = ownership::for_pair(&ctx.home, ctx.instance, &record.pubkey, relay)?;
    // Only finish a pending original fresh reservation. Never demand or restart
    // an unrelated primary context to enable an already-enrolled conversation.
    if fresh_pending(app, &record.pubkey, relay, held.as_ref())? {
        let reservation: FreshReservation = serde_json::from_slice(&private_bytes(
            &pair_directory(&record.pubkey, relay)?.join("fresh-reservation.json"),
        )?)
        .map_err(|_| "fresh reservation malformed")?;
        drop(guard);
        enroll_new_host_agent(
            app,
            &app.state::<AppState>(),
            &record.pubkey,
            relay,
            reservation.activate,
        )
        .await?;
        return Box::pin(start_conversation(app, record, relay, channel)).await;
    }
    require_modern(app, record, relay)?;
    let directory = pair_directory(&record.pubkey, relay)?.join(format!("conversation-{channel}"));
    let enrolled: Enrollment = serde_json::from_slice(&private_bytes(
        &pair_directory(&record.pubkey, relay)?.join("create.json"),
    )?)
    .map_err(|_| "modern enrollment receipt unavailable")?;
    let transport = Transport::owner(app, relay)?;
    let current = head(app, relay, &record.pubkey).await?;
    let observed = beekeeper_agent_management::status(
        &beekeeper_agent_management::LifecycleRequest {
            agent_pubkey: record.pubkey.clone(),
            channels: vec![channel],
            ..Default::default()
        },
        &transport,
    )
    .await
    .map_err(|e| e.to_string())?;
    let path = directory.join("pending.json");
    let prior: Option<RemoteStartRequest> = if path.exists() {
        Some(
            serde_json::from_slice(&private_bytes(&path)?)
                .map_err(|_| "conversation start intent malformed")?,
        )
    } else {
        None
    };
    if let Some(request) = &prior {
        if request.agent_pubkey != record.pubkey
            || request.channel != channel
            || request.provider_authority != enrolled.provider_authority
            || request.provider_instance != enrolled.provider_instance
            || request.project != record.project_ref
        {
            return Err("pending conversation scope changed; original operation retained".into());
        }
        let outcome_path = directory.join(format!("outcome-{}.json", request.operation_id));
        if outcome_path.exists() {
            let outcome: Value = serde_json::from_slice(&private_bytes(&outcome_path)?)
                .map_err(|_| "conversation outcome malformed")?;
            // A confirmed live execution can be observed after a settings
            // revision changes. This path never publishes a new Create or
            // rewrites the old revision, and does not claim config application.
            if beekeeper_agent_management::observe_remote_start(request, &outcome, &transport)
                .await
                .map_err(|e| e.to_string())?
                .is_some_and(|observation| observation["runtime_available"] == true)
            {
                return Ok(());
            }
        }
    }
    let advance = if let Some(request) = &prior {
        let outcome_path = directory.join(format!("outcome-{}.json", request.operation_id));
        if outcome_path.exists() {
            let outcome: Value = serde_json::from_slice(&private_bytes(&outcome_path)?)
                .map_err(|_| "conversation outcome malformed")?;
            prior_stopped(&outcome, &observed, channel, &request.provider_authority)
        } else {
            false
        }
    } else {
        false
    };
    // A missing reply, target or metadata never advances the pending operation.
    let request = match prior.filter(|_| !advance) {
        Some(request) => request,
        None => {
            let request = RemoteStartRequest {
                operation_id: Uuid::new_v4(),
                agent_pubkey: record.pubkey.clone(),
                expected_revision: current.id.to_hex(),
                channel,
                provider_authority: enrolled.provider_authority,
                provider_instance: enrolled.provider_instance,
                project: record.project_ref.clone(),
                wait_secs: 60,
            };
            save(
                &directory.join(format!("request-{}.json", request.operation_id)),
                &request,
            )?;
            save(&path, &request)?;
            request
        }
    };
    if request.agent_pubkey != record.pubkey
        || request.channel != channel
        || request.expected_revision != current.id.to_hex()
    {
        return Err(
            "pending conversation start scope/revision changed; original operation retained".into(),
        );
    }
    let outcome = beekeeper_agent_management::start_remote(&request, &transport, &ctx)
        .await
        .map_err(|e| e.to_string())?;
    save(
        &directory.join(format!("outcome-{}.json", request.operation_id)),
        &outcome,
    )?;
    if outcome["runtime_available"] != true {
        return Err("conversation responsibility admission is pending; original messages remain in their channel. Retry Enable conversation here without resending".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_or_unproved_targets_never_advance_conversation_start() {
        let channel = Uuid::new_v4();
        let outcome = json!({"target":{"sessionId":"exact","generation":1}});
        assert!(!prior_stopped(
            &outcome,
            &json!({"responsibilities":[]}),
            channel,
            "provider"
        ));
        let row = json!({"responsibility":{"target":outcome["target"],"channelId":channel,"providerAuthority":"provider"},"terminalStopped":false});
        assert!(!prior_stopped(
            &outcome,
            &json!({"responsibilities":[row]}),
            channel,
            "provider"
        ));
        let row = json!({"responsibility":{"target":outcome["target"],"channelId":channel,"providerAuthority":"provider"},"terminalStopped":true});
        assert!(prior_stopped(
            &outcome,
            &json!({"responsibilities":[row.clone()]}),
            channel,
            "provider"
        ));
        assert!(!prior_stopped(
            &outcome,
            &json!({"responsibilities":[row]}),
            Uuid::new_v4(),
            "provider"
        ));
        assert!(!prior_stopped(
            &json!({"pending":true}),
            &json!({"responsibilities":[]}),
            channel,
            "provider"
        ));
    }
}
