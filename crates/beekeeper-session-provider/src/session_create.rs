//! Existing session creation with managed admission and private configuration.
use super::*;

impl Provider {
    pub(crate) async fn create_session(
        &mut self,
        plan: CreatePlan,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        if self.actor_has_unproved_stop(plan.actor.as_deref()) {
            return self.refuse_with_staged_answer(
                plan.channel_id,
                &plan.command_id,
                &LifecycleReceipt::failed(
                    &plan.command_id,
                    "SESSION_QUIESCENCE_UNCONFIRMED",
                    "a predecessor stop is still unproved; no new responsibility is started",
                ),
                Some(coding_session_lifecycle_receipt_semantic_key(
                    &plan.command_id,
                )),
            );
        }
        let ownership_admission = match host_ownership::admit(
            &self.config,
            plan.actor.as_deref(),
            host_ownership::Admission::Create {
                command: &plan.command_id,
                initial_turn: plan.initial_turn.is_some(),
            },
        ) {
            Ok(guard) => guard,
            Err(failure) => {
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                )
            }
        };
        let reserved_target = self.reserve_create_start(&plan)?;
        self.state.consume_command(&plan.command_id, now_secs())?;
        self.state.advance_lifecycle_start(
            &plan.command_id,
            state::LifecycleStartPhase::EffectsStarted,
        )?;
        if let Err(failure) = self.require_actor_not_retired(plan.actor.as_deref()).await {
            self.state.consume_command(&plan.command_id, now_secs())?;
            if plan.actor.is_some() {
                self.forget_actor_seat(&plan.command_id);
            }
            self.consume_pending_hint(&plan.command_id, None);
            return self.enqueue_receipt(
                plan.channel_id,
                &plan.command_id,
                &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
            );
        }
        let outbox_before = self.outbox.pending_keys();

        // Infallible in practice: `decide_lifecycle` only mints a plan whose
        // ref matched a descriptor, and the runtime list never changes while
        // the provider runs. Failing the receipt is still better than a panic.
        let Some(mut descriptor) = self.config.runtime(&plan.runtime_instance_ref).cloned() else {
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                payload::PROVIDER_UNAVAILABLE,
                &format!(
                    "runtime {} disappeared between decision and dispatch",
                    plan.runtime_instance_ref
                ),
            );
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
        };

        let private_settings =
            match managed_configuration::Settings::load(&self.config, plan.actor.as_deref()) {
                Ok(settings) => settings,
                Err(failure) => {
                    return self.enqueue_receipt(
                        plan.channel_id,
                        &plan.command_id,
                        &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                    )
                }
            };
        if descriptor.instance_ref == "managed-actor" && private_settings.is_none() {
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &LifecycleReceipt::failed(&plan.command_id, "AGENT_PRIVATE_CONFIG_UNAVAILABLE", "managed-actor runtime requires protected configured command; no placeholder fallback"));
        }
        if let Some(settings) = &private_settings {
            match settings.parallelism() {
                Ok(Some(limit))
                    if self
                        .state
                        .sessions()
                        .filter(|record| {
                            record.actor == plan.actor && !record.closed && !record.is_retired()
                        })
                        .count()
                        >= limit =>
                {
                    return self.enqueue_receipt(
                        plan.channel_id,
                        &plan.command_id,
                        &LifecycleReceipt::failed(
                            &plan.command_id,
                            "AGENT_PARALLELISM_LIMIT",
                            "this actor has reached its configured responsibility limit",
                        ),
                    );
                }
                Ok(_) => {}
                Err(failure) => {
                    return self.enqueue_receipt(
                        plan.channel_id,
                        &plan.command_id,
                        &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                    )
                }
            }
            if let Err(failure) = settings.apply_runtime(&mut descriptor) {
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                );
            }
        }

        // A hired seat never runs in somebody else's tree. Item 80(a)/(b):
        // three seats joined one umbrella with the working directory the
        // dialog remembered — the checkout the app itself runs from — and
        // shared a git index and a HEAD, so each one's commits and branch
        // switches landed under the others. Refused before anything is
        // provisioned, and the staged key goes with the refusal.
        if plan.actor.is_some() {
            let refusal = {
                let live: Vec<session::LiveWorkdirClaim> = plan
                    .session_ref
                    .as_deref()
                    .map(|umbrella| {
                        self.state
                            .sessions()
                            .filter(|record| {
                                !record.closed && record.session_ref.as_deref() == Some(umbrella)
                            })
                            .map(|record| session::LiveWorkdirClaim {
                                cwd: record.cwd.clone(),
                                role: record.role.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                session::seated_workdir_refusal(&plan.cwd, &live, &session::shared_workdir_roots())
            };
            if let Some(failure) = refusal {
                tracing::warn!(
                    target: "csp",
                    command_id = %plan.command_id,
                    code = failure.code,
                    "seated create refused: {}", failure.message
                );
                self.forget_actor_seat(&plan.command_id);
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        }

        let target = reserved_target;
        // Full access ticked at creation was granted to this command's id;
        // it moves to the session before the session is prepared (ledger 303).
        match full_access::transfer(&self.config.state_dir, &plan.command_id, &target.session_id) {
            Ok(true) => tracing::info!(
                target: "csp::scope",
                session_id = %target.session_id,
                "full access granted at creation"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(
                target: "csp::scope",
                "full access granted at creation could not be applied; the session runs bounded: {error}"
            ),
        }
        let rehydration = self
            .prepare_rehydration_context(
                RehydrationTarget {
                    scope: RehydrationScope::Create,
                    command_id: &plan.command_id,
                    channel_id: plan.channel_id,
                    session_ref: plan.session_ref.as_deref(),
                    genesis_ref: plan.genesis_ref.as_deref(),
                    package_id: &target.session_id,
                },
                relay,
            )
            .await;
        let context_package_id = rehydration
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.package_id.clone());
        let unavailable_reason = rehydration.unavailable_reason;
        let rehydration_mcp = rehydration.descriptor;

        // Custody, resolved as late as possible and held as briefly as
        // possible: the seat is read here, converted straight into the child's
        // post-fence environment, and dropped at the end of this function. It
        // is deliberately not carried in `CreatePlan`, in `SessionRecord`, or
        // in anything this function logs.
        //
        // `decide_lifecycle` already refused a create whose seat this host does
        // not hold, so reaching this branch with no entry means the file moved
        // underneath us between the decision and the dispatch. That is still a
        // refusal, and the same one: an execution labelled as an agent that
        // cannot act as one is the lie this code exists to prevent.
        let (seat_identity, post_fence_env, seat_skills, seat_pack_ref) = match plan
            .actor
            .as_deref()
        {
            None => (None, Vec::new(), None, None),
            Some(actor) => {
                let seats = crate::actor_seats::ActorSeatsFile::load(
                    self.config.actor_seats_file.as_deref(),
                );
                match seats.seat(&plan.command_id) {
                    Some(seat) if seat.pubkey == actor => {
                        let identity = session::SeatIdentity {
                            actor_pubkey: seat.pubkey.clone(),
                            role: plan.role.clone().unwrap_or_default(),
                            relay_url: seat.relay_url.clone(),
                        };
                        // The pack is a host-local path staged beside the key,
                        // read here for the same reason the key is: it names
                        // machine state a signed create must never carry. Its
                        // `packRef` is the opposite kind of fact — the same on
                        // every machine — and is kept for the wire.
                        let skills = seat_skills(seat, &self.config.state_dir, &target.session_id);
                        let pack_ref = seat.pack_ref.clone();
                        (
                            Some(identity),
                            // The umbrella's project, so `bee pulse update`
                            // writes where the operator is looking rather than
                            // minting a project of its own (ledger 80 d) —
                            // plus the one `bee` this host chose, named
                            // absolutely and put first on the seat's PATH
                            // (item 103 finding 1).
                            seat.post_fence_env_with_bee(
                                plan.role.as_deref(),
                                plan.project_ref.as_deref(),
                                crate::seat_bee::host_seat_bee().map(|(bee, _)| bee),
                                std::env::var_os("PATH").as_ref(),
                            ),
                            skills,
                            pack_ref,
                        )
                    }
                    _ => {
                        self.forget_actor_seat(&plan.command_id);
                        self.discard_orphaned_context_package(context_package_id.as_deref());
                        let receipt = LifecycleReceipt::failed(
                            &plan.command_id,
                            payload::ACTOR_UNAVAILABLE,
                            "the agent seat's key material is no longer held by this host",
                        );
                        return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
                    }
                }
            }
        };
        // SV-33: every execution, seated or not, may drive its own preview.
        let mut post_fence_env = post_fence_env;
        crate::preview_grant::push_preview_env(
            &mut post_fence_env,
            &self.config,
            plan.channel_id,
            &target,
            &plan.command_id,
        );
        let mut seat_skills = seat_skills;
        if seat_skills.is_none() {
            if let Some(settings) = &private_settings {
                match settings.skills(&self.config.state_dir, &target.session_id) {
                    Ok(skills) => seat_skills = skills,
                    Err(failure) => {
                        return self.enqueue_receipt(
                            plan.channel_id,
                            &plan.command_id,
                            &LifecycleReceipt::failed(
                                &plan.command_id,
                                failure.code,
                                &failure.message,
                            ),
                        )
                    }
                }
            }
        }
        let seat_compose_ref = seat_skills
            .as_ref()
            .and_then(|skills| skills.compose_ref.clone());

        let mut agent_env =
            match runtime_credentials::environment(&self.config.state_dir, &descriptor) {
                Ok(env) => env,
                Err(failure) => {
                    if plan.actor.is_some() {
                        self.forget_actor_seat(&plan.command_id);
                    }
                    self.state.consume_command(&plan.command_id, now_secs())?;
                    let receipt =
                        LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                    return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
                }
            };
        if let Some(settings) = &private_settings {
            match settings.environment() {
                Ok(env) => agent_env.extend(env),
                Err(failure) => {
                    return self.enqueue_receipt(
                        plan.channel_id,
                        &plan.command_id,
                        &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                    )
                }
            }
        }
        let native_import = match managed_configuration::native(
            &self.config,
            plan.actor.as_deref(),
            &plan.command_id,
            plan.channel_id,
            &plan.cwd,
            &descriptor,
        ) {
            Ok(native) => native,
            Err(failure) => {
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                )
            }
        };
        if let Some((_, environment, _)) = &native_import {
            agent_env.extend(environment.clone());
        }
        let private_idle = match private_settings
            .as_ref()
            .map(|s| s.duration("idle_timeout_seconds", self.config.idle_timeout))
            .transpose()
        {
            Ok(value) => value.unwrap_or(self.config.idle_timeout),
            Err(failure) => {
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                )
            }
        };
        let private_turn = match private_settings
            .as_ref()
            .map(|s| s.duration("max_turn_duration_seconds", self.config.max_turn_duration))
            .transpose()
        {
            Ok(value) => value.unwrap_or(self.config.max_turn_duration),
            Err(failure) => {
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                )
            }
        };
        // An unseated execution reads its project's agents repository from a
        // read-only clone the host stages for it — never the host's own cache.
        // Revalidated at every start: an execution of a deleted project does
        // not start, and its own context is prepared or the start refused.
        let unseated_agents = if self.project_deleted(plan.project_ref.as_deref()).await {
            Err(execution_scope::refuse(
                execution_scope::EXECUTION_SCOPE_INVALID,
                "this project was deleted, so no execution of it starts",
            ))
        } else if plan.actor.is_none() {
            self.stage_unseated_agents(plan.project_ref.as_deref(), &target.session_id)
                .await
        } else {
            Ok(None)
        };
        // The execution scope, from this create's own project, seat and
        // rights — before any process exists.
        let execution = match unseated_agents
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|_| {
                self.execution_plan(&execution_scope::LaunchFacts {
                    session_id: &target.session_id,
                    runtime_instance: &descriptor.instance_ref,
                    project_ref: plan.project_ref.as_deref(),
                    association: if plan.workspace_bound {
                        execution_scope::WorkspaceAssociation::HostBound
                    } else {
                        execution_scope::WorkspaceAssociation::Unbound
                    },
                    unseated_agents: unseated_agents.as_ref().ok().and_then(Option::as_deref),
                    actor: plan.actor.as_deref(),
                    driver: &descriptor.driver,
                    cwd: &plan.cwd,
                    agent_command: &descriptor.agent_command,
                    agent_args: &descriptor.agent_args,
                    agent_env: &agent_env,
                    identity_env: &post_fence_env,
                    seat_skills: seat_skills.as_ref(),
                    role: plan.role.as_deref(),
                    rehydration: rehydration_mcp.as_ref(),
                    prior: None,
                })
            }) {
            Ok(execution) => execution,
            Err(failure) => {
                tracing::warn!(
                    target: "csp",
                    command_id = %plan.command_id,
                    code = failure.code,
                    "session creation refused: {}", failure.message
                );
                if plan.actor.is_some() {
                    self.forget_actor_seat(&plan.command_id);
                }
                self.discard_orphaned_context_package(context_package_id.as_deref());
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        };
        if let Some(import) = &native_import {
            if let Err(failure) = managed_configuration::materialize(import, &execution) {
                self.discard_orphaned_context_package(context_package_id.as_deref());
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                );
            }
        }
        let execution_binding = execution.binding().cloned();
        let execution_state = execution.state();

        let request = CreateRequest {
            execution,
            media: self.media.clone(),
            target: target.clone(),
            channel_id: plan.channel_id,
            cwd: plan.cwd.clone(),
            title: plan.title.clone(),
            model: plan.model.clone(),
            resume_cursor: native_import.as_ref().map(|(cursor, _, _)| cursor.clone()),
            // Never strict: an ordinary create or resume may legitimately
            // end up in a fresh conversation, and says so on the wire.
            strict_native: native_import.is_some(),
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

        self.state.advance_lifecycle_start(
            &plan.command_id,
            state::LifecycleStartPhase::RuntimeStarting,
        )?;
        let events = self.sessions.event_sender();
        let startup_future = SessionManager::start(request, events);
        let started = self
            .await_with_lease_maintenance(startup_future, relay.map(HarnessRelay::event_publisher))
            .await;
        let started = match started {
            Ok(started) => started,
            Err(failure) => {
                tracing::warn!(
                    target: "csp",
                    command_id = %plan.command_id,
                    code = failure.code,
                    "session creation failed: {}", failure.message
                );
                // An error carries no process-group exit witness. Retain the
                // package and one-shot custody alongside the unresolved intent.
                self.retire_unattached_context(None, context_package_id.as_deref())
                    .await;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        };
        if let Some(guard) = ownership_admission.as_ref() {
            if let Err(failure) = guard.recheck() {
                self.retire_unattached_context(Some(started), context_package_id.as_deref())
                    .await;
                return self.enqueue_receipt(
                    plan.channel_id,
                    &plan.command_id,
                    &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
                );
            }
        }
        if let Err(failure) = self.require_actor_not_retired(plan.actor.as_deref()).await {
            self.retire_unattached_context(Some(started), context_package_id.as_deref())
                .await;
            return self.enqueue_receipt(
                plan.channel_id,
                &plan.command_id,
                &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
            );
        }
        if let Err(failure) =
            crate::managed_enrollment::complete(&self.config, &self.state, &plan, &target)
        {
            self.retire_unattached_context(Some(started), context_package_id.as_deref())
                .await;
            return self.enqueue_receipt(
                plan.channel_id,
                &plan.command_id,
                &LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message),
            );
        }
        let startup = self.sessions.attach(started);
        #[cfg(test)]
        if std::mem::take(&mut self.state.fault_plan().fail_next_create_record_write) {
            return Err(anyhow::anyhow!(
                "injected crash after ACP start before record commit"
            ));
        }
        // What this exact process answered at `initialize`, recorded before
        // any metadata is built from it.
        self.steering
            .insert(target.session_id.clone(), startup.steering_supported);
        self.prompt_image
            .insert(target.session_id.clone(), startup.prompt_image_supported);
        self.model_switch
            .witness(&target.session_id, startup.model_switch_supported);

        // Cloned before the plan is partially moved into the record below.
        let hire_ref = plan.hire_ref.clone();
        let record = SessionRecord {
            session_id: target.session_id.clone(),
            generation: target.generation,
            channel_id: plan.channel_id,
            command_id: plan.command_id.clone(),
            generation_command_id: Some(plan.command_id.clone()),
            provider_instance_ref: descriptor.instance_ref.clone(),
            runtime: descriptor.runtime.clone(),
            driver: descriptor.driver.clone(),
            cwd: plan.cwd.clone(),
            project_ref: plan.project_ref.clone(),
            repo_ref: plan.repo_ref.clone(),
            session_ref: plan.session_ref.clone(),
            genesis_ref: plan.genesis_ref.clone(),
            actor: plan.actor.clone(),
            role: plan.role.clone(),
            compose_ref: seat_compose_ref,
            execution_binding,
            execution_boundary: crate::execution_scope::recorded_state(&execution_state),
            process_witness: Some(crate::process_witness::ExecutionProcessWitness {
                target: target.clone(),
                quiesced: false,
                group_id: self
                    .sessions
                    .handle(&target.session_id)
                    .and_then(session::SessionHandle::child_group),
            }),
            authority_withdrawn: None,
            project_head_seen_at: plan
                .project_ref
                .as_deref()
                .and_then(|project| self.seen_project_head(project)),
            pack_ref: seat_pack_ref,
            founder_pubkey: Some(plan.founder_pubkey.clone()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: startup.model.clone().or_else(|| plan.model.clone()),
            model_requested: plan.model.clone(),
            // What the adapter reports at open is its *selection* — for
            // claude-agent-acp the picker label (`opus`, `default`) — a request,
            // not the model that answers (ledger 272(d)). Unknown until a
            // turn's own result frame names one.
            model_effective: None,
            routing: plan.routing.clone(),
            resume_cursor: Some(startup.acp_session_id.clone()),
            title: plan.title.clone(),
            created_at_ms: now_ms(),
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: startup.bootstrap_transport,
            open_turn: None,
            closed: false,
            created_by: Some(plan.created_by.clone()),
            // Seeded from the umbrella, not left at `NoClaim`. A create that
            // reaches here under a claimed umbrella has already satisfied the
            // fence — it is the claimant, on the claimed body — but the record
            // it mints must carry the claim, or every later turn on this new
            // execution walks straight past a fence its siblings are behind.
            // An umbrella this provider holds no other record of folds to
            // `NoClaim`, and the first accepted receipt fills it in.
            handover: match plan.genesis_ref.as_deref() {
                None => ClaimState::NoClaim,
                Some(genesis_ref) => {
                    // The chain this create was fenced against, in preference
                    // to the local siblings: on a machine joining the umbrella
                    // for the first time there are no siblings, and that is
                    // the record that most needs to carry the claim.
                    let local = commands::umbrella_claim(&self.state, plan.channel_id, genesis_ref);
                    if matches!(plan.verified_claim, ClaimState::NoClaim) {
                        local
                    } else {
                        plan.verified_claim.clone()
                    }
                }
            },
            retired: None,
        };
        let accepted_outcome = self.accepted_create_disposition(&plan, &target)?;
        self.state.insert_session_with_lifecycle_outcome(
            record,
            &plan.command_id,
            accepted_outcome,
        )?;
        #[cfg(test)]
        if std::mem::take(&mut self.state.fault_plan().fail_next_create_initial_delivery) {
            return Err(anyhow::anyhow!(
                "injected crash after accepted record before initial delivery"
            ));
        }
        // Settlement: the record carrying the resolved cwd is now durable, so
        // the one-shot hint that produced it is spent and leaves a marker
        // saying what it produced. Deliberately after the persist and never
        // before it — see `consume_pending_hint`.
        self.consume_pending_hint(
            &plan.command_id,
            Some((plan.cwd.as_path(), target.session_id.as_str())),
        );
        // A seated create is the first moment this generation's custody could
        // ever need re-staging, so the request is stated as soon as the record
        // that implies it is durable.
        self.publish_seat_requests();
        // First create under an umbrella records who opened it. The D9 budget
        // exempts that pubkey and no other — an execution-scoped exemption
        // would let a delegated seat create its own session and buy itself an
        // unbounded allowance out of the crew's.
        if let Some(session_ref) = plan.session_ref.as_deref() {
            self.state
                .claim_umbrella_founder(session_ref, &plan.founder_pubkey)?;
        }

        if plan.actor.is_some() {
            // Consume only the one-shot delivery copy after accepted record custody.
            self.forget_actor_seat(&plan.command_id);
        }
        if let Some(package_id) = context_package_id {
            self.context_refresh.insert(
                target.session_id.clone(),
                ContextRefreshState::opened(package_id),
            );
        }

        // Who asked for this seat. Resolved after the record exists, because
        // the requester's role and reply address are read out of *this*
        // provider's own durable seat records — the same lookup a 44220 from a
        // sibling goes through — and never out of the hire's content.
        let attribution = self
            .hire_attribution(
                &plan.command_id,
                plan.channel_id,
                hire_ref.as_deref(),
                relay,
            )
            .await;
        if let Some(commands::HireAttribution::Disputed { claimed, signer }) = &attribution {
            tracing::warn!(
                target: "csp::hire",
                command_id = %plan.command_id,
                %claimed,
                %signer,
                "the hire this create answers claims a requester that did not sign it; the brief \
                 is delivered unattributed"
            );
            self.enqueue_transcript(
                plan.channel_id,
                &target,
                None,
                payload::status_item(HIRE_REQUESTER_DISPUTED_STATUS),
                Priority::High,
            )?;
        }
        // Only a claim the hire's own signer made is attribution. Everything
        // else — unclaimed, unreadable, disputed — delivers the founder-shaped
        // turn this path has always delivered, because an unknown requester
        // rendered as an attributed one is the exact failure this field exists
        // to end.
        let requester = match &attribution {
            Some(commands::HireAttribution::Attributed(pubkey)) => Some(pubkey.clone()),
            _ => None,
        };
        let initial_framing = match (&plan.initial_turn, requester.as_deref()) {
            (Some(text), Some(requester)) => self.turn_framing(
                &target.session_id,
                requester,
                CodingSessionDelivery::Boundary,
                plan.channel_id,
                text,
            ),
            _ => None,
        };
        let initial_operator = initial_framing
            .as_ref()
            .map(|framing| framing.sender_pubkey.clone())
            .unwrap_or_else(|| plan.founder_pubkey.clone());
        // A seat's first turn carries its situation card.
        let initial_card = match &plan.initial_turn {
            Some(text) => {
                self.seat_situation_card(&target.session_id, text, plan.hire_ref.as_deref())
                    .await
            }
            None => None,
        };
        let initial_framing = situation_card::attach_situation_card(
            initial_framing,
            initial_card,
            plan.channel_id,
            &initial_operator,
            CodingSessionDelivery::Boundary,
        );

        if let Some((status, reason)) = create_disclosure(&startup.continuity, unavailable_reason) {
            self.enqueue_transcript(
                plan.channel_id,
                &target,
                None,
                payload::status_item_with_reason(status, reason),
                Priority::High,
            )?;
        }
        // What this execution runs inside, disclosed on every open: enforced
        // with its backend, or not enforced with the reason.
        self.enqueue_transcript(
            plan.channel_id,
            &target,
            None,
            execution_scope::boundary_status_item(&execution_state),
            Priority::High,
        )?;
        // What the provider's isolation settings withheld, when any did.
        for item in crate::session_isolation::isolation_status_items(&execution_state) {
            self.enqueue_transcript(plan.channel_id, &target, None, item, Priority::High)?;
        }

        // A genesis may already carry an accepted authority chain — another
        // execution under the same umbrella can be granted operators before
        // this one exists. Fold those verified grants in now; live receipts
        // extend from here. Best-effort: a failed backfill leaves the session
        // founder-only until the next receipt or restart, never open.
        //
        // And its verdict is *kept*, because the initial turn below is handed
        // straight to the actor's mailbox without passing through
        // `decide_turn`. The create was already fenced against a chain read
        // before this adapter started; this is the second read, after the
        // record exists, and between those two moments a takeover can be
        // accepted. Ignoring what it learned is how a fenced execution still
        // ran its first prompt.
        let mut authority_refusal: Option<String> = None;
        if let Some(genesis_ref) = plan.genesis_ref.clone() {
            match relay {
                Some(relay) => {
                    let rest = relay.rest_client();
                    match self
                        .backfill_session_authority(&target.session_id, &rest)
                        .await
                    {
                        Ok(true) => {}
                        // Incomplete is not unclaimed: the umbrella joins the
                        // pending set and the first turn waits rather than
                        // running on the assumption that nothing changed.
                        Ok(false) => {
                            self.claims_pending_reverification
                                .insert(genesis_ref.clone());
                            authority_refusal = Some(format!(
                                "{}: this umbrella's authority chain could not be verified, so \
                                 whether the session has been handed over is not yet known and \
                                 the first turn was not delivered",
                                commands::AUTHORITY_NOT_REVERIFIED
                            ));
                        }
                        Err(error) => {
                            tracing::warn!(
                                target: "csp::authority",
                                session_id = %target.session_id,
                                "authority backfill at create failed: {error}"
                            );
                            self.claims_pending_reverification
                                .insert(genesis_ref.clone());
                            authority_refusal = Some(format!(
                                "{}: this umbrella's authority chain could not be read, so the \
                                 first turn was not delivered",
                                commands::AUTHORITY_NOT_REVERIFIED
                            ));
                        }
                    }
                }
                None => {
                    self.claims_pending_reverification
                        .insert(genesis_ref.clone());
                    authority_refusal = Some(format!(
                        "{}: no relay reader was available to verify this umbrella's authority \
                         chain, so the first turn was not delivered",
                        commands::AUTHORITY_NOT_REVERIFIED
                    ));
                }
            }
            // What the second read actually learned. A takeover or a void
            // accepted between the create's own fence check and this moment
            // lands here, and the prompt does not go out.
            //
            // Judged with the create's **signer**, not `initial_operator` and
            // not the founder. For a genesis-bearing create the founder is the
            // genesis's signer, so a claimant reconstructing somebody else's
            // session has a founder who is not them — and attributing their
            // own seeded first turn to that founder made this branch refuse
            // the very continuation the handover exists to allow, on the body
            // they had just claimed. A non-claimant signer on the claimed
            // body, and any signer on a body the claim does not name, are
            // still refused.
            if authority_refusal.is_none() {
                if let Some(refusal) = self.state.session(&target.session_id).and_then(|record| {
                    commands::handover_fence(record, &plan.created_by, &self.pubkey_hex)
                }) {
                    authority_refusal = Some(format!("{}: {}", refusal.code, refusal.message));
                }
            }
        }

        // The umbrella's published policy, read *here* and not earlier: the
        // predicate for "who may set policy" is the founder plus this
        // session's `granted_operators`, and that set is empty until the
        // authority backfill directly above has folded the chain. Read before
        // the backfill — as this call used to be — and only the founder's
        // ceiling could ever bind a create, because the grant set the fold
        // consults did not exist yet. Still ahead of the budget check below,
        // which is what may now be enforcing the ceiling this read records.
        self.refresh_policy_turn_budget(&target.session_id, relay)
            .await;

        // The initial turn is *dispatched* before the receipt is decided, so
        // `created_with_failed_initial_turn` means exactly what a consumer can
        // act on: the session exists but its first turn never reached the agent.
        // A turn that reaches the agent and then fails is a transcript
        // `result{error}`, not a lifecycle outcome — the session is fine and the
        // operator can simply try again.
        // D9 / contract B, on the one turn path that never passes through
        // `decide_turn`: a create's first turn is handed straight to the
        // actor's mailbox, so without this check one signed create would run —
        // and charge — a turn at an exhausted allowance, with no receipt
        // saying so. Same predicate as a 44220 turn's, so the two paths can
        // never disagree about who is exempt.
        let budget_refusal = plan.initial_turn.as_ref().and_then(|_| {
            commands::exhausted_umbrella_budget(
                &self.state,
                self.config.turn_budget,
                plan.session_ref
                    .as_deref()
                    .and_then(|session_ref| self.policy_turn_budget(session_ref)),
                plan.session_ref.as_deref(),
                &plan.founder_pubkey,
            )
        });
        let dispatch_error = match (&plan.initial_turn, self.sessions.handle(&target.session_id)) {
            // The handover fence and the authority read, ahead of the budget:
            // a turn nobody may send is not a turn that spent an allowance.
            (Some(_), _) if authority_refusal.is_some() => authority_refusal.clone(),
            _ if budget_refusal.is_some() => budget_refusal.map(|(used, limit, source)| {
                format!(
                    "{}: {}, so the first turn was not delivered",
                    payload::BUDGET_EXHAUSTED,
                    commands::budget_exhausted_message(used, limit, source)
                )
            }),
            (None, _) => None,
            (Some(_), None) => Some("session actor stopped before the first turn".to_owned()),
            (Some(text), Some(handle)) => handle
                .deliver(SessionCommand::Turn {
                    // The create's own `commandId`, not a derived one: the
                    // first turn's `user_prompt` echo has to name a command
                    // the operator actually sent, or the prompt they typed
                    // into the create dialog is the one prompt in the session
                    // they cannot join to anything.
                    command_id: plan.command_id.clone(),
                    text: text.clone(),
                    // A create's brief is text: 44221 carries no attachment
                    // field, so there is nothing to forward here.
                    attachments: Vec::new(),
                    // Who drove this first turn. The create's verified signer
                    // by default — the same fact that made them founder — but
                    // a create that answers a hire whose requester equals the
                    // hire's own signer is delivering *that seat's* brief, and
                    // the signed transcript has to say so.
                    operator_pubkey: Some(initial_operator.clone()),
                    // Framed exactly like a 44220 from the same seat: a hired
                    // seat's first words are a message from its lead, not
                    // words its founder typed.
                    framing: initial_framing.clone(),
                })
                .err()
                .map(|error| format!("could not deliver the first turn: {error:?}")),
        };

        let receipt = match &dispatch_error {
            None => LifecycleReceipt::created(&plan.command_id, &target),
            Some(message) => LifecycleReceipt::created_with_failed_initial_turn(
                &plan.command_id,
                &target,
                message,
            ),
        };
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)?;
        let status = if plan.initial_turn.is_some() && dispatch_error.is_none() {
            SessionStatus::Running
        } else {
            SessionStatus::Idle
        };
        // Started, not awaited: the first metadata publication goes out now,
        // without a branch, and is corrected when the observation lands. Waiting
        // here would delay this session's metadata *and* — because the probe ran
        // on the provider loop — every other session's transcripts.
        self.spawn_git_probe(&target.session_id);
        self.publish_metadata(plan.channel_id, &target, status)?;
        self.record_first_lease_prerequisites(&target, &outbox_before);

        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %target.session_id,
            "session created"
        );
        // If the second chain read learned a claim that fences this body, the
        // adapter that was started a moment ago can never take a turn. Release
        // it here — after the create's own receipt and metadata, so the
        // consumer sees the session appear and then go `disconnected` with the
        // handover block on it, rather than a create that answers nothing.
        if plan.genesis_ref.is_some()
            && self
                .state
                .session(&target.session_id)
                .is_some_and(|record| {
                    commands::handover_fence(record, &self.pubkey_hex, &self.pubkey_hex)
                        .is_some_and(|refusal| refusal.code == payload::HANDOVER_FENCED)
                })
        {
            if let Some(genesis_ref) = plan.genesis_ref.clone() {
                self.enforce_claim_for_genesis(&genesis_ref).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "session_create_tests.rs"]
mod tests;

impl Provider {
    pub(crate) async fn retire_unattached_context(
        &self,
        started: Option<session::StartedSession>,
        package_id: Option<&str>,
    ) -> session::ActorQuiescence {
        let proof = match started {
            Some(started) => started.discard().await,
            None => session::ActorQuiescence::Unproven,
        };
        if proof == session::ActorQuiescence::Unproven {
            tracing::warn!(target: "csp::context", "startup retirement is unproven; context and custody retained for repair");
        } else {
            self.discard_orphaned_context_package(package_id);
        }
        proof
    }
}
