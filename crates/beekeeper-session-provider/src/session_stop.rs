//! Existing lifecycle stop and verified worker shutdown.
use super::*;

/// The one durable stop fence used by explicit stop and authority withdrawal.
/// A restart without a live registry cannot turn interrupted intent into proof.
pub(crate) fn begin_quiescence(
    config: &crate::config::Config,
    target: &CodingSessionTarget,
    command: &str,
    has_runtime: bool,
) -> Result<std::path::PathBuf, session::CreateFailure> {
    let path = config
        .state_dir
        .join(format!("stop-quiescence-{}.json", target.session_id));
    if let Some(value) = crate::managed_configuration::protected(&path)? {
        if value.get("target")
            != Some(&serde_json::to_value(target).map_err(|_| {
                execution_scope::refuse(
                    "SESSION_QUIESCENCE_UNCONFIRMED",
                    "stop fence target unavailable",
                )
            })?)
            || !has_runtime
        {
            return Err(execution_scope::refuse("SESSION_QUIESCENCE_UNCONFIRMED", "a prior stop was interrupted before process quiescence was established; explicit recovery is required"));
        }
    }
    let intent = serde_json::json!({"schema":"beekeeper-stop-quiescence/v1","target":target,"commandId":command});
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&intent).map_err(|_| {
            execution_scope::refuse(
                "SESSION_QUIESCENCE_UNCONFIRMED",
                "stop fence encoding failed",
            )
        })?,
    )
    .map_err(|_| {
        execution_scope::refuse(
            "SESSION_QUIESCENCE_UNCONFIRMED",
            "stop fence persistence failed; execution remains fenced for repair",
        )
    })?;
    Ok(path)
}

impl Provider {
    /// Reconcile an exact provider-signed verified stop already in the outbox.
    /// No process is started and an unproven marker is never inferred terminal.
    pub(crate) fn reconcile_verified_stop(&mut self, event: &Event) -> anyhow::Result<()> {
        let Ok(receipt) = payload::decode_coding_session_lifecycle_receipt(&event.content) else {
            return Ok(());
        };
        if receipt.status != payload::ReceiptStatus::Stopped {
            return Ok(());
        }
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_LIFECYCLE_RECEIPT
            || event.pubkey != self.config.keys.public_key()
            || event.verify().is_err()
        {
            return Err(anyhow::anyhow!("verified stop event authority mismatch"));
        }
        let target = receipt
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("verified stop target missing"))?;
        let record = self
            .state
            .session(&target.session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("verified stop record missing"))?;
        if !record.closed
            || record.target(&self.config.instance_id) != *target
            || !event
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["h", &record.channel_id.to_string()])
        {
            return Err(anyhow::anyhow!("verified stop scope mismatch"));
        }
        self.record_quiesced(target)?;
        self.state
            .consume_command(&receipt.command_id, now_secs())?;
        let path = self
            .config
            .state_dir
            .join(format!("stop-quiescence-{}.json", target.session_id));
        if let Some(marker) = crate::managed_configuration::protected(&path)
            .map_err(|failure| anyhow::anyhow!("{}", failure.message))?
        {
            if marker.get("target") != Some(&serde_json::to_value(target)?)
                || marker.get("commandId").and_then(serde_json::Value::as_str)
                    != Some(receipt.command_id.as_str())
            {
                return Err(anyhow::anyhow!(
                    "verified stop does not own the pending marker"
                ));
            }
            std::fs::remove_file(&path)?;
        }
        self.discard_context_packages(&target.session_id);
        self.forget_redactions(&target.session_id);
        self.publish_metadata(record.channel_id, target, SessionStatus::Stopped)?;
        crate::managed_enrollment::stopped(&self.config, &self.state, &target.session_id)
            .map_err(|failure| anyhow::anyhow!("{}", failure.message))?;
        Ok(())
    }

    pub(crate) fn recover_verified_stops(&mut self) -> anyhow::Result<()> {
        let events: Vec<_> = self.outbox.pending_events().cloned().collect();
        for event in events {
            self.reconcile_verified_stop(&event)?;
        }
        Ok(())
    }

    fn refuse_stop(
        &mut self,
        channel: Uuid,
        command: &str,
        receipt: &LifecycleReceipt,
    ) -> anyhow::Result<()> {
        self.refuse_with_staged_answer(
            channel,
            command,
            receipt,
            Some(coding_session_lifecycle_receipt_semantic_key(command)),
        )
    }

    pub(crate) async fn stop_session(&mut self, plan: StopPlan) -> anyhow::Result<()> {
        let session_id = plan.target.session_id.clone();
        let target_key = coding_session_target_key(&plan.target);
        if self.state.unresolved_start_target(&plan.target) {
            return self.refuse_stop(plan.channel_id,&plan.command_id,&LifecycleReceipt::failed(&plan.command_id,"SESSION_QUIESCENCE_UNCONFIRMED","startup effects remain uncertain; no runtime absence is treated as proof of stop"));
        }
        // `checkpointOnStop` (off by default): one 44231 of the worktree before
        // the stop's quiescence, only when it changed since the last one. A
        // capture that fails never blocks the stop; it is logged. The 90 s
        // budget starts before the boundary is prepared (ledger 406 F7); when
        // it passes, the stop goes ahead and `checkpoint_skipped` says so.
        let mut checkpoint_skipped: Option<&'static str> = None;
        if self.checkpoint_on_stop {
            match self.checkpoint_before_stop(&plan).await {
                Ok(Some(crate::checkpoint_on_stop::StopCheckpointSkipped::Timeout)) => {
                    checkpoint_skipped =
                        Some(crate::checkpoint_on_stop::CHECKPOINT_SKIPPED_TIMEOUT);
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(target: "csp::checkpoint", command_id = %plan.command_id, "checkpoint before stop did not complete: {error}");
                }
            }
        }
        let pending_path = match begin_quiescence(
            &self.config,
            &plan.target,
            &plan.command_id,
            self.sessions.handle(&session_id).is_some()
                || self.recovered_group_absent(&plan.target),
        ) {
            Ok(path) => path,
            Err(failure) => {
                return self.refuse_stop(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                )
            }
        };
        prepare_terminal_stop(
            self,
            |provider| {
                // Persist terminal intent before a release can escape. If this
                // write fails, the actor remains live and no contradictory
                // ephemeral state is queued.
                provider.state.update_session(&session_id, |record| {
                    record.closed = true;
                    record.open_turn = None;
                })?;
                Ok(())
            },
            |provider| {
                // The release still precedes every durable terminal fact.
                // Arrival is sequence-fenced, while this clean ordering avoids
                // a transient live assertion after an intentional stop.
                match provider.queue_lease(&session_id, CodingSessionLeaseState::Released) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        // Never let an older queued/established live assertion
                        // escape after durable stopping intent. The terminal
                        // facts below complete; TTL handles the missing release.
                        provider.pending_leases.remove(&target_key);
                        provider.established_leases.remove(&target_key);
                        Err(error)
                    }
                }
            },
        )?;
        // A stopped generation must stop asking for custody: re-staging a key
        // for an execution that can never take another turn would leave usable
        // key material on disk for nothing.
        self.publish_seat_requests();
        if self.sessions.handle(&session_id).is_none() && !self.recovered_group_absent(&plan.target)
        {
            return self.refuse_stop(plan.channel_id, &plan.command_id, &LifecycleReceipt::failed(&plan.command_id, "SESSION_QUIESCENCE_UNCONFIRMED", "the recovered child group is present or cannot be proved absent; custody and context remain fenced for repair"));
        }
        let quiescence = self
            .sessions
            .shutdown_and_join(&plan.target.session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        if quiescence == session::ActorQuiescence::Unproven {
            return self.refuse_stop(plan.channel_id, &plan.command_id, &LifecycleReceipt::failed(&plan.command_id, "SESSION_QUIESCENCE_UNCONFIRMED", "the actor was fenced but its child process group could not be shown terminal; custody is retained for explicit recovery"));
        }
        self.record_quiesced(&plan.target)?;
        // Exact verified completion is durable before consumption or proof
        // retirement: a crash can project this answer without another child.
        // Round 2 Lane D wiring: a stop that ran out of checkpoint budget says
        // so on its own signed receipt (`checkpointSkipped: "timeout"`, the
        // one value `LifecycleReceipt` admits on `stopped`); a stop that
        // checkpointed, or was never asked to, signs the plain five keys.
        let receipt = match checkpoint_skipped {
            Some(_) => LifecycleReceipt::stopped_without_checkpoint(&plan.command_id, &plan.target),
            None => LifecycleReceipt::stopped(&plan.command_id, &plan.target),
        };
        let event = build_coding_session_lifecycle_receipt(
            plan.channel_id,
            &plan.command_id,
            &serde_json::to_string(&receipt)?,
        )?
        .sign_with_keys(&self.config.keys)?;
        self.state.stage_terminal_disposition(
            &plan.command_id,
            state::TerminalDisposition {
                lifecycle_start: None,
                semantic_key: coding_session_lifecycle_receipt_semantic_key(&plan.command_id),
                event,
            },
        )?;
        self.state.consume_command(&plan.command_id, now_secs())?;
        #[cfg(test)]
        if std::mem::take(&mut self.state.fault_plan().fail_next_verified_stop_projection) {
            self.outbox.fail_next_enqueue();
        }
        self.flush_terminal_dispositions()?;
        // The projection reconciler retires this exact marker only after the
        // signed terminal event has reached the durable outbox.
        if pending_path.exists() {
            return Err(anyhow::anyhow!(
                "verified stop outcome did not reconcile its proof marker"
            ));
        }
        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %plan.target.session_id,
            checkpoint_skipped = checkpoint_skipped.unwrap_or("none"),
            "session stopped"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipts(provider: &Provider, command: &str) -> Vec<LifecycleReceipt> {
        provider
            .outbox
            .pending_events()
            .filter_map(|event| {
                payload::decode_coding_session_lifecycle_receipt(&event.content).ok()
            })
            .filter(|receipt| receipt.command_id == command)
            .collect()
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn stopped_receipt_follows_actual_acp_child_group_quiescence() {
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("ordinary-work");
        std::fs::create_dir(&cwd).expect("workspace");
        let channel = Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        let event = crate::tests::create_event(&provider, channel, "create-quiescence");
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("real ACP child create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        let pid = provider
            .sessions
            .handle(&target.session_id)
            .expect("live registry")
            .child_pid_for_tests()
            .expect("actual child");
        assert!(nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), None).is_ok());
        let event = crate::tests::lifecycle_target_event(
            &provider,
            channel,
            "stop-quiescence",
            "session.stop",
            &target,
        );
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("stop joined");
        assert!(
            nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), None).is_err(),
            "signed terminal must not precede child group exit"
        );
        assert!(receipts(&provider, "stop-quiescence")
            .iter()
            .any(|receipt| receipt.status == payload::ReceiptStatus::Stopped));
        assert!(!provider
            .config
            .state_dir
            .join(format!("stop-quiescence-{}.json", target.session_id))
            .exists());
    }
    #[tokio::test]
    async fn interrupted_stop_without_live_registry_never_signs_false_stopped() {
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("ordinary-work");
        std::fs::create_dir(&cwd).expect("workspace");
        let channel = Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        let event = crate::tests::create_event(&provider, channel, "create-interrupted-stop");
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        provider
            .sessions
            .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        let pending = provider
            .config
            .state_dir
            .join(format!("stop-quiescence-{}.json", target.session_id));
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(&pending, b"{}")
            .expect("interrupted intent");
        provider
            .stop_session(StopPlan {
                command_id: "stop-after-interruption".into(),
                channel_id: channel,
                target,
            })
            .await
            .expect("explicit blocked disposition");
        let rows = receipts(&provider, "stop-after-interruption");
        assert!(!rows
            .iter()
            .any(|receipt| receipt.status == payload::ReceiptStatus::Stopped));
        assert!(rows.iter().any(|receipt| receipt
            .error
            .as_ref()
            .is_some_and(|error| error.code == "SESSION_QUIESCENCE_UNCONFIRMED")));
        assert!(
            pending.exists(),
            "unconfirmed intent remains for explicit recovery"
        );
    }
}
