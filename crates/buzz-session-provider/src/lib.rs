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

pub mod commands;
pub mod config;
pub mod payload;
pub mod publish;
pub mod session;
pub mod state;

use std::time::Duration;

use nostr::Event;
use uuid::Uuid;

use buzz_acp::relay::HarnessRelay;
use buzz_acp::ChannelFilter;
use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_TRANSCRIPT, KIND_MEMBER_ADDED_NOTIFICATION,
    KIND_MEMBER_REMOVED_NOTIFICATION,
};
use buzz_sdk::builders::{
    build_coding_session_lifecycle_receipt, build_coding_session_metadata,
    build_coding_session_transcript_item,
};
use buzz_sdk::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, coding_session_metadata_semantic_key,
    coding_session_transcript_semantic_key,
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
use session::{CreateRequest, SessionCommand, SessionManager};
use state::{now_ms, now_secs, SessionRecord, StateStore};

/// How often the outbox is drained when nothing else is happening.
const OUTBOX_TICK: Duration = Duration::from_secs(2);

/// Entry point: read the environment and run until shutdown.
pub async fn run() -> anyhow::Result<()> {
    init_tracing();
    let config = Config::from_env()?;
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
            _ = ticker.tick() => {
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
}

impl Provider {
    /// Open durable state and the publish outbox for `config`.
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let pubkey_hex = config.pubkey_hex();
        let state = StateStore::open(&config.state_dir, config.command_horizon.as_secs())?;
        let outbox = Outbox::open(&config.state_dir, &pubkey_hex)?;
        Ok(Self {
            config,
            pubkey_hex,
            state,
            outbox,
            sessions: SessionManager::new(),
        })
    }

    /// Repair state left behind by an unclean exit.
    ///
    /// A create that minted a session record but died before its `commandId`
    /// reached the ledger would otherwise replay into a second session for the
    /// same command, so the ledger is reconciled against the records here.
    /// Mid-turn repair — synthesizing the terminal item a dead process never
    /// published — arrives with the ACP binding.
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
        Ok(())
    }

    /// Subscribe to the two command kinds in one channel, replaying from the
    /// persisted watermark so a restart cannot silently skip an unseen command.
    async fn subscribe(
        &self,
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
            .await
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
            }
            KIND_MEMBER_REMOVED_NOTIFICATION => {
                tracing::info!(target: "csp", %channel_id, "membership revoked — unsubscribing");
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

        let target = CodingSessionTarget {
            driver: config::DRIVER.to_owned(),
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
            agent_command: self.config.agent_command.clone(),
            idle_timeout: self.config.idle_timeout,
            max_turn_duration: self.config.max_turn_duration,
        };

        if let Err(failure) = self.sessions.create(request).await {
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

        let record = SessionRecord {
            session_id: target.session_id.clone(),
            generation: target.generation,
            channel_id: plan.channel_id,
            command_id: plan.command_id.clone(),
            cwd: plan.cwd,
            project_ref: plan.project_ref.clone(),
            repo_ref: plan.repo_ref.clone(),
            model: plan.model.clone(),
            title: plan.title.clone(),
            created_at_ms: now_ms(),
            next_seq: 1,
            open_turn: None,
            closed: false,
        };
        self.state.insert_session(record)?;

        let receipt = LifecycleReceipt::created(&plan.command_id, &target);
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)?;
        self.enqueue_metadata(
            plan.channel_id,
            &target,
            self.metadata_for(&target, SessionStatus::Idle),
        )?;

        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %target.session_id,
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
            provider_instance_ref: config::PROVIDER_INSTANCE_REF,
            driver: config::DRIVER,
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
        SessionMetadata {
            schema: METADATA_SCHEMA.to_owned(),
            session: target.clone(),
            project_ref: record.and_then(|record| record.project_ref.clone()),
            repo_ref: record.and_then(|record| record.repo_ref.clone()),
            title: payload::nullable(record.and_then(|record| record.title.as_deref())),
            agent_ref: None,
            provider: Some(config::PROVIDER_INSTANCE_REF.to_owned()),
            runtime: Some(config::RUNTIME.to_owned()),
            model: record
                .and_then(|record| record.model.clone())
                .or_else(|| Some(self.config.default_model.clone())),
            status,
            branch: None,
            capabilities: Capabilities::claude_agent_acp(),
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

    /// Queue per-generation metadata (44223).
    pub fn enqueue_metadata(
        &mut self,
        channel_id: Uuid,
        target: &CodingSessionTarget,
        metadata: SessionMetadata,
    ) -> anyhow::Result<()> {
        let content = serde_json::to_string(&metadata)?;
        let event = build_coding_session_metadata(channel_id, target, &content)?
            .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(
            KIND_CODING_SESSION_METADATA,
            &coding_session_metadata_semantic_key(target),
            Priority::High,
            event,
        )?;
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
        let envelope = TranscriptEnvelope::new(target, event_seq, now_ms(), turn_id, item);
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

    fn config_of(keys: Keys, state_dir: &Path, projects: Option<&Path>) -> Config {
        Config {
            keys,
            relay_url: "ws://localhost:3000".into(),
            auth_tag: None,
            state_dir: state_dir.to_path_buf(),
            projects_file: projects.map(Path::to_path_buf),
            instance_id: "instance-1".into(),
            agent_command: "claude-agent-acp".into(),
            default_model: "claude-sonnet-4-6".into(),
            allowed_models: vec!["claude-sonnet-4-6".into()],
            max_sessions: 2,
            session_idle_shutdown: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(900),
            max_turn_duration: Duration::from_secs(7200),
            include_thoughts: true,
            command_horizon: Duration::from_secs(86_400),
        }
    }

    fn provider(state_dir: &Path, projects: Option<&Path>) -> Provider {
        Provider::new(config_of(Keys::generate(), state_dir, projects)).expect("provider")
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
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "model": null,
                "title": "Ship it",
                "initialTurn": null,
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
        let content = serde_json::json!({
            "schema": "buzz-coding-session-command/v1",
            "commandId": command_id,
            "target": target,
            "action": { "type": "thread.turn.start", "text": "do the thing" },
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
            .target(config::DRIVER, "instance-1");
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
        let mut first =
            Provider::new(config_of(keys.clone(), &state_dir, Some(&projects))).expect("provider");
        let event = create_event(&first, channel_id, "create-1");
        first
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        let sink = CollectingSink::new();
        first.flush(&sink).await.expect("flush");
        drop(first);

        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects))).expect("provider");
        restarted.recover().expect("recover");
        restarted
            .handle_command_event(channel_id, &event)
            .await
            .expect("replay");
        assert_eq!(restarted.state().sessions().count(), 1);
        assert_eq!(
            restarted.pending_publishes(),
            0,
            "the replay must not re-enqueue a receipt or metadata"
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
            .target(config::DRIVER, "instance-1");
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
            .target(config::DRIVER, "instance-1");

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
            .target(config::DRIVER, "instance-1");

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
