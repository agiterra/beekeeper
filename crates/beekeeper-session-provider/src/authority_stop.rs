//! Authority withdrawal reuses the exact terminal stop/quiescence fence.
use super::*;

impl Provider {
    /// Reconcile all executions of a deleted project, including uncertain closed stops.
    /// Returns whether verified terminal work scheduled checkpoint retirement.
    pub(crate) async fn stop_project_executions(&mut self, project: &str) -> bool {
        let affected: Vec<String> = self
            .state
            .sessions()
            .filter(|record| {
                record.project_ref.as_deref() == Some(project)
                    && (!record.closed
                        || self
                            .config
                            .state_dir
                            .join(format!("stop-quiescence-{}.json", record.session_id))
                            .exists()
                        || !self.recovered_group_absent(&record.target(&self.config.instance_id)))
            })
            .map(|record| record.session_id.clone())
            .collect();
        let mut verified = true;
        for session_id in affected {
            if let Err(error) = self
                .stop_for_withdrawn_authority(&session_id, "this execution's project was deleted")
                .await
            {
                verified = false;
                tracing::error!(target: "csp::projects", %session_id, "could not stop: {error}");
            }
        }
        if !verified {
            return false;
        } // Keep checkpoint/context recovery while terminal proof is missing.
          // SV-55: the deleted project's sessions' checkpoint refs go with it,
          // retired off the loop (git, with a per-repository ceiling).
        let retirements = project_deletion::checkpoint_retirements(
            self.state.sessions().map(|record| {
                (
                    record.session_id.as_str(),
                    record.project_ref.as_deref(),
                    record.cwd.as_path(),
                )
            }),
            project,
        );
        if !retirements.is_empty() {
            tokio::spawn(project_deletion::retire_checkpoint_refs(retirements));
            return true;
        }
        false
    }

    /// Stop one execution whose own authority was withdrawn, through the same
    /// terminal path an operator's stop takes: the record is closed and
    /// marked before anything else, the lease released, admitted turns
    /// refused, and the adapter's whole process group stopped. A resume or
    /// restore of the record is refused afterwards
    /// ([`state::SessionRecord::authority_withdrawn`]).
    pub(crate) async fn stop_for_withdrawn_authority(
        &mut self,
        session_id: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        let Some(record) = self.state.session(session_id).cloned() else {
            return Ok(());
        };
        if record.closed
            && !self
                .config
                .state_dir
                .join(format!("stop-quiescence-{session_id}.json"))
                .exists()
            && self.sessions.handle(session_id).is_none()
            && self.recovered_group_absent(&record.target(&self.config.instance_id))
        {
            return Ok(());
        }
        let target = self.target_for(&record);
        let target_key = coding_session_target_key(&target);
        // Persist the same stop fence BEFORE closed state can authorize a successor.
        let pending_path = crate::session_stop::begin_quiescence(
            &self.config,
            &target,
            "withdrawn-authority",
            self.sessions.handle(session_id).is_some() || self.recovered_group_absent(&target),
        )
        .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?;
        prepare_terminal_stop(
            self,
            |provider| {
                provider.state.update_session(session_id, |record| {
                    record.closed = true;
                    record.open_turn = None;
                    record.authority_withdrawn = Some(reason.to_owned());
                })?;
                Ok(())
            },
            |provider| match provider.queue_lease(session_id, CodingSessionLeaseState::Released) {
                Ok(()) => Ok(()),
                Err(error) => {
                    provider.pending_leases.remove(&target_key);
                    provider.established_leases.remove(&target_key);
                    Err(error)
                }
            },
        )?;
        self.publish_seat_requests();
        let admitted: Vec<String> = self
            .in_flight
            .iter()
            .filter(|(_, turn)| turn.session_id == session_id)
            .map(|(command_id, _)| command_id.clone())
            .collect();
        for command_id in admitted {
            self.in_flight.remove(&command_id);
            let receipt = LifecycleReceipt::failed(
                &command_id,
                payload::ACTOR_UNAVAILABLE,
                &format!("{reason}, so this turn will not run"),
            );
            if let Err(error) = self.enqueue_receipt(record.channel_id, &command_id, &receipt) {
                tracing::warn!(target: "csp", %command_id, "could not refuse a withdrawn turn: {error}");
            }
        }
        if self.sessions.handle(session_id).is_none() && !self.recovered_group_absent(&target) {
            return Err(anyhow::anyhow!("SESSION_QUIESCENCE_UNCONFIRMED: recovered child group absence is unproved; context and checkpoints retained"));
        }
        let quiescence = self
            .sessions
            .shutdown_and_join(session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        if quiescence == session::ActorQuiescence::Unproven {
            return Err(anyhow::anyhow!("SESSION_QUIESCENCE_UNCONFIRMED: withdrawn execution child group was not shown terminal; custody retained"));
        }
        self.record_quiesced(&target)?;
        std::fs::remove_file(&pending_path)?;
        self.discard_context_packages(session_id);
        if let Err(error) =
            self.publish_metadata(record.channel_id, &target, SessionStatus::Stopped)
        {
            tracing::warn!(target: "csp", %session_id, "could not publish the withdrawn stop: {error}");
        }
        crate::managed_enrollment::stopped(&self.config, &self.state, session_id)
            .map_err(|failure| anyhow::anyhow!("{}: {}", failure.code, failure.message))?;
        tracing::warn!(target: "csp", %session_id, "execution stopped: {reason}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn interrupted_withdrawal_keeps_exact_fence_and_never_publishes_stopped() {
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("work");
        std::fs::create_dir(&cwd).expect("work");
        let channel = Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        let create = crate::tests::create_event(&provider, channel, "withdrawal-interrupted");
        provider
            .handle_command_event(channel, &create)
            .await
            .expect("create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("record")
            .target(&provider.config.instance_id);
        let marker = crate::session_stop::begin_quiescence(
            &provider.config,
            &target,
            "withdrawn-authority",
            true,
        )
        .expect("persist intent");
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.closed = true;
                record.process_witness = None;
            })
            .expect("fence record");
        provider
            .sessions
            .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        assert!(provider
            .stop_for_withdrawn_authority(&target.session_id, "withdrawn")
            .await
            .is_err());
        assert!(marker.exists(), "interrupted intent retained for repair");
        assert!(
            !provider
                .outbox
                .pending_events()
                .filter(|event| event.kind.as_u16() == 44223)
                .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event.content).ok())
                .any(|value| value["status"] == "stopped"),
            "uncertainty must not be published as Stopped metadata"
        );
    }

    #[tokio::test]
    async fn repeated_cascade_and_state_recovery_retain_pending_checkpoint_and_context() {
        fn git(cwd: &std::path::Path, args: &[&str]) -> String {
            let mut command = crate::host_command::metadata_git_command(cwd);
            let out = command
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .output()
                .expect("fixture git");
            assert!(out.status.success(), "fixture git failed");
            String::from_utf8(out.stdout)
                .expect("git output")
                .trim()
                .into()
        }
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("work");
        std::fs::create_dir(&cwd).expect("work");
        git(&cwd, &["init", "-q", "-b", "main"]);
        git(
            &cwd,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
        let channel = Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        let create = crate::tests::create_event(&provider, channel, "cascade-pending-retry");
        provider
            .handle_command_event(channel, &create)
            .await
            .expect("create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("record")
            .target(&provider.config.instance_id);
        let project = format!(
            "30624:{}:pending",
            provider.config.keys.public_key().to_hex()
        );
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.project_ref = Some(project.clone());
                record.closed = true;
                record.process_witness = None; // interrupted generation has no proven capture
            })
            .expect("persist uncertain closed");
        let marker = crate::session_stop::begin_quiescence(
            &provider.config,
            &target,
            "withdrawn-authority",
            true,
        )
        .expect("intent");
        provider
            .sessions
            .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        let reference = format!("refs/beekeeper/checkpoints/{}/1/base", target.session_id);
        let head = git(&cwd, &["rev-parse", "HEAD"]);
        git(&cwd, &["update-ref", &reference, &head]);
        let package_id = Uuid::new_v4().to_string();
        let context_dir = provider
            .config
            .state_dir
            .join("context-packages")
            .join(&package_id);
        beekeeper_host_core::atomic_write::create_dir_all_restricted(&context_dir)
            .expect("private recovery context");
        let context = context_dir.join("0000000000.json");
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &context,
            b"{\"fixture\":\"retained-private-context\"}",
        )
        .expect("selected context fixture");
        provider.context_refresh.insert(
            target.session_id.clone(),
            ContextRefreshState::opened(package_id),
        );
        for attempt in 0..3 {
            if attempt == 2 {
                provider.state = StateStore::open(
                    &provider.config.state_dir,
                    provider.config.command_horizon.as_secs(),
                )
                .expect("durable StateStore recovery");
                provider
                    .recover()
                    .await
                    .expect("actual provider recovery without relay transport");
            }
            assert!(
                !provider.stop_project_executions(&project).await,
                "uncertain cascade must never schedule retirement on retry/recovery"
            );
            assert!(marker.exists(), "same exact pending fence retained");
            assert!(context.exists(), "selected context retained");
            assert!(
                !provider
                    .outbox
                    .pending_events()
                    .filter(|event| event.kind.as_u16() == 44223)
                    .filter_map(
                        |event| serde_json::from_str::<serde_json::Value>(&event.content).ok()
                    )
                    .any(|value| value["status"] == "stopped"),
                "pending recovery must not publish Stopped"
            );
            assert_eq!(
                git(&cwd, &["rev-parse", &reference]),
                head,
                "checkpoint recovery ref retained"
            );
        }
    }
    #[tokio::test]
    async fn provider_recovery_without_pending_stop_keeps_existing_context_sweep() {
        let dir = tempfile::tempdir().expect("fixture");
        let mut provider = crate::tests::provider(&dir.path().join("state"), None);
        let cache = provider
            .config
            .state_dir
            .join("context-packages")
            .join(Uuid::new_v4().to_string());
        beekeeper_host_core::atomic_write::create_dir_all_restricted(&cache)
            .expect("derived cache");
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &cache.join("0000000000.json"),
            b"{}",
        )
        .expect("derived fixture");
        provider.recover().await.expect("actual recovery");
        assert!(
            !cache.exists(),
            "ordinary no-pending sweep must remain unchanged"
        );
    }
}
