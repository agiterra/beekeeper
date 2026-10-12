//! Graceful harness admission draining and child cleanup.
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
pub(super) fn complete_shutdown(clean: bool) -> anyhow::Result<()> {
    if !clean {
        anyhow::bail!("ACP shutdown did not drain accepted work within its grace period");
    }
    Ok(())
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
