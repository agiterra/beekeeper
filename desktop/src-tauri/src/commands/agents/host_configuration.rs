//! Client patch translation and delivery handles; the shared service owns effects.
use super::*;
#[path = "host_persona_update.rs"]
mod persona_update;
#[path = "host_private_persona.rs"]
pub(super) mod private_persona;
use crate::managed_agents::{UpdateManagedAgentRequest, UpdateManagedAgentResponse};
use beekeeper_agent_management::{LifecycleRequest, UpdateRequest};
use beekeeper_host_core::atomic_write;
use persona_update::StagedPersonaUpdate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationIntent {
    operation_id: Uuid,
    owner_pubkey: String,
    expected_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_private_snapshot_hash: Option<String>,
    desired_hash: String,
    was_active: bool,
    quiesced: bool,
    accepted_revision: Option<String>,
    restarted: bool,
    complete: bool,
}

fn save_intent(path: &Path, intent: &ConfigurationIntent) -> Result<(), String> {
    atomic_write::atomic_write_json_restricted(
        path,
        &serde_json::to_vec(intent).map_err(|e| e.to_string())?,
    )
}
fn read_intent(path: &Path) -> Result<Option<ConfigurationIntent>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
        return Err("configuration delivery handle is not bounded regular custody".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("configuration delivery handle permissions are unsafe".into());
        }
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn configuration_hash(
    public: &beekeeper_core_pkg::managed_agent::ManagedAgentEventContent,
    private: &PrivateConfiguration,
) -> Result<String, String> {
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(&json!({"public":public,"private":private.record}))
            .map_err(|_| "configuration digest unavailable")?,
    )))
}
fn configuration_path(
    ctx: &Context,
    actor: &str,
    relay: &str,
    hash: &str,
    refresh: bool,
) -> Result<PathBuf, String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(actor, relay)?;
    let purpose = if refresh { "refresh" } else { "configure" };
    Ok(layout::host_dir(&ctx.home, ctx.instance)
        .join("management/desktop-configuration")
        .join(key.runtime_id())
        .join(format!("{purpose}-{hash}.json")))
}
async fn configuration_operation(
    app: &AppHandle,
    state: &AppState,
    actor: &str,
    relay: &str,
    public: &beekeeper_core_pkg::managed_agent::ManagedAgentEventContent,
    private: &PrivateConfiguration,
    expected: (Option<&ManagedAgentRecord>, Option<&str>),
) -> Result<(PathBuf, ConfigurationIntent), String> {
    let (expected_source, expected_private_hash) = expected;
    let owner_pubkey = Transport::owner(app, relay)?.keys().public_key().to_hex();
    let desired_hash = configuration_hash(public, private)?;
    let ctx = context()?;
    let path = configuration_path(
        &ctx,
        actor,
        relay,
        &desired_hash,
        expected_private_hash.is_some(),
    )?;
    let dir = path.parent().ok_or("configuration directory unavailable")?;
    atomic_write::create_dir_all_restricted(dir)?;
    let current = head(app, relay, actor).await?;
    let held = {
        let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
        read_intent(&path)?
    };
    if let Some(held) = held.as_ref() {
        if held.owner_pubkey != owner_pubkey || held.desired_hash != desired_hash {
            return Err("configuration delivery payload conflicts".into());
        }
        if expected_private_hash.is_some()
            || !held.complete
            || held.accepted_revision.as_deref() == Some(current.id.to_hex().as_str())
        {
            return Ok((path, held.clone()));
        }
    }
    // A fresh edit never replaces an unseen newer owner revision. Retry uses
    // its original expected revision and the service's exact signed journal.
    let expected_public = expected_source
        .map(crate::managed_agents::agent_events::agent_event_content)
        .unwrap_or_else(|| public.clone());
    let current_public: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&current.content).map_err(|_| "accepted definition is malformed")?;
    if current_public.public_projection() != expected_public {
        return Err(format!(
            "stale configuration cache; effective revision is {}; refresh before editing",
            current.id
        ));
    }
    let was_active = if expected_private_hash.is_some() {
        // Refresh never requests a lifecycle effect; lease availability is not
        // a prerequisite and must not be inferred from accepted ownership.
        false
    } else {
        lifecycle::active_host_agent_pubkeys(app, state)
            .await?
            .contains(actor)
    };
    let next = ConfigurationIntent {
        operation_id: Uuid::new_v4(),
        owner_pubkey,
        expected_revision: current.id.to_hex(),
        expected_private_snapshot_hash: expected_private_hash.map(str::to_owned),
        desired_hash,
        was_active,
        quiesced: false,
        accepted_revision: None,
        restarted: false,
        complete: false,
    };
    let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
    if let Some(old) = read_intent(&path)? {
        if expected_private_hash.is_some()
            || !old.complete
            || old.accepted_revision.as_deref() == Some(current.id.to_hex().as_str())
        {
            return Ok((path, old));
        }
        save_intent(
            &dir.join(format!("completed-{}.json", old.operation_id)),
            &old,
        )?;
    }
    save_intent(&path, &next)?;
    Ok((path, next))
}

async fn configure(
    app: &AppHandle,
    state: &AppState,
    desired: &ManagedAgentRecord,
    relay: &str,
    restart: bool,
    expected_source: Option<&ManagedAgentRecord>,
    staged_persona: Option<&StagedPersonaUpdate>,
) -> Result<(PathBuf, ConfigurationIntent), String> {
    if let Some(staged) = staged_persona {
        staged.assert_current(&load_personas(app)?)?;
    }
    let (public, mut private) = match staged_persona {
        Some(staged) => configuration_view_with_personas(
            app,
            desired,
            &staged.projected_personas(&load_personas(app)?),
        )?,
        None => configuration_view(app, desired)?,
    };
    let expected = expected_source.unwrap_or(desired);
    if record_for_pair(app, &desired.pubkey, relay)? != *expected {
        return Err("client source changed before private configuration projection".into());
    }
    match staged_persona {
        Some(staged) => private_persona::attach_private_persona_definition(
            app,
            state,
            desired,
            relay,
            &mut private,
            &staged.desired,
        )?,
        None => private_persona::attach_private_persona(app, state, desired, relay, &mut private)?,
    }
    let (path, mut intent) = configuration_operation(
        app,
        state,
        &desired.pubkey,
        relay,
        &public,
        &private,
        (expected_source, None),
    )
    .await?;
    if let Some(staged) = staged_persona {
        if intent.expected_private_snapshot_hash.is_none() {
            let snapshot = beekeeper_agent_management::private_configuration_snapshot(
                &context()?,
                &Transport::owner(app, relay)?,
                &desired.pubkey,
            )
            .map_err(|e| e.to_string())?;
            let (_, mut baseline) = configuration_view(app, expected)?;
            baseline.record["private_persona"] = staged
                .original_pin
                .clone()
                .ok_or("original source authentication absent")?;
            if snapshot.private.record != baseline.record {
                return Err("current accepted private configuration differs from this client source; refresh before editing the owned definition".into());
            }
            intent.expected_private_snapshot_hash = Some(snapshot.snapshot_hash);
            save_intent(&path, &intent)?;
        }
    }
    let current = head(app, relay, &desired.pubkey).await?;
    let before: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&current.content).map_err(|_| "accepted definition is malformed")?;
    if let Some(staged) = staged_persona {
        staged.assert_current(&load_personas(app)?)?;
    }
    let policy_changed = before.respond_to != public.respond_to
        || before.respond_to_allowlist != public.respond_to_allowlist;
    if !intent.quiesced && (policy_changed || (restart && intent.was_active)) {
        if current.id.to_hex() != intent.expected_revision {
            return Err(
                "configuration revision changed before quiescence; no new stop was dispatched"
                    .into(),
            );
        }
        let request = LifecycleRequest {
            operation_id: Uuid::new_v5(&intent.operation_id, b"configuration-quiescence"),
            agent_pubkey: desired.pubkey.clone(),
            channels: Vec::new(),
            wait_secs: 60,
        };
        let stopped =
            beekeeper_agent_management::stop(&request, &Transport::owner(app, relay)?, &context()?)
                .await
                .map_err(|e| e.to_string())?;
        if stopped.get("terminal").and_then(Value::as_bool) != Some(true) {
            return Err(
                "configuration withheld: previous access/execution termination remains pending"
                    .into(),
            );
        }
        intent.quiesced = true;
        save_intent(&path, &intent)?;
    }
    if let Some(staged) = staged_persona {
        staged.assert_current(&load_personas(app)?)?;
    }
    crate::relay::assert_expected_signer(
        Some(&intent.owner_pubkey),
        &state.signing_keys()?.public_key().to_hex(),
    )?;
    let request = UpdateRequest {
        operation_id: intent.operation_id,
        agent_pubkey: desired.pubkey.clone(),
        expected_revision: intent.expected_revision.clone(),
        display_name: None,
        system_prompt: None,
        model: None,
        home: Some(context()?.home),
        public_definition: Some(public),
        private_config: Some(private),
    };
    let transport = Transport::owner(app, relay)?;
    let ctx = context()?;
    let outcome = match intent.expected_private_snapshot_hash.as_deref() {
        Some(hash) if staged_persona.is_some() => {
            beekeeper_agent_management::update_if_private_unchanged(
                &request, &transport, &ctx, hash,
            )
            .await
        }
        _ => beekeeper_agent_management::update(&request, &transport, &ctx).await,
    }
    .map_err(|e| e.to_string())?;
    if outcome.get("relay_accepted").and_then(Value::as_bool) != Some(true) {
        return Err("configuration publication remains unconfirmed".into());
    }
    intent.accepted_revision = Some(
        outcome
            .get("effectiveRevision")
            .and_then(Value::as_str)
            .ok_or("accepted revision unavailable")?
            .into(),
    );
    acknowledge_revision(
        app,
        state,
        relay,
        &desired.pubkey,
        intent
            .accepted_revision
            .as_deref()
            .ok_or("accepted revision unavailable")?,
    )
    .await?;
    save_intent(&path, &intent)?;
    Ok((path, intent))
}

async fn acknowledge_revision(
    app: &AppHandle,
    state: &AppState,
    relay: &str,
    actor: &str,
    revision: &str,
) -> Result<(), String> {
    use crate::managed_agents::retention::{
        active_retention_scope, get_retained_event, open_retention_db, retain_inbound_event,
        InboundOutcome, RetainedEvent,
    };
    use nostr::JsonUtil;
    let accepted = head(app, relay, actor).await?;
    if accepted.id.to_hex() != revision {
        return Err(
            "Configuration accepted; a newer shared revision superseded cache reconciliation"
                .into(),
        );
    }
    let scope = active_retention_scope(app, state)?;
    if beekeeper_host_core::record::canonical_relay_key(&scope.relay_url)
        != beekeeper_host_core::record::canonical_relay_key(relay)
        || scope.owner_keys.public_key() != accepted.pubkey
    {
        return Err(
            "Configuration accepted; client identity/community changed before cache acknowledgment"
                .into(),
        );
    }
    let conn = open_retention_db(&scope.db_path)?;
    let retained = RetainedEvent {
        kind: 30177,
        pubkey: accepted.pubkey.to_hex(),
        d_tag: actor.into(),
        content: accepted.content.clone(),
        created_at: accepted.created_at.as_secs() as i64,
        raw_event: accepted.as_json(),
        pending_sync: false,
    };
    if retain_inbound_event(&conn, &retained)? == InboundOutcome::Skipped {
        let exact = get_retained_event(&conn, 30177, &accepted.pubkey.to_hex(), actor)?
            .is_some_and(|row| !row.pending_sync && row.raw_event == accepted.as_json());
        if !exact {
            return Err("Configuration accepted; a newer client publication is pending; refresh and reconcile".into());
        }
    }
    Ok(())
}

async fn finish_configuration(
    app: &AppHandle,
    desired: &ManagedAgentRecord,
    relay: &str,
    path: &Path,
    mut intent: ConfigurationIntent,
) -> Result<(), String> {
    if intent.was_active && intent.quiesced && !intent.restarted {
        enrollment::start_existing(app, desired, relay)
            .await
            .map_err(|e| {
                format!("Configuration accepted; successor admission remains unconfirmed: {e}")
            })?;
        intent.restarted = true;
        save_intent(path, &intent)?;
    }
    intent.complete = true;
    save_intent(path, &intent)
}

pub(crate) async fn reconcile_host_agent(
    app: &AppHandle,
    state: &AppState,
    actor: &str,
    relay: &str,
    restart: bool,
) -> Result<(), String> {
    let record = record_for_pair(app, actor, relay)?;
    if record.backend != BackendKind::Local {
        return Err("remote settings use their provider boundary".into());
    }
    enrollment::require_modern(app, &record, relay)?;
    let (path, intent) = configure(app, state, &record, relay, restart, None, None).await?;
    if record_for_pair(app, actor, relay)? != record {
        return Err(
            "Configuration accepted; client cache changed during reconciliation; refresh required"
                .into(),
        );
    }
    finish_configuration(app, &record, relay, &path, intent).await
}

pub(crate) async fn update_if_adopted(
    input: &UpdateManagedAgentRequest,
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<UpdateManagedAgentResponse>, String> {
    let source = load_managed_agents(app)?
        .into_iter()
        .find(|r| r.pubkey == input.pubkey)
        .ok_or("agent not found")?;
    if source.backend != BackendKind::Local {
        if input.owned_persona_update.is_some() {
            return Err("remote definition edits use their provider boundary".into());
        }
        return Ok(None);
    }
    let relay = crate::relay::effective_agent_relay_url(
        &source.relay_url,
        &crate::relay::relay_ws_url_with_override(state),
    );
    let personas = load_personas(app)?;
    let mut staged_persona = input
        .owned_persona_update
        .as_ref()
        .map(|edit| {
            StagedPersonaUpdate::prepare(&source, edit, &personas, &load_managed_agents(app)?)
        })
        .transpose()?;
    if let Some(staged) = staged_persona.as_mut() {
        staged.original_pin = Some(private_persona::verify_original_pin(
            app,
            state,
            &source,
            &relay,
            &staged.source,
        )?);
    }
    let projected = staged_persona
        .as_ref()
        .map(|staged| staged.projected_personas(&personas));
    let (mut desired, policy_changed) = crate::commands::agent_models::prospective_record(
        &source,
        input,
        projected.as_deref().unwrap_or(&personas),
    )?;
    if let Some(staged) = staged_persona.as_ref() {
        staged.apply_record(&mut desired);
    }
    let next_relay = crate::relay::effective_agent_relay_url(
        &desired.relay_url,
        &crate::relay::relay_ws_url_with_override(state),
    );
    if beekeeper_host_core::record::canonical_relay_key(&next_relay)
        != beekeeper_host_core::record::canonical_relay_key(&relay)
    {
        return Err("another community requires its own authorized host enrollment; this edit did not move the identity".into());
    }
    enrollment::require_modern(app, &source, &relay)?;
    let (path, intent) = configure(
        app,
        state,
        &desired,
        &relay,
        desired.auto_restart_on_config_change || policy_changed,
        Some(&source),
        staged_persona.as_ref(),
    )
    .await?;
    crate::relay::assert_expected_signer(
        Some(&intent.owner_pubkey),
        &state.signing_keys()?.public_key().to_hex(),
    )?;
    {
        let _lock = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let mut records = load_managed_agents(app)?;
        let current = records
            .iter_mut()
            .find(|r| r.pubkey == source.pubkey)
            .ok_or("Configuration accepted; source cache disappeared")?;
        if *current != source {
            return Err(
                "Configuration accepted; client cache changed before commit; refresh required"
                    .into(),
            );
        }
        desired.updated_at = crate::util::now_iso();
        *current = desired.clone();
        if let Some(staged) = staged_persona.as_ref() {
            let mut personas = load_personas(app)
                .map_err(|e| format!("Configuration accepted; owned cache read failed: {e}"))?;
            staged.commit_personas(&mut personas)?;
            crate::managed_agents::storage::save_agent_configuration(app, &personas, &records)
                .map_err(|e| format!("Configuration accepted; local cache commit failed: {e}"))?;
        } else {
            save_managed_agents(app, &records)?;
        }
    }
    let mut warning=Some("Configuration accepted; runtime application remains unconfirmed until observed through relay work.".into());
    if let Err(error) = finish_configuration(app, &desired, &relay, &path, intent).await {
        warning = Some(error);
    }
    if source.name != desired.name {
        match nostr::Keys::parse(&desired.private_key_nsec) {
            Ok(keys) if keys.public_key().to_hex() == desired.pubkey => {
                if let Err(error) = crate::relay::sync_managed_agent_profile(
                    state,
                    &relay,
                    &keys,
                    &desired.name,
                    desired.avatar_url.as_deref(),
                    desired.auth_tag.as_deref(),
                )
                .await
                {
                    warning = Some(format!(
                        "Configuration accepted; identity profile publication remains unconfirmed: {error}"
                    ));
                }
            }
            _ => {
                warning =
                    Some("Configuration accepted; scoped profile custody is unavailable.".into())
            }
        }
    }
    Ok(Some(UpdateManagedAgentResponse {
        agent: super::super::summarize_host_record(app, &desired)?,
        profile_sync_error: warning,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_configuration_retains_exact_operation_and_quiescence_before_retry() {
        let dir = tempfile::tempdir().expect("delivery fixture");
        let path = dir.path().join("configuration.json");
        let mut intent = ConfigurationIntent {
            operation_id: Uuid::new_v4(),
            owner_pubkey: "ab".repeat(32),
            expected_revision: "cd".repeat(32),
            expected_private_snapshot_hash: None,
            desired_hash: "ef".repeat(32),
            was_active: true,
            quiesced: false,
            accepted_revision: None,
            restarted: false,
            complete: false,
        };
        save_intent(&path, &intent).expect("stage before any effects");
        intent.quiesced = true;
        save_intent(&path, &intent).expect("persist verified stop");
        let recovered = read_intent(&path)
            .expect("read interrupted delivery")
            .expect("persisted");
        assert_eq!(recovered.operation_id, intent.operation_id);
        assert_eq!(recovered.expected_revision, intent.expected_revision);
        assert!(recovered.quiesced && recovered.was_active);
        assert!(!recovered.complete && !recovered.restarted);
        assert!(recovered.accepted_revision.is_none());
        let raw = std::fs::read_to_string(&path).expect("handle JSON");
        assert!(
            !raw.contains("private_key") && !raw.contains("auth_tag") && !raw.contains("env_vars")
        );
    }

    #[cfg(unix)]
    #[test]
    fn configuration_retry_refuses_shared_permissions_symlinks_and_unbounded_handles() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().expect("delivery fixture");
        let path = dir.path().join("handle.json");
        std::fs::write(&path, "{}").expect("fixture file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("permissions");
        assert!(read_intent(&path)
            .expect_err("shared custody")
            .contains("permissions"));
        let alias = dir.path().join("alias.json");
        symlink(&path, &alias).expect("fixture alias");
        assert!(read_intent(&alias)
            .expect_err("symlink custody")
            .contains("regular"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("permissions");
        std::fs::write(&path, vec![b'a'; 64 * 1024 + 1]).expect("oversized fixture");
        assert!(read_intent(&path)
            .expect_err("unbounded custody")
            .contains("bounded"));
    }
}

#[path = "host_accepted_configuration_refresh.rs"]
mod accepted_refresh;
pub(crate) use accepted_refresh::refresh_accepted_host_agents;
