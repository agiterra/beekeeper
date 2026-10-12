//! Protected delivery receipt for desktop Stop retries; shared service owns targets.
use super::*;
use beekeeper_host_core::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StopIntent {
    pub operation_id: uuid::Uuid,
    actor: String,
    relay: String,
    terminal: bool,
}
fn read(path: &Path) -> Result<Option<StopIntent>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("stop retry receipt unavailable; operation retained".into()),
    };
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
        return Err("stop retry receipt must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("stop retry receipt custody permissions unsafe".into());
        }
    }
    let bytes = std::fs::read(path).map_err(|_| "stop retry receipt unreadable")?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "stop retry receipt malformed".into())
}
fn path(ctx: &Context, actor: &str, relay: &str) -> Result<PathBuf, String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(actor, relay)?;
    Ok(layout::host_dir(&ctx.home, ctx.instance)
        .join("management/desktop-stop")
        .join(key.runtime_id())
        .join("stop-operation.json"))
}
fn scoped(intent: &StopIntent, actor: &str, relay: &str) -> Result<(), String> {
    if intent.actor != actor
        || intent.relay != beekeeper_host_core::record::canonical_relay_key(relay)
    {
        return Err("stop retry receipt scope differs; operation retained".into());
    }
    Ok(())
}
/// Refuse legacy/pending custody; an empty relay inventory cannot prove it stopped.
pub(super) fn require_host_ownership(
    ctx: &Context,
    actor: &str,
    relay: &str,
) -> Result<(), String> {
    let held = ownership::for_pair(&ctx.home, ctx.instance, actor, relay)?;
    if !held.is_some_and(|held| {
        matches!(
            held.phase,
            ownership::Phase::Adopted | ownership::Phase::HostStopped
        )
    }) {
        return Err("migration required or incomplete; use supported setup/Start to establish host custody before Stop. No legacy termination or stopped result is claimed".into());
    }
    Ok(())
}
pub(super) fn pending(ctx: &Context, actor: &str, relay: &str) -> Result<StopIntent, String> {
    let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
    require_host_ownership(ctx, actor, relay)?;
    let path = path(ctx, actor, relay)?;
    if let Some(held) = read(&path)? {
        scoped(&held, actor, relay)?;
        if !held.terminal {
            return Ok(held);
        }
    }
    let next = StopIntent {
        operation_id: uuid::Uuid::new_v4(),
        actor: actor.into(),
        relay: beekeeper_host_core::record::canonical_relay_key(relay),
        terminal: false,
    };
    atomic_write::create_dir_all_restricted(
        path.parent().ok_or("stop receipt directory unavailable")?,
    )?;
    atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&next).map_err(|_| "stop receipt encoding failed")?,
    )?;
    Ok(next)
}
pub(super) fn complete(
    ctx: &Context,
    actor: &str,
    relay: &str,
    operation: uuid::Uuid,
) -> Result<(), String> {
    let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
    let path = path(ctx, actor, relay)?;
    let mut held =
        read(&path)?.ok_or("stop retry receipt disappeared; terminal delivery unconfirmed")?;
    scoped(&held, actor, relay)?;
    if held.operation_id != operation {
        return Err("stop terminal result belongs to another delivery operation".into());
    }
    held.terminal = true;
    atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&held).map_err(|_| "stop receipt encoding failed")?,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_retry_keeps_operation_terminal_next_stop_gets_new_one() {
        let home = tempfile::tempdir().unwrap();
        let ctx = Context {
            home: home.path().into(),
            instance: layout::Instance::Dev,
            native_home: None,
        };
        let actor = "ab".repeat(32);
        let relay = "wss://relay.example";
        ownership::record_host_enrollment(
            &ctx.home,
            ctx.instance,
            &actor,
            relay,
            &uuid::Uuid::new_v4().to_string(),
            &"cd".repeat(32),
            None,
        )
        .unwrap();
        let first = pending(&ctx, &actor, relay).unwrap();
        let retry = pending(&ctx, &actor, relay).unwrap();
        assert_eq!(first.operation_id, retry.operation_id);
        assert!(complete(&ctx, &actor, relay, uuid::Uuid::new_v4()).is_err());
        assert_eq!(
            pending(&ctx, &actor, relay).unwrap().operation_id,
            first.operation_id
        );
        complete(&ctx, &actor, relay, first.operation_id).unwrap();
        assert_ne!(
            pending(&ctx, &actor, relay).unwrap().operation_id,
            first.operation_id
        );
    }
    #[test]
    fn missing_or_legacy_ownership_cannot_certify_empty_inventory_as_stopped() {
        let home = tempfile::tempdir().unwrap();
        let ctx = Context {
            home: home.path().into(),
            instance: layout::Instance::Dev,
            native_home: None,
        };
        let actor = "ab".repeat(32);
        let relay = "wss://relay.example";
        assert!(pending(&ctx, &actor, relay)
            .unwrap_err()
            .contains("migration required"));
        let held = ownership::Transfer {
            operation_id: "old".into(),
            actor_pubkey: actor.clone(),
            relay_url: relay.into(),
            legacy_nonce: "generation".into(),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: ownership::Phase::Prepared,
        };
        ownership::advance(&ctx.home, ctx.instance, held).unwrap();
        assert!(require_host_ownership(&ctx, &actor, relay)
            .unwrap_err()
            .contains("migration required"));
        assert!(!path(&ctx, &actor, relay).unwrap().exists());
    }
}
