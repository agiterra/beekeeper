//! Revoke queued native inputs without changing ordinary turn ownership.

use super::*;

impl Provider {
    /// Native admission and fallback failures need an answer durable with
    /// their command fence before the steer intent can be closed.
    pub(super) fn report_undelivered_native_steer(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        error: &DeliverError,
    ) -> anyhow::Result<String> {
        if matches!(error, DeliverError::QueueFull) {
            let receipt = LifecycleReceipt::turn_dropped(
                command_id,
                target,
                payload::QUEUE_FULL,
                "the execution's queue is full",
            );
            self.enqueue_terminal_receipt(channel_id, command_id, &receipt)?;
            Ok(payload::QUEUE_FULL.to_owned())
        } else {
            self.report_no_live_execution(channel_id, command_id, target, true)
        }
    }

    /// Fence native queues before awaiting another record's chain lookup.
    pub(super) fn enforce_native_steer_authority_for_genesis(
        &mut self,
        genesis: &str,
    ) -> anyhow::Result<()> {
        let records: Vec<_> = self
            .state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.as_deref() == Some(genesis))
            .cloned()
            .collect();
        // No persistence may interrupt this complete fence/latch pass: a
        // failed first receipt must not leave later commands or siblings writable.
        let plans: Vec<_> = records
            .into_iter()
            .map(|record| {
                let queued = self.fence_native_steer_authority(&record);
                (record, queued)
            })
            .collect();
        for (record, queued) in plans {
            for (command_id, attempt_id, reason) in queued {
                let (code, message) = native_steer_refusal(reason);
                self.prevent_queued_native_steer(&record, &command_id, &attempt_id, code, message)?;
            }
        }
        Ok(())
    }

    /// A failed verified fold leaves the umbrella uncertain, including siblings
    /// whose persisted ACL could not yet be advanced. Fence them without I/O.
    pub(super) fn fence_unverified_native_steers(&mut self, genesis: &str) {
        self.claims_pending_reverification
            .insert(genesis.to_owned());
        let records: Vec<_> = self
            .state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.as_deref() == Some(genesis))
            .cloned()
            .collect();
        for record in records {
            self.fence_native_steer_authority(&record);
        }
    }

    /// Recheck native eligibility without any fallible I/O or suspension.
    /// Called immediately after a verified fold as well as in umbrella batches.
    /// Ordinary turn ownership and cancellation semantics are unchanged.
    pub(super) fn fence_native_steer_authority(
        &mut self,
        record: &SessionRecord,
    ) -> Vec<(String, String, SteerWriteRefusal)> {
        let pending = record
            .genesis_ref
            .as_deref()
            .is_some_and(|genesis| self.claims_pending_reverification.contains(genesis));
        let candidates: Vec<_> = self
            .in_flight
            .iter()
            .filter_map(|(command_id, turn)| {
                if turn.session_id != record.session_id {
                    return None;
                }
                let attempt_id = turn.steer_attempt.as_ref()?;
                let reason = if pending {
                    SteerWriteRefusal::AuthorityUnverified
                } else if commands::handover_fence(record, &turn.operator_pubkey, &self.pubkey_hex)
                    .is_some_and(|refusal| refusal.code == payload::HANDOVER_FENCED)
                {
                    SteerWriteRefusal::Fenced
                } else if !commands::operator_may_steer(record, &turn.operator_pubkey) {
                    SteerWriteRefusal::OperatorRevoked
                } else {
                    return None;
                };
                Some((command_id.clone(), attempt_id.clone(), reason))
            })
            .collect();
        let mut queued = Vec::new();
        for (command_id, attempt_id, reason) in candidates {
            // Also needed while queued: prompt completion can drop the input
            // before its guard runs. A later regrant cannot revive that fallback.
            if let Some(turn) = self.in_flight.get_mut(&command_id) {
                turn.native_authority_refusal.get_or_insert(reason);
            }
            let fenced_at = self
                .sessions
                .handle(&record.session_id)
                .map_or(session::FencedAt::Queued, |handle| {
                    handle.fence_command_with_reason(&command_id, reason)
                });
            if fenced_at == session::FencedAt::AlreadyDequeued {
                // Authority cannot recall written bytes. Keep Injected/Unknown
                // truthful, but never revive this input after a later regrant.
            } else {
                queued.push((command_id, attempt_id, reason));
            }
        }
        queued
    }

    /// Persist the exact terminal answer before closing the steer intent.
    /// Failed outbox or ledger projections leave a recoverable signed answer;
    /// a failed attempt append leaves the command terminally fenced too.
    pub(super) fn prevent_queued_native_steer(
        &mut self,
        record: &SessionRecord,
        command_id: &str,
        attempt_id: &str,
        code: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        let receipt =
            LifecycleReceipt::turn_refused(command_id, &self.target_for(record), code, message);
        self.enqueue_terminal_receipt(record.channel_id, command_id, &receipt)?;
        self.state
            .resolve_steer_attempt(attempt_id, SteerDisposition::Prevented, None)?;
        self.in_flight.remove(command_id);
        Ok(())
    }
}

pub(super) fn native_steer_refusal(reason: SteerWriteRefusal) -> (&'static str, &'static str) {
    match reason {
        SteerWriteRefusal::Fenced => (
            payload::HANDOVER_FENCED,
            "the handover fenced this undelivered steer; it will not be retried",
        ),
        SteerWriteRefusal::OperatorRevoked => (
            payload::UNAUTHORIZED_OPERATOR,
            "the sender's operator grant was withdrawn; this undelivered steer will not be retried",
        ),
        SteerWriteRefusal::AuthorityUnverified => (
            commands::AUTHORITY_NOT_REVERIFIED,
            "the session authority chain could not be verified; this undelivered steer will not be retried",
        ),
        SteerWriteRefusal::Unavailable => (
            payload::ACTOR_UNAVAILABLE,
            "the actor could not verify native steering authority; nothing was written",
        ),
    }
}
