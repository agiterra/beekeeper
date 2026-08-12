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
//!
//! # Environment
//!
//! `BUZZ_PRIVATE_KEY`, `BUZZ_RELAY_URL`, `BUZZ_AUTH_TAG`, `RUST_LOG`, plus the
//! `BUZZ_CSP_*` surface documented on [`config::Config`].

#![deny(unsafe_code)]

pub mod catalog;
pub mod commands;
pub mod config;
mod model_catalog;
pub mod payload;
pub mod publish;
pub mod session;
pub mod state;
pub mod transcript;

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, SystemTime};

use nostr::Event;
use tokio::sync::mpsc;
use uuid::Uuid;

use buzz_acp::relay::HarnessRelay;
use buzz_acp::{ChannelFilter, TurnUsage};
use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_PROVIDER_CATALOG, KIND_CODING_SESSION_TRANSCRIPT,
    KIND_MEMBER_ADDED_NOTIFICATION, KIND_MEMBER_REMOVED_NOTIFICATION,
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
    TurnDecision,
};
use config::Config;
use payload::{
    Capabilities, LifecycleReceipt, SessionMetadata, SessionStatus, TranscriptEnvelope,
    METADATA_SCHEMA,
};
use publish::{EventSink, Outbox, Priority};
use session::{CreateRequest, SessionCommand, SessionEvent, SessionManager, TurnOutcome};
use state::{now_ms, now_secs, CatalogState, OpenTurn, SessionRecord, StateStore};

/// How often the outbox is drained when nothing else is happening.
const OUTBOX_TICK: Duration = Duration::from_secs(2);
/// Backlog of actor reports the provider loop will buffer.
const SESSION_EVENT_CAPACITY: usize = 256;

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
        provider.subscribe(&mut relay, channel_id).await?;
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

/// The provider's whole runtime state, minus the relay socket.
pub struct Provider {
    config: Config,
    pubkey_hex: String,
    state: StateStore,
    outbox: Outbox,
    sessions: SessionManager,
    session_events: mpsc::Receiver<SessionEvent>,
    last_metadata: HashMap<String, String>,
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
            sessions: SessionManager::new(events_tx),
            session_events,
            last_metadata: HashMap::new(),
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
    /// 2. Every session that was still open is retired: a turn caught mid-flight
    ///    gets the terminal `result` item its consumer is waiting on — without
    ///    it the turn renders as running forever — and every open generation
    ///    gets `disconnected` metadata.
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
                record.closed = true;
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
    ) -> Result<(), buzz_acp::relay::RelayError> {
        let filter = ChannelFilter {
            kinds: Some(vec![
                KIND_CODING_SESSION_COMMAND,
                KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            ]),
            require_mention: false,
        };
        relay
            .subscribe_channel_from(channel_id, filter, self.state.watermark(channel_id))
            .await?;
        self.subscribed.insert(channel_id);
        Ok(())
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
                self.subscribe(relay, channel_id).await?;
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
                self.handle_command_event(channel_id, event).await?;
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
        let kind = u32::from(event.kind.as_u16());
        let created_at = event.created_at.as_secs();
        match kind {
            KIND_CODING_SESSION_LIFECYCLE_COMMAND => {
                self.on_lifecycle(channel_id, created_at, &event.content)
                    .await?;
            }
            KIND_CODING_SESSION_COMMAND => {
                self.on_turn(channel_id, created_at, &event.content).await?;
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
        content: &str,
    ) -> anyhow::Result<()> {
        // Re-read on every command so an operator can fix a missing working
        // directory and republish without restarting the provider.
        let projects = ProjectsFile::load(self.config.projects_file.as_deref());
        let decision = decide_lifecycle(&self.context(&projects), channel_id, created_at, content);
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
                tracing::warn!(target: "csp", %command_id, code, "create rejected: {message}");
                self.enqueue_receipt(channel_id, &command_id, &receipt)
            }
            LifecycleDecision::Create(plan) => self.create_session(*plan).await,
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
            model: startup.model.clone().or_else(|| plan.model.clone()),
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
        self.publish_metadata(plan.channel_id, &target, status)?;

        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %target.session_id,
            acp_session_id = %startup.acp_session_id,
            "session created"
        );
        Ok(())
    }

    async fn on_turn(
        &mut self,
        _channel_id: Uuid,
        created_at: u64,
        content: &str,
    ) -> anyhow::Result<()> {
        let projects = ProjectsFile::default();
        let decision = commands::decide_turn(&self.context(&projects), created_at, content);
        let (command_id, session_id, message) = match decision {
            TurnDecision::Ignore(reason) => {
                log_ignored("turn", &reason);
                return Ok(());
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

    fn context<'a>(&'a self, projects: &'a ProjectsFile) -> CommandContext<'a> {
        CommandContext {
            provider_pubkey: &self.pubkey_hex,
            runtimes: &self.config.runtimes,
            instance_id: &self.config.instance_id,
            now_secs: now_secs(),
            horizon_secs: self.config.command_horizon.as_secs(),
            max_sessions: self.config.max_sessions,
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
            branch: None,
            capabilities: descriptor
                .and_then(|descriptor| descriptor.capabilities)
                .unwrap_or_else(|| Capabilities::v1_for_runtime(&runtime_slug)),
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
        if self.last_metadata.get(&target.session_id) == Some(&content) {
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
        self.last_metadata
            .insert(target.session_id.clone(), content);
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

    /// Turn one actor report into durable state and queued events.
    pub fn handle_session_event(&mut self, event: SessionEvent) -> anyhow::Result<()> {
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
                let (item, status, fatal) = turn_result(&outcome, duration_ms, usage.as_deref());
                self.enqueue_transcript(channel_id, &target, Some(&turn_id), item, Priority::High)?;
                self.state.update_session(&session_id, |record| {
                    record.open_turn = None;
                    record.closed |= fatal;
                })?;
                self.publish_metadata(channel_id, &target, status)?;
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
                self.state.update_session(&session_id, |record| {
                    record.open_turn = None;
                    record.closed = true;
                })?;
                self.publish_metadata(channel_id, &target, SessionStatus::Disconnected)?;
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

/// Map a turn outcome onto its terminal transcript item, the generation's next
/// status, and whether the session can serve another turn.
fn turn_result(
    outcome: &TurnOutcome,
    duration_ms: u64,
    usage: Option<&TurnUsage>,
) -> (serde_json::Value, SessionStatus, bool) {
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
                false,
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
            false,
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
            *agent_gone,
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
    use std::sync::Mutex;

    use nostr::Keys;

    use crate::session::testing::{fake_agent, GOOD_AGENT, STALLING_AGENT};

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

    /// Record every actor report that is already available.
    async fn pump_available(provider: &mut Provider) {
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(250), provider.next_session_event()).await
        {
            provider.handle_session_event(event).expect("record");
        }
    }

    /// Drain and record actor reports until a turn has finished.
    async fn pump_until_turn_finished(provider: &mut Provider) {
        loop {
            let event =
                tokio::time::timeout(Duration::from_secs(20), provider.next_session_event())
                    .await
                    .expect("session event within timeout")
                    .expect("channel open");
            let finished = matches!(event, session::SessionEvent::TurnFinished { .. });
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
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16),
            content,
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(&Keys::generate())
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

    fn command_event(
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        action: serde_json::Value,
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
        .sign_with_keys(&Keys::generate())
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

        assert_eq!(provider.state().sessions().count(), 1);
        assert_eq!(provider.sessions.live_count(), 1);
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
        assert!(record.closed);
        assert!(record.open_turn.is_none());
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
                    model: None,
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
