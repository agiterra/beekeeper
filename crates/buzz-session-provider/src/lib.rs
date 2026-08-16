//! Coding-session provider adapter.
//!
//! Consumes signed operator intent — kind 44221 `session.create` and kind 44220
//! turn commands — from Buzz channels, drives Claude Code over ACP, and
//! publishes the four provider-authored facts a consumer projects: the provider
//! catalog (44222), per-generation metadata (44223), lifecycle receipts (44224),
//! and transcript items (44225).
//!
//! # Boundaries this crate holds
//!
//! - **No host paths in signed content.** The working directory a session runs
//!   in is resolved from `BUZZ_CSP_PROJECTS_FILE`, stored in local state, and
//!   never serialized into an event. A test in this module asserts it.
//! - **At most once per command.** A `commandId` is durably consumed before any
//!   side effect, so a replayed subscription re-derives no state.
//! - **Fenced to the exact generation.** A turn addressed at a generation this
//!   provider is not running is dropped, never delivered late to a session the
//!   operator was no longer looking at.
//! - **Gaps, never duplicates.** Transcript sequences are reserved and persisted
//!   before publication, so a crash burns a number rather than reusing one.
//! - **The agent is not the provider.** Adapters are spawned behind
//!   [`agent_fence::FENCE`], so a session's agent cannot read the signing key
//!   whose events consumers trust as provider fact.
//!
//! # Environment
//!
//! `BUZZ_PRIVATE_KEY`, `BUZZ_RELAY_URL`, `BUZZ_AUTH_TAG`, `RUST_LOG`, plus the
//! `BUZZ_CSP_*` surface documented on [`config::Config`].

#![deny(unsafe_code)]

mod agent_fence;
pub mod catalog;
pub mod commands;
pub mod config;
mod git_probe;
mod model_catalog;
pub mod payload;
pub mod publish;
pub mod session;
pub mod state;
pub mod transcript;

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, SystemTime};

use nostr::{Event, Kind};
use tokio::sync::mpsc;
use uuid::Uuid;

use buzz_acp::relay::HarnessRelay;
use buzz_acp::{ChannelFilter, TurnUsage};
use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_PROVIDER_CATALOG,
    KIND_CODING_SESSION_TRANSCRIPT, KIND_MEMBER_ADDED_NOTIFICATION,
    KIND_MEMBER_REMOVED_NOTIFICATION,
};
use buzz_sdk::builders::{
    build_coding_session_lifecycle_receipt, build_coding_session_metadata,
    build_coding_session_provider_catalog, build_coding_session_transcript_item,
};
use buzz_sdk::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, coding_session_metadata_semantic_key,
    coding_session_provider_catalog_semantic_key, coding_session_transcript_semantic_key,
    MAX_TRANSCRIPT_CONTENT_BYTES,
};

use commands::{
    decide_lifecycle, CommandContext, CreatePlan, Ignored, LifecycleDecision, ProjectsFile,
    ResumePlan, StopPlan, TurnDecision,
};
use config::Config;
use payload::{
    Capabilities, LifecycleReceipt, SessionMetadata, SessionStatus, TranscriptEnvelope,
    GENESIS_NOT_FOUND, METADATA_SCHEMA, PROVIDER_UNAVAILABLE, SESSION_ALREADY_ATTACHED,
    UNAUTHORIZED_OPERATOR,
};
use publish::{EventSink, Outbox, Priority};
use session::{
    CreateRequest, SessionCommand, SessionContinuity, SessionEvent, SessionManager, TurnOutcome,
};
use state::{now_ms, now_secs, CatalogState, OpenTurn, SessionRecord, StateStore};

/// How often the outbox is drained when nothing else is happening.
const OUTBOX_TICK: Duration = Duration::from_secs(2);
/// Backlog of actor reports the provider loop will buffer.
const SESSION_EVENT_CAPACITY: usize = 256;
/// A genesis published immediately before its create may take a brief moment
/// to become query-visible through the HTTP bridge.
const GENESIS_QUERY_ATTEMPTS: usize = 4;
const GENESIS_QUERY_RETRY_DELAY: Duration = Duration::from_millis(150);

/// Entry point: read the environment and run until shutdown.
pub async fn run() -> anyhow::Result<()> {
    init_tracing();
    let mut config = Config::from_env()?;
    model_catalog::discover(&mut config).await;
    run_with(config).await
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Run the provider against an already-resolved configuration.
pub async fn run_with(config: Config) -> anyhow::Result<()> {
    let mut provider = Provider::new(config)?;
    provider.recover()?;

    let pubkey_hex = provider.config.pubkey_hex();
    tracing::info!(
        target: "csp",
        pubkey = %pubkey_hex,
        instance_id = %provider.config.instance_id,
        "coding-session provider starting"
    );

    let mut relay = HarnessRelay::connect(
        &provider.config.relay_url,
        &provider.config.keys,
        &pubkey_hex,
        provider.config.auth_tag.clone(),
    )
    .await?;
    relay.set_startup_watermark(now_secs()).await?;

    let channels = relay.discover_channels().await?;
    for channel_id in channels.keys().copied() {
        provider.subscribe(&mut relay, channel_id, None).await?;
    }
    relay.subscribe_membership_notifications().await?;
    provider.refresh_catalog(true)?;

    let publisher = relay.event_publisher();
    let mut ticker = tokio::time::interval(OUTBOX_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            event = relay.next_event() => match event {
                Some(event) => {
                    if let Err(error) = provider
                        .handle_relay_event(&mut relay, event.channel_id, &event.event)
                        .await
                    {
                        tracing::error!(target: "csp", "failed to handle event: {error}");
                    }
                }
                None => {
                    if let Err(error) = relay.reconnect().await {
                        return Err(error.into());
                    }
                }
            },
            Some(event) = provider.next_session_event() => {
                if let Err(error) = provider.handle_session_event(event) {
                    tracing::error!(target: "csp", "failed to record session event: {error}");
                }
            }
            _ = ticker.tick() => {
                // Hot-reload: an operator who adds a project to the file should
                // see it offered without restarting the provider.
                if let Err(error) = provider.refresh_catalog(false) {
                    tracing::error!(target: "csp", "catalog refresh failed: {error}");
                }
                if let Err(error) = provider.flush(&publisher).await {
                    tracing::error!(target: "csp", "outbox flush failed: {error}");
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(target: "csp", "shutdown requested");
                break;
            }
        }
    }

    let _ = provider.flush(&publisher).await;
    relay.shutdown().await;
    Ok(())
}

/// What the provider last said about one session's generation.
///
/// The content is kept to suppress a republication that would say exactly the
/// same thing; the status is kept so a correction that arrives out of band — a
/// worktree observation landing after the publication it belongs to — can
/// restate the lifecycle it found rather than guess a new one.
struct PublishedMetadata {
    content: String,
    status: SessionStatus,
}

/// The provider's whole runtime state, minus the relay socket.
pub struct Provider {
    config: Config,
    pubkey_hex: String,
    state: StateStore,
    outbox: Outbox,
    sessions: SessionManager,
    session_events: mpsc::Receiver<SessionEvent>,
    /// Sender for the queue [`Provider::next_session_event`] drains.
    ///
    /// Held so a spawned worktree probe can report back through the one inbox
    /// the loop already selects on, rather than through a parallel channel with
    /// its own ordering and shutdown rules.
    session_events_tx: mpsc::Sender<SessionEvent>,
    last_metadata: HashMap<String, PublishedMetadata>,
    /// Last bounded git observation per session id.
    ///
    /// A side map rather than a `SessionRecord` field on purpose: the
    /// observation is a fact about the host filesystem *right now*, so it must
    /// not survive a restart into a metadata publication that predates a
    /// re-probe. It exists because [`Provider::metadata_for`] is sync `&self`
    /// and cannot await: the probe runs on its own task and parks its result
    /// here when [`SessionEvent::WorktreeObserved`] reaches the loop.
    git_probes: HashMap<String, git_probe::GitProbe>,
    /// Generation of the most recently *launched* worktree probe per session.
    ///
    /// Bumped synchronously in [`Provider::spawn_git_probe`], before the probe
    /// ever touches the filesystem, so this always names the newest launch —
    /// never merely the newest arrival. [`SessionEvent::WorktreeObserved`]
    /// results are fenced against it (see the invariant documented on that
    /// match arm). Absent entry means "no probe has ever been launched for
    /// this session id," which compares as generation `0`.
    git_probe_generation: HashMap<String, u64>,
    subscribed: BTreeSet<Uuid>,
    projects_fingerprint: Option<(SystemTime, u64)>,
}

impl Provider {
    /// Open durable state and the publish outbox for `config`.
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let pubkey_hex = config.pubkey_hex();
        let state = StateStore::open(&config.state_dir, config.command_horizon.as_secs())?;
        let outbox = Outbox::open(&config.state_dir, &pubkey_hex)?;
        let (events_tx, session_events) = mpsc::channel(SESSION_EVENT_CAPACITY);
        Ok(Self {
            config,
            pubkey_hex,
            state,
            outbox,
            sessions: SessionManager::new(events_tx.clone()),
            session_events,
            session_events_tx: events_tx,
            last_metadata: HashMap::new(),
            git_probes: HashMap::new(),
            git_probe_generation: HashMap::new(),
            subscribed: BTreeSet::new(),
            projects_fingerprint: None,
        })
    }

    /// Repair state left behind by an unclean exit.
    ///
    /// Two repairs, both consequences of the same fact: a restarted provider has
    /// no process behind any session it previously owned, and v1 never re-binds
    /// a generation.
    ///
    /// 1. A create that minted a session record but died before its `commandId`
    ///    reached the ledger would replay into a *second* session for one
    ///    command, so the ledger is reconciled against the records first.
    /// 2. Every session that was still open is detached: a turn caught mid-flight
    ///    gets the terminal `result` item its consumer is waiting on — without
    ///    it the turn renders as running forever — and every open generation
    ///    gets `disconnected` metadata. The durable record remains resumable;
    ///    only an explicit `session.stop` retires it.
    pub fn recover(&mut self) -> anyhow::Result<()> {
        let orphans: Vec<(String, String)> = self
            .state
            .sessions()
            .filter(|record| !self.state.is_command_consumed(&record.command_id))
            .map(|record| (record.command_id.clone(), record.session_id.clone()))
            .collect();
        for (command_id, session_id) in orphans {
            tracing::warn!(
                target: "csp::recovery",
                %session_id,
                %command_id,
                "session record exists without a consumed command — reconciling"
            );
            self.state.consume_command(&command_id, now_secs())?;
        }

        let stranded: Vec<SessionRecord> = self
            .state
            .sessions()
            .filter(|record| !record.closed)
            .cloned()
            .collect();
        for record in stranded {
            let target = self.target_for(&record);
            if let Some(open_turn) = &record.open_turn {
                tracing::warn!(
                    target: "csp::recovery",
                    session_id = %record.session_id,
                    turn_id = %open_turn.turn_id,
                    "turn was in flight when the provider stopped — synthesizing its result"
                );
                if let Some(command_id) = &open_turn.command_id {
                    self.state.consume_command(command_id, now_secs())?;
                }
                self.enqueue_transcript(
                    record.channel_id,
                    &target,
                    Some(&open_turn.turn_id),
                    payload::result_item(
                        payload::ResultSubtype::Error,
                        u64::try_from(now_ms().saturating_sub(open_turn.started_at_ms))
                            .unwrap_or_default(),
                        "provider terminated mid-turn",
                        payload::TurnCost::default(),
                    ),
                    Priority::High,
                )?;
            }
            self.state.update_session(&record.session_id, |record| {
                record.open_turn = None;
            })?;
            self.publish_metadata(record.channel_id, &target, SessionStatus::Disconnected)?;
        }
        Ok(())
    }

    /// Await the next actor report. `None` once every actor is gone.
    pub async fn next_session_event(&mut self) -> Option<SessionEvent> {
        self.session_events.recv().await
    }

    /// Advertise the catalog wherever it is not yet current.
    ///
    /// Called at startup, whenever a new channel is joined, and on every tick.
    /// `force` skips the projects-file change check — the tick path uses a
    /// modified-time comparison so a file that never changes costs a `stat`
    /// rather than a read, a hash, and a serialization.
    ///
    /// The revision advances only when the canonical body changes. That is what
    /// makes "the highest revision I have seen from this signer" mean "the
    /// newest thing on offer": a restart or a projects-file rewrite that alters
    /// nothing must not look like new capabilities.
    pub fn refresh_catalog(&mut self, force: bool) -> anyhow::Result<()> {
        let fingerprint = catalog::fingerprint(self.config.projects_file.as_deref());
        let unchanged_file = !force && fingerprint == self.projects_fingerprint;
        let already_everywhere = self
            .subscribed
            .iter()
            .all(|channel| self.state.catalog().advertised_channels.contains(channel));
        if unchanged_file && already_everywhere && self.state.catalog().revision > 0 {
            return Ok(());
        }
        self.projects_fingerprint = fingerprint;

        let projects = ProjectsFile::load(self.config.projects_file.as_deref());
        let stored = self.state.catalog().clone();
        let probe = catalog::build(&self.config, &projects, stored.revision.max(1));
        let digest = catalog::body_digest(&probe);

        let (revision, mut advertised) =
            if stored.content_digest.as_deref() == Some(&digest) && stored.revision > 0 {
                (stored.revision, stored.advertised_channels.clone())
            } else {
                // New content: a fresh revision, advertised nowhere yet.
                (stored.revision.saturating_add(1).max(1), Vec::new())
            };

        let catalog = catalog::build(&self.config, &projects, revision);
        let content = catalog::to_canonical_json(&catalog)?;
        let mut published = 0usize;
        for channel_id in self.subscribed.clone() {
            if advertised.contains(&channel_id) {
                continue;
            }
            let event = build_coding_session_provider_catalog(channel_id, revision, &content)?
                .sign_with_keys(&self.config.keys)?;
            self.outbox.enqueue(
                KIND_CODING_SESSION_PROVIDER_CATALOG,
                &coding_session_provider_catalog_semantic_key(
                    &channel_id.to_string(),
                    revision,
                    &content,
                ),
                Priority::High,
                event,
            )?;
            advertised.push(channel_id);
            published += 1;
        }

        self.state.set_catalog(CatalogState {
            revision,
            content_digest: Some(digest),
            advertised_channels: advertised,
        })?;
        if published > 0 {
            tracing::info!(
                target: "csp::catalog",
                revision,
                channels = published,
                "advertised provider catalog"
            );
        }
        Ok(())
    }

    /// Subscribe to the two command kinds in one channel, replaying from the
    /// persisted watermark so a restart cannot silently skip an unseen command.
    async fn subscribe(
        &mut self,
        relay: &mut HarnessRelay,
        channel_id: Uuid,
        membership_created_at: Option<u64>,
    ) -> Result<(), buzz_acp::relay::RelayError> {
        let filter = ChannelFilter {
            kinds: Some(vec![
                KIND_CODING_SESSION_COMMAND,
                KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            ]),
            require_mention: false,
        };
        // A newly granted membership and the first command are published back
        // to back. `subscribe_channel_from` only queues the REQ, so falling
        // back to `since=now` when the background relay task eventually sends
        // it can skip the command by one second. Replay from the membership
        // notification until a consumed-command watermark exists.
        let replay_since = self.subscription_replay_since(channel_id, membership_created_at);
        relay
            .subscribe_channel_from(channel_id, filter, replay_since)
            .await?;
        self.subscribed.insert(channel_id);
        Ok(())
    }

    fn subscription_replay_since(
        &self,
        channel_id: Uuid,
        membership_created_at: Option<u64>,
    ) -> Option<u64> {
        self.state.watermark(channel_id).or(membership_created_at)
    }

    /// Route one relay event.
    async fn handle_relay_event(
        &mut self,
        relay: &mut HarnessRelay,
        channel_id: Uuid,
        event: &Event,
    ) -> anyhow::Result<()> {
        let kind = u32::from(event.kind.as_u16());
        match kind {
            KIND_MEMBER_ADDED_NOTIFICATION => {
                tracing::info!(target: "csp", %channel_id, "membership granted — subscribing");
                self.subscribe(relay, channel_id, Some(event.created_at.as_secs()))
                    .await?;
                // A channel that cannot see the catalog cannot create a session
                // in it, so advertising is part of joining, not a side effect.
                self.refresh_catalog(false)?;
            }
            KIND_MEMBER_REMOVED_NOTIFICATION => {
                tracing::info!(target: "csp", %channel_id, "membership revoked — unsubscribing");
                self.subscribed.remove(&channel_id);
                relay.unsubscribe_channel(channel_id).await?;
            }
            _ => {
                self.handle_command_event_inner(channel_id, event, Some(&*relay))
                    .await?;
            }
        }
        Ok(())
    }

    /// Handle one 44220 or 44221 event and advance the channel watermark.
    pub async fn handle_command_event(
        &mut self,
        channel_id: Uuid,
        event: &Event,
    ) -> anyhow::Result<()> {
        self.handle_command_event_inner(channel_id, event, None)
            .await
    }

    async fn handle_command_event_inner(
        &mut self,
        channel_id: Uuid,
        event: &Event,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        let kind = u32::from(event.kind.as_u16());
        let created_at = event.created_at.as_secs();
        let operator_pubkey = event.pubkey.to_hex();
        match kind {
            KIND_CODING_SESSION_LIFECYCLE_COMMAND => {
                self.on_lifecycle(
                    channel_id,
                    created_at,
                    &operator_pubkey,
                    &event.content,
                    relay,
                )
                .await?;
            }
            KIND_CODING_SESSION_COMMAND => {
                self.on_turn(channel_id, created_at, &operator_pubkey, &event.content)
                    .await?;
            }
            other => {
                tracing::debug!(target: "csp", kind = other, "ignoring unrelated event");
                return Ok(());
            }
        }
        self.state.record_watermark(channel_id, created_at)?;
        Ok(())
    }

    async fn on_lifecycle(
        &mut self,
        channel_id: Uuid,
        created_at: u64,
        operator_pubkey: &str,
        content: &str,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        // Re-read on every command so an operator can fix a missing working
        // directory and republish without restarting the provider.
        let projects = ProjectsFile::load(self.config.projects_file.as_deref());
        let decision = decide_lifecycle(
            &self.context(&projects, operator_pubkey),
            channel_id,
            created_at,
            content,
        );
        match decision {
            LifecycleDecision::Ignore(reason) => {
                log_ignored("session.create", &reason);
                Ok(())
            }
            LifecycleDecision::Fail {
                command_id,
                code,
                message,
            } => {
                // Consume first: at-most-once side effects is the stronger
                // guarantee. A crash between here and the enqueue leaves the
                // consumer's create pending, which its durable-create
                // transaction already knows how to retry under a fresh id.
                self.state.consume_command(&command_id, now_secs())?;
                let receipt = LifecycleReceipt::failed(&command_id, code, &message);
                tracing::warn!(target: "csp", %command_id, code, "lifecycle command rejected: {message}");
                self.enqueue_receipt(channel_id, &command_id, &receipt)
            }
            LifecycleDecision::Create(plan) => {
                let mut plan = *plan;
                if let Some(genesis_ref) = plan.genesis_ref.as_deref() {
                    let founder = match relay {
                        Some(relay) => {
                            resolve_genesis_founder(
                                relay,
                                channel_id,
                                plan.session_ref.as_deref(),
                                genesis_ref,
                            )
                            .await
                        }
                        None => Err("no relay resolver is available for this command".into()),
                    };
                    match founder {
                        Ok(founder) => plan.founder_pubkey = founder,
                        Err(message) => {
                            self.state.consume_command(&plan.command_id, now_secs())?;
                            let receipt = LifecycleReceipt::failed(
                                &plan.command_id,
                                GENESIS_NOT_FOUND,
                                &message,
                            );
                            tracing::warn!(
                                target: "csp",
                                command_id = %plan.command_id,
                                genesis_ref,
                                "genesis-bearing create rejected: {message}"
                            );
                            return self.enqueue_receipt(
                                plan.channel_id,
                                &plan.command_id,
                                &receipt,
                            );
                        }
                    }
                }
                self.create_session(plan).await
            }
            LifecycleDecision::Resume(plan) => self.resume_session(plan).await,
            LifecycleDecision::Stop(plan) => self.stop_session(plan),
        }
    }

    async fn create_session(&mut self, plan: CreatePlan) -> anyhow::Result<()> {
        self.state.consume_command(&plan.command_id, now_secs())?;

        // Infallible in practice: `decide_lifecycle` only mints a plan whose
        // ref matched a descriptor, and the runtime list never changes while
        // the provider runs. Failing the receipt is still better than a panic.
        let Some(descriptor) = self.config.runtime(&plan.runtime_instance_ref).cloned() else {
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

        let target = CodingSessionTarget {
            driver: descriptor.driver.clone(),
            instance_id: self.config.instance_id.clone(),
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
        let request = CreateRequest {
            target: target.clone(),
            channel_id: plan.channel_id,
            cwd: plan.cwd.clone(),
            title: plan.title.clone(),
            model: plan.model.clone(),
            resume_cursor: None,
            agent_command: descriptor.agent_command.clone(),
            agent_args: descriptor.agent_args.clone(),
            agent_env: descriptor
                .cli_env
                .iter()
                .map(|env| (env.name.clone(), env.value.clone()))
                .collect(),
            idle_timeout: self.config.idle_timeout,
            max_turn_duration: self.config.max_turn_duration,
            idle_shutdown: self.config.session_idle_shutdown,
            include_thoughts: self.config.include_thoughts,
        };

        let startup = match self.sessions.create(request).await {
            Ok(startup) => startup,
            Err(failure) => {
                tracing::warn!(
                    target: "csp",
                    command_id = %plan.command_id,
                    code = failure.code,
                    "session creation failed: {}", failure.message
                );
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        };

        let record = SessionRecord {
            session_id: target.session_id.clone(),
            generation: target.generation,
            channel_id: plan.channel_id,
            command_id: plan.command_id.clone(),
            provider_instance_ref: descriptor.instance_ref.clone(),
            runtime: descriptor.runtime.clone(),
            driver: descriptor.driver.clone(),
            cwd: plan.cwd,
            project_ref: plan.project_ref.clone(),
            repo_ref: plan.repo_ref.clone(),
            session_ref: plan.session_ref.clone(),
            genesis_ref: plan.genesis_ref.clone(),
            founder_pubkey: Some(plan.founder_pubkey.clone()),
            model: startup.model.clone().or_else(|| plan.model.clone()),
            resume_cursor: Some(startup.acp_session_id.clone()),
            title: plan.title.clone(),
            created_at_ms: now_ms(),
            next_seq: 1,
            open_turn: None,
            closed: false,
        };
        self.state.insert_session(record)?;

        // The initial turn is *dispatched* before the receipt is decided, so
        // `created_with_failed_initial_turn` means exactly what a consumer can
        // act on: the session exists but its first turn never reached the agent.
        // A turn that reaches the agent and then fails is a transcript
        // `result{error}`, not a lifecycle outcome — the session is fine and the
        // operator can simply try again.
        let dispatch_error = match (&plan.initial_turn, self.sessions.handle(&target.session_id)) {
            (None, _) => None,
            (Some(_), None) => Some("session actor stopped before the first turn".to_owned()),
            (Some(text), Some(handle)) => handle
                .deliver(SessionCommand::Turn {
                    command_id: format!("{}:initial", plan.command_id),
                    text: text.clone(),
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

        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %target.session_id,
            "session created"
        );
        Ok(())
    }

    async fn resume_session(&mut self, plan: ResumePlan) -> anyhow::Result<()> {
        if self
            .sessions
            .handle(&plan.target.session_id)
            .is_some_and(session::SessionHandle::is_live)
        {
            self.state.consume_command(&plan.command_id, now_secs())?;
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                SESSION_ALREADY_ATTACHED,
                "the execution is already attached on this provider",
            );
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
        }

        let Some(record) = self.state.session(&plan.target.session_id).cloned() else {
            return Ok(());
        };
        let Some(descriptor) = self.config.runtime(&record.provider_instance_ref).cloned() else {
            self.state.consume_command(&plan.command_id, now_secs())?;
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                PROVIDER_UNAVAILABLE,
                "the execution's runtime is no longer installed",
            );
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
        };
        let Some(generation) = record
            .generation
            .checked_add(1)
            .filter(|value| *value <= buzz_core::coding_session_command::MAX_SAFE_GENERATION)
        else {
            self.state.consume_command(&plan.command_id, now_secs())?;
            let receipt = LifecycleReceipt::failed(
                &plan.command_id,
                PROVIDER_UNAVAILABLE,
                "the execution generation cannot advance further",
            );
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
        };
        let target = CodingSessionTarget {
            driver: record.driver.clone(),
            instance_id: self.config.instance_id.clone(),
            session_id: record.session_id.clone(),
            generation,
        };
        let request = CreateRequest {
            target: target.clone(),
            channel_id: record.channel_id,
            cwd: record.cwd.clone(),
            title: record.title.clone(),
            model: record.model.clone(),
            resume_cursor: record.resume_cursor.clone(),
            agent_command: descriptor.agent_command.clone(),
            agent_args: descriptor.agent_args.clone(),
            agent_env: descriptor
                .cli_env
                .iter()
                .map(|env| (env.name.clone(), env.value.clone()))
                .collect(),
            idle_timeout: self.config.idle_timeout,
            max_turn_duration: self.config.max_turn_duration,
            idle_shutdown: self.config.session_idle_shutdown,
            include_thoughts: self.config.include_thoughts,
        };
        let startup = match self.sessions.create(request).await {
            Ok(startup) => startup,
            Err(failure) => {
                self.state.consume_command(&plan.command_id, now_secs())?;
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        };

        if let Err(error) = self.state.update_session(&record.session_id, |record| {
            record.generation = generation;
            record.next_seq = 1;
            record.open_turn = None;
            record.closed = false;
            record.resume_cursor = Some(startup.acp_session_id.clone());
            if startup.model.is_some() {
                record.model = startup.model.clone();
            }
        }) {
            self.sessions.shutdown(&record.session_id);
            return Err(error.into());
        }
        self.state.consume_command(&plan.command_id, now_secs())?;

        let (receipt, status) = match startup.continuity {
            SessionContinuity::Resumed => (
                LifecycleReceipt::resumed(&plan.command_id, &target),
                "session_resumed",
            ),
            SessionContinuity::Loaded => (
                LifecycleReceipt::resumed(&plan.command_id, &target),
                "session_loaded",
            ),
            SessionContinuity::RestartedWithoutContext { reason } => (
                LifecycleReceipt::resumed_without_context(&plan.command_id, &target, reason),
                "session_restarted_without_context",
            ),
            SessionContinuity::Fresh => (
                LifecycleReceipt::resumed_without_context(
                    &plan.command_id,
                    &target,
                    "the provider had no saved session cursor",
                ),
                "session_restarted_without_context",
            ),
        };
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)?;
        self.enqueue_transcript(
            plan.channel_id,
            &target,
            None,
            payload::status_item(status),
            Priority::High,
        )?;
        self.publish_metadata(plan.channel_id, &target, SessionStatus::Idle)?;
        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %target.session_id,
            generation,
            "session generation attached"
        );
        Ok(())
    }

    fn stop_session(&mut self, plan: StopPlan) -> anyhow::Result<()> {
        // Persist the operator's terminal intent before signalling the actor.
        // If the process dies between this write and command-ledger append, a
        // replay is harmless and completes the same stop transaction.
        self.state
            .update_session(&plan.target.session_id, |record| {
                record.closed = true;
                record.open_turn = None;
            })?;
        self.state.consume_command(&plan.command_id, now_secs())?;
        self.sessions.shutdown(&plan.target.session_id);
        let receipt = LifecycleReceipt::stopped(&plan.command_id, &plan.target);
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)?;
        self.publish_metadata(plan.channel_id, &plan.target, SessionStatus::Stopped)?;
        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %plan.target.session_id,
            "session stopped"
        );
        Ok(())
    }

    async fn on_turn(
        &mut self,
        channel_id: Uuid,
        created_at: u64,
        operator_pubkey: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let projects = ProjectsFile::default();
        let decision = commands::decide_turn(
            &self.context(&projects, operator_pubkey),
            created_at,
            content,
        );
        let (command_id, session_id, message) = match decision {
            TurnDecision::Ignore(reason) => {
                log_ignored("turn", &reason);
                return Ok(());
            }
            TurnDecision::Fail {
                command_id,
                message,
            } => {
                self.state.consume_command(&command_id, now_secs())?;
                let receipt =
                    LifecycleReceipt::failed(&command_id, UNAUTHORIZED_OPERATOR, &message);
                tracing::warn!(
                    target: "csp",
                    %command_id,
                    %operator_pubkey,
                    "turn command rejected: {message}"
                );
                return self.enqueue_receipt(channel_id, &command_id, &receipt);
            }
            TurnDecision::Start {
                command_id,
                target,
                text,
            } => (
                command_id.clone(),
                target.session_id,
                SessionCommand::Turn { command_id, text },
            ),
            TurnDecision::Interrupt { command_id, target } => (
                command_id.clone(),
                target.session_id,
                SessionCommand::Interrupt { command_id },
            ),
        };

        self.state.consume_command(&command_id, now_secs())?;
        match self.sessions.handle(&session_id) {
            Some(handle) => {
                if let Err(error) = handle.deliver(message) {
                    tracing::warn!(
                        target: "csp",
                        %command_id,
                        %session_id,
                        "could not deliver command to session: {error:?}"
                    );
                }
            }
            None => tracing::warn!(
                target: "csp",
                %command_id,
                %session_id,
                "no live actor for a persisted session — command dropped"
            ),
        }
        Ok(())
    }

    fn context<'a>(
        &'a self,
        projects: &'a ProjectsFile,
        operator_pubkey: &'a str,
    ) -> CommandContext<'a> {
        CommandContext {
            provider_pubkey: &self.pubkey_hex,
            operator_pubkey,
            runtimes: &self.config.runtimes,
            instance_id: &self.config.instance_id,
            now_secs: now_secs(),
            horizon_secs: self.config.command_horizon.as_secs(),
            max_sessions: self.config.max_sessions,
            active_session_count: self.sessions.live_count(),
            state: &self.state,
            projects,
        }
    }

    /// Build the metadata payload describing one generation in `status`.
    pub fn metadata_for(
        &self,
        target: &CodingSessionTarget,
        status: SessionStatus,
    ) -> SessionMetadata {
        let record = self.state.session(&target.session_id);
        // The record is the durable truth about which runtime serves this
        // session, so metadata stays correct even if the descriptor disappears
        // across a restart. A target with no record falls back to the first
        // descriptor — claude in practice, same shape as before.
        let (provider_ref, runtime_slug) = match record {
            Some(record) => (record.provider_instance_ref.clone(), record.runtime.clone()),
            None => {
                let first = self.config.runtimes.first();
                (
                    first
                        .map(|descriptor| descriptor.instance_ref.clone())
                        .unwrap_or_else(|| config::PROVIDER_INSTANCE_REF.to_owned()),
                    first
                        .map(|descriptor| descriptor.runtime.clone())
                        .unwrap_or_else(|| config::RUNTIME.to_owned()),
                )
            }
        };
        let descriptor = self.config.runtime(&provider_ref);
        SessionMetadata {
            schema: METADATA_SCHEMA.to_owned(),
            session: target.clone(),
            project_ref: record.and_then(|record| record.project_ref.clone()),
            repo_ref: record.and_then(|record| record.repo_ref.clone()),
            title: payload::nullable(record.and_then(|record| record.title.as_deref())),
            agent_ref: None,
            provider: Some(provider_ref),
            runtime: Some(runtime_slug.clone()),
            model: record
                .and_then(|record| record.model.clone())
                .or_else(|| descriptor.map(|descriptor| descriptor.default_model.clone()))
                .or_else(|| Some(config::DEFAULT_MODEL.to_owned())),
            status,
            // Filled from the last bounded worktree observation, or left null
            // when the cwd is not a repository, sits on a detached HEAD, or was
            // never successfully observed. Note what is *not* here: the probe
            // also learns dirty state, and deliberately keeps it internal —
            // carrying that as a signed fact is a schema question, not a
            // free ride on a field that already exists.
            branch: self
                .git_probes
                .get(&target.session_id)
                .and_then(|observed| observed.branch.clone()),
            capabilities: descriptor
                .and_then(|descriptor| descriptor.capabilities)
                .unwrap_or_else(|| Capabilities::v1_for_runtime(&runtime_slug)),
            // Echoed only when the create claimed one; the struct then omits
            // the key entirely, so unclaimed sessions keep the exact 12-key
            // shape pre-amendment consumers require.
            session_ref: record.and_then(|record| record.session_ref.clone()),
        }
    }

    /// Queue a lifecycle receipt (44224). Receipts drain ahead of transcripts.
    pub fn enqueue_receipt(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        receipt: &LifecycleReceipt,
    ) -> anyhow::Result<()> {
        let content = serde_json::to_string(receipt)?;
        let event = build_coding_session_lifecycle_receipt(channel_id, command_id, &content)?
            .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            &coding_session_lifecycle_receipt_semantic_key(command_id),
            Priority::High,
            event,
        )?;
        Ok(())
    }

    /// Start a bounded re-observation of `session_id`'s working directory.
    ///
    /// The probe runs up to two short-lived `git` processes, each under
    /// [`git_probe`]'s own ceiling, so awaiting it here would stall *every other*
    /// session's transcript delivery and the outbox flush for as long as the
    /// slowest of them. It therefore runs on its own task and reports back as
    /// [`SessionEvent::WorktreeObserved`], which the loop folds in exactly like
    /// an actor report. The consequence is deliberate: metadata is published
    /// immediately without a branch and corrected a moment later, rather than
    /// published late but complete.
    ///
    /// A session whose record has vanished is a no-op, not an error. Two probes
    /// in flight for one session are allowed rather than serialized: both are
    /// read-only, so there is nothing to contend over. They are not, however,
    /// left to resolve by arrival order — each is stamped with a per-session
    /// generation minted here, and [`SessionEvent::WorktreeObserved`] fences
    /// out any result whose generation is not the newest one launched (see the
    /// invariant on that match arm), so a slow older probe can never overwrite
    /// a faster newer one.
    fn spawn_git_probe(&mut self, session_id: &str) {
        let Some(cwd) = self
            .state
            .session(session_id)
            .map(|record| record.cwd.clone())
        else {
            return;
        };
        let generation = {
            let next = self
                .git_probe_generation
                .entry(session_id.to_owned())
                .or_insert(0);
            *next += 1;
            *next
        };
        let events = self.session_events_tx.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let observed = git_probe::probe(&cwd).await;
            // A closed receiver means the provider loop is gone — shutdown, or
            // a dropped `Provider` in a test. The observation has nowhere to be
            // published, so it is dropped rather than logged as a failure.
            let _ = events
                .send(SessionEvent::WorktreeObserved {
                    session_id,
                    generation,
                    observed,
                })
                .await;
        });
    }

    /// Queue per-generation metadata (44223) describing `status`.
    ///
    /// Metadata is the one provider-authored kind whose latest value is the
    /// whole truth, so a queued-but-unsent row is superseded rather than
    /// preserved. Publishing a status the provider has already left behind is
    /// not merely stale — two metadata events landing in the same second read to
    /// the consumer as conflicting claims about one generation, and it refuses
    /// to pick a winner. Identical content is skipped for the same reason.
    pub fn publish_metadata(
        &mut self,
        channel_id: Uuid,
        target: &CodingSessionTarget,
        status: SessionStatus,
    ) -> anyhow::Result<()> {
        let metadata = self.metadata_for(target, status);
        let content = serde_json::to_string(&metadata)?;
        if self
            .last_metadata
            .get(&target.session_id)
            .is_some_and(|published| published.content == content)
        {
            return Ok(());
        }
        let event = build_coding_session_metadata(channel_id, target, &content)?
            .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue_latest(
            KIND_CODING_SESSION_METADATA,
            &coding_session_metadata_semantic_key(target),
            Priority::High,
            event,
        )?;
        self.last_metadata.insert(
            target.session_id.clone(),
            PublishedMetadata { content, status },
        );
        Ok(())
    }

    /// Reserve a sequence, wrap `item`, and queue it as a transcript item (44225).
    ///
    /// The sequence is persisted by [`StateStore::allocate_seq`] before the
    /// event is built, which is what makes a crash cost a gap rather than a
    /// duplicate.
    pub fn enqueue_transcript(
        &mut self,
        channel_id: Uuid,
        target: &CodingSessionTarget,
        turn_id: Option<&str>,
        item: serde_json::Value,
        priority: Priority,
    ) -> anyhow::Result<Option<u64>> {
        let Some(event_seq) = self.state.allocate_seq(&target.session_id)? else {
            return Ok(None);
        };
        let timestamp = now_ms();
        // Measure the envelope around an empty item, then shrink the item to
        // whatever is left. Doing it here rather than in the translator keeps the
        // cap honest: it is the *signed event* that must fit 32 KiB, and only
        // this layer knows what the envelope costs.
        let overhead = serde_json::to_string(&TranscriptEnvelope::new(
            target,
            event_seq,
            timestamp,
            turn_id,
            serde_json::Value::Null,
        ))?
        .len();
        let item = transcript::fit_item(item, overhead, MAX_TRANSCRIPT_CONTENT_BYTES);
        let envelope = TranscriptEnvelope::new(target, event_seq, timestamp, turn_id, item);
        let content = serde_json::to_string(&envelope)?;
        let event = build_coding_session_transcript_item(channel_id, target, event_seq, &content)?
            .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(
            KIND_CODING_SESSION_TRANSCRIPT,
            &coding_session_transcript_semantic_key(target, event_seq),
            priority,
            event,
        )?;
        Ok(Some(event_seq))
    }

    /// Turn one report from the session inbox into durable state and queued
    /// events.
    ///
    /// Synchronous on purpose, and the loop depends on it: every arm is
    /// bookkeeping plus an enqueue, so no session's report can be delayed by
    /// another's. Work that has to wait on the world — the worktree probe — is
    /// spawned and comes back through this same inbox as
    /// [`SessionEvent::WorktreeObserved`].
    pub(crate) fn handle_session_event(&mut self, event: SessionEvent) -> anyhow::Result<()> {
        match event {
            SessionEvent::TurnStarted {
                session_id,
                turn_id,
                command_id,
                text: _,
            } => {
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                self.state.update_session(&session_id, |record| {
                    record.open_turn = Some(OpenTurn {
                        turn_id: turn_id.clone(),
                        command_id: Some(command_id.clone()),
                        started_at_ms: now_ms(),
                    });
                })?;
                self.publish_metadata(channel_id, &target, SessionStatus::Running)?;
            }
            SessionEvent::TurnFinished {
                session_id,
                turn_id,
                outcome,
                duration_ms,
                usage,
            } => {
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                let (item, status) = turn_result(&outcome, duration_ms, usage.as_deref());
                self.enqueue_transcript(channel_id, &target, Some(&turn_id), item, Priority::High)?;
                self.state.update_session(&session_id, |record| {
                    record.open_turn = None;
                })?;
                let stopped = self
                    .state
                    .session(&session_id)
                    .is_some_and(|record| record.closed);
                // A finished turn is the moment the branch can have changed —
                // the agent may have checked out or created one. The probe is
                // started rather than awaited, so the turn's metadata reports
                // the status now and the branch a moment later.
                self.spawn_git_probe(&session_id);
                self.publish_metadata(
                    channel_id,
                    &target,
                    if stopped {
                        SessionStatus::Stopped
                    } else {
                        status
                    },
                )?;
            }
            SessionEvent::TranscriptItems {
                session_id,
                turn_id,
                items,
            } => {
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                for item in items {
                    self.enqueue_transcript(
                        channel_id,
                        &target,
                        Some(&turn_id),
                        item,
                        Priority::Normal,
                    )?;
                }
            }
            SessionEvent::TurnDropped {
                session_id,
                command_id,
            } => {
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                tracing::warn!(
                    target: "csp",
                    %session_id,
                    %command_id,
                    "turn dropped: the session's queue is full"
                );
                self.enqueue_transcript(
                    channel_id,
                    &target,
                    None,
                    payload::status_item("turn_dropped:queue_full"),
                    Priority::High,
                )?;
            }
            SessionEvent::Exited { session_id, reason } => {
                self.sessions.forget(&session_id);
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                tracing::info!(target: "csp", %session_id, "session ended: {reason:?}");
                let stopped = self
                    .state
                    .session(&session_id)
                    .is_some_and(|record| record.closed);
                self.state.update_session(&session_id, |record| {
                    record.open_turn = None;
                })?;
                self.publish_metadata(
                    channel_id,
                    &target,
                    if stopped {
                        SessionStatus::Stopped
                    } else {
                        SessionStatus::Disconnected
                    },
                )?;
            }
            SessionEvent::WorktreeObserved {
                session_id,
                generation,
                observed,
            } => {
                // Invariant: a result is applied only when its generation is
                // the newest one launched for this session id — never merely
                // the newest one to *arrive*. `git_probe_generation` is bumped
                // synchronously at launch (see `spawn_git_probe`), so it can
                // only be greater than an in-flight probe's stamped generation
                // once a newer probe has actually been launched; a probe's own
                // generation can never exceed it. Discarding anything older
                // than that ceiling — rather than trusting whichever result
                // shows up last — is what makes a slow, superseded probe unable
                // to overwrite a faster, newer one.
                let latest_launched = self
                    .git_probe_generation
                    .get(&session_id)
                    .copied()
                    .unwrap_or(0);
                if generation < latest_launched {
                    tracing::debug!(
                        target: "csp::git",
                        %session_id,
                        generation,
                        latest_launched,
                        "discarding a worktree observation superseded by a newer probe"
                    );
                    return Ok(());
                }
                // A session that was stopped, or whose record vanished, while
                // its probe was in flight keeps whatever its terminal metadata
                // already said. A filesystem read landing late is not a reason
                // to speak for a session that has finished speaking, so the
                // observation is dropped whole — not even cached.
                let ended = self
                    .state
                    .session(&session_id)
                    .is_none_or(|record| record.closed);
                if ended {
                    tracing::debug!(
                        target: "csp::git",
                        %session_id,
                        "discarding a worktree observation for an ended session"
                    );
                    return Ok(());
                }
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                tracing::debug!(
                    target: "csp::git",
                    %session_id,
                    branch = ?observed.branch,
                    dirty = ?observed.dirty,
                    "observed the session worktree"
                );
                self.git_probes.insert(session_id.clone(), observed);
                // Republished under the status the last publication claimed:
                // the observation corrects the branch, it says nothing about
                // the lifecycle. When the branch is unchanged the serialized
                // content is identical and `publish_metadata` drops it, so a
                // probe that learns nothing new costs no event.
                if let Some(status) = self
                    .last_metadata
                    .get(&session_id)
                    .map(|published| published.status)
                {
                    self.publish_metadata(channel_id, &target, status)?;
                }
            }
        }
        Ok(())
    }

    /// Resolve a session id to the channel it publishes into and its wire target.
    fn locate(&self, session_id: &str) -> Option<(Uuid, CodingSessionTarget)> {
        let record = self.state.session(session_id)?;
        Some((record.channel_id, self.target_for(record)))
    }

    /// The wire target for a persisted record, minted with the driver the
    /// record itself persisted at creation. The live descriptor table is
    /// deliberately not consulted: a descriptor that disappeared across a
    /// restart (adapter uninstalled) must not mutate the wire identity of
    /// events for a session that already published under the original driver —
    /// consumers match transcripts by the full target, driver included.
    fn target_for(&self, record: &SessionRecord) -> CodingSessionTarget {
        record.target(&self.config.instance_id)
    }

    /// Drain the outbox into `sink`.
    pub async fn flush<S: EventSink>(&mut self, sink: &S) -> anyhow::Result<usize> {
        Ok(self.outbox.flush(sink).await?)
    }

    /// Read-only access to durable state, for tests and diagnostics.
    pub fn state(&self) -> &StateStore {
        &self.state
    }

    /// Number of rows still awaiting publication.
    pub fn pending_publishes(&self) -> usize {
        self.outbox.pending_len()
    }
}

fn validate_genesis_envelope(
    event: &Event,
    channel_id: Uuid,
    session_ref: &str,
) -> Result<(), String> {
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    let channel = channel_id.to_string();
    if tags.len() != 3 || tags.iter().any(|tag| tag.len() != 2) {
        return Err("referenced genesis must carry exactly three two-field tags".into());
    }
    if tags[0][0] != "h" || tags[0][1] != channel {
        return Err("referenced genesis is not scoped to the create channel".into());
    }
    if tags[1][0] != "csg-v" || tags[1][1] != CODING_SESSION_GENESIS_TAG_VERSION {
        return Err("referenced genesis has an unsupported csg-v envelope".into());
    }
    if tags[2][0] != "csg-session" || tags[2][1] != session_ref {
        return Err("referenced genesis csg-session does not match the create sessionRef".into());
    }
    Ok(())
}

/// Resolve the exact genesis named by a create, with a short visibility retry.
///
/// Selection is always by event id. The payload and channel are checked only
/// after that exact event has been found; neither is ever used as a lookup.
async fn resolve_genesis_founder(
    relay: &HarnessRelay,
    channel_id: Uuid,
    session_ref: Option<&str>,
    genesis_ref: &str,
) -> Result<String, String> {
    let session_ref = session_ref.ok_or_else(|| "genesisRef requires sessionRef".to_owned())?;
    let mut last_error = None;
    for attempt in 0..GENESIS_QUERY_ATTEMPTS {
        match relay
            .query_event_by_id(
                genesis_ref,
                Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
            )
            .await
        {
            Ok(Some(event)) => {
                let payload = decode_coding_session_genesis(&event.content)
                    .map_err(|error| format!("referenced genesis payload is invalid: {error}"))?;
                if payload.session_ref != session_ref {
                    return Err("referenced genesis names a different sessionRef".into());
                }
                validate_genesis_envelope(&event, channel_id, session_ref)?;
                return Ok(event.pubkey.to_hex());
            }
            Ok(None) => {
                last_error = Some("referenced genesis is not query-visible".to_owned());
            }
            Err(error) => {
                last_error = Some(format!("genesis lookup failed: {error}"));
            }
        }
        if attempt + 1 < GENESIS_QUERY_ATTEMPTS {
            tokio::time::sleep(GENESIS_QUERY_RETRY_DELAY).await;
        }
    }
    Err(last_error.unwrap_or_else(|| "referenced genesis was not found".into()))
}

/// Map a turn outcome onto its terminal transcript item, the generation's next
/// status, and whether the session can serve another turn.
fn turn_result(
    outcome: &TurnOutcome,
    duration_ms: u64,
    usage: Option<&TurnUsage>,
) -> (serde_json::Value, SessionStatus) {
    let cost = usage.map(turn_cost).unwrap_or_default();
    match outcome {
        TurnOutcome::Completed { stop_reason } => {
            // `refusal` and the two limit stops are real answers, not provider
            // failures: the agent decided the turn was over. They are surfaced
            // as errors so the operator sees *why* the turn stopped short
            // instead of a silent success with no output.
            let is_error = !matches!(stop_reason, buzz_acp::acp::StopReason::EndTurn);
            let subtype = if is_error {
                payload::ResultSubtype::Error
            } else {
                payload::ResultSubtype::Success
            };
            // The generation is `idle` either way: a refusal or a token limit
            // ends the turn, not the session, and the operator's next prompt is
            // perfectly serviceable.
            (
                payload::result_item(subtype, duration_ms, stop_reason_text(stop_reason), cost),
                SessionStatus::Idle,
            )
        }
        TurnOutcome::Cancelled => (
            payload::result_item(
                payload::ResultSubtype::Cancelled,
                duration_ms,
                "interrupted by the operator",
                cost,
            ),
            SessionStatus::Interrupted,
        ),
        TurnOutcome::Failed {
            message,
            agent_gone,
        } => (
            payload::result_item(payload::ResultSubtype::Error, duration_ms, message, cost),
            if *agent_gone {
                SessionStatus::Disconnected
            } else {
                SessionStatus::Failed
            },
        ),
    }
}

fn stop_reason_text(stop_reason: &buzz_acp::acp::StopReason) -> &'static str {
    use buzz_acp::acp::StopReason;
    match stop_reason {
        StopReason::EndTurn => "completed",
        StopReason::Cancelled => "cancelled",
        StopReason::MaxTokens => "stopped: token limit reached",
        StopReason::MaxTurnRequests => "stopped: request limit reached",
        StopReason::Refusal => "stopped: the agent refused the prompt",
    }
}

fn turn_cost(usage: &TurnUsage) -> payload::TurnCost {
    payload::TurnCost {
        cost_usd: usage.turn_cost_usd,
        input_tokens: usage.turn_input_tokens,
        output_tokens: usage.turn_output_tokens,
        total_tokens: usage.turn_total_tokens,
    }
}

fn log_ignored(what: &str, reason: &Ignored) {
    match reason {
        Ignored::Malformed(detail) => {
            tracing::warn!(target: "csp", "ignoring malformed {what} command: {detail}")
        }
        other => tracing::debug!(target: "csp", "ignoring {what} command: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade};
    use axum::extract::State;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use nostr::Keys;

    use crate::session::testing::{fake_agent, GOOD_AGENT, RESUMABLE_AGENT, STALLING_AGENT};

    struct CollectingSink {
        events: Mutex<Vec<Event>>,
    }

    impl EventSink for CollectingSink {
        async fn publish(&self, event: Event) -> Result<(), String> {
            self.events.lock().expect("lock").push(event);
            Ok(())
        }
    }

    impl CollectingSink {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn contents_of(&self, kind: u32) -> Vec<serde_json::Value> {
            self.events
                .lock()
                .expect("lock")
                .iter()
                .filter(|event| u32::from(event.kind.as_u16()) == kind)
                .map(|event| serde_json::from_str(&event.content).expect("json content"))
                .collect()
        }

        fn all(&self) -> Vec<Event> {
            self.events.lock().expect("lock").clone()
        }
    }

    use buzz_core::coding_session_runtime::RuntimeDescriptor;

    #[derive(Clone)]
    struct TestRelayState {
        event: Option<Event>,
        queries: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    async fn test_relay_ws(ws: WebSocketUpgrade) -> impl axum::response::IntoResponse {
        ws.on_upgrade(|socket| async move { serve_test_relay_socket(socket).await })
    }

    async fn serve_test_relay_socket(mut socket: WebSocket) {
        socket
            .send(AxumWsMessage::Text(
                serde_json::json!(["AUTH", "genesis-unit-test"])
                    .to_string()
                    .into(),
            ))
            .await
            .expect("send auth challenge");
        let auth = socket
            .recv()
            .await
            .expect("auth response")
            .expect("valid websocket message");
        let AxumWsMessage::Text(auth) = auth else {
            panic!("expected text AUTH response");
        };
        let auth: serde_json::Value = serde_json::from_str(auth.as_str()).expect("AUTH json");
        let event_id = auth
            .pointer("/1/id")
            .and_then(serde_json::Value::as_str)
            .expect("AUTH event id");
        socket
            .send(AxumWsMessage::Text(
                serde_json::json!(["OK", event_id, true, "authenticated"])
                    .to_string()
                    .into(),
            ))
            .await
            .expect("send auth OK");
        while socket.recv().await.is_some() {}
    }

    async fn test_relay_query(
        State(state): State<TestRelayState>,
        Json(query): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        state.queries.lock().expect("queries lock").push(query);
        Json(match &state.event {
            Some(event) => serde_json::json!([event]),
            None => serde_json::json!([]),
        })
    }

    async fn spawn_test_relay(
        keys: &Keys,
        event: Option<Event>,
    ) -> (
        HarnessRelay,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let queries = Arc::new(Mutex::new(Vec::new()));
        let state = TestRelayState {
            event,
            queries: queries.clone(),
        };
        let app = Router::new()
            .route("/", get(test_relay_ws))
            .route("/query", post(test_relay_query))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test relay");
        let address = listener.local_addr().expect("test relay address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test relay");
        });
        let relay_url = format!("ws://{address}");
        let relay = HarnessRelay::connect(&relay_url, keys, &keys.public_key().to_hex(), None)
            .await
            .expect("connect test relay");
        (relay, queries, server)
    }

    fn test_operator_keys() -> &'static Keys {
        static KEYS: OnceLock<Keys> = OnceLock::new();
        KEYS.get_or_init(Keys::generate)
    }

    fn claude_runtime(agent_command: String) -> RuntimeDescriptor {
        RuntimeDescriptor {
            instance_ref: "claude-primary".into(),
            driver: "claude-agent-acp".into(),
            runtime: "claude".into(),
            agent_command,
            agent_args: Vec::new(),
            cli_env: None,
            default_model: "claude-sonnet-4-6".into(),
            allowed_models: vec!["claude-sonnet-4-6".into()],
            discover_models: false,
            capabilities: None,
        }
    }

    fn config_of(
        keys: Keys,
        state_dir: &Path,
        projects: Option<&Path>,
        agent_command: String,
    ) -> Config {
        config_of_runtimes(
            keys,
            state_dir,
            projects,
            vec![claude_runtime(agent_command)],
        )
    }

    fn config_of_runtimes(
        keys: Keys,
        state_dir: &Path,
        projects: Option<&Path>,
        runtimes: Vec<RuntimeDescriptor>,
    ) -> Config {
        Config {
            keys,
            relay_url: "ws://localhost:3000".into(),
            auth_tag: None,
            state_dir: state_dir.to_path_buf(),
            projects_file: projects.map(Path::to_path_buf),
            instance_id: "instance-1".into(),
            runtimes,
            max_sessions: 2,
            session_idle_shutdown: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(900),
            max_turn_duration: Duration::from_secs(7200),
            include_thoughts: true,
            command_horizon: Duration::from_secs(86_400),
        }
    }

    /// A provider wired to a cooperative scripted agent.
    fn provider(state_dir: &Path, projects: Option<&Path>) -> Provider {
        let agent = fake_agent(state_dir_parent(state_dir), "good-agent", GOOD_AGENT);
        Provider::new(config_of(Keys::generate(), state_dir, projects, agent)).expect("provider")
    }

    fn state_dir_parent(state_dir: &Path) -> &Path {
        state_dir.parent().unwrap_or(state_dir)
    }

    /// Transcript items as the *record* orders them — by sequence — which is
    /// what a consumer sorts on. Publish order differs deliberately: terminal
    /// items are high priority and drain first.
    fn transcript_items_in_sequence(sink: &CollectingSink) -> Vec<serde_json::Value> {
        let mut items = sink.contents_of(KIND_CODING_SESSION_TRANSCRIPT);
        items.sort_by_key(|item| item["eventSeq"].as_u64().unwrap_or(0));
        items
    }

    /// Record every session report that is already available.
    async fn pump_available(provider: &mut Provider) {
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(250), provider.next_session_event()).await
        {
            provider.handle_session_event(event).expect("record");
        }
    }

    /// Drain and record session reports until a turn has finished.
    async fn pump_until_turn_finished(provider: &mut Provider) {
        pump_until(provider, |event| {
            matches!(event, session::SessionEvent::TurnFinished { .. })
        })
        .await;
    }

    /// Drain and record session reports until a worktree observation lands.
    ///
    /// The probe no longer runs on the caller's stack, so a test that asserts on
    /// a published branch has to let its result come back through the same inbox
    /// the provider loop drains.
    async fn pump_until_worktree_observed(provider: &mut Provider) {
        pump_until(provider, |event| {
            matches!(event, session::SessionEvent::WorktreeObserved { .. })
        })
        .await;
    }

    /// Drain and record session reports until `session_id`'s *applied*
    /// worktree state shows `branch`.
    ///
    /// Stronger than [`pump_until_worktree_observed`] when more than one probe
    /// can be in flight: it checks `provider.git_probes` after every event is
    /// applied, rather than sniffing the raw event's content, because R17
    /// fencing means a stale probe can still land in the channel carrying the
    /// right-looking branch (it read the worktree late, after a newer probe
    /// had already been launched) without ever winning the fence. Only
    /// applied state proves the observation under test is the one that stuck.
    async fn pump_until_branch_observed(provider: &mut Provider, session_id: &str, branch: &str) {
        loop {
            let event =
                tokio::time::timeout(Duration::from_secs(20), provider.next_session_event())
                    .await
                    .expect("session event within timeout")
                    .expect("channel open");
            provider.handle_session_event(event).expect("record");
            if provider
                .git_probes
                .get(session_id)
                .and_then(|observed| observed.branch.as_deref())
                == Some(branch)
            {
                return;
            }
        }
    }

    /// Drain and record session reports until one satisfies `done`.
    async fn pump_until(provider: &mut Provider, done: impl Fn(&SessionEvent) -> bool) {
        loop {
            let event =
                tokio::time::timeout(Duration::from_secs(20), provider.next_session_event())
                    .await
                    .expect("session event within timeout")
                    .expect("channel open");
            let finished = done(&event);
            provider.handle_session_event(event).expect("record");
            if finished {
                return;
            }
        }
    }

    fn write_projects(dir: &Path, channel_id: Uuid, cwd: &Path) -> std::path::PathBuf {
        let path = dir.join("projects.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "version": 1,
                "channels": { channel_id.to_string(): cwd },
            })
            .to_string(),
        )
        .expect("write projects");
        path
    }

    fn create_event(provider: &Provider, channel_id: Uuid, command_id: &str) -> Event {
        create_event_inner(
            provider,
            channel_id,
            command_id,
            "claude-primary",
            serde_json::Value::Null,
        )
    }

    fn create_event_for_ref(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        instance_ref: &str,
    ) -> Event {
        create_event_inner(
            provider,
            channel_id,
            command_id,
            instance_ref,
            serde_json::Value::Null,
        )
    }

    fn create_event_with_initial_turn(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        turn: &str,
    ) -> Event {
        create_event_inner(
            provider,
            channel_id,
            command_id,
            "claude-primary",
            serde_json::json!(turn),
        )
    }

    /// A create claiming an umbrella: the 9-key post-amendment action form.
    fn create_event_with_session_ref(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        session_ref: &str,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "sessionRef": session_ref,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "model": null,
                "title": "Ship it",
                "initialTurn": null,
            },
        })
        .to_string();
        signed_lifecycle_event(channel_id, content)
    }

    fn create_event_with_genesis_ref(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        session_ref: &str,
        genesis_ref: &str,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "sessionRef": session_ref,
                "genesisRef": genesis_ref,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "model": null,
                "title": "Ship it",
                "initialTurn": null,
            },
        })
        .to_string();
        signed_lifecycle_event(channel_id, content)
    }

    fn genesis_event(channel_id: Uuid, session_ref: &str) -> Event {
        let payload =
            buzz_core::coding_session_genesis::CodingSessionGenesisPayload::new(session_ref);
        buzz_sdk::builders::build_coding_session_genesis(channel_id, &payload)
            .expect("genesis builder")
            .sign_with_keys(test_operator_keys())
            .expect("sign genesis")
    }

    fn genesis_event_with_tags(session_ref: &str, tags: Vec<nostr::Tag>) -> Event {
        let payload =
            buzz_core::coding_session_genesis::CodingSessionGenesisPayload::new(session_ref);
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
            serde_json::to_string(&payload).expect("genesis content"),
        )
        .tags(tags)
        .sign_with_keys(test_operator_keys())
        .expect("sign genesis")
    }

    fn create_event_inner(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        instance_ref: &str,
        initial_turn: serde_json::Value,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": instance_ref,
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "model": null,
                "title": "Ship it",
                "initialTurn": initial_turn,
            },
        })
        .to_string();
        signed_lifecycle_event(channel_id, content)
    }

    fn signed_lifecycle_event(channel_id: Uuid, content: String) -> Event {
        signed_lifecycle_event_by(channel_id, content, test_operator_keys())
    }

    fn signed_lifecycle_event_by(channel_id: Uuid, content: String, keys: &Keys) -> Event {
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16),
            content,
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(keys)
        .expect("sign")
    }

    fn turn_event(channel_id: Uuid, command_id: &str, target: &CodingSessionTarget) -> Event {
        command_event(
            channel_id,
            command_id,
            target,
            serde_json::json!({ "type": "thread.turn.start", "text": "do the thing" }),
        )
    }

    fn interrupt_event(channel_id: Uuid, command_id: &str, target: &CodingSessionTarget) -> Event {
        command_event(
            channel_id,
            command_id,
            target,
            serde_json::json!({ "type": "thread.turn.interrupt" }),
        )
    }

    fn lifecycle_target_event(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        action: &str,
        target: &CodingSessionTarget,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": action,
                "session": target,
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
            },
        })
        .to_string();
        signed_lifecycle_event(channel_id, content)
    }

    fn command_event(
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        action: serde_json::Value,
    ) -> Event {
        command_event_by(channel_id, command_id, target, action, test_operator_keys())
    }

    fn command_event_by(
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        action: serde_json::Value,
        keys: &Keys,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-command/v1",
            "commandId": command_id,
            "target": target,
            "action": action,
        })
        .to_string();
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_COMMAND as u16),
            content,
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(keys)
        .expect("sign")
    }

    #[tokio::test]
    async fn a_create_produces_a_receipt_and_metadata_and_a_live_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event = create_event(&provider, channel_id, "create-1");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "created");
        assert_eq!(receipts[0]["commandId"], "create-1");

        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0]["status"], "idle");
        assert_eq!(metadata[0]["title"], "Ship it");
        assert!(metadata[0]["projectRef"].is_null());
        assert_eq!(metadata[0]["capabilities"]["threadTurnInterrupt"], true);
        assert!(
            !metadata[0]
                .as_object()
                .expect("object")
                .contains_key("sessionRef"),
            "a create with no umbrella claim keeps the exact pre-amendment metadata shape"
        );

        assert_eq!(provider.state().sessions().count(), 1);
        assert_eq!(provider.sessions.live_count(), 1);
    }

    #[tokio::test]
    async fn unresolved_genesis_fails_closed_with_a_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let event = create_event_with_genesis_ref(
            &provider,
            channel_id,
            "create-genesis-missing",
            session_ref,
            &"12".repeat(32),
        );
        let (mut relay, queries, server) = spawn_test_relay(&provider.config.keys, None).await;

        provider
            .handle_relay_event(&mut relay, channel_id, &event)
            .await
            .expect("handle");
        assert_eq!(provider.state().sessions().count(), 0);
        assert!(provider
            .state()
            .is_command_consumed("create-genesis-missing"));

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(receipts[0]["error"]["code"], GENESIS_NOT_FOUND);
        assert_eq!(
            queries.lock().expect("queries lock").len(),
            GENESIS_QUERY_ATTEMPTS,
            "a not-yet-visible genesis is retried before the refusal receipt"
        );

        relay.shutdown().await;
        server.abort();
    }

    #[tokio::test]
    async fn referenced_genesis_resolves_by_id_and_create_emits_created_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let genesis = genesis_event(channel_id, session_ref);
        let create = create_event_with_genesis_ref(
            &provider,
            channel_id,
            "create-genesis-resolved",
            session_ref,
            &genesis.id.to_hex(),
        );
        let (mut relay, queries, server) =
            spawn_test_relay(&provider.config.keys, Some(genesis.clone())).await;

        provider
            .handle_relay_event(&mut relay, channel_id, &create)
            .await
            .expect("resolve genesis and create");

        let record = provider.state().sessions().next().expect("session record");
        assert_eq!(
            record.genesis_ref.as_deref(),
            Some(genesis.id.to_hex().as_str())
        );
        assert_eq!(
            record.founder_pubkey.as_deref(),
            Some(genesis.pubkey.to_hex().as_str())
        );
        {
            let captured = queries.lock().expect("queries lock");
            assert_eq!(captured.len(), 1);
            assert_eq!(captured[0][0]["ids"][0], genesis.id.to_hex());
            assert_eq!(captured[0][0]["kinds"][0], KIND_CODING_SESSION_GENESIS);
        }

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipt = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "create-genesis-resolved")
            .expect("created receipt");
        assert_eq!(receipt["status"], "created");
        assert!(receipt["session"].is_object());

        relay.shutdown().await;
        server.abort();
    }

    #[tokio::test]
    async fn resolved_genesis_with_wrong_envelope_fails_closed_with_receipts() {
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let channel_id = Uuid::new_v4();
        let channel = channel_id.to_string();
        let other_channel = Uuid::new_v4().to_string();
        let other_session_ref = "11111111-2222-3333-4444-555555555555";
        let tag = |fields: &[&str]| nostr::Tag::parse(fields.iter().copied()).expect("tag");
        let cases = [
            (
                "wrong-channel",
                vec![
                    tag(&["h", &other_channel]),
                    tag(&["csg-v", CODING_SESSION_GENESIS_TAG_VERSION]),
                    tag(&["csg-session", session_ref]),
                ],
            ),
            (
                "wrong-version",
                vec![
                    tag(&["h", &channel]),
                    tag(&["csg-v", "csg1-2"]),
                    tag(&["csg-session", session_ref]),
                ],
            ),
            (
                "wrong-session-tag",
                vec![
                    tag(&["h", &channel]),
                    tag(&["csg-v", CODING_SESSION_GENESIS_TAG_VERSION]),
                    tag(&["csg-session", other_session_ref]),
                ],
            ),
            (
                "extra-tag",
                vec![
                    tag(&["h", &channel]),
                    tag(&["csg-v", CODING_SESSION_GENESIS_TAG_VERSION]),
                    tag(&["csg-session", session_ref]),
                    tag(&["x", "smuggled"]),
                ],
            ),
        ];

        for (case, tags) in cases {
            let dir = tempfile::tempdir().expect("tempdir");
            let cwd = dir.path().join("checkout");
            std::fs::create_dir_all(&cwd).expect("mkdir");
            let projects = write_projects(dir.path(), channel_id, &cwd);
            let mut provider = provider(&dir.path().join("state"), Some(&projects));
            let genesis = genesis_event_with_tags(session_ref, tags);
            let command_id = format!("create-{case}");
            let create = create_event_with_genesis_ref(
                &provider,
                channel_id,
                &command_id,
                session_ref,
                &genesis.id.to_hex(),
            );
            let (mut relay, _queries, server) =
                spawn_test_relay(&provider.config.keys, Some(genesis)).await;

            provider
                .handle_relay_event(&mut relay, channel_id, &create)
                .await
                .expect("refuse invalid envelope");
            assert_eq!(provider.state().sessions().count(), 0, "case {case}");
            let sink = CollectingSink::new();
            provider.flush(&sink).await.expect("flush");
            let receipt = sink
                .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
                .into_iter()
                .find(|receipt| receipt["commandId"] == command_id)
                .expect("failure receipt");
            assert_eq!(receipt["status"], "failed", "case {case}");
            assert_eq!(receipt["error"]["code"], GENESIS_NOT_FOUND, "case {case}");

            relay.shutdown().await;
            server.abort();
        }
    }

    #[tokio::test]
    async fn non_founder_turn_stop_and_resume_each_publish_unauthorized_receipts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let create = create_event(&provider, channel_id, "create-authorized");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let record = provider.state().sessions().next().expect("session");
        assert_eq!(
            record.founder_pubkey.as_deref(),
            Some(create.pubkey.to_hex().as_str()),
            "legacy authority is the locally witnessed create signer"
        );
        let target = record.target(&provider.config.instance_id);
        let stranger = Keys::generate();

        let turn = command_event_by(
            channel_id,
            "turn-unauthorized",
            &target,
            serde_json::json!({ "type": "thread.turn.start", "text": "no" }),
            &stranger,
        );
        let stop_content = lifecycle_target_event(
            &provider,
            channel_id,
            "stop-unauthorized",
            "session.stop",
            &target,
        )
        .content;
        let stop = signed_lifecycle_event_by(channel_id, stop_content, &stranger);
        let resume_content = lifecycle_target_event(
            &provider,
            channel_id,
            "resume-unauthorized",
            "session.resume",
            &target,
        )
        .content;
        let resume = signed_lifecycle_event_by(channel_id, resume_content, &stranger);

        for event in [&turn, &stop, &resume] {
            provider
                .handle_command_event(channel_id, event)
                .await
                .expect("refuse");
        }
        assert!(
            !provider
                .state()
                .session(&target.session_id)
                .expect("session")
                .closed
        );
        assert_eq!(
            provider
                .state()
                .session(&target.session_id)
                .expect("session")
                .generation,
            1
        );

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        for command_id in [
            "turn-unauthorized",
            "stop-unauthorized",
            "resume-unauthorized",
        ] {
            let receipt = receipts
                .iter()
                .find(|receipt| receipt["commandId"] == command_id)
                .expect("unauthorized receipt");
            assert_eq!(receipt["status"], "failed");
            assert_eq!(receipt["error"]["code"], UNAUTHORIZED_OPERATOR);
            assert!(provider.state().is_command_consumed(command_id));
        }
    }

    /// Initialize `cwd` as a repository on `branch` with one commit.
    fn init_repo(cwd: &Path, branch: &str) {
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q", "-b", branch, "."]);
        std::fs::write(cwd.join("README.md"), "hello").expect("write");
        run(&["add", "README.md"]);
        run(&[
            "-c",
            "user.email=probe@example.invalid",
            "-c",
            "user.name=probe",
            "commit",
            "-q",
            "--no-gpg-sign",
            "-m",
            "one",
        ]);
    }

    /// A cwd that is not a repository publishes no branch, and — the part that
    /// matters — publishes it as *absent knowledge* rather than as an error the
    /// operator has to read.
    #[tokio::test]
    async fn a_non_repository_cwd_publishes_a_null_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.len(), 1);
        assert!(metadata[0]["branch"].is_null());
        // The receipt is unconditionally a success: a plain directory is a
        // legitimate place to run an agent, not a failed create.
        assert_eq!(
            sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)[0]["status"],
            "created"
        );
    }

    /// The probe is observable end to end: a create in a real checkout carries
    /// the branch git reported into signed metadata.
    #[tokio::test]
    async fn a_create_in_a_checkout_publishes_its_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        init_repo(&cwd, "probe-branch");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        // The probe runs off the loop, so its result has to come back through
        // the session inbox before metadata can name the branch. Nothing has
        // been flushed yet, so the corrected publication supersedes the queued
        // branchless one and the consumer still sees exactly one metadata event.
        pump_until_worktree_observed(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0]["branch"], "probe-branch");

        // Dirty state is observed but deliberately *not* published: carrying it
        // as a signed fact is a schema question, and smuggling it into `branch`
        // would answer that question by accident.
        let session_id = &provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();
        assert_eq!(
            provider.git_probes.get(session_id).and_then(|p| p.dirty),
            Some(false)
        );
        for event in sink.all() {
            let serialized = serde_json::to_string(&event).expect("serialize");
            assert!(!serialized.contains("dirty"), "dirty leaked: {serialized}");
        }
    }

    /// A finished turn re-observes the worktree, so a branch the agent switched
    /// to mid-session is what the next metadata publication reports.
    #[tokio::test]
    async fn a_finished_turn_republishes_a_changed_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        init_repo(&cwd, "before");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");

        // Stand in for an agent that checked out a branch during its turn.
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&cwd)
            .args(["checkout", "-q", "-b", "after"])
            .status()
            .expect("git");
        assert!(status.success());

        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-1", &target))
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;
        // The finished turn started a probe rather than awaiting one, so the
        // re-observation arrives as its own report. The create's own probe
        // can still be in flight too — and, on a fast checkout, can even read
        // the post-checkout worktree late and report "after" itself, despite
        // being the older, superseded generation. Waiting on *applied* state
        // for this branch (not merely an event carrying it) is what makes the
        // assertion below test the fence rather than get lucky past it.
        pump_until_branch_observed(&mut provider, &target.session_id, "after").await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(
            metadata.last().expect("metadata")["branch"],
            "after",
            "the turn-finished probe did not re-observe the worktree"
        );
    }

    /// The whole point of probing off the loop: metadata is published *now*,
    /// without a branch, and corrected by a second event when git answers. A
    /// consumer that already read the first publication is not left with a
    /// session that never names its branch.
    #[tokio::test]
    async fn an_observation_landing_after_publication_corrects_the_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        init_repo(&cwd, "probe-branch");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");

        // Flushed before the observation is folded in: this is exactly what a
        // consumer sees while git is still running.
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let published = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(published.len(), 1);
        assert!(
            published[0]["branch"].is_null(),
            "the create waited on the probe: {}",
            published[0]
        );

        pump_until_worktree_observed(&mut provider).await;
        provider.flush(&sink).await.expect("flush");
        let published = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(
            published.len(),
            2,
            "the observation did not correct the publication"
        );
        assert_eq!(published[1]["branch"], "probe-branch");
        assert_eq!(
            published[1]["status"], published[0]["status"],
            "the correction invented a lifecycle change the probe never saw"
        );

        // Dedupe survives the round trip: an observation identical to the last
        // one costs no event, however many times it is repeated.
        let session_id = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();
        provider.spawn_git_probe(&session_id);
        pump_until_worktree_observed(&mut provider).await;
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            sink.contents_of(KIND_CODING_SESSION_METADATA).len(),
            2,
            "an unchanged observation published a duplicate"
        );
    }

    /// A session stopped while its probe was in flight keeps the last thing it
    /// said. A filesystem read landing late must not republish metadata for a
    /// generation the operator already retired.
    #[tokio::test]
    async fn an_observation_for_an_ended_session_is_discarded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        init_repo(&cwd, "probe-branch");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        // Stopped while the create's probe is still running.
        provider
            .handle_command_event(
                channel_id,
                &lifecycle_target_event(&provider, channel_id, "stop-1", "session.stop", &target),
            )
            .await
            .expect("handle");

        pump_until_worktree_observed(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let published = sink.contents_of(KIND_CODING_SESSION_METADATA);
        let last = published.last().expect("metadata");
        assert_eq!(last["status"], "stopped");
        assert!(
            last["branch"].is_null(),
            "a late observation spoke for a stopped session: {last}"
        );
        // Not merely unpublished — not cached either, so nothing can resurrect
        // it into a later publication.
        assert!(!provider.git_probes.contains_key(&target.session_id));
    }

    /// R17: a stale probe completing *after* a newer one must not win.
    ///
    /// Deterministic by construction — no sleeps, no racing real `git`
    /// subprocesses. The launch side is simulated by hand-setting
    /// `git_probe_generation` to `2` (as if generation 1, then generation 2,
    /// had both been launched); the completion side is simulated by feeding
    /// `handle_session_event` two synthetic `WorktreeObserved` events directly,
    /// generation 2 ("B", the newer launch) first and generation 1 ("A", the
    /// older launch) second — the exact reversed-completion order the ruling
    /// requires. If fencing were arrival-order-based rather than
    /// generation-based, A's later arrival would overwrite B's result.
    #[tokio::test]
    async fn a_stale_generation_probe_completing_after_a_newer_one_does_not_win() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        init_repo(&cwd, "initial-branch");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        // Drain the create's own probe (generation 1) so it cannot interleave
        // with the synthetic events below.
        pump_until_worktree_observed(&mut provider).await;

        let session_id = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();

        // Simulate two more probes having been launched for this session:
        // generation 2 ("A") launched first, generation 3 ("B") launched
        // second. Setting the counter directly (rather than calling
        // `spawn_git_probe` twice and racing real subprocesses) is what makes
        // the completion order below controllable by hand.
        provider.git_probe_generation.insert(session_id.clone(), 3);

        let older = git_probe::GitProbe {
            branch: Some("branch-a".to_owned()),
            dirty: Some(false),
        };
        let newer = git_probe::GitProbe {
            branch: Some("branch-b".to_owned()),
            dirty: Some(true),
        };

        // B (generation 3, launched second) completes first.
        provider
            .handle_session_event(SessionEvent::WorktreeObserved {
                session_id: session_id.clone(),
                generation: 3,
                observed: newer.clone(),
            })
            .expect("record newer");
        // A (generation 2, launched first) completes after — the reversed
        // completion order this test exists to force.
        provider
            .handle_session_event(SessionEvent::WorktreeObserved {
                session_id: session_id.clone(),
                generation: 2,
                observed: older,
            })
            .expect("record older");

        assert_eq!(
            provider.git_probes.get(&session_id),
            Some(&newer),
            "the older, later-arriving probe overwrote the newer one"
        );
    }

    /// A create claiming an umbrella round-trips its `sessionRef` into the
    /// published metadata — the projection the catalog groups by — while the
    /// receipt contract stays byte-identical to an unclaimed create's.
    #[tokio::test]
    async fn a_create_with_a_session_ref_echoes_it_into_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let event = create_event_with_session_ref(&provider, channel_id, "create-1", umbrella);
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "created");

        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0]["sessionRef"], umbrella);

        // The claim is durable: it lives on the session record, so every later
        // metadata publication for this session carries it too.
        let record = provider
            .state()
            .sessions()
            .next()
            .expect("one session record");
        assert_eq!(record.session_ref.as_deref(), Some(umbrella));
    }

    /// Multi-runtime routing: a create naming a second runtime's ref must spawn
    /// *that* runtime's adapter and mint its driver into the target.
    #[tokio::test]
    async fn a_create_routes_to_the_named_runtimes_adapter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);

        // Two distinct fake adapters, so the spawn choice is observable: only
        // the "codex" one exists — spawning the claude one would fail.
        let codex_agent = fake_agent(dir.path(), "codex-agent", GOOD_AGENT);
        let runtimes = vec![
            claude_runtime(dir.path().join("missing-claude").to_string_lossy().into()),
            RuntimeDescriptor {
                instance_ref: "codex-primary".into(),
                driver: "codex-acp".into(),
                runtime: "codex".into(),
                agent_command: codex_agent,
                agent_args: Vec::new(),
                cli_env: None,
                default_model: "default".into(),
                allowed_models: vec!["default".into()],
                discover_models: false,
                capabilities: None,
            },
        ];
        let mut provider = Provider::new(config_of_runtimes(
            Keys::generate(),
            &dir.path().join("state"),
            Some(&projects),
            runtimes,
        ))
        .expect("provider");

        let event = create_event_for_ref(&provider, channel_id, "create-1", "codex-primary");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts[0]["status"], "created");
        assert_eq!(receipts[0]["session"]["driver"], "codex-acp");

        let record = provider.state().sessions().next().expect("session");
        assert_eq!(record.provider_instance_ref, "codex-primary");
        assert_eq!(record.runtime, "codex");

        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata[0]["provider"], "codex-primary");
        assert_eq!(metadata[0]["runtime"], "codex");
        // Codex gets the conservative baseline vector: no plan claim yet.
        assert_eq!(metadata[0]["capabilities"]["plan"], false);
        assert_eq!(metadata[0]["capabilities"]["threadTurnInterrupt"], true);
    }

    /// A create naming a ref this signer does not offer is *ours* — no other
    /// process will answer it — so it must fail loudly rather than strand the
    /// consumer's durable create in silence.
    #[tokio::test]
    async fn an_unknown_provider_instance_ref_fails_the_create() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event = create_event_for_ref(&provider, channel_id, "create-1", "ghost-primary");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(receipts[0]["error"]["code"], "PROVIDER_UNAVAILABLE");
        let message = receipts[0]["error"]["message"].as_str().expect("message");
        assert!(message.contains("ghost-primary"));
        assert!(message.contains("claude-primary"));
        assert_eq!(provider.state().sessions().count(), 0);
        assert!(provider.state().is_command_consumed("create-1"));
    }

    /// The invariant the whole workdir seam exists to protect.
    #[tokio::test]
    async fn no_published_event_ever_carries_the_host_working_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("secret-checkout-path");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        provider
            .enqueue_transcript(
                channel_id,
                &target,
                Some("turn-1"),
                serde_json::json!({ "kind": "assistant_text", "text": "done" }),
                Priority::Normal,
            )
            .expect("transcript");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let needle = cwd.to_string_lossy().to_string();
        assert!(!needle.is_empty());
        for event in sink.all() {
            let serialized = serde_json::to_string(&event).expect("serialize event");
            assert!(
                !serialized.contains(&needle) && !serialized.contains("secret-checkout-path"),
                "a host path leaked into a signed event: {serialized}"
            );
        }
        // …while local state still knows exactly where the session runs.
        assert_eq!(
            provider.state().sessions().next().expect("session").cwd,
            cwd
        );
    }

    #[tokio::test]
    async fn replaying_the_same_create_has_no_second_effect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let event = create_event(&provider, channel_id, "create-1");

        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("replay");

        assert_eq!(provider.state().sessions().count(), 1);
        assert_eq!(provider.pending_publishes(), 2);
    }

    /// Restart mid-flight: the durable ledger, not memory, is what makes the
    /// replayed subscription a no-op.
    #[tokio::test]
    async fn a_restart_replaying_the_same_create_is_still_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);

        // The same signing identity across the restart — a rotated key would
        // change the addressing and make this test prove nothing.
        let keys = Keys::generate();
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let mut first = Provider::new(config_of(
            keys.clone(),
            &state_dir,
            Some(&projects),
            agent.clone(),
        ))
        .expect("provider");
        let event = create_event(&first, channel_id, "create-1");
        first
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        let sink = CollectingSink::new();
        first.flush(&sink).await.expect("flush");
        drop(first);

        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        restarted.recover().expect("recover");
        // Recovery retires the generation the dead process owned; that one
        // `disconnected` metadata is the only thing it may publish.
        let after_recovery = restarted.pending_publishes();
        assert_eq!(after_recovery, 1);

        restarted
            .handle_command_event(channel_id, &event)
            .await
            .expect("replay");
        assert_eq!(restarted.state().sessions().count(), 1);
        assert_eq!(
            restarted.pending_publishes(),
            after_recovery,
            "the replay must not re-enqueue a receipt or a second session"
        );
    }

    /// A turn caught mid-flight by a crash has no terminal item, so a consumer
    /// would render it running forever. Recovery has to close it out.
    #[tokio::test]
    async fn recovery_synthesizes_the_terminal_item_a_dead_process_never_published() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let keys = Keys::generate();
        // A stalling agent means the turn is genuinely still open when the
        // provider goes away, which is exactly the state being repaired.
        let agent = fake_agent(dir.path(), "stalling-agent", STALLING_AGENT);

        let session_id = {
            let mut first = Provider::new(config_of(
                keys.clone(),
                &state_dir,
                Some(&projects),
                agent.clone(),
            ))
            .expect("provider");
            let create = create_event_with_initial_turn(&first, channel_id, "create-1", "go");
            first
                .handle_command_event(channel_id, &create)
                .await
                .expect("handle");
            let started = tokio::time::timeout(Duration::from_secs(10), first.next_session_event())
                .await
                .expect("turn start")
                .expect("event");
            first.handle_session_event(started).expect("record");
            // The turn is genuinely open: the agent will never answer it. Record
            // its opening items so the synthesized result continues the
            // sequence rather than starting it.
            pump_available(&mut first).await;
            first.flush(&CollectingSink::new()).await.expect("flush");
            let session_id = first
                .state()
                .sessions()
                .next()
                .expect("session")
                .session_id
                .clone();
            assert!(first
                .state()
                .session(&session_id)
                .expect("session")
                .open_turn
                .is_some());
            session_id
        };

        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        restarted.recover().expect("recover");
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");

        let transcripts = transcript_items_in_sequence(&sink);
        assert_eq!(transcripts.len(), 1);
        assert_eq!(transcripts[0]["item"]["kind"], "result");
        assert_eq!(transcripts[0]["item"]["subtype"], "error");
        assert_eq!(transcripts[0]["item"]["isError"], true);
        assert_eq!(
            transcripts[0]["item"]["result"],
            "provider terminated mid-turn"
        );
        assert!(transcripts[0]["turnId"].is_string());
        assert!(
            transcripts[0]["eventSeq"].as_u64().expect("seq") > 1,
            "the synthesized item continues the turn's sequence rather than restarting it"
        );

        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.last().expect("metadata")["status"], "disconnected");

        let record = restarted.state().session(&session_id).expect("session");
        assert!(
            !record.closed,
            "process death detaches the generation but does not override durable stop intent"
        );
        assert!(record.open_turn.is_none());
    }

    #[tokio::test]
    async fn a_restart_can_resume_into_a_new_generation_and_stop_it_durably() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let keys = Keys::generate();
        let agent = fake_agent(dir.path(), "resumable-agent", RESUMABLE_AGENT);

        let session_id = {
            let mut first = Provider::new(config_of(
                keys.clone(),
                &state_dir,
                Some(&projects),
                agent.clone(),
            ))
            .expect("provider");
            let create = create_event(&first, channel_id, "create-resumable");
            first
                .handle_command_event(channel_id, &create)
                .await
                .expect("create");
            let record = first.state().sessions().next().expect("session");
            assert_eq!(record.resume_cursor.as_deref(), Some("saved-acp-session"));
            record.session_id.clone()
        };

        let mut restarted = Provider::new(config_of(
            keys.clone(),
            &state_dir,
            Some(&projects),
            agent.clone(),
        ))
        .expect("provider");
        restarted.recover().expect("recover");
        let previous = restarted
            .state()
            .session(&session_id)
            .expect("session")
            .target(&restarted.config.instance_id);
        assert_eq!(previous.generation, 1);
        assert!(
            !restarted
                .state()
                .session(&session_id)
                .expect("session")
                .closed
        );

        let resume = lifecycle_target_event(
            &restarted,
            channel_id,
            "resume-1",
            "session.resume",
            &previous,
        );
        restarted
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");
        let current = restarted
            .state()
            .session(&session_id)
            .expect("session")
            .target(&restarted.config.instance_id);
        assert_eq!(current.generation, 2);
        assert!(restarted.sessions.handle(&session_id).is_some());

        let stale_turn = turn_event(channel_id, "stale-after-resume", &previous);
        restarted
            .handle_command_event(channel_id, &stale_turn)
            .await
            .expect("stale turn");
        assert!(
            !restarted.state().is_command_consumed("stale-after-resume"),
            "the old generation stays fenced after reattachment"
        );

        let stop =
            lifecycle_target_event(&restarted, channel_id, "stop-1", "session.stop", &current);
        restarted
            .handle_command_event(channel_id, &stop)
            .await
            .expect("stop");
        assert!(
            restarted
                .state()
                .session(&session_id)
                .expect("session")
                .closed
        );

        drop(restarted);
        let mut after_stop =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        after_stop.recover().expect("recover");
        assert!(
            after_stop
                .state()
                .session(&session_id)
                .expect("session")
                .closed
        );
        assert_eq!(after_stop.sessions.live_count(), 0);
    }

    /// A recovered session keeps the driver it was created under even when its
    /// runtime descriptor is gone (adapter uninstalled across the restart).
    /// Consumers match transcripts by the full target, driver included — a
    /// synthesized result under a different driver would never render, so the
    /// turn would hang forever in the UI.
    #[tokio::test]
    async fn recovery_keeps_the_persisted_driver_when_the_descriptor_is_gone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let keys = Keys::generate();
        let channel_id = Uuid::new_v4();

        // A codex session with a turn in flight, written by a previous run.
        {
            let mut store = StateStore::open(&state_dir, 86_400).expect("state store");
            store
                .insert_session(SessionRecord {
                    session_id: "11111111-1111-4111-8111-111111111111".into(),
                    generation: 1,
                    channel_id,
                    command_id: "create-codex".into(),
                    provider_instance_ref: "codex-primary".into(),
                    runtime: "codex".into(),
                    driver: "codex-acp".into(),
                    cwd: dir.path().join("checkout"),
                    project_ref: None,
                    repo_ref: None,
                    session_ref: None,
                    genesis_ref: None,
                    founder_pubkey: Some("ab".repeat(32)),
                    model: None,
                    resume_cursor: Some("private-acp-cursor".into()),
                    title: None,
                    created_at_ms: now_ms(),
                    next_seq: 3,
                    open_turn: Some(OpenTurn {
                        turn_id: "turn-1".into(),
                        command_id: Some("turn-cmd-1".into()),
                        started_at_ms: now_ms(),
                    }),
                    closed: false,
                })
                .expect("insert");
            store
                .consume_command("create-codex", now_secs())
                .expect("consume");
        }

        // The restarted provider offers claude only — codex was uninstalled.
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let mut restarted =
            Provider::new(config_of(keys, &state_dir, None, agent)).expect("provider");
        restarted.recover().expect("recover");
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");

        let transcripts = sink.contents_of(KIND_CODING_SESSION_TRANSCRIPT);
        assert_eq!(transcripts.len(), 1);
        assert_eq!(
            transcripts[0]["session"]["driver"], "codex-acp",
            "the synthesized result must keep the driver the session published under"
        );
        assert_eq!(transcripts[0]["item"]["kind"], "result");

        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(
            metadata.last().expect("metadata")["session"]["driver"],
            "codex-acp"
        );
        assert_eq!(metadata.last().expect("metadata")["status"], "disconnected");
    }

    /// The whole ACP binding, end to end against a scripted agent.
    #[tokio::test]
    async fn a_turn_reaches_the_agent_and_publishes_a_terminal_result() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");

        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-1", &target))
            .await
            .expect("handle");

        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let transcripts = transcript_items_in_sequence(&sink);
        let kinds: Vec<&str> = transcripts
            .iter()
            .filter_map(|item| item["item"]["kind"].as_str())
            .collect();
        assert_eq!(kinds, vec!["user_prompt", "assistant_text", "result"]);

        // Sequences are dense and start at 1, and every item of the turn shares
        // the producer-minted turn id the consumer groups on.
        let seqs: Vec<u64> = transcripts
            .iter()
            .filter_map(|item| item["eventSeq"].as_u64())
            .collect();
        assert_eq!(seqs, vec![1, 2, 3]);
        let turn_ids: Vec<&str> = transcripts
            .iter()
            .filter_map(|item| item["turnId"].as_str())
            .collect();
        assert_eq!(turn_ids.len(), 3);
        assert!(turn_ids.windows(2).all(|pair| pair[0] == pair[1]));

        let last = transcripts.last().expect("result item");
        assert_eq!(last["item"]["subtype"], "success");
        assert_eq!(last["item"]["result"], "completed");
        assert_eq!(last["item"]["isError"], false);
        assert!(last["item"]["durationMs"].is_number());
    }

    /// An interrupt must produce a `cancelled` terminal item, not a hung turn.
    #[tokio::test]
    async fn an_interrupt_closes_the_turn_as_cancelled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let agent = fake_agent(dir.path(), "stalling-agent", STALLING_AGENT);
        let mut provider = Provider::new(config_of(
            Keys::generate(),
            &dir.path().join("state"),
            Some(&projects),
            agent,
        ))
        .expect("provider");

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-1", &target))
            .await
            .expect("handle");
        let started = tokio::time::timeout(Duration::from_secs(10), provider.next_session_event())
            .await
            .expect("turn start")
            .expect("event");
        provider.handle_session_event(started).expect("record");

        provider
            .handle_command_event(channel_id, &interrupt_event(channel_id, "int-1", &target))
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let transcripts = transcript_items_in_sequence(&sink);
        let last = transcripts.last().expect("result item");
        assert_eq!(last["item"]["kind"], "result");
        assert_eq!(last["item"]["subtype"], "cancelled");
        assert_eq!(last["item"]["isError"], false);
        assert_eq!(
            sink.contents_of(KIND_CODING_SESSION_METADATA)
                .last()
                .expect("metadata")["status"],
            "interrupted"
        );
    }

    #[tokio::test]
    async fn an_unresolvable_working_directory_produces_a_failed_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(receipts[0]["error"]["code"], "PROJECT_CWD_UNRESOLVED");
        assert!(receipts[0]["session"].is_null());
        assert_eq!(provider.state().sessions().count(), 0);
    }

    #[tokio::test]
    async fn the_session_cap_is_enforced_across_commands() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        for index in 0..3 {
            let event = create_event(&provider, channel_id, &format!("create-{index}"));
            provider
                .handle_command_event(channel_id, &event)
                .await
                .expect("handle");
        }
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 3);
        assert_eq!(receipts[2]["status"], "failed");
        assert_eq!(receipts[2]["error"]["code"], "SESSION_LIMIT");
        assert_eq!(provider.state().sessions().count(), 2);
    }

    #[tokio::test]
    async fn a_stale_generation_turn_is_dropped_without_consuming_its_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");

        let mut target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        target.generation = 2;

        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-1", &target))
            .await
            .expect("handle");
        assert!(
            !provider.state().is_command_consumed("turn-1"),
            "a fenced-out command must stay unconsumed so a correctly targeted retry still runs"
        );
    }

    #[tokio::test]
    async fn a_live_turn_is_consumed_and_delivered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");

        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-1", &target))
            .await
            .expect("handle");
        assert!(provider.state().is_command_consumed("turn-1"));
    }

    #[tokio::test]
    async fn watermarks_advance_so_a_restart_resumes_where_it_stopped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);
        let event = create_event(&provider, channel_id, "create-1");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        assert_eq!(
            provider.state().watermark(channel_id),
            Some(event.created_at.as_secs())
        );
    }

    #[test]
    fn first_membership_subscription_replays_from_the_membership_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);

        assert_eq!(
            provider.subscription_replay_since(channel_id, Some(1_000)),
            Some(1_000),
            "a queued first subscription must not fall forward to since=now"
        );

        provider
            .state
            .record_watermark(channel_id, 900)
            .expect("watermark");
        assert_eq!(
            provider.subscription_replay_since(channel_id, Some(1_000)),
            Some(900),
            "an existing command watermark remains the earliest safe replay floor"
        );
    }

    /// The relay caps 44225 at 32 KiB and the SDK re-checks it, so an item the
    /// agent made too big has to be shrunk here — silently dropping it would
    /// leave a hole in the record with nothing to explain it.
    #[tokio::test]
    async fn an_oversized_item_is_shrunk_to_fit_the_signed_event_cap() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");

        provider
            .enqueue_transcript(
                channel_id,
                &target,
                Some("turn-1"),
                serde_json::json!({
                    "kind": "assistant_text",
                    "text": "w".repeat(MAX_TRANSCRIPT_CONTENT_BYTES * 2),
                }),
                Priority::Normal,
            )
            .expect("transcript");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let transcripts = transcript_items_in_sequence(&sink);
        let published = transcripts.last().expect("transcript");
        assert_eq!(published["item"]["kind"], "assistant_text");
        assert_eq!(published["item"]["truncated"], true);
        let text = published["item"]["text"].as_str().expect("text");
        assert!(
            text.contains("…[elided "),
            "the elision is recorded in-band"
        );
        assert!(
            text.contains("sha256:"),
            "with a digest of what was dropped"
        );
        assert!(
            serde_json::to_string(published).expect("json").len() <= MAX_TRANSCRIPT_CONTENT_BYTES
        );
    }

    fn catalog_events(sink: &CollectingSink) -> Vec<Event> {
        sink.all()
            .into_iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_PROVIDER_CATALOG)
            .collect()
    }

    fn tag_value(event: &Event, name: &str) -> Option<String> {
        event
            .tags
            .iter()
            .map(nostr::Tag::as_slice)
            .find(|tag| tag.first().map(String::as_str) == Some(name))
            .and_then(|tag| tag.get(1).cloned())
    }

    #[tokio::test]
    async fn the_catalog_is_advertised_once_per_channel() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut provider = provider(&dir.path().join("state"), None);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        provider.subscribed.insert(first);
        provider.subscribed.insert(second);

        provider.refresh_catalog(true).expect("advertise");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let catalogs = catalog_events(&sink);
        assert_eq!(catalogs.len(), 2);
        assert_eq!(provider.state().catalog().revision, 1);

        // A second pass over unchanged input advertises nothing new.
        provider.refresh_catalog(true).expect("advertise");
        let quiet = CollectingSink::new();
        provider.flush(&quiet).await.expect("flush");
        assert!(catalog_events(&quiet).is_empty());
        assert_eq!(provider.state().catalog().revision, 1);
    }

    /// `cspc-revision` must equal the revision inside the content and `cspc-key`
    /// must digest the exact signed bytes — the consumer checks both before it
    /// will offer any target from the catalog.
    #[tokio::test]
    async fn catalog_tags_agree_with_the_content_they_describe() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut provider = provider(&dir.path().join("state"), None);
        let channel_id = Uuid::new_v4();
        provider.subscribed.insert(channel_id);
        provider.refresh_catalog(true).expect("advertise");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let event = catalog_events(&sink).into_iter().next().expect("catalog");

        let content: serde_json::Value =
            serde_json::from_str(&event.content).expect("catalog content");
        assert_eq!(content["schema"], catalog::CATALOG_SCHEMA);
        assert_eq!(content["revision"], 1);
        assert_eq!(
            content["providers"][0]["providerInstanceRef"],
            "claude-primary"
        );
        assert_eq!(content["providers"][0]["driver"], config::DRIVER);
        assert_eq!(content["providers"][0]["runtime"], config::RUNTIME);
        assert_eq!(content["providers"][0]["capabilities"]["plan"], true);

        assert_eq!(
            tag_value(&event, "h").as_deref(),
            Some(channel_id.to_string().as_str())
        );
        assert_eq!(tag_value(&event, "cspc-revision").as_deref(), Some("1"));
        assert_eq!(
            tag_value(&event, "cspc-key"),
            Some(coding_session_provider_catalog_semantic_key(
                &channel_id.to_string(),
                1,
                &event.content,
            ))
        );

        // Byte-for-byte re-serialization, which is what the consumer performs.
        let reparsed: catalog::Catalog =
            serde_json::from_value(content).expect("catalog round-trips");
        assert_eq!(
            catalog::to_canonical_json(&reparsed).expect("serialize"),
            event.content
        );
    }

    /// The revision is a claim that something changed. It has to bump when the
    /// offer changes and stay put when it does not.
    #[tokio::test]
    async fn the_revision_bumps_only_when_the_offer_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let projects_path = dir.path().join("projects.json");
        std::fs::write(&projects_path, br#"{"version":1}"#).expect("write");
        let mut provider = provider(&dir.path().join("state"), Some(&projects_path));
        let channel_id = Uuid::new_v4();
        provider.subscribed.insert(channel_id);

        provider.refresh_catalog(true).expect("advertise");
        provider.flush(&CollectingSink::new()).await.expect("flush");
        assert_eq!(provider.state().catalog().revision, 1);

        // Rewriting the file with equivalent content is not a new offer.
        std::fs::write(&projects_path, br#"{"version":1,"channels":{}}"#).expect("write");
        provider.refresh_catalog(true).expect("advertise");
        assert_eq!(provider.state().catalog().revision, 1);

        // Adding a servable project is.
        std::fs::write(
            &projects_path,
            format!(
                r#"{{"version":1,"projects":{{"30621:{}:demo":"{}"}}}}"#,
                "cd".repeat(32),
                dir.path().display()
            ),
        )
        .expect("write");
        provider.refresh_catalog(true).expect("advertise");
        assert_eq!(provider.state().catalog().revision, 2);

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let event = catalog_events(&sink).into_iter().next().expect("catalog");
        let content: serde_json::Value = serde_json::from_str(&event.content).expect("content");
        assert_eq!(content["revision"], 2);
        assert_eq!(content["projects"][0]["repoRef"], serde_json::Value::Null);
        assert_eq!(tag_value(&event, "cspc-revision").as_deref(), Some("2"));
    }

    /// A channel joined later must receive the catalog it missed; without it an
    /// operator in that channel has no provider to pick.
    #[tokio::test]
    async fn a_channel_joined_later_still_receives_the_current_catalog() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut provider = provider(&dir.path().join("state"), None);
        let first = Uuid::new_v4();
        provider.subscribed.insert(first);
        provider.refresh_catalog(true).expect("advertise");
        provider.flush(&CollectingSink::new()).await.expect("flush");

        let late = Uuid::new_v4();
        provider.subscribed.insert(late);
        provider.refresh_catalog(false).expect("advertise");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let catalogs = catalog_events(&sink);
        assert_eq!(catalogs.len(), 1);
        assert_eq!(
            tag_value(&catalogs[0], "h").as_deref(),
            Some(late.to_string().as_str())
        );
        assert_eq!(
            provider.state().catalog().revision,
            1,
            "reaching a new channel is not a new offer"
        );
    }

    /// The revision survives a restart: restarting is not a capability change,
    /// and a consumer that kept revision 3 must not be handed a fresh 1.
    #[tokio::test]
    async fn the_revision_survives_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let keys = Keys::generate();
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let channel_id = Uuid::new_v4();

        let mut first =
            Provider::new(config_of(keys.clone(), &state_dir, None, agent.clone())).expect("first");
        first.subscribed.insert(channel_id);
        first.refresh_catalog(true).expect("advertise");
        first.flush(&CollectingSink::new()).await.expect("flush");
        drop(first);

        let mut restarted =
            Provider::new(config_of(keys, &state_dir, None, agent)).expect("restarted");
        restarted.subscribed.insert(channel_id);
        restarted.refresh_catalog(true).expect("advertise");
        assert_eq!(restarted.state().catalog().revision, 1);
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        assert!(
            catalog_events(&sink).is_empty(),
            "an unchanged catalog is not re-advertised after a restart"
        );
    }

    #[tokio::test]
    async fn transcript_sequences_start_at_one_and_never_repeat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");

        let mut seqs = Vec::new();
        for index in 0..3 {
            seqs.push(
                provider
                    .enqueue_transcript(
                        channel_id,
                        &target,
                        Some("turn-1"),
                        serde_json::json!({ "kind": "assistant_text", "text": index.to_string() }),
                        Priority::Normal,
                    )
                    .expect("transcript"),
            );
        }
        assert_eq!(seqs, vec![Some(1), Some(2), Some(3)]);
    }
}
