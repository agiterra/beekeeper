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
    if !path.is_absolute() || nonce.is_empty() {
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
    let payload = serde_json::json!({"schema":"beekeeper-local-quiescence/v1","startNonce":nonce,"actorPubkey":actor,"relayUrl":relay,"acceptedWorkDrained":true,"harnessPid":std::process::id()});
    file.write_all(&serde_json::to_vec(&payload)?)?;
    file.sync_all()?;
    std::fs::rename(pending, path)?;
    Ok(())
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
}
