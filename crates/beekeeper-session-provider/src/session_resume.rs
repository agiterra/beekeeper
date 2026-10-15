//! Existing generation resume, shared by lifecycle and native recovery.
use super::*;

impl Provider {
    /// The resume path, shared by `session.resume`, `session.restart` and —
    /// with `rewind` — `session.rewind` (SV-29). A rewind opens the next
    /// generation with **no** native cursor (`session/new`) and a context
    /// package cut at the rewind, answers with its `rewind` object, and turns
    /// every failure to open into `REWIND_NOT_RESTARTED`
    /// ([`Self::rewind_not_restarted`]) rather than a bare refusal.
    pub(crate) async fn resume_generation(
        &mut self,
        plan: ResumePlan,
        relay: Option<&HarnessRelay>,
        rewind: Option<crate::rewind::RewindOpen>,
    ) -> anyhow::Result<()> {
        if self.state.unresolved_start_target(&plan.target) {
            self.state.consume_command(&plan.command_id, now_secs())?;
            return self.answer_resume_failure(&plan, rewind, LifecycleReceipt::failed(&plan.command_id, payload::CREATE_START_OUTCOME_UNCONFIRMED, "startup effects remain uncertain; no replacement child is launched until explicit repair"), relay).await;
        }
        let actor = self
            .state
            .session(&plan.target.session_id)
            .and_then(|record| record.actor.clone());
        if let Err(failure) = self.require_actor_not_retired(actor.as_deref()).await {
            self.state.consume_command(&plan.command_id, now_secs())?;
            return self.enqueue_receipt(
                plan.channel_id,
                &plan.command_id,
                &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
            );
        }
        let outbox_before = self.outbox.pending_keys();
        // Every refusal below consumes the command, so the seat the desktop
        // staged for it will never be read: it goes with the refusal, the same
        // one-shot rule the create path follows. Reads the record rather than
        // the custody file, so an unseated resume does no extra work.
        let seated = self
            .state
            .session(&plan.target.session_id)
            .is_some_and(|record| record.actor.is_some());
        if self
            .sessions
            .handle(&plan.target.session_id)
            .is_some_and(session::SessionHandle::is_live)
        {
            self.state.consume_command(&plan.command_id, now_secs())?;
            if seated {
                self.forget_actor_seat(&plan.command_id);
            }
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                SESSION_ALREADY_ATTACHED,
                "the execution is already attached on this provider",
            );
            return self
                .answer_resume_failure(&plan, rewind, receipt, relay)
                .await;
        }

        let Some(record) = self.state.session(&plan.target.session_id).cloned() else {
            return Ok(());
        };
        if !self.recovered_group_absent(&plan.target) {
            self.state.consume_command(&plan.command_id, now_secs())?;
            return self
                .answer_resume_failure(
                    &plan,
                    rewind,
                    LifecycleReceipt::failed(
                        &plan.command_id,
                        "SESSION_QUIESCENCE_UNCONFIRMED",
                        "the predecessor group is present or unknown; no successor is launched",
                    ),
                    relay,
                )
                .await;
        }
        let ownership_admission = match host_ownership::admit(
            &self.config,
            record.actor.as_deref(),
            host_ownership::Admission::Resume,
        ) {
            Ok(guard) => guard,
            Err(failure) => {
                self.state.consume_command(&plan.command_id, now_secs())?;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self
                    .answer_resume_failure(&plan, rewind, receipt, relay)
                    .await;
            }
        };

        // Revalidated at every start: an execution whose own authority was
        // withdrawn does not come back under it.
        if let Some(reason) = record.authority_withdrawn.as_deref() {
            self.state.consume_command(&plan.command_id, now_secs())?;
            if seated {
                self.forget_actor_seat(&plan.command_id);
            }
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                payload::ACTOR_UNAVAILABLE,
                &format!("{reason}, so this execution will not be resumed"),
            );
            return self
                .answer_resume_failure(&plan, rewind, receipt, relay)
                .await;
        }
        let Some(mut descriptor) = self.config.runtime(&record.provider_instance_ref).cloned()
        else {
            self.state.consume_command(&plan.command_id, now_secs())?;
            if seated {
                self.forget_actor_seat(&plan.command_id);
            }
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                PROVIDER_UNAVAILABLE,
                "the execution's runtime is no longer installed",
            );
            return self
                .answer_resume_failure(&plan, rewind, receipt, relay)
                .await;
        };
        let private_settings =
            match managed_configuration::Settings::load(&self.config, record.actor.as_deref()) {
                Ok(settings) => settings,
                Err(failure) => {
                    self.state.consume_command(&plan.command_id, now_secs())?;
                    if seated {
                        self.forget_actor_seat(&plan.command_id);
                    }
                    let receipt =
                        LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                    return self
                        .answer_resume_failure(&plan, rewind, receipt, relay)
                        .await;
                }
            };
        if descriptor.instance_ref == "managed-actor" && private_settings.is_none() {
            self.state.consume_command(&plan.command_id, now_secs())?;
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                "AGENT_PRIVATE_CONFIG_UNAVAILABLE",
                "managed-actor runtime requires protected configured adapter",
            );
            return self
                .answer_resume_failure(&plan, rewind, receipt, relay)
                .await;
        }
        if let Some(settings) = &private_settings {
            if let Err(failure) = settings.apply_runtime(&mut descriptor) {
                self.state.consume_command(&plan.command_id, now_secs())?;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self
                    .answer_resume_failure(&plan, rewind, receipt, relay)
                    .await;
            }
        }
        let Some(generation) = record
            .generation
            .checked_add(1)
            .filter(|value| *value <= beekeeper_core::coding_session_command::MAX_SAFE_GENERATION)
        else {
            self.state.consume_command(&plan.command_id, now_secs())?;
            if seated {
                self.forget_actor_seat(&plan.command_id);
            }
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                PROVIDER_UNAVAILABLE,
                "the execution generation cannot advance further",
            );
            return self
                .answer_resume_failure(&plan, rewind, receipt, relay)
                .await;
        };
        let target = CodingSessionTarget {
            driver: record.driver.clone(),
            instance_id: self.config.instance_id.clone(),
            session_id: record.session_id.clone(),
            generation,
        };
        let previous_semantic_key = coding_session_target_key(&self.target_for(&record));
        let package_id = Uuid::new_v4().to_string();
        // A rewind's package is cut at the rewind; everything else's is whole.
        let cuts: Vec<context_projector::ContextCut> =
            rewind.iter().map(|open| open.cut.clone()).collect();
        let rehydration = self
            .prepare_rehydration_context_cut(
                RehydrationTarget {
                    scope: RehydrationScope::Resume,
                    command_id: &plan.command_id,
                    channel_id: record.channel_id,
                    session_ref: record.session_ref.as_deref(),
                    genesis_ref: record.genesis_ref.as_deref(),
                    package_id: &package_id,
                },
                cuts.clone(),
                relay,
            )
            .await;
        // A rewound generation never reattaches the native cursor: it still
        // remembers the rewound turns. `session/new`, seeded from the record.
        let resume_cursor = if rewind.is_some() {
            None
        } else {
            record.resume_cursor.clone()
        };
        let context_package_id = rehydration
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.package_id.clone());
        // `prepare_rehydration_context` speaks the create path's language: a
        // missing umbrella `sessionRef`/`genesisRef` there means this is the
        // session's first execution. On a resume an earlier generation of this
        // very execution already ran — its turns are in the transcript this row
        // lands in — so the same missing pair means only that the execution was
        // never created under an umbrella. Publishing `no_prior_execution` here
        // would render "this is the session's first execution, so there was no
        // prior work to carry" directly above that prior work.
        let unavailable_reason = match rehydration.unavailable_reason {
            Some("no_prior_execution") => Some("no_umbrella_context"),
            other => other,
        };
        let rehydration_mcp = rehydration.descriptor;

        // A resumed agent seat needs its identity again, and the entry that
        // carried it was consumed by the create. So the resume names its own
        // custody entry, keyed by the resume command's own `commandId` — the
        // same one-shot hand-off, one generation later.
        //
        // Refusing is the only honest answer when it is absent. The alternative
        // — reattach the execution with the fence intact — produces a
        // generation whose 44223 still says `agentRef: <seat>` while the
        // process behind it holds no credentials at all, which is precisely the
        // "control that lies about what it enforces" class of bug.
        let (seat_identity, post_fence_env, mut seat_skills, seat_pack_ref) =
            match record.actor.as_deref() {
                None => (None, Vec::new(), None, None),
                Some(actor) => {
                    let seats = crate::actor_seats::ActorSeatsFile::load(
                        self.config.actor_seats_file.as_deref(),
                    );
                    let conversation_seat = self.conversation_actor_seat(&record)?;
                    match seats.seat(&plan.command_id).or(conversation_seat.as_ref()) {
                        Some(seat) if seat.pubkey == actor => {
                            let identity = session::SeatIdentity {
                                actor_pubkey: seat.pubkey.clone(),
                                role: record.role.clone().unwrap_or_default(),
                                relay_url: seat.relay_url.clone(),
                            };
                            // A resume re-materializes the pack's skills: the
                            // workdir may have moved on since the create, and the
                            // write is a no-op when it has not.
                            let skills =
                                seat_skills(seat, &self.config.state_dir, &record.session_id);
                            // …and re-states which pack that was, because a resume
                            // may have been staged from a moved ref. The new
                            // generation's 44223 must describe the pack it is
                            // actually running, not the one the create ran.
                            let pack_ref = seat.pack_ref.clone();
                            (
                                Some(identity),
                                // Same coordinate the create carried: a resumed
                                // generation writes the same project's pulse — and
                                // the same host-chosen `bee`, so a reconnect does
                                // not quietly change which binary the seat runs.
                                seat.post_fence_env_with_bee(
                                    record.role.as_deref(),
                                    record.project_ref.as_deref(),
                                    crate::seat_bee::host_seat_bee().map(|(bee, _)| bee),
                                    std::env::var_os("PATH").as_ref(),
                                ),
                                skills,
                                pack_ref,
                            )
                        }
                        _ => {
                            self.state.consume_command(&plan.command_id, now_secs())?;
                            self.forget_actor_seat(&plan.command_id);
                            self.discard_orphaned_context_package(context_package_id.as_deref());
                            // Precise about which fact is missing: this host may
                            // well hold the agent's key and simply have staged
                            // nothing under *this* command. Saying "this host does
                            // not hold the key" on the machine that does is the
                            // falsehood class this project treats as a bug.
                            let receipt = LifecycleReceipt::failed(
                            &plan.command_id,
                            payload::ACTOR_UNAVAILABLE,
                            "no key material was staged for this reconnect; reconnect from the \
                             host that holds this agent's key",
                        );
                            return self
                                .answer_resume_failure(&plan, rewind, receipt, relay)
                                .await;
                        }
                    }
                }
            };
        // SV-33: every execution, seated or not, may drive its own preview.
        let mut post_fence_env = post_fence_env;
        crate::preview_grant::push_preview_env(
            &mut post_fence_env,
            &self.config,
            record.channel_id,
            &target,
            &plan.command_id,
        );
        let seat_compose_ref = seat_skills
            .as_ref()
            .and_then(|skills| skills.compose_ref.clone());

        let mut agent_env =
            match runtime_credentials::environment(&self.config.state_dir, &descriptor) {
                Ok(env) => env,
                Err(failure) => {
                    self.state.consume_command(&plan.command_id, now_secs())?;
                    if record.actor.is_some() {
                        self.forget_actor_seat(&plan.command_id);
                    }
                    self.discard_orphaned_context_package(context_package_id.as_deref());
                    let receipt =
                        LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                    return self
                        .answer_resume_failure(&plan, rewind, receipt, relay)
                        .await;
                }
            };
        if let Some(settings) = &private_settings {
            match settings.environment() {
                Ok(environment) => agent_env.extend(environment),
                Err(failure) => {
                    self.state.consume_command(&plan.command_id, now_secs())?;
                    let receipt =
                        LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                    return self
                        .answer_resume_failure(&plan, rewind, receipt, relay)
                        .await;
                }
            }
            if seat_skills.is_none() {
                match settings.skills(&self.config.state_dir, &record.session_id) {
                    Ok(skills) => seat_skills = skills,
                    Err(failure) => {
                        self.state.consume_command(&plan.command_id, now_secs())?;
                        let receipt = LifecycleReceipt::failed(
                            &plan.command_id,
                            failure.code,
                            &failure.message,
                        );
                        return self
                            .answer_resume_failure(&plan, rewind, receipt, relay)
                            .await;
                    }
                }
            }
        }
        // Same scope derivation as the create, from the record — plus the
        // record's binding, so native history is reattached only to the scope
        // that wrote it.
        // Revalidated at every start: an execution of a deleted project does
        // not start, and its own context is prepared or the start refused.
        let unseated_agents = if self.project_deleted(record.project_ref.as_deref()).await {
            Err(execution_scope::refuse(
                execution_scope::EXECUTION_SCOPE_INVALID,
                "this project was deleted, so no execution of it starts",
            ))
        } else if record.actor.is_none() {
            self.stage_unseated_agents(record.project_ref.as_deref(), &record.session_id)
                .await
        } else {
            Ok(None)
        };
        let execution = match unseated_agents
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|_| {
                self.execution_plan(&execution_scope::LaunchFacts {
                    session_id: &record.session_id,
                    runtime_instance: &descriptor.instance_ref,
                    project_ref: record.project_ref.as_deref(),
                    association: execution_scope::recorded_association(&record),
                    unseated_agents: unseated_agents.as_ref().ok().and_then(Option::as_deref),
                    actor: record.actor.as_deref(),
                    driver: &record.driver,
                    cwd: &record.cwd,
                    agent_command: &descriptor.agent_command,
                    agent_args: &descriptor.agent_args,
                    agent_env: &agent_env,
                    identity_env: &post_fence_env,
                    seat_skills: seat_skills.as_ref(),
                    role: record.role.as_deref(),
                    rehydration: rehydration_mcp.as_ref(),
                    prior: Some((record.execution_binding.as_ref(), resume_cursor.as_deref())),
                })
            }) {
            Ok(execution) => execution,
            Err(failure) => {
                self.state.consume_command(&plan.command_id, now_secs())?;
                if record.actor.is_some() {
                    self.forget_actor_seat(&plan.command_id);
                }
                self.retire_unattached_context(None, context_package_id.as_deref())
                    .await;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self
                    .answer_resume_failure(&plan, rewind, receipt, relay)
                    .await;
            }
        };
        let execution_binding = execution.binding().cloned();
        let execution_state = execution.state();

        let private_timeouts: Result<
            Option<(std::time::Duration, std::time::Duration)>,
            session::CreateFailure,
        > = private_settings
            .as_ref()
            .map(|settings| {
                Ok((
                    settings.duration("idle_timeout_seconds", self.config.idle_timeout)?,
                    settings
                        .duration("max_turn_duration_seconds", self.config.max_turn_duration)?,
                ))
            })
            .transpose();
        let (private_idle, private_turn) = match private_timeouts {
            Ok(value) => value.unwrap_or((self.config.idle_timeout, self.config.max_turn_duration)),
            Err(failure) => {
                self.state.consume_command(&plan.command_id, now_secs())?;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self
                    .answer_resume_failure(&plan, rewind, receipt, relay)
                    .await;
            }
        };
        let request = CreateRequest {
            execution,
            media: self.media.clone(),
            target: target.clone(),
            channel_id: record.channel_id,
            cwd: record.cwd.clone(),
            title: record.title.clone(),
            model: record.model.clone(),
            resume_cursor: resume_cursor.clone(),
            // Imported native continuity cannot silently become a fresh conversation.
            // Ordinary existing resume keeps its preexisting disclosed fallback.
            strict_native: false,
            rehydration_mcp,
            agent_command: descriptor.agent_command.clone(),
            agent_args: descriptor.agent_args.clone(),
            agent_env,
            seat: seat_identity,
            post_fence_env,
            seat_skills,
            idle_timeout: private_idle,
            answer_stall_timeout: self.config.answer_stall_timeout,
            emit_raw_sdk_frames: self.config.emit_raw_sdk_frames,
            max_turn_duration: private_turn,
            idle_shutdown: self.config.session_idle_shutdown,
            include_thoughts: self.config.include_thoughts,
            transcript_paragraph_flush: self.config.transcript_paragraph_flush,
        };
        self.stage_reopening(&record.session_id, &target)?;
        let events = self.sessions.event_sender();
        #[cfg(test)]
        let (started, failed_witness) =
            if rewind.is_some() && std::mem::take(&mut self.rewind_fail_next_open) {
                drop(request);
                (
                    Err(session::CreateFailure {
                        code: PROVIDER_UNAVAILABLE,
                        message: "injected: the rewound generation's adapter did not start".into(),
                    }),
                    crate::process_witness::ExecutionProcessWitness {
                        target: target.clone(),
                        attempted_target: None,
                        group_id: None,
                        quiesced: true,
                    },
                )
            } else {
                let startup_future = SessionManager::start_observed(request, events);
                self.await_with_lease_maintenance(
                    startup_future,
                    relay.map(HarnessRelay::event_publisher),
                )
                .await
            };
        #[cfg(not(test))]
        let (started, failed_witness) = {
            let startup_future = SessionManager::start_observed(request, events);
            self.await_with_lease_maintenance(
                startup_future,
                relay.map(HarnessRelay::event_publisher),
            )
            .await
        };
        if record.actor.is_some() {
            self.forget_actor_seat(&plan.command_id);
        }
        let started = match started {
            Ok(started) => started,
            Err(failure) => {
                self.state.consume_command(&plan.command_id, now_secs())?;
                let retained = self.record_failed_startup(&record, &failed_witness)?;
                if retained && failed_witness.quiesced {
                    self.discard_orphaned_context_package(context_package_id.as_deref());
                } else {
                    tracing::warn!(target: "csp::context", "failed resume cleanup is unproven or scope moved; context retained");
                }
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self
                    .answer_resume_failure(&plan, rewind, receipt, relay)
                    .await;
            }
        };
        if let Some(guard) = ownership_admission.as_ref() {
            if let Err(failure) = guard.recheck() {
                let proof = self
                    .retire_unattached_context(Some(started), context_package_id.as_deref())
                    .await;
                let mut witness = failed_witness;
                witness.quiesced = proof != session::ActorQuiescence::Unproven;
                self.record_failed_startup(&record, &witness)?;
                self.state.consume_command(&plan.command_id, now_secs())?;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self
                    .answer_resume_failure(&plan, rewind, receipt, relay)
                    .await;
            }
        }
        let startup = self.sessions.attach(started);
        self.steering
            .insert(record.session_id.clone(), startup.steering_supported);
        self.prompt_image
            .insert(record.session_id.clone(), startup.prompt_image_supported);
        self.model_switch
            .witness(&record.session_id, startup.model_switch_supported);

        let group_id = self
            .sessions
            .handle(&target.session_id)
            .and_then(session::SessionHandle::child_group);
        if let Err(error) = self.state.update_session(&record.session_id, |record| {
            record.process_witness = Some(crate::process_witness::ExecutionProcessWitness {
                target: target.clone(),
                attempted_target: None,
                group_id,
                quiesced: false,
            });
            record.generation = generation;
            record.generation_command_id = Some(plan.command_id.clone());
            record.next_seq = 1;
            record.next_lease_sequence = 1;
            record.open_turn = None;
            record.closed = false;
            record.resume_cursor = Some(startup.acp_session_id.clone());
            record.execution_binding = execution_binding.clone();
            record.execution_boundary = crate::execution_scope::recorded_state(&execution_state);
            // This generation's pack, not the previous one's: a seat restaged
            // from a moved ref runs a different commit, and the 44223 has to
            // say which.
            record.pack_ref = seat_pack_ref.clone();
            record.compose_ref = seat_compose_ref.clone();
            if startup.model.is_some() {
                record.model = startup.model.clone();
            }
            // `model_effective` is left as the last turn's own report: the
            // selection the adapter reports at open is a request, never an
            // effective model (ledger 272(d)). A record written before that
            // rule may hold a picker label there; it is cleared, not carried.
            if record
                .model_effective
                .as_deref()
                .is_some_and(crate::price_table::is_picker_label)
            {
                record.model_effective = None;
            }
        }) {
            self.sessions.shutdown(&record.session_id);
            return Err(error.into());
        }
        // An admitted generation continues the same responsibility; preserve
        // consumed request targets while moving future thread inputs forward.
        if let Err(failure) = crate::managed_enrollment::follow_generation(
            &self.config,
            record.actor.as_deref(),
            &self.target_for(&record),
            &target,
        ) {
            self.sessions
                .shutdown_and_join(&record.session_id, session::ACTOR_RETIRE_GRACE)
                .await;
            return Err(anyhow::anyhow!(
                "accepted generation requires ownership reconciliation: {}",
                failure.message
            ));
        }
        self.refresh_conversation_generation(&self.target_for(&record), &target)?;
        // The generation's command id just moved to this resume's, and that is
        // the key custody is filed under. Restate it before anything else can
        // read the old one.
        self.publish_seat_requests();
        self.established_leases.remove(&previous_semantic_key);
        self.pending_leases.remove(&previous_semantic_key);
        self.state.consume_command(&plan.command_id, now_secs())?;

        // The previous generation's package is unreferenced from here: a
        // resume only proceeds against an execution that is not live, so its
        // sidecar is gone, and this open minted a fresh package id and a fresh
        // directory. Dropping it now is what keeps a long-lived session from
        // leaking one package directory per resume.
        self.discard_context_packages(&target.session_id);
        if let Some(package_id) = context_package_id {
            let mut refresh = ContextRefreshState::opened(package_id);
            refresh.cuts = cuts;
            self.context_refresh
                .insert(target.session_id.clone(), refresh);
        }

        // Only the two restarted arms can carry a package reason. On a native
        // resume or load the adapter re-attached its own conversation, so
        // whether a verified package could also be built is irrelevant to what
        // the agent can see, and naming a failure there would imply a loss that
        // did not happen. `session_rehydrated` lost nothing either.
        // A rewind answers with its own receipt and opening row (SV-29).
        // A runtime that keeps its own conversation still holds it however
        // this open went: Beekeeper restored nothing, but "restarted without
        // prior context" would be untrue. Its receipt keeps
        // `resumed_without_context` (Beekeeper did not recover context) with
        // a message saying the runtime may hold its own, and the opening row
        // is withheld in favour of `session_runtime_context`, which carries
        // the withheld row's reason. (A rewind of such a runtime is refused
        // before it gets here: Beekeeper cannot cut the runtime's memory.)
        let runtime_context = self.runtime_keeps_context(&target.session_id);
        let mut context_reason = None;
        let (receipt, opening_item) = if let Some(open) = rewind {
            let (receipt, item) = crate::rewind::rewound_answer(
                &plan.command_id,
                &target,
                open,
                &startup.continuity,
                unavailable_reason,
            );
            (receipt, Some(item))
        } else {
            let (receipt, status, reason) = match startup.continuity {
                SessionContinuity::Resumed => (
                    LifecycleReceipt::resumed(&plan.command_id, &target),
                    "session_resumed",
                    None,
                ),
                SessionContinuity::Loaded => (
                    LifecycleReceipt::resumed(&plan.command_id, &target),
                    "session_loaded",
                    None,
                ),
                SessionContinuity::Rehydrated => (
                    LifecycleReceipt::resumed(&plan.command_id, &target),
                    "session_rehydrated",
                    None,
                ),
                SessionContinuity::RestartedWithoutContext { reason } => (
                    LifecycleReceipt::resumed_without_context(&plan.command_id, &target, reason),
                    "session_restarted_without_context",
                    unavailable_reason,
                ),
                SessionContinuity::Fresh => (
                    LifecycleReceipt::resumed_without_context(
                        &plan.command_id,
                        &target,
                        "the provider had no saved session cursor",
                    ),
                    "session_restarted_without_context",
                    unavailable_reason,
                ),
            };
            if runtime_context && status == "session_restarted_without_context" {
                context_reason = reason;
                (
                    LifecycleReceipt::resumed_without_context(
                        &plan.command_id,
                        &target,
                        RUNTIME_CONTEXT_RESUME_MESSAGE,
                    ),
                    None,
                )
            } else {
                (
                    receipt,
                    Some(payload::status_item_with_reason(status, reason)),
                )
            }
        };
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)?;
        if let Some(opening_item) = opening_item {
            self.enqueue_transcript(plan.channel_id, &target, None, opening_item, Priority::High)?;
        }
        // What the provider's isolation settings withheld, when any did; for
        // a runtime that executes remotely, that the boundary is the
        // adapter's only; and for one that keeps its own context, that this
        // execution may remember more than Beekeeper gave it.
        for item in self.open_disclosure_for(&target.session_id, &execution_state, context_reason) {
            self.enqueue_transcript(plan.channel_id, &target, None, item, Priority::High)?;
        }
        self.publish_metadata(plan.channel_id, &target, SessionStatus::Idle)?;
        self.record_first_lease_prerequisites(&target, &outbox_before);
        // A restart lost every in-memory policy ceiling, and a resume is where
        // a seat comes back. Re-read it here or the umbrella runs unbounded
        // under a policy its founder published and can see.
        self.refresh_policy_turn_budget(&target.session_id, relay)
            .await;
        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %target.session_id,
            generation,
            "session generation attached"
        );
        Ok(())
    }
}

/// The `resumed_without_context` message for a runtime that keeps its own
/// conversation: what Beekeeper did not do, and what the runtime may still
/// hold.
const RUNTIME_CONTEXT_RESUME_MESSAGE: &str = "Beekeeper restored no prior context for this \
     generation, but this runtime keeps its own conversation across sessions and may still hold \
     it";
