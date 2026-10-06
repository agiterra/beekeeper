//! The native steering write boundary.

use super::*;

impl AcpClient {
    /// Write one taken steer input into the running turn.
    ///
    /// Picks the transport at write time from the freshest `active_run_id`
    /// and the capability advertised at `initialize`:
    ///
    /// - `Some(run_id)` → [`GOOSE_STEER_METHOD`]; `expectedRunId` is strictly
    ///   more precise about *which* run is steered.
    /// - `None` + `steering_supported` → [`ACP_STEER_METHOD`], carrying the
    ///   input's idle guard as `_meta.steering.idleBehavior`.
    /// - neither → nothing is written and the input is answered
    ///   [`NotDeliveredReason::Unsupported`].
    ///
    /// The capability flag selects the wire; the caller's authority guard
    /// separately admits each write after any queue wait.
    /// Probing an unknown method is unsafe: codex-acp answers unrecognized
    /// extension methods with `{}` — a JSON-RPC success — which would read as
    /// a delivered steer and silently drop the input.
    ///
    /// Returns the pending record when the request went out. A write error
    /// answers [`UnknownReason::WriteFailed`] (a partial write cannot be
    /// excluded) and returns `None`.
    pub(super) async fn write_steer(
        &mut self,
        session_id: &str,
        taken: TakenSteer,
    ) -> Option<PendingSteer> {
        let TakenSteer {
            attempt_id,
            prompt_blocks,
            idle_guard,
            write_guard,
            sink,
        } = taken;
        let prompt_block_refs: Vec<&str> = prompt_blocks.iter().map(String::as_str).collect();
        let selected = match (&self.active_run_id, self.steering_supported) {
            (Some(run_id), _) => Some((
                SteerWire::Goose,
                GOOSE_STEER_METHOD,
                build_goose_steer_params(session_id, run_id, &prompt_block_refs),
                Some(run_id.clone()),
            )),
            (None, true) => Some((
                SteerWire::AcpExtension,
                ACP_STEER_METHOD,
                build_acp_steer_params(session_id, &prompt_block_refs, idle_guard),
                None,
            )),
            (None, false) => None,
        };
        let Some((wire, method, params, native_run_id)) = selected else {
            tracing::warn!(
                attempt_id = ?attempt_id,
                "steer: no active_run_id and agent did not advertise \
                 {ACP_STEER_METHOD} — nothing written"
            );
            sink.resolve(
                SteerResolution::NotDelivered {
                    reason: NotDeliveredReason::Unsupported,
                },
                session_id,
            );
            return None;
        };

        let id = self.next_id;
        self.next_id += 1;
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut pending = PendingSteer {
            request_id: id,
            wire,
            native_run_id,
            attempt_id,
            session_id: session_id.to_owned(),
            sink: Some(sink),
            unresolved: self.unresolved_steers.clone(),
        };
        // The caller's fence and dispatch evidence share one lock. Nothing
        // awaited between this admission and starting the write: entries that
        // waited behind another ACK remained queued and preventable until now.
        if let Some(Err(reason)) = write_guard.as_ref().map(|guard| guard.begin_write()) {
            pending.resolve(SteerResolution::NotDelivered {
                reason: NotDeliveredReason::DispatchPrevented { reason },
            });
            return None;
        }
        tracing::debug!(
            target: "acp::wire",
            "→ {}",
            serde_json::to_string(&msg).unwrap_or_default()
        );
        match self.write_ndjson(&msg).await {
            Ok(()) => Some(pending),
            Err(e) => {
                tracing::warn!(
                    request_id = id,
                    "steer write failed ({method}): {e} — delivery unknown"
                );
                pending.abandon(UnknownReason::WriteFailed {
                    message: e.to_string(),
                });
                None
            }
        }
    }
}
