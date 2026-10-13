//! Thin fresh enrollment and modern lifecycle receipts; no legacy conversion.
use super::*;
use beekeeper_agent_management::{CreateRequest, IdentityInput, RemoteStartRequest};
use beekeeper_host_core::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;
#[path = "host_channel.rs"]
mod discovery;
#[path = "host_runtime_selection.rs"]
mod runtime_selection;
pub(super) async fn client_guard(
    app: &AppHandle,
    actor: &str,
    relay: &str,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(actor, relay)?;
    let state = app.state::<AppState>();
    let lock = {
        let mut locks = state
            .provider_deploy_locks
            .lock()
            .map_err(|_| "client custody lock unavailable")?;
        locks
            .entry(format!("management:{}", key.runtime_id()))
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    Ok(lock.lock_owned().await)
}
pub(super) fn pair_directory(actor: &str, relay: &str) -> Result<PathBuf, String> {
    let ctx = context()?;
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(actor, relay)?;
    Ok(layout::host_dir(&ctx.home, ctx.instance)
        .join("management/desktop-agent")
        .join(key.runtime_id()))
}
pub(super) fn private_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let meta =
        std::fs::symlink_metadata(path).map_err(|_| "protected management receipt unavailable")?;
    if !meta.file_type().is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err("protected management receipt must be bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("management receipt permissions unsafe".into());
        }
    }
    std::fs::read(path).map_err(|_| "management receipt unreadable".into())
}
pub(super) fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.file_type().is_file()) {
        return Err(
            "management receipt must remain a regular file; original state retained".into(),
        );
    }
    atomic_write::create_dir_all_restricted(path.parent().ok_or("management path unavailable")?)?;
    atomic_write::atomic_write_json_restricted(
        path,
        &serde_json::to_vec(value).map_err(|_| "management receipt encoding failed")?,
    )
}
pub(super) async fn bootstrap(
    app: &AppHandle,
    relay: &str,
) -> Result<beekeeper_host_core::config::HostConfig, String> {
    let ctx = context()?;
    let path = layout::host_config_path(&ctx.home, ctx.instance);
    let config = beekeeper_host_core::config::HostConfig::load(&path)?;
    if config.as_ref().is_some_and(|c| {
        beekeeper_host_core::record::canonical_relay_key(&c.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(relay)
    }) {
        return Err(format!(
            "This host is commissioned for {}. Agent creation for {} is retained but blocked. Use a separately commissioned host scope for this community; changing the desktop community does not move its running executor. Existing identity and pending operation were preserved.",
            config.as_ref().map(|c| c.relay_url.as_str()).unwrap_or("another community"),
            relay
        ));
    }
    if config.is_none() {
        crate::session_provider::commands::provision_coding_session_provider(
            app.clone(),
            app.state::<AppState>(),
            app.state::<crate::agent_host::AgentHost>(),
            Some(relay.into()),
        )
        .await?;
    }
    crate::session_provider::commands::ensure_host_running(
        app,
        &app.state::<crate::agent_host::AgentHost>(),
        relay,
    )
    .await?;
    beekeeper_host_core::config::HostConfig::load(&path)?
        .ok_or_else(|| "host commissioning remains unconfirmed".into())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FreshReservation {
    operation: Uuid,
    actor: String,
    relay: String,
    owner: String,
    activate: bool,
}
fn reserve_receipt(
    path: &Path,
    actor: &str,
    relay: &str,
    owner: &str,
    activate: bool,
) -> Result<(), String> {
    if path.exists() {
        let held: FreshReservation = serde_json::from_slice(&private_bytes(path)?)
            .map_err(|_| "fresh reservation malformed")?;
        if held.actor != actor
            || held.relay != beekeeper_host_core::record::canonical_relay_key(relay)
            || held.owner != owner
            || held.activate != activate
        {
            return Err("fresh reservation differs; original operation retained".into());
        }
        return Ok(());
    }
    save(
        path,
        &FreshReservation {
            operation: Uuid::new_v4(),
            actor: actor.into(),
            relay: beekeeper_host_core::record::canonical_relay_key(relay),
            owner: owner.into(),
            activate,
        },
    )
}
pub(super) fn fresh_reserved(app: &AppHandle, actor: &str, relay: &str) -> Result<bool, String> {
    let path = pair_directory(actor, relay)?.join("fresh-reservation.json");
    if matches!(std::fs::symlink_metadata(&path),Err(e)if e.kind()==std::io::ErrorKind::NotFound) {
        return Ok(false);
    }
    let held: FreshReservation = serde_json::from_slice(&private_bytes(&path)?)
        .map_err(|_| "fresh reservation malformed")?;
    if held.actor != actor
        || held.relay != beekeeper_host_core::record::canonical_relay_key(relay)
        || held.owner
            != app
                .state::<AppState>()
                .signing_keys()?
                .public_key()
                .to_hex()
    {
        return Err("fresh reservation scope differs; ownership is unconfirmed".into());
    }
    Ok(true)
}
fn original_fresh_create_pending(activate: bool, held: Option<&ownership::Transfer>) -> bool {
    held.is_none()
        || held.is_some_and(|h| {
            ownership::is_modern_enrollment(h)
                && h.phase == ownership::Phase::HostStopped
                && h.target.is_none()
                && activate
        })
}
pub(super) fn fresh_pending(
    app: &AppHandle,
    actor: &str,
    relay: &str,
    held: Option<&ownership::Transfer>,
) -> Result<bool, String> {
    if !fresh_reserved(app, actor, relay)? {
        return Ok(false);
    }
    let reservation: FreshReservation = serde_json::from_slice(&private_bytes(
        &pair_directory(actor, relay)?.join("fresh-reservation.json"),
    )?)
    .map_err(|_| "fresh reservation malformed")?;
    Ok(original_fresh_create_pending(reservation.activate, held))
}
/// Reserve only an identity minted by this canonical setup boundary, before effects.
pub(crate) fn reserve_fresh_host_agent(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
    activate: bool,
) -> Result<(), String> {
    if record.backend != BackendKind::Local {
        return Ok(());
    }
    let keys =
        Keys::parse(&record.private_key_nsec).map_err(|_| "fresh scoped identity unavailable")?;
    if keys.public_key().to_hex() != record.pubkey {
        return Err("fresh identity does not match scoped custody".into());
    }
    let owner = app
        .state::<AppState>()
        .signing_keys()?
        .public_key()
        .to_hex();
    let ctx = context()?;
    let _management = ownership::management_lock(&ctx.home, ctx.instance)?;
    let path = pair_directory(&record.pubkey, relay)?.join("fresh-reservation.json");
    reserve_receipt(&path, &record.pubkey, relay, &owner, activate)
}
fn fresh_replay_floor(record: &ManagedAgentRecord) -> Result<u64, String> {
    // Fresh reservation is required before this path. This is an inclusion
    // boundary from minting, never a claim that earlier messages executed.
    // It also covers a message sent while first commissioning is blocked.
    let timestamp = chrono::DateTime::parse_from_rfc3339(&record.created_at)
        .map_err(|_| "fresh identity creation time unavailable; original intent retained")?
        .timestamp();
    u64::try_from(timestamp)
        .map_err(|_| "fresh identity creation time invalid; original intent retained".into())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Enrollment {
    operation: Uuid,
    actor: String,
    relay: String,
    channel: Uuid,
    cwd: PathBuf,
    public: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent,
    private: PrivateConfiguration,
    provider_instance: String,
    provider_authority: String,
    activate: bool,
    #[serde(default)]
    replay_floor_unix: Option<u64>,
}
pub(crate) async fn enroll_new_host_agent(
    app: &AppHandle,
    _state: &AppState,
    actor: &str,
    relay: &str,
    activate: bool,
) -> Result<(), String> {
    let _guard = client_guard(app, actor, relay).await?;
    let record = record_for_pair(app, actor, relay)?;
    if record.backend != BackendKind::Local {
        return Err("remote enrollment uses its provider boundary".into());
    }
    require_clean_setup(app, relay)?;
    require_no_legacy_conflict(app, &record, relay)?;
    let reservation: FreshReservation = serde_json::from_slice(&private_bytes(
        &pair_directory(actor, relay)?.join("fresh-reservation.json"),
    )?)
    .map_err(|_| {
        "canonical fresh reservation unavailable; retained old records require recreation"
    })?;
    if reservation.actor != actor
        || reservation.relay != beekeeper_host_core::record::canonical_relay_key(relay)
        || reservation.owner
            != app
                .state::<AppState>()
                .signing_keys()?
                .public_key()
                .to_hex()
        || reservation.activate != activate
    {
        return Err("fresh reservation scope changed; original intent retained".into());
    }
    let path = pair_directory(actor, relay)?.join("create.json");
    let (public, mut private) = configuration_view(app, &record)?;
    configuration::private_persona::attach_private_persona(
        app,
        &app.state::<AppState>(),
        &record,
        relay,
        &mut private,
    )?;
    let intent: Enrollment = if path.exists() {
        let held: Enrollment = serde_json::from_slice(&private_bytes(&path)?)
            .map_err(|_| "fresh create receipt malformed")?;
        if held.actor != actor
            || held.relay != beekeeper_host_core::record::canonical_relay_key(relay)
            || held.public != public
            || held.private.record != private.record
            || held.activate != activate
        {
            return Err("pending fresh create inputs changed; original operation retained".into());
        }
        held
    } else {
        let config = bootstrap(app, relay).await?;
        let runtime = runtime_selection::select(&config.runtimes, &private)?;
        let cwd = context()?
            .native_home
            .ok_or("work root unavailable")?
            .join(".beekeeper/workspaces")
            .join(actor);
        atomic_write::create_dir_all_restricted(&cwd)?;
        let held = Enrollment {
            operation: reservation.operation,
            actor: actor.into(),
            relay: beekeeper_host_core::record::canonical_relay_key(relay),
            channel: discovery::channel(app, relay, &record).await?,
            cwd,
            public,
            private,
            provider_instance: runtime.instance_ref.clone(),
            provider_authority: config.provider_pubkey.clone(),
            activate,
            replay_floor_unix: Some(fresh_replay_floor(&record)?),
        };
        let ctx = context()?;
        let _management = ownership::management_lock(&ctx.home, ctx.instance)?;
        if path.exists() {
            let concurrent: Enrollment = serde_json::from_slice(&private_bytes(&path)?)
                .map_err(|_| "fresh intent malformed")?;
            if serde_json::to_value(&concurrent).map_err(|_| "intent unavailable")?
                != serde_json::to_value(&held).map_err(|_| "intent unavailable")?
            {
                return Err("concurrent fresh enrollment differs; original intent retained".into());
            }
            concurrent
        } else {
            save(&path, &held)?;
            held
        }
    };
    let request = CreateRequest {
        operation_id: intent.operation,
        channel: intent.channel,
        display_name: record.name.clone(),
        system_prompt: record.system_prompt.clone().unwrap_or_default(),
        home: Some(context()?.home),
        cwd: intent.cwd.clone(),
        project: record.project_ref.clone(),
        model: record.model.clone(),
        provider_instance: Some(intent.provider_instance.clone()),
        public_definition: Some(intent.public),
        private_config: Some(intent.private),
        identity: Some(IdentityInput {
            keys: Keys::parse(&record.private_key_nsec)
                .map_err(|_| "scoped identity unavailable")?,
            auth_tag: record
                .auth_tag
                .clone()
                .ok_or("scoped attestation unavailable")?,
        }),
        activate: intent.activate,
        replay_floor_unix: Some(match intent.replay_floor_unix {
            Some(floor) => floor,
            None => fresh_replay_floor(&record)?,
        }),
        wait_secs: 60,
        ..Default::default()
    };
    if record_for_pair(app, actor, relay)? != record {
        return Err("fresh source changed before enrollment; original operation retained".into());
    }
    let outcome =
        beekeeper_agent_management::create(&request, &Transport::owner(app, relay)?, &context()?)
            .await
            .map_err(|e| e.to_string())?;
    save(
        &pair_directory(actor, relay)?.join("create-outcome.json"),
        &outcome,
    )?;
    if activate && outcome.get("host_admitted").and_then(Value::as_bool) != Some(true) {
        return Err("fresh host admission unconfirmed; retry original create".into());
    }
    Ok(())
}
pub(super) fn require_modern(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<(), String> {
    require_no_legacy_conflict(app, record, relay)?;
    stop_intent::require_host_ownership(&context()?, &record.pubkey, relay)
}
/// Refuse visible old-owner conflict before fresh host custody/commission effects.
pub(crate) fn require_clean_setup(app: &AppHandle, relay: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let ctx = context()?;
    let active = crate::relay::relay_ws_url_with_override(&state);
    for old in load_managed_agents(app)? {
        if old.backend != BackendKind::Local || old.pubkey.is_empty() {
            continue;
        }
        let effective = crate::relay::effective_agent_relay_url(&old.relay_url, &active);
        if beekeeper_host_core::record::canonical_relay_key(&effective)
            != beekeeper_host_core::record::canonical_relay_key(relay)
        {
            continue;
        }
        if ownership::for_pair(&ctx.home, ctx.instance, &old.pubkey, &effective)?
            .is_some_and(|h| ownership::is_modern_enrollment(&h))
        {
            continue;
        }
        require_no_legacy_conflict(app,&old,&effective).map_err(|reason|format!("Fresh setup refused while retained identity {} has an old/uncertain execution conflict: {reason}",old.pubkey))?;
    }
    Ok(())
}
fn old_evidence_clear(
    runtime_pid: Option<u32>,
    receipt: Result<bool, String>,
) -> Result<(), String> {
    if runtime_pid.is_some() || receipt? {
        return Err("old desktop execution provenance remains unresolved; stop it with its original installation before recreating an agent. No old owner was signalled".into());
    }
    Ok(())
}
pub(super) fn require_no_legacy_conflict(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<(), String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(&record.pubkey, relay)?;
    let receipt = app
        .path()
        .app_data_dir()
        .map_err(|_| "old custody observation unavailable")?
        .join("agents/agent-pids")
        .join(format!("{}.json", key.runtime_id()));
    let receipt_present = match std::fs::symlink_metadata(&receipt) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => {
            return Err(
                "old execution receipt presence is unknown; no admission was attempted".into(),
            )
        }
    };
    old_evidence_clear(record.runtime_pid, Ok(receipt_present))
}
#[path = "host_conversation.rs"]
mod conversation;
pub(super) use conversation::start_conversation;

pub(super) async fn start_existing(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<(), String> {
    let _guard = client_guard(app, &record.pubkey, relay).await?;
    require_no_legacy_conflict(app, record, relay)?;
    let directory = pair_directory(&record.pubkey, relay)?;
    let prior = ownership::for_pair(&context()?.home, context()?.instance, &record.pubkey, relay)?;
    if fresh_pending(app, &record.pubkey, relay, prior.as_ref())? {
        let reservation: FreshReservation =
            serde_json::from_slice(&private_bytes(&directory.join("fresh-reservation.json"))?)
                .map_err(|_| "fresh reservation malformed")?;
        drop(_guard);
        enroll_new_host_agent(
            app,
            &app.state::<AppState>(),
            &record.pubkey,
            relay,
            reservation.activate,
        )
        .await?;
        return Box::pin(start_existing(app, record, relay)).await;
    }
    let enrolled: Enrollment = serde_json::from_slice(&private_bytes(
        &directory.join("create.json"),
    )?)
    .map_err(|_| {
        "modern enrollment receipt unavailable; retained old identities require fresh recreation"
    })?;
    let Some(held) =
        ownership::for_pair(&context()?.home, context()?.instance, &record.pubkey, relay)?
    else {
        return Err("modern ownership is unavailable; original enrollment retained, no commissioning or host control was attempted".into());
    };
    if !ownership::is_modern_enrollment(&held) {
        return Err("historical ownership cannot authorize fresh execution; retained source is not imported or reset".into());
    }
    conversation::ensure_enrolled_running(app, record, relay, &enrolled).await?;
    let current = head(app, relay, &record.pubkey).await?;
    let primary_channel = match held.target.as_ref() {
        Some(target) => {
            let scope = RemoteStartRequest {
                operation_id: enrolled.operation,
                agent_pubkey: record.pubkey.clone(),
                expected_revision: current.id.to_hex(),
                channel: enrolled.channel,
                provider_authority: enrolled.provider_authority.clone(),
                provider_instance: enrolled.provider_instance.clone(),
                project: record.project_ref.clone(),
                wait_secs: 60,
            };
            beekeeper_agent_management::resolve_remote_start_channel(
                &scope,
                target,
                &Transport::owner(app, relay)?,
            )
            .await
            .map_err(|e| e.to_string())?
        }
        None => enrolled.channel,
    };
    if held.phase == ownership::Phase::Adopted {
        let observed = beekeeper_agent_management::status(
            &beekeeper_agent_management::LifecycleRequest {
                agent_pubkey: record.pubkey.clone(),
                channels: vec![primary_channel],
                ..Default::default()
            },
            &Transport::owner(app, relay)?,
        )
        .await
        .map_err(|e| e.to_string())?;
        let target =
            serde_json::to_value(&held.target).map_err(|_| "accepted target unavailable")?;
        if observed_live_target(
            &observed,
            &target,
            primary_channel,
            &enrolled.provider_authority,
        ) {
            return Ok(());
        }
        return Err("existing host admission is retained but exact runtime availability is unconfirmed; no new execution was created".into());
    }
    if held.phase != ownership::Phase::HostStopped {
        return Err("retained ownership is pending/uncertain; diagnose its original operation rather than recreating or starting a duplicate".into());
    }
    let path = directory.join(format!("start-{}.json", held.operation_id));
    let request: RemoteStartRequest = if path.exists() {
        serde_json::from_slice(&private_bytes(&path)?).map_err(|_| "start receipt malformed")?
    } else {
        let next = RemoteStartRequest {
            operation_id: Uuid::new_v4(),
            agent_pubkey: record.pubkey.clone(),
            expected_revision: current.id.to_hex(),
            channel: primary_channel,
            provider_authority: enrolled.provider_authority,
            provider_instance: enrolled.provider_instance,
            project: record.project_ref.clone(),
            wait_secs: 60,
        };
        save(&path, &next)?;
        next
    };
    if request.agent_pubkey != record.pubkey
        || request.expected_revision != current.id.to_hex()
        || request.channel != primary_channel
    {
        return Err("pending Start scope/revision changed; original operation retained".into());
    }
    let outcome = beekeeper_agent_management::start_remote(
        &request,
        &Transport::owner(app, relay)?,
        &context()?,
    )
    .await
    .map_err(|e| e.to_string())?;
    save(&directory.join("start-outcome.json"), &outcome)?;
    let observation = beekeeper_agent_management::status(
        &beekeeper_agent_management::LifecycleRequest {
            agent_pubkey: record.pubkey.clone(),
            channels: vec![request.channel],
            ..Default::default()
        },
        &Transport::owner(app, relay)?,
    )
    .await
    .map_err(|e| e.to_string())?;
    if outcome["already_adopted"] == true && outcome["runtime_available"] == true {
        return Ok(());
    }
    if !observed_live_target(
        &observation,
        &outcome["target"],
        request.channel,
        &request.provider_authority,
    ) {
        return Err("Start admission is retained but runtime availability is unconfirmed; retry the original operation".into());
    }
    Ok(())
}

fn observed_live_target(observed: &Value, target: &Value, channel: Uuid, provider: &str) -> bool {
    observed
        .get("responsibilities")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row["responsibility"]["target"] == *target
                    && row["responsibility"]["channelId"] == channel.to_string()
                    && row["responsibility"]["providerAuthority"] == provider
                    && row["runtime_available"] == true
            })
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_fresh_configured_reservation_keeps_identity_operation_and_refuses_legacy_or_changed_scope(
    ) {
        let home = tempfile::tempdir().expect("isolated custody");
        let path = home.path().join("fresh.json");
        assert!(private_bytes(&path).is_err()); // absent provenance never authorizes retained old records
        reserve_receipt(&path, "actor", "wss://relay.example", "owner", false)
            .expect("mint boundary");
        let original = private_bytes(&path).expect("protected reservation");
        assert!(!home.path().join("create.json").exists()); // interrupt before provider bootstrap/admission
        reserve_receipt(&path, "actor", "wss://relay.example/", "owner", false)
            .expect("same configured retry");
        assert_eq!(private_bytes(&path).expect("retained"), original);
        let reservation: FreshReservation = serde_json::from_slice(&original).expect("receipt");
        assert!(!reservation.activate);
        assert!(!reservation.operation.is_nil());
        for (actor, relay, owner, activate) in [
            ("other", "wss://relay.example", "owner", false),
            ("actor", "wss://other.example", "owner", false),
            ("actor", "wss://relay.example", "foreign", false),
            ("actor", "wss://relay.example", "owner", true),
        ] {
            assert!(reserve_receipt(&path, actor, relay, owner, activate).is_err());
            assert_eq!(private_bytes(&path).expect("original preserved"), original);
        }
    }
    #[test]
    fn original_activate_true_create_retries_configured_cut_instead_of_new_start() {
        let op = Uuid::new_v4();
        let mut held = ownership::Transfer {
            operation_id: op.to_string(),
            actor_pubkey: "ab".repeat(32),
            relay_url: "wss://relay.example".into(),
            legacy_nonce: format!("fresh-identity:{op}"),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: ownership::Phase::HostStopped,
        };
        assert!(original_fresh_create_pending(true, Some(&held)));
        assert!(!original_fresh_create_pending(false, Some(&held)));
        held.target = Some(
            beekeeper_core_pkg::coding_session_command::CodingSessionTarget {
                driver: "codex-acp".into(),
                instance_id: "provider".into(),
                session_id: Uuid::new_v4().to_string(),
                generation: 1,
            },
        );
        assert!(!original_fresh_create_pending(true, Some(&held)));
        held.target = None;
        held.legacy_nonce = "historical".into();
        assert!(!original_fresh_create_pending(true, Some(&held)));
    }
    #[test]
    fn old_or_unknown_tracked_owner_blocks_fresh_effects_without_signalling() {
        assert!(old_evidence_clear(Some(10), Ok(false)).is_err());
        assert!(old_evidence_clear(None, Ok(true)).is_err());
        assert!(old_evidence_clear(None, Err("inspection unavailable".into())).is_err());
        assert!(old_evidence_clear(None, Ok(false)).is_ok()); // absence is no known conflict, not process exit proof
    }
    #[test]
    fn ownership_or_foreign_lease_never_proves_exact_start_availability() {
        let channel = Uuid::new_v4();
        let target = json!({"generation":1,"sessionId":"exact"});
        let row = json!({"responsibility":{"target":target,"channelId":channel,"providerAuthority":"provider"},"runtime_available":true});
        assert!(observed_live_target(
            &json!({"responsibilities":[row.clone()]}),
            &target,
            channel,
            "provider"
        ));
        for changed in [json!({"generation":2,"sessionId":"exact"}), Value::Null] {
            assert!(!observed_live_target(
                &json!({"responsibilities":[row.clone()]}),
                &changed,
                channel,
                "provider"
            ));
        }
        assert!(!observed_live_target(
            &json!({"responsibilities":[row.clone()]}),
            &target,
            Uuid::new_v4(),
            "provider"
        ));
        assert!(!observed_live_target(
            &json!({"responsibilities":[row]}),
            &target,
            channel,
            "foreign"
        ));
        assert!(!observed_live_target(
            &json!({"ownershipPhase":"adopted","responsibilities":[]}),
            &target,
            channel,
            "provider"
        ));
    }
}
