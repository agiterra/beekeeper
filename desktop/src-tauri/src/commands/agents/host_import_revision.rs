//! Bounded revision repair for a proven existing standalone-definition fold.
use super::*;

fn original(staged: &ImportStage) -> Result<ownership::Transfer, String> {
    Ok(ownership::Transfer {
        operation_id: staged.operation_id.to_string(),
        actor_pubkey: staged.record.pubkey.clone(),
        relay_url: staged.relay_url.clone(),
        legacy_nonce: staged
            .generation
            .as_ref()
            .map(|generation| generation.nonce.clone())
            .ok_or("original legacy generation proof unavailable")?,
        definition_event_id: staged
            .definition_revision
            .clone()
            .ok_or("original definition revision unavailable")?,
        target: None,
        phase: ownership::Phase::Quiesced,
    })
}

pub(super) fn capture(app: &AppHandle, staged: &ImportStage, event: &Event) -> Result<(), String> {
    let ctx = context()?;
    let transport = Transport::owner(app, &staged.relay_url)?;
    let owner = transport.keys().public_key();
    if !source_compare::verified_equivalent_heads(
        &staged.record,
        &staged.record,
        event,
        event,
        &owner,
    ) {
        return Err("original signed definition does not match protected source configuration; source remains unadmitted".into());
    }
    let _management = ownership::management_lock(&ctx.home, ctx.instance)?;
    let _pair = ownership::admission_lock(
        &ctx.home,
        ctx.instance,
        &staged.record.pubkey,
        &staged.relay_url,
    )?;
    beekeeper_agent_management::supply_import_definition_evidence(
        &ctx,
        &original(staged)?,
        &owner,
        event,
    )
    .map_err(|error| error.to_string())
}

pub(super) async fn ensure_source_evidence(
    app: &AppHandle,
    staged: &ImportStage,
) -> Result<(), String> {
    let ctx = context()?;
    let expected = original(staged)?;
    let transport = Transport::owner(app, &staged.relay_url)?;
    let owner = transport.keys().public_key();
    if beekeeper_agent_management::read_import_definition_evidence(&ctx, &expected, &owner)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let events = transport
        .query_all(
            json!({"kinds":[30177], "ids":[expected.definition_event_id],
        "authors":[owner.to_hex()], "#d":[expected.actor_pubkey]}),
        )
        .await
        .map_err(|error| error.to_string())?;
    let event = events.into_iter().filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .find(|event| event.id.to_hex() == expected.definition_event_id)
        .ok_or("original signed definition evidence unavailable; supply the exact original event using agents supply-migration-definition before stopping the source")?;
    capture(app, staged, &event)
}

pub(super) async fn reconcile(app: &AppHandle, staged: &ImportStage) -> Result<(), String> {
    let ctx = context()?;
    let expected = original(staged)?;
    let transport = Transport::owner(app, &staged.relay_url)?;
    let owner = transport.keys().public_key();
    let new = head(app, &staged.relay_url, &staged.record.pubkey).await?;
    if new.id.to_hex() == expected.definition_event_id {
        return Ok(());
    }
    let old = beekeeper_agent_management::read_import_definition_evidence(&ctx, &expected, &owner)
        .map_err(|error| error.to_string())?
        .ok_or("original signed definition evidence unavailable; supply the exact original event using agents supply-migration-definition, then retry the same migration")?;
    let _management = ownership::management_lock(&ctx.home, ctx.instance)?;
    let _pair = ownership::admission_lock(
        &ctx.home,
        ctx.instance,
        &staged.record.pubkey,
        &staged.relay_url,
    )?;
    let state = app.state::<AppState>();
    let _source = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| "source settings lock unavailable")?;
    let current = record_for_pair(app, &staged.record.pubkey, &staged.relay_url)?;
    let (_, private) = configuration_view(app, &current)?;
    let (_, fallback) = configuration_view(app, &staged.record)?;
    let personas = load_personas(app)?;
    if !source_compare::proven_backfill(
        &staged.record,
        &current,
        staged.source_configuration.as_ref().unwrap_or(&fallback),
        &private,
        &personas,
    ) || !source_compare::verified_equivalent_heads(&staged.record, &current, &old, &new, &owner)
    {
        return Err("definition revision changed beyond the proven standalone backfill; original source/evidence retained, execution not admitted".into());
    }
    let observed = ownership::for_pair(
        &ctx.home,
        ctx.instance,
        &staged.record.pubkey,
        &staged.relay_url,
    )?
    .ok_or("staged import ownership unavailable")?;
    if !replacement_needed(&expected, &observed, &new.id.to_hex())? {
        // An earlier CAS is already durable. Do not reinterpret or clear any
        // reserved/uncertain Create intent: existing shared create/receipt
        // reconciliation is the only path forward, with the same operation.
        return Ok(());
    }
    beekeeper_agent_management::require_unadmitted_import(
        &ctx,
        staged.operation_id,
        &staged.record.pubkey,
        &staged.relay_url,
    )
    .map_err(|error| error.to_string())?;
    ownership::revise_quiesced_import_definition(
        &ctx.home,
        ctx.instance,
        &expected,
        &new.id.to_hex(),
    )
}

fn replacement_needed(
    expected: &ownership::Transfer,
    observed: &ownership::Transfer,
    new_revision: &str,
) -> Result<bool, String> {
    if expected.operation_id != observed.operation_id
        || expected.actor_pubkey != observed.actor_pubkey
        || expected.legacy_nonce != observed.legacy_nonce
        || beekeeper_host_core::record::canonical_relay_key(&expected.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(&observed.relay_url)
    {
        return Err("import revision reconciliation scope changed".into());
    }
    if observed.definition_event_id == new_revision {
        return Ok(false);
    }
    if observed.definition_event_id != expected.definition_event_id {
        return Err("import revision reconciliation conflicts with another revision".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prior_cas_retry_retains_same_original_proof_without_resetting_reserved_intent() {
        let expected = ownership::Transfer {
            operation_id: Uuid::new_v4().to_string(),
            actor_pubkey: "ab".repeat(32),
            relay_url: "wss://relay.example".into(),
            legacy_nonce: "old-generation".into(),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: ownership::Phase::Quiesced,
        };
        let mut after = expected.clone();
        after.definition_event_id = "ef".repeat(32);
        let original = serde_json::to_vec(&expected).unwrap();
        let uncertain = json!({"phase":"runtimeStarting","operationId":expected.operation_id,"reservedTarget":"retained"});
        let intent = serde_json::to_vec(&uncertain).unwrap();
        assert!(replacement_needed(&expected, &expected, &after.definition_event_id).unwrap());
        assert!(!replacement_needed(&expected, &after, &after.definition_event_id).unwrap());
        assert!(!replacement_needed(&expected, &after, &after.definition_event_id).unwrap());
        assert_eq!(serde_json::to_vec(&expected).unwrap(), original);
        assert_eq!(serde_json::to_vec(&uncertain).unwrap(), intent);
        after.operation_id = Uuid::new_v4().to_string();
        assert!(replacement_needed(&expected, &after, &after.definition_event_id).is_err());
        after = expected.clone();
        after.legacy_nonce = "other-generation".into();
        assert!(replacement_needed(&expected, &after, &"ef".repeat(32)).is_err());
    }
}
