//! Conditional setup/reopen refresh of existing accepted actor settings.
//! Never enroll, start, stop or restart an execution.
use super::*;

async fn refresh_one(
    app: &AppHandle,
    state: &AppState,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<(), String> {
    let ctx = context()?;
    let transport = Transport::owner(app, relay)?;
    let snapshot = beekeeper_agent_management::private_configuration_snapshot(
        &ctx,
        &transport,
        &record.pubkey,
    )
    .map_err(|error| error.to_string())?;
    let head = head(app, relay, &record.pubkey).await?;
    let (public, mut private) = configuration_view(app, record)?;
    let head_content: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&head.content).map_err(|_| "current accepted definition malformed")?;
    if head_content.public_projection() != public {
        return Err("current owner revision differs from local settings; refresh refused".into());
    }

    {
        let _source = state
            .managed_agents_store_lock
            .lock()
            .map_err(|_| "client source lock unavailable")?;
        if record_for_pair(app, &record.pubkey, relay)? != *record {
            return Err("client source changed before accepted refresh".into());
        }
    }
    private_persona::attach_private_persona(app, state, record, relay, &mut private)?;
    if snapshot.private.record != private.record {
        return Err("current effective private configuration differs from accepted settings; use explicit Configure, automatic compatibility conversion is unsupported".into());
    }

    let delivery_path = configuration_path(
        &ctx,
        &record.pubkey,
        relay,
        &configuration_hash(&public, &private)?,
        true,
    )?;
    let pending_delivery = read_intent(&delivery_path)?;
    if pending_delivery.is_none()
        && snapshot.definition.id == head.id
        && snapshot.private.record == private.record
    {
        if snapshot.enrollment_definition.id != snapshot.definition.id {
            return Err("private materialization is incomplete without its original refresh delivery; retry the original configuration operation".into());
        }
        return Ok(());
    }
    let (path, mut intent) = configuration_operation(
        app,
        state,
        &record.pubkey,
        relay,
        &public,
        &private,
        (Some(record), Some(&snapshot.snapshot_hash)),
    )
    .await?;
    let expected_hash = intent
        .expected_private_snapshot_hash
        .as_deref()
        .ok_or("accepted refresh lost its immutable private snapshot guard")?;
    if record_for_pair(app, &record.pubkey, relay)? != *record {
        return Err("client source changed before conditional refresh delivery".into());
    }
    let request = UpdateRequest {
        operation_id: intent.operation_id,
        agent_pubkey: record.pubkey.clone(),
        expected_revision: intent.expected_revision.clone(),
        display_name: None,
        system_prompt: None,
        model: None,
        home: Some(ctx.home.clone()),
        public_definition: Some(public),
        private_config: Some(private),
    };
    let outcome = beekeeper_agent_management::update_if_private_unchanged(
        &request,
        &transport,
        &ctx,
        expected_hash,
    )
    .await
    .map_err(|error| error.to_string())?;
    if outcome["relay_accepted"] != true {
        return Err("accepted refresh publication unconfirmed".into());
    }
    intent.accepted_revision = Some(
        outcome["effectiveRevision"]
            .as_str()
            .ok_or("accepted refresh revision missing")?
            .into(),
    );
    finish_refresh_delivery(
        &path,
        &mut intent,
        acknowledge_revision(
            app,
            state,
            relay,
            &record.pubkey,
            outcome["effectiveRevision"]
                .as_str()
                .ok_or("accepted refresh revision missing")?,
        ),
    )
    .await
}

async fn finish_refresh_delivery(
    path: &Path,
    intent: &mut ConfigurationIntent,
    acknowledgment: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    // Accepted relay publication does not imply client retention or executor
    // application. Persist the retry handle before attempting acknowledgment.
    intent.complete = false;
    save_intent(path, intent)?;
    acknowledgment.await?;
    intent.complete = true;
    save_intent(path, intent)
}

pub(crate) async fn refresh_accepted_host_agents(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let relay = crate::relay::relay_ws_url_with_override(&state);
    state.signing_keys()?;
    let ctx = context()?;
    let mut errors = Vec::new();
    for record in load_managed_agents(app)? {
        if record.backend != BackendKind::Local || record.pubkey.is_empty() {
            continue;
        }
        let effective = crate::relay::effective_agent_relay_url(&record.relay_url, &relay);
        if beekeeper_host_core::record::canonical_relay_key(&effective)
            != beekeeper_host_core::record::canonical_relay_key(&relay)
        {
            continue;
        }
        let Some(owned) = ownership::for_pair(&ctx.home, ctx.instance, &record.pubkey, &relay)?
        else {
            continue;
        };
        if !ownership::is_modern_enrollment(&owned)
            || !matches!(
                owned.phase,
                ownership::Phase::Adopted | ownership::Phase::HostStopped
            )
        {
            continue;
        }
        if let Err(error) = refresh_one(app, &state, &record, &relay).await {
            errors.push(format!("{}: {error}", record.pubkey));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "accepted settings refresh remains pending: {}",
            errors.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lost_refresh_ack_preserves_original_delivery_until_exact_retry_completes() {
        let dir = tempfile::tempdir().expect("isolated client receipt");
        let path = dir.path().join("refresh.json");
        let mut intent = ConfigurationIntent {
            operation_id: Uuid::new_v4(),
            owner_pubkey: "ab".repeat(32),
            expected_revision: "cd".repeat(32),
            expected_private_snapshot_hash: Some("ef".repeat(32)),
            desired_hash: "12".repeat(32),
            was_active: false,
            quiesced: false,
            accepted_revision: Some("34".repeat(32)),
            restarted: false,
            complete: true,
        };
        let operation = intent.operation_id;
        assert!(finish_refresh_delivery(&path, &mut intent, async {
            Err("accepted reply retention lost".into())
        })
        .await
        .is_err());
        let mut recovered = read_intent(&path)
            .expect("read protected delivery")
            .expect("original delivery retained");
        assert!(!recovered.complete);
        assert_eq!(recovered.operation_id, operation);
        assert_eq!(
            recovered.expected_private_snapshot_hash,
            Some("ef".repeat(32))
        );
        assert_eq!(recovered.accepted_revision, Some("34".repeat(32)));
        finish_refresh_delivery(&path, &mut recovered, async { Ok(()) })
            .await
            .expect("exact acknowledgment retry");
        let completed = read_intent(&path).expect("read").expect("retained");
        assert!(completed.complete);
        assert_eq!(completed.operation_id, operation);
        assert!(!completed.was_active && !completed.restarted && !completed.quiesced);
    }
}
