//! Local acknowledgment for the existing graceful ACP shutdown boundary.
use std::{io::Write, path::Path};

pub(super) const RECEIPT_PATH: &str = "BEEKEEPER_MANAGED_AGENT_QUIESCE_RECEIPT";

pub(super) fn acknowledge(actor: &str, relay: &str) -> anyhow::Result<()> {
    let Ok(path) = std::env::var(RECEIPT_PATH) else {
        return Ok(());
    };
    let nonce = std::env::var("BEEKEEPER_MANAGED_AGENT_START_NONCE")?;
    write(Path::new(&path), &nonce, actor, relay)
}

fn write(path: &Path, nonce: &str, actor: &str, relay: &str) -> anyhow::Result<()> {
    if !path.is_absolute() || !valid_native_nonce(nonce) {
        anyhow::bail!("quiescence acknowledgment requires an absolute path and tracked nonce");
    }
    let pending = path.with_extension("pending");
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&pending)?;
    let mut payload = serde_json::json!({"schema":"beekeeper-local-quiescence/v1","startNonce":nonce,"actorPubkey":actor,"relayUrl":relay,"acceptedWorkDrained":true,"harnessPid":std::process::id()});
    payload["nativeContextExportVersion"] = 1.into();
    let native = path.with_file_name(format!("native-context-{nonce}.json"));
    payload["nativeContextAvailable"] = native.exists().into();
    if native.exists() {
        use sha2::{Digest, Sha256};
        payload["nativeContextSha256"] =
            hex::encode(Sha256::digest(std::fs::read(&native)?)).into();
    }
    file.write_all(&serde_json::to_vec(&payload)?)?;
    file.sync_all()?;
    std::fs::rename(pending, path)?;
    Ok(())
}

/// Persist host-local native cursors for one-way upgrade import. No prompts or credentials.
pub(super) struct NativeContextCapture<'a> {
    pub actor: &'a str,
    pub relay: &'a str,
    pub worker: usize,
    pub channel: &'a str,
    pub root: Option<&'a str>,
    pub cursor: &'a str,
    pub runtime: &'a str,
    pub cwd: &'a str,
    pub launch: &'a crate::acp::NativeLaunchFacts,
}

pub(super) fn persist_native_context(capture: NativeContextCapture<'_>) -> anyhow::Result<()> {
    let NativeContextCapture {
        actor,
        relay,
        worker,
        channel,
        root,
        cursor,
        runtime,
        cwd,
        launch,
    } = capture;
    let Ok(receipt) = std::env::var(RECEIPT_PATH) else {
        return Ok(());
    };
    let nonce = std::env::var("BEEKEEPER_MANAGED_AGENT_START_NONCE")?;
    if !valid_native_nonce(&nonce) {
        anyhow::bail!("native context generation nonce is invalid");
    }
    let path = Path::new(&receipt).with_file_name(format!("native-context-{nonce}.json"));
    let identity = crate::config::normalize_agent_command_identity(&launch.command);
    let config_home = match identity.as_str() {
        "codex" | "codex-acp" => launch.home_environment.get("CODEX_HOME"),
        "claude-agent-acp" | "claude-code-acp" | "claude-code" | "claudecode" => {
            launch.home_environment.get("CLAUDE_CONFIG_DIR")
        }
        _ => None,
    };
    let entry = serde_json::json!({"workerIndex":worker,"channelId":channel,"rootEventId":root,"resumeCursor":cursor,"runtimeName":runtime,"runtimeCommand":launch.command,"cwd":cwd,"sourceHome":launch.home_environment.get("HOME"),"runtimeHomeEnvironment":launch.home_environment,"nativeConfigHome":config_home});
    update_native_snapshot(&path, &nonce, actor, relay, entry)
}

fn valid_native_nonce(nonce: &str) -> bool {
    !nonce.is_empty()
        && nonce.len() <= 128
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

static NATIVE_SNAPSHOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn update_native_snapshot(
    path: &Path,
    nonce: &str,
    actor: &str,
    relay: &str,
    entry: serde_json::Value,
) -> anyhow::Result<()> {
    let _held = NATIVE_SNAPSHOT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("native context snapshot lock failed"))?;
    if !path.is_absolute() || !valid_native_nonce(nonce) {
        anyhow::bail!("native context requires a restricted absolute generation path");
    }
    let mut snapshot = if path.exists() {
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(path)?)?
    } else {
        serde_json::json!({"schema":"beekeeper-legacy-native-context/v1","actorPubkey":actor,"relayUrl":relay,"harnessPid":std::process::id(),"startNonce":nonce,"sessions":[]})
    };
    if snapshot["actorPubkey"] != actor
        || snapshot["relayUrl"] != relay
        || snapshot["startNonce"] != nonce
        || snapshot["harnessPid"].as_u64() != Some(u64::from(std::process::id()))
    {
        anyhow::bail!("native snapshot generation conflicts with existing custody");
    }
    let entries = snapshot["sessions"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("native snapshot session list is invalid"))?;
    entries.retain(|old| {
        !(old["workerIndex"] == entry["workerIndex"]
            && old["channelId"] == entry["channelId"]
            && old["rootEventId"] == entry["rootEventId"])
    });
    entries.push(entry);
    write_native_snapshot(path, &snapshot)
}

fn write_native_snapshot(path: &Path, snapshot: &serde_json::Value) -> anyhow::Result<()> {
    let pending = path.with_extension("pending");
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&pending)?;
    file.write_all(&serde_json::to_vec(&snapshot)?)?;
    file.sync_all()?;
    std::fs::rename(pending, path)?;
    Ok(())
}

pub(super) fn forget_native_cursor(cursor: &str) -> anyhow::Result<()> {
    let Ok(receipt) = std::env::var(RECEIPT_PATH) else {
        return Ok(());
    };
    let nonce = std::env::var("BEEKEEPER_MANAGED_AGENT_START_NONCE")?;
    if !valid_native_nonce(&nonce) {
        anyhow::bail!("native context generation nonce is invalid");
    }
    let path = Path::new(&receipt).with_file_name(format!("native-context-{nonce}.json"));
    if !path.exists() {
        return Ok(());
    }
    let _held = NATIVE_SNAPSHOT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("native context snapshot lock failed"))?;
    let mut snapshot: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    if snapshot["startNonce"] != nonce
        || snapshot["harnessPid"].as_u64() != Some(u64::from(std::process::id()))
    {
        anyhow::bail!("native cursor retirement has foreign generation custody");
    }
    snapshot["sessions"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("invalid native scope list"))?
        .retain(|entry| entry["resumeCursor"].as_str() != Some(cursor));
    write_native_snapshot(&path, &snapshot)
}

pub(super) async fn shutdown_agent_pool(pool: &mut crate::pool::AgentPool) {
    pool.join_set.shutdown().await;
    while let Ok(mut result) = pool.result_rx_try_recv() {
        result.agent.acp.shutdown().await;
    }
    for slot in pool.agents_mut() {
        if let Some(mut agent) = slot.take() {
            agent.acp.shutdown().await;
        }
    }
}

/// Forced cleanup and pending accepted work cannot certify a safe boundary.
pub(super) fn complete_shutdown(clean: bool, actor: &str, relay: &str) -> anyhow::Result<()> {
    if !clean {
        anyhow::bail!("ACP shutdown did not drain accepted work within its grace period");
    }
    acknowledge(actor, relay)
}

/// Reuse the harness shutdown watch for signal-driven admission quiescence.
pub(super) fn shutdown_watch() -> (
    tokio::sync::watch::Sender<()>,
    tokio::sync::watch::Receiver<()>,
) {
    use tokio::sync::watch;
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let tx = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = tx.send(());
    });

    #[cfg(unix)]
    {
        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let Ok(mut sigterm) = signal(SignalKind::terminate()) else {
                return;
            };
            sigterm.recv().await;
            let _ = tx.send(());
        });
    }

    (shutdown_tx, shutdown_rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acknowledgment_is_atomic_restricted_and_generation_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        write(
            &path,
            "generation-a",
            &"ab".repeat(32),
            "wss://relay.example",
        )
        .unwrap();
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(receipt["startNonce"], "generation-a");
        assert_eq!(receipt["acceptedWorkDrained"], true);
        assert!(!path.with_extension("pending").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(write(Path::new("relative"), "generation-a", "actor", "relay").is_err());
    }
    #[test]
    fn native_export_preserves_scopes_and_refuses_foreign_generation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native.json");
        let first = serde_json::json!({"workerIndex":0,"channelId":"channel-a","rootEventId":"root-a","resumeCursor":"native-a","runtimeCommand":"codex-acp","runtimeName":"codex","cwd":"/project","sourceHome":"/agent-home"});
        update_native_snapshot(&path, "nonce-a", "actor-a", "relay-a", first.clone()).unwrap();
        let mut second = first;
        second["rootEventId"] = "root-b".into();
        second["resumeCursor"] = "native-b".into();
        update_native_snapshot(&path, "nonce-a", "actor-a", "relay-a", second.clone()).unwrap();
        let snapshot: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(snapshot["sessions"].as_array().unwrap().len(), 2);
        assert!(
            update_native_snapshot(&path, "nonce-b", "actor-a", "relay-a", second.clone()).is_err()
        );
        assert!(update_native_snapshot(&path, "nonce-a", "actor-b", "relay-a", second).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
