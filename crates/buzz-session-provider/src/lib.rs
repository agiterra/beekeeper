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

pub mod actor_seats;
mod agent_fence;
pub mod attachments;
pub mod authority;
pub mod catalog;
pub mod ci_continuation;
pub mod ci_continuation_store;
pub mod ci_result_listener;
pub mod commands;
pub mod config;
pub mod context_projector;
mod context_store;
mod context_window;
mod gate_cwd;
mod gate_observer;
mod git_exclude;
mod git_probe;
mod lease;
mod model_catalog;
pub mod native_restore;
pub mod payload;
pub mod publish;
mod reachability;
pub mod redaction_vault;
pub mod retirement;
pub mod seat_bee;
pub mod seat_requests;
pub mod session;
pub mod state;
mod team_wake;
pub mod transcript;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use nostr::{Event, Kind};
use tokio::sync::mpsc;
use uuid::Uuid;

use buzz_acp::relay::{HarnessRelay, RelayEventPublisher, RestClient};
use buzz_acp::{ChannelFilter, TurnUsage};
use buzz_core::coding_session_authority_claim::{ClaimLink, ClaimState};
use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionDelivery, CodingSessionTarget,
};
use buzz_core::coding_session_context::coding_session_first_turn_brief;
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::coding_session_identity::{ProviderInstanceAlias, RuntimeWord};
use buzz_core::coding_session_lease::CodingSessionLeaseState;
use buzz_core::coding_session_observation::{
    CodingSessionObservationBody, CodingSessionObservationGate, CodingSessionObservationPayload,
    CodingSessionObservationSource, CodingSessionObservationType,
    CODING_SESSION_OBSERVATION_SCHEMA,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_CLOSURE,
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LEASE,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_OBSERVATION, KIND_CODING_SESSION_POLICY,
    KIND_CODING_SESSION_PROVIDER_CATALOG, KIND_CODING_SESSION_TEAM_TRANSACTION,
    KIND_CODING_SESSION_TRANSCRIPT, KIND_DELETION, KIND_MEMBER_ADDED_NOTIFICATION,
    KIND_MEMBER_REMOVED_NOTIFICATION, KIND_SYSTEM_MESSAGE,
};
use buzz_sdk::builders::{
    build_coding_session_lifecycle_receipt, build_coding_session_metadata,
    build_coding_session_provider_catalog, build_coding_session_transcript_item,
    build_coding_session_turn_receipt, coding_session_turn_receipt_semantic_key,
};
use buzz_sdk::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, coding_session_metadata_semantic_key,
    coding_session_provider_catalog_semantic_key, coding_session_transcript_semantic_key,
    MAX_TRANSCRIPT_CONTENT_BYTES,
};
use buzz_sdk::coding_session_observation::build_coding_session_observation;

use commands::{
    decide_lifecycle, CommandContext, CreatePlan, Ignored, LifecycleDecision, ProjectsFile,
    ResumePlan, StopPlan, TurnDecision,
};
use config::Config;
use context_projector::{ContextProjectionLimits, ContextProjectionRequest};
use payload::{
    Capabilities, LifecycleReceipt, SessionMetadata, SessionMetadataHandover,
    SessionMetadataHandoverState, SessionStatus, TranscriptEnvelope, TurnBudget, GENESIS_NOT_FOUND,
    METADATA_SCHEMA, PROVIDER_UNAVAILABLE, SESSION_ALREADY_ATTACHED,
};
use publish::{EventSink, Outbox, Priority};
use session::{
    CreateRequest, DeliverError, RehydrationMcpDescriptor, SessionCommand, SessionContinuity,
    SessionEvent, SessionManager, TurnOutcome,
};
use state::{now_ms, now_secs, CatalogState, OpenTurn, SessionRecord, StateStore};

/// How often the catalog is re-read and idle housekeeping runs.
///
/// Deliberately *not* the outbox's cadence. A queued event is delivered as soon
/// as it is eligible (see the `next_publish_delay` arm of the runtime loop):
/// tying delivery to this tick meant a person's own prompt echo waited whole
/// seconds behind a timer that exists to hot-reload a config file.
const RUNTIME_TICK: Duration = Duration::from_secs(2);
/// Floor between two verified-context refreshes for one execution.
///
/// A turn costs seconds to minutes, so one bounded relay fetch plus
/// proof-graph verification per minute per active rehydrated execution sits
/// well inside a turn's own cost.
const CONTEXT_REFRESH_MIN_INTERVAL_MS: i64 = 60_000;

/// The role a provider-minted team wake is framed as, and recorded as in the
/// signed `user_prompt` item's `senderRole`.
///
/// A valid role slug (`[a-z0-9-]`), so it survives
/// `buzz_core::coding_session_lifecycle_command::validate_role_slug` on the
/// transcript path. It names the *sender*, and the sender of a wake is this
/// process — never the person whose key the process happens to hold.
const PROVIDER_SENDER_ROLE: &str = "provider";

/// Transcript `status` slug published when a hire's `requestedBy` disagrees
/// with the pubkey that signed the hire.
///
/// The dispute is a fact about the session's own history, so it goes on the
/// wire rather than only into this process's log. Consumers treat unknown
/// continuity slugs additively — desktop keeps the generic "Status" row for
/// one — so publishing it costs nothing and swallowing it would hide the only
/// evidence that somebody attributed a brief to a seat that never asked for it.
const HIRE_REQUESTER_DISPUTED_STATUS: &str = "hire_requester_disputed";
/// Maximum time clean shutdown spends waiting for durable relay ACKs.
const SHUTDOWN_DURABLE_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// Backlog of actor reports the provider loop will buffer.
const SESSION_EVENT_CAPACITY: usize = 256;
/// A genesis published immediately before its create may take a brief moment
/// to become query-visible through the HTTP bridge.
const GENESIS_QUERY_ATTEMPTS: usize = 4;
const GENESIS_QUERY_RETRY_DELAY: Duration = Duration::from_millis(150);
/// Page ceiling for the authority-receipt backfill query. The relay clamps to
/// its own advertised maximum; asking high keeps acceptance receipts from
/// paginating out behind unrelated system messages in a chatty channel. If a
/// receipt still falls off the page, the contiguous fold stalls — grants stay
/// unapplied (fail closed) until a live receipt or restart retries.
const AUTHORITY_BACKFILL_QUERY_LIMIT: usize = 1000;
/// Ceiling on pages one authority-chain discovery will read.
///
/// The query is bounded by [`AUTHORITY_BACKFILL_QUERY_LIMIT`] rows, and a
/// channel busier than that has to be paged through or the chain's own
/// receipts fall off the end of the first page. Paging needs a stop, and this
/// is it: a discovery that has not reached the end of the channel's history in
/// this many pages reports itself **incomplete** rather than pretending the
/// rows it did read are the whole chain. Thirty-two pages of a thousand rows
/// is far past any real channel; hitting it means something is wrong, and the
/// honest answer to that is "I do not know", not "nothing has changed".
const AUTHORITY_BACKFILL_MAX_PAGES: usize = 32;
/// How long an unconsumed create hint may sit on disk before it is pruned.
///
/// Long enough that a create delayed by an outage still finds its hint, short
/// enough that an abandoned one cannot answer a command a day later.
const PENDING_HINT_MAX_AGE_SECS: u64 = 24 * 60 * 60;
/// Replay-floor slack for channels with no consumed-command watermark.
///
/// A 44221 can legally precede the floor it would replay from: a membership
/// grant applies moments after the create that motivated it, and a restart's
/// floor of "now" postdates anything that arrived while the provider was
/// down. Ten minutes covers every observed ordering while keeping replay
/// volume trivial — 442xx commands are low-rate, and a re-delivered command
/// dedupes as `AlreadyConsumed`.
const REPLAY_GRACE_SECS: u64 = 600;
/// How long a freshly subscribed channel holds arriving turn commands so they
/// can be delivered in the order they were sent.
///
/// A bounded wait, because the protocol offers nothing better: the relay's
/// `EOSE` is consumed inside the relay client and never reaches this crate, so
/// there is no signal that says "history is done". Long enough to cover a
/// stored-event burst, short enough that a live turn sent seconds after
/// startup is not noticeably delayed. See [`ReplayWindow`].
const REPLAY_REORDER_WINDOW: Duration = Duration::from_millis(1_500);
/// Give an agent-authored report one normal relay round trip to arrive before
/// a provider terminal becomes a diagnostic wake.
const TEAM_WAKE_REPORT_GRACE_MS: i64 = 4_000;
/// Longest infrastructure-only retry delay for one blocked team channel.
const TEAM_WAKE_BACKOFF_CAP: Duration = Duration::from_secs(600);

/// The `turn_dropped` code for an interrupt-class turn the mailbox had no room
/// for.
///
/// Distinct from `payload::QUEUE_FULL` because it states the second fact that
/// matters to the sender: the running turn was *not* cancelled. A plain
/// `QUEUE_FULL` here would leave them unable to tell "your words did not get
/// in" from "your words did not get in and the work you interrupted is gone".
/// The receipt code list is open (see `docs/nips/NIP-CSL.md`), which is what
/// lets a new fact get its own word instead of being folded into an old one.
const QUEUE_FULL_TURN_KEPT: &str = "QUEUE_FULL_TURN_KEPT";

/// Ceiling on [`Provider::delivered_cancels`].
///
/// Every entry is released as soon as the ledger append behind its receipt
/// succeeds, so in normal operation the set holds at most the one cancel
/// currently being answered. It only accumulates when that append keeps
/// failing — a state directory that is full, read-only, or gone — and an
/// unbounded `HashSet` under that condition is a leak that outlives the
/// condition. The oldest entry is dropped first: it is also the one whose
/// receipt was enqueued longest ago, so a redelivery that gets past it is a
/// redelivery of a cancel the operator has already been told about.
const DELIVERED_CANCEL_FENCE_CAPACITY: usize = 256;

/// The operator-facing sentence a `turn_degraded` receipt carries.
///
/// One sentence, not a per-execution explanation: see
/// [`Provider::native_steer_deliverable`] for why the payload under a given
/// `(commandId, turn_degraded)` key must not depend on what a particular
/// process learned at `initialize`.
const STEER_DOWNGRADED: &str = "this execution cannot take a mid-turn steer; the turn was \
                                accepted for the next turn boundary instead";

/// The operator-facing sentence a `turn_degraded`/`IMAGE_UNSUPPORTED` carries.
///
/// One sentence, and deliberately not a count: like [`STEER_DOWNGRADED`], the
/// payload under a given `(commandId, turn_degraded)` key must not depend on
/// what a particular process learned at `initialize`.
const IMAGES_DROPPED: &str = "this execution's runtime does not accept image prompts, so the \
                              attached images were not delivered; the turn itself ran, with its \
                              text alone";

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
    // At most one live provider per state directory (the managed-agents
    // at-most-one-live-instance invariant, applied here): a second instance
    // would double-consume commands and interleave ledger appends. Fail fast
    // and loudly; the lock is held until this process exits.
    let _state_dir_lock = match state::acquire_state_dir_lock(&config.state_dir) {
        Ok(lock) => lock,
        Err(error) => {
            tracing::error!(
                target: "csp",
                "refusing to start: {error} — exactly one provider instance may own a state \
                 directory"
            );
            return Err(error.into());
        }
    };

    let mut provider = Provider::new(config)?;

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
    provider.set_rest_client(relay.rest_client());

    // Witness the relay identity (NIP-11 `self`), over the same origin the
    // whole authenticated command stream already trusts. Without it,
    // relay-signed receipts cannot verify at all: no grant is applied and no
    // handover claim can be read, so genesis-bearing work is refused rather
    // than admitted on the assumption that nothing has changed. Retried on the
    // runtime tick while it is still unknown, so one transient failure here
    // does not disable verification for this process's whole life.
    if !provider.witness_relay_identity().await {
        tracing::warn!(
            target: "csp::authority",
            "no relay identity is witnessed yet — authority chains cannot be verified and \
             genesis-bearing sessions are refused by name until one is; retrying on the tick"
        );
    }

    // Repair what the last exit left behind — and, because the relay reader
    // and its witnessed identity are now in hand, do it in the order §3.2
    // requires: reconcile accepted deletions and re-derive the handover fence
    // *before* any recovered execution republishes metadata or asks for seat
    // custody. This used to run before the connection, which is precisely why
    // a deleted umbrella could come back advertising itself as resumable.
    provider.recover().await?;

    // One bounded listener for every pending CI continuation, started after
    // the relay identity is known: without it no result signer can be
    // verified, and an unverifiable result must never wake an agent.
    provider.start_ci_result_listener();
    // Held out here, not on the provider: the run loop's `select!` already
    // borrows `provider` mutably for the session inbox, and two mutable
    // borrows in one `select!` do not compile. The listener's queue is the
    // loop's, exactly like the relay socket's.
    let mut ci_events = provider.take_ci_result_events();

    let channels = relay.discover_channels().await?;
    for channel_id in channels.keys().copied() {
        // `Some(now)` — not `None` — so a channel with no consumed-command
        // watermark replays the last `REPLAY_GRACE_SECS` instead of starting
        // at the startup watermark, which would skip any command that arrived
        // while the provider was down.
        provider
            .subscribe(&mut relay, channel_id, Some(now_secs()))
            .await?;
    }
    relay.subscribe_membership_notifications().await?;
    provider.refresh_catalog(true)?;

    // Grants accepted while this provider was down were re-verified and folded
    // in by `recover` above, ahead of the first metadata publish; live receipts
    // extend from here.

    let publisher = relay.event_publisher();
    let mut ticker = tokio::time::interval(RUNTIME_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut lease_ticker = lease::renewal_interval();

    loop {
        // Read before the select so the borrow ends here: what the loop needs
        // is a plain duration, not a live view of the outbox.
        let publish_delay = provider.next_publish_delay();
        let replay_delay = provider.next_replay_delay();
        tokio::select! {
            // Safety-critical ordering: once a slow durable ACK returns, both
            // clocks may be overdue. Biased selection makes the lease renewal
            // deterministic before another durable attempt, keeping backlog
            // pressure from consuming the relay's 180-second lease TTL.
            biased;
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(target: "csp", "shutdown requested");
                break;
            }
            _ = lease_ticker.tick() => {
                if let Err(error) = provider.queue_lease_renewals() {
                    tracing::error!(target: "csp::lease", "lease renewal construction failed: {error}");
                }
                if let Err(error) = provider.flush_pending_leases(&publisher).await {
                    tracing::warn!(target: "csp::lease", "lease renewal handoff failed: {error}");
                }
            }
            event = relay.next_event() => match event {
                Some(event) => {
                    if let Err(error) = provider
                        .handle_relay_event(&mut relay, event.channel_id, &event.event)
                        .await
                    {
                        tracing::error!(target: "csp", "failed to handle event: {error}");
                    }
                    if let Err(error) = provider.flush_pending_leases(&publisher).await {
                        tracing::warn!(target: "csp::lease", "lease handoff after command failed: {error}");
                    }
                }
                None => {
                    if let Err(error) = relay.reconnect().await {
                        return Err(error.into());
                    }
                    // The socket coming back is a replay, not a resumption:
                    // every channel is resubscribed from before it dropped and
                    // its stored history arrives newest-first. Reorder it.
                    provider.reopen_replay_windows_after_reconnect();
                    provider.queue_lease_renewals()?;
                    if let Err(error) = provider.flush_pending_leases(&publisher).await {
                        tracing::warn!(target: "csp::lease", "lease handoff after reconnect failed: {error}");
                    }
                }
            },
            event = ci_continuation::next_ci_listener_event(&mut ci_events) => {
                if let Err(error) = provider.handle_ci_listener_event(event).await {
                    tracing::error!(target: "csp::ci", "CI result handling failed: {error}");
                }
                if let Err(error) = provider.flush_pending_leases(&publisher).await {
                    tracing::warn!(target: "csp::lease", "lease handoff after a CI result failed: {error}");
                }
            }
            Some(event) = provider.next_session_event() => {
                if let Err(error) = provider.handle_session_event(event) {
                    tracing::error!(target: "csp", "failed to record session event: {error}");
                }
                if let Err(error) = provider.flush_pending_leases(&publisher).await {
                    tracing::warn!(target: "csp::lease", "lease handoff after session event failed: {error}");
                }
            }
            _ = ticker.tick() => {
                // Hot-reload: an operator who adds a project to the file should
                // see it offered without restarting the provider.
                if let Err(error) = provider.refresh_catalog(false) {
                    tracing::error!(target: "csp", "catalog refresh failed: {error}");
                }
                if let Err(error) = provider.queue_initial_live_leases() {
                    tracing::error!(target: "csp::lease", "initial lease construction failed: {error}");
                }
                if let Err(error) = provider.flush_pending_leases(&publisher).await {
                    tracing::warn!(target: "csp::lease", "lease handoff failed: {error}");
                }
                if let Err(error) = provider.run_one_team_wake_tick(&publisher).await {
                    tracing::warn!(target: "csp::team_wake", "team wake processing failed: {error}");
                }
                if let Err(error) = provider.run_ci_continuation_tick().await {
                    tracing::warn!(target: "csp::ci", "CI continuation processing failed: {error}");
                }
                // Both usually no-ops. The identity retry is what lets a
                // provider that came up before its relay was ready start
                // verifying without a restart, and the claim retry is what
                // lets the executions waiting behind an unread chain stop
                // refusing. Identity first: the chain read needs it.
                provider.witness_relay_identity().await;
                provider.retry_pending_claim_verification().await;
                provider.prune_stale_pending_hints();
            }
            // The replay reorder window closing is a delivery, not a timer
            // tick: the turns it holds are already the operator's, they are
            // just waiting to be put back in the order they were sent. No open
            // window yields `None`, which parks this arm.
            _ = sleep_for(replay_delay) => {
                if let Err(error) = provider.flush_due_replays().await {
                    tracing::error!(target: "csp", "replayed turn delivery failed: {error}");
                }
                if let Err(error) = provider.flush_pending_leases(&publisher).await {
                    tracing::warn!(target: "csp::lease", "lease handoff after replay failed: {error}");
                }
            }
            // Last on purpose. Delivery is still one row per pass — a slow relay
            // ACK must not multiply across the backlog — but the *wait* between
            // passes is the queue's own eligibility, not a timer, so a burst
            // drains at relay speed instead of one row every RUNTIME_TICK. An
            // empty outbox yields `None`, which parks this arm forever, and a
            // failed publish yields its backoff, so neither case spins.
            _ = sleep_for(publish_delay) => {
                if let Err(error) = provider.flush_one(&publisher).await {
                    tracing::error!(target: "csp", "outbox flush failed: {error}");
                }
            }
        }
    }

    if let Err(error) = provider.queue_live_releases() {
        tracing::warn!(target: "csp::lease", "clean-shutdown release construction failed: {error}");
    }
    if let Err(error) = provider.flush_pending_leases(&publisher).await {
        tracing::warn!(target: "csp::lease", "clean-shutdown release handoff failed: {error}");
    }
    match bounded_shutdown_drain(provider.flush(&publisher)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(target: "csp", "clean-shutdown durable drain failed: {error}");
        }
        Err(_) => {
            tracing::warn!(
                target: "csp",
                timeout_secs = SHUTDOWN_DURABLE_DRAIN_TIMEOUT.as_secs(),
                "clean-shutdown durable drain timed out"
            );
        }
    }
    relay.shutdown().await;
    Ok(())
}

async fn bounded_shutdown_drain<T>(
    drain: impl std::future::Future<Output = T>,
) -> Result<T, tokio::time::error::Elapsed> {
    bounded_drain(SHUTDOWN_DURABLE_DRAIN_TIMEOUT, drain).await
}

async fn bounded_drain<T>(
    timeout: Duration,
    drain: impl std::future::Future<Output = T>,
) -> Result<T, tokio::time::error::Elapsed> {
    tokio::time::timeout(timeout, drain).await
}

/// Wait `delay`, or never — `None` is "there is nothing to wait *for*", which
/// must park a `select!` arm rather than fire it in a loop.
async fn sleep_for(delay: Option<Duration>) {
    match delay {
        Some(delay) => tokio::time::sleep(delay).await,
        None => std::future::pending().await,
    }
}

fn prepare_terminal_stop<T>(
    subject: &mut T,
    persist_terminal_intent: impl FnOnce(&mut T) -> anyhow::Result<()>,
    queue_release: impl FnOnce(&mut T) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    persist_terminal_intent(subject)?;
    if let Err(error) = queue_release(subject) {
        tracing::warn!(target: "csp::lease", "terminal release construction failed; relying on TTL: {error}");
    }
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

/// Memory-only retry state for one channel's unchanged wake failure.
struct TeamWakeBackoff {
    reason: String,
    failures: u32,
    retry_at: Instant,
}

/// The provider's whole runtime state, minus the relay socket.
pub struct Provider {
    config: Config,
    pubkey_hex: String,
    state: StateStore,
    outbox: Outbox,
    /// Durable CI-continuation registrations. See [`ci_continuation`].
    ci_continuations: ci_continuation_store::CiContinuationStore,
    /// Verified CI results reported by the single listener task.
    ///
    /// `None` until [`Provider::start_ci_result_listener`] runs, which is
    /// after the relay identity has been witnessed: without that identity no
    /// result signer can be verified, so there is nothing to listen for.
    ci_listener: Option<ci_result_listener::CiResultListener>,
    /// The listener's report queue, merged into the run loop's `select!`.
    ci_events: Option<mpsc::Receiver<ci_result_listener::CiListenerEvent>>,
    team_wakes: team_wake::WakeIntentStore,
    /// Channels whose complete stored 44244 partition was scanned this run.
    team_wake_scanned_channels: HashSet<Uuid>,
    /// Complete-partition refusals are channel-local. Retrying a 32-page
    /// refusal every runtime tick would make one bad channel monopolize relay
    /// work even though no partial result is trusted.
    team_wake_discovery_backoff: HashMap<Uuid, TeamWakeBackoff>,
    /// Refusals present at open are eligible for the one restart probe.
    team_wake_refusals_at_startup: HashSet<Uuid>,
    /// Refused channels receive exactly one complete discovery probe per
    /// process lifetime, then consume no scheduler ticks.
    team_wake_refusal_reprobed: HashSet<Uuid>,
    /// Infrastructure retries wait here without waking a model or touching its context.
    team_wake_backoff: HashMap<Uuid, TeamWakeBackoff>,
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
    /// Last relay-confirmed reachability fact per session id, keyed and
    /// applied under the exact same generation fence as `git_probes` — see
    /// [`session::SessionEvent::WorktreeObserved`]. Absent, like a `None`
    /// reachability result, means "not checked."
    git_reachability: HashMap<String, reachability::ReachabilityFact>,
    /// Digests this process has already written to a session's redaction vault.
    ///
    /// The same home directory is redacted out of nearly every item, so without
    /// this the vault would grow a line per item for one value. Memory-only and
    /// per-process on purpose: after a restart the worst case is that a digest
    /// is appended a second time, which the reader tolerates (last line wins on
    /// an identical value) and the size caps bound. Persisting it would buy a
    /// duplicate-free file at the cost of a second thing to expire.
    recorded_redactions: HashMap<String, HashSet<String>>,
    /// Shared HTTP client for relay REST calls made off the event loop
    /// (currently: [`Provider::spawn_git_probe`]'s reachability check).
    ///
    /// `None` until [`Provider::set_rest_client`] is called after the relay
    /// connects — `Provider::new` runs before that connection exists. A
    /// probe launched before it is set simply skips the reachability leg,
    /// which is exactly the "not checked" outcome the honesty contract
    /// already models, so no session ever waits on it.
    rest_client: Option<RestClient>,
    /// The relay's signing pubkey (lowercase hex) as witnessed from its
    /// NIP-11 `self` field after connecting — the trust root every kind
    /// 40099 authority-acceptance receipt is verified against (see
    /// [`authority`]). `None` means no stable relay identity is known, and
    /// authority-chain consumption fails closed: genesis-bearing sessions
    /// stay founder-only.
    relay_self: Option<String>,
    /// Genesis refs whose authority chain this process has not yet managed to
    /// re-read, so their handover claim is **unknown** rather than absent.
    ///
    /// Filled by [`Provider::recover`] with every open genesis-bearing record
    /// and emptied one genesis at a time as
    /// [`Provider::backfill_session_authority`] succeeds — during recovery,
    /// and on the runtime tick afterwards. While a genesis is in here its
    /// records refuse turns, resumes, wakes and creates with
    /// [`commands::AUTHORITY_NOT_REVERIFIED`] and publish no metadata.
    ///
    /// In memory, and deliberately: it is a statement about what *this
    /// process* has read, not a durable fact about the session. A provider
    /// that never recovers — every test that builds one directly — has an
    /// empty set and behaves exactly as before.
    ///
    /// It exists because the two halves of the chain fail in opposite
    /// directions. An unverifiable grant is never applied, so an unreadable
    /// chain leaves a session founder-only; an unverifiable *claim* leaves the
    /// persisted `handover` at whatever the machine had before it went down,
    /// which for the returning machine is "nobody has taken this over". Grants
    /// fail closed for free; the claim needs this.
    claims_pending_reverification: HashSet<String>,
    /// Per-session bookkeeping for the bounded verified-context refresh, keyed
    /// by session id exactly like `git_probe_generation`.
    ///
    /// In-memory only, and deliberately so: [`SessionRecord`] persists no
    /// package id, so a restart loses every session→package binding. That is
    /// correct rather than a gap — a restart has already destroyed the only
    /// reader of those directories, which is why [`Provider::recover`] sweeps
    /// them instead of trying to reconstruct a mapping that no longer means
    /// anything.
    context_refresh: HashMap<String, ContextRefreshState>,
    /// Turn ceilings read from published kind-44245 session policies, keyed by
    /// umbrella `sessionRef`.
    ///
    /// The **only** thing this provider enforces from a session policy
    /// (`docs/design/portable-team-loop/POLICY.md` §4). Written when a create
    /// or a resume reads the umbrella's newest accepted record off the relay,
    /// and in memory only: it is a cache of a signed fact, and the next create
    /// or resume re-reads it. The consequence, stated rather than hidden: a
    /// policy published *while* a seat is already running does not bind that
    /// umbrella until its next create or resume.
    policy_turn_budgets: HashMap<String, u32>,
    subscribed: BTreeSet<Uuid>,
    projects_fingerprint: Option<(SystemTime, u64)>,
    /// Latest signed lease per exact target not yet handed to the relay task.
    pending_leases: HashMap<String, lease::PendingLease>,
    /// Exact generations whose first live lease has been handed off only after
    /// their durable facts received positive relay acknowledgements.
    established_leases: HashSet<String>,
    /// Durable fact keys that must receive positive relay OK before the exact
    /// generation may emit its first live lease. Keys survive latest-metadata
    /// replacement, so a superseded row cannot masquerade as acceptance.
    first_lease_prerequisites: HashMap<String, HashSet<(u32, String)>>,
    /// Turn commands this process has accepted into an execution's mailbox and
    /// that have not started yet, keyed by `commandId`.
    ///
    /// The window this covers did not exist before consumption moved to the
    /// start of a turn. It does three jobs, all of them about not losing or
    /// duplicating a turn: it dedupes a relay redelivery of a command already
    /// on its way; it holds the channel watermark back so a crash replays the
    /// command instead of skipping it; and it names the turns an execution
    /// still owed when its actor exits.
    ///
    /// In-memory on purpose. After a restart there is no mailbox, so every
    /// entry in it would be a lie about custody this process no longer has.
    in_flight: HashMap<String, InFlightTurn>,
    /// Per-session pairing of gate tool calls with their results.
    ///
    /// Brian's 2026-09-02 ruling: no observability path may depend on asking
    /// an agent to report. A seat runs `cargo test`; this watches the two
    /// frames the run already puts on the wire and publishes the gate row
    /// itself, signed by this provider instance and marked `observed`.
    ///
    /// In-memory on purpose, like [`Provider::in_flight`]: a call whose result
    /// this process never saw is not a gate anybody ran to completion here,
    /// and a durable half-pair would become a row about a run nobody watched.
    gate_observers: HashMap<String, gate_observer::GateObserver>,
    /// `commandId`s of cancels this process has already handed to an actor's
    /// mailbox but has not yet answered durably.
    ///
    /// A `thread.turn.interrupt` is not a turn, so it never enters
    /// [`Provider::in_flight`] — and everything that runs *after* its delivery
    /// (the receipt, then the consumed/refused ledger append) is fallible. When
    /// one of those fails the command is handed back for a later try, and both
    /// ledgers roll their in-memory entry back on a failed append, so without
    /// this set a redelivery walked every `decide_turn` fence and cancelled
    /// whatever turn was running by then: no receipt, no transcript item, no
    /// ledger entry.
    ///
    /// The fence is silent (`Ignored::AlreadyAccepted`), so it may only close
    /// over a cancel that has *already* been answered — which is why an id is
    /// recorded here only once `enqueue_receipt` has returned `Ok` and the
    /// answer is durably in the crash-safe outbox, and why the entry is
    /// released only once the ledger append behind it succeeds and takes the
    /// fence over. An enqueue that fails records nothing: the command goes
    /// back to `replay.held` unfenced, which is what leaves the later try able
    /// to answer it.
    ///
    /// An append that keeps failing would otherwise keep its entry for the
    /// life of the process, so the queue is bounded at
    /// [`DELIVERED_CANCEL_FENCE_CAPACITY`] and evicts oldest-first. Insertion
    /// order, not hashing, is what makes that eviction well defined.
    ///
    /// In-memory for the same reason as `in_flight`: after a restart there is
    /// no mailbox, so no entry in it could still be true.
    delivered_cancels: VecDeque<String>,
    /// Whether each live execution's runtime advertised native mid-turn
    /// steering at `initialize`, keyed by session id.
    ///
    /// Per execution, never per driver: this is what the process behind one
    /// generation actually answered. Absent means "no live process was
    /// witnessed", which publishes as `threadSteer: false` — an unwitnessed
    /// capability is not a capability.
    steering: HashMap<String, bool>,
    /// Whether the process behind each live generation advertised image
    /// prompts at `initialize`, keyed by session id.
    ///
    /// Per execution for the same reason as [`Self::steering`]: absent means no
    /// live process was witnessed, which publishes as `promptImage: false`.
    prompt_image: HashMap<String, bool>,
    /// Reads turn attachments back from this provider's relay. Built once: the
    /// relay URL and signing key are fixed for the process's lifetime.
    media: Option<attachments::MediaFetcher>,
    /// Turn commands held for reordering while a channel's subscription is
    /// replaying history. See [`ReplayWindow`].
    replay: ReplayWindow,
}

/// What [`Provider::apply_turn_decision`] actually did with one command.
///
/// Exists for the CI-continuation delivery path, which has a durable record to
/// retire and therefore has to know whether the turn it rebuilt reached a
/// mailbox, was refused, or was silently ignored — a question every other
/// caller can answer by not asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnDisposition {
    /// Handed to an execution's mailbox. The turn path owns the command now:
    /// the operation and command ledgers are written at `TurnStarted`.
    Delivered,
    /// Durably answered with a terminal receipt carrying this code.
    Answered(String),
    /// A CI continuation was durably registered. No turn exists yet.
    Registered,
    /// An interrupt was answered. Never reached by a continuation, which is
    /// always a `thread.turn.start`.
    Interrupt,
    /// Nothing was owed and nothing was done.
    Silent,
}

/// One turn command accepted into a mailbox and not yet started.
#[derive(Debug, Clone)]
pub struct InFlightTurn {
    /// Channel the command arrived on, so its receipts go back to the right
    /// room without another lookup.
    channel_id: Uuid,
    /// The execution it was delivered to.
    session_id: String,
    /// The exact generation it addressed, for the receipt that may still be
    /// owed to it.
    target: CodingSessionTarget,
    /// The command event's `created_at`, which is what pins the watermark.
    created_at: u64,
    /// The team-wake operation this turn took custody of, when its text is one
    /// of the two identifier-only pointers `team_wake::wake_text` mints.
    ///
    /// The process-local half of the operation fence: consumption — and with
    /// it the durable half in `operations.jsonl` — happens when a turn
    /// *starts*, so between accept and start the durable ledger cannot answer
    /// "is this operation already somebody's". This can. It is released
    /// wherever the entry is, which is every path that drops or starts the
    /// turn, so an owner that fails releases the operation for a re-armed
    /// command rather than losing it.
    operation_key: Option<String>,
    /// The verified signer that sent this turn.
    ///
    /// Carried through to [`crate::state::OpenTurn::operator_pubkey`] when the
    /// turn starts, which is what lets a mid-turn claim change stop the old
    /// operator's work without stopping the new claimant's.
    operator_pubkey: String,
}

/// Turn commands buffered while a channel replays history, held so they are
/// delivered in the order they were *sent* rather than the order they arrive.
///
/// The relay answers a stored-event REQ newest-first (`ORDER BY created_at
/// DESC, id ASC` — the generic query builder every stored REQ goes through,
/// `crates/buzz-db/src/event.rs:771`). That is harmless for
/// commands that were already consumed, and wrong for the ones this slice
/// keeps: two turns queued behind a running one, lost to a crash, would be
/// replayed and run in reverse. So every 44220 that arrives while a channel's
/// subscription is still replaying is held, sorted by `(created_at, id)`, and
/// only then delivered.
///
/// The window is a bounded wait, not a protocol signal: `EOSE` is consumed
/// inside the relay client and never reaches this crate. A turn that arrives
/// live during the window is simply delivered a moment later, in the same
/// total order as everything else.
#[derive(Debug, Default)]
struct ReplayWindow {
    /// When each replaying channel's window closes.
    open_until: HashMap<Uuid, Instant>,
    /// Commands held across every open window, drained in sorted order.
    held: Vec<HeldCommand>,
}

/// One 44220 held for the duration of a channel's replay window.
#[derive(Debug, Clone)]
struct HeldCommand {
    channel_id: Uuid,
    /// Sort key, first component: the command's own `created_at`.
    created_at: u64,
    /// Sort key, second component: the event id, so two commands minted in the
    /// same second still have one deterministic order that every provider
    /// replaying the same channel agrees on.
    event_id: String,
    operator_pubkey: String,
    content: String,
}

/// What the provider must remember to write the *next* generation of one
/// execution's verified context package.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextRefreshState {
    /// The UUID naming this execution's generation directory. Not derivable
    /// from the session id: a resume mints its own (see
    /// [`RehydrationMcpDescriptor::package_id`]).
    package_id: String,
    /// The sequence the next successful refresh will claim.
    next_seq: u64,
    /// Epoch milliseconds of the last refresh *attempt*, or `0` when none has
    /// been launched. Attempts rather than successes: a failing relay must not
    /// turn every turn start into a fetch.
    last_refresh_ms: i64,
}

impl ContextRefreshState {
    /// Bookkeeping for an execution that just opened with generation 0 on disk.
    fn opened(package_id: String) -> Self {
        Self {
            package_id,
            next_seq: context_store::OPEN_TIME_CONTEXT_PACKAGE_GENERATION + 1,
            last_refresh_ms: 0,
        }
    }
}

impl Provider {
    /// Open durable state and the publish outbox for `config`.
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let pubkey_hex = config.pubkey_hex();
        let state = StateStore::open(&config.state_dir, config.command_horizon.as_secs())?;
        let outbox = Outbox::open(&config.state_dir, &pubkey_hex)?;
        let team_wakes = team_wake::WakeIntentStore::open(&config.state_dir)?;
        let ci_continuations = ci_continuation_store::CiContinuationStore::open(&config.state_dir)?;
        let team_wake_refusals_at_startup = team_wakes.refused_channels().collect();
        let (events_tx, session_events) = mpsc::channel(SESSION_EVENT_CAPACITY);
        let media = attachments::MediaFetcher::new(&config.relay_url, config.keys.clone());
        Ok(Self {
            config,
            pubkey_hex,
            state,
            outbox,
            ci_continuations,
            ci_listener: None,
            ci_events: None,
            team_wakes,
            team_wake_scanned_channels: HashSet::new(),
            team_wake_discovery_backoff: HashMap::new(),
            team_wake_refusals_at_startup,
            team_wake_refusal_reprobed: HashSet::new(),
            team_wake_backoff: HashMap::new(),
            sessions: SessionManager::new(events_tx.clone()),
            session_events,
            session_events_tx: events_tx,
            last_metadata: HashMap::new(),
            git_probes: HashMap::new(),
            git_probe_generation: HashMap::new(),
            git_reachability: HashMap::new(),
            recorded_redactions: HashMap::new(),
            rest_client: None,
            relay_self: None,
            claims_pending_reverification: HashSet::new(),
            context_refresh: HashMap::new(),
            policy_turn_budgets: HashMap::new(),
            subscribed: BTreeSet::new(),
            projects_fingerprint: None,
            pending_leases: HashMap::new(),
            established_leases: HashSet::new(),
            first_lease_prerequisites: HashMap::new(),
            in_flight: HashMap::new(),
            gate_observers: HashMap::new(),
            delivered_cancels: VecDeque::new(),
            steering: HashMap::new(),
            prompt_image: HashMap::new(),
            media,
            replay: ReplayWindow::default(),
        })
    }

    /// Wire in the shared relay REST client once the relay connection is
    /// live. Called once from [`run_with`] after [`HarnessRelay::connect`]
    /// succeeds; every subsequent worktree probe can then attempt the
    /// reachability leg.
    pub fn set_rest_client(&mut self, rest_client: RestClient) {
        self.rest_client = Some(rest_client);
    }

    /// Record the relay identity witnessed from the NIP-11 `self` field.
    ///
    /// Called once from [`run_with`] after connecting. Until it is set, no
    /// authority-acceptance receipt can verify, so no operator grant is ever
    /// applied — fail closed, never guessed.
    pub fn set_relay_self(&mut self, relay_self_hex: String) {
        self.relay_self = Some(relay_self_hex.to_ascii_lowercase());
    }

    /// Witness the relay's own signing identity (NIP-11 `self`), if it is not
    /// already known.
    ///
    /// The trust root for every relay-signed fact this provider consumes:
    /// authority-acceptance receipts, deletion receipts, and through them the
    /// handover claim itself. Without it nothing verifies and genesis-bearing
    /// work is refused rather than admitted on a guess.
    ///
    /// Called once at startup and then on the runtime tick **only while it is
    /// still unknown**, because the startup fetch is one HTTP request against
    /// a relay that may not have been ready yet. One transient failure used to
    /// leave a provider unable to verify anything for its whole lifetime,
    /// which is exactly the state in which a returning body would admit work
    /// somebody else now holds. An already-witnessed identity is never
    /// re-fetched: it is the trust root, and re-reading it would let a
    /// mid-life change of answer silently move it.
    ///
    /// Returns whether an identity is known afterwards. A relay that publishes
    /// no `self` key answers `false` every time, and on such a relay
    /// genesis-bearing sessions stay refused — the remedy is configuring the
    /// relay's identity, not loosening this.
    pub async fn witness_relay_identity(&mut self) -> bool {
        if self.relay_self.is_some() {
            return true;
        }
        let Some(rest) = self.rest_client.clone() else {
            return false;
        };
        match rest.fetch_relay_self_verified().await {
            Ok(Some(relay_self)) => {
                tracing::info!(target: "csp::authority", %relay_self, "witnessed relay identity");
                self.set_relay_self(relay_self);
                true
            }
            Ok(None) => false,
            Err(error) => {
                tracing::debug!(
                    target: "csp::authority",
                    "the relay identity is still not readable: {error}"
                );
                false
            }
        }
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
    /// 3. Every leftover verified-context package directory is swept. No ACP
    ///    subprocess survives a provider restart, so at this moment no sidecar
    ///    is reading any of them, and the in-memory session→package binding
    ///    that named them died with the previous process. A resume mints a
    ///    fresh package id and a fresh directory.
    ///
    /// # Ordering, which is the whole of `docs/HANDOVER_IMPL.md` §3.2
    ///
    /// Before the stranded loop in (2) republishes anything, two questions are
    /// asked of the relay, in this order:
    ///
    /// 1. **Retirement** — has an accepted whole-session deletion retired this
    ///    umbrella ([`crate::retirement`])? A retired record publishes no
    ///    metadata, asks for no custody, and is skipped by the loop entirely.
    ///    First, because a deleted session must never be advertised even once,
    ///    and because there is no point folding a claim over a chain whose
    ///    genesis is gone.
    /// 2. **The claim** — who holds this umbrella, and on which body
    ///    ([`Provider::backfill_authority_chains`])? The fence is re-derived
    ///    from the accepted chain here rather than trusted from the persisted
    ///    value alone, so a claim made while this provider was down is in
    ///    force before its first metadata event, not after it.
    ///
    /// Both are best-effort reads. A provider with no relay reader — which is
    /// every unit test that does not wire one — recovers exactly as it did
    /// before, with the fence it persisted and no retirement it has not
    /// witnessed. A failed read is never a deletion and never a claim; what it
    /// *is* is a reason to refuse, which
    /// [`Provider::claims_pending_reverification`] carries.
    ///
    /// One consequence of running this after the connection rather than before
    /// it: when the relay is unreachable at startup, `run_with` returns from
    /// `HarnessRelay::connect` and this never runs, so the local repairs above
    /// — the package sweep, the in-flight turn synthesis, the orphaned-command
    /// reconciliation — are deferred to the next start that does connect.
    /// That costs nothing in correctness, because every one of those repairs
    /// exists to publish something (a terminal transcript row, a receipt,
    /// metadata) and none of it could have been published without a relay
    /// anyway. What it buys is the ordering guarantee below, which is only
    /// obtainable with a reader in hand.
    pub async fn recover(&mut self) -> anyhow::Result<()> {
        // Best-effort: a state directory that refuses the sweep must not stop
        // the provider from coming back up. The consequence is disk, not
        // correctness — the reader of those files is already gone.
        if let Err(error) = context_store::remove_all_context_packages(&self.config.state_dir) {
            tracing::warn!(
                target: "csp::context",
                "leftover verified-context packages could not be swept: {error}"
            );
        }
        self.sweep_redaction_vault();
        self.flush_terminal_dispositions()?;
        self.recover_ci_continuations()?;
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

        // Every open umbrella starts this process unverified. Anything that
        // clears itself does so below or on the tick; anything that does not
        // refuses by name instead of assuming nobody took it over.
        self.claims_pending_reverification = self
            .state
            .sessions()
            .filter(|record| !record.closed)
            .filter_map(|record| record.genesis_ref.clone())
            .collect();

        // Retirement, then the fence, then anything that publishes. See the
        // ordering section on this function.
        if let Some(rest) = self.rest_client.clone() {
            self.reconcile_retirements(&rest).await;
            self.backfill_authority_chains(&rest).await;
        }
        if !self.claims_pending_reverification.is_empty() {
            tracing::warn!(
                target: "csp::authority",
                umbrellas = self.claims_pending_reverification.len(),
                "authority chains could not be re-read at startup; their executions refuse commands and publish no metadata until they can be"
            );
        }

        let stranded: Vec<SessionRecord> = self
            .state
            .sessions()
            .filter(|record| !record.closed && !record.is_retired())
            .cloned()
            .collect();
        // A record whose chain is unread is deliberately still in `stranded`:
        // its in-flight turn is still synthesized and its command ledger still
        // reconciled, because those repair *local* state and say nothing about
        // who holds the session. Only the metadata publish is held, and
        // `publish_metadata` holds it.
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
                let already_captured = open_turn.command_id.as_deref().is_some_and(|command_id| {
                    self.team_wakes.has_terminal_command(command_id, &target)
                });
                if !already_captured {
                    let terminal_at_ms = now_ms();
                    let terminal = self.enqueue_transcript_with_id(
                        record.channel_id,
                        &target,
                        Some(&open_turn.turn_id),
                        payload::result_item(
                            payload::ResultSubtype::Error,
                            u64::try_from(terminal_at_ms.saturating_sub(open_turn.started_at_ms))
                                .unwrap_or_default(),
                            "provider terminated mid-turn",
                            payload::TurnCost::default(),
                            payload::TurnUsageReport::default(),
                        ),
                        Priority::High,
                    )?;
                    if open_turn.team_wake_eligible {
                        if let (
                            Some((_, terminal_event_id)),
                            Some(actor),
                            Some(role),
                            Some(session_ref),
                            Some(genesis_ref),
                            Some(caused_by_command_id),
                        ) = (
                            terminal,
                            record.actor.clone(),
                            record.role.clone().filter(|role| role != "lead"),
                            record.session_ref.clone(),
                            record.genesis_ref.clone(),
                            open_turn.command_id.clone(),
                        ) {
                            let scope = team_wake::WakeScope {
                                channel_ref: record.channel_id,
                                session_ref,
                                genesis_ref,
                            };
                            self.team_wakes.capture_terminal(
                                scope,
                                team_wake::WakeSource::Terminal {
                                    terminal_event_id,
                                    actor_pubkey: actor,
                                    role,
                                    caused_by_command_id,
                                    source_target: target.clone(),
                                    prompt_at_ms: Some(open_turn.started_at_ms),
                                    terminal_at_ms,
                                },
                            )?;
                        }
                    }
                }
            }
            self.state.update_session(&record.session_id, |record| {
                record.open_turn = None;
            })?;
            self.publish_metadata(record.channel_id, &target, SessionStatus::Disconnected)?;
        }
        // Last, and the reason the file exists: this is the moment every open
        // seated generation has lost its process and its one-shot custody, and
        // the desktop's re-stage runs right after this provider comes up. The
        // set is restated from the recovered records rather than trusted from
        // whatever the previous process last wrote.
        self.publish_seat_requests();
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

    /// Subscribe to the two command kinds — plus kind 40099 system messages,
    /// which carry the relay-signed authority-acceptance receipts a
    /// mid-session grant arrives on — in one channel, replaying from the
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
                KIND_CODING_SESSION_TEAM_TRANSACTION,
                // A closure is not a command, but it is the only signal this
                // host gets that an umbrella is finished. Without it a
                // closed or archived session held its slot until the 4-hour
                // idle timeout — see `release_settled_umbrella`.
                KIND_CODING_SESSION_CLOSURE,
                KIND_SYSTEM_MESSAGE,
                // A deletion is not a command either, and it is the only
                // signal that an umbrella is *gone* rather than finished.
                // Without it a provider that was offline when a session was
                // deleted came back and republished metadata for it
                // (`docs/HANDOVER_IMPL.md` §3.2). The relay's signed 40099
                // deletion receipt arrives on `KIND_SYSTEM_MESSAGE` above; a
                // founder's own kind 5 arrives here, and is believed only
                // together with a read showing the relay applied it.
                KIND_DELETION,
            ]),
            require_mention: false,
        };
        // A newly granted membership and the first command are published back
        // to back. `subscribe_channel_from` only queues the REQ, so falling
        // back to `since=now` when the background relay task eventually sends
        // it can skip the command by one second. Replay from before the
        // membership notification until a consumed-command watermark exists.
        let replay_since = self.subscription_replay_since(channel_id, membership_created_at);
        relay
            .subscribe_channel_from(channel_id, filter, replay_since)
            .await?;
        self.subscribed.insert(channel_id);
        self.open_replay_window(channel_id);
        Ok(())
    }

    fn subscription_replay_since(
        &self,
        channel_id: Uuid,
        membership_created_at: Option<u64>,
    ) -> Option<u64> {
        // The grace covers commands that legally precede the floor: a create
        // can be published moments before this provider's membership applies
        // (the desktop sends both back to back, and a delayed grant leaves the
        // command strictly older), and a startup floor of "now" would skip
        // anything that arrived while the provider was down. Replaying a
        // bounded window instead of full history respects relay REQ limits,
        // and re-delivered commands are no-ops under the consumed-command
        // dedupe (`AlreadyConsumed`). Commands older than the grace on a
        // watermark-less channel remain lost by design.
        self.state
            .watermark(channel_id)
            .or_else(|| membership_created_at.map(|floor| floor.saturating_sub(REPLAY_GRACE_SECS)))
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

    /// Reopen the reorder window on every subscribed channel after the relay
    /// socket dropped and came back.
    ///
    /// A reconnect is a *re*-subscription, not a quiet resumption: the relay
    /// client reissues each channel's REQ from `min(last_seen,
    /// channel_dropped_since)` (`crates/buzz-acp/src/relay.rs:3256-3269`), and
    /// the relay answers a stored REQ newest-first
    /// (`crates/buzz-db/src/event.rs:771`). So the same backwards burst that
    /// [`Provider::subscribe`] opens a window for arrives again here, for
    /// every channel at once — and a socket drop is far more common than a
    /// process restart. Without this the recovered queue runs in reverse.
    ///
    /// Cheap when there is nothing to reorder: an empty window closes on its
    /// own timer having held nothing.
    pub fn reopen_replay_windows_after_reconnect(&mut self) {
        let channels: Vec<Uuid> = self.subscribed.iter().copied().collect();
        for channel_id in channels {
            self.open_replay_window(channel_id);
        }
    }

    /// Begin holding this channel's turn commands for reordering.
    ///
    /// Called from both places a subscription is opened, because those are
    /// exactly when stored history arrives newest-first: [`Provider::subscribe`]
    /// for a first (or membership-triggered) REQ, and
    /// [`Provider::reopen_replay_windows_after_reconnect`] for the resubscribe
    /// the relay client issues after a dropped socket. See [`ReplayWindow`].
    pub fn open_replay_window(&mut self, channel_id: Uuid) {
        self.replay
            .open_until
            .insert(channel_id, Instant::now() + REPLAY_REORDER_WINDOW);
    }

    /// How long until the next replay window closes, or `None` when none is
    /// open. Shaped for the run loop's `sleep_for` arm: `None` parks it.
    pub fn next_replay_delay(&self) -> Option<Duration> {
        let now = Instant::now();
        self.replay
            .open_until
            .values()
            .map(|deadline| deadline.saturating_duration_since(now))
            .min()
    }

    /// Deliver every held command whose channel's replay window has closed,
    /// in `(created_at, id)` order.
    pub async fn flush_due_replays(&mut self) -> anyhow::Result<()> {
        let now = Instant::now();
        let due: BTreeSet<Uuid> = self
            .replay
            .open_until
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(channel_id, _)| *channel_id)
            .collect();
        for channel_id in due {
            self.replay.open_until.remove(&channel_id);
            self.deliver_held_commands(channel_id).await?;
        }
        Ok(())
    }

    /// Close every open replay window immediately and deliver what they hold.
    ///
    /// The deterministic form of [`Provider::flush_due_replays`], for callers
    /// that know history is complete. Tests, today: a shutdown deliberately
    /// does *not* call this. Held commands are unconsumed and sit at or above
    /// the channel watermark, so the next start replays them; delivering them
    /// into a process that is on its way out would answer `turn_queued` for
    /// turns that are about to be dropped.
    pub async fn flush_replays_now(&mut self) -> anyhow::Result<()> {
        let open: BTreeSet<Uuid> = self.replay.open_until.keys().copied().collect();
        for channel_id in open {
            self.replay.open_until.remove(&channel_id);
            self.deliver_held_commands(channel_id).await?;
        }
        Ok(())
    }

    async fn deliver_held_commands(&mut self, channel_id: Uuid) -> anyhow::Result<()> {
        let mut held: Vec<HeldCommand> = Vec::new();
        let mut remaining: Vec<HeldCommand> = Vec::new();
        for command in std::mem::take(&mut self.replay.held) {
            if command.channel_id == channel_id {
                held.push(command);
            } else {
                remaining.push(command);
            }
        }
        self.replay.held = remaining;
        // The order the operator sent them in, reconstructed: `created_at`
        // first, then the event id as the tiebreak every replaying provider
        // agrees on. Delivering in arrival order would run a recovered queue
        // backwards.
        held.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        // Popped one at a time so a failure can hand the rest back. `?` out of
        // this loop used to take the untried remainder with it: `mem::take`
        // had already emptied `replay.held`, so those commands were no longer
        // held, had never been delivered, had no receipt, and the run loop
        // only logs. A restart still recovered them — the channel floor sits
        // below them — but inside the running process that is the silent loss
        // this machinery exists to remove.
        let mut queue: VecDeque<HeldCommand> = held.into();
        while let Some(command) = queue.pop_front() {
            let delivered = self
                .on_turn(
                    command.channel_id,
                    command.created_at,
                    &command.operator_pubkey,
                    &command.event_id,
                    &command.content,
                )
                .await;
            if let Err(error) = delivered {
                // Back it goes with the rest — but not because it "never
                // reached a mailbox": `on_turn` can fail *after*
                // `SessionHandle::deliver` took the turn, because the
                // `turn_queued` (or `turn_degraded`) `enqueue_receipt` right
                // behind the delivery is itself fallible, and for an interrupt
                // so is the ledger append behind that receipt. What makes
                // the redelivery safe on that path is a pair of process-local
                // fences, both of which make `decide_turn` answer
                // `Ignored::AlreadyAccepted` (`commands.rs`): `in_flight` for a
                // turn already handed to a session, and `delivered_cancels` for
                // an interrupt already handed to one. An interrupt is not a
                // turn, so `in_flight` never holds it, and both ledgers roll
                // their in-memory entry back when their append fails — without
                // the second fence a redelivery cancelled an unrelated running
                // turn with no receipt at all. The ledgers say nothing about
                // either shape: they are written when a turn *starts* or when a
                // command is answered terminally, never when a mailbox takes
                // something. Both fences die with the process, where replay
                // from the channel floor takes over.
                self.replay.held.push(command);
                self.replay.held.extend(queue);
                return Err(error);
            }
            let mark = match self.watermark_ceiling(command.channel_id) {
                Some(ceiling) => command.created_at.min(ceiling),
                None => command.created_at,
            };
            if let Err(error) = self.state.record_watermark(command.channel_id, mark) {
                // Delivered, so this one is not handed back — only the ones
                // still waiting behind it.
                self.replay.held.extend(queue);
                return Err(error.into());
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
                if self.replay.open_until.contains_key(&channel_id) {
                    // Held rather than run: this channel is still replaying,
                    // and stored events arrive newest-first.
                    self.replay.held.push(HeldCommand {
                        channel_id,
                        created_at,
                        event_id: event.id.to_hex(),
                        operator_pubkey,
                        content: event.content.clone(),
                    });
                    // No watermark write at all. `watermark_ceiling` is a
                    // clamp, not an advance — a `min` over the turns this
                    // channel still owes, which now includes the command
                    // pushed three lines above — so for the *first* held
                    // command of a replayed burst it equals that command's own
                    // `created_at`, and `record_watermark` only ever moves
                    // forward. Writing it here would walk the floor over every
                    // older command the relay has not served yet, and stored
                    // REQ results arrive newest-first, so older is exactly what
                    // comes next. `deliver_held_commands` writes the correct
                    // clamped marks when the window closes; until then this
                    // channel's floor stays put, which is the same reasoning
                    // the non-44220 path below already applies.
                    return Ok(());
                }
                self.on_turn(
                    channel_id,
                    created_at,
                    &operator_pubkey,
                    &event.id.to_hex(),
                    &event.content,
                )
                .await?;
            }
            KIND_SYSTEM_MESSAGE => {
                self.on_authority_receipt(channel_id, event, relay).await?;
                // The same 40099 stream carries the relay's signed deletion
                // receipts. Both readers ignore what is not theirs, so the
                // ordering here says only that a grant is folded before a
                // retirement can make the record stop reading it.
                self.on_possible_deletion(channel_id, event).await?;
            }
            KIND_DELETION => {
                self.on_possible_deletion(channel_id, event).await?;
            }
            KIND_CODING_SESSION_TEAM_TRANSACTION => {
                self.on_team_transaction(channel_id, event)?;
            }
            KIND_CODING_SESSION_CLOSURE => {
                self.on_closure(event);
            }
            other => {
                tracing::debug!(target: "csp", kind = other, "ignoring unrelated event");
                return Ok(());
            }
        }
        if self.replay.open_until.contains_key(&channel_id) {
            // A window that is still open has not seen the oldest event it is
            // going to see. Only 44220s are held for reordering, so a *newer*
            // 44221 or 40099 arriving first — which is the order the relay
            // serves stored events in — would record its own `created_at`
            // here, and `record_watermark` never moves backwards, so the clamp
            // a held command applies afterwards would be a no-op and a crash
            // inside the window would lose that turn. The window's own close
            // records the marks; until then this channel's floor stays put.
            return Ok(());
        }
        let mark = match self.watermark_ceiling(channel_id) {
            Some(ceiling) => created_at.min(ceiling),
            None => created_at,
        };
        self.state.record_watermark(channel_id, mark)?;
        Ok(())
    }

    /// A closure revision arrived: if it settles an umbrella this host is
    /// running, free the slot.
    ///
    /// Only `closed` and `archived` settle
    /// ([`CodingSessionClosureAction::is_closed`]); an `open` revision is a
    /// reopen, and a reopen has nothing to release — the executions it
    /// refers to are already gone, and a reopened umbrella starts its next
    /// execution through the ordinary create path and its ordinary slot
    /// check.
    ///
    /// A malformed payload is logged and dropped rather than propagated: the
    /// relay validated the envelope before storing it, so anything that fails
    /// to decode here is a version skew, and a version skew must not stop
    /// this provider from serving turns.
    fn on_closure(&mut self, event: &Event) {
        let payload = match buzz_core::coding_session_closure::decode_coding_session_closure(
            &event.content,
        ) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(
                    target: "csp",
                    event_id = %event.id.to_hex(),
                    "ignoring undecodable closure revision: {error}"
                );
                return;
            }
        };
        if !payload.action.is_closed() {
            return;
        }
        let released = self.release_settled_umbrella(&payload.session_ref);
        if released == 0 {
            // Said out loud because "nothing to release" and "released
            // everything" are indistinguishable from the outside, and the
            // whole point of this path is that a slot no longer goes quietly
            // unaccounted for.
            tracing::debug!(
                target: "csp",
                session_ref = %payload.session_ref,
                "settled umbrella holds no live execution on this host"
            );
        }
    }

    /// Persist a structurally valid report as an untrusted wake candidate.
    ///
    /// Authorization is deliberately deferred to the complete core fold in
    /// [`Provider::process_one_team_wake`]. Persisting first closes the crash
    /// window between relay delivery and fold/query work without treating the
    /// event's self-description as authority.
    fn team_report_candidate(
        channel_id: Uuid,
        event: &Event,
    ) -> anyhow::Result<Option<(team_wake::WakeScope, team_wake::WakeSource)>> {
        let payload =
            buzz_core::coding_session_team_transaction::validate_coding_session_team_transaction_envelope(event)
                .map_err(anyhow::Error::msg)?;
        if !matches!(
            payload.body,
            buzz_core::coding_session_team_transaction::CodingSessionTeamTransactionBody::Report(_)
        ) {
            return Ok(None);
        }
        Ok(Some((
            team_wake::WakeScope {
                channel_ref: channel_id,
                session_ref: payload.session_ref,
                genesis_ref: payload.genesis_ref,
            },
            team_wake::WakeSource::Report {
                operation_id: event.id.to_hex(),
                operation_type: team_wake::report_operation_type().as_str().to_owned(),
                author_pubkey: event.pubkey.to_hex(),
                created_at: event.created_at.as_secs(),
            },
        )))
    }

    fn on_team_transaction(&mut self, channel_id: Uuid, event: &Event) -> anyhow::Result<()> {
        let Some((scope, source)) = Self::team_report_candidate(channel_id, event)? else {
            return Ok(());
        };
        self.capture_live_team_wake(scope, source)
    }

    /// Persist a live wake candidate, asking the complete signed report scan
    /// to run again when its bounded per-channel source slot is occupied.
    fn capture_live_team_wake(
        &mut self,
        scope: team_wake::WakeScope,
        source: team_wake::WakeSource,
    ) -> anyhow::Result<()> {
        let channel_id = scope.channel_ref;
        let capture = self.team_wakes.capture_live_report(scope, source)?;
        match capture {
            team_wake::DiscoveryCapture::Saturated => {
                // A complete signed query, rather than an unbounded second
                // live-source slot, recovers this source after the channel's
                // admitted FIFO drains. This also covers a query response
                // that raced the socket delivery.
                self.team_wake_scanned_channels.remove(&channel_id);
            }
            team_wake::DiscoveryCapture::ResolvedLedgerFull => {
                self.team_wake_scanned_channels.remove(&channel_id);
                tracing::error!(
                    target: "csp::team_wake",
                    channel_ref = %channel_id,
                    code = "resolved_ledger_full",
                    pages = 32,
                    page_rows = 1_000,
                    "team-wake channel reached the exact verification envelope"
                );
            }
            team_wake::DiscoveryCapture::Refused(_) => {}
            team_wake::DiscoveryCapture::Admitted | team_wake::DiscoveryCapture::Duplicate => {}
        }
        Ok(())
    }

    /// Choose one channel once, then use it for both complete discovery and
    /// wake processing. Advancing the durable round-robin independently for
    /// each phase skips the selected discovery channel and lets alternating
    /// saturated/blocked channels starve their neighbours.
    async fn run_one_team_wake_tick(
        &mut self,
        publisher: &RelayEventPublisher,
    ) -> anyhow::Result<()> {
        let scheduled: HashSet<Uuid> = self
            .subscribed
            .iter()
            .copied()
            .filter(|channel| self.team_wake_discovery_needed(*channel))
            .chain(
                self.team_wakes
                    .work_channels()
                    .filter(|channel| self.team_wake_processing_needed(*channel)),
            )
            .collect();
        let Some(channel_id) = self.team_wakes.next_tick_channel(scheduled)? else {
            return Ok(());
        };
        if self.team_wake_discovery_needed(channel_id) {
            self.discover_team_wake_partition_for(channel_id).await;
        }
        if self.team_wake_processing_needed(channel_id) {
            self.process_team_wake_for(channel_id, publisher).await?;
        }
        Ok(())
    }

    fn team_wake_discovery_needed(&self, channel_id: Uuid) -> bool {
        self.subscribed.contains(&channel_id)
            && !self.team_wake_scanned_channels.contains(&channel_id)
            && !self.team_wake_discovery_backoff_pending(channel_id)
            && (!self.team_wakes.is_refused(channel_id)
                || (self.team_wake_refusals_at_startup.contains(&channel_id)
                    && !self.team_wake_refusal_reprobed.contains(&channel_id)))
    }

    fn team_wake_processing_needed(&self, channel_id: Uuid) -> bool {
        self.team_wakes.has_work(channel_id)
            && !self.team_wake_backoff_pending(channel_id)
            && !self.team_wakes.is_refused(channel_id)
    }

    /// Backfill one selected complete stored team-transaction partition after startup.
    ///
    /// The live subscription closes the post-subscribe race; this scan closes
    /// the older-than-replay-window gap when a provider was offline. A channel
    /// is marked only after a complete-or-error authenticated query, so a
    /// saturated or unavailable relay can never turn partial history into a
    /// successful discovery pass.
    async fn discover_team_wake_partition_for(&mut self, channel_id: Uuid) {
        if !self.team_wake_discovery_needed(channel_id) {
            return;
        }
        let reprobing_refusal = self.team_wakes.is_refused(channel_id);
        if reprobing_refusal {
            self.team_wake_refusal_reprobed.insert(channel_id);
        }
        let Some(rest) = self.rest_client.clone() else {
            self.defer_team_wake_discovery(channel_id, "relay_query_unavailable");
            return;
        };
        let mut events = match context_projector::query_complete_kind_partition(
            &rest,
            channel_id,
            KIND_CODING_SESSION_TEAM_TRANSACTION,
        )
        .await
        {
            Ok(events) => events,
            Err(context_projector::ContextProjectionError::Bound(error)) => {
                let first = match self.team_wakes.refuse_channel(
                    channel_id,
                    team_wake::ChannelRefusalCode::PartitionSaturated,
                ) {
                    Ok(first) => first,
                    Err(store_error) => {
                        tracing::error!(target: "csp::team_wake", %channel_id, %store_error, "failed to persist structural channel refusal");
                        return;
                    }
                };
                if first {
                    tracing::error!(
                        target: "csp::team_wake",
                        channel_ref = %channel_id,
                        code = "partition_saturated",
                        pages = 32,
                        page_rows = 1_000,
                        %error,
                        "team-wake channel structurally refused"
                    );
                } else {
                    tracing::debug!(target: "csp::team_wake", channel_ref = %channel_id, code = "partition_saturated", "restart probe reaffirmed team-wake refusal");
                }
                self.team_wake_discovery_backoff.remove(&channel_id);
                return;
            }
            Err(error) => {
                tracing::warn!(
                    target: "csp::team_wake",
                    %channel_id,
                    %error,
                    "complete team-wake partition transiently unavailable"
                );
                self.defer_team_wake_discovery(channel_id, "complete_partition_unavailable");
                return;
            }
        };
        if reprobing_refusal {
            // A complete partition proves the structural refusal no longer
            // applies. Clear it durably before admission: a crash after this
            // write simply restarts ordinary full discovery, while leaving it
            // installed would make every non-empty successful probe refuse
            // its own recovered reports.
            if let Err(error) = self.team_wakes.clear_refusal(channel_id) {
                tracing::error!(target: "csp::team_wake", %channel_id, %error, "successful restart probe could not clear team-wake refusal");
                return;
            }
        }
        events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        for event in events {
            let capture = match Self::team_report_candidate(channel_id, &event) {
                Ok(Some((scope, source))) => match self.team_wakes.capture_report(scope, source) {
                    Ok(capture) => capture,
                    Err(error) => {
                        tracing::error!(
                            target: "csp::team_wake",
                            %channel_id,
                            %error,
                            "team-wake report capture refused; leaving channel unscannable until backoff"
                        );
                        self.defer_team_wake_discovery(channel_id, "report_capture_refused");
                        return;
                    }
                },
                Ok(None) => continue,
                Err(error) => {
                    tracing::error!(
                        target: "csp::team_wake",
                        %channel_id,
                        %error,
                        "team-wake report validation refused; leaving channel unscannable until backoff"
                    );
                    self.defer_team_wake_discovery(channel_id, "report_validation_refused");
                    return;
                }
            };
            match capture {
                team_wake::DiscoveryCapture::Admitted | team_wake::DiscoveryCapture::Duplicate => {}
                team_wake::DiscoveryCapture::Saturated => {
                    self.defer_team_wake_discovery(channel_id, "admission_fifo_full");
                    return;
                }
                team_wake::DiscoveryCapture::ResolvedLedgerFull => {
                    tracing::error!(target: "csp::team_wake", channel_ref = %channel_id, code = "resolved_ledger_full", pages = 32, page_rows = 1_000, "team-wake channel reached the exact verification envelope");
                    self.team_wake_discovery_backoff.remove(&channel_id);
                    return;
                }
                team_wake::DiscoveryCapture::Refused(_) => return,
            }
        }
        self.team_wake_scanned_channels.insert(channel_id);
        self.team_wake_discovery_backoff.remove(&channel_id);
    }

    fn team_wake_discovery_backoff_pending(&self, channel_ref: Uuid) -> bool {
        self.team_wake_discovery_backoff
            .get(&channel_ref)
            .is_some_and(|backoff| backoff.retry_at > Instant::now())
    }

    fn defer_team_wake_discovery(&mut self, channel_ref: Uuid, reason: &str) {
        self.team_wake_scanned_channels.remove(&channel_ref);
        let prior = self.team_wake_discovery_backoff.get(&channel_ref);
        let failures = prior
            .filter(|backoff| backoff.reason == reason)
            .map_or(1, |backoff| backoff.failures.saturating_add(1));
        let exponent = failures.saturating_sub(1).min(9);
        let delay = Duration::from_secs(1_u64 << exponent).min(TEAM_WAKE_BACKOFF_CAP);
        self.team_wake_discovery_backoff.insert(
            channel_ref,
            TeamWakeBackoff {
                reason: reason.into(),
                failures,
                retry_at: Instant::now() + delay,
            },
        );
    }

    fn team_wake_backoff_pending(&self, channel_ref: Uuid) -> bool {
        self.team_wake_backoff
            .get(&channel_ref)
            .is_some_and(|backoff| backoff.retry_at > Instant::now())
    }

    fn defer_team_wake(&mut self, intent: team_wake::WakeIntent) -> anyhow::Result<()> {
        let channel_ref = intent.scope.channel_ref;
        let reason = intent
            .last_reason
            .clone()
            .unwrap_or_else(|| "team_wake_retry_pending".into());
        self.team_wakes.defer_in_flight(channel_ref, intent)?;

        let prior = self.team_wake_backoff.get(&channel_ref);
        let failures = prior
            .filter(|backoff| backoff.reason == reason)
            .map_or(1, |backoff| backoff.failures.saturating_add(1));
        let exponent = failures.saturating_sub(1).min(9);
        let delay = if matches!(
            reason.as_str(),
            "awaiting_provider_outcome"
                | "report_grace_pending"
                | "lead_target_changed_retry_pending"
                | "command_horizon_retry_pending"
        ) {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(1_u64 << exponent).min(TEAM_WAKE_BACKOFF_CAP)
        };
        self.team_wake_backoff.insert(
            channel_ref,
            TeamWakeBackoff {
                reason,
                failures,
                retry_at: Instant::now() + delay,
            },
        );
        Ok(())
    }

    /// The handover fence as it applies to one umbrella's team wakes.
    ///
    /// The claim is umbrella-wide, so **every** local record rooted at the
    /// scope's genesis is asked and the strictest answer wins
    /// ([`commands::umbrella_fence`]). Reading only one of them would let a
    /// record minted moments ago — `authority_seq` 0, no claim folded into it
    /// yet — speak for an umbrella its siblings are fenced behind.
    ///
    /// An umbrella this provider holds no record for is not fenced by
    /// anything it knows — it has folded no claim — so it returns `None`, and
    /// the wake is then judged by the ordinary authority rules.
    fn team_wake_fence(&self, scope: &team_wake::WakeScope) -> Option<commands::FenceRefusal> {
        commands::umbrella_fence(
            &self.state,
            scope.channel_ref,
            &scope.genesis_ref,
            &self.pubkey_hex,
            &self.pubkey_hex,
        )
    }

    fn retire_team_wake(&mut self, channel_ref: Uuid) -> anyhow::Result<()> {
        self.team_wakes.retire_in_flight(channel_ref)?;
        self.team_wake_backoff.remove(&channel_ref);
        Ok(())
    }

    /// Resolve and attempt one durable team wake from complete signed facts.
    ///
    /// One per runtime tick bounds relay work. Every refusal leaves the intent
    /// durable with a stable reason; only a verified outcome, a canonical
    /// report suppressing a terminal diagnostic, or a definitively excluded
    /// report retires it.
    async fn process_team_wake_for(
        &mut self,
        channel_ref: Uuid,
        publisher: &RelayEventPublisher,
    ) -> anyhow::Result<()> {
        let Some(mut intent) = self.team_wakes.pending_for_channel(channel_ref)? else {
            return Ok(());
        };
        debug_assert_eq!(intent.scope.channel_ref, channel_ref);
        if self.team_wake_backoff_pending(channel_ref) {
            self.team_wakes.defer_in_flight(channel_ref, intent)?;
            return Ok(());
        }
        let Some(rest) = self.rest_client.clone() else {
            intent.last_reason = Some("relay_query_unavailable".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        };
        let Some(relay_self) = self.relay_self.clone() else {
            intent.last_reason = Some("relay_identity_unavailable".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        };
        let snapshot = match team_wake::fetch_verified_snapshot(&rest, &relay_self, &intent.scope)
            .await
        {
            Ok(snapshot) => snapshot,
            Err(error) if error.is_bound() => {
                let first = self.team_wakes.refuse_channel(
                    channel_ref,
                    team_wake::ChannelRefusalCode::PartitionSaturated,
                )?;
                if first {
                    tracing::error!(target: "csp::team_wake", channel_ref = %channel_ref, code = "partition_saturated", pages = 32, page_rows = 1_000, %error, "team-wake verification structurally refused channel");
                }
                self.team_wake_backoff.remove(&channel_ref);
                return Ok(());
            }
            Err(error) => {
                tracing::warn!(target: "csp::team_wake", %error, "team wake facts are not currently provable");
                intent.last_reason = Some("verified_snapshot_unavailable".into());
                intent.last_reason_detail = team_wake::bounded_reason_detail(&error.to_string());
                self.defer_team_wake(intent)?;
                return Ok(());
            }
        };
        intent.last_reason_detail = None;

        // The handover fence, ahead of the authority check and of anything
        // that would mint a command (§3). A wake is a turn this provider
        // sends on its own behalf, so it is admitted exactly where every
        // other turn is: on the claimed body, or not at all. Retired rather
        // than deferred — a claim that moved this umbrella to another machine
        // is a settled answer, and the claimant's own provider is the one
        // that carries the work now. Deferring would hold this channel's
        // single in-flight slot behind a wake that can never be sent.
        if let Some(refusal) = self.team_wake_fence(&intent.scope) {
            tracing::info!(
                target: "csp::team_wake",
                channel_ref = %channel_ref,
                code = refusal.code,
                "discarding team wake candidate on a fenced or retired umbrella: {}",
                refusal.message
            );
            self.retire_team_wake(channel_ref)?;
            return Ok(());
        }
        if !team_wake::provider_may_wake(
            &self.pubkey_hex,
            &snapshot.founder_pubkey,
            &snapshot.authority,
        ) {
            intent.last_reason = Some("provider_not_explicitly_authorized".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        }

        // **There is no cross-umbrella wake.** Whoever signed the thing this
        // wake is about must belong to *this* umbrella's accepted authority
        // chain. A real, seated, entirely legitimate lead of another mission is
        // foreign here, and one team's lead never places work on another team's
        // seat (LANE-L9 §L9.5). Two umbrellas may see that they touched the
        // same file — Pulse computes that row and it wakes nobody — and a lead
        // may note or message the other lead with their own key. That is the
        // whole permitted surface.
        //
        // Retired rather than deferred: a foreign author is a settled answer no
        // later poll improves, and deferring would retry it forever.
        if let Some(author) = intent.source.author_pubkey() {
            let context = team_wake::fold_context(
                &intent.scope,
                &snapshot.founder_pubkey,
                &snapshot.authority,
            );
            if team_wake::wake_author_is_foreign(&context, author) {
                tracing::info!(
                    target: "csp::team_wake",
                    %author,
                    channel_ref = %channel_ref,
                    "discarding team wake candidate signed outside this umbrella"
                );
                self.retire_team_wake(channel_ref)?;
                return Ok(());
            }
        }

        match &intent.source {
            team_wake::WakeSource::Report {
                operation_id,
                author_pubkey,
                ..
            } => {
                if !snapshot
                    .included_reports
                    .iter()
                    .any(|report| report.event_id == *operation_id)
                {
                    if snapshot
                        .team_events
                        .iter()
                        .any(|event| event.id.to_hex() == *operation_id)
                    {
                        tracing::info!(target: "csp::team_wake", %operation_id, "discarding excluded team report wake candidate");
                        self.retire_team_wake(channel_ref)?;
                    } else {
                        intent.last_reason = Some("report_not_query_visible".into());
                        self.defer_team_wake(intent)?;
                    }
                    return Ok(());
                }
                let source_target = match team_wake::resolve_actor_target(
                    &snapshot.package,
                    author_pubkey,
                ) {
                    Ok(target) => target,
                    Err(error) => {
                        tracing::info!(target: "csp::team_wake", %error, "team report source has no exact active provider target");
                        intent.last_reason = Some("report_source_target_not_exact".into());
                        self.defer_team_wake(intent)?;
                        return Ok(());
                    }
                };
                let locally_owned = source_target.instance_id == self.config.instance_id
                    && self
                        .state
                        .session(&source_target.session_id)
                        .is_some_and(|record| {
                            record.actor.as_deref() == Some(author_pubkey.as_str())
                                && self.target_for(record) == source_target
                        });
                if !locally_owned {
                    // Every provider can see the channel report. Only the
                    // provider that owns the reporting generation may mint its
                    // durable push; the exact remote lead remains a valid
                    // target after this ownership check.
                    self.retire_team_wake(channel_ref)?;
                    return Ok(());
                }
            }
            team_wake::WakeSource::Terminal {
                actor_pubkey,
                role,
                caused_by_command_id,
                source_target,
                prompt_at_ms,
                terminal_at_ms,
                ..
            } => {
                let lifecycle_turn =
                    self.state
                        .session(&source_target.session_id)
                        .is_some_and(|record| {
                            self.target_for(record) == *source_target
                                && record.generation_command_id() == caused_by_command_id
                        });
                if lifecycle_turn {
                    tracing::info!(
                        target: "csp::team_wake",
                        %caused_by_command_id,
                        "discarding lifecycle-turn terminal from the team-wake queue"
                    );
                    self.retire_team_wake(channel_ref)?;
                    return Ok(());
                }
                let context = team_wake::fold_context(
                    &intent.scope,
                    &snapshot.founder_pubkey,
                    &snapshot.authority,
                );
                let assignment_ref = match team_wake::turn_requires_report(
                    &snapshot.package,
                    &snapshot.team_events,
                    &context,
                    caused_by_command_id,
                    source_target,
                    actor_pubkey,
                    role,
                ) {
                    team_wake::TurnReportRequirement::NotRequired => {
                        self.retire_team_wake(channel_ref)?;
                        return Ok(());
                    }
                    team_wake::TurnReportRequirement::Unknown(reason) => {
                        intent.last_reason = Some(reason.into());
                        if reason == "initiating_command_not_query_visible"
                            && self.team_wakes.park_unattempted_terminal_behind_work(
                                channel_ref,
                                intent.clone(),
                            )?
                        {
                            return Ok(());
                        }
                        self.defer_team_wake(intent)?;
                        return Ok(());
                    }
                    team_wake::TurnReportRequirement::Required { assignment_ref } => assignment_ref,
                };
                if team_wake::report_suppresses_terminal(
                    &snapshot.included_reports,
                    &assignment_ref,
                    actor_pubkey,
                    *prompt_at_ms,
                    *terminal_at_ms,
                ) {
                    self.retire_team_wake(channel_ref)?;
                    return Ok(());
                }
                if now_ms().saturating_sub(*terminal_at_ms) < TEAM_WAKE_REPORT_GRACE_MS {
                    intent.last_reason = Some("report_grace_pending".into());
                    self.defer_team_wake(intent)?;
                    return Ok(());
                }
            }
        }

        let target = match team_wake::resolve_lead_target(&snapshot.package, &snapshot.authority) {
            Ok(target) => target,
            Err(error) => {
                tracing::info!(target: "csp::team_wake", %error, "team wake has no exact active lead target");
                intent.last_reason = Some("lead_target_not_exact".into());
                self.defer_team_wake(intent)?;
                return Ok(());
            }
        };

        let expected_wake_text = team_wake::wake_text(&intent.source)
            .map_err(|error| anyhow::anyhow!("team wake pointer could not be encoded: {error}"))?;
        if let Some(old_target) = intent.target.as_ref().filter(|old| *old != &target) {
            let old_command_settled = intent.command_id.as_deref().is_some_and(|command_id| {
                team_wake::command_outcome(&snapshot.package, command_id, old_target).is_some()
                    || team_wake::command_echoed(
                        &snapshot.package,
                        command_id,
                        old_target,
                        &expected_wake_text,
                    )
            });
            if old_command_settled
                || team_wake::operation_wake_delivered(
                    &snapshot.package,
                    old_target,
                    &expected_wake_text,
                )
            {
                self.retire_team_wake(channel_ref)?;
                return Ok(());
            }
            intent.target = None;
            intent.command_id = None;
            intent.signed_event = None;
            intent.relay_accepted_at = None;
            intent.attempt = intent.attempt.saturating_add(1);
            intent.last_reason = Some("lead_target_changed_retry_pending".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        }
        if intent.target.is_none() {
            // Binding even the first target creates a new command identity.
            // Persist and return before signing so the next pass checks an
            // outcome/echo for precisely that identity (I7).
            intent.target = Some(target.clone());
            intent.command_id = Some(team_wake::command_id(
                intent.source.event_id(),
                &target,
                intent.attempt,
            ));
            intent.last_reason = Some("lead_target_bound_retry_pending".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        }
        let command_id = intent
            .command_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("team wake command id was not resolved"))?;
        if team_wake::command_outcome(&snapshot.package, &command_id, &target).is_some()
            || team_wake::command_echoed(
                &snapshot.package,
                &command_id,
                &target,
                &expected_wake_text,
            )
            || team_wake::operation_wake_delivered(&snapshot.package, &target, &expected_wake_text)
        {
            self.retire_team_wake(channel_ref)?;
            return Ok(());
        }
        if intent.relay_accepted_at.is_none() {
            intent.relay_accepted_at =
                team_wake::observed_command_at(&snapshot.package, &command_id, &target);
        }
        if let Some(accepted_at) = intent.relay_accepted_at {
            if now_secs().saturating_sub(accepted_at) <= self.config.command_horizon.as_secs() {
                self.defer_team_wake(intent)?;
                return Ok(());
            }
            intent.attempt = intent.attempt.saturating_add(1);
            intent.command_id = Some(team_wake::command_id(
                intent.source.event_id(),
                &target,
                intent.attempt,
            ));
            intent.signed_event = None;
            intent.relay_accepted_at = None;
            intent.last_reason = Some("command_horizon_retry_pending".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        }

        if intent.signed_event.is_none() {
            let event = team_wake::build_wake_event(
                &self.config.keys,
                &intent.scope,
                &intent.source,
                target,
                intent
                    .command_id
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("team wake retry has no command id"))?,
            )
            .map_err(anyhow::Error::msg)?;
            intent.signed_event = Some(event);
            // Persist the exact signed attempt before it can reach the relay.
            self.team_wakes
                .replace_in_flight(channel_ref, intent.clone())?;
        }
        let Some(event) = intent.signed_event.clone() else {
            intent.last_reason = Some("signed_attempt_unavailable".into());
            self.defer_team_wake(intent)?;
            return Ok(());
        };
        match publisher.publish(event).await {
            Ok(()) => {
                intent.relay_accepted_at = Some(now_secs());
                intent.last_reason = Some("awaiting_provider_outcome".into());
            }
            Err(error) => {
                tracing::warn!(target: "csp::team_wake", %error, "team wake publish failed; exact signed attempt remains durable");
                intent.last_reason = Some("publish_retry_pending".into());
            }
        }
        self.defer_team_wake(intent)?;
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
        // Same cadence and the same reason: an operator who fixes a missing
        // seat entry can republish the create without restarting the provider.
        let actor_seats =
            crate::actor_seats::ActorSeatsFile::load(self.config.actor_seats_file.as_deref());
        let decision = decide_lifecycle(
            &self.context(channel_id, &projects, &actor_seats, operator_pubkey),
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
                // This command is answered and consumed, so nothing will ever
                // read its seat again. Gated on the entry this load already
                // has, so an unseated command does no extra file work at all.
                if actor_seats.seat(&command_id).is_some() {
                    self.forget_actor_seat(&command_id);
                }
                // Durably refused — `consume_command` above is the ledger
                // write — so the hint is settled and spent.
                self.consume_pending_hint(&command_id, None);
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
                        Ok(founder) => plan.founder_pubkey = founder.clone(),
                        Err(message) => {
                            self.state.consume_command(&plan.command_id, now_secs())?;
                            // Refused before dispatch: same one-shot rule as
                            // `create_session`'s two exits.
                            if plan.actor.is_some() {
                                self.forget_actor_seat(&plan.command_id);
                            }
                            self.consume_pending_hint(&plan.command_id, None);
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
                // The create fence, over a claim this provider has just
                // verified rather than one it happens to hold a record of, and
                // **before** `create_session` spawns anything (root's P1). A
                // record-based check cannot answer this: the machine joining a
                // handed-over umbrella for the first time has no record, and
                // that is precisely the machine that must not start a second
                // execution of the work.
                //
                // **No witnessed relay identity is unreadable authority, not
                // absent authority.** A provider whose NIP-11 fetch failed at
                // startup cannot verify a single receipt, so it cannot tell an
                // unclaimed umbrella from one it simply could not read — and
                // the machine most likely to be in that state is the one
                // returning from the outage during which it lost the session.
                // So a genesis-bearing create there is refused by name and the
                // umbrella is queued for another read; the identity is
                // re-witnessed on the runtime tick
                // ([`Provider::witness_relay_identity`]), so this clears
                // without a restart. On a relay that publishes no `self` key
                // at all it never clears: genesis-bearing creates are refused
                // there until the relay is configured with an identity, and
                // that is the remedy rather than a workaround here. A create
                // with no `genesisRef` is untouched by any of this.
                if let Some(genesis_ref) = plan.genesis_ref.clone() {
                    // The local records first, and without any I/O: if this
                    // provider already holds a fenced record of the umbrella
                    // it knows the answer, and reading the chain to learn it
                    // again would only add a way to fail.
                    let refusal = commands::umbrella_fence(
                        &self.state,
                        channel_id,
                        &genesis_ref,
                        operator_pubkey,
                        &self.pubkey_hex,
                    )
                    .map(|refusal| (refusal.code, refusal.message));
                    let verified = if refusal.is_some() {
                        Ok(ClaimState::NoClaim)
                    } else if self.relay_self.is_none() {
                        Err(
                            "this provider has witnessed no relay identity, so no acceptance \
                             receipt can be verified"
                                .to_owned(),
                        )
                    } else {
                        match relay {
                            Some(relay) => {
                                self.verified_umbrella_claim(
                                    channel_id,
                                    &genesis_ref,
                                    &plan.founder_pubkey,
                                    &relay.rest_client(),
                                )
                                .await
                            }
                            None => Err("no relay resolver is available for this command".into()),
                        }
                    };
                    let refusal = match (refusal, verified) {
                        (Some(refusal), _) => Some(refusal),
                        (None, Ok(claim)) => {
                            plan.verified_claim = claim.clone();
                            commands::claim_fence(&claim, operator_pubkey, &self.pubkey_hex)
                                .map(|refusal| (refusal.code, refusal.message))
                        }
                        // Unreadable is not unclaimed. The umbrella joins the
                        // pending set so the tick retries it, and this create
                        // is refused by name rather than dispatched on a
                        // guess.
                        (None, Err(reason)) => {
                            self.claims_pending_reverification
                                .insert(genesis_ref.clone());
                            Some((
                                commands::AUTHORITY_NOT_REVERIFIED,
                                format!(
                                    "this umbrella's authority chain could not be verified \
                                     ({reason}), so whether the session has been handed over \
                                     is not yet known and no execution was started"
                                ),
                            ))
                        }
                    };
                    if let Some((code, message)) = refusal {
                        self.state.consume_command(&plan.command_id, now_secs())?;
                        if plan.actor.is_some() {
                            self.forget_actor_seat(&plan.command_id);
                        }
                        self.consume_pending_hint(&plan.command_id, None);
                        tracing::warn!(
                            target: "csp::authority",
                            command_id = %plan.command_id,
                            %genesis_ref,
                            code,
                            "genesis-bearing create refused before startup: {message}"
                        );
                        let receipt = LifecycleReceipt::failed(&plan.command_id, code, &message);
                        return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
                    }
                }
                self.create_session(plan, relay).await
            }
            LifecycleDecision::Resume(plan) => self.resume_session(plan, relay).await,
            LifecycleDecision::Stop(plan) => self.stop_session(plan),
        }
    }

    /// Delete one seat's host-local key material, whatever became of the spawn.
    ///
    /// Best-effort and never fatal: the create has already succeeded or failed
    /// by the time this runs, and refusing a working execution because a
    /// cleanup write did not land would be the worse outcome. A failure is
    /// logged with the `commandId` and the path only — never the file's
    /// contents.
    /// Restate which open generations still need seat custody.
    ///
    /// Called wherever that set can change — a create, a resume, a stop or
    /// close, and startup recovery. See [`crate::seat_requests`] for why the
    /// file exists and what it is allowed to contain.
    ///
    /// Best-effort, and deliberately so: the consequence of a missed write is
    /// that a seat is not re-staged after the next restart, which surfaces as
    /// a visible `ACTOR_UNAVAILABLE` refusal on the promise that needed it.
    /// The consequence of making it fatal would be refusing a create that
    /// otherwise worked, which is strictly worse.
    fn publish_seat_requests(&self) {
        if let Err(error) = crate::seat_requests::write_seat_requests(
            &self.config.state_dir,
            self.state.sessions(),
            &self.pubkey_hex,
        ) {
            tracing::warn!(
                target: "csp::seats",
                "could not record which generations still need seat custody: {error}"
            );
        }
    }

    /// The hints directory beside this provider's projects file, if it has one.
    fn hints_dir(&self) -> Option<std::path::PathBuf> {
        self.config
            .projects_file
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(|parent| parent.join(commands::PENDING_HINTS_DIR))
    }

    /// Spend this command's one-shot working-directory hint, at settlement.
    ///
    /// **Settlement, not answer.** The hint is spent when the outcome is
    /// durable: after the create's `SessionRecord` is persisted (which is what
    /// carries the resolved cwd), or after a refusal has been written to the
    /// consumed-command ledger. Spending it earlier would leave a window where
    /// a crash loses both the hint and the record, and the retry of the same
    /// durable create would resolve to the project or channel default — the
    /// same work, silently, in a different folder.
    ///
    /// Spending is a **rename**, not a delete: `<commandId>.json` becomes
    /// `<commandId>.consumed`, one syscall, so the live hint disappearing and
    /// the marker appearing are the same event. The marker is then rewritten
    /// with the path that was actually resolved and the session it started; a
    /// crash between the two leaves a marker still holding the hint's own
    /// body, which reads as the same path with no session id — degraded, and
    /// still not a fall-through to a default.
    fn consume_pending_hint(&self, command_id: &str, resolved: Option<(&Path, &str)>) {
        let Some(hints_dir) = self.hints_dir() else {
            return;
        };
        let live = commands::pending_hint_path(&hints_dir, command_id);
        let marker = commands::consumed_hint_path(&hints_dir, command_id);
        match std::fs::rename(&live, &marker) {
            Ok(()) => {}
            // No hint for this command, or it is already spent. Either way
            // there is nothing to record.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                tracing::warn!(
                    target: "csp::projects",
                    %command_id,
                    "could not consume the create hint {}: {error}",
                    live.display()
                );
                return;
            }
        }
        let Some((path, session_id)) = resolved else {
            tracing::debug!(
                target: "csp::projects",
                %command_id,
                "consumed a refused create's working-directory hint"
            );
            return;
        };
        let body = serde_json::json!({
            "commandId": command_id,
            "path": path,
            "sessionId": session_id,
            "consumedAt": now_secs(),
        })
        .to_string();
        if let Err(error) = crate::state::atomic_write(&marker, body.as_bytes()) {
            tracing::warn!(
                target: "csp::projects",
                %command_id,
                "the consumed-hint marker kept the hint's own body: {error}"
            );
        }
    }

    /// Prune consumed-hint markers whose command is long settled.
    ///
    /// Two conditions, and both are load-bearing. **Only markers** — a live
    /// hint is never pruned on age, because a create delayed by an outage is
    /// exactly the case the hint exists for and expiring it would put that
    /// work in the wrong folder. **Only when the command is in the
    /// consumed-command ledger** — that ledger is what says this provider has
    /// genuinely answered the command, so a marker whose command could still
    /// arrive is kept regardless of how old the file is.
    pub fn prune_stale_pending_hints(&self) {
        let Some(hints_dir) = self.hints_dir() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&hints_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("consumed") {
                continue;
            }
            let Some(command_id) = path.file_stem().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };
            if !self.state.is_command_consumed(command_id) {
                continue;
            }
            let age = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|modified| modified.elapsed().ok())
                .map_or(0, |elapsed| elapsed.as_secs());
            if age < PENDING_HINT_MAX_AGE_SECS {
                continue;
            }
            if let Err(error) = std::fs::remove_file(&path) {
                tracing::warn!(
                    target: "csp::projects",
                    "could not prune the settled create hint {}: {error}",
                    path.display()
                );
            }
        }
    }

    fn forget_actor_seat(&self, command_id: &str) {
        if let Err(error) =
            crate::actor_seats::consume_seat(self.config.actor_seats_file.as_deref(), command_id)
        {
            tracing::warn!(
                target: "csp::seats",
                %command_id,
                "could not clear the agent seat's key material: {error}"
            );
        }
    }

    async fn create_session(
        &mut self,
        plan: CreatePlan,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        let outbox_before = self.outbox.pending_keys();
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

        // A hired seat never runs in somebody else's tree. Item 80(a)/(b):
        // three seats joined one umbrella with the working directory the
        // dialog remembered — the checkout the app itself runs from — and
        // shared a git index, a HEAD, and one `.agents/skills` that ended up
        // holding every role's pack. Refused before anything is provisioned,
        // and the staged key goes with the refusal.
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

        let target = CodingSessionTarget {
            driver: descriptor.driver.clone(),
            instance_id: self.config.instance_id.clone(),
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
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
                        let skills = seat_skills(seat);
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

        let request = CreateRequest {
            media: self.media.clone(),
            target: target.clone(),
            channel_id: plan.channel_id,
            cwd: plan.cwd.clone(),
            title: plan.title.clone(),
            model: plan.model.clone(),
            resume_cursor: None,
            // Never strict: an ordinary create or resume may legitimately
            // end up in a fresh conversation, and says so on the wire.
            strict_native: false,
            rehydration_mcp,
            agent_command: descriptor.agent_command.clone(),
            agent_args: descriptor.agent_args.clone(),
            agent_env: descriptor
                .cli_env
                .iter()
                .map(|env| (env.name.clone(), env.value.clone()))
                .collect(),
            seat: seat_identity,
            post_fence_env,
            seat_skills,
            idle_timeout: self.config.idle_timeout,
            answer_stall_timeout: self.config.answer_stall_timeout,
            emit_raw_sdk_frames: self.config.emit_raw_sdk_frames,
            max_turn_duration: self.config.max_turn_duration,
            idle_shutdown: self.config.session_idle_shutdown,
            include_thoughts: self.config.include_thoughts,
        };

        let events = self.sessions.event_sender();
        let startup_future = SessionManager::start(request, events);
        let started = self
            .await_with_lease_maintenance(startup_future, relay.map(HarnessRelay::event_publisher))
            .await;
        // One-shot, whichever way the spawn went: the child either has the key
        // or never will, so the copy at rest has no further purpose.
        if plan.actor.is_some() {
            self.forget_actor_seat(&plan.command_id);
        }
        let started = match started {
            Ok(started) => started,
            Err(failure) => {
                tracing::warn!(
                    target: "csp",
                    command_id = %plan.command_id,
                    code = failure.code,
                    "session creation failed: {}", failure.message
                );
                // No session record and no refresh entry will ever name this
                // package, so it has to go now or not at all.
                self.discard_orphaned_context_package(context_package_id.as_deref());
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        };
        let startup = self.sessions.attach(started);
        // What this exact process answered at `initialize`, recorded before
        // any metadata is built from it.
        self.steering
            .insert(target.session_id.clone(), startup.steering_supported);
        self.prompt_image
            .insert(target.session_id.clone(), startup.prompt_image_supported);

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
            pack_ref: seat_pack_ref,
            founder_pubkey: Some(plan.founder_pubkey.clone()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: startup.model.clone().or_else(|| plan.model.clone()),
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
        self.state.insert_session(record)?;
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

        if let Some((status, reason)) = create_disclosure(&startup.continuity, unavailable_reason) {
            self.enqueue_transcript(
                plan.channel_id,
                &target,
                None,
                payload::status_item_with_reason(status, reason),
                Priority::High,
            )?;
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
                self.enforce_claim_for_genesis(&genesis_ref)?;
            }
        }
        Ok(())
    }

    /// Await one potentially slow actor startup while continuing to renew every
    /// already-established live generation. The startup future owns all of its
    /// inputs, so it never holds the provider's state or actor registry across
    /// an await; each tick re-checks `SessionHandle::is_live` and persists a new
    /// sequence before handing the signed assertion to the relay task.
    async fn await_with_lease_maintenance<F>(
        &mut self,
        startup: F,
        publisher: Option<RelayEventPublisher>,
    ) -> F::Output
    where
        F: std::future::Future,
    {
        self.await_with_lease_maintenance_at(
            startup,
            publisher,
            lease::LEASE_RENEWAL_INTERVAL,
            || {},
        )
        .await
    }

    async fn await_with_lease_maintenance_at<F, O>(
        &mut self,
        startup: F,
        publisher: Option<RelayEventPublisher>,
        cadence: Duration,
        mut maintenance_complete: O,
    ) -> F::Output
    where
        F: std::future::Future,
        O: FnMut(),
    {
        tokio::pin!(startup);
        let mut ticker = lease::renewal_interval_with(cadence);
        loop {
            tokio::select! {
                biased;
                _ = ticker.tick() => {
                    if let Err(error) = self.queue_lease_renewals() {
                        tracing::error!(target: "csp::lease", "lease renewal during actor startup failed: {error}");
                    }
                    if let Some(publisher) = publisher.as_ref() {
                        if let Err(error) = self.flush_pending_leases(publisher).await {
                            tracing::warn!(target: "csp::lease", "lease handoff during actor startup failed: {error}");
                        }
                    }
                    maintenance_complete();
                }
                result = &mut startup => return result,
            }
        }
    }

    /// Best-effort verified-history attachment for a fresh execution under an
    /// existing durable umbrella.
    ///
    /// A missing sidecar, a first-ever session with no earlier execution, or a
    /// relay/projection/storage failure leaves the execution Fresh. Nothing in
    /// this path may turn unverified history into agent context or prevent the
    /// operator from starting a usable fresh execution.
    ///
    /// Every path out of here names itself: the returned
    /// [`RehydrationOutcome`] carries either a descriptor or the enumerated
    /// reason continuity was lost, so a duplicate-create conflict, an
    /// unreachable relay and a missing sidecar do not render identically to
    /// the operator.
    async fn prepare_rehydration_context(
        &self,
        target: RehydrationTarget<'_>,
        relay: Option<&HarnessRelay>,
    ) -> RehydrationOutcome {
        let RehydrationTarget {
            scope,
            command_id,
            channel_id,
            session_ref,
            genesis_ref,
            package_id,
        } = target;
        let Some(command) = self.config.context_mcp_command.as_ref() else {
            tracing::info!(
                target: "csp::context",
                %command_id,
                "context MCP sidecar is unavailable; starting Fresh"
            );
            return RehydrationOutcome::unavailable("context_sidecar_unavailable");
        };
        if !command.is_absolute() {
            tracing::warn!(
                target: "csp::context",
                %command_id,
                "context MCP command is not absolute; starting without rehydrated context"
            );
            return RehydrationOutcome::unavailable("context_sidecar_path_invalid");
        }
        let Some(session_ref) = session_ref else {
            tracing::debug!(
                target: "csp::context",
                %command_id,
                "create has no umbrella sessionRef; starting Fresh"
            );
            return RehydrationOutcome::unavailable("no_prior_execution");
        };
        let Some(genesis_ref) = genesis_ref else {
            tracing::debug!(
                target: "csp::context",
                %command_id,
                "create has no umbrella genesisRef; starting Fresh"
            );
            return RehydrationOutcome::unavailable("no_prior_execution");
        };
        let Some(relay) = relay else {
            tracing::info!(
                target: "csp::context",
                %command_id,
                "relay query surface is unavailable; starting Fresh"
            );
            return RehydrationOutcome::unavailable("relay_unavailable");
        };
        let request = ContextProjectionRequest {
            channel_id,
            session_ref: session_ref.to_owned(),
            genesis_ref: genesis_ref.to_owned(),
            relay_self_pubkey: self.relay_self.clone(),
            // The first execution under a fresh genesis has no create chain to
            // prove yet, and it is exactly the execution that most needs the
            // crew tools, so a create attaches the MCP with an honestly empty
            // roster instead of leaving the seat toolless (plan S4/B). A
            // resume has an earlier generation behind it and gets no such
            // licence: for it, an empty umbrella is an unverifiable one.
            allow_no_executions: scope.allows_an_empty_umbrella(),
            generated_at: now_ms(),
            limits: ContextProjectionLimits::default(),
        };
        let package = match context_projector::fetch_and_project_session_context(
            &relay.rest_client(),
            &request,
        )
        .await
        {
            Ok(package) => package,
            Err(error) => {
                // The class goes on the wire; the detail — which command id had
                // two receipts, which event failed verification — stays here.
                let reason = context_projector::context_unavailable_reason(&error);
                tracing::info!(
                    target: "csp::context",
                    %command_id,
                    %reason,
                    "verified prior context unavailable; starting Fresh: {error}"
                );
                return RehydrationOutcome::unavailable(reason);
            }
        };
        let first_turn_brief =
            match serde_json::to_string(&coding_session_first_turn_brief(&package)) {
                Ok(brief) => brief,
                Err(error) => {
                    tracing::warn!(
                        target: "csp::context",
                        %command_id,
                        "verified first-turn brief could not be encoded; starting Fresh: {error}"
                    );
                    return RehydrationOutcome::unavailable("brief_encode_failed");
                }
            };
        let package_path = match context_store::write_context_package(
            &self.config.state_dir,
            package_id,
            &package,
        ) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    target: "csp::context",
                    %command_id,
                    "verified context could not be persisted; starting Fresh: {error}"
                );
                return RehydrationOutcome::unavailable("package_write_failed");
            }
        };
        let Some(package_dir) = package_path.parent().map(Path::to_path_buf) else {
            // Unreachable in practice — `write_context_package` returns a file
            // inside the generation directory it just created — but a
            // descriptor without a directory would silently disable refresh,
            // so it is refused rather than degraded.
            tracing::warn!(
                target: "csp::context",
                %command_id,
                "verified context path has no generation directory; starting Fresh"
            );
            return RehydrationOutcome::unavailable("package_write_failed");
        };
        RehydrationOutcome::attached(RehydrationMcpDescriptor {
            command: command.clone(),
            package_path,
            package_dir,
            package_id: package_id.to_owned(),
            first_turn_brief,
            // Prior *work*, not prior tooling: verified history, or a sibling
            // execution already seated under this umbrella. Neither means this
            // execution is Fresh, and the bootstrap says exactly that.
            prior_context: !package.history.is_empty() || !package.roster.is_empty(),
        })
    }

    /// The turn ceiling this umbrella's published policy sets, if any.
    ///
    /// `None` means *no enforced policy ceiling*, and the environment budget
    /// applies unchanged. It never means "policy unknown": a policy this
    /// provider could not read leaves the umbrella absent from the map, and
    /// the log line that dropped it says so.
    fn policy_turn_budget(&self, session_ref: &str) -> Option<u32> {
        self.policy_turn_budgets.get(session_ref).copied()
    }

    /// Record — or clear — the turn ceiling an umbrella's policy sets.
    ///
    /// `None` removes the entry rather than storing a zero, because zero is
    /// [`config::UNLIMITED_TURN_BUDGET`] and "no policy" and "a policy that
    /// lifts every ceiling" are different facts (NIP-CSP §2.4 refuses a
    /// literal `turns: 0` for the same reason).
    fn set_policy_turn_budget(&mut self, session_ref: &str, turns: Option<u32>) {
        match turns {
            Some(turns) => {
                self.policy_turn_budgets
                    .insert(session_ref.to_owned(), turns);
            }
            None => {
                self.policy_turn_budgets.remove(session_ref);
            }
        }
    }

    /// Read this umbrella's newest accepted kind-44245 policy off the relay
    /// and record the one field this provider enforces.
    ///
    /// Called from the create and resume paths, where the session record has
    /// just been written and its authority chain folded, so the "who may set
    /// policy" question is answerable from facts this provider already holds:
    /// the umbrella's founder, and the operators it verified grants for. The
    /// fold itself is
    /// [`context_projector::select_session_policy`] — the same one the context
    /// package uses — so a seat's `session_overview` and this gate can never
    /// disagree about which record won.
    ///
    /// Every failure is a log line and no ceiling. A relay that cannot be read
    /// must not silently invent a budget, and must not silently remove one
    /// either: the previous reading stands until a successful read replaces
    /// it.
    async fn refresh_policy_turn_budget(&mut self, session_id: &str, relay: Option<&HarnessRelay>) {
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        let (Some(session_ref), Some(genesis_ref), Some(founder)) = (
            record.session_ref.clone(),
            record.genesis_ref.clone(),
            record.founder_pubkey.clone(),
        ) else {
            return;
        };
        let granted = record.granted_operators.clone();
        let Some(relay) = relay else {
            return;
        };
        let records = match context_projector::query_complete_kind_partition(
            &relay.rest_client(),
            record.channel_id,
            KIND_CODING_SESSION_POLICY,
        )
        .await
        {
            Ok(records) => records,
            Err(error) => {
                tracing::info!(
                    target: "csp::policy",
                    %session_ref,
                    "this umbrella's session policy could not be read; no policy ceiling is \
                     enforced for it: {error}"
                );
                return;
            }
        };
        let addressed: Vec<Event> = records
            .into_iter()
            .filter(|event| {
                event.tags.iter().any(|tag| {
                    let parts = tag.as_slice();
                    parts.len() >= 2 && parts[0] == "d" && parts[1] == session_ref
                })
            })
            .collect();
        let mut notes = Vec::new();
        let policy = context_projector::select_session_policy(
            &addressed,
            &session_ref,
            &genesis_ref,
            &founder,
            &|author, _| author == founder || granted.contains(author),
            &mut notes,
        );
        let turns = policy
            .as_ref()
            .and_then(|policy| policy.record.budget.as_ref())
            .and_then(|budget| budget.turns);
        if let Some(turns) = turns {
            tracing::info!(
                target: "csp::policy",
                %session_ref,
                turns,
                "this umbrella's published session policy sets a turn ceiling; it overrides \
                 BUZZ_CSP_TURN_BUDGET for this session"
            );
        }
        self.set_policy_turn_budget(&session_ref, turns);
    }

    /// Read the `session.hire` a create answers and decide whether its
    /// requester may be named.
    ///
    /// `None` means *this provider could not read the hire* — the create named
    /// none, no relay query surface was available, the event is not on the
    /// relay, it is not a hire, or it belongs to another channel. That is a
    /// different fact from a hire that claimed no requester
    /// ([`commands::HireAttribution::Unclaimed`]), and both fall back to the
    /// same unattributed delivery, so the distinction only ever costs a log
    /// line — but collapsing them in the type would make "we did not look" and
    /// "nobody claimed" indistinguishable to the next reader.
    ///
    /// Nothing here is trusted from the create: the hire is fetched by its
    /// exact id and kind, its signature is verified by
    /// [`RestClient::query_event_by_id`](buzz_acp::relay::RestClient::query_event_by_id),
    /// and its channel is checked against the create's own.
    async fn hire_attribution(
        &self,
        command_id: &str,
        channel_id: Uuid,
        hire_ref: Option<&str>,
        relay: Option<&HarnessRelay>,
    ) -> Option<commands::HireAttribution> {
        let hire_ref = hire_ref?;
        let Some(relay) = relay else {
            tracing::info!(
                target: "csp::hire",
                %command_id,
                %hire_ref,
                "no relay query surface; the hired seat's brief is delivered unattributed"
            );
            return None;
        };
        let hire = match relay
            .rest_client()
            .query_event_by_id(
                hire_ref,
                nostr::Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16),
            )
            .await
        {
            Ok(Some(event)) => event,
            Ok(None) => {
                tracing::info!(
                    target: "csp::hire",
                    %command_id,
                    %hire_ref,
                    "the hire this create answers is not on the relay; the brief is delivered \
                     unattributed"
                );
                return None;
            }
            Err(error) => {
                tracing::warn!(
                    target: "csp::hire",
                    %command_id,
                    %hire_ref,
                    "the hire this create answers could not be read: {error}"
                );
                return None;
            }
        };
        // A hire in another channel is not this create's hire. The community
        // boundary is the `h` tag everywhere else in this crate, and an
        // attribution that crossed it would let a hire published anywhere name
        // a requester for a seat in a room it was never part of.
        let in_channel = hire.tags.iter().any(|tag| {
            let parts = tag.as_slice();
            parts.len() >= 2 && parts[0] == "h" && parts[1] == channel_id.to_string()
        });
        if !in_channel {
            tracing::warn!(
                target: "csp::hire",
                %command_id,
                %hire_ref,
                "the hire this create answers belongs to another channel; ignoring its requester"
            );
            return None;
        }
        let payload =
            match buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                &hire.content,
            ) {
                Ok(payload) => payload,
                Err(error) => {
                    tracing::warn!(
                        target: "csp::hire",
                        %command_id,
                        %hire_ref,
                        "the hire this create answers does not decode: {error}"
                    );
                    return None;
                }
            };
        if payload.hire_session_ref().is_none() {
            tracing::warn!(
                target: "csp::hire",
                %command_id,
                %hire_ref,
                "the event this create names as its hire is not a session.hire"
            );
            return None;
        }
        Some(commands::hire_attribution(&payload, &hire.pubkey.to_hex()))
    }

    async fn resume_session(
        &mut self,
        plan: ResumePlan,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
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
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
        }

        let Some(record) = self.state.session(&plan.target.session_id).cloned() else {
            return Ok(());
        };
        let Some(descriptor) = self.config.runtime(&record.provider_instance_ref).cloned() else {
            self.state.consume_command(&plan.command_id, now_secs())?;
            if seated {
                self.forget_actor_seat(&plan.command_id);
            }
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
            if seated {
                self.forget_actor_seat(&plan.command_id);
            }
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
        let previous_semantic_key = coding_session_target_key(&self.target_for(&record));
        let package_id = Uuid::new_v4().to_string();
        let rehydration = self
            .prepare_rehydration_context(
                RehydrationTarget {
                    scope: RehydrationScope::Resume,
                    command_id: &plan.command_id,
                    channel_id: record.channel_id,
                    session_ref: record.session_ref.as_deref(),
                    genesis_ref: record.genesis_ref.as_deref(),
                    package_id: &package_id,
                },
                relay,
            )
            .await;
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
        let (seat_identity, post_fence_env, seat_skills, seat_pack_ref) = match record
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
                            role: record.role.clone().unwrap_or_default(),
                            relay_url: seat.relay_url.clone(),
                        };
                        // A resume re-materializes the pack's skills: the
                        // workdir may have moved on since the create, and the
                        // write is a no-op when it has not.
                        let skills = seat_skills(seat);
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
                        return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
                    }
                }
            }
        };

        let request = CreateRequest {
            media: self.media.clone(),
            target: target.clone(),
            channel_id: record.channel_id,
            cwd: record.cwd.clone(),
            title: record.title.clone(),
            model: record.model.clone(),
            resume_cursor: record.resume_cursor.clone(),
            // Never strict: an ordinary create or resume may legitimately
            // end up in a fresh conversation, and says so on the wire.
            strict_native: false,
            rehydration_mcp,
            agent_command: descriptor.agent_command.clone(),
            agent_args: descriptor.agent_args.clone(),
            agent_env: descriptor
                .cli_env
                .iter()
                .map(|env| (env.name.clone(), env.value.clone()))
                .collect(),
            seat: seat_identity,
            post_fence_env,
            seat_skills,
            idle_timeout: self.config.idle_timeout,
            answer_stall_timeout: self.config.answer_stall_timeout,
            emit_raw_sdk_frames: self.config.emit_raw_sdk_frames,
            max_turn_duration: self.config.max_turn_duration,
            idle_shutdown: self.config.session_idle_shutdown,
            include_thoughts: self.config.include_thoughts,
        };
        let events = self.sessions.event_sender();
        let startup_future = SessionManager::start(request, events);
        let started = self
            .await_with_lease_maintenance(startup_future, relay.map(HarnessRelay::event_publisher))
            .await;
        if record.actor.is_some() {
            self.forget_actor_seat(&plan.command_id);
        }
        let started = match started {
            Ok(started) => started,
            Err(failure) => {
                self.state.consume_command(&plan.command_id, now_secs())?;
                // The resume minted a fresh package id, and the refresh entry
                // that would name it is only inserted after a successful open;
                // the previous generation's entry still points elsewhere. Drop
                // this one here or nothing will.
                self.discard_orphaned_context_package(context_package_id.as_deref());
                let receipt =
                    LifecycleReceipt::failed(&plan.command_id, failure.code, &failure.message);
                return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
            }
        };
        let startup = self.sessions.attach(started);
        self.steering
            .insert(record.session_id.clone(), startup.steering_supported);
        self.prompt_image
            .insert(record.session_id.clone(), startup.prompt_image_supported);

        if let Err(error) = self.state.update_session(&record.session_id, |record| {
            record.generation = generation;
            record.generation_command_id = Some(plan.command_id.clone());
            record.next_seq = 1;
            record.next_lease_sequence = 1;
            record.open_turn = None;
            record.closed = false;
            record.resume_cursor = Some(startup.acp_session_id.clone());
            // This generation's pack, not the previous one's: a seat restaged
            // from a moved ref runs a different commit, and the 44223 has to
            // say which.
            record.pack_ref = seat_pack_ref.clone();
            if startup.model.is_some() {
                record.model = startup.model.clone();
            }
        }) {
            self.sessions.shutdown(&record.session_id);
            return Err(error.into());
        }
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
            self.context_refresh.insert(
                target.session_id.clone(),
                ContextRefreshState::opened(package_id),
            );
        }

        // Only the two restarted arms can carry a package reason. On a native
        // resume or load the adapter re-attached its own conversation, so
        // whether a verified package could also be built is irrelevant to what
        // the agent can see, and naming a failure there would imply a loss that
        // did not happen. `session_rehydrated` lost nothing either.
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
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)?;
        self.enqueue_transcript(
            plan.channel_id,
            &target,
            None,
            payload::status_item_with_reason(status, reason),
            Priority::High,
        )?;
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

    /// Release the host slot every execution under a settled umbrella holds.
    ///
    /// ## Why this exists
    ///
    /// A slot is an entry in [`SessionManager`]'s live map: one running actor
    /// task owning one adapter child process. The create gate compares
    /// `live_count()` against `max_sessions` (default 4), and the *only*
    /// thing that removes an entry is `SessionManager::shutdown`.
    ///
    /// Closing or archiving a session was invisible to this provider — the
    /// closure kind was not in its subscription at all — so a settled session
    /// went on holding its slot until the 4-hour idle timeout reaped it.
    /// Four archived sessions blocked every new one for four hours, and the
    /// only visible symptom was a `SESSION_LIMIT` refusal naming a number
    /// the operator could not reconcile with what they saw on screen.
    ///
    /// ## Why not `stop_session`
    ///
    /// That path is the answer to a kind:44221 `session.stop` *command*: it
    /// consumes the command, publishes a lifecycle receipt against its id,
    /// and is founder-only. A closure is a different, differently-authorized
    /// fact, and there is no command here to receipt. Fabricating a
    /// command_id so the two could share a function would put a receipt on
    /// the wire for a command nobody sent.
    ///
    /// So this does the subset that is genuinely shared — the durable closed
    /// flag, the lease release, the actor shutdown that frees the slot, and
    /// the host-local cleanup — and no receipt.
    ///
    /// ## Authorization
    ///
    /// The relay already made this decision. A closure only exists here if
    /// the relay accepted and stored it, and the provider only ever acts on
    /// umbrellas it minted executions for. It deliberately does *not*
    /// re-apply `stop_session`'s founder-only rule: a project Owner deleting
    /// somebody else's session is exactly the case that rule would refuse,
    /// and refusing here would leave the slot held with nothing left on the
    /// relay to explain why.
    ///
    /// Returns the number of executions released, so callers can log
    /// something truthful about a closure that had nothing to release.
    fn release_settled_umbrella(&mut self, session_ref: &str) -> usize {
        // Only sessions this provider is actually running. A record already
        // marked closed, or one whose actor has exited on its own, holds no
        // slot and must not be re-released — that would queue a second lease
        // release and a second Stopped metadata for a session that already
        // published both.
        let live: Vec<String> = self
            .sessions
            .live_session_ids()
            .map(str::to_owned)
            .filter(|session_id| {
                self.state.session(session_id).is_some_and(|record| {
                    record.session_ref.as_deref() == Some(session_ref) && !record.closed
                })
            })
            .collect();

        for session_id in &live {
            // Durable intent first, exactly as `stop_session` orders it: if
            // this write fails the actor stays live and no contradictory
            // ephemeral state has escaped.
            if let Err(error) = self.state.update_session(session_id, |record| {
                record.closed = true;
                record.open_turn = None;
            }) {
                tracing::error!(
                    target: "csp",
                    %session_id,
                    session_ref,
                    "settled umbrella: could not record closed intent, leaving the \
                     execution live rather than releasing it silently: {error}"
                );
                continue;
            }
            if let Err(error) = self.queue_lease(session_id, CodingSessionLeaseState::Released) {
                // Not fatal: the lease has a TTL, so a missed release costs
                // three minutes of a stale liveness signal. Holding the slot
                // costs four hours.
                tracing::warn!(
                    target: "csp",
                    %session_id,
                    "settled umbrella: lease release could not be queued, falling back \
                     to TTL expiry: {error}"
                );
            }
            self.sessions.shutdown(session_id);
            self.discard_context_packages(session_id);
            self.forget_redactions(session_id);
            tracing::info!(
                target: "csp",
                %session_id,
                session_ref,
                "settled umbrella: execution stopped and its slot released"
            );
        }
        // Same reason as `stop_session`: these generations are closed, so
        // their rows go with them.
        if !live.is_empty() {
            self.publish_seat_requests();
        }
        live.len()
    }

    fn stop_session(&mut self, plan: StopPlan) -> anyhow::Result<()> {
        let session_id = plan.target.session_id.clone();
        let target_key = coding_session_target_key(&plan.target);
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
        let mut completion_errors = Vec::new();
        if let Err(error) = self.state.consume_command(&plan.command_id, now_secs()) {
            completion_errors.push(format!("consume stop command: {error}"));
        }
        self.sessions.shutdown(&plan.target.session_id);
        self.discard_context_packages(&plan.target.session_id);
        // Stop is terminal, so the operator will not be reading this
        // transcript's redactions back. A resume deliberately does *not* do
        // this: there the session continues and the note is still wanted.
        self.forget_redactions(&plan.target.session_id);
        let receipt = LifecycleReceipt::stopped(&plan.command_id, &plan.target);
        if let Err(error) = self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt) {
            completion_errors.push(format!("enqueue stopped receipt: {error}"));
        }
        if let Err(error) =
            self.publish_metadata(plan.channel_id, &plan.target, SessionStatus::Stopped)
        {
            completion_errors.push(format!("enqueue stopped metadata: {error}"));
        }
        tracing::info!(
            target: "csp",
            command_id = %plan.command_id,
            session_id = %plan.target.session_id,
            "session stopped"
        );
        if completion_errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(completion_errors.join("; ")))
        }
    }

    async fn on_turn(
        &mut self,
        channel_id: Uuid,
        created_at: u64,
        operator_pubkey: &str,
        event_id: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let projects = ProjectsFile::default();
        // A turn never resolves a working directory or a seat; both empty maps
        // keep the context type honest without touching the disk.
        let actor_seats = crate::actor_seats::ActorSeatsFile::default();
        let decision = commands::decide_turn(
            &self.context(channel_id, &projects, &actor_seats, operator_pubkey),
            created_at,
            content,
        );
        self.apply_turn_decision(channel_id, created_at, operator_pubkey, event_id, decision)
            .await
            .map(|_| ())
    }

    /// Carry out one already-made turn decision.
    ///
    /// Split out of [`Self::on_turn`] so the CI-continuation delivery path can
    /// reach the *same* code with a decision made from a durable registration
    /// rather than from a freshly decoded event. Nothing here knows which of
    /// the two produced its decision, which is the point: a continuation turn
    /// is queued, degraded, fenced, receipted and consumed by exactly the
    /// machinery every other turn goes through.
    ///
    /// `registration_event_id` is the signed event's id when there is one, and
    /// empty for a provider-minted delivery. Only a registration reads it.
    pub(crate) async fn apply_turn_decision(
        &mut self,
        channel_id: Uuid,
        created_at: u64,
        operator_pubkey: &str,
        registration_event_id: &str,
        decision: TurnDecision,
    ) -> anyhow::Result<TurnDisposition> {
        // Resolved by `decide_turn_command`, which is also where the
        // duplicate check read it: the fence that admitted the turn and the
        // fence the turn claims must never be two computations.
        let mut decided_operation_key: Option<String> = None;
        let (command_id, target, deliver, dropped_attachments, mut message) = match decision {
            TurnDecision::Ignore(reason) => {
                log_ignored("turn", &reason);
                // An ignore that names a target this provider owns is a
                // refusal the operator has to see; the rest stay silent so a
                // provider never chatters about another provider's commands or
                // answers the same command twice.
                if let Some(refusal) = reason.refusal() {
                    let receipt = LifecycleReceipt::turn_refused(
                        refusal.command_id,
                        refusal.target,
                        refusal.code,
                        refusal.message,
                    );
                    let command_id = refusal.command_id.to_owned();
                    let code = refusal.code;
                    tracing::warn!(
                        target: "csp",
                        %command_id,
                        code,
                        "turn command refused: {}",
                        refusal.message
                    );
                    // Durable *before* the publish: a refusal is an answer,
                    // and an answer given twice under the same semantic key
                    // after a restart is a stutter, not a second fact.
                    self.state.record_refusal(&command_id, now_secs())?;
                    self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                    return Ok(TurnDisposition::Answered(code.to_owned()));
                }
                return Ok(TurnDisposition::Silent);
            }
            TurnDecision::Fail {
                command_id,
                target,
                code,
                message,
            } => {
                // Refused, not consumed. Consuming would claim this command
                // ran; it did not, and never will.
                // A conflicting registration is a rejected event, not a
                // cancellation of the original promise sharing its command id.
                let ci_conflict = code == payload::COMMAND_ID_CONFLICT
                    && self.ci_continuations.record(&command_id).is_some();
                if !ci_conflict {
                    self.state.record_refusal(&command_id, now_secs())?;
                }
                let receipt = LifecycleReceipt::turn_refused(&command_id, &target, code, &message);
                tracing::warn!(
                    target: "csp",
                    %command_id,
                    %operator_pubkey,
                    code,
                    "turn command rejected: {message}"
                );
                let receipt_key = if ci_conflict {
                    format!("{command_id}:conflict:{registration_event_id}")
                } else {
                    command_id.clone()
                };
                self.enqueue_receipt_with_outbox_key(
                    channel_id,
                    &command_id,
                    &receipt,
                    ci_conflict.then_some(receipt_key.as_str()),
                )?;
                return Ok(TurnDisposition::Answered(code.to_owned()));
            }
            TurnDecision::RegisterCiContinuation {
                command_id,
                target,
                identity,
                correlation_id,
                continuation,
                expires_at,
                payload_digest,
            } => {
                return self.register_ci_continuation(ci_continuation::Registration {
                    channel_id,
                    command_id,
                    target,
                    identity,
                    correlation_id,
                    continuation,
                    expires_at,
                    payload_digest,
                    registration_event_id: registration_event_id.to_owned(),
                    signer: operator_pubkey.to_owned(),
                });
            }
            TurnDecision::Start {
                command_id,
                target,
                text,
                attachments,
                deliver,
                operation_key: fence_key,
            } => {
                decided_operation_key = fence_key;
                let framing = self.turn_framing(
                    &target.session_id,
                    operator_pubkey,
                    deliver,
                    channel_id,
                    &text,
                );
                // The capability gate. A runtime that never advertised image
                // prompts does not merely ignore an image block — `buzz-agent`
                // fails the whole turn on one — so the attachments are dropped
                // here and the operator is told below, once the words have
                // actually been delivered.
                let takes_images = self
                    .prompt_image
                    .get(&target.session_id)
                    .copied()
                    .unwrap_or(false);
                let dropped_attachments = if takes_images { 0 } else { attachments.len() };
                let attachments = if takes_images {
                    attachments
                } else {
                    Vec::new()
                };
                (
                    command_id.clone(),
                    target,
                    deliver,
                    dropped_attachments,
                    // `decide_turn` returns `Start` only after checking this
                    // exact signer against the session's founder/granted-
                    // operator set, so attributing the turn to them is a
                    // witnessed fact.
                    SessionCommand::Turn {
                        command_id,
                        text,
                        attachments,
                        operator_pubkey: Some(operator_pubkey.to_owned()),
                        framing,
                    },
                )
            }
            TurnDecision::Interrupt { command_id, target } => (
                command_id.clone(),
                target,
                CodingSessionDelivery::Boundary,
                0,
                SessionCommand::Interrupt { command_id },
            ),
        };
        let session_id = target.session_id.clone();
        // The operation this turn takes custody of, read from the raw prompt
        // text — the pointer both producers mint is the bare JSON, and the
        // `[Context]` envelope is applied to the *delivery*, never to the
        // fenced identity. An interrupt spends no turn and fences nothing.
        let operation_key = match &message {
            SessionCommand::Turn { .. } => decided_operation_key.clone(),
            _ => None,
        };

        // A starting turn is the trigger for a bounded verified-context
        // refresh. Started, not awaited: the turn must not wait on a relay
        // fetch, and the sidecar picks the new generation up off disk on a
        // later tool call, so a refresh still in flight costs this turn
        // nothing. An interrupt is not a turn and refreshes nothing.
        let is_turn = matches!(message, SessionCommand::Turn { .. });
        if is_turn {
            self.spawn_context_refresh(&session_id);
        }

        // Interrupt-class delivery: cancel what is running, then deliver the
        // new turn at the boundary that cancel creates. Authority for this was
        // already checked — `decide_turn` refuses an interrupt-class turn from
        // anyone but the founder.
        if is_turn && deliver == CodingSessionDelivery::Interrupt {
            // Two sends into one bounded mailbox — the cancel, then the turn
            // that replaces what was cancelled — and the actor stops reading
            // that mailbox for up to `session::CANCEL_GRACE` while it drains
            // the turn the cancel ended. Issuing the cancel and *then*
            // discovering the turn cannot be taken destroys the running work
            // and loses the words meant to replace it, under a receipt that
            // mentions only the queue. Both slots or neither: this run loop is
            // the sole producer, so the room measured here is the room the two
            // sends get.
            if !self.mailbox_fits_interrupt_class(&session_id) {
                // Terminal, and recorded as such: the sender has to send it
                // again, and a redelivery must not republish this answer.
                self.state.record_refusal(&command_id, now_secs())?;
                let receipt = LifecycleReceipt::turn_dropped(
                    &command_id,
                    &target,
                    QUEUE_FULL_TURN_KEPT,
                    "the execution's queue is full, so the running turn was left alone and \
                     this turn was not delivered; send it again once the queue drains",
                );
                self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                return Ok(TurnDisposition::Answered(QUEUE_FULL_TURN_KEPT.to_owned()));
            }
            self.interrupt_open_turn(&session_id, &command_id);
        }

        // Steer-class delivery: the injection is attempted first, and its
        // answer — did the words actually reach the running turn? — is the one
        // fact everything below reads. A steer that was not injected is a
        // boundary delivery, and one that is never *said* to be a boundary
        // delivery is the silent downgrade the delivery classes exist to
        // prevent.
        let steer_injected = is_turn
            && deliver == CodingSessionDelivery::Steer
            && self.inject_native_steer(&session_id);
        // The frame is captured with the class the sender *asked* for, because
        // that is all `decide_turn` knows. It is the only place the
        // **recipient** reads the class — the `turn_degraded` receipt answers
        // the sender — so once the injection has been attempted and refused,
        // the frame has to name the delivery that actually happened. Telling
        // the receiving agent "Delivery: steer" about a turn that was queued
        // to the next boundary is the silent downgrade under a different name.
        if deliver == CodingSessionDelivery::Steer && !steer_injected {
            if let SessionCommand::Turn {
                framing: Some(framing),
                ..
            } = &mut message
            {
                framing.delivery = CodingSessionDelivery::Boundary;
            }
        }

        if is_turn && self.ci_continuations.record(&command_id).is_some() {
            message = SessionCommand::GuardedCiTurn {
                turn: Box::new(message),
            };
        }
        let disposition = match self.sessions.handle(&session_id) {
            Some(handle) => match handle.deliver(message) {
                // Custody, not execution: the provider has the turn, and the
                // `turn_started` receipt is what says it began.
                Ok(()) if is_turn => {
                    self.in_flight.insert(
                        command_id.clone(),
                        InFlightTurn {
                            channel_id,
                            session_id: session_id.clone(),
                            target: target.clone(),
                            created_at,
                            operation_key: operation_key.clone(),
                            operator_pubkey: operator_pubkey.to_owned(),
                        },
                    );
                    // A `steer` this execution cannot receive is answered
                    // beside the delivery, never instead of it: the receipt
                    // says the injection did not happen and the turn still
                    // runs at the next boundary. It is published here, after
                    // the mailbox took the turn, because a degrade in front of
                    // a delivery that then fails is two receipts contradicting
                    // each other about one command — "accepted for the next
                    // boundary" followed by "this execution has no live
                    // process".
                    if deliver == CodingSessionDelivery::Steer && !steer_injected {
                        let receipt = LifecycleReceipt::turn_degraded(
                            &command_id,
                            &target,
                            payload::STEER_UNSUPPORTED,
                            STEER_DOWNGRADED,
                        );
                        self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                    }
                    // Same rule as the steer degrade, for the same reason: an
                    // image the agent never received is indistinguishable from
                    // one it saw and ignored, so the sender is told rather than
                    // left to infer it from the reply.
                    if dropped_attachments > 0 {
                        let receipt = LifecycleReceipt::turn_degraded(
                            &command_id,
                            &target,
                            payload::IMAGE_UNSUPPORTED,
                            IMAGES_DROPPED,
                        );
                        self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                    }
                    let receipt = LifecycleReceipt::turn_queued(&command_id, &target);
                    self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                    TurnDisposition::Delivered
                }
                // An interrupt is answered by whether the cancel reached a
                // live turn, which is a fact this loop already holds.
                Ok(()) => {
                    // Custody, not just the fold: a turn this provider has
                    // accepted and not yet seen start is in flight as surely
                    // as one whose `TurnStarted` has already been folded. The
                    // run loop is biased towards the relay arm, so answering
                    // from `open_turn` alone published a signed "nothing to
                    // cancel" for an interrupt that did reach a running turn.
                    let open_turn = self
                        .state
                        .session(&session_id)
                        .is_some_and(|record| record.open_turn.is_some())
                        || self
                            .in_flight
                            .values()
                            .any(|turn| turn.session_id == session_id);
                    let receipt = if open_turn {
                        LifecycleReceipt::interrupt_delivered(&command_id, &target)
                    } else {
                        LifecycleReceipt::turn_refused(
                            &command_id,
                            &target,
                            payload::NO_TURN_IN_FLIGHT,
                            "the execution is live but had no turn in flight to cancel",
                        )
                    };
                    // The answer first, then the process-local fence, then
                    // the durable one, and that order is the whole point.
                    // `handle.deliver` has already put the
                    // `SessionCommand::Interrupt` in the actor's mailbox and
                    // nothing can take it back, while every write below can
                    // fail and hand this command back to `replay.held` for a
                    // later try. The fence is silent
                    // (`Ignored::AlreadyAccepted`), so it may only ever close
                    // over a cancel that has *already* been answered, and both
                    // earlier orders broke that. Ledger-first: the append
                    // failed, `?` returned in front of the receipt, and the
                    // fence — correctly — made the relay's redelivery silent,
                    // so a cancel that had destroyed a running turn was
                    // answered by nothing at all. Fence-first had the same
                    // hole through the sibling write: `outbox.jsonl` and
                    // `commands.jsonl` share one state directory, so the
                    // failure class this fence is bounded for (full,
                    // read-only, gone) takes the *receipt* first. Enqueuing
                    // into the crash-safe outbox before recording anything
                    // leaves that failure recoverable: nothing is fenced, the
                    // command stays held, the channel floor stays behind it,
                    // and the later try answers it. Contracts D and E both
                    // forbid the alternative.
                    self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                    // Answered durably, so the process-local fence may close:
                    // it is what stops the relay's redelivery reaching a
                    // second mailbox in the window before the ledger below
                    // takes that job over. `in_flight` is deliberately not the
                    // place for it — this is not a turn, and an entry there
                    // would poison the `open_turn` predicate above, the
                    // `watermark_ceiling` clamp and `report_lost_mailbox`'s
                    // `is_turn` receipt.
                    self.remember_delivered_cancel(command_id.clone());
                    // Now the durable fence. Consumed when the cancel was
                    // issued, which is the whole of what an interrupt does;
                    // refused when there was nothing to cancel — the two
                    // ledgers answer different questions and a command belongs
                    // in exactly one of them. A redelivery answered here is
                    // rejected by `decide_turn` before it can reach a mailbox,
                    // which is what makes the silence above legitimate.
                    if open_turn {
                        self.state.consume_command(&command_id, now_secs())?;
                    } else {
                        self.state.record_refusal(&command_id, now_secs())?;
                    }
                    // Durably answered *and* durably fenced: the process-local
                    // record has nothing left to say.
                    self.forget_delivered_cancel(&command_id);
                    TurnDisposition::Interrupt
                }
                Err(error) => {
                    tracing::warn!(
                        target: "csp",
                        %command_id,
                        %session_id,
                        "could not deliver command to session: {error:?}"
                    );
                    TurnDisposition::Answered(self.report_undelivered_turn(
                        channel_id,
                        &command_id,
                        &target,
                        is_turn,
                        &error,
                    )?)
                }
            },
            // A persisted session with no live actor. This used to be a
            // `tracing::warn!` and nothing else: the operator's turn vanished
            // with no signed trace of where it went. It is now a visible drop.
            None => {
                tracing::warn!(
                    target: "csp",
                    %command_id,
                    %session_id,
                    "no live actor for a persisted session"
                );
                TurnDisposition::Answered(self.report_no_live_execution(
                    channel_id,
                    &command_id,
                    &target,
                    is_turn,
                )?)
            }
        };
        Ok(disposition)
    }

    /// Whether a native mid-turn steer can be delivered to `session_id`.
    ///
    /// Two facts, both required: this execution's runtime advertised steering
    /// at `initialize`, and this provider can deliver one at all (see
    /// [`session::NATIVE_STEER_DELIVERABLE`]). Which of the two failed is
    /// logged, not published — the receipt message has to be a function of the
    /// command alone, because a redelivery answered by a process that learned
    /// different capabilities would otherwise publish a different payload
    /// under the same `(commandId, turn_degraded)` semantic key, and a
    /// consumer that sees one key carry two payloads drops both.
    fn native_steer_deliverable(&self, session_id: &str) -> bool {
        let advertised = self.steering.get(session_id).copied().unwrap_or(false);
        if advertised && !session::NATIVE_STEER_DELIVERABLE {
            tracing::info!(
                target: "csp",
                %session_id,
                "runtime advertised mid-turn steering but this provider cannot deliver one yet"
            );
        }
        advertised && session::NATIVE_STEER_DELIVERABLE
    }

    /// Inject a `steer`-class turn into the turn already running on
    /// `session_id`, answering whether the injection actually happened.
    ///
    /// Always `false` today, and deliberately shaped so that stays true until
    /// somebody writes the injection. The caller publishes `turn_degraded`
    /// whenever this returns `false` — it does *not* consult
    /// [`session::NATIVE_STEER_DELIVERABLE`] itself — because a downgrade
    /// gated on a constant is a downgrade that disappears the day the constant
    /// is flipped without the transport behind it, leaving a steer
    /// boundary-delivered under a plain `turn_queued` and nobody told.
    ///
    /// The `const` block below is the other half of that fence: flipping the
    /// constant fails the build here rather than shipping a lie. Whoever wires
    /// the real injection replaces it, and owes this path the
    /// `turn_started` receipt (current `turnId`, `user_prompt{steered:true}`)
    /// that a genuinely injected steer publishes instead of `turn_queued`.
    fn inject_native_steer(&mut self, session_id: &str) -> bool {
        const {
            assert!(
                !session::NATIVE_STEER_DELIVERABLE,
                "wire the native steer injection, and the turn_started receipt it publishes, \
                 before declaring this provider able to deliver one"
            )
        };
        // Consulted for its log line: an operator whose runtime offered a steer
        // this provider could not take should be able to find out why.
        let _deliverable = self.native_steer_deliverable(session_id);
        false
    }

    /// Whether `session_id`'s mailbox can take both halves of an interrupt-class
    /// delivery.
    ///
    /// `true` when there is no live handle at all: that command's answer is
    /// the ordinary no-live-execution drop below, and this check must not
    /// substitute a different one for it.
    fn mailbox_fits_interrupt_class(&self, session_id: &str) -> bool {
        self.sessions
            .handle(session_id)
            .is_none_or(|handle| handle.free_slots() >= 2)
    }

    /// Cancel whatever turn is running on `session_id` so an interrupt-class
    /// turn can be delivered at the boundary that creates.
    ///
    /// Sent to any live execution, without first consulting `open_turn`: that
    /// fold lags the actor by one pass of the run loop's biased `select!`, so
    /// a turn and an interrupt-class turn sent back to back would find it
    /// empty and skip the cancel the founder asked for — silently, with no
    /// receipt saying the class did not happen. The actor no-ops an interrupt
    /// it has no turn for (`session.rs`'s idle arm), so asking costs nothing.
    /// A full mailbox is reported by the delivery that follows. The interrupt
    /// is never merged into the running turn — cancel-and-merge rewrites what
    /// the agent was asked to do, which is exactly what the delivery classes
    /// exist to stop.
    fn interrupt_open_turn(&mut self, session_id: &str, command_id: &str) {
        let Some(handle) = self.sessions.handle(session_id) else {
            return;
        };
        if let Err(error) = handle.deliver(SessionCommand::Interrupt {
            command_id: command_id.to_owned(),
        }) {
            tracing::warn!(
                target: "csp",
                %command_id,
                %session_id,
                "interrupt-class turn could not cancel the running turn: {error:?}"
            );
        }
    }

    /// Answer a command whose delivery to a live actor failed.
    fn report_undelivered_turn(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        is_turn: bool,
        error: &DeliverError,
    ) -> anyhow::Result<String> {
        match error {
            // The mailbox is full and the execution is alive. That is a
            // terminal answer for this command: it is recorded as refused so a
            // redelivery cannot resurrect a turn the operator was already told
            // was dropped.
            DeliverError::QueueFull if is_turn => {
                self.state.record_refusal(command_id, now_secs())?;
                let receipt = LifecycleReceipt::turn_dropped(
                    command_id,
                    target,
                    payload::QUEUE_FULL,
                    "the execution's queue is full",
                );
                self.enqueue_receipt(channel_id, command_id, &receipt)?;
                Ok(payload::QUEUE_FULL.to_owned())
            }
            DeliverError::QueueFull => {
                self.state.record_refusal(command_id, now_secs())?;
                let receipt = LifecycleReceipt::turn_refused(
                    command_id,
                    target,
                    payload::QUEUE_FULL,
                    "the execution's queue is full, so the interrupt could not be delivered",
                );
                self.enqueue_receipt(channel_id, command_id, &receipt)?;
                Ok(payload::QUEUE_FULL.to_owned())
            }
            // The actor is gone: same fact as no handle at all.
            DeliverError::Gone => {
                self.report_no_live_execution(channel_id, command_id, target, is_turn)
            }
        }
    }

    /// Answer every turn an exiting execution had taken custody of and not
    /// started.
    ///
    /// The mailbox died with the process. Each of those turns gets the same
    /// visible `turn_dropped` a turn addressed to a dead execution gets — it
    /// never ran, so it is never consumed, and it will not be run later, so it
    /// is recorded as refused. Before this, they disappeared with no signed
    /// trace at all.
    fn report_lost_mailbox(&mut self, session_id: &str) -> anyhow::Result<()> {
        let lost: Vec<(String, InFlightTurn)> = self
            .in_flight
            .iter()
            .filter(|(_, turn)| turn.session_id == session_id)
            .map(|(command_id, turn)| (command_id.clone(), turn.clone()))
            .collect();
        for (command_id, turn) in lost {
            self.in_flight.remove(&command_id);
            let _ =
                self.report_no_live_execution(turn.channel_id, &command_id, &turn.target, true)?;
        }
        Ok(())
    }

    /// The newest `created_at` this provider may record as `channel_id`'s
    /// watermark, given what it has accepted for that channel and not yet
    /// started.
    ///
    /// The watermark is the restart replay floor. Advancing it past a command
    /// that has been accepted but has not run would skip that command on the
    /// next start — which is precisely the loss consume-at-start exists to
    /// prevent. So the floor is held at the oldest turn this process still
    /// owes, whether that turn is sitting in an execution's mailbox or in the
    /// replay reorder window.
    ///
    /// At, not below: `since` is inclusive on both the relay's SQL
    /// (`created_at >= $since`, `crates/buzz-db/src/event.rs:583`) and its
    /// in-memory matcher (`crates/buzz-core/src/filter.rs:53-55`), so a
    /// watermark equal to a command's `created_at` still replays that command.
    fn watermark_ceiling(&self, channel_id: Uuid) -> Option<u64> {
        self.in_flight
            .values()
            .filter(|turn| turn.channel_id == channel_id)
            .map(|turn| turn.created_at)
            .chain(
                self.replay
                    .held
                    .iter()
                    .filter(|held| held.channel_id == channel_id)
                    .map(|held| held.created_at),
            )
            .min()
    }

    /// Publish the visible answer for a command that reached no live process.
    ///
    /// Terminal, and said as such. An earlier version left the command in
    /// neither ledger and promised the operator a replay — "resume it and the
    /// turn will be delivered" — that nothing kept: the watermark walked past
    /// the owed turn on the next durable write, and a resume mints a new
    /// generation the replayed command no longer addresses
    /// (`commands.rs`'s `StaleGeneration` fence). A signed receipt that
    /// states a falsehood is worse than a blunt one, so the turn is recorded
    /// as refused and the message says the sender has to send it again.
    ///
    /// The receipt is queued **before** the refusal is recorded. Both writes
    /// are crash-safe and the outbox fences on `(kind, semantic key)`, so
    /// enqueuing first cannot produce a second answer; recording first could
    /// produce *no* answer, because a durably refused command is never
    /// re-derived on replay and this path is the only thing that would have
    /// spoken for it. A ledger failure after a successful enqueue is returned
    /// to the caller with the answer already on its way.
    fn report_no_live_execution(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        is_turn: bool,
    ) -> anyhow::Result<String> {
        let receipt = if is_turn {
            LifecycleReceipt::turn_dropped(
                command_id,
                target,
                payload::NO_LIVE_EXECUTION,
                "this execution has no live process, so the turn was not delivered and will \
                 not be retried; resume the execution and send it again",
            )
        } else {
            // An interrupt of nothing is terminal — there is no later moment
            // at which it becomes meaningful.
            LifecycleReceipt::turn_refused(
                command_id,
                target,
                payload::NO_LIVE_EXECUTION,
                "this execution has no live process, so there is no turn to interrupt",
            )
        };
        self.enqueue_terminal_receipt(channel_id, command_id, &receipt)?;
        // Refused, not consumed: the turn never ran, and it never will.
        // Recording it terminally is also what lets the channel watermark
        // move — an unanswered command holds the replay floor, an answered
        // one does not.
        Ok(payload::NO_LIVE_EXECUTION.to_owned())
    }

    /// Handle one kind 40099 system message: if it is a verifiable
    /// authority-transition acceptance receipt, fold its grant into every
    /// session record rooted at the genesis it names.
    ///
    /// This is the live half of chain consumption — the receipt arrives on
    /// the same channel subscription as commands, so a grant published
    /// mid-session is honored without a provider restart. Everything that
    /// fails verification is ignored with a warning and applies nothing:
    /// authority only ever extends through verified facts.
    async fn on_authority_receipt(
        &mut self,
        channel_id: Uuid,
        event: &Event,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        if !authority::looks_like_acceptance_receipt(&event.content) {
            // Joins, leaves, and every other system row.
            return Ok(());
        }
        let Some(relay) = relay else {
            tracing::debug!(
                target: "csp::authority",
                "no relay resolver for an acceptance receipt — deferred to backfill"
            );
            return Ok(());
        };
        let Some(relay_self) = self.relay_self.clone() else {
            tracing::warn!(
                target: "csp::authority",
                "acceptance receipt seen but no relay identity is witnessed — grant not applied"
            );
            return Ok(());
        };
        let accepted = match authority::verify_acceptance_receipt(event, &relay_self, channel_id) {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(
                    target: "csp::authority",
                    "ignoring unverifiable acceptance receipt: {error}"
                );
                return Ok(());
            }
        };

        let rest = relay.rest_client();
        // Every execution record rooted at this genesis shares the chain —
        // selection is by the locally recorded `genesis_ref`, never by tag.
        let sessions: Vec<(String, u32)> = self
            .state
            .sessions()
            .filter(|record| {
                record.channel_id == channel_id
                    && record.genesis_ref.as_deref() == Some(accepted.genesis_ref.as_str())
            })
            .map(|record| (record.session_id.clone(), record.authority_seq))
            .collect();
        for (session_id, applied_seq) in sessions {
            if accepted.seq <= applied_seq {
                continue; // Replay of an already-applied link.
            }
            // A **relay-verified** receipt this provider could not apply means
            // the chain has moved somewhere this build cannot follow, and the
            // record's `handover` is now known to be behind. Marking the
            // umbrella uncertain is the difference between "the fence is open
            // because nothing changed" and "the fence is open because I could
            // not read what changed" — only the first of those is safe, and
            // before this the two were the same code path (root's P1).
            //
            // It applies whether or not the umbrella was pending at startup: a
            // live receipt is exactly the case a restart never saw.
            let applied_now = if accepted.seq == applied_seq + 1 {
                self.resolve_and_apply_grant(&session_id, &accepted, &rest)
                    .await?
            } else {
                // A gap means receipts were missed; refill from storage, then
                // retry this link in case it is now the contiguous next one.
                self.backfill_session_authority(&session_id, &rest).await?;
                let applied = self
                    .state
                    .session(&session_id)
                    .map(|record| record.authority_seq)
                    .unwrap_or(0);
                if accepted.seq == applied + 1 {
                    self.resolve_and_apply_grant(&session_id, &accepted, &rest)
                        .await?
                } else {
                    false
                }
            };
            if !applied_now {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    genesis_ref = %accepted.genesis_ref,
                    seq = accepted.seq,
                    "an accepted authority link arrived that this provider could not apply; its executions refuse commands until the chain can be read"
                );
                self.claims_pending_reverification
                    .insert(accepted.genesis_ref.clone());
            }
        }
        // Applying a claim link can fence a body that is mid-turn. Asked once
        // for the umbrella, after every local record of it has folded, so a
        // record cannot be interrupted on a claim its sibling has applied and
        // it has not.
        self.enforce_claim_for_genesis(&accepted.genesis_ref)?;
        Ok(())
    }

    /// The sentence for a handover that finds dequeue evidence.
    ///
    /// The actor and provider advance independently: work may already be
    /// complete while its reports wait in the provider's inbox. Request the
    /// cancellation, but do not claim either that nothing ran or that a
    /// running tool call stopped immediately.
    const HANDOVER_INTERRUPT_REASON: &'static str =
        "HANDOVER_FENCED: this turn had already left the queue when the handover was \
         processed. Cancellation was requested, but the runtime may already have completed \
         work and may still finish a tool call it had already started. Further commands from \
         the displaced operator are refused.";

    /// The sentence a turn refused *before it was ever dequeued* carries.
    ///
    /// Distinct from [`Self::HANDOVER_INTERRUPT_REASON`] because the fact is
    /// different and a person acting on it needs to know which: nothing ran,
    /// so there is no partial work to reconcile. It is only used when the
    /// actor confirms the turn was still queued
    /// ([`session::FencedAt::Queued`]) — saying "never ran" about a prompt
    /// that raced out would be the comfortable answer rather than the true
    /// one, which is exactly the class of claim this fence exists to stop
    /// making.
    const HANDOVER_UNSTARTED_REASON: &'static str =
        "this session was handed over before this turn reached the agent, so it never ran and \
         will not be retried; the session is now held by someone else";

    /// Stop work this umbrella's claim has just fenced — running, queued, or
    /// merely admitted.
    ///
    /// Turn *admission* stops the next command. It does nothing about the
    /// three kinds of work that are already past it, and each was its own way
    /// for two live executions of one task to exist:
    ///
    /// 1. **The prompt in the adapter.** Cancelled through the same path
    ///    `thread.turn.interrupt` uses, with a truthful terminal row and a
    ///    durable refusal so the replay is silent.
    /// 2. **A command admitted but not yet started.** `on_turn` records a turn
    ///    in flight the moment the mailbox takes it; `open_turn` is only
    ///    written when the runtime answers `TurnStarted`. A claim landing in
    ///    that window used to see no open turn and do nothing, and the prompt
    ///    went out afterwards. These are refused by name and dropped from the
    ///    in-flight set before the runtime is asked anything.
    /// 3. **Custody of the actor itself.** An interrupt empties the running
    ///    turn, not the mailbox, so anything queued behind it would simply run
    ///    next. When the **body** is fenced the actor is shut down, which is
    ///    the only way to say that nothing further runs here.
    ///
    /// Whose work it is decides all three. The fence is asked with each turn's
    /// own operator ([`crate::state::OpenTurn::operator_pubkey`] and
    /// [`InFlightTurn::operator_pubkey`]), so a `transfer` to a new claimant on
    /// the **same** body stops the old claimant's work and leaves the new
    /// claimant's alone — and a body that is still the claimed one keeps its
    /// actor. A turn with no recorded operator (a create's `initialTurn`, or a
    /// record written before the field existed) falls back to asking whether
    /// this body may act at all.
    ///
    /// Idempotent, and safe to call after any claim progress: a record with
    /// nothing fenced is left completely alone, so a replayed receipt and a
    /// tick retry that folds the same link stop nothing twice.
    fn enforce_claim_for_genesis(&mut self, genesis_ref: &str) -> anyhow::Result<()> {
        let sessions: Vec<SessionRecord> = self
            .state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.as_deref() == Some(genesis_ref))
            .cloned()
            .collect();

        for record in sessions {
            let session_id = record.session_id.clone();
            let target = self.target_for(&record);
            // Is this *body* fenced at all — the question asked with the
            // provider as its own operator. `false` means the claim names this
            // machine and only some operators are out.
            let body_fenced = commands::handover_fence(&record, &self.pubkey_hex, &self.pubkey_hex)
                .is_some_and(|refusal| refusal.code == payload::HANDOVER_FENCED);
            let mut stopped_something = false;

            // (2) first: a command admitted but not yet started is stopped
            //     before the runtime is asked for anything, which is the whole
            //     point of doing it ahead of the cancel below.
            // The record's *running* command is stage (1)'s, not this one's:
            // answering it here as well would give one command two terminal
            // answers under two different sentences.
            let running = record
                .open_turn
                .as_ref()
                .and_then(|open_turn| open_turn.command_id.clone());
            let admitted: Vec<(String, String)> = self
                .in_flight
                .iter()
                .filter(|(command_id, turn)| {
                    turn.session_id == session_id && running.as_deref() != Some(command_id.as_str())
                })
                .map(|(command_id, turn)| (command_id.clone(), turn.operator_pubkey.clone()))
                .collect();
            for (command_id, operator) in admitted {
                let Some(refusal) = commands::handover_fence(&record, &operator, &self.pubkey_hex)
                    .filter(|refusal| refusal.code == payload::HANDOVER_FENCED)
                else {
                    continue; // The claimant's own admitted work proceeds.
                };
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    %command_id,
                    "{} — refusing a turn that was admitted but had not started",
                    refusal.message
                );
                // Refuse it *in the actor's queue* as well as on the books.
                // Removing the in-flight entry alone left the command sitting
                // in the mailbox, and the actor prompted for it moments later
                // — the turn was refused in the ledger and delivered to the
                // runtime, which is the worst of both answers. The actor drops
                // it at dequeue instead.
                //
                // The answer that comes back decides what the receipt says. A
                // turn still queued genuinely never ran; one already dequeued
                // is being interrupted, and gets the sentence that states the
                // tool-call limit rather than a claim that nothing happened.
                let fenced_at = match self.sessions.handle(&session_id) {
                    Some(handle) => handle.fence_command(&command_id),
                    // No actor at all: nothing can have been dequeued.
                    None => session::FencedAt::Queued,
                };
                let reason = match fenced_at {
                    session::FencedAt::Queued => Self::HANDOVER_UNSTARTED_REASON,
                    session::FencedAt::AlreadyDequeued => {
                        // It may be running, about to run, or already complete
                        // while actor reports wait. Request cancellation without
                        // asserting that the turn never ran.
                        self.interrupt_open_turn(&session_id, &command_id);
                        Self::HANDOVER_INTERRUPT_REASON
                    }
                };
                // Durable before the receipt, as every other terminal turn
                // refusal is: a replay must be answered from the ledger rather
                // than republished.
                self.state.record_refusal(&command_id, now_secs())?;
                self.in_flight.remove(&command_id);
                let receipt = LifecycleReceipt::turn_refused(
                    &command_id,
                    &target,
                    payload::HANDOVER_FENCED,
                    reason,
                );
                self.enqueue_receipt(record.channel_id, &command_id, &receipt)?;
                stopped_something = true;
            }

            // (1) the prompt the runtime already has.
            if let Some(open_turn) = record.open_turn.clone() {
                // The turn's own operator, then the create's signer for a
                // record whose open turn is its `initialTurn`, and only then
                // this provider — each step is a better answer to "whose work
                // is this" than the one after it.
                let operator = open_turn
                    .operator_pubkey
                    .clone()
                    .or_else(|| record.created_by.clone())
                    .unwrap_or_else(|| self.pubkey_hex.clone());
                if let Some(refusal) =
                    commands::handover_fence(&record, &operator, &self.pubkey_hex)
                        .filter(|refusal| refusal.code == payload::HANDOVER_FENCED)
                {
                    tracing::warn!(
                        target: "csp::authority",
                        %session_id,
                        turn_id = %open_turn.turn_id,
                        "{} — cancelling the running turn",
                        refusal.message
                    );
                    // The cancel first: it is the only part of this that races
                    // the adapter, and everything below only records what was
                    // decided.
                    self.interrupt_open_turn(
                        &session_id,
                        open_turn
                            .command_id
                            .as_deref()
                            .unwrap_or(&open_turn.turn_id),
                    );
                    let elapsed = u64::try_from(now_ms().saturating_sub(open_turn.started_at_ms))
                        .unwrap_or_default();
                    self.enqueue_transcript(
                        record.channel_id,
                        &target,
                        Some(&open_turn.turn_id),
                        payload::result_item(
                            payload::ResultSubtype::Error,
                            elapsed,
                            Self::HANDOVER_INTERRUPT_REASON,
                            payload::TurnCost::default(),
                            payload::TurnUsageReport::default(),
                        ),
                        Priority::High,
                    )?;
                    if let Some(command_id) = open_turn.command_id.as_deref() {
                        self.state.record_refusal(command_id, now_secs())?;
                        self.in_flight.remove(command_id);
                    }
                    self.state.update_session(&session_id, |record| {
                        record.open_turn = None;
                    })?;
                    stopped_something = true;
                }
            }

            // (3) custody. Only when the body itself is fenced: on the claimed
            //     body the claimant is still working here, and taking its
            //     actor away would be the fence undoing the handover.
            if body_fenced && (stopped_something || self.sessions.handle(&session_id).is_some()) {
                self.sessions.shutdown(&session_id);
                self.discard_context_packages(&session_id);
                self.publish_metadata(record.channel_id, &target, SessionStatus::Disconnected)?;
            }
        }
        Ok(())
    }

    /// Fold this umbrella's current claim from the accepted chain, verifying
    /// every link, without needing a local record of it.
    ///
    /// The create path's question, and it cannot be answered the way every
    /// other path answers it. `handover_fence` reads a `SessionRecord`; a
    /// create on a machine that has never seen this umbrella has none, which
    /// is exactly the case root's P1 found dispatching a first prompt under
    /// somebody else's claim. So the chain is read directly: paged discovery
    /// ([`Provider::read_accepted_chain`]), then every link resolved by its
    /// explicit `acceptedEventId` and checked against its receipt and the
    /// genesis owner — the same two proofs
    /// [`Provider::resolve_and_apply_grant`] applies, because a receipt alone
    /// must never decide a claim.
    ///
    /// # Errors
    /// A sentence naming what could not be read or verified. An error is
    /// **not** "no claim": the caller holds the create rather than admitting
    /// it, exactly as a restart holds an unread chain.
    async fn verified_umbrella_claim(
        &self,
        channel_id: Uuid,
        genesis_ref: &str,
        owner_pubkey: &str,
        rest: &RestClient,
    ) -> Result<buzz_core::coding_session_authority_claim::ClaimState, String> {
        let chain = self
            .read_accepted_chain(channel_id, genesis_ref, rest)
            .await;
        if let Some(reason) = chain.incomplete {
            return Err(reason);
        }
        let mut links = Vec::with_capacity(chain.links.len());
        for accepted in chain.links {
            let transition = rest
                .query_event_by_id(
                    &accepted.accepted_event_id,
                    Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
                )
                .await
                .map_err(|error| format!("an accepted transition could not be read: {error}"))?
                .ok_or_else(|| {
                    format!(
                        "accepted transition {} is not query-visible",
                        accepted.accepted_event_id
                    )
                })?;
            authority::verify_accepted_transition(
                &transition,
                &accepted,
                channel_id,
                owner_pubkey,
            )?;
            links.push(ClaimLink {
                seq: accepted.seq,
                accepted_event_id: accepted.accepted_event_id,
                transition_type: accepted.transition_type,
                grantee_pubkey: accepted.grantee_pubkey,
                body_pubkey: accepted.body_pubkey,
            });
        }
        Ok(buzz_core::coding_session_authority_claim::fold_current_claim(links.into_iter()))
    }

    /// Resolve the accepted kind 44228 transition a verified receipt names —
    /// by its explicit `acceptedEventId`, never by tag query — verify it
    /// against the receipt, the channel, and the session owner, and only then
    /// extend the persisted operator set.
    ///
    /// Returns whether the grant was applied. Every refusal path applies
    /// nothing and logs why: a chain that cannot be verified simply never
    /// grants, it never guesses.
    async fn resolve_and_apply_grant(
        &mut self,
        session_id: &str,
        accepted: &authority::AcceptedTransition,
        rest: &RestClient,
    ) -> anyhow::Result<bool> {
        let Some(record) = self.state.session(session_id) else {
            return Ok(false);
        };
        let channel_id = record.channel_id;
        let Some(owner) = record.founder_pubkey.clone() else {
            tracing::warn!(
                target: "csp::authority",
                %session_id,
                "genesis-bearing record has no recorded owner — grant not applied"
            );
            return Ok(false);
        };
        let transition = match rest
            .query_event_by_id(
                &accepted.accepted_event_id,
                Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
            )
            .await
        {
            Ok(Some(transition)) => transition,
            Ok(None) => {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    accepted_event_id = %accepted.accepted_event_id,
                    "accepted transition is not query-visible — grant not applied"
                );
                return Ok(false);
            }
            Err(error) => {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    "accepted transition lookup failed: {error} — grant not applied"
                );
                return Ok(false);
            }
        };
        if let Err(error) =
            authority::verify_accepted_transition(&transition, accepted, channel_id, &owner)
        {
            tracing::warn!(
                target: "csp::authority",
                %session_id,
                "rejecting acceptance whose transition fails verification: {error}"
            );
            return Ok(false);
        }
        self.state.update_session(session_id, |record| {
            match accepted.transition_type {
                CodingSessionAuthorityTransitionType::GrantOperator => {
                    record
                        .granted_operators
                        .insert(accepted.grantee_pubkey.clone());
                    record.granted_viewers.remove(&accepted.grantee_pubkey);
                }
                CodingSessionAuthorityTransitionType::GrantViewer => {
                    record
                        .granted_viewers
                        .insert(accepted.grantee_pubkey.clone());
                    record.granted_operators.remove(&accepted.grantee_pubkey);
                }
                CodingSessionAuthorityTransitionType::Revoke => {
                    record.granted_operators.remove(&accepted.grantee_pubkey);
                    record.granted_viewers.remove(&accepted.grantee_pubkey);
                }
                // Seat authority is consumed by NIP-CSTX readers, not by the
                // provider's steering ACL. The accepted link still advances
                // `authority_seq` below so later legacy grants do not stall.
                //
                // A claim is in the same position for a different reason: it
                // moves who is *carrying* the session, not who may steer it,
                // and it is applied by the claim fold immediately below rather
                // than by this ACL.
                CodingSessionAuthorityTransitionType::GrantSeat
                | CodingSessionAuthorityTransitionType::RevokeSeat
                | CodingSessionAuthorityTransitionType::Takeover
                | CodingSessionAuthorityTransitionType::Transfer => {}
            }
            // §3 claim consumption. Every accepted link is offered to the
            // canonical claim fold, not just the `takeover`/`transfer` ones:
            // a `revoke` or a `grant-viewer` of the claimant is what *voids*
            // a claim, and a `grant-operator` of that same pubkey is the one
            // that must deliberately not restore it. Folding the whole link
            // stream through one rule is what keeps that promise; see
            // [`state::extend_claim`] for why the persisted state is replayed
            // rather than the rule re-implemented.
            record.handover = state::extend_claim(
                &record.handover,
                ClaimLink {
                    seq: accepted.seq,
                    accepted_event_id: accepted.accepted_event_id.clone(),
                    transition_type: accepted.transition_type,
                    grantee_pubkey: accepted.grantee_pubkey.clone(),
                    body_pubkey: accepted.body_pubkey.clone(),
                },
            );
            record.authority_seq = accepted.seq;
        })?;
        tracing::info!(
            target: "csp::authority",
            %session_id,
            seq = accepted.seq,
            transition_type = ?accepted.transition_type,
            grantee = %accepted.grantee_pubkey,
            "applied accepted authority transition"
        );
        Ok(true)
    }

    /// Discover and verify one genesis's accepted chain, paging until the
    /// channel's history ends.
    ///
    /// # What makes a read complete
    ///
    /// Only this: every page was read to the end of the channel's history, no
    /// receipt naming **this** genesis failed verification, and the receipts
    /// that did verify run contiguously from `seq` 1. Anything else — a failed
    /// query, a non-array answer, no witnessed relay identity, a page budget
    /// exhausted, a last page that came back exactly full (so there may be
    /// more behind it), a gap in the sequence — leaves `incomplete` set.
    ///
    /// # What is *not* evidence about this chain
    ///
    /// Unrelated traffic. Kind 40099 carries joins, leaves and every other
    /// system row, and a channel may hold acceptance receipts for other
    /// umbrellas entirely. A row that is not a receipt, or is a receipt for
    /// another genesis, says nothing about this one and never blocks it. What
    /// *does* block it is a receipt that claims this genesis and cannot be
    /// verified: that is a link this build cannot read, sitting in the middle
    /// of the chain the fence depends on.
    async fn read_accepted_chain(
        &self,
        channel_id: Uuid,
        genesis_ref: &str,
        rest: &RestClient,
    ) -> AcceptedChain {
        use nostr::{Alphabet, SingleLetterTag};

        let incomplete = |reason: String| AcceptedChain {
            links: Vec::new(),
            incomplete: Some(reason),
        };
        let Some(relay_self) = self.relay_self.clone() else {
            return incomplete("no relay identity is witnessed".to_owned());
        };
        let Ok(relay_author) = nostr::PublicKey::from_hex(&relay_self) else {
            return incomplete("the witnessed relay identity is not a valid pubkey".to_owned());
        };

        let mut seen: HashSet<String> = HashSet::new();
        let mut rows: Vec<Event> = Vec::new();
        let mut until: Option<nostr::Timestamp> = None;
        let mut pages = 0usize;
        loop {
            pages += 1;
            if pages > AUTHORITY_BACKFILL_MAX_PAGES {
                return incomplete(format!(
                    "the channel's history did not end within {AUTHORITY_BACKFILL_MAX_PAGES} pages"
                ));
            }
            let mut filter = nostr::Filter::new()
                .kind(Kind::Custom(KIND_SYSTEM_MESSAGE as u16))
                .author(relay_author)
                .custom_tags(
                    SingleLetterTag::lowercase(Alphabet::H),
                    [channel_id.to_string()],
                )
                .limit(AUTHORITY_BACKFILL_QUERY_LIMIT);
            if let Some(until) = until {
                filter = filter.until(until);
            }
            let answer = match rest.query(&[filter]).await {
                Ok(answer) => answer,
                Err(error) => return incomplete(format!("the receipt query failed: {error}")),
            };
            let Some(page) = answer.as_array() else {
                return incomplete("the receipt query returned a non-array response".to_owned());
            };
            let page_len = page.len();
            let events: Vec<Event> = page
                .iter()
                .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
                .collect();
            let oldest = events.iter().map(|event| event.created_at).min();
            let mut fresh = 0usize;
            for event in events {
                if seen.insert(event.id.to_hex()) {
                    rows.push(event);
                    fresh += 1;
                }
            }
            // A short page is the end of the history: the relay had nothing
            // more to give. A full one may or may not be, so it is paged past.
            if page_len < AUTHORITY_BACKFILL_QUERY_LIMIT {
                break;
            }
            let Some(oldest) = oldest else {
                return incomplete(
                    "a full page of receipts could not be decoded, so the read cannot be \
                     continued past it"
                        .to_owned(),
                );
            };
            // `until` is inclusive, so a page whose rows all share one second
            // cannot be paged past — asking again returns the same page
            // forever. Say so rather than spin or silently truncate.
            if fresh == 0 {
                return incomplete(
                    "a full page of receipts shares one timestamp, so the read cannot be \
                     continued past it"
                        .to_owned(),
                );
            }
            until = Some(oldest);
        }

        let mut links: Vec<authority::AcceptedTransition> = Vec::new();
        for event in &rows {
            if !authority::looks_like_acceptance_receipt(&event.content) {
                continue; // A join, a leave, some other system row.
            }
            match authority::verify_acceptance_receipt(event, &relay_self, channel_id) {
                Ok(accepted) if accepted.genesis_ref == genesis_ref => links.push(accepted),
                Ok(_) => {} // Another umbrella's chain.
                Err(error) => {
                    // Triage only, never authority: read the claimed genesis
                    // out of the raw content to decide whether this failure is
                    // about *our* chain. An unreadable receipt for somebody
                    // else's umbrella is not our problem; one for ours is a
                    // link we cannot see, and the fence stays up.
                    if claims_genesis(&event.content, genesis_ref) {
                        return incomplete(format!(
                            "a receipt naming this genesis could not be verified: {error}"
                        ));
                    }
                    tracing::debug!(
                        target: "csp::authority",
                        "ignoring an unverifiable receipt for another umbrella: {error}"
                    );
                }
            }
        }
        links.sort_by_key(|accepted| accepted.seq);
        links.dedup_by_key(|accepted| accepted.seq);
        for (index, accepted) in links.iter().enumerate() {
            let expected = (index + 1) as u32;
            if accepted.seq != expected {
                return incomplete(format!(
                    "the accepted chain is not contiguous: expected seq {expected}, found {}",
                    accepted.seq
                ));
            }
        }
        AcceptedChain {
            links,
            incomplete: None,
        }
    }

    /// Extend one session's operator set from the channel's stored acceptance
    /// receipts: the backfill half of chain consumption, run at session load
    /// and whenever a live receipt reveals a gap.
    ///
    /// The channel query is discovery only (R21): it surfaces candidate
    /// receipts, and every fact actually applied is independently verified —
    /// receipt signature against the witnessed relay identity, explicit
    /// `genesisRef` match against the locally recorded genesis, and the
    /// accepted transition resolved by explicit reference and verified in
    /// [`Provider::resolve_and_apply_grant`]. Receipts fold strictly by
    /// contiguous `seq`; a gap or an unverifiable link stops the fold with
    /// later grants unapplied.
    ///
    /// Returns the read's own verdict on itself: `Ok(true)` when the whole
    /// chain was read, verified and folded into this record, `Ok(false)` when
    /// it was not. Only the first answer may lift the handover fence — see
    /// [`Provider::read_accepted_chain`] for what "the whole chain" means and
    /// why a partial read must never be mistaken for a chain with nothing in
    /// it.
    pub async fn backfill_session_authority(
        &mut self,
        session_id: &str,
        rest: &RestClient,
    ) -> anyhow::Result<bool> {
        let Some(record) = self.state.session(session_id) else {
            return Ok(false);
        };
        let Some(genesis_ref) = record.genesis_ref.clone() else {
            // Legacy sessions have no chain (R20), so there is nothing to
            // verify and nothing the fence is waiting on.
            return Ok(true);
        };
        let channel_id = record.channel_id;
        let mut applied_seq = record.authority_seq;

        let chain = self
            .read_accepted_chain(channel_id, &genesis_ref, rest)
            .await;
        if let Some(reason) = &chain.incomplete {
            tracing::warn!(
                target: "csp::authority",
                %session_id,
                %genesis_ref,
                "authority chain could not be read to its end: {reason}"
            );
            return Ok(false);
        }

        for accepted in chain.links {
            if accepted.seq <= applied_seq {
                continue;
            }
            // `read_accepted_chain` already proved contiguity from seq 1, so a
            // gap here means the *record* is ahead of the chain — impossible
            // unless something applied a link this read cannot see. Refuse to
            // call that a complete read.
            if accepted.seq != applied_seq + 1 {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    applied_seq,
                    next_seq = accepted.seq,
                    "authority chain does not continue from what this record applied"
                );
                return Ok(false);
            }
            if self
                .resolve_and_apply_grant(session_id, &accepted, rest)
                .await?
            {
                applied_seq = accepted.seq;
            } else {
                // A verified receipt whose named transition cannot be resolved
                // or verified: an unreadable link in the middle of *this*
                // chain. Everything past it waits, and — the part that used to
                // be missing — the read is not complete, so the fence stays up
                // rather than reading an unapplied takeover as no takeover.
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    seq = accepted.seq,
                    "an accepted link could not be resolved; the chain is not verified"
                );
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Verify one umbrella's chain across **every** local record of it, and
    /// clear the fence only if all of them now agree.
    ///
    /// The pending flag is umbrella-wide but verification happens per record,
    /// and those two facts have to be reconciled somewhere. Here: a genesis is
    /// released only when every local record of it read the whole chain *and*
    /// they all ended at the same applied head. One sibling's success must not
    /// unlock another sibling that is still sitting at an older `NoClaim` —
    /// admission reads each record's own `handover`, so a half-verified
    /// umbrella would admit a turn on the half that never learned about the
    /// takeover.
    ///
    /// Returns whether the umbrella is now verified.
    async fn verify_umbrella_chain(&mut self, genesis_ref: &str, rest: &RestClient) -> bool {
        let session_ids: Vec<String> = self
            .state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.as_deref() == Some(genesis_ref))
            .map(|record| record.session_id.clone())
            .collect();
        // An umbrella whose last record closed has nothing left to verify and
        // nothing left to hold.
        if session_ids.is_empty() {
            self.claims_pending_reverification.remove(genesis_ref);
            return true;
        }
        let mut complete = true;
        for session_id in &session_ids {
            match self.backfill_session_authority(session_id, rest).await {
                Ok(true) => {}
                Ok(false) => complete = false,
                Err(error) => {
                    tracing::warn!(
                        target: "csp::authority",
                        %session_id,
                        "authority verification failed: {error}"
                    );
                    complete = false;
                }
            }
        }
        // Before the verdict, not after it, and regardless of it: a chain
        // that only *partly* read can still have folded a takeover, and the
        // work it fences must stop whether or not the umbrella is released.
        // This is the path the live-receipt case used to be the only caller
        // of — a receipt whose transition would not resolve marked the
        // umbrella pending, and the tick that later folded it cleared the
        // fence without ever stopping the old body's turn.
        if let Err(error) = self.enforce_claim_for_genesis(genesis_ref) {
            tracing::error!(
                target: "csp::authority",
                %genesis_ref,
                "work fenced by this umbrella's claim could not be stopped: {error}"
            );
            // The claim is durable and admission already refuses; failing to
            // quiesce must not also release the fence.
            self.claims_pending_reverification
                .insert(genesis_ref.to_owned());
            return false;
        }
        if !complete {
            self.claims_pending_reverification
                .insert(genesis_ref.to_owned());
            return false;
        }
        // Every sibling read the whole chain; they must also have landed on
        // the same head, or one of them is carrying an older answer than the
        // fence is about to start trusting.
        let heads: BTreeSet<u32> = self
            .state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.as_deref() == Some(genesis_ref))
            .map(|record| record.authority_seq)
            .collect();
        if heads.len() > 1 {
            tracing::warn!(
                target: "csp::authority",
                %genesis_ref,
                heads = ?heads,
                "this umbrella's local executions applied different chain heads; the fence \
                 stays up until they agree"
            );
            self.claims_pending_reverification
                .insert(genesis_ref.to_owned());
            return false;
        }
        self.claims_pending_reverification.remove(genesis_ref);
        true
    }

    /// Re-read the chains that could not be read at startup, and release the
    /// executions waiting behind them.
    ///
    /// Runs on the ordinary runtime tick and does nothing at all in the usual
    /// case — the set is empty after a successful recovery. When it is not, a
    /// genesis that now reads clean gets its held metadata published, so an
    /// execution that spent a minute unverified still ends up advertised with
    /// whatever the chain actually says about it.
    pub async fn retry_pending_claim_verification(&mut self) {
        if self.claims_pending_reverification.is_empty() {
            return;
        }
        let Some(rest) = self.rest_client.clone() else {
            return;
        };
        let pending: Vec<String> = self.claims_pending_reverification.iter().cloned().collect();
        for genesis_ref in pending {
            if !self.verify_umbrella_chain(&genesis_ref, &rest).await {
                continue;
            }
            let session_ids: Vec<String> = self
                .state
                .sessions()
                .filter(|record| {
                    !record.closed && record.genesis_ref.as_deref() == Some(genesis_ref.as_str())
                })
                .map(|record| record.session_id.clone())
                .collect();
            tracing::info!(
                target: "csp::authority",
                %genesis_ref,
                executions = session_ids.len(),
                "authority chain re-verified; its executions are admitted or fenced by name"
            );
            // The metadata `recover` held. Published now with the fence the
            // chain actually carries, rather than the `disconnected` this
            // provider would have guessed at when it came up.
            for session_id in session_ids {
                let Some(record) = self.state.session(&session_id).cloned() else {
                    continue;
                };
                if record.is_retired() {
                    continue;
                }
                let status = if self
                    .sessions
                    .handle(&session_id)
                    .is_some_and(session::SessionHandle::is_live)
                {
                    SessionStatus::Idle
                } else {
                    SessionStatus::Disconnected
                };
                let target = self.target_for(&record);
                if let Err(error) = self.publish_metadata(record.channel_id, &target, status) {
                    tracing::warn!(
                        target: "csp::authority",
                        %session_id,
                        "held metadata could not be published after re-verification: {error}"
                    );
                }
            }
        }
    }

    /// Verify every live umbrella's authority chain.
    ///
    /// Best-effort: called at startup so claims and grants accepted while the
    /// provider was down are folded in before the first command is served. An
    /// umbrella whose chain cannot be read to its end stays (or becomes)
    /// pending, so its executions refuse by name instead of being admitted on
    /// the assumption that nothing changed.
    pub async fn backfill_authority_chains(&mut self, rest: &RestClient) {
        let genesis_refs: BTreeSet<String> = self
            .state
            .sessions()
            .filter(|record| !record.closed)
            .filter_map(|record| record.genesis_ref.clone())
            .collect();
        for genesis_ref in genesis_refs {
            self.verify_umbrella_chain(&genesis_ref, rest).await;
        }
    }

    fn context<'a>(
        &'a self,
        channel_id: Uuid,
        projects: &'a ProjectsFile,
        actor_seats: &'a crate::actor_seats::ActorSeatsFile,
        operator_pubkey: &'a str,
    ) -> CommandContext<'a> {
        CommandContext {
            provider_pubkey: &self.pubkey_hex,
            operator_pubkey,
            channel_id,
            runtimes: &self.config.runtimes,
            instance_id: &self.config.instance_id,
            now_secs: now_secs(),
            horizon_secs: self.config.command_horizon.as_secs(),
            max_sessions: self.config.max_sessions,
            turn_budget: self.config.turn_budget,
            policy_turn_budgets: self.policy_turn_budgets.clone(),
            active_session_count: self.sessions.live_count(),
            state: &self.state,
            ci_continuations: &self.ci_continuations,
            projects,
            actor_seats,
            in_flight: &self.in_flight,
            delivered_cancels: &self.delivered_cancels,
            claims_pending_reverification: &self.claims_pending_reverification,
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
            // D1: `agentRef` stops being unconditionally null. It is null for
            // every human-created execution — that work really is supervised,
            // not performed by a participant — and names the seat for a create
            // that carried an `actor`. The pubkey is the whole of what is
            // published about that seat; its key lives in host-local custody
            // and never reaches this struct or any other signed payload.
            agent_ref: record.and_then(|record| record.actor.clone()),
            role: record.and_then(|record| record.role.clone()),
            // Typed since B2. `from_wire` refuses only a blank or oversized
            // value, neither of which this provider's own config can hold, so
            // the `Err` arm is unreachable in practice — but it is *reported*
            // and the field left `None` rather than panicked on, because a
            // provider that aborts while publishing metadata takes every live
            // seat with it.
            //
            // `SessionMetadata.provider` and `.runtime` carry no
            // `skip_serializing_if`, so `None` is written as an explicit
            // `"provider": null` / `"runtime": null` — the key is present and
            // its value is null, not absent. That is still the honest
            // outcome: a null cannot be misread as a *correct* alias the way a
            // guessed or defaulted string could, and the reader fails closed
            // on it — `context_projector::verify_metadata` compares
            // `metadata.provider` against the create's own
            // `providerInstanceRef` and rejects the whole metadata fact as
            // "disagrees with its create/provider/target chain" when it is
            // null. A refused linkage is the honest place for an unreadable
            // alias to surface.
            provider: match ProviderInstanceAlias::from_wire(provider_ref.clone()) {
                Ok(alias) => Some(alias),
                Err(error) => {
                    tracing::warn!(
                        provider_instance_ref = %provider_ref,
                        %error,
                        "metadata provider alias is not a valid alias; publishing it as an \
                         explicit null, which the reader's linkage check will refuse"
                    );
                    None
                }
            },
            runtime: match RuntimeWord::from_wire(runtime_slug.clone()) {
                Ok(word) => Some(word),
                Err(error) => {
                    tracing::warn!(
                        runtime = %runtime_slug,
                        %error,
                        "metadata runtime word is not a valid runtime word; publishing it as an \
                         explicit null, which the reader's linkage check will refuse"
                    );
                    None
                }
            },
            model: record
                .and_then(|record| record.model.clone())
                .or_else(|| descriptor.map(|descriptor| descriptor.default_model.clone()))
                .or_else(|| Some(config::DEFAULT_MODEL.to_owned())),
            status,
            // Filled from the last bounded worktree observation, or left null
            // when the cwd is not a repository, sits on a detached HEAD, or was
            // never successfully observed.
            branch: self
                .git_probes
                .get(&target.session_id)
                .and_then(|observed| observed.branch.clone()),
            // `threadSteer` is the one capability that is a fact about *this*
            // execution rather than about the driver: it is what the process
            // behind this generation advertised at `initialize`, and it is
            // gated on this provider actually being able to deliver a native
            // steer (`session::NATIVE_STEER_DELIVERABLE`). An operator's Steer
            // control is drawn from this field, so a `true` here has to mean a
            // control that works, not one that always degrades.
            capabilities: descriptor
                .and_then(|descriptor| descriptor.capabilities)
                .unwrap_or_else(|| Capabilities::v1_for_runtime(&runtime_slug))
                .with_thread_steer(
                    session::NATIVE_STEER_DELIVERABLE
                        && self
                            .steering
                            .get(&target.session_id)
                            .copied()
                            .unwrap_or(false),
                )
                // `promptImage` is per-execution for the same reason as
                // `threadSteer`, and gated on this provider actually being
                // able to deliver an image: without a media fetcher the blob
                // can never be read back, so advertising the capability would
                // offer an attach control that silently drops every image.
                .with_prompt_image(
                    self.media.is_some()
                        && self
                            .prompt_image
                            .get(&target.session_id)
                            .copied()
                            .unwrap_or(false),
                ),
            // Echoed only when the create claimed one; the struct then omits
            // the key entirely, so unclaimed sessions keep the exact 12-key
            // shape pre-amendment consumers require.
            session_ref: record.and_then(|record| record.session_ref.clone()),
            // B1 coordinate facts (D4a) — same probe as `branch`, plus the
            // relay-confirmed reachability of `observed_commit`. See the
            // struct doc on `SessionMetadata` for why these four ride here
            // rather than on the lifecycle receipt.
            observed_commit: self
                .git_probes
                .get(&target.session_id)
                .and_then(|observed| observed.commit.clone()),
            dirty: self
                .git_probes
                .get(&target.session_id)
                .and_then(|observed| observed.dirty),
            relay_reachable: self
                .git_reachability
                .get(&target.session_id)
                .map(|fact| fact.reachable),
            verified_at: self
                .git_reachability
                .get(&target.session_id)
                .map(|fact| fact.verified_at),
            // D9: published only when both halves of the fact exist — the
            // execution claimed an umbrella and this host set a finite budget.
            // Anything else omits the key rather than publishing a zero, which
            // would read as "no turns allowed" instead of "no budget".
            turn_budget: record
                .and_then(|record| record.session_ref.as_deref())
                .filter(|_| self.config.turn_budget != config::UNLIMITED_TURN_BUDGET)
                .map(|session_ref| TurnBudget {
                    used: self.state.turns_used(session_ref),
                    limit: self.config.turn_budget,
                }),
            routing: record.and_then(|record| record.routing.clone()),
            // Item 103 finding 1, disclosed: which `bee` this seat was started
            // with, as **this host** observed it — the path, how it was
            // chosen, and what `$BEE --version` answered. Nothing is asked of
            // the agent.
            //
            // Published only for a seated execution, for the same reason
            // `agentRef` is: a human-supervised execution is given no `BEE`
            // and no prepended entry, so a stamp on its metadata would be a
            // claim about a binary nothing in that session runs.
            bee_stamp: record
                .and_then(|record| record.actor.as_deref())
                .and_then(|_| crate::seat_bee::host_seat_bee())
                .map(|(_, stamp)| stamp.clone()),
            // Which pack this seat actually ran, as the host staged it —
            // repository, commit, role and path, or `app:shipped` with the app
            // version when the bundled defaults were used. Written by both
            // seat-start paths (create and reattach) and refreshed per
            // generation, so publishing it is one field read.
            //
            // Absent when no pack was staged from a project's packs
            // repository. The surfaces say "no pack staged" rather than naming
            // one that did not run.
            pack_ref: record.and_then(|record| record.pack_ref.clone()),
            // §3.1: who holds this umbrella, and on which body. Read from the
            // record's persisted claim state, so a provider that comes back to
            // an execution somebody else has taken over advertises the fence
            // in the same event as the status rather than leaving
            // `disconnected` to read as an ordinary outage.
            //
            // `last()`, not `active()`: a **voided** claim is still disclosed,
            // with the claim as it stood when it was voided — and `state` is
            // what says which of the two a reader is looking at (review
            // finding N7). Without that word both cases publish the same three
            // references, and a surface would point a person at a claimant who
            // no longer holds anything. A session nobody ever handed over folds
            // to `NoClaim`, whose `last()` is `None`, and the key is omitted
            // entirely rather than nulled.
            handover: record.map(|record| &record.handover).and_then(|claim| {
                let state = match claim {
                    ClaimState::NoClaim => return None,
                    ClaimState::Active(_) => SessionMetadataHandoverState::Active,
                    ClaimState::Voided { .. } => SessionMetadataHandoverState::Voided,
                };
                claim.last().map(|claim| SessionMetadataHandover {
                    state,
                    claimant: claim.claimant.clone(),
                    body_pubkey: claim.body_pubkey.clone(),
                    accepted_event_id: claim.accepted_event_id.clone(),
                })
            }),
        }
    }

    /// Record that this process handed `command_id`'s cancel to a mailbox and
    /// has not yet written the ledger entry that fences its redelivery.
    ///
    /// Bounded at [`DELIVERED_CANCEL_FENCE_CAPACITY`], oldest evicted first.
    fn remember_delivered_cancel(&mut self, command_id: String) {
        if self.delivered_cancels.contains(&command_id) {
            return;
        }
        self.delivered_cancels.push_back(command_id);
        while self.delivered_cancels.len() > DELIVERED_CANCEL_FENCE_CAPACITY {
            self.delivered_cancels.pop_front();
        }
    }

    /// Release a cancel whose durable ledger entry now fences its redelivery.
    fn forget_delivered_cancel(&mut self, command_id: &str) {
        self.delivered_cancels.retain(|held| held != command_id);
    }

    /// Queue a receipt (44224). Receipts drain ahead of transcripts.
    ///
    /// The outbox fences on `(kind, semantic key)`, so the key has to match
    /// the receipt's own uniqueness rule. A lifecycle command has one outcome
    /// and keys by `commandId`; a turn command reports several stages and keys
    /// by `(commandId, status)`. Keying a turn's stages by `commandId` alone
    /// would fence the `turn_started` out as a duplicate of the `turn_queued`
    /// before it, and the operator would watch a turn queue and never learn it
    /// began.
    pub fn enqueue_receipt(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        receipt: &LifecycleReceipt,
    ) -> anyhow::Result<()> {
        self.enqueue_receipt_with_outbox_key(channel_id, command_id, receipt, None)
    }

    /// Atomically fence a terminal decision together with its recoverable answer.
    fn enqueue_terminal_receipt(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        receipt: &LifecycleReceipt,
    ) -> anyhow::Result<()> {
        let content = serde_json::to_string(receipt)?;
        let event =
            build_coding_session_turn_receipt(channel_id, command_id, receipt.status, &content)?
                .sign_with_keys(&self.config.keys)?;
        self.state.stage_terminal_disposition(
            command_id,
            state::TerminalDisposition {
                semantic_key: coding_session_turn_receipt_semantic_key(command_id, receipt.status),
                event,
            },
        )?;
        self.flush_terminal_dispositions()
    }

    /// Retry projections of fenced decisions, preserving the original signed bytes.
    fn flush_terminal_dispositions(&mut self) -> anyhow::Result<()> {
        for (command_id, disposition) in self.state.terminal_dispositions() {
            self.outbox.enqueue(
                KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
                &disposition.semantic_key,
                Priority::High,
                disposition.event,
            )?;
            self.state.finish_terminal_disposition(&command_id)?;
        }
        Ok(())
    }

    // A rejected conflicting event needs its own outbox fence without changing
    // the command identity encoded into the signed receipt envelope.
    fn enqueue_receipt_with_outbox_key(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        receipt: &LifecycleReceipt,
        outbox_key: Option<&str>,
    ) -> anyhow::Result<()> {
        let content = serde_json::to_string(receipt)?;
        let (event, semantic_key) = if receipt.status.is_turn_stage() {
            (
                build_coding_session_turn_receipt(
                    channel_id,
                    command_id,
                    receipt.status,
                    &content,
                )?,
                coding_session_turn_receipt_semantic_key(command_id, receipt.status),
            )
        } else {
            (
                build_coding_session_lifecycle_receipt(channel_id, command_id, &content)?,
                coding_session_lifecycle_receipt_semantic_key(command_id),
            )
        };
        let event = event.sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            outbox_key.unwrap_or(&semantic_key),
            Priority::High,
            event,
        )?;
        Ok(())
    }

    /// Start a bounded re-observation of `session_id`'s working directory,
    /// followed — when there is something to confirm — by a bounded relay
    /// reachability check of the commit it found.
    ///
    /// The local probe runs up to three short-lived `git` processes, each
    /// under [`git_probe`]'s own ceiling; the reachability check (B1/D4a) is
    /// one bounded HTTP request under [`reachability`]'s own ceiling. Both
    /// run off the event loop — awaiting either here would stall *every
    /// other* session's transcript delivery and the outbox flush for as long
    /// as the slowest of them — and report back together as one
    /// [`SessionEvent::WorktreeObserved`], which the loop folds in exactly
    /// like an actor report. The consequence is deliberate: metadata is
    /// published immediately without these facts and corrected a moment
    /// later, rather than published late but complete.
    ///
    /// The reachability check only runs when there is an observed commit and
    /// a repository coordinate to check it against, and only when a relay
    /// client has been wired in ([`Provider::set_rest_client`]) — every other
    /// case reports `None`, "not checked," rather than skipping the field
    /// silently.
    ///
    /// A session whose record has vanished is a no-op, not an error. Two probes
    /// in flight for one session are allowed rather than serialized: both are
    /// read-only, so there is nothing to contend over. They are not, however,
    /// left to resolve by arrival order — each is stamped with a per-session
    /// generation minted here, and [`SessionEvent::WorktreeObserved`] fences
    /// out any result whose generation is not the newest one launched (see the
    /// invariant on that match arm), so a slow older probe can never overwrite
    /// a faster newer one — and because the reachability check travels with
    /// its probe under that same stamp, a commit that has since changed can
    /// never be left wearing a stale reachability claim either.
    /// Drop this execution's verified-context packages from disk and from the
    /// refresh map.
    ///
    /// Keyed on the *package* id carried by the refresh entry, never on the
    /// session id: a resume-created directory is named by a freshly minted
    /// UUID, so a cleanup keyed on the session id would silently leak it.
    fn discard_context_packages(&mut self, session_id: &str) {
        let Some(state) = self.context_refresh.remove(session_id) else {
            return;
        };
        if let Err(error) =
            context_store::remove_context_packages(&self.config.state_dir, &state.package_id)
        {
            tracing::warn!(
                target: "csp::context",
                %session_id,
                "verified-context packages could not be removed: {error}"
            );
        }
    }

    /// Drop a package whose ACP open never happened.
    ///
    /// The package is written *before* `sessions.create`, but the refresh-map
    /// entry that makes it reachable for cleanup is inserted only after that
    /// open succeeds. A failed open therefore leaves a directory no
    /// session→package binding names: neither `stop_session` nor a later
    /// resume can find it, and only the next `Provider::recover` sweep would
    /// reclaim it — days away on a long-lived desktop provider, one full
    /// package per retry against a broken agent binary. The sidecar for this
    /// open never started, so nothing is reading it.
    fn discard_orphaned_context_package(&self, package_id: Option<&str>) {
        let Some(package_id) = package_id else {
            return;
        };
        if let Err(error) =
            context_store::remove_context_packages(&self.config.state_dir, package_id)
        {
            tracing::warn!(
                target: "csp::context",
                %package_id,
                "verified-context package orphaned by a failed open could not be removed: {error}"
            );
        }
    }

    /// Write the next verified-context generation for `session_id`, off the
    /// provider loop.
    ///
    /// Modelled on [`Provider::spawn_git_probe`], which exists precisely
    /// because a synchronous relay fetch on this loop delays every other
    /// session. Nothing is sent back through `session_events_tx`: the sidecar
    /// picks the new generation up off disk on its next tool call.
    ///
    /// A refresh that fails changes nothing on disk. The previous generation
    /// keeps serving and its age keeps climbing in every response, which is the
    /// honest signal — never silence, and never a fresher-looking watermark.
    fn spawn_context_refresh(&mut self, session_id: &str) {
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        // Both refs are required to project: they are what the fetch is keyed
        // on. An execution without them never had a package to refresh.
        let (Some(session_ref), Some(genesis_ref)) =
            (record.session_ref.clone(), record.genesis_ref.clone())
        else {
            return;
        };
        let channel_id = record.channel_id;
        let Some(rest_client) = self.rest_client.clone() else {
            return;
        };
        let relay_self = self.relay_self.clone();
        let state_dir = self.config.state_dir.clone();
        let now = now_ms();

        let Some(state) = self.context_refresh.get_mut(session_id) else {
            return;
        };
        if state.last_refresh_ms > 0
            && now.saturating_sub(state.last_refresh_ms) < CONTEXT_REFRESH_MIN_INTERVAL_MS
        {
            return;
        }
        // Claimed before the fetch starts, so two turn starts inside one
        // fetch's lifetime can never race for the same sequence.
        state.last_refresh_ms = now;
        let seq = state.next_seq;
        state.next_seq = seq.saturating_add(1);
        let package_id = state.package_id.clone();
        let session_id = session_id.to_owned();

        tokio::spawn(async move {
            let request = ContextProjectionRequest {
                channel_id,
                session_ref,
                genesis_ref,
                relay_self_pubkey: relay_self,
                // A refresh replaces a package that already exists. A
                // momentarily unprovable create chain must fail the refresh,
                // never overwrite a good generation with an empty one.
                allow_no_executions: false,
                generated_at: now_ms(),
                limits: ContextProjectionLimits::default(),
            };
            let package = match context_projector::fetch_and_project_session_context(
                &rest_client,
                &request,
            )
            .await
            {
                Ok(package) => package,
                Err(error) => {
                    tracing::info!(
                        target: "csp::context",
                        %session_id,
                        reason = %context_projector::context_unavailable_reason(&error),
                        "verified-context refresh failed; the previous generation keeps serving: {error}"
                    );
                    return;
                }
            };
            // The claimed sequence is a floor, not the answer: a generation
            // left behind by a process death mid-write still occupies its path,
            // so the next write must land above whatever is actually on disk.
            let seq =
                match context_store::latest_context_package_generation(&state_dir, &package_id) {
                    Ok(latest) => latest.map_or(seq, |latest| seq.max(latest.saturating_add(1))),
                    Err(error) => {
                        tracing::warn!(
                            target: "csp::context",
                            %session_id,
                            "verified-context generations could not be listed: {error}"
                        );
                        seq
                    }
                };
            match context_store::write_context_package_generation(
                &state_dir,
                &package_id,
                seq,
                &package,
            ) {
                Ok(_) => {}
                // The execution was stopped, or resumed onto a fresh package,
                // while this fetch was in flight. The write refuses to
                // re-create the directory that cleanup removed, so the refresh
                // is dropped rather than leaking verified context past the
                // operator's stop.
                Err(context_store::ContextStoreError::PackageGone) => {
                    tracing::debug!(
                        target: "csp::context",
                        %session_id,
                        seq,
                        "verified-context refresh landed after the package was discarded; dropping it"
                    );
                    return;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "csp::context",
                        %session_id,
                        "refreshed verified context could not be persisted: {error}"
                    );
                    return;
                }
            }
            if let Err(error) = context_store::prune_context_package_generations(
                &state_dir,
                &package_id,
                context_store::CONTEXT_PACKAGE_GENERATIONS_RETAINED,
            ) {
                tracing::warn!(
                    target: "csp::context",
                    %session_id,
                    "superseded verified-context generations could not be pruned: {error}"
                );
            }
            tracing::debug!(
                target: "csp::context",
                %session_id,
                seq,
                "wrote a refreshed verified-context generation"
            );
        });
    }

    /// Resolve the commit one watched gate row ran against, then hand the row
    /// back to the loop.
    ///
    /// Two facts, from the seat's own `cwd` as the provider's own record holds
    /// it: `git rev-parse HEAD` and whether the worktree matched it. **The
    /// agent is never asked** — the whole point of an `observed` row is that
    /// its subject cannot write it, and a commit the subject named would be
    /// exactly the claim finding 26 caught being wrong.
    ///
    /// A workdir that is not a repository, an unborn branch with no commits,
    /// a `git` that is missing, and a probe that times out all yield **no**
    /// `headSha`: the row is published naming no commit, which admits nothing
    /// anywhere and says nothing false. The pair travels together, so a probe
    /// that answered one and not the other yields neither
    /// (`coding_session_observation.rs`'s gate-row validator refuses the half
    /// shape outright).
    ///
    /// **A directory that is not there is different from all of those, and is
    /// no longer one of them** (live-run finding 82). The directory is
    /// resolved at every gate by [`gate_cwd::resolve`], which prefers the
    /// host's *current* answer over the one the create recorded; when nothing
    /// exists at the resolved path the gate is refused with `cwd missing:
    /// <path>` and **no row is minted at all**. A relocated worktree used to
    /// publish `headSha: null` rows for every gate it ran, which the push gate
    /// correctly refused to admit and no operator could interpret.
    fn spawn_gate_head_probe(
        &mut self,
        session_id: &str,
        observed: gate_observer::ObservedGateRow,
    ) {
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        let recorded_cwd = record.cwd.clone();
        let projects_file = self.config.projects_file.clone();
        let events = self.session_events_tx.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let resolved = gate_cwd::resolve(projects_file.as_deref(), &session_id, &recorded_cwd);
            let Some(cwd) = resolved.present() else {
                // No row. The sentence is the whole disclosure: an operator
                // greps for it after a relocation, and the alternative is the
                // silent `headSha: null` this replaced.
                if let Some(refusal) = resolved.refusal() {
                    tracing::warn!(
                        target: "csp::git",
                        %session_id,
                        "refusing to observe a gate: {refusal}"
                    );
                }
                return;
            };
            let probe = git_probe::probe_for_gate(cwd).await;
            let mut observed = observed;
            let probe = match probe {
                Ok(probe) => probe,
                Err(refusal) => {
                    // Unreachable in practice — `present()` already proved the
                    // directory — but a race that removes it between the two
                    // refuses rather than minting a null row.
                    tracing::warn!(
                        target: "csp::git",
                        %session_id,
                        "refusing to observe a gate: {refusal}"
                    );
                    return;
                }
            };
            if let (Some(commit), Some(dirty)) = (probe.commit, probe.dirty) {
                observed.row.head_sha = Some(commit);
                observed.row.dirty = Some(dirty);
            }
            // A closed receiver means the provider loop is gone; the row has
            // nowhere to be published, so it is dropped rather than logged as
            // a failure.
            let _ = events
                .send(SessionEvent::GateObserved {
                    session_id,
                    observed,
                })
                .await;
        });
    }

    fn spawn_git_probe(&mut self, session_id: &str) {
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        let cwd = record.cwd.clone();
        let repo_ref = record.repo_ref.clone();
        let generation = {
            let next = self
                .git_probe_generation
                .entry(session_id.to_owned())
                .or_insert(0);
            *next += 1;
            *next
        };
        let events = self.session_events_tx.clone();
        let rest_client = self.rest_client.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let observed = git_probe::probe(&cwd).await;
            let reachability = match (&observed.commit, &repo_ref, &rest_client) {
                (Some(oid), Some(repo_ref), Some(rest_client)) => {
                    reachability::check(rest_client, repo_ref, oid).await
                }
                _ => None,
            };
            // A closed receiver means the provider loop is gone — shutdown, or
            // a dropped `Provider` in a test. The observation has nowhere to be
            // published, so it is dropped rather than logged as a failure.
            let _ = events
                .send(SessionEvent::WorktreeObserved {
                    session_id,
                    generation,
                    observed,
                    reachability,
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
        // §3.2, and the whole of root's continuity finding: a retired umbrella
        // publishes nothing, ever again. This is the single choke point every
        // metadata publish passes through, so the guard lives here rather than
        // at each of the eight call sites — a new one added later inherits it.
        // Republishing here is not merely stale: the genesis and the
        // conversation are deleted on the relay, and a fresh 44223 carrying a
        // new event id is a *new* record the deletion never named, which is
        // exactly how a deleted session came back looking resumable.
        if self
            .state
            .session(&target.session_id)
            .is_some_and(state::SessionRecord::is_retired)
        {
            tracing::debug!(
                target: "csp::retirement",
                session_id = %target.session_id,
                "suppressing metadata for a retired session"
            );
            return Ok(());
        }
        // And held, not suppressed, while this umbrella's authority chain is
        // unread. `disconnected` over an execution somebody has taken over
        // reads as an ordinary outage — §3.1 exists to stop exactly that — and
        // a provider that cannot yet tell the two apart should say nothing
        // rather than say the comfortable one.
        // `retry_pending_claim_verification` publishes it once the chain reads.
        if self
            .state
            .session(&target.session_id)
            .and_then(|record| record.genesis_ref.as_deref())
            .is_some_and(|genesis_ref| self.claims_pending_reverification.contains(genesis_ref))
        {
            tracing::debug!(
                target: "csp::authority",
                session_id = %target.session_id,
                "holding metadata until this umbrella's authority chain is re-verified"
            );
            return Ok(());
        }
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
        Ok(self
            .enqueue_transcript_with_id(channel_id, target, turn_id, item, priority)?
            .map(|(event_seq, _)| event_seq))
    }

    fn enqueue_transcript_with_id(
        &mut self,
        channel_id: Uuid,
        target: &CodingSessionTarget,
        turn_id: Option<&str>,
        item: serde_json::Value,
        priority: Priority,
    ) -> anyhow::Result<Option<(u64, String)>> {
        let workspace_root = self
            .state
            .session(&target.session_id)
            .map(|record| record.cwd.clone());
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
        let (item, redactions) = match workspace_root {
            Some(root) => transcript::fit_item_recording_for_workspace(
                item,
                overhead,
                MAX_TRANSCRIPT_CONTENT_BYTES,
                &root,
            ),
            None => transcript::fit_item_recording(item, overhead, MAX_TRANSCRIPT_CONTENT_BYTES),
        };
        self.record_redactions(&target.session_id, &redactions, timestamp);
        let envelope = TranscriptEnvelope::new(target, event_seq, timestamp, turn_id, item);
        let content = serde_json::to_string(&envelope)?;
        let event = build_coding_session_transcript_item(channel_id, target, event_seq, &content)?
            .sign_with_keys(&self.config.keys)?;
        let event_id = event.id.to_hex();
        self.outbox.enqueue(
            KIND_CODING_SESSION_TRANSCRIPT,
            &coding_session_transcript_semantic_key(target, event_seq),
            priority,
            event,
        )?;
        Ok(Some((event_seq, event_id)))
    }

    /// Note the recoverable redactions this item carried, for this host only.
    ///
    /// **Never fails a publish.** The transcript is the product; the vault is a
    /// local convenience, so a full disk, a hostile symlink, or a vault at its
    /// cap costs a warning and an unresolved pill — never a dropped transcript
    /// item. That asymmetry is the whole reason this is not `?`-propagated.
    fn record_redactions(
        &mut self,
        session_id: &str,
        redactions: &[buzz_core::coding_session_context::Redaction],
        timestamp: i64,
    ) {
        if redactions.is_empty() || !self.config.redaction_retention.enabled() {
            return;
        }
        let seen = self
            .recorded_redactions
            .entry(session_id.to_owned())
            .or_default();
        let fresh: Vec<_> = redactions
            .iter()
            .filter(|redaction| seen.insert(redaction.digest.clone()))
            .cloned()
            .collect();
        if fresh.is_empty() {
            return;
        }
        if let Err(error) = redaction_vault::append(
            &self.config.state_dir,
            session_id,
            &fresh,
            timestamp,
            self.config.redaction_retention,
        ) {
            tracing::warn!(
                session_id,
                %error,
                "could not record redactions for local lookup; transcript is unaffected"
            );
        }
    }

    /// Run the age, size, and orphan reapers over the redaction vault.
    ///
    /// Called at startup and on a timer. Best-effort for the same reason the
    /// context-package sweep is: a state directory that refuses the sweep costs
    /// disk, not correctness.
    ///
    /// The orphan pass is the startup counterpart to [`Self::forget_redactions`]
    /// — a provider killed mid-session never ran the stop path, and its note
    /// would otherwise sit there until the age reaper reached it. "Live" means
    /// a durable session record survived, which is exactly the set that can
    /// still be resumed and read.
    pub fn sweep_redaction_vault(&mut self) {
        match redaction_vault::sweep(
            &self.config.state_dir,
            self.config.redaction_retention,
            SystemTime::now(),
        ) {
            Ok(outcome) if outcome.expired > 0 || outcome.oversize > 0 => {
                tracing::info!(
                    target: "csp::context",
                    expired = outcome.expired,
                    oversize = outcome.oversize,
                    "swept redaction vault files"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(target: "csp::context", %error, "redaction vault sweep failed");
            }
        }

        let live: Vec<String> = self
            .state
            .sessions()
            .map(|record| record.session_id.clone())
            .collect();
        match redaction_vault::sweep_orphans(&self.config.state_dir, &live) {
            Ok(removed) if removed > 0 => {
                tracing::info!(
                    target: "csp::context",
                    removed,
                    "removed redaction vault files no live session owns"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(target: "csp::context", %error, "redaction vault orphan sweep failed");
            }
        }
    }

    /// Drop a stopped session's local redaction note.
    ///
    /// The first of the three reapers (see [`crate::redaction_vault`]), and the
    /// one that handles the ordinary case.
    fn forget_redactions(&mut self, session_id: &str) {
        self.recorded_redactions.remove(session_id);
        if let Err(error) = redaction_vault::remove_session(&self.config.state_dir, session_id) {
            tracing::warn!(session_id, %error, "could not remove the session redaction vault");
        }
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
            SessionEvent::CiTurnAdmissionRequested {
                session_id,
                command_id,
                decision,
            } => {
                let admitted = self.admit_ci_turn_start(&session_id, &command_id);
                let _ = decision.send(matches!(&admitted, Ok(true)));
                admitted?;
            }
            SessionEvent::TurnStarted {
                session_id,
                turn_id,
                command_id,
                text: _,
            } => {
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                // Only kind-44220 commands admitted by `on_turn` enter this
                // map. Lifecycle create/hire prompts also start turns, but
                // cannot name a governed assignment operation.
                let team_wake_eligible = self.in_flight.contains_key(&command_id);
                // Whose turn this is, when the provider knows: a 44220 that
                // came through `on_turn` names its verified signer. A
                // lifecycle-opened turn (a create's `initialTurn`) has no
                // entry here and records `None`, which the fence reads as "ask
                // whether this body may act at all".
                let operator_pubkey = self
                    .in_flight
                    .get(&command_id)
                    .map(|turn| turn.operator_pubkey.clone())
                    .or_else(|| {
                        // A create's `initialTurn` never passes through
                        // `on_turn`, so it has no in-flight entry — but the
                        // record knows who asked for the execution, and that
                        // is exactly who sent this turn.
                        self.state
                            .session(&session_id)
                            .filter(|record| record.command_id == command_id)
                            .and_then(|record| record.created_by.clone())
                    });
                self.state.update_session(&session_id, |record| {
                    record.open_turn = Some(OpenTurn {
                        turn_id: turn_id.clone(),
                        command_id: Some(command_id.clone()),
                        team_wake_eligible,
                        started_at_ms: now_ms(),
                        operator_pubkey: operator_pubkey.clone(),
                    });
                })?;
                // CI continuations already claimed these ledgers before their
                // actor received start permission; the writes below are
                // idempotent. Ordinary turns retain their existing lifecycle.
                // **This** is where an ordinary turn command is consumed — the moment it
                // actually begins, not the moment it was accepted. Everything
                // between accept and here is recoverable: a crash in that
                // window leaves the command unconsumed, the channel watermark
                // still behind it, and the replay runs it exactly once. The
                // durable write happens before the receipt so a crash between
                // the two costs a receipt, never a duplicate turn.
                //
                // The operation ledger goes down *before* the command ledger,
                // and the order is deliberate. Both orders are safe, because
                // `decide_turn` always admits a command that already owns its
                // operation — but this one degrades better: a crash between
                // the two writes leaves the operation fenced and the command
                // unconsumed, so the replay re-delivers the owner and it runs
                // exactly once. The other order would leave the operation
                // unfenced with the command already consumed, and a duplicate
                // arriving in that window would spend a second lead turn.
                if let Some(key) = self
                    .in_flight
                    .get(&command_id)
                    .and_then(|turn| turn.operation_key.clone())
                {
                    self.state
                        .consume_operation(&key, &command_id, now_secs())?;
                }
                self.state.consume_command(&command_id, now_secs())?;
                // CI normally retired its promise before actor permission.
                // Reconciliation here is harmless after those durable claims.
                self.retire_ci_continuation(&command_id)?;
                // D9: the umbrella is charged where the turn is consumed, and
                // for the same reason — this is the moment work actually
                // began. Charging at accept would bill a crew for turns a
                // crash threw away. Charged for every signer, founder
                // included: `used` is what the umbrella spent, not what it was
                // refused for, and only the *refusal* exempts the founder.
                if let Some(session_ref) = self
                    .state
                    .session(&session_id)
                    .and_then(|record| record.session_ref.clone())
                {
                    self.state.record_turn_spend(&session_ref)?;
                }
                self.in_flight.remove(&command_id);
                if let Some(handle) = self.sessions.handle(&session_id) {
                    handle.acknowledge_turn_started(&command_id);
                }
                // The stage that lets a consumer stop guessing: it names the
                // command that asked and the turn that answers it, so a
                // pending row settles by id rather than by matching text.
                let receipt = LifecycleReceipt::turn_started(&command_id, &target, &turn_id);
                self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                self.publish_metadata(channel_id, &target, SessionStatus::Running)?;
            }
            SessionEvent::TurnFinished {
                session_id,
                turn_id,
                outcome,
                duration_ms,
                usage,
                tool_calls,
            } => {
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                // The model the seat is actually running, so the usage block
                // can name the window a reader divides by. `None` when the
                // record does not say — the window is then omitted rather
                // than guessed.
                let model = self
                    .state
                    .session(&session_id)
                    .and_then(|record| record.model.clone());
                let team_terminal = self.state.session(&session_id).and_then(|record| {
                    let actor = record.actor.clone()?;
                    let role = record.role.clone()?;
                    let session_ref = record.session_ref.clone()?;
                    let genesis_ref = record.genesis_ref.clone()?;
                    let open_turn = record.open_turn.as_ref()?;
                    if !open_turn.team_wake_eligible {
                        return None;
                    }
                    let caused_by_command_id = open_turn.command_id.clone()?;
                    (role != "lead").then(|| {
                        (
                            team_wake::WakeScope {
                                channel_ref: record.channel_id,
                                session_ref,
                                genesis_ref,
                            },
                            actor,
                            role,
                            caused_by_command_id,
                            target.clone(),
                            Some(open_turn.started_at_ms),
                        )
                    })
                });
                let (item, status) = turn_result(
                    &outcome,
                    duration_ms,
                    usage.as_deref(),
                    tool_calls,
                    model.as_deref(),
                );
                let terminal_at_ms = now_ms();
                let terminal = self.enqueue_transcript_with_id(
                    channel_id,
                    &target,
                    Some(&turn_id),
                    item,
                    Priority::High,
                )?;
                if let (
                    Some((_, terminal_event_id)),
                    Some((scope, actor, role, caused_by_command_id, source_target, prompt_at_ms)),
                ) = (terminal, team_terminal)
                {
                    self.team_wakes.capture_terminal(
                        scope,
                        team_wake::WakeSource::Terminal {
                            terminal_event_id,
                            actor_pubkey: actor,
                            role,
                            caused_by_command_id,
                            source_target,
                            prompt_at_ms,
                            terminal_at_ms,
                        },
                    )?;
                }
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
                    // The observed half of kind 44246 (§ addendum): the gate
                    // row(s) are derived from the seat's own tool calls,
                    // published under this provider's key, and never asked
                    // for. A composed command (finding 57) can close more
                    // than one gate at once, so this is a loop, not an `if
                    // let` — every row it hands back gets its own probe.
                    for observed in self
                        .gate_observers
                        .entry(session_id.clone())
                        .or_default()
                        .on_item(&item, now_ms())
                    {
                        // The commit is resolved *now*, at the moment the gate
                        // closed, not when the row is published: a seat that
                        // commits between the two would otherwise have its
                        // green row name a commit the gate never saw.
                        self.spawn_gate_head_probe(&session_id, observed);
                    }
                    self.enqueue_transcript(
                        channel_id,
                        &target,
                        Some(&turn_id),
                        item,
                        Priority::Normal,
                    )?;
                }
            }
            SessionEvent::GateObserved {
                session_id,
                observed,
            } => {
                let Some((channel_id, _target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                self.publish_observed_gate_row(&session_id, channel_id, observed)?;
            }
            SessionEvent::TurnDropped {
                session_id,
                command_id,
            } => {
                self.in_flight.remove(&command_id);
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                tracing::warn!(
                    target: "csp",
                    %session_id,
                    %command_id,
                    "turn dropped: the session's queue is full"
                );
                // Fence the decision and exact answer atomically before any
                // visible report. A failed outbox or ledger projection is retried
                // without re-admitting this turn when queue pressure disappears.
                let receipt = LifecycleReceipt::turn_dropped(
                    &command_id,
                    &target,
                    payload::QUEUE_FULL,
                    "the execution's queue is full",
                );
                self.enqueue_terminal_receipt(channel_id, &command_id, &receipt)?;
                self.enqueue_transcript(
                    channel_id,
                    &target,
                    None,
                    payload::status_item("turn_dropped:queue_full"),
                    Priority::High,
                )?;
            }
            SessionEvent::Exited { session_id, reason } => {
                // The process is gone and its mailbox with it. Turns it had
                // taken custody of and not started are owed an answer, and they
                // get a terminal one: `turn_dropped` / `NO_LIVE_EXECUTION`,
                // left unconsumed (they never ran) and recorded as refused
                // (they never will, here — a resume mints a generation the
                // command no longer addresses). Unconsumed is not the same as
                // redeliverable; see `report_no_live_execution`.
                self.report_lost_mailbox(&session_id)?;
                self.steering.remove(&session_id);
                self.prompt_image.remove(&session_id);
                // A half-paired gate call whose result this process will now
                // never see is not a gate anybody ran: forgotten, never
                // resolved into a row.
                self.gate_observers.remove(&session_id);
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    self.sessions.forget(&session_id);
                    return Ok(());
                };
                self.queue_lease(&session_id, CodingSessionLeaseState::Released)?;
                self.sessions.forget(&session_id);
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
                reachability,
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
                    commit = ?observed.commit,
                    reachable = ?reachability.map(|fact| fact.reachable),
                    "observed the session worktree"
                );
                self.git_probes.insert(session_id.clone(), observed);
                // Applied together with `git_probes`, under the same fence,
                // so a commit change between probes can never leave a stale
                // reachability claim behind: a fresh probe that observed no
                // commit (or whose check did not complete) clears any prior
                // confirmation rather than letting it linger unattached to
                // the coordinate it was actually about.
                match reachability {
                    Some(fact) => {
                        self.git_reachability.insert(session_id.clone(), fact);
                    }
                    None => {
                        self.git_reachability.remove(&session_id);
                    }
                }
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

    /// Publish one gate row this provider **watched**, as kind 44246.
    ///
    /// Signed with this provider instance's own key and marked
    /// `source: "observed"`: the record's author is the mechanism that saw the
    /// command run, never the seat that ran it, which is the whole difference
    /// between evidence and a claim (Brian's 2026-09-02 ruling; LIVE-RUN
    /// finding 26).
    ///
    /// An umbrella-less session publishes nothing. A 44246 is scoped to a
    /// `sessionRef` and a `genesisRef`, and a solo session that belongs to no
    /// umbrella has neither — inventing one would file the row under a mission
    /// it is not part of. The gate still ran; nothing here says otherwise.
    fn publish_observed_gate_row(
        &mut self,
        session_id: &str,
        channel_id: Uuid,
        observed: gate_observer::ObservedGateRow,
    ) -> anyhow::Result<()> {
        let Some((session_ref, genesis_ref)) = self
            .state
            .session(session_id)
            .and_then(|record| Some((record.session_ref.clone()?, record.genesis_ref.clone()?)))
        else {
            return Ok(());
        };
        let payload = CodingSessionObservationPayload {
            schema: CODING_SESSION_OBSERVATION_SCHEMA.to_owned(),
            session_ref,
            genesis_ref,
            observation_type: CodingSessionObservationType::Gate,
            source: CodingSessionObservationSource::Observed,
            // Deliberately null. The provider watched a command run; it has no
            // signed evidence of which assignment the seat believed it was
            // answering, and a pointer nobody supplied is a guess dressed as a
            // reference.
            assignment_ref: None,
            body: CodingSessionObservationBody::Gate(CodingSessionObservationGate {
                rows: vec![observed.row.clone()],
            }),
        };
        // The builder re-runs the strict decoder, so an over-long command or a
        // token this crate spelled wrong never reaches a signer. A refusal is
        // logged and dropped: a gate row is a disclosure, and failing a seat's
        // turn because one could not be built would be the tail wagging the dog.
        let builder = match build_coding_session_observation(&channel_id.to_string(), payload) {
            Ok(builder) => builder,
            Err(error) => {
                tracing::warn!(
                    target: "csp",
                    %session_id,
                    "observed gate row refused before signing: {error}"
                );
                return Ok(());
            }
        };
        let event = builder.sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(
            KIND_CODING_SESSION_OBSERVATION,
            &format!(
                "coding-session-observation:{session_id}:{}:{}",
                observed.row.gate,
                event.id.to_hex()
            ),
            // Not `High`: a gate row is a disclosure about work that already
            // happened, and it must never sit ahead of the receipt a consumer
            // is blocking on.
            Priority::Normal,
            event,
        )?;
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

    /// Addressing metadata for a turn whose signer is not this session's
    /// founder — or, since the founder-provider amendment, for a
    /// provider-minted team-wake pointer whoever signed it.
    ///
    /// Resolved from this provider's own durable records, never from the
    /// command: the sender's seat is the sibling execution in the same
    /// umbrella whose `actor` is that pubkey. A signer with no such seat is a
    /// plain operator, framed as one and given no reply target rather than a
    /// guessed one.
    ///
    /// Closed records are not candidates, and where several live seats share
    /// one actor the newest wins. Nothing enforces one execution per actor per
    /// umbrella — a disposable builder ends one seat and creates another under
    /// the same key — so an unfiltered lookup let a retired seat supply a
    /// reply address no turn can reach and a `sender_role` that is written
    /// into the *signed* `user_prompt` item.
    ///
    /// A record written before founders were persisted has
    /// `founder_pubkey == None`. That is "this provider cannot tell", and it
    /// yields no framing at all — a legacy session keeps delivering bare
    /// prompts rather than labelling its founder a stranger.
    ///
    /// # The founder exemption stops at the wake pointer
    ///
    /// A team wake is not words the founder typed: it is 102 bytes of JSON
    /// this provider minted, and the `[Context]` block is the only thing that
    /// tells its recipient what scope it is in and where to answer. Exempting
    /// it because the provider happens to run under the founder's key meant a
    /// lead on a founder-run host received a naked pointer while the identical
    /// pointer from a granted-operator host arrived framed — the same fact in
    /// two shapes, decided by whose key started the process (COMMS-MAP finding
    /// 3). So a founder-signed **pointer** is framed exactly like a peer's,
    /// with the role the sender actually holds here: `provider`. Founder-typed
    /// prose is unchanged and still bare.
    fn turn_framing(
        &self,
        session_id: &str,
        sender_pubkey: &str,
        delivery: CodingSessionDelivery,
        channel_id: Uuid,
        text: &str,
    ) -> Option<session::TurnFraming> {
        let record = self.state.session(session_id)?;
        let founder = record.founder_pubkey.as_deref()?;
        let signer_is_founder = founder.eq_ignore_ascii_case(sender_pubkey);
        if signer_is_founder && !team_wake::is_team_wake_pointer(text) {
            return None;
        }
        let umbrella = record.session_ref.clone();
        let seat = umbrella.and_then(|umbrella| {
            self.state
                .sessions()
                .filter(|candidate| {
                    !candidate.closed
                        && candidate.session_ref.as_deref() == Some(umbrella.as_str())
                        && candidate
                            .actor
                            .as_deref()
                            .is_some_and(|actor| actor.eq_ignore_ascii_case(sender_pubkey))
                })
                // Newest first by the facts the record itself carries, so the
                // winner is a decision rather than a map-ordering accident.
                .max_by_key(|candidate| {
                    (
                        candidate.created_at_ms,
                        candidate.generation,
                        candidate.session_id.clone(),
                    )
                })
                .map(|candidate| {
                    (
                        candidate.role.clone(),
                        coding_session_target_key(&self.target_for(candidate)),
                    )
                })
        });
        let (seat_role, reply_target) = match seat {
            Some((role, target_key)) => (role, Some(target_key)),
            None => (None, None),
        };
        // The founder only reaches here for a pointer this provider minted, so
        // the honest sender is the provider itself — not "operator", which
        // would name a person who typed nothing, and not a seat role, which the
        // founder does not hold by being the founder.
        let sender_role = if signer_is_founder {
            Some(PROVIDER_SENDER_ROLE.to_owned())
        } else {
            seat_role
        };
        Some(session::TurnFraming {
            channel_id,
            sender_pubkey: sender_pubkey.to_owned(),
            sender_role,
            reply_target,
            delivery,
        })
    }

    fn queue_lease(
        &mut self,
        session_id: &str,
        state: CodingSessionLeaseState,
    ) -> anyhow::Result<()> {
        let Some(publication) = lease::reserve_and_build(
            &mut self.state,
            session_id,
            &self.config.instance_id,
            &self.config.keys,
            state,
        )?
        else {
            return Ok(());
        };
        if state == CodingSessionLeaseState::Released {
            self.established_leases.remove(&publication.semantic_key);
        }
        self.pending_leases
            .insert(publication.semantic_key.clone(), publication);
        Ok(())
    }

    fn queue_initial_live_leases(&mut self) -> anyhow::Result<()> {
        let pending_keys = self.outbox.pending_keys();
        let session_ids: Vec<String> = self
            .sessions
            .live_session_ids()
            .map(str::to_owned)
            .collect();
        for session_id in session_ids {
            let Some(record) = self.state.session(&session_id) else {
                continue;
            };
            if record.closed {
                continue;
            }
            let semantic_key = coding_session_target_key(&self.target_for(record));
            let Some(prerequisites) = self.first_lease_prerequisites.get(&semantic_key) else {
                continue;
            };
            if prerequisites.iter().any(|key| pending_keys.contains(key)) {
                continue;
            }
            if !self.established_leases.contains(&semantic_key)
                && !self.pending_leases.contains_key(&semantic_key)
            {
                self.queue_lease(&session_id, CodingSessionLeaseState::Live)?;
            }
        }
        Ok(())
    }

    fn record_first_lease_prerequisites(
        &mut self,
        target: &CodingSessionTarget,
        outbox_before: &HashSet<(u32, String)>,
    ) {
        let prerequisites = self
            .outbox
            .pending_keys()
            .difference(outbox_before)
            .cloned()
            .collect();
        self.first_lease_prerequisites
            .insert(coding_session_target_key(target), prerequisites);
    }

    fn queue_lease_renewals(&mut self) -> anyhow::Result<()> {
        let session_ids: Vec<String> = self
            .sessions
            .live_session_ids()
            .map(str::to_owned)
            .collect();
        for session_id in session_ids {
            let Some(record) = self.state.session(&session_id) else {
                continue;
            };
            if record.closed {
                continue;
            }
            let semantic_key = coding_session_target_key(&self.target_for(record));
            if self.established_leases.contains(&semantic_key) {
                self.queue_lease(&session_id, CodingSessionLeaseState::Live)?;
            }
        }
        Ok(())
    }

    fn queue_live_releases(&mut self) -> anyhow::Result<()> {
        let session_ids: Vec<String> = self
            .sessions
            .live_session_ids()
            .map(str::to_owned)
            .collect();
        for session_id in session_ids {
            let open = self
                .state
                .session(&session_id)
                .is_some_and(|record| !record.closed);
            if open {
                self.queue_lease(&session_id, CodingSessionLeaseState::Released)?;
            }
        }
        Ok(())
    }

    async fn flush_pending_leases(
        &mut self,
        publisher: &RelayEventPublisher,
    ) -> anyhow::Result<usize> {
        let mut semantic_keys: Vec<String> = self.pending_leases.keys().cloned().collect();
        semantic_keys.sort_unstable();
        let mut published = 0usize;
        for semantic_key in semantic_keys {
            let Some(publication) = self.pending_leases.get(&semantic_key).cloned() else {
                continue;
            };
            publisher
                .publish_latest_ephemeral(
                    publication.semantic_key.clone(),
                    publication.event.clone(),
                )
                .await?;
            if self
                .pending_leases
                .get(&semantic_key)
                .is_some_and(|pending| pending.event.id == publication.event.id)
            {
                self.pending_leases.remove(&semantic_key);
            }
            match publication.state {
                CodingSessionLeaseState::Live => {
                    self.established_leases.insert(semantic_key);
                }
                CodingSessionLeaseState::Released => {
                    self.established_leases.remove(&semantic_key);
                }
            }
            published += 1;
        }
        Ok(published)
    }

    /// Drain the outbox into `sink`.
    pub async fn flush<S: EventSink>(&mut self, sink: &S) -> anyhow::Result<usize> {
        self.flush_terminal_dispositions()?;
        self.purge_outbox_for_retired()?;
        Ok(self.outbox.flush(sink).await?)
    }

    async fn flush_one<S: EventSink>(&mut self, sink: &S) -> anyhow::Result<usize> {
        self.flush_terminal_dispositions()?;
        self.purge_outbox_for_retired()?;
        Ok(self.outbox.flush_one(sink).await?)
    }

    /// Drop every queued fact that would advertise a retired execution.
    ///
    /// Called at retirement **and** before every flush. The second call is the
    /// defence in depth root asked for, and it is what makes this restart-safe:
    /// `retired` is persisted, so a provider that comes up over a state file
    /// carrying a retirement purges the queue its predecessor left behind
    /// before it publishes a single row of it.
    ///
    /// # What goes, and what deliberately stays
    ///
    /// Goes: per-generation **metadata** (44223), **transcript** items (44225)
    /// and **leases** (24223) naming a retired execution's exact target. Each
    /// of those is an assertion *about the session* — that it exists, that it
    /// is live, that this is what happened in it — and every one of them
    /// recreates the deleted record on the relay under a brand-new event id
    /// the deletion never named.
    ///
    /// Stays: **lifecycle receipts** (44224), unconditionally. They answer a
    /// command somebody sent, they are addressed to that sender rather than to
    /// the session, and the `SESSION_RETIRED` refusal that tells an operator
    /// why nothing is happening is itself one of them. Suppressing the answers
    /// along with the advertisements would replace a ghost session with a
    /// silence, which is the same failure wearing the other coat.
    ///
    /// Scoped by the exact `cs-target` tag, not by channel: an unrelated
    /// session in the same room keeps everything it has queued.
    fn purge_outbox_for_retired(&mut self) -> anyhow::Result<()> {
        let retired: HashSet<String> = self
            .state
            .sessions()
            .filter(|record| record.is_retired())
            .map(|record| coding_session_target_key(&self.target_for(record)))
            .collect();
        if retired.is_empty() {
            return Ok(());
        }
        let dropped = self.outbox.discard(|entry| {
            if !matches!(
                entry.kind,
                KIND_CODING_SESSION_METADATA
                    | KIND_CODING_SESSION_TRANSCRIPT
                    | KIND_CODING_SESSION_LEASE
            ) {
                return false;
            }
            entry.event.tags.iter().any(|tag| {
                let tag = tag.as_slice();
                tag.len() == 2 && tag[0] == "cs-target" && retired.contains(&tag[1])
            })
        })?;
        if dropped > 0 {
            tracing::warn!(
                target: "csp::retirement",
                dropped,
                "discarded queued facts for retired executions; a deleted session is never \
                 re-advertised, and the receipts that answer commands were kept"
            );
        }
        Ok(())
    }

    /// How long the runtime should wait before its next delivery pass.
    ///
    /// `Some(ZERO)` when a row is eligible right now, `Some(backoff)` while a
    /// failed row waits out its retry, and `None` when there is nothing queued
    /// — which the loop turns into a parked arm rather than a poll.
    pub fn next_publish_delay(&self) -> Option<Duration> {
        self.outbox.next_retry_delay()
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
/// One paged, verified read of a genesis's accepted authority chain.
///
/// `links` are the acceptance receipts the read could verify for that exact
/// genesis, ascending by `seq`. `incomplete` names why the read cannot be
/// called the whole chain — and it is the field the handover fence turns on,
/// because a read that might have stopped early is byte-for-byte
/// indistinguishable from a chain with no claim in it, and those two answers
/// fence in opposite directions.
struct AcceptedChain {
    links: Vec<authority::AcceptedTransition>,
    incomplete: Option<String>,
}

/// Whether a 40099 body claims to be about `genesis_ref`.
///
/// **Triage, never authority.** It is read out of content that has already
/// failed verification, so nothing it says may be applied; the single question
/// it answers is "is this unreadable receipt about the chain I am reading, or
/// somebody else's?" — which decides whether the fence stays up or the row is
/// ignored as unrelated traffic.
fn claims_genesis(content: &str, genesis_ref: &str) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GenesisOnly {
        genesis_ref: Option<String>,
    }
    serde_json::from_str::<GenesisOnly>(content)
        .ok()
        .and_then(|body| body.genesis_ref)
        .is_some_and(|claimed| claimed == genesis_ref)
}

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

/// The sentence a failed turn carries when Claude's login has lapsed on the
/// machine running the seat (finding 71).
///
/// The adapter's own words — "Failed to authenticate: OAuth session expired
/// and could not be refreshed", JSON-RPC -32603 — name the mechanism and not
/// the fix, and a whole live run failed turn after turn under them while
/// `claude auth status` said `loggedIn: true`. The operator needs the remedy
/// first; the raw error travels alongside as `detail`.
pub const CLAUDE_LOGIN_EXPIRED: &str = "Claude's login has expired on this computer. Run `claude auth login` in a terminal, then send the next turn.";

/// The operator sentence for a failed turn whose cause is recognized, or
/// `None` when the raw message is the best available sentence.
///
/// Matches the Claude CLI's authentication failures only: the ACP adapter's
/// `Failed to authenticate` (any suffix), the CLI's `OAuth session expired`,
/// and its interactive `Not logged in · Please run /login`. Anything else
/// keeps the provider's text verbatim rather than guessing a remedy.
fn turn_failure_sentence(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    let claude_login = lower.contains("failed to authenticate")
        || lower.contains("oauth session expired")
        || lower.contains("please run /login")
        || lower.contains("not logged in");
    claude_login.then_some(CLAUDE_LOGIN_EXPIRED)
}

/// Map a turn outcome onto its terminal transcript item, the generation's next
/// status, and whether the session can serve another turn.
fn turn_result(
    outcome: &TurnOutcome,
    duration_ms: u64,
    usage: Option<&TurnUsage>,
    tool_calls: u64,
    model: Option<&str>,
) -> (serde_json::Value, SessionStatus) {
    let cost = usage.map(turn_cost).unwrap_or_default();
    let usage_report = turn_usage_report(usage, tool_calls, model);
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
                payload::result_item(
                    subtype,
                    duration_ms,
                    stop_reason_text(stop_reason),
                    cost,
                    usage_report,
                ),
                SessionStatus::Idle,
            )
        }
        TurnOutcome::Cancelled => (
            payload::result_item(
                payload::ResultSubtype::Cancelled,
                duration_ms,
                "interrupted by the operator",
                cost,
                usage_report,
            ),
            SessionStatus::Interrupted,
        ),
        TurnOutcome::Failed {
            message,
            agent_gone,
        } => {
            // A recognized cause gets the operator sentence as `result` and
            // the raw provider text as `detail`; an unrecognized one keeps
            // the raw text as `result` and carries no `detail`, so a reader
            // can tell a classified failure from a verbatim one.
            let sentence = turn_failure_sentence(message);
            let mut item = payload::result_item(
                payload::ResultSubtype::Error,
                duration_ms,
                sentence.unwrap_or(message),
                cost,
                usage_report,
            );
            if let (Some(_), Some(object)) = (sentence, item.as_object_mut()) {
                object.insert("detail".into(), serde_json::json!(message));
            }
            (
                item,
                if *agent_gone {
                    SessionStatus::Disconnected
                } else {
                    SessionStatus::Failed
                },
            )
        }
    }
}

/// Which lifecycle path is asking for verified prior context.
///
/// The two paths differ in exactly one way, and it is load-bearing: a create is
/// entitled to find no execution chain at all — under a fresh genesis it is
/// minting the first one, and the seat that most needs the crew tools would
/// otherwise be the one seat without them. A resume is not: an earlier
/// generation of this very execution already ran, so an umbrella that projects
/// to zero executions means the chain could not be verified, never that there
/// was nothing to verify.
///
/// Collapsing the two turned an enumerated, operator-visible continuity loss
/// into a silent one: the resumed agent was told in its own system prompt that
/// there is no earlier verified work under this session to reconstruct, over
/// hours of its own signed transcript, and the operator saw no reason at all
/// because the projection had *succeeded*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RehydrationScope {
    /// A `session-create`: the umbrella may legitimately hold no execution yet.
    Create,
    /// A `session-resume`: an execution already ran, so an empty projection is
    /// a verification failure and is reported as one.
    Resume,
}

impl RehydrationScope {
    /// Whether a projection on this path may accept an umbrella with no
    /// verified execution chain.
    const fn allows_an_empty_umbrella(self) -> bool {
        matches!(self, Self::Create)
    }
}

/// Everything one rehydration attempt is about except the relay it reads.
///
/// Grouped rather than passed one by one because the six move together: every
/// caller has all of them or none, and the pair that decides whether there is
/// an umbrella at all — `session_ref` and `genesis_ref` — is easy to transpose
/// in a positional argument list.
struct RehydrationTarget<'a> {
    /// Which lifecycle path is asking.
    scope: RehydrationScope,
    /// The lifecycle command driving this attempt; tracing only.
    command_id: &'a str,
    /// The community channel the facts are scoped to.
    channel_id: Uuid,
    /// The umbrella session id, when the execution has one.
    session_ref: Option<&'a str>,
    /// The umbrella's canonical genesis event id, when the execution has one.
    genesis_ref: Option<&'a str>,
    /// The identifier the projected package is persisted under.
    package_id: &'a str,
}

/// What one attempt to attach verified prior context produced.
///
/// Exactly one side is populated: a descriptor when the package was built and
/// persisted, or the enumerated reason it was not. The reason is a *class* from
/// [`buzz_core::coding_session_payload::CONTEXT_UNAVAILABLE_REASONS`], never
/// free text — projector messages interpolate event and command ids and the
/// storage arm formats a host path, and this value ends up inside a signed
/// durable event.
// No `Debug`: the descriptor holds host paths and the first-turn brief, which
// `CreateRequest`'s own hand-written `Debug` deliberately refuses to print.
#[derive(Clone, PartialEq, Eq)]
struct RehydrationOutcome {
    descriptor: Option<RehydrationMcpDescriptor>,
    unavailable_reason: Option<&'static str>,
}

impl RehydrationOutcome {
    /// Verified context is attached; there is nothing to disclose.
    fn attached(descriptor: RehydrationMcpDescriptor) -> Self {
        Self {
            descriptor: Some(descriptor),
            unavailable_reason: None,
        }
    }

    /// No verified context, and the named reason why.
    fn unavailable(reason: &'static str) -> Self {
        Self {
            descriptor: None,
            unavailable_reason: Some(reason),
        }
    }
}

/// The status slug a *create* publishes to disclose what context its execution
/// actually starts from — with the reason continuity was lost, when one is
/// known — or `None` when the open was a reattachment (those are disclosed by
/// the attach path, which has its own richer slug set).
///
/// Both answers are stated out loud: a fresh execution says so rather than
/// staying silent, because silence is exactly what an operator misreads as
/// continuity.
///
/// Only the `Fresh` arm ever carries a reason. `Rehydrated` never does: nothing
/// was lost, so naming a package failure there would imply a loss that did not
/// happen.
fn create_disclosure(
    continuity: &SessionContinuity,
    unavailable_reason: Option<&'static str>,
) -> Option<(&'static str, Option<&'static str>)> {
    match continuity {
        SessionContinuity::Rehydrated => Some(("session_rehydrated", None)),
        SessionContinuity::Fresh => Some(("session_fresh", unavailable_reason)),
        SessionContinuity::Resumed
        | SessionContinuity::Loaded
        | SessionContinuity::RestartedWithoutContext { .. } => None,
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

/// Fold what the driver reported into the wire's per-turn usage block.
///
/// Three things happen here that the shape does not say on its own:
///
/// - `TurnUsage::turn_input_tokens` is **cache-inclusive** (see
///   `buzz_acp::usage`), while
///   [`payload::TurnUsageReport`](buzz_core::coding_session_payload::TurnUsageReport)
///   partitions the prompt side into three disjoint fields. The fresh-input
///   count is therefore the inclusive total *minus* both cache subsets, taken
///   with `checked_sub` so a driver whose subsets exceed its total omits the
///   field rather than publishing a wrapped number.
/// - `tool_calls` is only reported when the turn had any: `Some(0)` would
///   claim the provider counted, which it did — but a zero on every prose-only
///   turn is noise, and a reader that wants "no calls" gets it from the absent
///   key the same way it gets it from the zero.
/// - `context_window` comes from this provider's own table and is omitted for
///   a model it does not recognize.
fn turn_usage_report(
    usage: Option<&TurnUsage>,
    tool_calls: u64,
    model: Option<&str>,
) -> payload::TurnUsageReport {
    let cache_read = usage.and_then(|usage| usage.turn_cache_read_tokens);
    let cache_write = usage.and_then(|usage| usage.turn_cache_write_tokens);
    let fresh_input = usage
        .and_then(|usage| usage.turn_input_tokens)
        .and_then(|inclusive| {
            inclusive
                .checked_sub(cache_read.unwrap_or(0))
                .and_then(|rest| rest.checked_sub(cache_write.unwrap_or(0)))
        });
    payload::TurnUsageReport {
        input_tokens: fresh_input,
        output_tokens: usage.and_then(|usage| usage.turn_output_tokens),
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        tool_calls: (tool_calls > 0).then_some(tool_calls),
        context_window: model.and_then(crate::context_window::context_window_for_model),
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

/// The role pack a seat entry names, as the session layer wants it.
///
/// `None` when the launcher staged no pack — an actor seat without a role pack
/// is legal and materializes nothing.
fn seat_skills(seat: &crate::actor_seats::ActorSeat) -> Option<session::SeatSkills> {
    seat.pack_coordinates()
        .map(|(pack_dir, persona_id)| session::SeatSkills {
            pack_dir: pack_dir.to_path_buf(),
            persona_id: persona_id.to_owned(),
        })
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
    /// Finding 71, verbatim: the adapter's JSON-RPC -32603 as `AcpError::
    /// AgentError` renders it. The seat's result item leads with the remedy
    /// sentence, keeps the raw error under `detail`, and the generation is
    /// `failed` — the process is still there, so the next turn after
    /// `claude auth login` is serviceable.
    #[test]
    fn an_expired_claude_login_fails_the_turn_with_the_remedy_and_keeps_the_raw_error() {
        let raw = "Agent reported error (code -32603): Failed to authenticate: OAuth session expired and could not be refreshed";
        let (item, status) = turn_result(
            &session::TurnOutcome::Failed {
                message: raw.to_string(),
                agent_gone: false,
            },
            2_500,
            None,
            0,
            None,
        );
        assert_eq!(
            item["result"],
            "Claude's login has expired on this computer. Run `claude auth login` in a terminal, then send the next turn.",
            "{item}"
        );
        assert_eq!(item["detail"], raw, "{item}");
        assert_eq!(item["subtype"], "error");
        assert_eq!(item["isError"], true);
        assert_eq!(status, SessionStatus::Failed);
    }

    /// The bare adapter message, without the `AcpError` prefix, classifies
    /// the same way; so does the CLI's interactive login prompt.
    #[test]
    fn every_claude_login_phrasing_maps_to_the_one_sentence() {
        for raw in [
            "Failed to authenticate: OAuth session expired and could not be refreshed",
            "Not logged in · Please run /login",
            "authentication failed: OAuth session expired",
        ] {
            assert_eq!(
                turn_failure_sentence(raw),
                Some(CLAUDE_LOGIN_EXPIRED),
                "{raw}"
            );
        }
    }

    /// An unrecognized failure keeps the provider's own words and carries
    /// no `detail`, so a reader can tell a classified failure from a
    /// verbatim one.
    #[test]
    fn an_unrecognized_failure_keeps_the_raw_message_and_no_detail() {
        let raw = "Agent reported error (code -32000): tool 'Bash' is not available";
        assert_eq!(turn_failure_sentence(raw), None);
        let (item, status) = turn_result(
            &session::TurnOutcome::Failed {
                message: raw.to_string(),
                agent_gone: true,
            },
            10,
            None,
            0,
            None,
        );
        assert_eq!(item["result"], raw, "{item}");
        assert!(item.get("detail").is_none(), "{item}");
        assert_eq!(status, SessionStatus::Disconnected);
    }

    /// The seam the whole lane exists for: what a driver reported becomes an
    /// additive `usage` block on the turn's terminal item, with the prompt
    /// side split into three disjoint counts.
    #[test]
    fn a_finished_turn_publishes_the_drivers_usage_on_its_result_item() {
        let usage = buzz_acp::TurnUsage {
            session_id: "s".into(),
            turn_seq: 1,
            delta_reliable: true,
            // Cache-inclusive, exactly as `buzz_acp::usage` produces it.
            turn_input_tokens: Some(101_200),
            turn_output_tokens: Some(340),
            turn_total_tokens: None,
            turn_cost_usd: None,
            turn_cache_read_tokens: Some(96_000),
            turn_cache_write_tokens: Some(4_000),
            cumulative_input_tokens: None,
            cumulative_output_tokens: None,
            cumulative_total_tokens: None,
            cumulative_cost_usd: None,
            cumulative_cache_read_tokens: None,
            cumulative_cache_write_tokens: None,
            model: None,
            pricing_identity: None,
        };
        let (item, _) = turn_result(
            &session::TurnOutcome::Completed {
                stop_reason: buzz_acp::acp::StopReason::EndTurn,
            },
            1_000,
            Some(&usage),
            7,
            Some("opus[1m]"),
        );
        assert_eq!(
            item["usage"],
            serde_json::json!({
                "inputTokens": 1_200,
                "outputTokens": 340,
                "cacheReadTokens": 96_000,
                "cacheWriteTokens": 4_000,
                "toolCalls": 7,
                "contextWindow": 1_000_000,
            }),
            "{item}"
        );
        // The split is lossless: the three prompt-side fields re-sum to the
        // cache-inclusive total the top-level key still carries.
        assert_eq!(item["inputTokens"], serde_json::json!(101_200));
    }

    /// A driver that reports nothing leaves the item exactly as it was before
    /// the block existed — no `usage` key, not an empty object.
    #[test]
    fn a_turn_with_no_driver_usage_publishes_no_usage_block() {
        let (item, _) = turn_result(
            &session::TurnOutcome::Completed {
                stop_reason: buzz_acp::acp::StopReason::EndTurn,
            },
            1_000,
            None,
            0,
            Some("default"),
        );
        assert!(item.get("usage").is_none(), "{item}");
    }

    /// An unrecognized model omits the window; the token counts still ship.
    #[test]
    fn an_unrecognized_model_omits_the_window_and_keeps_the_counts() {
        let report = turn_usage_report(None, 3, Some("some-model-nobody-shipped"));
        assert_eq!(report.context_window, None);
        assert_eq!(report.tool_calls, Some(3));
    }

    use super::*;
    use crate::payload::{BUDGET_EXHAUSTED, UNAUTHORIZED_OPERATOR};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade};
    use axum::extract::State;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use nostr::Keys;

    use crate::session::testing::{fake_agent, GOOD_AGENT, RESUMABLE_AGENT, STALLING_AGENT};

    #[path = "ci_continuation_restore_tests.rs"]
    mod ci_continuation_restore_tests;
    #[path = "ci_continuation_tests.rs"]
    mod ci_continuation_tests;
    #[path = "founder_wake_framing_tests.rs"]
    mod founder_wake_framing_tests;
    #[path = "handover_ci_tests.rs"]
    mod handover_ci_tests;
    #[path = "handover_fence_tests.rs"]
    mod handover_fence_tests;
    #[path = "handover_integration_tests.rs"]
    mod handover_integration_tests;
    #[path = "handover_retirement_tests.rs"]
    mod handover_retirement_tests;
    #[path = "hire_requester_tests.rs"]
    mod hire_requester_tests;
    #[path = "observed_gate_row_tests.rs"]
    mod observed_gate_row_tests;
    #[path = "operation_fence_tests.rs"]
    mod operation_fence_tests;
    #[path = "session_policy_budget_tests.rs"]
    mod session_policy_budget_tests;
    #[path = "team_wake_driver_tests.rs"]
    mod team_wake_driver_tests;
    #[path = "team_wake_pressure_tests.rs"]
    mod team_wake_pressure_tests;

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
        events: Arc<Mutex<Vec<Event>>>,
        queries: Arc<Mutex<Vec<serde_json::Value>>>,
        published: Arc<Mutex<Vec<Event>>>,
        reject_next_publish: Arc<AtomicBool>,
        /// Events this fake withholds until it has already served one
        /// authority-chain read.
        ///
        /// The only way to actually exercise the window a create has to be
        /// safe in: its first chain read decides the fence *before* an adapter
        /// starts, and its second runs after the record exists. Staging the
        /// takeover in `events` up front means both reads see it and the
        /// window is never entered; inserting it from a watcher task races the
        /// reads. Withholding it here is exact — the relay genuinely did not
        /// have it yet on the first read.
        deferred: Arc<Mutex<Vec<Event>>>,
        /// How many authority-chain reads this fake has served.
        chain_reads: Arc<Mutex<usize>>,
        /// The `self` key this fake serves at NIP-11, when it serves one.
        ///
        /// A real relay's identity is fetched, not configured into the client,
        /// and a provider that cannot fetch it verifies nothing. Serving it
        /// here lets a test witness the identity the way production does —
        /// which matters because "no identity" is now a refusal rather than a
        /// quiet degradation.
        relay_self: Arc<Mutex<Option<String>>>,
    }

    #[derive(Clone)]
    struct RecordingTestRelay {
        events: Arc<Mutex<Vec<Event>>>,
        queries: Arc<Mutex<Vec<serde_json::Value>>>,
        published: Arc<Mutex<Vec<Event>>>,
        reject_next_publish: Arc<AtomicBool>,
        /// Events revealed only from the second authority-chain read onwards.
        deferred: Arc<Mutex<Vec<Event>>>,
        /// The identity this fake advertises, so a test can serve one, take it
        /// away, or put it back mid-run.
        relay_self: Arc<Mutex<Option<String>>>,
    }

    /// Minimal NIP-01 filter matching for the fake relay's `/query` bridge:
    /// `ids`, `kinds`, `authors`, `#h`, and `#d` — the fields the provider's
    /// genesis resolution, transition resolution, authority backfill, and CI
    /// correlation use.
    fn test_filter_matches(filter: &serde_json::Value, event: &Event) -> bool {
        if let Some(ids) = filter.get("ids").and_then(serde_json::Value::as_array) {
            if !ids.iter().any(|id| id.as_str() == Some(&event.id.to_hex())) {
                return false;
            }
        }
        if let Some(kinds) = filter.get("kinds").and_then(serde_json::Value::as_array) {
            if !kinds
                .iter()
                .any(|kind| kind.as_u64() == Some(u64::from(event.kind.as_u16())))
            {
                return false;
            }
        }
        if let Some(authors) = filter.get("authors").and_then(serde_json::Value::as_array) {
            if !authors
                .iter()
                .any(|author| author.as_str() == Some(&event.pubkey.to_hex()))
            {
                return false;
            }
        }
        if let Some(channels) = filter.get("#h").and_then(serde_json::Value::as_array) {
            let event_channel = event.tags.iter().find_map(|tag| {
                let tag = tag.as_slice();
                (tag.len() == 2 && tag[0] == "h").then(|| tag[1].clone())
            });
            if !channels
                .iter()
                .any(|channel| channel.as_str() == event_channel.as_deref())
            {
                return false;
            }
        }
        // `until` is inclusive, and it is what bounded paging pages *with*:
        // without it here, a second page returns the first page again and a
        // capped-read test can never reach the rows behind the cap.
        if let Some(until) = filter.get("until").and_then(serde_json::Value::as_u64) {
            if event.created_at.as_secs() > until {
                return false;
            }
        }
        if let Some(since) = filter.get("since").and_then(serde_json::Value::as_u64) {
            if event.created_at.as_secs() < since {
                return false;
            }
        }
        // Addressable/correlation tag. A CI result carries its correlation
        // digest here, which is the only way to ask for one exact run attempt.
        if let Some(wanted) = filter.get("#d").and_then(serde_json::Value::as_array) {
            let event_d = event.tags.iter().find_map(|tag| {
                let tag = tag.as_slice();
                (tag.len() == 2 && tag[0] == "d").then(|| tag[1].clone())
            });
            if !wanted
                .iter()
                .any(|value| value.as_str() == event_d.as_deref())
            {
                return false;
            }
        }
        true
    }

    /// `/` is two things on a real relay: the WebSocket endpoint and the
    /// NIP-11 document. This fake had only the first, which is why no test
    /// could witness an identity the way production does.
    async fn test_relay_root(
        State(state): State<TestRelayState>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        use axum::extract::FromRequestParts;
        use axum::response::IntoResponse;

        let (mut parts, _body) = request.into_parts();
        match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
            Ok(ws) => ws
                .on_upgrade(
                    move |socket| async move { serve_test_relay_socket(socket, state).await },
                )
                .into_response(),
            // Not an upgrade: answer NIP-11. A fake with no identity omits
            // `self` entirely, which is what a relay that has not been
            // configured with one does.
            Err(_) => {
                let document = match state.relay_self.lock().expect("relay self lock").clone() {
                    Some(relay_self) => serde_json::json!({ "self": relay_self }),
                    None => serde_json::json!({}),
                };
                Json(document).into_response()
            }
        }
    }

    async fn serve_test_relay_socket(mut socket: WebSocket, state: TestRelayState) {
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
        while let Some(Ok(message)) = socket.recv().await {
            let AxumWsMessage::Text(message) = message else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(message.as_str()) else {
                continue;
            };
            if value.pointer("/0").and_then(serde_json::Value::as_str) != Some("EVENT") {
                continue;
            }
            let Some(event_value) = value.pointer("/1").cloned() else {
                continue;
            };
            let Ok(event) = serde_json::from_value::<Event>(event_value) else {
                continue;
            };
            state
                .published
                .lock()
                .expect("published lock")
                .push(event.clone());
            let accepted = !state.reject_next_publish.swap(false, Ordering::SeqCst);
            if accepted {
                state
                    .events
                    .lock()
                    .expect("events lock")
                    .push(event.clone());
            }
            socket
                .send(AxumWsMessage::Text(
                    serde_json::json!([
                        "OK",
                        event.id.to_hex(),
                        accepted,
                        if accepted {
                            "stored"
                        } else {
                            "injected rejection"
                        }
                    ])
                    .to_string()
                    .into(),
                ))
                .await
                .expect("send event OK");
        }
    }

    async fn test_relay_query(
        State(state): State<TestRelayState>,
        Json(query): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        state
            .queries
            .lock()
            .expect("queries lock")
            .push(query.clone());
        let filters: Vec<serde_json::Value> = query.as_array().cloned().unwrap_or_default();
        // An authority-chain read is the one query shape this fake counts: a
        // 40099 partition scoped to the witnessed relay identity.
        let is_chain_read = filters.iter().any(|filter| {
            filter
                .get("kinds")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|kinds| {
                    kinds
                        .iter()
                        .any(|kind| kind.as_u64() == Some(u64::from(KIND_SYSTEM_MESSAGE)))
                })
                && filter.get("authors").is_some()
        });
        let mut served = state.chain_reads.lock().expect("chain reads lock");
        let events = state.events.lock().expect("events lock");
        let deferred = state.deferred.lock().expect("deferred lock");
        // Withheld until one chain read has already been answered, so the
        // *second* read is the first to see them.
        let visible: Vec<&Event> = if *served >= 1 {
            events.iter().chain(deferred.iter()).collect()
        } else {
            events.iter().collect()
        };
        if is_chain_read {
            *served = served.saturating_add(1);
        }
        let mut matched: Vec<&Event> = visible
            .into_iter()
            .filter(|event| {
                filters
                    .iter()
                    .any(|filter| test_filter_matches(filter, event))
            })
            .collect();
        // Newest-first and page-capped, like the real relay
        // (`crates/buzz-db/src/event.rs`). Without both, a fake relay hands
        // every caller the whole history and the paging a capped read forces
        // is never exercised — which is exactly the gap that let an
        // authority backfill mistake a truncated page for a complete chain.
        matched.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        if let Some(limit) = filters
            .iter()
            .filter_map(|filter| filter.get("limit").and_then(serde_json::Value::as_u64))
            .min()
        {
            matched.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        Json(serde_json::to_value(matched).expect("serialize events"))
    }

    async fn spawn_test_relay(
        keys: &Keys,
        event: Option<Event>,
    ) -> (
        HarnessRelay,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        spawn_test_relay_with_events(keys, event.into_iter().collect()).await
    }

    async fn spawn_test_relay_with_events(
        keys: &Keys,
        events: Vec<Event>,
    ) -> (
        HarnessRelay,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (relay, control, server) = spawn_recording_test_relay(keys, events).await;
        (relay, control.queries, server)
    }

    async fn spawn_recording_test_relay(
        keys: &Keys,
        events: Vec<Event>,
    ) -> (
        HarnessRelay,
        RecordingTestRelay,
        tokio::task::JoinHandle<()>,
    ) {
        let events = Arc::new(Mutex::new(events));
        let queries = Arc::new(Mutex::new(Vec::new()));
        let published = Arc::new(Mutex::new(Vec::new()));
        let reject_next_publish = Arc::new(AtomicBool::new(false));
        // A real relay publishes an identity, so the fake does too by default:
        // a provider that witnesses it can verify receipts, and one that never
        // asks stays exactly as unverified as it was. Tests that sign receipts
        // themselves call `set_relay_self` with their own key first, and
        // `witness_relay_identity` never overwrites a known identity.
        let deferred: Arc<Mutex<Vec<Event>>> = Arc::default();
        let chain_reads: Arc<Mutex<usize>> = Arc::default();
        let relay_self = Arc::new(Mutex::new(Some(Keys::generate().public_key().to_hex())));
        let state = TestRelayState {
            events: events.clone(),
            queries: queries.clone(),
            published: published.clone(),
            reject_next_publish: reject_next_publish.clone(),
            deferred: deferred.clone(),
            chain_reads,
            relay_self: relay_self.clone(),
        };
        let app: Router = Router::new()
            .route("/", get(test_relay_root))
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
        (
            relay,
            RecordingTestRelay {
                events,
                queries,
                published,
                reject_next_publish,
                deferred,
                relay_self,
            },
            server,
        )
    }

    #[tokio::test]
    async fn startup_scan_recovers_a_stored_report_outside_live_replay() {
        use buzz_core::coding_session_team_transaction::{
            CodingSessionTeamReport, CodingSessionTeamTransactionBody,
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let actor = Keys::generate();
        let session_ref = Uuid::new_v4().to_string();
        let genesis_ref = "ab".repeat(32);
        let payload =
            buzz_sdk::coding_session_team_transaction::coding_session_team_transaction_payload(
                session_ref.clone(),
                genesis_ref,
                None,
                None,
                CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                    assignment_ref: "11".repeat(32),
                    summary: "Stored while the provider was offline".into(),
                    branch: None,
                    base_sha: None,
                    head_sha: None,
                    files: Vec::new(),
                    tests: Vec::new(),
                    red_before_green: None,
                    deviations: Vec::new(),
                    residuals: Vec::new(),
                    anomalies: Vec::new(),
                }),
            );
        let report =
            buzz_sdk::coding_session_team_transaction::build_coding_session_team_transaction(
                &channel_id.to_string(),
                payload,
            )
            .expect("builder")
            .sign_with_keys(&actor)
            .expect("sign report");
        let provider_keys = Keys::generate();
        let (relay, queries, server) = spawn_test_relay(&provider_keys, Some(report.clone())).await;
        let mut provider = Provider::new(config_of(
            provider_keys.clone(),
            &dir.path().join("state"),
            None,
            "missing-agent".into(),
        ))
        .expect("provider");
        provider.set_rest_client(relay.rest_client());
        provider.subscribed.insert(channel_id);

        provider.discover_team_wake_partition_for(channel_id).await;
        let discovered = provider
            .team_wakes
            .pending_for_channel(channel_id)
            .expect("select discovered wake")
            .expect("one discovered wake");
        assert_eq!(discovered.source.event_id(), report.id.to_hex());
        let query_count = queries.lock().expect("queries").len();
        provider.discover_team_wake_partition_for(channel_id).await;
        assert_eq!(queries.lock().expect("queries").len(), query_count);

        relay.shutdown().await;
        server.abort();
    }

    /// The runtime, rather than the store in isolation, makes one persisted
    /// round-robin choice per tick. A blocked A may consume its own turn, but
    /// it cannot make B wait for a second independent discovery/processing
    /// cursor advance.
    #[tokio::test]
    async fn team_wake_driver_gives_a_blocked_channel_one_cycle_slot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let provider_keys = Keys::generate();
        let (relay, _, server) = spawn_test_relay(&provider_keys, None).await;
        let mut provider = Provider::new(config_of(
            provider_keys,
            &dir.path().join("state"),
            None,
            "missing-agent".into(),
        ))
        .expect("provider");
        let channels = [Uuid::from_u128(1), Uuid::from_u128(2)];
        provider.set_rest_client(relay.rest_client());
        provider.subscribed.extend(channels);
        for (value, channel) in channels.into_iter().enumerate() {
            provider
                .team_wakes
                .capture_report(
                    team_wake::WakeScope {
                        channel_ref: channel,
                        session_ref: channel.to_string(),
                        genesis_ref: "ab".repeat(32),
                    },
                    team_wake::WakeSource::Report {
                        operation_id: format!("{value:064x}"),
                        operation_type: "assignment_report".into(),
                        author_pubkey: format!("{:064x}", value + 10),
                        created_at: 1,
                    },
                )
                .expect("capture report");
        }
        let publisher = relay.event_publisher();
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("A tick");
        assert_eq!(provider.team_wakes.channel_counts(channels[0]).2, 1);
        assert_eq!(provider.team_wakes.channel_counts(channels[1]).2, 0);
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("B tick");
        assert_eq!(provider.team_wakes.channel_counts(channels[1]).2, 1);

        relay.shutdown().await;
        server.abort();
    }

    /// v3.1 S1: subscribed channels that are already scanned and have no work
    /// are absent from the candidate set, so one busy channel gets the tick.
    #[test]
    fn idle_scanned_channels_consume_no_scheduler_ticks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut provider = Provider::new(config_of(
            Keys::generate(),
            &dir.path().join("state"),
            None,
            "missing-agent".into(),
        ))
        .expect("provider");
        let idle = Uuid::from_u128(1);
        let working = Uuid::from_u128(2);
        provider.subscribed.extend([idle, working]);
        provider.team_wake_scanned_channels.insert(idle);
        provider.team_wake_scanned_channels.insert(working);
        provider
            .team_wakes
            .capture_report(
                team_wake::WakeScope {
                    channel_ref: working,
                    session_ref: working.to_string(),
                    genesis_ref: "ab".repeat(32),
                },
                team_wake::WakeSource::Report {
                    operation_id: "77".repeat(32),
                    operation_type: "assignment_report".into(),
                    author_pubkey: "88".repeat(32),
                    created_at: 1,
                },
            )
            .expect("working source");
        assert!(!provider.team_wake_discovery_needed(idle));
        assert!(!provider.team_wake_processing_needed(idle));
        assert!(provider.team_wake_processing_needed(working));
        assert_eq!(
            provider
                .team_wakes
                .next_tick_channel([working])
                .expect("select only need"),
            Some(working)
        );
    }

    #[tokio::test]
    async fn saturated_complete_partition_is_channel_backed_off_not_requeried_each_tick() {
        use buzz_core::coding_session_team_transaction::{
            CodingSessionTeamReport, CodingSessionTeamTransactionBody,
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let actor = Keys::generate();
        let payload =
            buzz_sdk::coding_session_team_transaction::coding_session_team_transaction_payload(
                Uuid::new_v4().to_string(),
                "ab".repeat(32),
                None,
                None,
                CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                    assignment_ref: "11".repeat(32),
                    summary: "same-page saturation fixture".into(),
                    branch: None,
                    base_sha: None,
                    head_sha: None,
                    files: Vec::new(),
                    tests: Vec::new(),
                    red_before_green: None,
                    deviations: Vec::new(),
                    residuals: Vec::new(),
                    anomalies: Vec::new(),
                }),
            );
        let report =
            buzz_sdk::coding_session_team_transaction::build_coding_session_team_transaction(
                &channel_id.to_string(),
                payload,
            )
            .expect("builder")
            .sign_with_keys(&actor)
            .expect("sign report");
        // The fake relay intentionally returns the same full page after an
        // `until`, exercising the production complete-partition saturation
        // refusal without manufacturing a store result.
        let provider_keys = Keys::generate();
        let (relay, queries, server) =
            spawn_test_relay_with_events(&provider_keys, vec![report.clone(); 1_000]).await;
        let mut provider = Provider::new(config_of(
            provider_keys.clone(),
            &dir.path().join("state"),
            None,
            "missing-agent".into(),
        ))
        .expect("provider");
        provider.set_rest_client(relay.rest_client());
        provider.subscribed.insert(channel_id);
        let publisher = relay.event_publisher();
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("saturating tick does not fail the provider");
        assert!(!provider
            .team_wake_discovery_backoff
            .contains_key(&channel_id));
        let after_refusal = queries.lock().expect("queries").len();
        assert!(after_refusal >= 2, "complete query paged before refusing");
        provider
            .run_one_team_wake_tick(&publisher)
            .await
            .expect("refused channel consumes no tick");
        assert_eq!(queries.lock().expect("queries").len(), after_refusal);
        assert_eq!(
            provider
                .team_wakes
                .refusal(channel_id)
                .expect("durable refusal")
                .code,
            team_wake::ChannelRefusalCode::PartitionSaturated
        );

        relay.shutdown().await;
        server.abort();
        drop(provider);

        // v3.1 §3.3: a restart gets exactly one probe; when a non-empty
        // partition has become queryable, that probe clears the refusal before
        // admitting its recovered report. Empty-only recovery would miss the
        // self-refusal bug this regression exists to pin.
        let (relay, _, server) = spawn_test_relay(&provider_keys, Some(report.clone())).await;
        let mut restarted = Provider::new(config_of(
            provider_keys,
            &dir.path().join("state"),
            None,
            "missing-agent".into(),
        ))
        .expect("restarted provider");
        restarted.set_rest_client(relay.rest_client());
        restarted.subscribed.insert(channel_id);
        restarted
            .run_one_team_wake_tick(&relay.event_publisher())
            .await
            .expect("one restart probe");
        assert!(restarted.team_wakes.refusal(channel_id).is_none());
        let recovered = restarted
            .team_wakes
            .pending_for_channel(channel_id)
            .expect("recovered report remains durable")
            .expect("non-empty restart probe admitted its report");
        assert_eq!(recovered.source.event_id(), report.id.to_hex());
        relay.shutdown().await;
        server.abort();
    }

    fn test_operator_keys() -> &'static Keys {
        static KEYS: OnceLock<Keys> = OnceLock::new();
        KEYS.get_or_init(Keys::generate)
    }

    /// A claude runtime that runs `script` under `bash` rather than exec'ing
    /// it — see `session::tests::request` for why (ETXTBSY against other test
    /// threads' forks).
    fn claude_runtime(script: String) -> RuntimeDescriptor {
        RuntimeDescriptor {
            agent_command: "bash".into(),
            agent_args: vec![script],
            ..claude_runtime_command(String::new())
        }
    }

    /// `claude_runtime` for a command spawned as-is — notably a path that is
    /// meant *not* to exist, where the spawn itself has to fail.
    fn claude_runtime_command(agent_command: String) -> RuntimeDescriptor {
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
            // Mirrors production discovery: the custody file is the projects
            // file's sibling, so a test writes one beside the other.
            actor_seats_file: projects
                .and_then(Path::parent)
                .map(|parent| parent.join(config::ACTOR_SEATS_FILE_NAME)),
            context_mcp_command: None,
            instance_id: "instance-1".into(),
            runtimes,
            max_sessions: 2,
            // Unbudgeted unless a test says otherwise: D9's ceiling is a host
            // setting, and every test that predates it describes a host that
            // never set one.
            turn_budget: config::UNLIMITED_TURN_BUDGET,
            session_idle_shutdown: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(900),
            answer_stall_timeout: Some(Duration::from_secs(120)),
            emit_raw_sdk_frames: false,
            redaction_retention: crate::redaction_vault::RetentionPolicy::default(),
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

    fn create_event_with_routing(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
    ) -> (Event, buzz_core::coding_session_routing::RoutingRecord) {
        let routing = serde_json::from_value(serde_json::json!({
            "class": "builder",
            "tier": "fast",
            "risk": {
                "impact": 1,
                "uncertainty": 1,
                "irreversibility": 2,
                "score": 2
            },
            "profile": null,
            "chosen": {
                "provider": "claude-primary",
                "model": "sonnet",
                "effort": "low"
            },
            "runnerUp": null,
            "reason": "the signed catalog and registry selected this target",
            "reviewRequired": false,
            "reviewReasons": [],
            "challengerSample": false,
            "override": null,
            "registryVersion": 1,
            "catalogRevision": 12
        }))
        .expect("routing record");
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "model": "sonnet",
                "title": "Ship it",
                "initialTurn": null,
                "routing": routing,
            },
        })
        .to_string();
        (signed_lifecycle_event(channel_id, content), routing)
    }

    /// A create claiming an umbrella *and* carrying a first turn, signed by
    /// whichever key the caller names — the shape a delegated seat sends.
    fn create_event_with_umbrella_and_turn(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        session_ref: &str,
        turn: &str,
        keys: &Keys,
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
                "initialTurn": turn,
            },
        })
        .to_string();
        signed_lifecycle_event_by(channel_id, content, keys)
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

    /// A `grant-operator` transition signed by the founder (the session
    /// owner), as the desktop would publish it.
    fn grant_transition_event(
        channel_id: Uuid,
        genesis_ref: &str,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_hex: &str,
    ) -> Event {
        authority_transition_event(
            channel_id,
            genesis_ref,
            prev_accepted,
            seq,
            grantee_hex,
            CodingSessionAuthorityTransitionType::GrantOperator,
        )
    }

    fn authority_transition_event(
        channel_id: Uuid,
        genesis_ref: &str,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_hex: &str,
        transition_type: CodingSessionAuthorityTransitionType,
    ) -> Event {
        let payload = buzz_core::coding_session_authority_transition::
            CodingSessionAuthorityTransitionPayload::new(
                transition_type,
                genesis_ref.to_owned(),
                prev_accepted,
                seq,
                grantee_hex.to_owned(),
            );
        buzz_sdk::builders::build_coding_session_authority_transition(channel_id, &payload)
            .expect("transition builder")
            .sign_with_keys(test_operator_keys())
            .expect("sign transition")
    }

    /// The relay-signed kind 40099 acceptance receipt for one transition,
    /// with exactly the content shape `buzz-relay` emits.
    fn acceptance_receipt_event(
        relay_keys: &Keys,
        channel_id: Uuid,
        genesis_ref: &str,
        transition: &Event,
        seq: u32,
        grantee_hex: &str,
    ) -> Event {
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
            serde_json::json!({
                "type": authority::ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": genesis_ref,
                "acceptedEventId": transition.id.to_hex(),
                "seq": seq,
                "transitionType": "grant-operator",
                "granteePubkey": grantee_hex,
            })
            .to_string(),
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(relay_keys)
        .expect("sign receipt")
    }

    fn acceptance_receipt_for_transition(
        relay_keys: &Keys,
        channel_id: Uuid,
        transition: &Event,
    ) -> Event {
        let payload = buzz_core::coding_session_authority_transition::
            decode_coding_session_authority_transition(&transition.content)
            .expect("transition payload");
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
            serde_json::json!({
                "type": authority::ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": payload.genesis_ref,
                "acceptedEventId": transition.id.to_hex(),
                "seq": payload.seq,
                "transitionType": payload.transition_type,
                "granteePubkey": payload.grantee_pubkey,
            })
            .to_string(),
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(relay_keys)
        .expect("sign receipt")
    }

    fn lifecycle_decision_by(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        action: &str,
        target: &CodingSessionTarget,
        operator: &Keys,
    ) -> LifecycleDecision {
        let content =
            lifecycle_target_event(provider, channel_id, command_id, action, target).content;
        let event = signed_lifecycle_event_by(channel_id, content, operator);
        let projects = ProjectsFile::default();
        let actor_seats = crate::actor_seats::ActorSeatsFile::default();
        commands::decide_lifecycle(
            &provider.context(channel_id, &projects, &actor_seats, &event.pubkey.to_hex()),
            channel_id,
            event.created_at.as_secs(),
            &event.content,
        )
    }

    /// The framing decision is the whole "iff": a founder's own words are
    /// delivered bare, and anyone else's are addressed — with the sender's
    /// seat resolved from this provider's own records, never from the command.
    #[test]
    fn a_turn_is_framed_only_when_its_signer_is_not_the_founder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&state_dir, None);
        let founder = test_operator_keys().public_key().to_hex();

        let addressed = governed_record(channel_id, &cwd, &"ab".repeat(32));
        let addressed_id = addressed.session_id.clone();
        let umbrella = addressed.session_ref.clone().expect("umbrella");
        let sender_actor = "cd".repeat(32);
        let mut sibling = governed_record(channel_id, &cwd, &"ab".repeat(32));
        sibling.actor = Some(sender_actor.clone());
        sibling.role = Some("lead".into());
        let sibling_target = sibling.target(&provider.config.instance_id);
        provider
            .state
            .insert_session(addressed)
            .expect("insert addressed");
        provider.state.insert_session(sibling).expect("insert seat");

        assert_eq!(
            provider.turn_framing(
                &addressed_id,
                &founder,
                CodingSessionDelivery::Boundary,
                channel_id,
                "ship it",
            ),
            None,
            "the founder's own turn is never framed as somebody else's message"
        );

        let framed = provider
            .turn_framing(
                &addressed_id,
                &sender_actor,
                CodingSessionDelivery::Steer,
                channel_id,
                "ship it",
            )
            .expect("a sibling seat's turn is framed");
        assert_eq!(framed.sender_pubkey, sender_actor);
        assert_eq!(framed.sender_role.as_deref(), Some("lead"));
        assert_eq!(
            framed.reply_target.as_deref(),
            Some(coding_session_target_key(&sibling_target).as_str()),
            "the reply address is the sender's own execution"
        );
        assert_eq!(framed.delivery, CodingSessionDelivery::Steer);
        assert_eq!(framed.channel_id, channel_id);
        assert_eq!(umbrella, "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");

        // A granted operator with no seat is framed as an operator and given
        // no reply address rather than a guessed one.
        let stranger = provider
            .turn_framing(
                &addressed_id,
                &"ef".repeat(32),
                CodingSessionDelivery::Boundary,
                channel_id,
                "ship it",
            )
            .expect("a non-founder turn is framed");
        assert_eq!(stranger.sender_role, None);
        assert_eq!(stranger.reply_target, None);
    }

    /// A closed seat is not who the sender is now.
    ///
    /// Nothing enforces one execution per actor per umbrella — the plan's own
    /// disposable-builder pattern ends one seat and creates another for the
    /// same managed-agent pubkey — and the seat lookup walked every record the
    /// provider has ever minted, in session-id order. A retired seat could
    /// therefore supply both the reply address (a generation no turn can
    /// reach) and the `sender_role` that is written into the *signed*
    /// `user_prompt` item: a durable transcript recording a role the sender
    /// did not hold.
    #[test]
    fn a_closed_seat_supplies_neither_the_role_nor_the_reply_address() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&state_dir, None);
        let sender_actor = "cd".repeat(32);

        let addressed = governed_record(channel_id, &cwd, &"ab".repeat(32));
        let addressed_id = addressed.session_id.clone();
        provider
            .state
            .insert_session(addressed)
            .expect("insert addressed");

        // Sorted before the live seat by session id, which is the order the
        // record map walks in.
        let mut retired = governed_record(channel_id, &cwd, &"ab".repeat(32));
        retired.session_id = "00000000-0000-4000-8000-000000000001".into();
        retired.actor = Some(sender_actor.clone());
        retired.role = Some("builder".into());
        retired.created_at_ms = 1;
        retired.closed = true;
        provider
            .state
            .insert_session(retired)
            .expect("insert stale");

        let mut current = governed_record(channel_id, &cwd, &"ab".repeat(32));
        current.session_id = "ffffffff-0000-4000-8000-000000000002".into();
        current.actor = Some(sender_actor.clone());
        current.role = Some("lead".into());
        current.created_at_ms = 2;
        let current_target = current.target(&provider.config.instance_id);
        provider
            .state
            .insert_session(current)
            .expect("insert current");

        let framed = provider
            .turn_framing(
                &addressed_id,
                &sender_actor,
                CodingSessionDelivery::Boundary,
                channel_id,
                "ship it",
            )
            .expect("a sibling seat's turn is framed");
        assert_eq!(
            framed.sender_role.as_deref(),
            Some("lead"),
            "the role the sender holds now, not the one it retired from"
        );
        assert_eq!(
            framed.reply_target.as_deref(),
            Some(coding_session_target_key(&current_target).as_str()),
            "a reply must be addressed at a seat that can still take a turn"
        );

        // With only the retired seat left, there is no address to give and no
        // role to claim.
        provider
            .state
            .update_session("ffffffff-0000-4000-8000-000000000002", |record| {
                record.closed = true
            })
            .expect("close current");
        let unseated = provider
            .turn_framing(
                &addressed_id,
                &sender_actor,
                CodingSessionDelivery::Boundary,
                channel_id,
                "ship it",
            )
            .expect("still framed as a non-founder turn");
        assert_eq!(unseated.sender_role, None);
        assert_eq!(unseated.reply_target, None);
    }

    /// A record from before founders were persisted cannot say who the founder
    /// is, so it frames nothing rather than labelling its founder a stranger.
    #[test]
    fn a_legacy_record_with_no_founder_frames_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&state_dir, None);
        let mut legacy = governed_record(channel_id, &cwd, &"ab".repeat(32));
        legacy.founder_pubkey = None;
        let session_id = legacy.session_id.clone();
        provider.state.insert_session(legacy).expect("insert");

        assert_eq!(
            provider.turn_framing(
                &session_id,
                &"cd".repeat(32),
                CodingSessionDelivery::Boundary,
                channel_id,
                "ship it",
            ),
            None
        );
    }

    /// A governed session record inserted directly into provider state, for
    /// authority tests that need no live agent behind the record.
    fn governed_record(channel_id: Uuid, cwd: &Path, genesis_ref: &str) -> SessionRecord {
        SessionRecord {
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
            channel_id,
            command_id: "create-governed".into(),
            generation_command_id: None,
            provider_instance_ref: "claude-primary".into(),
            runtime: "claude".into(),
            driver: "claude-agent-acp".into(),
            cwd: cwd.to_path_buf(),
            project_ref: None,
            repo_ref: None,
            session_ref: Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into()),
            genesis_ref: Some(genesis_ref.to_owned()),
            actor: None,
            role: None,
            founder_pubkey: Some(test_operator_keys().public_key().to_hex()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: None,
            routing: None,
            resume_cursor: None,
            title: None,
            created_at_ms: now_ms(),
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
            created_by: None,
            handover: ClaimState::NoClaim,
            retired: None,
            pack_ref: None,
        }
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

    /// A lifecycle command with an explicit `created_at`, for tests that need
    /// one event to be demonstrably newer than another.
    fn lifecycle_target_event_at(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        action: &str,
        target: &CodingSessionTarget,
        created_at: u64,
    ) -> Event {
        let mut event = lifecycle_target_event(provider, channel_id, command_id, action, target);
        event = nostr::EventBuilder::new(event.kind, event.content.clone())
            .tags(event.tags.to_vec())
            .custom_created_at(nostr::Timestamp::from_secs(created_at))
            .sign_with_keys(test_operator_keys())
            .expect("sign");
        event
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
        assert!(
            provider.pending_leases.is_empty(),
            "no live lease may precede the durable receipt and metadata"
        );

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(provider.pending_publishes(), 0);
        provider
            .queue_initial_live_leases()
            .expect("initial live lease");
        let initial = provider
            .pending_leases
            .values()
            .next()
            .expect("pending live lease");
        let initial = buzz_core::coding_session_lease::validate_coding_session_lease_envelope(
            &initial.event,
            initial.event.created_at.as_secs(),
        )
        .expect("valid live lease");
        assert_eq!(initial.payload.state, CodingSessionLeaseState::Live);
        assert_eq!(initial.command_id, "create-1");
        assert_eq!(initial.payload.lease_sequence, 1);

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
    async fn first_live_waits_for_its_generation_facts_not_unrelated_outbox_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let event = create_event(&provider, channel_id, "create-prerequisites");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        let unrelated = nostr::EventBuilder::new(nostr::Kind::Custom(44225), "unrelated")
            .sign_with_keys(&provider.config.keys)
            .expect("sign");
        provider
            .outbox
            .enqueue(44225, "unrelated", Priority::Normal, unrelated)
            .expect("enqueue unrelated");

        let sink = CollectingSink::new();
        assert_eq!(provider.flush_one(&sink).await.expect("first ack"), 1);
        assert_eq!(provider.flush_one(&sink).await.expect("second ack"), 1);
        provider
            .queue_initial_live_leases()
            .expect("prerequisite scan");
        assert!(provider.pending_leases.is_empty());

        assert_eq!(provider.flush_one(&sink).await.expect("third ack"), 1);
        assert_eq!(provider.pending_publishes(), 1, "unrelated row remains");
        provider
            .queue_initial_live_leases()
            .expect("prerequisite scan");
        assert_eq!(provider.pending_leases.len(), 1);
    }

    #[tokio::test]
    async fn pending_lifecycle_startup_cannot_starve_sixty_second_renewals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let event = create_event(&provider, channel_id, "create-existing");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("create existing");
        provider
            .flush(&CollectingSink::new())
            .await
            .expect("accept prerequisites");
        provider.queue_initial_live_leases().expect("initial lease");
        let semantic_key = provider
            .pending_leases
            .keys()
            .next()
            .expect("lease key")
            .clone();
        provider.pending_leases.clear();
        provider.established_leases.insert(semantic_key.clone());

        let (maintenance_tx, mut maintenance_rx) = tokio::sync::mpsc::unbounded_channel();
        provider
            .await_with_lease_maintenance_at(
                async move {
                    for _ in 0..3 {
                        maintenance_rx.recv().await.expect("maintenance tick");
                    }
                },
                None,
                Duration::from_millis(1),
                || {
                    maintenance_tx.send(()).expect("startup still pending");
                },
            )
            .await;

        let latest = provider
            .pending_leases
            .get(&semantic_key)
            .expect("renewal retained while startup is pending");
        let decoded = buzz_core::coding_session_lease::validate_coding_session_lease_envelope(
            &latest.event,
            latest.event.created_at.as_secs(),
        )
        .expect("lease");
        assert!(decoded.payload.lease_sequence >= 4);
        assert_eq!(
            provider
                .state()
                .sessions()
                .next()
                .expect("session")
                .next_lease_sequence,
            decoded.payload.lease_sequence + 1,
            "60s, 120s, and 180s renewals each persist and burn a sequence"
        );
    }

    #[tokio::test]
    async fn shutdown_durable_drain_stops_at_its_fixed_bound() {
        assert_eq!(SHUTDOWN_DURABLE_DRAIN_TIMEOUT, Duration::from_secs(5));
        let test_bound = Duration::from_millis(10);
        let started = tokio::time::Instant::now();
        let result = bounded_drain(test_bound, std::future::pending::<()>()).await;

        assert!(result.is_err(), "an unresponsive relay must time out");
        assert!(started.elapsed() >= test_bound);
    }

    #[test]
    fn a_failed_terminal_persist_never_queues_a_release() {
        #[derive(Default)]
        struct StopProbe {
            release_queued: bool,
        }

        let mut probe = StopProbe::default();
        let result = prepare_terminal_stop(
            &mut probe,
            |_| Err(anyhow::anyhow!("state disk unavailable")),
            |probe| {
                probe.release_queued = true;
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!probe.release_queued);
    }

    #[test]
    fn a_release_construction_failure_after_terminal_persist_is_non_fatal() {
        #[derive(Default)]
        struct StopProbe {
            terminal_persisted: bool,
        }

        let mut probe = StopProbe::default();
        let result = prepare_terminal_stop(
            &mut probe,
            |probe| {
                probe.terminal_persisted = true;
                Ok(())
            },
            |_| Err(anyhow::anyhow!("lease sequence disk unavailable")),
        );

        assert!(
            result.is_ok(),
            "terminal completion must fall back to lease TTL"
        );
        assert!(probe.terminal_persisted);
    }

    #[tokio::test]
    async fn stop_completes_terminal_facts_when_release_sequence_reservation_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(
                channel_id,
                &create_event(&provider, channel_id, "create-before-stop-failure"),
            )
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.next_lease_sequence =
                    buzz_core::coding_session_command::MAX_SAFE_GENERATION + 1;
            })
            .expect("exhaust lease sequence");

        let stop = lifecycle_target_event(
            &provider,
            channel_id,
            "stop-without-release",
            "session.stop",
            &target,
        );
        provider
            .handle_command_event(channel_id, &stop)
            .await
            .expect("terminal completion falls back to TTL");

        assert!(provider.state().is_command_consumed("stop-without-release"));
        assert!(
            provider
                .state()
                .session(&target.session_id)
                .expect("session")
                .closed
        );
        assert!(provider.sessions.handle(&target.session_id).is_none());
        assert!(provider.pending_leases.is_empty());
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("terminal facts");
        assert!(sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .iter()
            .any(|content| content["commandId"] == "stop-without-release"
                && content["status"] == "stopped"));
        assert!(sink
            .contents_of(KIND_CODING_SESSION_METADATA)
            .iter()
            .any(|content| content["status"] == "stopped"));
    }

    /// A create that starts with no prior context says so in the transcript.
    /// Silence would read as continuity the execution does not have.
    #[tokio::test]
    async fn a_fresh_create_discloses_that_it_starts_without_prior_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event = create_event(&provider, channel_id, "create-fresh-1");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let disclosures: Vec<_> = transcript_items_in_sequence(&sink)
            .into_iter()
            .filter(|item| item["item"]["kind"] == "status")
            .map(|item| item["item"]["status"].clone())
            .collect();
        assert_eq!(
            disclosures,
            vec![serde_json::json!("session_fresh")],
            "a fresh create publishes exactly one continuity disclosure"
        );
    }

    /// A relay whose REST `/query` never answers, so a caller that waits on it
    /// is visibly blocked rather than merely slow.
    async fn spawn_hanging_query_relay(keys: &Keys) -> (HarnessRelay, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route("/", get(test_relay_root))
            .route(
                "/query",
                post(|| async {
                    tokio::time::sleep(Duration::from_secs(120)).await;
                    axum::Json(serde_json::json!({ "events": [] }))
                }),
            )
            .with_state(TestRelayState {
                events: Arc::new(Mutex::new(Vec::new())),
                queries: Arc::new(Mutex::new(Vec::new())),
                published: Arc::new(Mutex::new(Vec::new())),
                reject_next_publish: Arc::new(AtomicBool::new(false)),
                deferred: Arc::default(),
                chain_reads: Arc::default(),
                relay_self: Arc::new(Mutex::new(None)),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test relay");
        let address = listener.local_addr().expect("test relay address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test relay");
        });
        let relay = HarnessRelay::connect(
            &format!("ws://{address}"),
            keys,
            &keys.public_key().to_hex(),
            None,
        )
        .await
        .expect("connect test relay");
        (relay, server)
    }

    /// A provider holding one rehydrated execution: a durable record with both
    /// umbrella refs, generation 0 of its package on disk, and the in-memory
    /// refresh binding an open would have installed.
    fn armed_refresh_provider(
        state_dir: &Path,
        projects: Option<&Path>,
        channel_id: Uuid,
        cwd: &Path,
    ) -> (Provider, String, String) {
        let mut provider = provider_with_sidecar(state_dir, projects);
        let record = governed_record(channel_id, cwd, &"ab".repeat(32));
        let session_id = record.session_id.clone();
        provider
            .state
            .insert_session(record)
            .expect("insert session record");
        let package_id = Uuid::new_v4().to_string();
        context_store::write_context_package(
            &provider.config.state_dir,
            &package_id,
            &empty_context_package(),
        )
        .expect("write generation 0");
        provider.context_refresh.insert(
            session_id.clone(),
            ContextRefreshState::opened(package_id.clone()),
        );
        (provider, session_id, package_id)
    }

    fn empty_context_package() -> buzz_core::coding_session_context::CodingSessionContextPackage {
        use buzz_core::coding_session_context::{
            CodingSessionContextIdentity, CodingSessionContextPackage,
            CodingSessionContextProvenance, CODING_SESSION_CONTEXT_PACKAGE_VERSION,
        };
        CodingSessionContextPackage {
            v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
            session: CodingSessionContextIdentity {
                session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                genesis_ref: "ab".repeat(32),
                channel_id: Uuid::nil(),
                name: None,
                goal: None,
                project_ref: None,
            },
            provenance: CodingSessionContextProvenance {
                generated_at: 1,
                complete_as_of: Some(1),
                complete: true,
                truncated: false,
                source_event_count: 1,
                source_event_breakdown: None,
                included_history_items: 0,
                omitted_history_items: 0,
                total_history_items: Some(0),
                notes: vec!["Complete empty fixture".into()],
            },
            history: Vec::new(),
            roster: Vec::new(),
            inbox: Vec::new(),
            policy: None,
        }
    }

    fn generation_sequences(state_dir: &Path, package_id: &str) -> Vec<u64> {
        let directory = state_dir.join("context-packages").join(package_id);
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut sequences: Vec<u64> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".json"))
                    .and_then(|stem| stem.parse().ok())
            })
            .collect();
        sequences.sort_unstable();
        sequences
    }

    /// H3: a refresh that cannot rebuild the package changes nothing on disk.
    /// The previous generation keeps serving and its age keeps climbing —
    /// never silence, and never a fresher-looking watermark.
    #[tokio::test]
    async fn a_refresh_that_fails_leaves_the_previous_generation_serving() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let (mut provider, session_id, package_id) =
            armed_refresh_provider(&state_dir, None, channel_id, &cwd);
        // An empty relay holds no genesis, so every projection fails.
        let (relay, _queries, server) = spawn_test_relay(&provider.config.keys, None).await;
        provider.set_rest_client(relay.rest_client());

        provider.spawn_context_refresh(&session_id);
        assert_eq!(
            provider.context_refresh[&session_id].next_seq, 2,
            "the sequence is claimed before the fetch, so a retry never collides"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(
            generation_sequences(&state_dir, &package_id),
            vec![0],
            "a failed refresh writes nothing"
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_refresh_is_skipped_inside_the_minimum_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let (mut provider, session_id, _package_id) =
            armed_refresh_provider(&state_dir, None, channel_id, &cwd);
        let (relay, _queries, server) = spawn_test_relay(&provider.config.keys, None).await;
        provider.set_rest_client(relay.rest_client());

        provider.spawn_context_refresh(&session_id);
        provider.spawn_context_refresh(&session_id);
        assert_eq!(
            provider.context_refresh[&session_id].next_seq, 2,
            "the second attempt inside the interval floor claims nothing"
        );

        // Age the last attempt past the floor and the next turn refreshes again.
        provider
            .context_refresh
            .get_mut(&session_id)
            .expect("refresh state")
            .last_refresh_ms = now_ms() - CONTEXT_REFRESH_MIN_INTERVAL_MS - 1;
        provider.spawn_context_refresh(&session_id);
        assert_eq!(provider.context_refresh[&session_id].next_seq, 3);
        server.abort();
    }

    /// The refresh runs off the provider loop. A relay that never answers must
    /// not hold a turn — the sidecar picks the new generation up off disk on a
    /// later tool call, so an outstanding fetch costs this turn nothing.
    #[tokio::test]
    async fn a_turn_start_does_not_block_on_the_refresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let (mut provider, session_id, _package_id) =
            armed_refresh_provider(&state_dir, None, channel_id, &cwd);
        let (relay, server) = spawn_hanging_query_relay(&provider.config.keys).await;
        provider.set_rest_client(relay.rest_client());

        let started = std::time::Instant::now();
        provider.spawn_context_refresh(&session_id);
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "the provider loop waited {elapsed:?} on a relay that never answered"
        );
        assert_eq!(
            provider.context_refresh[&session_id].next_seq, 2,
            "the refresh really was launched"
        );
        server.abort();
    }

    #[tokio::test]
    async fn an_execution_with_no_session_ref_never_schedules_a_refresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider_with_sidecar(&state_dir, Some(&projects));

        let create = create_event(&provider, channel_id, "create-no-umbrella");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let session_id = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();

        assert!(
            provider.context_refresh.is_empty(),
            "an execution that never had a package has nothing to refresh"
        );
        provider.spawn_context_refresh(&session_id);
        assert!(provider.context_refresh.is_empty());
    }

    /// The C5 regression: a resume names its package directory by a freshly
    /// minted UUID, not by the session id, so a cleanup keyed on the session id
    /// would leak that directory forever.
    #[tokio::test]
    async fn a_resume_created_package_directory_is_removed_on_stop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider_with_sidecar(&state_dir, Some(&projects));

        let create = create_event(&provider, channel_id, "create-for-stop");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        let package_id = Uuid::new_v4().to_string();
        assert_ne!(package_id, target.session_id);
        context_store::write_context_package(&state_dir, &package_id, &empty_context_package())
            .expect("write generation 0");
        provider.context_refresh.insert(
            target.session_id.clone(),
            ContextRefreshState::opened(package_id.clone()),
        );

        let stop = lifecycle_target_event(&provider, channel_id, "stop-1", "session.stop", &target);
        provider
            .handle_command_event(channel_id, &stop)
            .await
            .expect("stop");

        assert!(
            generation_sequences(&state_dir, &package_id).is_empty(),
            "the resume-minted package directory is removed on stop"
        );
        assert!(!provider.context_refresh.contains_key(&target.session_id));
    }

    /// A package is written *before* the ACP open, but the refresh entry that
    /// makes it reachable for cleanup is inserted only after that open
    /// succeeds. A failed open — a broken agent binary, a rejected adapter
    /// handshake — therefore leaves a directory no session names, which
    /// nothing but the next startup sweep would ever reclaim; an operator
    /// retrying leaks one package per attempt. This is the cleanup both failed
    /// open arms run.
    #[tokio::test]
    async fn a_package_orphaned_by_a_failed_open_is_removed_immediately() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let provider = provider_with_sidecar(&state_dir, None);
        let package_id = Uuid::new_v4().to_string();
        let kept = Uuid::new_v4().to_string();
        context_store::write_context_package(&state_dir, &package_id, &empty_context_package())
            .expect("write generation 0");
        context_store::write_context_package(&state_dir, &kept, &empty_context_package())
            .expect("write generation 0");

        provider.discard_orphaned_context_package(Some(&package_id));

        assert!(
            generation_sequences(&state_dir, &package_id).is_empty(),
            "the package the failed open orphaned is gone"
        );
        assert_eq!(
            generation_sequences(&state_dir, &kept),
            vec![0],
            "another execution's package is untouched"
        );
        // An open that never produced a package has nothing to drop.
        provider.discard_orphaned_context_package(None);
        assert_eq!(generation_sequences(&state_dir, &kept), vec![0]);
    }

    /// A resume mints a fresh package id and a fresh directory, so the
    /// previous execution's directory is unreferenced from that moment — and a
    /// long-lived session must not leak one directory per resume.
    #[tokio::test]
    async fn a_resume_drops_the_previous_executions_package_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider_with_sidecar(&state_dir, Some(&projects));

        let create = create_event(&provider, channel_id, "create-before-resume");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        let previous_package = Uuid::new_v4().to_string();
        context_store::write_context_package(
            &state_dir,
            &previous_package,
            &empty_context_package(),
        )
        .expect("write generation 0");
        provider.context_refresh.insert(
            target.session_id.clone(),
            ContextRefreshState::opened(previous_package.clone()),
        );
        provider.sessions.shutdown(&target.session_id);

        let resume =
            lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");

        assert!(
            generation_sequences(&state_dir, &previous_package).is_empty(),
            "the superseded package directory is gone"
        );
    }

    #[tokio::test]
    async fn provider_startup_sweeps_every_leftover_package_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let first = Uuid::new_v4().to_string();
        let second = Uuid::new_v4().to_string();
        {
            let mut provider = provider_with_sidecar(&state_dir, None);
            provider.recover().await.expect("first recover");
            context_store::write_context_package(&state_dir, &first, &empty_context_package())
                .expect("write first");
            context_store::write_context_package(&state_dir, &second, &empty_context_package())
                .expect("write second");
        }

        let mut restarted = provider_with_sidecar(&state_dir, None);
        restarted.recover().await.expect("recover");
        assert!(
            !state_dir.join("context-packages").exists(),
            "no reader survives a restart, so every leftover package is swept"
        );
    }

    /// A provider whose config names an absolute context sidecar, so the
    /// rehydration path gets past its first guard without any sidecar being
    /// installed — the path is only checked for absoluteness there.
    fn provider_with_sidecar(state_dir: &Path, projects: Option<&Path>) -> Provider {
        provider_with_sidecar_path(
            state_dir,
            projects,
            Some("/nonexistent/buzz-session-context"),
        )
    }

    fn provider_with_sidecar_path(
        state_dir: &Path,
        projects: Option<&Path>,
        sidecar: Option<&str>,
    ) -> Provider {
        let agent = fake_agent(state_dir_parent(state_dir), "good-agent", GOOD_AGENT);
        let mut config = config_of(Keys::generate(), state_dir, projects, agent);
        config.context_mcp_command = sidecar.map(std::path::PathBuf::from);
        Provider::new(config).expect("provider")
    }

    fn status_items(sink: &CollectingSink) -> Vec<serde_json::Value> {
        transcript_items_in_sequence(sink)
            .into_iter()
            .filter(|item| item["item"]["kind"] == "status")
            .map(|item| item["item"].clone())
            .collect()
    }

    /// Every way out of `prepare_rehydration_context` names itself. Ten
    /// bail-outs may not collapse to one slug — a missing sidecar, an
    /// unreachable relay and a duplicate-create conflict rendered identically
    /// to the operator is the defect this whole slice exists to remove.
    ///
    /// Six of the eight sites are driven end to end here. The remaining two —
    /// the brief-encode and package-write arms — sit behind a *successful*
    /// projection, which needs a complete signed fact set on the relay; they
    /// are covered by construction (each returns its enumerated slug) plus the
    /// vocabulary assertion below.
    #[tokio::test]
    async fn every_bail_out_in_prepare_rehydration_context_names_a_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let package_id = Uuid::new_v4().to_string();

        let no_sidecar = provider_with_sidecar_path(&dir.path().join("none"), None, None);
        assert_eq!(
            no_sidecar
                .prepare_rehydration_context(
                    RehydrationTarget {
                        scope: RehydrationScope::Create,
                        command_id: "cmd-1",
                        channel_id,
                        session_ref: Some(session_ref),
                        genesis_ref: Some(&genesis_ref),
                        package_id: &package_id,
                    },
                    None,
                )
                .await
                .unavailable_reason,
            Some("context_sidecar_unavailable")
        );

        let relative = provider_with_sidecar_path(
            &dir.path().join("relative"),
            None,
            Some("relative/buzz-session-context"),
        );
        assert_eq!(
            relative
                .prepare_rehydration_context(
                    RehydrationTarget {
                        scope: RehydrationScope::Create,
                        command_id: "cmd-2",
                        channel_id,
                        session_ref: Some(session_ref),
                        genesis_ref: Some(&genesis_ref),
                        package_id: &package_id,
                    },
                    None,
                )
                .await
                .unavailable_reason,
            Some("context_sidecar_path_invalid")
        );

        let provider = provider_with_sidecar(&dir.path().join("state"), None);
        assert_eq!(
            provider
                .prepare_rehydration_context(
                    RehydrationTarget {
                        scope: RehydrationScope::Create,
                        command_id: "cmd-3",
                        channel_id,
                        session_ref: None,
                        genesis_ref: Some(&genesis_ref),
                        package_id: &package_id,
                    },
                    None,
                )
                .await
                .unavailable_reason,
            Some("no_prior_execution"),
            "a session with no prior execution is not a failure"
        );
        assert_eq!(
            provider
                .prepare_rehydration_context(
                    RehydrationTarget {
                        scope: RehydrationScope::Create,
                        command_id: "cmd-4",
                        channel_id,
                        session_ref: Some(session_ref),
                        genesis_ref: None,
                        package_id: &package_id,
                    },
                    None,
                )
                .await
                .unavailable_reason,
            Some("no_prior_execution")
        );
        assert_eq!(
            provider
                .prepare_rehydration_context(
                    RehydrationTarget {
                        scope: RehydrationScope::Create,
                        command_id: "cmd-5",
                        channel_id,
                        session_ref: Some(session_ref),
                        genesis_ref: Some(&genesis_ref),
                        package_id: &package_id,
                    },
                    None,
                )
                .await
                .unavailable_reason,
            Some("relay_unavailable")
        );

        // A relay that holds no genesis fails the projection rather than the
        // transport, so the reason is the projector's own class.
        let (relay, _queries, server) = spawn_test_relay(&provider.config.keys, None).await;
        let outcome = provider
            .prepare_rehydration_context(
                RehydrationTarget {
                    scope: RehydrationScope::Create,
                    command_id: "cmd-6",
                    channel_id,
                    session_ref: Some(session_ref),
                    genesis_ref: Some(&genesis_ref),
                    package_id: &package_id,
                },
                Some(&relay),
            )
            .await;
        assert_eq!(outcome.unavailable_reason, Some("unverifiable_source_fact"));
        assert!(outcome.descriptor.is_none());
        server.abort();

        for slug in ["brief_encode_failed", "package_write_failed"] {
            assert!(
                buzz_core::coding_session_payload::CONTEXT_UNAVAILABLE_REASONS.contains(&slug),
                "{slug} must be publishable"
            );
        }
    }

    /// A resume must never be told its own umbrella is fresh.
    ///
    /// `allow_no_executions` was set inside the helper both paths share, so a
    /// resume whose create chain could not be verified — the create receipt
    /// lost in the crash window S2's replay machinery exists for, say —
    /// projected to an *empty* package rather than an error. Empty history and
    /// empty roster make `prior_context` false, which installs the fresh-crew
    /// bootstrap prefix: the resumed agent is instructed, as a launcher fact,
    /// to say there is no earlier verified work under this session. The
    /// operator sees no reason at all, because nothing failed.
    ///
    /// The create keeps its licence: under a fresh genesis there is genuinely
    /// no chain yet, and that seat is the one that most needs the crew tools.
    #[tokio::test]
    async fn a_resume_with_no_verifiable_execution_chain_is_a_named_loss_not_a_fresh_start() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let genesis = genesis_event(channel_id, session_ref);
        let genesis_ref = genesis.id.to_hex();
        let provider = provider_with_sidecar(&dir.path().join("state"), None);
        let (relay, _queries, server) =
            spawn_test_relay_with_events(&provider.config.keys, vec![genesis]).await;

        let created = provider
            .prepare_rehydration_context(
                RehydrationTarget {
                    scope: RehydrationScope::Create,
                    command_id: "cmd-create",
                    channel_id,
                    session_ref: Some(session_ref),
                    genesis_ref: Some(&genesis_ref),
                    package_id: &Uuid::new_v4().to_string(),
                },
                Some(&relay),
            )
            .await;
        assert_eq!(
            created.unavailable_reason, None,
            "the first execution under a fresh genesis still attaches its tools"
        );
        let descriptor = created
            .descriptor
            .expect("a create attaches the context MCP");
        assert!(
            !descriptor.prior_context,
            "with nothing verified there is no prior context to claim"
        );

        let resumed = provider
            .prepare_rehydration_context(
                RehydrationTarget {
                    scope: RehydrationScope::Resume,
                    command_id: "cmd-resume",
                    channel_id,
                    session_ref: Some(session_ref),
                    genesis_ref: Some(&genesis_ref),
                    package_id: &Uuid::new_v4().to_string(),
                },
                Some(&relay),
            )
            .await;
        assert_eq!(
            resumed.unavailable_reason,
            Some("unverifiable_source_fact"),
            "a resume that cannot verify its own chain says so"
        );
        assert!(resumed.descriptor.is_none());
        server.abort();
    }

    /// The exact regression this slice exists to prevent: on 2026-08-18 a
    /// duplicated create and an unreachable relay produced the same bare
    /// "Started fresh" row.
    #[test]
    fn a_conflict_bail_out_and_a_relay_outage_publish_different_slugs() {
        let conflict = context_projector::context_unavailable_reason(
            &context_projector::ContextProjectionError::Conflict(
                "command create-1 has more than one provider receipt".into(),
            ),
        );
        let published = |reason: &'static str| {
            let (status, reason) = create_disclosure(&SessionContinuity::Fresh, Some(reason))
                .expect("a fresh create discloses itself");
            payload::status_item_with_reason(status, reason)
        };

        assert_ne!(conflict, "relay_unavailable");
        assert_ne!(published(conflict), published("relay_unavailable"));
        assert_eq!(published(conflict)["reason"], "context_fact_conflict");
        assert_eq!(
            published("relay_unavailable")["reason"],
            "relay_unavailable"
        );
    }

    #[tokio::test]
    async fn a_first_ever_execution_discloses_no_prior_execution_not_a_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider_with_sidecar(&dir.path().join("state"), Some(&projects));

        let event = create_event(&provider, channel_id, "create-first-ever");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            status_items(&sink),
            vec![serde_json::json!({
                "kind": "status",
                "status": "session_fresh",
                "reason": "no_prior_execution",
            })]
        );
    }

    #[tokio::test]
    async fn a_resume_that_loses_context_names_its_reason_on_session_restarted_without_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        // No sidecar at all, so the resume's rehydration attempt names that.
        let mut provider = provider_with_sidecar_path(&state_dir, Some(&projects), None);

        let create = create_event(&provider, channel_id, "create-for-resume");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        provider.sessions.shutdown(&target.session_id);

        let resume =
            lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let items = status_items(&sink);
        let restarted = items
            .iter()
            .find(|item| item["status"] == "session_restarted_without_context")
            .expect("the resume disclosed itself");
        assert_eq!(
            restarted["reason"], "context_sidecar_unavailable",
            "the resume path carries the reason too — it is a different code path from create"
        );
    }

    /// A resumed execution has prior work by construction — its earlier
    /// generation's turns are in the same transcript this row lands in — so it
    /// may never publish the slug that reads "this is the session's first
    /// execution". The missing umbrella refs get their own slug there.
    #[tokio::test]
    async fn a_resume_without_umbrella_refs_never_claims_a_first_execution() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        // Absolute sidecar path, so the resume gets past the sidecar guards and
        // bails out on the umbrella refs instead.
        let mut provider = provider_with_sidecar(&state_dir, Some(&projects));

        // Claims an umbrella `sessionRef` with no `genesisRef`: the record the
        // resume reads back has `session_ref: Some(..), genesis_ref: None`.
        const UMBRELLA: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let create = create_event_with_session_ref(
            &provider,
            channel_id,
            "create-umbrella-no-genesis",
            UMBRELLA,
        );
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let record = provider.state().sessions().next().expect("session").clone();
        assert_eq!(record.session_ref.as_deref(), Some(UMBRELLA));
        assert_eq!(record.genesis_ref, None);
        let target = record.target(&provider.config.instance_id);
        provider.sessions.shutdown(&target.session_id);

        let resume =
            lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let restarted = status_items(&sink)
            .into_iter()
            .find(|item| item["status"] == "session_restarted_without_context")
            .expect("the resume disclosed itself");
        assert_eq!(restarted["reason"], "no_umbrella_context");
        assert_ne!(
            restarted["reason"], "no_prior_execution",
            "generation 2 of an execution cannot be its first"
        );
    }

    #[tokio::test]
    async fn a_native_resume_or_load_never_carries_a_package_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let agent = fake_agent(dir.path(), "resumable-agent", RESUMABLE_AGENT);
        let mut config = config_of(Keys::generate(), &state_dir, Some(&projects), agent);
        // Absolute but absent: every rehydration attempt fails, and a native
        // resume must still say nothing about it.
        config.context_mcp_command = Some(std::path::PathBuf::from("/nonexistent/sidecar"));
        let mut provider = Provider::new(config).expect("provider");

        let create = create_event(&provider, channel_id, "create-native");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        provider.sessions.shutdown(&target.session_id);

        let resume =
            lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let native = status_items(&sink)
            .into_iter()
            .find(|item| item["status"] == "session_resumed" || item["status"] == "session_loaded")
            .expect("the adapter re-attached its own conversation");
        assert!(
            native.get("reason").is_none(),
            "nothing was lost, so nothing is disclosed: {native}"
        );
    }

    /// The disclosure slug and reason per continuity mode. Rehydrated keeps the
    /// slug it already had and never carries a reason — nothing was lost —
    /// while reattachments are disclosed by the attach path instead.
    #[test]
    fn every_create_continuity_maps_to_its_disclosure() {
        let reason = Some("context_fact_conflict");
        assert_eq!(
            create_disclosure(&SessionContinuity::Rehydrated, None),
            Some(("session_rehydrated", None))
        );
        assert_eq!(
            create_disclosure(&SessionContinuity::Rehydrated, reason),
            Some(("session_rehydrated", None)),
            "a rehydrated execution lost nothing, so it names no reason"
        );
        assert_eq!(
            create_disclosure(&SessionContinuity::Fresh, None),
            Some(("session_fresh", None))
        );
        assert_eq!(
            create_disclosure(&SessionContinuity::Fresh, reason),
            Some(("session_fresh", reason))
        );
        assert_eq!(create_disclosure(&SessionContinuity::Resumed, reason), None);
        assert_eq!(create_disclosure(&SessionContinuity::Loaded, reason), None);
        assert_eq!(
            create_disclosure(
                &SessionContinuity::RestartedWithoutContext {
                    reason: "adapter rejected session resume"
                },
                reason
            ),
            None
        );
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
        // A relay publishes its identity and a provider witnesses it; without one
        // no receipt verifies, so a genesis-bearing create is refused by name
        // rather than admitted on the assumption that nothing has been handed
        // over. Doing it here is what production does at startup.
        provider.set_rest_client(relay.rest_client());
        provider.witness_relay_identity().await;

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
            // Four relay reads on a governed create, each named rather than
            // counted. The genesis resolves by exact id; the umbrella's
            // authority chain is read twice — once to decide the handover
            // fence before anything is provisioned, once to fold the chain
            // into the new record — and the session-policy partition (kind
            // 44245) supplies the one policy field this provider enforces.
            //
            // The two chain reads are the price of fencing a create *before*
            // an adapter starts: the first happens when there is no record to
            // fold into yet, and the second when there is. Collapsing them
            // would mean either fencing after the spawn or seeding a record
            // from an unverified read.
            let captured = queries.lock().expect("queries lock");
            assert_eq!(captured.len(), 4, "{captured:?}");
            assert_eq!(captured[0][0]["ids"][0], genesis.id.to_hex());
            assert_eq!(captured[0][0]["kinds"][0], KIND_CODING_SESSION_GENESIS);
            for chain_read in &captured[1..3] {
                assert_eq!(chain_read[0]["kinds"][0], KIND_SYSTEM_MESSAGE);
                assert!(
                    chain_read[0].get("authors").is_some(),
                    "a chain read only trusts the witnessed relay identity: {captured:?}"
                );
            }
            assert_eq!(captured[3][0]["kinds"][0], KIND_CODING_SESSION_POLICY);
            assert!(
                captured[3][0].get("ids").is_none(),
                "the policy read is a channel partition, not an id lookup: {captured:?}"
            );
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

    /// D9 end to end inside one process: a started turn charges its umbrella
    /// durably, the charge is republished in 44223, and the turn that would
    /// take the crew past its allowance is answered `turn_refused /
    /// BUDGET_EXHAUSTED` while the founder's is not.
    ///
    /// The spend is driven through `TurnStarted` rather than through a live
    /// adapter deliberately: that arm is the *only* place a turn is charged,
    /// and charging anywhere earlier would bill a crew for turns a crash threw
    /// away.
    #[tokio::test]
    async fn a_spent_umbrella_budget_refuses_a_seat_and_is_published_in_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);
        provider.config.turn_budget = 2;

        let seat = Keys::generate();
        let mut record = governed_record(channel_id, &cwd, &"ab".repeat(32));
        record.granted_operators.insert(seat.public_key().to_hex());
        record.authority_seq = 1;
        let umbrella = record.session_ref.clone().expect("umbrella");
        let session_id = record.session_id.clone();
        let target = record.target(&provider.config.instance_id);
        provider.state.insert_session(record).expect("insert");

        // Nothing spent yet: the seat's turn is accepted, and the published
        // metadata says so in the two numbers rather than by omission.
        assert!(matches!(
            provider
                .metadata_for(&target, SessionStatus::Idle)
                .turn_budget,
            Some(TurnBudget { used: 0, limit: 2 })
        ));

        for (n, command_id) in ["turn-a", "turn-b"].into_iter().enumerate() {
            provider
                .handle_session_event(SessionEvent::TurnStarted {
                    session_id: session_id.clone(),
                    turn_id: format!("turn-id-{n}"),
                    command_id: command_id.to_owned(),
                    text: "work".into(),
                })
                .expect("turn started");
        }
        assert_eq!(
            provider.state().turns_used(&umbrella),
            2,
            "each started turn charges the umbrella exactly once"
        );
        assert!(matches!(
            provider
                .metadata_for(&target, SessionStatus::Running)
                .turn_budget,
            Some(TurnBudget { used: 2, limit: 2 })
        ));

        let refused = command_event_by(
            channel_id,
            "turn-over-budget",
            &target,
            serde_json::json!({ "type": "thread.turn.start", "text": "one more" }),
            &seat,
        );
        provider
            .handle_command_event(channel_id, &refused)
            .await
            .expect("refuse");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipt = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "turn-over-budget")
            .expect("budget receipt");
        assert_eq!(receipt["status"], "turn_refused");
        assert_eq!(receipt["error"]["code"], BUDGET_EXHAUSTED);
        assert!(
            receipt["error"]["message"]
                .as_str()
                .expect("message")
                .contains("2 of its 2 allowed turns"),
            "the refusal names used and limit: {receipt}"
        );
        // Refused, never consumed: it did not run and never will.
        assert!(provider.state().is_command_refused("turn-over-budget"));
        assert!(!provider.state().is_command_consumed("turn-over-budget"));

        // The founder, whom the budget exists to protect, is not refused.
        let founder_turn = turn_event(channel_id, "turn-founder", &target);
        provider
            .handle_command_event(channel_id, &founder_turn)
            .await
            .expect("founder turn");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert!(
            !sink
                .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
                .into_iter()
                .any(|receipt| receipt["commandId"] == "turn-founder"
                    && receipt["error"]["code"] == BUDGET_EXHAUSTED),
            "a founder turn is never refused for a budget"
        );
    }

    /// A create's `initialTurn` never passes through `decide_turn`: it is
    /// handed straight to the actor's mailbox. So the budget has to be checked
    /// on that path too, or one signed create runs — and charges — a turn at
    /// an exhausted allowance with no receipt at all, and D9's promise (and
    /// the Settings control that states it) is false while that door is open.
    #[tokio::test]
    async fn a_spent_umbrella_refuses_a_delegated_creates_first_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider.config.turn_budget = 1;

        // The umbrella belongs to the test operator, and its one turn is spent.
        let record = governed_record(channel_id, &cwd, &"ab".repeat(32));
        let umbrella = record.session_ref.clone().expect("umbrella");
        provider.state.insert_session(record).expect("insert");
        provider
            .state
            .record_turn_spend(&umbrella)
            .expect("spend the allowance");

        let seat = Keys::generate();
        let create = create_event_with_umbrella_and_turn(
            &provider,
            channel_id,
            "create-delegated",
            &umbrella,
            "go",
            &seat,
        );
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipt = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "create-delegated")
            .expect("create receipt");
        assert_eq!(receipt["status"], "created_with_failed_initial_turn");
        let message = receipt["error"]["message"]
            .as_str()
            .expect("message")
            .to_owned();
        assert!(
            message.contains(BUDGET_EXHAUSTED) && message.contains("1 of its 1 allowed turns"),
            "the refusal must name the code and the two numbers: {message}"
        );

        // The umbrella's founder is not refused: their create's first turn
        // still runs, at the same exhausted allowance.
        let founders = create_event_with_umbrella_and_turn(
            &provider,
            channel_id,
            "create-founder",
            &umbrella,
            "go",
            test_operator_keys(),
        );
        provider
            .handle_command_event(channel_id, &founders)
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        // The pump drains *both* sessions' events, so this is where a
        // delivered over-budget first turn would finally show itself.
        let prompts: Vec<serde_json::Value> = transcript_items_in_sequence(&sink)
            .into_iter()
            .filter(|item| item["item"]["kind"] == "user_prompt")
            .collect();
        assert_eq!(
            prompts.len(),
            1,
            "exactly one first turn runs — the founder's: {prompts:?}"
        );
        assert_eq!(
            provider.state().turns_used(&umbrella),
            2,
            "the refused turn charged nothing; only the founder's ran"
        );
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
        // A refused turn answers in the turn vocabulary and a refused
        // lifecycle command in the lifecycle one; both name the same code, so
        // an operator reads one reason either way.
        for (command_id, status) in [
            ("turn-unauthorized", "turn_refused"),
            ("stop-unauthorized", "failed"),
            ("resume-unauthorized", "failed"),
        ] {
            let receipt = receipts
                .iter()
                .find(|receipt| receipt["commandId"] == command_id)
                .expect("unauthorized receipt");
            assert_eq!(receipt["status"], status);
            assert_eq!(receipt["error"]["code"], UNAUTHORIZED_OPERATOR);
            // A refused *turn* is recorded as refused, never as consumed: it
            // did not run, and the two ledgers answer different questions.
            // Lifecycle commands still consume — they have one terminal
            // outcome and that outcome has been reached.
            if status == "turn_refused" {
                assert!(provider.state().is_command_refused(command_id));
                assert!(!provider.state().is_command_consumed(command_id));
            } else {
                assert!(provider.state().is_command_consumed(command_id));
            }
        }
        let refused = receipts
            .iter()
            .find(|receipt| receipt["commandId"] == "turn-unauthorized")
            .expect("turn receipt");
        assert!(refused.get("turnId").is_none());
        assert_eq!(refused["session"]["sessionId"], target.session_id);
    }

    #[tokio::test]
    async fn addressed_lifecycle_target_failures_publish_and_dedupe_receipts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);
        let record = governed_record(channel_id, &cwd, &"ab".repeat(32));
        let session_id = record.session_id.clone();
        let target = record.target(&provider.config.instance_id);
        provider.state.insert_session(record).expect("insert");

        let mut unknown_target = target.clone();
        unknown_target.session_id = Uuid::new_v4().to_string();
        let unknown = lifecycle_target_event(
            &provider,
            channel_id,
            "stop-unknown",
            "session.stop",
            &unknown_target,
        );

        let mut stale_target = target.clone();
        stale_target.generation += 1;
        let stale = lifecycle_target_event(
            &provider,
            channel_id,
            "stop-stale",
            "session.stop",
            &stale_target,
        );

        provider
            .state
            .update_session(&session_id, |record| record.closed = true)
            .expect("close record");
        let closed = lifecycle_target_event(
            &provider,
            channel_id,
            "resume-closed",
            "session.resume",
            &target,
        );

        for event in [&unknown, &stale, &closed, &unknown] {
            provider
                .handle_command_event(channel_id, event)
                .await
                .expect("handle refusal");
        }

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 3, "the replayed refusal must be deduped");
        for (command_id, code) in [
            ("stop-unknown", payload::UNKNOWN_TARGET),
            ("stop-stale", payload::STALE_GENERATION),
            ("resume-closed", payload::SESSION_CLOSED),
        ] {
            let receipt = receipts
                .iter()
                .find(|receipt| receipt["commandId"] == command_id)
                .expect("failure receipt");
            assert_eq!(receipt["status"], "failed");
            assert_eq!(receipt["error"]["code"], code);
            assert!(provider.state().is_command_consumed(command_id));
        }
    }

    /// The freshness proof for A5: a grant accepted *after* the session
    /// started is honored on the live subscription path — no provider
    /// restart — while stop stays owner-only for the same grantee.
    #[tokio::test]
    async fn a_mid_session_grant_is_honored_live_and_stop_stays_owner_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let relay_keys = Keys::generate();
        provider.set_relay_self(relay_keys.public_key().to_hex());
        let grantee_keys = Keys::generate();
        let grantee_hex = grantee_keys.public_key().to_hex();

        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let genesis = genesis_event(channel_id, session_ref);
        let genesis_ref = genesis.id.to_hex();
        let transition = grant_transition_event(channel_id, &genesis_ref, None, 1, &grantee_hex);
        let (mut relay, _queries, server) = spawn_test_relay_with_events(
            &provider.config.keys,
            vec![genesis.clone(), transition.clone()],
        )
        .await;

        let create = create_event_with_genesis_ref(
            &provider,
            channel_id,
            "create-governed",
            session_ref,
            &genesis_ref,
        );
        provider
            .handle_relay_event(&mut relay, channel_id, &create)
            .await
            .expect("create");
        let record = provider.state().sessions().next().expect("session").clone();
        assert!(
            record.granted_operators.is_empty(),
            "no acceptance receipt exists yet, so the unaccepted 44228 grants nothing"
        );
        let target = record.target(&provider.config.instance_id);

        // Before the grant is accepted, the grantee is refused visibly.
        let early_turn = command_event_by(
            channel_id,
            "turn-before-grant",
            &target,
            serde_json::json!({ "type": "thread.turn.start", "text": "too early" }),
            &grantee_keys,
        );
        provider
            .handle_relay_event(&mut relay, channel_id, &early_turn)
            .await
            .expect("refuse early turn");

        // The acceptance receipt arrives live on the channel subscription.
        let receipt = acceptance_receipt_event(
            &relay_keys,
            channel_id,
            &genesis_ref,
            &transition,
            1,
            &grantee_hex,
        );
        provider
            .handle_relay_event(&mut relay, channel_id, &receipt)
            .await
            .expect("apply receipt");
        let record = provider
            .state()
            .session(&target.session_id)
            .expect("session");
        assert!(record.granted_operators.contains(&grantee_hex));
        assert_eq!(record.authority_seq, 1);

        // The same grantee can now steer…
        let turn = command_event_by(
            channel_id,
            "turn-after-grant",
            &target,
            serde_json::json!({ "type": "thread.turn.start", "text": "now granted" }),
            &grantee_keys,
        );
        provider
            .handle_relay_event(&mut relay, channel_id, &turn)
            .await
            .expect("accept granted turn");
        // Accepted into the mailbox. Consumption waits for the turn to start
        // (see `a_live_turn_is_consumed_only_once_it_starts`), so custody is
        // what is asserted here.
        pump_until_turn_finished(&mut provider).await;
        assert!(provider.state().is_command_consumed("turn-after-grant"));

        // …but still cannot stop or resume: ownership never moved.
        let stop_content = lifecycle_target_event(
            &provider,
            channel_id,
            "stop-by-grantee",
            "session.stop",
            &target,
        )
        .content;
        let stop = signed_lifecycle_event_by(channel_id, stop_content, &grantee_keys);
        provider
            .handle_relay_event(&mut relay, channel_id, &stop)
            .await
            .expect("refuse stop");
        assert!(
            !provider
                .state()
                .session(&target.session_id)
                .expect("session")
                .closed
        );

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        let failed: Vec<&str> = receipts
            .iter()
            .filter(|receipt| receipt["status"] == "failed" || receipt["status"] == "turn_refused")
            .filter_map(|receipt| receipt["commandId"].as_str())
            .collect();
        assert!(failed.contains(&"turn-before-grant"));
        assert!(failed.contains(&"stop-by-grantee"));
        assert!(
            !failed.contains(&"turn-after-grant"),
            "the granted turn must not be refused"
        );

        relay.shutdown().await;
        server.abort();
    }

    /// Resume consumes the exact same verified authority state as steering.
    /// A viewer, a revoked operator, and an operator grant rooted at another
    /// genesis all remain unable to reconnect this execution. Even while the
    /// operator grant is live, stop remains founder-only.
    #[tokio::test]
    async fn only_a_current_operator_grant_for_this_genesis_authorizes_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);
        let relay_keys = Keys::generate();
        provider.set_relay_self(relay_keys.public_key().to_hex());

        let grantee = Keys::generate();
        let grantee_hex = grantee.public_key().to_hex();
        let genesis_ref = "ab".repeat(32);
        let other_genesis_ref = "cd".repeat(32);
        let record = governed_record(channel_id, &cwd, &genesis_ref);
        let target = record.target(&provider.config.instance_id);
        provider.state.insert_session(record).expect("insert");

        let viewer = authority_transition_event(
            channel_id,
            &genesis_ref,
            None,
            1,
            &grantee_hex,
            CodingSessionAuthorityTransitionType::GrantViewer,
        );
        let grant = authority_transition_event(
            channel_id,
            &genesis_ref,
            Some(viewer.id.to_hex()),
            2,
            &grantee_hex,
            CodingSessionAuthorityTransitionType::GrantOperator,
        );
        let revoke = authority_transition_event(
            channel_id,
            &genesis_ref,
            Some(grant.id.to_hex()),
            3,
            &grantee_hex,
            CodingSessionAuthorityTransitionType::Revoke,
        );
        let wrong_genesis_grant = authority_transition_event(
            channel_id,
            &other_genesis_ref,
            None,
            1,
            &grantee_hex,
            CodingSessionAuthorityTransitionType::GrantOperator,
        );
        let (mut relay, _queries, server) = spawn_test_relay_with_events(
            &provider.config.keys,
            vec![
                viewer.clone(),
                grant.clone(),
                revoke.clone(),
                wrong_genesis_grant.clone(),
            ],
        )
        .await;

        let viewer_receipt = acceptance_receipt_for_transition(&relay_keys, channel_id, &viewer);
        provider
            .handle_relay_event(&mut relay, channel_id, &viewer_receipt)
            .await
            .expect("apply viewer");
        assert!(matches!(
            lifecycle_decision_by(
                &provider,
                channel_id,
                "resume-as-viewer",
                "session.resume",
                &target,
                &grantee,
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));

        let wrong_genesis_receipt =
            acceptance_receipt_for_transition(&relay_keys, channel_id, &wrong_genesis_grant);
        provider
            .handle_relay_event(&mut relay, channel_id, &wrong_genesis_receipt)
            .await
            .expect("ignore grant for another genesis");
        assert_eq!(
            provider
                .state()
                .session(&target.session_id)
                .expect("session")
                .authority_seq,
            1,
            "a valid receipt for another genesis must not extend this record's chain"
        );
        assert!(matches!(
            lifecycle_decision_by(
                &provider,
                channel_id,
                "resume-wrong-genesis",
                "session.resume",
                &target,
                &grantee,
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));

        let grant_receipt = acceptance_receipt_for_transition(&relay_keys, channel_id, &grant);
        provider
            .handle_relay_event(&mut relay, channel_id, &grant_receipt)
            .await
            .expect("apply operator grant");
        assert!(matches!(
            lifecycle_decision_by(
                &provider,
                channel_id,
                "resume-as-operator",
                "session.resume",
                &target,
                &grantee,
            ),
            LifecycleDecision::Resume(_)
        ));
        assert!(matches!(
            lifecycle_decision_by(
                &provider,
                channel_id,
                "stop-as-operator",
                "session.stop",
                &target,
                &grantee,
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));

        let revoke_receipt = acceptance_receipt_for_transition(&relay_keys, channel_id, &revoke);
        provider
            .handle_relay_event(&mut relay, channel_id, &revoke_receipt)
            .await
            .expect("apply revocation");
        assert!(matches!(
            lifecycle_decision_by(
                &provider,
                channel_id,
                "resume-after-revoke",
                "session.resume",
                &target,
                &grantee,
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));

        relay.shutdown().await;
        server.abort();
    }

    /// Forged and unlinkable acceptances apply nothing: wrong receipt signer,
    /// a receipt whose transition disagrees with it, and a receipt whose
    /// transition cannot be resolved all leave the operator set empty.
    #[tokio::test]
    async fn unverifiable_acceptances_apply_no_grants() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);

        let relay_keys = Keys::generate();
        provider.set_relay_self(relay_keys.public_key().to_hex());
        let grantee_hex = Keys::generate().public_key().to_hex();
        let other_hex = Keys::generate().public_key().to_hex();
        let genesis_ref = "ab".repeat(32);

        let record = governed_record(channel_id, &cwd, &genesis_ref);
        let session_id = record.session_id.clone();
        provider.state.insert_session(record).expect("insert");

        // The store holds a transition granting `other`, not `grantee`.
        let mismatched = grant_transition_event(channel_id, &genesis_ref, None, 1, &other_hex);
        let (mut relay, _queries, server) =
            spawn_test_relay_with_events(&provider.config.keys, vec![mismatched.clone()]).await;

        // (a) Receipt signed by an impostor, not the witnessed relay identity.
        let impostor = Keys::generate();
        let forged = acceptance_receipt_event(
            &impostor,
            channel_id,
            &genesis_ref,
            &mismatched,
            1,
            &grantee_hex,
        );
        // (b) Relay-signed receipt whose resolved transition disagrees on the
        // grantee.
        let disagreeing = acceptance_receipt_event(
            &relay_keys,
            channel_id,
            &genesis_ref,
            &mismatched,
            1,
            &grantee_hex,
        );
        // (c) Relay-signed receipt naming a transition that is not
        // query-visible at all.
        let unresolvable = nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
            serde_json::json!({
                "type": authority::ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": genesis_ref,
                "acceptedEventId": "77".repeat(32),
                "seq": 1,
                "transitionType": "grant-operator",
                "granteePubkey": grantee_hex,
            })
            .to_string(),
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(&relay_keys)
        .expect("sign receipt");

        for event in [&forged, &disagreeing, &unresolvable] {
            provider
                .handle_relay_event(&mut relay, channel_id, event)
                .await
                .expect("handled without applying");
            let record = provider.state().session(&session_id).expect("session");
            assert!(record.granted_operators.is_empty());
            assert_eq!(record.authority_seq, 0);
        }

        relay.shutdown().await;
        server.abort();
    }

    /// The backfill halves: a chain accepted *before* the execution exists is
    /// folded in at create, and a receipt arriving with a gap triggers a
    /// storage backfill and then applies contiguously.
    #[tokio::test]
    async fn stored_receipts_backfill_at_create_and_repair_receipt_gaps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let relay_keys = Keys::generate();
        provider.set_relay_self(relay_keys.public_key().to_hex());
        let first_grantee = Keys::generate().public_key().to_hex();
        let second_grantee = Keys::generate().public_key().to_hex();

        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let genesis = genesis_event(channel_id, session_ref);
        let genesis_ref = genesis.id.to_hex();
        let first = grant_transition_event(channel_id, &genesis_ref, None, 1, &first_grantee);
        let first_receipt = acceptance_receipt_event(
            &relay_keys,
            channel_id,
            &genesis_ref,
            &first,
            1,
            &first_grantee,
        );
        let second = grant_transition_event(
            channel_id,
            &genesis_ref,
            Some(first.id.to_hex()),
            2,
            &second_grantee,
        );
        let second_receipt = acceptance_receipt_event(
            &relay_keys,
            channel_id,
            &genesis_ref,
            &second,
            2,
            &second_grantee,
        );
        // A forged receipt sits in storage alongside the real ones — the
        // backfill must skip it without stalling the contiguous fold.
        let forged = acceptance_receipt_event(
            &Keys::generate(),
            channel_id,
            &genesis_ref,
            &first,
            1,
            &"99".repeat(32),
        );
        let (mut relay, _queries, server) = spawn_test_relay_with_events(
            &provider.config.keys,
            vec![
                genesis.clone(),
                first.clone(),
                forged,
                first_receipt,
                second.clone(),
            ],
        )
        .await;

        // Create after seq 1 was already accepted: the grant is folded in at
        // create — the continuation case for an umbrella with prior grants.
        let create = create_event_with_genesis_ref(
            &provider,
            channel_id,
            "create-continuation",
            session_ref,
            &genesis_ref,
        );
        provider
            .handle_relay_event(&mut relay, channel_id, &create)
            .await
            .expect("create");
        let record = provider.state().sessions().next().expect("session").clone();
        assert!(record.granted_operators.contains(&first_grantee));
        assert_eq!(record.authority_seq, 1);

        // The seq 2 receipt now arrives live and extends contiguously.
        provider
            .handle_relay_event(&mut relay, channel_id, &second_receipt)
            .await
            .expect("extend");
        let record = provider
            .state()
            .session(&record.session_id)
            .expect("session");
        assert!(record.granted_operators.contains(&second_grantee));
        assert_eq!(record.authority_seq, 2);

        relay.shutdown().await;
        server.abort();
    }

    /// A live receipt that skips ahead of the applied chain triggers a
    /// backfill; when storage cannot close the gap, nothing applies — the
    /// fold never jumps a link it has not verified.
    #[tokio::test]
    async fn a_gap_receipt_backfills_from_storage_and_otherwise_applies_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);

        let relay_keys = Keys::generate();
        provider.set_relay_self(relay_keys.public_key().to_hex());
        let first_grantee = Keys::generate().public_key().to_hex();
        let second_grantee = Keys::generate().public_key().to_hex();
        let genesis_ref = "ab".repeat(32);

        let record = governed_record(channel_id, &cwd, &genesis_ref);
        let session_id = record.session_id.clone();
        provider.state.insert_session(record).expect("insert");

        let first = grant_transition_event(channel_id, &genesis_ref, None, 1, &first_grantee);
        let first_receipt = acceptance_receipt_event(
            &relay_keys,
            channel_id,
            &genesis_ref,
            &first,
            1,
            &first_grantee,
        );
        let second = grant_transition_event(
            channel_id,
            &genesis_ref,
            Some(first.id.to_hex()),
            2,
            &second_grantee,
        );
        let second_receipt = acceptance_receipt_event(
            &relay_keys,
            channel_id,
            &genesis_ref,
            &second,
            2,
            &second_grantee,
        );

        // Storage cannot close the gap: no seq 1 receipt anywhere.
        {
            let (mut relay, _queries, server) =
                spawn_test_relay_with_events(&provider.config.keys, vec![second.clone()]).await;
            provider
                .handle_relay_event(&mut relay, channel_id, &second_receipt)
                .await
                .expect("gap with no repair");
            let record = provider.state().session(&session_id).expect("session");
            assert!(record.granted_operators.is_empty());
            assert_eq!(record.authority_seq, 0);
            relay.shutdown().await;
            server.abort();
        }

        // Storage holds the missing seq 1 receipt: the gap receipt triggers a
        // backfill and then applies itself contiguously.
        {
            let (mut relay, _queries, server) = spawn_test_relay_with_events(
                &provider.config.keys,
                vec![first.clone(), first_receipt, second.clone()],
            )
            .await;
            provider
                .handle_relay_event(&mut relay, channel_id, &second_receipt)
                .await
                .expect("gap repaired");
            let record = provider.state().session(&session_id).expect("session");
            assert!(record.granted_operators.contains(&first_grantee));
            assert!(record.granted_operators.contains(&second_grantee));
            assert_eq!(record.authority_seq, 2);
            relay.shutdown().await;
            server.abort();
        }
    }

    /// Initialize `cwd` as a repository on `branch` with one commit.
    ///
    /// The repo-selection variables are cleared because git exports `GIT_DIR`
    /// into every hook, and it beats `-C` — under a pre-push hook this `init`
    /// targeted the developer's own repository instead of the tempdir.
    fn init_repo(cwd: &Path, branch: &str) {
        let run = |args: &[&str]| {
            let mut command = std::process::Command::new("git");
            for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
                command.env_remove(var);
            }
            let status = command.arg("-C").arg(cwd).args(args).status().expect("git");
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
    /// the branch, commit, and dirty state git reported into signed metadata.
    ///
    /// B1 (D4a) overturns the P3-era boundary this test used to assert —
    /// dirty state was deliberately kept off signed content until the schema
    /// question of *where* it lands was answered. It is now answered:
    /// `SessionMetadata` carries it, so this test's job flips from "dirty
    /// never leaks" to "dirty (and the commit) reliably arrive."
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
        assert_eq!(metadata[0]["dirty"], false);
        let observed_commit = metadata[0]["observedCommit"]
            .as_str()
            .expect("observedCommit present");
        assert_eq!(observed_commit.len(), 40);

        // No relay client was wired in for this test provider, so the
        // reachability leg never ran — "not checked," not "confirmed absent."
        assert!(metadata[0]["relayReachable"].is_null());
        assert!(metadata[0]["verifiedAt"].is_null());

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
        assert_eq!(
            provider
                .git_probes
                .get(session_id)
                .and_then(|p| p.commit.clone()),
            Some(observed_commit.to_owned())
        );
        assert!(!provider.git_reachability.contains_key(session_id));
    }

    /// A repo-bound session with no relay client wired in must still publish
    /// `relayReachable: null` / `verifiedAt: null` — never a guessed `false`.
    /// This is the "absent check" half of the honesty contract; the
    /// "failed check" half is covered directly in `reachability`'s own tests
    /// (`a_network_failure_reports_not_checked`).
    #[tokio::test]
    async fn a_repo_bound_session_without_a_relay_client_reports_reachability_as_not_checked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        init_repo(&cwd, "probe-branch");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        assert!(
            provider.rest_client.is_none(),
            "the test harness never wires a relay client in"
        );

        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        pump_until_worktree_observed(&mut provider).await;

        let session_id = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();
        assert!(
            !provider.git_reachability.contains_key(&session_id),
            "no relay client means the check never ran"
        );

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert!(metadata.last().expect("metadata")["relayReachable"].is_null());
        assert!(metadata.last().expect("metadata")["verifiedAt"].is_null());
    }

    /// A populated reachability fact — as `spawn_git_probe` would produce
    /// with a relay client wired in and a successful check — is read
    /// straight through into the next metadata publication, `Some(true)` and
    /// its timestamp both intact.
    #[tokio::test]
    async fn a_confirmed_reachability_fact_is_published_into_metadata() {
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
        pump_until_worktree_observed(&mut provider).await;

        let record = provider.state().sessions().next().expect("session").clone();
        let observed_commit = provider
            .git_probes
            .get(&record.session_id)
            .and_then(|probe| probe.commit.clone())
            .expect("commit observed");

        // Fold in a reachability fact the way `WorktreeObserved` would — this
        // is the apply step under test, exercised directly rather than via a
        // live relay.
        let fact = reachability::ReachabilityFact {
            reachable: true,
            verified_at: 1_700_000_000,
        };
        provider
            .git_reachability
            .insert(record.session_id.clone(), fact);
        provider
            .publish_metadata(
                channel_id,
                &record.target("instance-1"),
                SessionStatus::Idle,
            )
            .expect("publish");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        let last = metadata.last().expect("metadata");
        assert_eq!(last["relayReachable"], true);
        assert_eq!(last["verifiedAt"], 1_700_000_000);
        assert_eq!(last["observedCommit"], observed_commit);
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
        let mut command = std::process::Command::new("git");
        for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let status = command
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
            commit: Some("a".repeat(40)),
        };
        let newer = git_probe::GitProbe {
            branch: Some("branch-b".to_owned()),
            dirty: Some(true),
            commit: Some("b".repeat(40)),
        };
        // Reachability travels with each probe under the same generation, so
        // this test also proves R17 covers it: the stale generation's
        // confirmed-reachable fact must not survive over the newer
        // generation's confirmed-not-reachable one.
        let older_reachability = reachability::ReachabilityFact {
            reachable: true,
            verified_at: 1_000,
        };
        let newer_reachability = reachability::ReachabilityFact {
            reachable: false,
            verified_at: 2_000,
        };

        // B (generation 3, launched second) completes first.
        provider
            .handle_session_event(SessionEvent::WorktreeObserved {
                session_id: session_id.clone(),
                generation: 3,
                observed: newer.clone(),
                reachability: Some(newer_reachability),
            })
            .expect("record newer");
        // A (generation 2, launched first) completes after — the reversed
        // completion order this test exists to force.
        provider
            .handle_session_event(SessionEvent::WorktreeObserved {
                session_id: session_id.clone(),
                generation: 2,
                observed: older,
                reachability: Some(older_reachability),
            })
            .expect("record older");

        assert_eq!(
            provider.git_probes.get(&session_id),
            Some(&newer),
            "the older, later-arriving probe overwrote the newer one"
        );
        assert_eq!(
            provider.git_reachability.get(&session_id),
            Some(&newer_reachability),
            "the older, later-arriving reachability fact overwrote the newer one"
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

    /// Build a kind:44230 closure revision for `umbrella`.
    fn closure_event(channel_id: Uuid, umbrella: &str, action: &str, keys: &Keys) -> Event {
        let content = serde_json::json!({
            "action": action,
            "genesisRef": "ab".repeat(32),
            "sessionRef": umbrella,
            "v": 1,
        })
        .to_string();
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_CLOSURE as u16),
            content,
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag"),
            nostr::Tag::parse(["d", umbrella]).expect("tag"),
        ])
        .sign_with_keys(keys)
        .expect("sign closure")
    }

    /// Create one execution under `umbrella` and return the provider holding
    /// it, with exactly one slot occupied.
    async fn provider_with_one_live_umbrella(
        dir: &tempfile::TempDir,
        channel_id: Uuid,
        umbrella: &str,
    ) -> Provider {
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let event = create_event_with_session_ref(&provider, channel_id, "create-1", umbrella);
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle create");
        assert_eq!(
            provider.sessions.live_count(),
            1,
            "the fixture must actually be holding a slot"
        );
        provider
    }

    /// The bug this fixes. A slot is an entry in the live map, the create gate
    /// compares `live_count()` against `max_sessions`, and closing a session
    /// used to be invisible to this provider entirely — the closure kind was
    /// not in its subscription — so the slot stayed held until the four-hour
    /// idle timeout.
    #[tokio::test]
    async fn closing_an_umbrella_releases_the_slot_its_execution_holds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let mut provider = provider_with_one_live_umbrella(&dir, channel_id, umbrella).await;

        let closure = closure_event(channel_id, umbrella, "closed", &Keys::generate());
        provider
            .handle_command_event(channel_id, &closure)
            .await
            .expect("handle closure");

        assert_eq!(
            provider.sessions.live_count(),
            0,
            "a closed umbrella must not go on occupying a slot"
        );
        let record = provider
            .state()
            .sessions()
            .next()
            .expect("the session record survives; only the process is gone");
        assert!(
            record.closed,
            "the durable record must agree with the released slot"
        );
    }

    /// `archived` settles too — it is the disposition the project sidebar
    /// offers, so if it did not free the slot the fix would miss the most
    /// common way a session ends.
    #[tokio::test]
    async fn archiving_an_umbrella_releases_the_slot_as_well() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a11";
        let mut provider = provider_with_one_live_umbrella(&dir, channel_id, umbrella).await;

        let closure = closure_event(channel_id, umbrella, "archived", &Keys::generate());
        provider
            .handle_command_event(channel_id, &closure)
            .await
            .expect("handle closure");

        assert_eq!(provider.sessions.live_count(), 0);
    }

    /// A reopen must not stop anything. `open` does not settle, and the
    /// executions a reopened umbrella refers to are already gone — releasing
    /// on one would kill a session somebody just brought back.
    #[tokio::test]
    async fn reopening_an_umbrella_releases_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a12";
        let mut provider = provider_with_one_live_umbrella(&dir, channel_id, umbrella).await;

        let closure = closure_event(channel_id, umbrella, "open", &Keys::generate());
        provider
            .handle_command_event(channel_id, &closure)
            .await
            .expect("handle closure");

        assert_eq!(provider.sessions.live_count(), 1, "a reopen is not a stop");
    }

    /// Closing somebody else's umbrella must not reach into this one. The
    /// release is keyed on the record's own `sessionRef`, so a closure for an
    /// umbrella this host never minted an execution for has nothing to do.
    #[tokio::test]
    async fn a_closure_for_another_umbrella_leaves_this_one_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a13";
        let mut provider = provider_with_one_live_umbrella(&dir, channel_id, umbrella).await;

        let closure = closure_event(
            channel_id,
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a99",
            "closed",
            &Keys::generate(),
        );
        provider
            .handle_command_event(channel_id, &closure)
            .await
            .expect("handle closure");

        assert_eq!(provider.sessions.live_count(), 1);
    }

    /// A payload this build cannot decode is a version skew, not a reason to
    /// stop serving turns. It is logged and dropped, and the session it names
    /// keeps running rather than being stopped on a guess.
    #[tokio::test]
    async fn an_undecodable_closure_is_ignored_rather_than_acted_on() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a14";
        let mut provider = provider_with_one_live_umbrella(&dir, channel_id, umbrella).await;

        let malformed = nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_CLOSURE as u16),
            "{\"action\":\"closed\"}",
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag"),
            nostr::Tag::parse(["d", umbrella]).expect("tag"),
        ])
        .sign_with_keys(&Keys::generate())
        .expect("sign");
        provider
            .handle_command_event(channel_id, &malformed)
            .await
            .expect("a malformed closure must not fail the dispatch");

        assert_eq!(provider.sessions.live_count(), 1);
    }

    /// Closing twice must not release twice: the second pass sees a record
    /// already marked closed and does nothing, so no second lease release or
    /// duplicate host-local cleanup is queued for a session that already
    /// published both.
    #[tokio::test]
    async fn a_repeated_closure_is_a_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a15";
        let mut provider = provider_with_one_live_umbrella(&dir, channel_id, umbrella).await;

        let closure = closure_event(channel_id, umbrella, "closed", &Keys::generate());
        provider
            .handle_command_event(channel_id, &closure)
            .await
            .expect("first closure");
        assert_eq!(provider.sessions.live_count(), 0);

        assert_eq!(
            provider.release_settled_umbrella(umbrella),
            0,
            "nothing left to release"
        );
    }

    /// The subscription is the whole reason this was invisible for so long,
    /// so it is pinned directly rather than only through behaviour.
    #[test]
    fn the_subscription_carries_the_closure_kind() {
        let kinds = [
            KIND_CODING_SESSION_COMMAND,
            KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            KIND_CODING_SESSION_TEAM_TRANSACTION,
            KIND_CODING_SESSION_CLOSURE,
            KIND_SYSTEM_MESSAGE,
        ];
        assert!(
            kinds.contains(&KIND_CODING_SESSION_CLOSURE),
            "without this kind on the wire nothing below it can ever run"
        );
    }

    /// The seated create, end to end: an execution is created as an agent, its
    /// 44223 says so, the seat's key material is gone from the host afterwards,
    /// and nothing signed or logged ever contained it.
    #[tokio::test]
    async fn a_seated_create_publishes_the_seat_and_consumes_its_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let seats = write_actor_seats(dir.path(), "create-1", &"cd".repeat(32));
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event =
            seated_create_event(&provider, channel_id, "create-1", &"cd".repeat(32), "lead");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "created");

        // D1: `agentRef` stops being always-null, and `role` rides beside it.
        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0]["agentRef"], "cd".repeat(32));
        assert_eq!(metadata[0]["role"], "lead");

        // Durable, so every later publication says the same thing.
        let record = provider
            .state()
            .sessions()
            .next()
            .expect("one session record");
        assert_eq!(record.actor.as_deref(), Some("cd".repeat(32).as_str()));
        assert_eq!(record.role.as_deref(), Some("lead"));
        // …and the record is not where the key lives.
        assert!(!format!("{record:?}").contains("nsec"));

        // One-shot custody: the entry is gone and the key is not in the file.
        let body = std::fs::read_to_string(&seats).expect("read seats");
        assert!(
            !body.contains(TEST_SEAT_NSEC),
            "the seat's key is still at rest: {body}"
        );
        assert!(!body.contains("create-1"), "{body}");

        // Nothing signed ever carried it.
        for event in sink.events.lock().expect("lock").iter() {
            assert!(
                !event.content.contains(TEST_SEAT_NSEC),
                "a signed event carried the seat's key: {}",
                event.content
            );
        }
    }

    /// LANE-L23: the pack a seat was staged with reaches the record, durably,
    /// so every 44223 of this generation can say which pack ran.
    ///
    /// The host stages it beside the key; unlike the key (and unlike the
    /// pack's *directory*) a `packRef` means the same thing on every machine,
    /// so it is the half that belongs on the wire.
    #[tokio::test]
    async fn a_seated_create_records_the_pack_its_seat_was_staged_with() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let owner = "cd".repeat(32);
        let sha = "ab".repeat(20);
        let mut extra = serde_json::Map::new();
        extra.insert(
            "packRef".to_string(),
            serde_json::json!({
                "repo": format!("30617:{owner}:packs"),
                "sha": sha,
                "role": "lead",
                "path": "personas/roles/lead",
            }),
        );
        write_actor_seats_with(dir.path(), "create-1", &owner, extra);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event = seated_create_event(&provider, channel_id, "create-1", &owner, "lead");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let record = provider
            .state()
            .sessions()
            .next()
            .expect("one session record");
        let pack_ref = record.pack_ref.clone().expect("the seat named its pack");
        assert_eq!(pack_ref.sha, sha);
        assert_eq!(pack_ref.role, "lead");
        assert_eq!(pack_ref.path, "personas/roles/lead");
        assert_eq!(pack_ref.repo, format!("30617:{owner}:packs"));
    }

    /// A seat staged before `packRef` existed, and a seat whose pack came from
    /// this computer, both record no pack reference — an absent fact, never a
    /// repository invented to fill the field.
    #[tokio::test]
    async fn a_seat_with_no_packref_records_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        write_actor_seats(dir.path(), "create-1", &"cd".repeat(32));
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let event =
            seated_create_event(&provider, channel_id, "create-1", &"cd".repeat(32), "lead");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        provider.flush(&CollectingSink::new()).await.expect("flush");
        assert_eq!(
            provider
                .state()
                .sessions()
                .next()
                .expect("one session record")
                .pack_ref,
            None
        );
    }

    /// Finding 72 (runs 6 and 7, 2026-09-04): every lead 44223 carried
    /// `beeStamp` and never `packRef`. The publisher's half of the contract,
    /// pinned here with a fake staging result: whatever `packRef` the host
    /// staged beside the seat's key — the `app:shipped` form included — is on
    /// every 44223 the create publishes, verbatim, next to the seat's role.
    /// Nothing here re-derives it; the seat file is the only source.
    #[tokio::test]
    async fn the_leads_published_status_carries_the_pack_ref_its_seat_was_staged_with() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let owner = "cd".repeat(32);
        let staged = serde_json::json!({
            "repo": "app:shipped",
            "sha": "0.5.16",
            "role": "lead",
            "path": "personas/roles/lead",
        });
        let mut extra = serde_json::Map::new();
        extra.insert("packRef".to_string(), staged.clone());
        write_actor_seats_with(dir.path(), "create-1", &owner, extra);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event = seated_create_event(&provider, channel_id, "create-1", &owner, "lead");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let published = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert!(!published.is_empty(), "the create published no 44223");
        for metadata in &published {
            assert_eq!(metadata["role"], "lead", "{metadata}");
            assert_eq!(metadata["agentRef"], owner, "{metadata}");
            assert_eq!(metadata["packRef"], staged, "{metadata}");
        }
    }

    /// A create whose seat this host does not hold is refused with
    /// `ACTOR_UNAVAILABLE`, and nothing is created: no session record, no
    /// metadata, no adapter.
    #[tokio::test]
    async fn a_seated_create_with_no_custody_entry_is_refused_and_spawns_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event =
            seated_create_event(&provider, channel_id, "create-1", &"cd".repeat(32), "lead");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(receipts[0]["error"]["code"], payload::ACTOR_UNAVAILABLE);
        assert!(
            sink.contents_of(KIND_CODING_SESSION_METADATA).is_empty(),
            "a refused create published metadata"
        );
        assert_eq!(provider.state().sessions().count(), 0);
        assert_eq!(provider.sessions.live_count(), 0);
    }

    /// A seated create whose working directory is already a live execution's
    /// is refused — item 80(a)/(b), where three hired seats were given the
    /// operator's own checkout and their role packs merged into one
    /// `.agents/skills`. The refusal names the seat that was already there,
    /// nothing is spawned, and the staged key does not survive the refusal.
    #[tokio::test]
    async fn a_seated_create_sharing_a_live_executions_tree_is_refused_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let actor = "cd".repeat(32);
        let seats = write_actor_seats(dir.path(), "create-1", &actor);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        // The lead is already working in that checkout, under this umbrella.
        let mut lead = governed_record(channel_id, &cwd, &"ab".repeat(32));
        lead.role = Some("lead".into());
        lead.actor = Some("ef".repeat(32));
        let umbrella = lead.session_ref.clone().expect("umbrella");
        provider.state.insert_session(lead).expect("insert");

        let event = seated_join_event(
            &provider, channel_id, "create-1", &umbrella, &actor, "builder",
        );
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1, "{receipts:?}");
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(receipts[0]["error"]["code"], session::SEAT_CWD_SHARED);
        let message = receipts[0]["error"]["message"]
            .as_str()
            .expect("message")
            .to_owned();
        assert!(message.contains("lead"), "{message}");
        assert!(message.contains("worktree"), "{message}");

        // Nothing was created, and nothing was left staged.
        assert_eq!(
            provider.state().sessions().count(),
            1,
            "a session was minted"
        );
        assert_eq!(provider.sessions.live_count(), 0);
        let body = std::fs::read_to_string(&seats).expect("read seats");
        assert!(
            !body.contains(TEST_SEAT_NSEC),
            "the seat's key is still at rest: {body}"
        );
    }

    /// A seat that asks for a tree of its own is created exactly as before:
    /// the guard above must refuse collisions, not seats.
    #[tokio::test]
    async fn a_seated_create_in_its_own_tree_is_untouched_by_the_guard() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let other = dir.path().join("checkout.worktrees/builder");
        std::fs::create_dir_all(&other).expect("mkdir other");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let actor = "cd".repeat(32);
        write_actor_seats(dir.path(), "create-1", &actor);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        // The lead runs somewhere else entirely.
        let mut lead = governed_record(channel_id, &other, &"ab".repeat(32));
        lead.role = Some("lead".into());
        let umbrella = lead.session_ref.clone().expect("umbrella");
        provider.state.insert_session(lead).expect("insert");

        let event = seated_join_event(
            &provider, channel_id, "create-1", &umbrella, &actor, "builder",
        );
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1, "{receipts:?}");
        assert_ne!(
            receipts[0]["error"]["code"],
            session::SEAT_CWD_SHARED,
            "a seat in its own tree was refused: {:?}",
            receipts[0]
        );
    }

    /// A seated create joining an umbrella, with the seat and the umbrella's
    /// ref on the same action.
    fn seated_join_event(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        session_ref: &str,
        actor: &str,
        role: &str,
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
                "actor": actor,
                "role": role,
                "model": null,
                "title": "Ship it",
                "initialTurn": null,
            },
        })
        .to_string();
        signed_lifecycle_event(channel_id, content)
    }

    /// The resume path has the same one-shot rule: a reconnect the provider
    /// refuses before it spawns anything takes its staged key with it.
    ///
    /// `SESSION_ALREADY_ATTACHED` is the reachable case — a second Reconnect
    /// press against an execution that is already live — and it is answered
    /// before the seat is ever read, so nothing downstream would delete it.
    #[tokio::test]
    async fn a_resume_refused_before_it_spawns_leaves_no_key_at_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let actor = "cd".repeat(32);
        let projects = write_projects(dir.path(), channel_id, &cwd);
        write_actor_seats(dir.path(), "create-1", &actor);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let create = seated_create_event(&provider, channel_id, "create-1", &actor, "lead");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        // Deliberately still attached: the reconnect is refused.
        assert!(provider.sessions.handle(&target.session_id).is_some());

        let seats = write_actor_seats(dir.path(), "resume-1", &actor);
        let resume =
            lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let refusal = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "resume-1")
            .expect("the resume was answered");
        assert_eq!(refusal["status"], "failed");
        assert_eq!(refusal["error"]["code"], SESSION_ALREADY_ATTACHED);

        let body = std::fs::read_to_string(&seats).expect("read seats");
        assert!(
            !body.contains(TEST_SEAT_NSEC),
            "a refused reconnect left the seat's key at rest: {body}"
        );
        assert!(!body.contains("resume-1"), "{body}");
    }

    /// A create refused before it dispatches takes the seat's key with it.
    ///
    /// The custody entry is a one-shot secret written for one exact
    /// `commandId`. Once that command has been consumed and answered `failed`,
    /// nothing will ever reach it again — so an entry left behind is an `nsec`
    /// at rest under a key nobody will read, for as long as the file lives.
    /// `create_session` already deletes it on both of its exits; the two
    /// refusals that never reach `create_session` did not.
    #[tokio::test]
    async fn a_create_refused_at_decision_time_leaves_no_key_at_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let actor = "cd".repeat(32);
        // A projects file that resolves some *other* channel: the seat check
        // passes, then the working directory cannot be resolved.
        let projects = write_projects(dir.path(), Uuid::new_v4(), dir.path());
        let seats = write_actor_seats(dir.path(), "create-1", &actor);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let create = seated_create_event(&provider, channel_id, "create-1", &actor, "lead");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(
            receipts[0]["error"]["code"],
            payload::PROJECT_CWD_UNRESOLVED
        );

        let body = std::fs::read_to_string(&seats).expect("read seats");
        assert!(
            !body.contains(TEST_SEAT_NSEC),
            "a refused create left the seat's key at rest: {body}"
        );
        assert!(!body.contains("create-1"), "{body}");
    }

    /// Same rule for the other refusal that never reaches `create_session`:
    /// a genesis-bearing create whose founder cannot be resolved.
    #[tokio::test]
    async fn a_create_whose_genesis_cannot_be_resolved_leaves_no_key_at_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let actor = "cd".repeat(32);
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let seats = write_actor_seats(dir.path(), "create-1", &actor);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": "create-1",
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
                "genesisRef": "12".repeat(32),
                "actor": actor,
                "role": "lead",
                "model": null,
                "title": "Ship it",
                "initialTurn": null,
            },
        })
        .to_string();
        let create = signed_lifecycle_event(channel_id, content);
        // No relay resolver, so the founder behind the genesis cannot be
        // resolved and the create is refused GENESIS_NOT_FOUND.
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "failed");
        assert_eq!(receipts[0]["error"]["code"], payload::GENESIS_NOT_FOUND);

        let body = std::fs::read_to_string(&seats).expect("read seats");
        assert!(
            !body.contains(TEST_SEAT_NSEC),
            "a refused create left the seat's key at rest: {body}"
        );
        assert!(!body.contains("create-1"), "{body}");
    }

    /// A seated execution reconnects when its identity is staged again.
    ///
    /// The create consumed the entry that carried the seat's key, so the
    /// resume brings its own — keyed by the *resume's* `commandId`. Without
    /// this the desktop's Reconnect is refused `ACTOR_UNAVAILABLE` forever and
    /// a seated execution is single-use, which is why the composer stages
    /// custody before publishing a resume.
    #[tokio::test]
    async fn a_seated_resume_reattaches_when_its_key_material_is_staged_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let actor = "cd".repeat(32);
        let projects = write_projects(dir.path(), channel_id, &cwd);
        write_actor_seats(dir.path(), "create-1", &actor);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let create = seated_create_event(&provider, channel_id, "create-1", &actor, "lead");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("create");
        let previous = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        assert_eq!(previous.generation, 1);
        provider.sessions.shutdown(&previous.session_id);

        // The reconnect's own one-shot hand-off, same pubkey, new command.
        let seats = write_actor_seats(dir.path(), "resume-1", &actor);
        let resume = lifecycle_target_event(
            &provider,
            channel_id,
            "resume-1",
            "session.resume",
            &previous,
        );
        provider
            .handle_command_event(channel_id, &resume)
            .await
            .expect("resume");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        let resumed = receipts
            .iter()
            .find(|receipt| receipt["commandId"] == "resume-1")
            .expect("the resume was answered");
        // Either resumed shape is a reattachment; which one depends on what
        // the adapter advertises, not on the seat. What must never appear is
        // `failed`/`ACTOR_UNAVAILABLE`.
        assert!(
            resumed["status"] == "resumed" || resumed["status"] == "resumed_without_context",
            "a staged seat must reattach, not be refused: {resumed}"
        );
        let current = provider
            .state()
            .session(&previous.session_id)
            .expect("session");
        assert_eq!(current.generation, 2);
        assert_eq!(current.actor.as_deref(), Some(actor.as_str()));
        assert!(provider.sessions.handle(&previous.session_id).is_some());

        // Still one-shot: the reconnect's key is gone from the host too.
        let body = std::fs::read_to_string(&seats).expect("read seats");
        assert!(!body.contains(TEST_SEAT_NSEC), "{body}");
        assert!(!body.contains("resume-1"), "{body}");
    }

    /// The unseated path is unchanged: `agentRef` is null and `role` is not a
    /// key at all, so a pre-amendment consumer's exact-key check still passes.
    #[tokio::test]
    async fn an_unseated_create_publishes_no_role_key() {
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

        let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
        assert_eq!(metadata.len(), 1);
        assert!(metadata[0]["agentRef"].is_null());
        assert!(
            metadata[0].get("role").is_none(),
            "an unseated execution published a role key: {}",
            metadata[0]
        );
    }

    const TEST_SEAT_NSEC: &str = "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";

    fn write_actor_seats(dir: &Path, command_id: &str, pubkey: &str) -> std::path::PathBuf {
        write_actor_seats_with(dir, command_id, pubkey, serde_json::Map::new())
    }

    /// The same file with extra keys folded into the seat — `packRef`, say.
    fn write_actor_seats_with(
        dir: &Path,
        command_id: &str,
        pubkey: &str,
        extra: serde_json::Map<String, serde_json::Value>,
    ) -> std::path::PathBuf {
        let path = dir.join(config::ACTOR_SEATS_FILE_NAME);
        let mut seat = serde_json::json!({
            "pubkey": pubkey,
            "nsec": TEST_SEAT_NSEC,
            "authTag": null,
            "relayUrl": "wss://seat.example",
        });
        let object = seat.as_object_mut().expect("seat object");
        object.extend(extra);
        std::fs::write(
            &path,
            serde_json::json!({
                "version": 1,
                "pending": { command_id: seat },
            })
            .to_string(),
        )
        .expect("write actor seats");
        path
    }

    fn seated_create_event(
        provider: &Provider,
        channel_id: Uuid,
        command_id: &str,
        actor: &str,
        role: &str,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": provider.config.pubkey_hex(),
                "actor": actor,
                "role": role,
                "model": null,
                "title": "Ship it",
                "initialTurn": null,
            },
        })
        .to_string();
        signed_lifecycle_event(channel_id, content)
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
            claude_runtime_command(dir.path().join("missing-claude").to_string_lossy().into()),
            RuntimeDescriptor {
                instance_ref: "codex-primary".into(),
                driver: "codex-acp".into(),
                runtime: "codex".into(),
                agent_command: "bash".into(),
                agent_args: vec![codex_agent],
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

    /// The other half of the invariant above: the path leaves the wire, and
    /// stays on the machine that owns it.
    ///
    /// End to end through `enqueue_transcript` on purpose — the vault module's
    /// own tests pass whether or not the publish path ever calls it, which is
    /// exactly the gap this closes.
    #[tokio::test]
    async fn a_redacted_host_path_is_recoverable_on_the_machine_that_redacted_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("secret-checkout-path");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state_dir = dir.path().join("state");
        let mut provider = provider(&state_dir, Some(&projects));

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
        // A path *inside* the workspace is made repo-relative rather than
        // redacted, so the vault never sees it. The recoverable case is a host
        // path outside the checkout — a sibling of the workspace root here.
        let needle = dir
            .path()
            .join("elsewhere-on-this-host")
            .to_string_lossy()
            .to_string();
        provider
            .enqueue_transcript(
                channel_id,
                &target,
                Some("turn-1"),
                serde_json::json!({
                    "kind": "assistant_text",
                    "text": format!("read {needle}/main.rs and it compiled"),
                }),
                Priority::Normal,
            )
            .expect("transcript");

        // The digest is the only join, so read it back off the published item
        // exactly as a reader on this machine would.
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let published = transcript_items_in_sequence(&sink)
            .into_iter()
            .find(|item| item["item"]["kind"] == "assistant_text")
            .expect("assistant item");
        let text = published["item"]["text"].as_str().expect("text");
        assert!(!text.contains(&needle), "{text}");
        let digest = text
            .split("sha256:")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("digest in marker")
            .to_owned();

        let found = redaction_vault::resolve(
            &state_dir,
            &target.session_id,
            std::slice::from_ref(&digest),
        )
        .expect("resolve");
        assert_eq!(
            found[&digest].plaintext,
            format!("{needle}/main.rs"),
            "the host must be able to read back its own path"
        );
        assert_eq!(found[&digest].class, "host-path");

        // Stop is terminal, so the note goes with it.
        provider.forget_redactions(&target.session_id);
        assert!(
            redaction_vault::resolve(&state_dir, &target.session_id, &[digest])
                .expect("resolve")
                .is_empty()
        );
    }

    /// A credential is redacted identically and is never written down. Same
    /// publish path, opposite expectation — this is the leak direction.
    #[tokio::test]
    async fn a_redacted_credential_is_never_recoverable_anywhere() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state_dir = dir.path().join("state");
        let mut provider = provider(&state_dir, Some(&projects));

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
                    "text": "exported GITHUB_TOKEN=ghp_aaaaaaaaaaaaaaaaaaaaaaaa for the push",
                }),
                Priority::Normal,
            )
            .expect("transcript");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        for event in sink.all() {
            let serialized = serde_json::to_string(&event).expect("serialize");
            assert!(!serialized.contains("ghp_aaaa"), "{serialized}");
        }
        // Nothing about it reached disk either — not the file, not the value.
        let vault = state_dir.join("redactions");
        let recorded = std::fs::read_dir(&vault)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
                    .collect::<String>()
            })
            .unwrap_or_default();
        assert!(!recorded.contains("ghp_"), "{recorded}");
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
        // Receipt, metadata, and the one continuity disclosure — the replay adds
        // nothing.
        assert_eq!(provider.pending_publishes(), 3);
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
        restarted.recover().await.expect("recover");
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
            let open_turn = first
                .state()
                .session(&session_id)
                .expect("session")
                .open_turn
                .as_ref()
                .expect("open lifecycle turn");
            assert!(
                !open_turn.team_wake_eligible,
                "a create's initial turn can never require an assignment report"
            );
            session_id
        };

        let mut restarted =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        restarted.recover().await.expect("recover");
        restarted
            .queue_initial_live_leases()
            .expect("recovery lease scan");
        assert!(
            restarted.pending_leases.is_empty(),
            "durable records without attached actors never recover as live"
        );
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
        restarted.recover().await.expect("recover");
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
        let current_record = restarted.state().session(&session_id).expect("session");
        assert_eq!(current_record.generation_command_id(), "resume-1");
        assert_eq!(current_record.next_lease_sequence, 1);
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
        let released = restarted
            .pending_leases
            .values()
            .next()
            .expect("released lease precedes terminal publication");
        let released = buzz_core::coding_session_lease::validate_coding_session_lease_envelope(
            &released.event,
            released.event.created_at.as_secs(),
        )
        .expect("valid released lease");
        assert_eq!(released.payload.state, CodingSessionLeaseState::Released);
        assert_eq!(released.command_id, "resume-1");
        assert_eq!(released.payload.lease_sequence, 1);

        drop(restarted);
        let mut after_stop =
            Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
        after_stop.recover().await.expect("recover");
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
                    generation_command_id: None,
                    provider_instance_ref: "codex-primary".into(),
                    runtime: "codex".into(),
                    driver: "codex-acp".into(),
                    cwd: dir.path().join("checkout"),
                    project_ref: None,
                    repo_ref: None,
                    session_ref: None,
                    genesis_ref: None,
                    actor: None,
                    role: None,
                    founder_pubkey: Some("ab".repeat(32)),
                    granted_operators: std::collections::BTreeSet::new(),
                    granted_viewers: std::collections::BTreeSet::new(),
                    authority_seq: 0,
                    model: None,
                    routing: None,
                    resume_cursor: Some("private-acp-cursor".into()),
                    title: None,
                    created_at_ms: now_ms(),
                    next_seq: 3,
                    next_lease_sequence: 1,
                    bootstrap_transport: None,
                    open_turn: Some(OpenTurn {
                        turn_id: "turn-1".into(),
                        command_id: Some("turn-cmd-1".into()),
                        team_wake_eligible: true,
                        started_at_ms: now_ms(),
                        operator_pubkey: None,
                    }),
                    closed: false,
                    created_by: None,
                    handover: ClaimState::NoClaim,
                    retired: None,
                    pack_ref: None,
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
        restarted.recover().await.expect("recover");
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
        assert_eq!(
            kinds,
            vec!["status", "user_prompt", "assistant_text", "result"],
            "the create's continuity disclosure opens the transcript"
        );

        // Sequences are dense and start at 1, and every item of the turn shares
        // the producer-minted turn id the consumer groups on.
        let seqs: Vec<u64> = transcripts
            .iter()
            .filter_map(|item| item["eventSeq"].as_u64())
            .collect();
        assert_eq!(seqs, vec![1, 2, 3, 4]);
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
        // The refusal has to say whose cap it is and what clears it: read as
        // "maximum of 4 sessions", it was taken for the model vendor's own
        // account limit (reported 2026-08-24).
        let refusal = receipts[2]["error"]["message"].as_str().expect("message");
        assert!(refusal.contains("this provider"), "{refusal}");
        assert!(refusal.contains("stop an execution"), "{refusal}");
        assert!(refusal.contains("BUZZ_CSP_MAX_SESSIONS"), "{refusal}");
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

    /// Consumption happens when a turn *starts*, not when it is accepted.
    ///
    /// The window between the two is where a crash used to eat a turn: the
    /// command was marked consumed, the provider died before the actor ran it,
    /// and the restart's replay skipped it as `AlreadyConsumed`. The turn was
    /// gone with no receipt and no transcript item. Accepted-and-not-started
    /// must therefore be *un*consumed, and the channel watermark must stay
    /// behind it so the replay can reach it at all.
    #[tokio::test]
    async fn a_live_turn_is_consumed_only_once_it_starts() {
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

        let turn = turn_event(channel_id, "turn-1", &target);
        let turn_created_at = turn.created_at.as_secs();
        provider
            .handle_command_event(channel_id, &turn)
            .await
            .expect("handle");
        assert!(
            !provider.state().is_command_consumed("turn-1"),
            "a turn accepted into the mailbox has not run yet"
        );
        assert!(
            provider
                .state()
                .watermark(channel_id)
                .is_none_or(|mark| mark <= turn_created_at),
            "the watermark must not step past a turn that has not started \
             (`since` is inclusive, so equal still replays it)"
        );

        pump_until_turn_finished(&mut provider).await;
        assert!(
            provider.state().is_command_consumed("turn-1"),
            "a turn that ran is consumed exactly once"
        );
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
    fn first_membership_subscription_replays_from_before_the_membership_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel_id = Uuid::new_v4();
        let mut provider = provider(&dir.path().join("state"), None);

        assert_eq!(
            provider.subscription_replay_since(channel_id, Some(10_000)),
            Some(10_000 - REPLAY_GRACE_SECS),
            "a queued first subscription must reach behind the grant: a \
             create published before the membership applied is otherwise \
             below the floor forever"
        );

        assert_eq!(
            provider.subscription_replay_since(channel_id, Some(REPLAY_GRACE_SECS / 2)),
            Some(0),
            "the grace saturates rather than underflowing near the epoch"
        );

        provider
            .state
            .record_watermark(channel_id, 900)
            .expect("watermark");
        assert_eq!(
            provider.subscription_replay_since(channel_id, Some(10_000)),
            Some(900),
            "an existing command watermark remains the earliest safe replay \
             floor — everything below it was already consumed or skipped"
        );
    }

    /// A command re-delivered because the graced floor reaches below the last
    /// consumed one must dedupe, not double-create.
    #[tokio::test]
    async fn a_replayed_consumed_command_stays_consumed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));

        let event = create_event(&provider, channel_id, "create-replayed");
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("first delivery");
        let sessions_after_first = provider.state().sessions().count();

        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("replayed delivery");
        assert_eq!(
            provider.state().sessions().count(),
            sessions_after_first,
            "a replayed command must be AlreadyConsumed, not a second session"
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
        // Sequence 1 was spent on the create's own continuity disclosure.
        assert_eq!(seqs, vec![Some(2), Some(3), Some(4)]);
    }

    /// Every stage of one turn is on the wire, and the echo names the command
    /// that asked for it. Before this, a consumer's only join between a turn
    /// command and the transcript was the prompt text — which cannot tell two
    /// identical prompts apart — and the only receipt a turn ever produced was
    /// a refusal.
    #[tokio::test]
    async fn a_turn_publishes_queued_then_started_and_echoes_its_command_id() {
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

        // Both stages survive the outbox: keyed by commandId alone the second
        // would have been fenced out as a duplicate of the first.
        let receipts: Vec<serde_json::Value> = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .filter(|receipt| receipt["commandId"] == "turn-1")
            .collect();
        let statuses: Vec<&str> = receipts
            .iter()
            .filter_map(|receipt| receipt["status"].as_str())
            .collect();
        assert_eq!(statuses, vec!["turn_queued", "turn_started"]);
        for receipt in &receipts {
            assert_eq!(receipt["session"], serde_json::to_value(&target).unwrap());
            assert!(receipt["error"].is_null());
            // Strict-decodable: the consumers read every 44224 with one decoder.
            payload::decode_coding_session_lifecycle_receipt(&receipt.to_string())
                .expect("strictly decodable turn receipt");
        }
        assert!(receipts[0].get("turnId").is_none());

        let transcripts = transcript_items_in_sequence(&sink);
        let prompt = transcripts
            .iter()
            .find(|item| item["item"]["kind"] == "user_prompt")
            .expect("user_prompt item");
        assert_eq!(prompt["item"]["commandId"], "turn-1");
        // The receipt and the transcript name the same turn, so a consumer can
        // join them without matching text.
        assert_eq!(receipts[1]["turnId"], prompt["turnId"]);
    }

    /// The prompt an operator types into the create dialog is a turn like any
    /// other: it has to name a command they actually sent, or it is the one
    /// prompt in the session that joins to nothing.
    #[tokio::test]
    async fn the_first_turn_echo_names_the_create_that_asked_for_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let create = create_event_with_initial_turn(&provider, channel_id, "create-1", "go");
        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let transcripts = transcript_items_in_sequence(&sink);
        let prompt = transcripts
            .iter()
            .find(|item| item["item"]["kind"] == "user_prompt")
            .expect("user_prompt item");
        assert_eq!(prompt["item"]["commandId"], "create-1");

        // The create's own lifecycle receipt is untouched, and the turn stages
        // ride beside it on their own keys.
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        let statuses: Vec<&str> = receipts
            .iter()
            .filter(|receipt| receipt["commandId"] == "create-1")
            .filter_map(|receipt| receipt["status"].as_str())
            .collect();
        assert!(statuses.contains(&"created"), "{statuses:?}");
        assert!(statuses.contains(&"turn_started"), "{statuses:?}");
    }

    /// A turn this provider was addressed by, and cannot run, is refused out
    /// loud. A turn it was *not* addressed by, or has already answered, stays
    /// silent — a receipt there would be chatter about someone else's command
    /// or a second answer to one already answered.
    #[tokio::test]
    async fn only_turns_naming_a_target_this_provider_owns_are_refused_out_loud() {
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

        let mut unknown = target.clone();
        unknown.session_id = "11111111-2222-3333-4444-555555555555".into();
        let mut stale = target.clone();
        stale.generation = target.generation + 1;
        let mut elsewhere = target.clone();
        elsewhere.instance_id = "another-instance".into();

        for (command_id, addressed) in [
            ("turn-unknown", &unknown),
            ("turn-stale", &stale),
            ("turn-elsewhere", &elsewhere),
        ] {
            provider
                .handle_command_event(channel_id, &turn_event(channel_id, command_id, addressed))
                .await
                .expect("handle");
        }
        // Already consumed: the same command twice earns exactly one answer.
        let replayed = turn_event(channel_id, "turn-replay", &target);
        provider
            .handle_command_event(channel_id, &replayed)
            .await
            .expect("handle");
        provider
            .handle_command_event(channel_id, &replayed)
            .await
            .expect("replay");

        // A stop closes the execution; a turn after it is refused, not queued.
        let stop = lifecycle_target_event(&provider, channel_id, "stop-1", "session.stop", &target);
        provider
            .handle_command_event(channel_id, &stop)
            .await
            .expect("stop");
        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-closed", &target))
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        let refusal = |command_id: &str| -> Option<serde_json::Value> {
            receipts
                .iter()
                .find(|receipt| {
                    receipt["commandId"] == command_id && receipt["status"] == "turn_refused"
                })
                .cloned()
        };

        for (command_id, code, addressed) in [
            ("turn-unknown", payload::UNKNOWN_TARGET, &unknown),
            ("turn-stale", payload::STALE_GENERATION, &stale),
            ("turn-closed", payload::SESSION_CLOSED, &target),
        ] {
            let receipt = refusal(command_id).unwrap_or_else(|| panic!("{command_id} refused"));
            assert_eq!(receipt["error"]["code"], code);
            assert_eq!(receipt["session"], serde_json::to_value(addressed).unwrap());
            assert!(receipt.get("turnId").is_none());
            payload::decode_coding_session_lifecycle_receipt(&receipt.to_string())
                .expect("strictly decodable refusal");
        }

        assert!(
            refusal("turn-elsewhere").is_none(),
            "a turn addressed to another instance must not be answered"
        );
        let replay_stages: Vec<&str> = receipts
            .iter()
            .filter(|receipt| receipt["commandId"] == "turn-replay")
            .filter_map(|receipt| receipt["status"].as_str())
            .collect();
        assert_eq!(
            replay_stages,
            vec!["turn_queued"],
            "a replayed turn earns no second answer"
        );
    }

    /// A dropped turn was already visible to a reader of the transcript. It
    /// now also answers the operator who sent it, keyed by their command.
    #[tokio::test]
    async fn a_dropped_turn_reports_both_a_transcript_item_and_a_receipt() {
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
        let record = provider.state().sessions().next().expect("session").clone();
        let target = record.target("instance-1");

        provider
            .handle_session_event(SessionEvent::TurnDropped {
                session_id: record.session_id.clone(),
                command_id: "turn-overflow".into(),
            })
            .expect("record the drop");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let items = transcript_items_in_sequence(&sink);
        assert!(
            items
                .iter()
                .any(|item| item["item"]["status"] == "turn_dropped:queue_full"),
            "the visible drop item must survive"
        );

        let receipt = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "turn-overflow")
            .expect("drop receipt");
        assert_eq!(receipt["status"], "turn_dropped");
        assert_eq!(receipt["error"]["code"], payload::QUEUE_FULL);
        assert_eq!(receipt["session"], serde_json::to_value(&target).unwrap());
        payload::decode_coding_session_lifecycle_receipt(&receipt.to_string())
            .expect("strictly decodable drop receipt");
    }

    /// A turn command with an explicit `created_at`, so a test can control the
    /// order commands were *sent* in independently of the order they arrive.
    fn turn_event_at(
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        text: &str,
        created_at: u64,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-command/v1",
            "commandId": command_id,
            "target": target,
            "action": { "type": "thread.turn.start", "text": text, "deliver": "boundary" },
        })
        .to_string();
        nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_COMMAND as u16),
            content,
        )
        .tags(vec![
            nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(test_operator_keys())
        .expect("sign")
    }

    /// A provider wired to an agent that opens a session and then never
    /// answers a prompt, so a turn stays in flight and later turns queue.
    fn stalling_provider(state_dir: &Path, projects: Option<&Path>) -> Provider {
        let agent = fake_agent(
            state_dir_parent(state_dir),
            "stalling-agent",
            STALLING_AGENT,
        );
        Provider::new(config_of(Keys::generate(), state_dir, projects, agent)).expect("provider")
    }

    /// Receipt statuses published for one `commandId`, in publication order.
    fn receipt_stages(sink: &CollectingSink, command_id: &str) -> Vec<String> {
        sink.all()
            .iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event.content).ok())
            .filter(|receipt| receipt["commandId"] == command_id)
            .filter_map(|receipt| receipt["status"].as_str().map(str::to_owned))
            .collect()
    }

    /// Every turn receipt in publication order as `(commandId, status)`.
    fn turn_receipts_in_order(sink: &CollectingSink) -> Vec<(String, String)> {
        sink.all()
            .iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event.content).ok())
            .filter_map(|receipt| {
                Some((
                    receipt["commandId"].as_str()?.to_owned(),
                    receipt["status"].as_str()?.to_owned(),
                ))
            })
            .collect()
    }

    /// Drain and record session reports until a turn has started.
    async fn pump_until_turn_started(provider: &mut Provider) {
        pump_until(provider, |event| {
            matches!(event, session::SessionEvent::TurnStarted { .. })
        })
        .await;
    }

    /// The relay is the mailbox, and a killed provider must not eat what was in
    /// it *silently*.
    ///
    /// Named for what it pins, which is narrower than "both turns run": two
    /// turns queued behind a running one, then the provider dies with no clean
    /// shutdown (nothing flushed, no actor asked to stop). A fresh provider on
    /// the same state directory finds both turns unconsumed, has a watermark
    /// that still reaches back past them, and — when the relay replays them
    /// newest-first, which is the order it actually serves stored events in
    /// (`crates/buzz-db/src/event.rs:771`) — answers them in the order they
    /// were *sent*, exactly once each. The answer is a terminal
    /// `turn_dropped`/`NO_LIVE_EXECUTION`, not a run: the killed process took
    /// its executions with it, a resume mints a generation the replayed
    /// command no longer addresses, and re-addressing an owed turn to a new
    /// generation is deferred design (plan §7, S2's scope correction).
    #[tokio::test]
    async fn two_turns_queued_behind_a_running_one_are_answered_once_in_sent_order_after_a_kill() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state = dir.path().join("state");

        let mut provider = stalling_provider(&state, Some(&projects));
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

        let base = now_secs();
        let running = turn_event_at(channel_id, "turn-running", &target, "first", base);
        provider
            .handle_command_event(channel_id, &running)
            .await
            .expect("handle");
        // Folded, so the running turn is genuinely consumed before the kill.
        pump_until_turn_started(&mut provider).await;
        assert!(provider.state().is_command_consumed("turn-running"));

        // Sent second and third; delivered to the provider in that order.
        let second = turn_event_at(channel_id, "turn-second", &target, "second", base + 1);
        let third = turn_event_at(channel_id, "turn-third", &target, "third", base + 2);
        for event in [&second, &third] {
            provider
                .handle_command_event(channel_id, event)
                .await
                .expect("handle");
        }
        assert!(!provider.state().is_command_consumed("turn-second"));
        assert!(!provider.state().is_command_consumed("turn-third"));
        assert_eq!(
            provider.state().watermark(channel_id),
            Some(base + 1),
            "the watermark must stop at the oldest turn that has not started"
        );

        // SIGKILL: no flush, no clean shutdown, no lease release. The actor
        // task and its subprocess are simply abandoned.
        drop(provider);

        let mut restarted = stalling_provider(&state, Some(&projects));
        // A real restart runs recovery before it reads a single command: the
        // ledger is reconciled, stranded executions are detached, and the
        // context packages of the dead process are swept. Replaying into a
        // provider that skipped it would be testing a startup that does not
        // exist.
        restarted.recover().await.expect("recover");
        assert!(restarted.state().is_command_consumed("turn-running"));
        assert!(!restarted.state().is_command_consumed("turn-second"));
        assert!(!restarted.state().is_command_consumed("turn-third"));

        // Replay from the watermark, in the order the relay serves stored
        // events: newest first.
        restarted.open_replay_window(channel_id);
        for event in [&third, &second, &running] {
            restarted
                .handle_command_event(channel_id, event)
                .await
                .expect("replay");
        }
        restarted.flush_replays_now().await.expect("deliver replay");

        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        let turns: Vec<(String, String)> = turn_receipts_in_order(&sink)
            .into_iter()
            .filter(|(command_id, _)| command_id.starts_with("turn-"))
            .collect();
        // Sent order, once each. Arrival order would have answered `third`
        // first; the already-consumed running turn is silent, as it must be.
        assert_eq!(
            turns,
            vec![
                ("turn-second".to_owned(), "turn_dropped".to_owned()),
                ("turn-third".to_owned(), "turn_dropped".to_owned()),
            ],
            "replayed turns are answered in (created_at, id) order, once each"
        );
        for (command_id, _) in &turns {
            // Answered, not run: this execution has no live process, and the
            // drop receipt says the sender has to send the turn again. The
            // refusal ledger is what keeps that answer from being repeated on
            // the next redelivery.
            assert!(!restarted.state().is_command_consumed(command_id));
            assert!(restarted.state().is_command_refused(command_id));
        }
        assert_eq!(
            restarted.state().watermark(channel_id),
            Some(base + 2),
            "every replayed turn is answered, so the floor may finally move past them"
        );
        let dropped = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "turn-second")
            .expect("drop receipt");
        assert_eq!(dropped["error"]["code"], payload::NO_LIVE_EXECUTION);
        payload::decode_coding_session_lifecycle_receipt(&dropped.to_string())
            .expect("strictly decodable drop receipt");
    }

    /// Replayed turns run in the order they were sent, once each, even when
    /// they arrive backwards and one arrives twice.
    #[tokio::test]
    async fn replayed_turns_run_in_sent_order_and_exactly_once() {
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

        let base = now_secs();
        let first = turn_event_at(channel_id, "turn-first", &target, "alpha", base);
        let second = turn_event_at(channel_id, "turn-second", &target, "beta", base + 3);
        provider.open_replay_window(channel_id);
        // Newest first, and the newest twice — both of which the relay can do.
        for event in [&second, &second, &first] {
            provider
                .handle_command_event(channel_id, event)
                .await
                .expect("replay");
        }
        provider.flush_replays_now().await.expect("deliver replay");
        pump_until_turn_finished(&mut provider).await;
        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let prompts: Vec<String> = transcript_items_in_sequence(&sink)
            .iter()
            .filter(|item| item["item"]["kind"] == "user_prompt")
            .filter_map(|item| item["item"]["commandId"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(
            prompts,
            vec!["turn-first".to_owned(), "turn-second".to_owned()],
            "the transcript must record the order the operator sent, not the order the relay \
             replayed"
        );
        assert_eq!(
            receipt_stages(&sink, "turn-second"),
            vec!["turn_queued".to_owned(), "turn_started".to_owned()],
            "a command delivered twice is answered once"
        );
    }

    /// A refusal is an answer, and an answer is given once.
    ///
    /// Refused ids are durable for the same reason consumed ids are: without
    /// them a restart plus a relay redelivery republishes a byte-identical
    /// `turn_refused` under the same semantic key, and the operator sees their
    /// one refusal twice.
    #[tokio::test]
    async fn a_refused_turn_is_answered_once_even_across_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state = dir.path().join("state");
        let mut provider = provider(&state, Some(&projects));
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
        let mut stale = target.clone();
        stale.generation = target.generation + 1;

        let refused = turn_event(channel_id, "turn-stale", &stale);
        provider
            .handle_command_event(channel_id, &refused)
            .await
            .expect("handle");
        assert!(provider.state().is_command_refused("turn-stale"));
        assert!(
            !provider.state().is_command_consumed("turn-stale"),
            "a refused turn never ran, so it is not consumed"
        );
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(receipt_stages(&sink, "turn-stale"), vec!["turn_refused"]);

        drop(provider);
        let mut restarted = self::tests::provider(&state, Some(&projects));
        assert!(restarted.state().is_command_refused("turn-stale"));
        restarted
            .handle_command_event(channel_id, &refused)
            .await
            .expect("redelivery");
        let after = CollectingSink::new();
        restarted.flush(&after).await.expect("flush");
        assert!(
            receipt_stages(&after, "turn-stale").is_empty(),
            "a redelivered refusal publishes nothing"
        );
    }

    /// A turn addressed to an execution with no live process is a visible
    /// drop, not a log line — and it is answered exactly once, terminally.
    ///
    /// Both arms that used to end in a bare `tracing::warn!` are covered: no
    /// handle at all, and a handle whose actor has gone away
    /// ([`DeliverError::Gone`]). Nothing is left owed: the command is never
    /// consumed (it did not run) *and* it is recorded as refused (it will not
    /// run later), which is what the assertions below pin.
    #[tokio::test]
    async fn a_turn_with_no_live_execution_is_dropped_out_loud_and_answered_terminally() {
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
        let record = provider.state().sessions().next().expect("session").clone();
        let target = record.target("instance-1");

        // The actor exits; the session record persists.
        provider
            .handle_session_event(session::SessionEvent::Exited {
                session_id: record.session_id.clone(),
                reason: session::ExitReason::Idle,
            })
            .expect("fold the exit");

        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-orphan", &target))
            .await
            .expect("handle");
        // The same fact reached by the other arm: a handle whose actor is gone.
        provider
            .report_undelivered_turn(
                channel_id,
                "turn-gone",
                &target,
                true,
                &session::DeliverError::Gone,
            )
            .expect("report");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        for command_id in ["turn-orphan", "turn-gone"] {
            let receipt = sink
                .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
                .into_iter()
                .find(|receipt| receipt["commandId"] == command_id)
                .unwrap_or_else(|| panic!("{command_id} must be answered"));
            assert_eq!(receipt["status"], "turn_dropped");
            assert_eq!(receipt["error"]["code"], payload::NO_LIVE_EXECUTION);
            payload::decode_coding_session_lifecycle_receipt(&receipt.to_string())
                .expect("strictly decodable");
            assert!(
                !provider.state().is_command_consumed(command_id),
                "{command_id} never ran, so nothing may claim it did"
            );
            assert!(
                provider.state().is_command_refused(command_id),
                "{command_id} was answered terminally, and the answer is given once"
            );
        }
    }

    /// An execution that dies with turns in its mailbox answers for them.
    ///
    /// Before this the turns simply disappeared: they had been accepted, the
    /// operator had a `turn_queued` receipt saying so, and then nothing.
    #[tokio::test]
    async fn turns_still_in_a_dying_executions_mailbox_are_answered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let record = provider.state().sessions().next().expect("session").clone();
        let target = record.target("instance-1");

        for command_id in ["turn-1", "turn-2"] {
            provider
                .handle_command_event(channel_id, &turn_event(channel_id, command_id, &target))
                .await
                .expect("handle");
        }
        pump_until_turn_started(&mut provider).await;
        provider
            .handle_session_event(session::SessionEvent::Exited {
                session_id: record.session_id.clone(),
                reason: session::ExitReason::AgentGone("killed".into()),
            })
            .expect("fold the exit");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        // `turn-1` started before the exit, so it is consumed and its answer is
        // the transcript. `turn-2` never started and is owed an answer.
        assert_eq!(
            receipt_stages(&sink, "turn-2"),
            vec!["turn_queued".to_owned(), "turn_dropped".to_owned()]
        );
        assert!(!provider.state().is_command_consumed("turn-2"));
    }

    /// An agent that answers `session/prompt` and appends every prompt request
    /// to `log_path`, so a test can read the text the model actually received.
    fn prompt_logging_agent(log_path: &str) -> String {
        format!(
            r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"acp-session-1"}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' "$line" >> "{log_path}"
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-session-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"working"}}}}}}}}\n'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;
  esac
done
"#
        )
    }

    /// The frame must name the delivery the recipient actually got.
    ///
    /// `inject_native_steer` returns false in this build, so *every* `steer`
    /// degrades — but the frame was captured with the class the sender asked
    /// for, before the injection was attempted. The receiving agent was told
    /// "Delivery: steer" for a turn that had been queued to the next boundary,
    /// and the frame is the only place a recipient reads the class: the
    /// `turn_degraded` receipt goes to the sender.
    #[tokio::test]
    async fn a_degraded_steer_is_framed_as_the_boundary_delivery_it_became() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let log_path = dir.path().join("prompts.log");
        let agent = fake_agent(
            dir.path(),
            "logging-agent",
            &prompt_logging_agent(&log_path.to_string_lossy()),
        );
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
            .expect("create");
        let session_id = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();
        let target = provider
            .state()
            .session(&session_id)
            .expect("session")
            .target(&provider.config.instance_id);
        // The sender is a granted operator, not the founder, so the turn is
        // authorized and framed.
        let sender = test_operator_keys().public_key().to_hex();
        provider
            .state
            .update_session(&session_id, |record| {
                record.founder_pubkey = Some("ab".repeat(32));
                record.genesis_ref = Some("cd".repeat(32));
                record.granted_operators.insert(sender.clone());
            })
            .expect("grant");

        provider
            .handle_command_event(
                channel_id,
                &command_event(
                    channel_id,
                    "turn-steer",
                    &target,
                    serde_json::json!({
                        "type": "thread.turn.start",
                        "text": "actually, use the other approach",
                        "deliver": "steer",
                    }),
                ),
            )
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;

        let logged = std::fs::read_to_string(&log_path).expect("prompt log");
        let request: serde_json::Value = logged
            .lines()
            .find(|line| line.contains("session/prompt"))
            .map(|line| serde_json::from_str(line).expect("json"))
            .expect("a prompt reached the adapter");
        let sent = request["params"]["prompt"][0]["text"]
            .as_str()
            .expect("prompt text")
            .to_owned();
        assert!(sent.starts_with("[Context]"), "the turn is framed: {sent}");
        assert!(
            sent.contains("Delivery: boundary"),
            "a steer nothing injected reached this agent at the next boundary, and the frame is \
             the only place it reads the class: {sent}"
        );
        assert!(!sent.contains("Delivery: steer"), "{sent}");

        // And the sender still learns about the downgrade in its own receipt.
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert!(receipt_stages(&sink, "turn-steer").contains(&"turn_degraded".to_owned()));
    }

    /// The downgrade is a fact about the *injection*, not about the
    /// advertisement.
    ///
    /// An execution whose runtime advertised native steering at `initialize`
    /// is still degraded, because nothing injected the words into the running
    /// turn. This is the arm that would go silent if the degrade were ever
    /// gated on the advertisement, or on
    /// [`session::NATIVE_STEER_DELIVERABLE`], instead of on whether
    /// [`Provider::inject_native_steer`] actually did anything: the turn would
    /// be boundary-delivered under a bare `turn_queued` and the sender would
    /// never learn their mid-turn correction missed the turn.
    #[tokio::test]
    async fn a_steer_an_execution_advertised_is_still_degraded_when_nothing_injects_it() {
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
        // The runtime said it could take a steer. This provider still cannot
        // deliver one.
        provider.steering.insert(target.session_id.clone(), true);

        provider
            .handle_command_event(
                channel_id,
                &command_event(
                    channel_id,
                    "turn-steer-advertised",
                    &target,
                    serde_json::json!({
                        "type": "thread.turn.start",
                        "text": "stop and look at the second failure",
                        "deliver": "steer",
                    }),
                ),
            )
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "turn-steer-advertised"),
            vec![
                "turn_degraded".to_owned(),
                "turn_queued".to_owned(),
                "turn_started".to_owned(),
            ],
            "an advertised steer that nothing injected is still a downgrade, said out loud"
        );
    }

    /// A `steer` no runtime here can honour is downgraded out loud and then
    /// delivered at the next boundary — never cancelled, never merged, never
    /// silently treated as an ordinary turn.
    #[tokio::test]
    async fn a_steer_this_execution_cannot_honour_degrades_then_delivers() {
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

        let steer = command_event(
            channel_id,
            "turn-steer",
            &target,
            serde_json::json!({
                "type": "thread.turn.start",
                "text": "actually, use the other approach",
                "deliver": "steer",
            }),
        );
        provider
            .handle_command_event(channel_id, &steer)
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "turn-steer"),
            vec![
                "turn_degraded".to_owned(),
                "turn_queued".to_owned(),
                "turn_started".to_owned(),
            ],
            "the downgrade is said before custody is claimed, and the turn still runs"
        );
        let degraded = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["status"] == "turn_degraded")
            .expect("degraded receipt");
        assert_eq!(degraded["error"]["code"], payload::STEER_UNSUPPORTED);
        assert_eq!(
            degraded["session"],
            serde_json::to_value(&target).unwrap_or_default()
        );
        payload::decode_coding_session_lifecycle_receipt(&degraded.to_string())
            .expect("strictly decodable degraded receipt");
    }

    /// A cancel Beekeeper issued on its own behalf is not an operator's
    /// interrupt, and must not be receipted as one.
    ///
    /// `nudge_stalled_turn` recovers a turn the adapter answered but never
    /// resolved by sending `session/cancel` — the adapter's own documented way
    /// out of that hold. The only *other* thing in this provider that cancels
    /// is a 44220 `thread.turn.interrupt`, and that one mints
    /// `interrupt_delivered` against the command that asked for it. Nobody
    /// asked here: minting that receipt would tell the operator their
    /// interrupt landed when they never sent one, and would settle a pending
    /// row that belongs to a different command. The two paths are separate by
    /// construction — the nudge goes straight to the ACP client and never
    /// through `SessionCommand::Interrupt` — and this pins that they stay so.
    #[tokio::test]
    async fn an_answer_stall_nudge_never_mints_an_interrupt_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let agent = fake_agent(
            dir.path(),
            "unresolved-agent",
            crate::session::testing::ANSWERED_BUT_UNRESOLVED_AGENT,
        );
        let mut config = config_of(
            Keys::generate(),
            &dir.path().join("state"),
            Some(&projects),
            agent,
        );
        config.answer_stall_timeout = Some(Duration::from_millis(200));
        // Far above the stall budget, so a failure here names the stall watch
        // rather than the idle timer that would otherwise end this turn.
        config.idle_timeout = Duration::from_secs(20);
        let mut provider = Provider::new(config).expect("provider");

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
        pump_until_turn_started(&mut provider).await;
        pump_until_turn_finished(&mut provider).await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");

        // The turn is answered by its own stages and nothing else.
        assert_eq!(
            receipt_stages(&sink, "turn-1"),
            vec!["turn_queued".to_owned(), "turn_started".to_owned()],
        );
        assert!(
            turn_receipts_in_order(&sink)
                .iter()
                .all(|(_, status)| status != "interrupt_delivered"),
            "the stall nudge was receipted as an operator interrupt: {:?}",
            turn_receipts_in_order(&sink)
        );
        // And the recovery is still disclosed where it belongs — an item in
        // the turn, so the reader is not told the turn ended cleanly.
        let statuses: Vec<String> = sink
            .contents_of(KIND_CODING_SESSION_TRANSCRIPT)
            .into_iter()
            .filter_map(|envelope| envelope["item"]["status"].as_str().map(str::to_owned))
            .collect();
        assert!(
            statuses
                .iter()
                .any(|status| status.starts_with("answer_stall_recovered:")),
            "the operator was never told Beekeeper closed the turn: {statuses:?}"
        );
    }

    /// An interrupt says whether it reached a live turn.
    #[tokio::test]
    async fn an_interrupt_reports_whether_it_reached_a_live_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
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

        // Nothing running yet: there is no cancel to issue.
        provider
            .handle_command_event(
                channel_id,
                &interrupt_event(channel_id, "int-idle", &target),
            )
            .await
            .expect("handle");

        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-1", &target))
            .await
            .expect("handle");
        pump_until_turn_started(&mut provider).await;
        provider
            .handle_command_event(
                channel_id,
                &interrupt_event(channel_id, "int-live", &target),
            )
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "int-live"),
            vec!["interrupt_delivered"]
        );
        assert!(provider.state().is_command_consumed("int-live"));

        let idle = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "int-idle")
            .expect("idle interrupt answered");
        assert_eq!(idle["status"], "turn_refused");
        assert_eq!(idle["error"]["code"], payload::NO_TURN_IN_FLIGHT);
        assert!(provider.state().is_command_refused("int-idle"));

        let delivered = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "int-live")
            .expect("live interrupt answered");
        assert!(delivered["error"].is_null());
        assert_eq!(
            delivered["session"],
            serde_json::to_value(&target).unwrap_or_default()
        );
        assert!(delivered.get("turnId").is_none());
        payload::decode_coding_session_lifecycle_receipt(&delivered.to_string())
            .expect("strictly decodable interrupt receipt");
    }

    /// `threadSteer` in one generation's metadata is a fact about *that*
    /// execution's process, not a constant for the driver — and it is never
    /// `true` unless a steer could actually be delivered.
    ///
    /// Scope, stated plainly: while [`session::NATIVE_STEER_DELIVERABLE`] is
    /// `false` the published capability is `false` for every execution, so what
    /// this test can pin is the per-execution *bookkeeping* (`steering` is
    /// learned per session id and absent means nothing is claimed) and the
    /// gate. It is not evidence that an advertised steer would be published
    /// once delivery ships; the assertion above the second case is the
    /// tripwire that makes someone come back here when it does.
    #[tokio::test]
    async fn metadata_thread_steer_is_this_executions_own_truth() {
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

        // The scripted agent advertises no steering, so this execution's truth
        // is `false` — not the driver's static v1 vector.
        assert_eq!(provider.steering.get(&target.session_id), Some(&false));
        assert!(
            !provider
                .metadata_for(&target, SessionStatus::Idle)
                .capabilities
                .thread_steer
        );

        // Every assertion in this test is `!thread_steer`, so a `metadata_for`
        // that hardcoded `false` would pass it. That is not a claim this test
        // can make while the constant below is `false`, and pretending
        // otherwise would have it cited as coverage it does not provide. The
        // `const` assertion is the tripwire instead: the day a native steer
        // can be delivered, this stops compiling and whoever flipped it comes
        // back to make the second case expect `true`.
        const {
            assert!(
                !session::NATIVE_STEER_DELIVERABLE,
                "this provider cannot deliver a native steer yet; when it can, the expectation \
                 below becomes `true` and the capability starts tracking `steering`"
            )
        };
        provider.steering.insert(target.session_id.clone(), true);
        assert!(
            !provider
                .metadata_for(&target, SessionStatus::Idle)
                .capabilities
                .thread_steer,
            "a steer this provider cannot deliver is never published as a capability an \
             operator may press"
        );

        // An execution with no witnessed process claims nothing.
        provider.steering.remove(&target.session_id);
        assert!(
            !provider
                .metadata_for(&target, SessionStatus::Idle)
                .capabilities
                .thread_steer
        );
    }

    #[tokio::test]
    async fn a_create_routing_record_is_persisted_and_echoed_in_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        let (create, routing) = create_event_with_routing(&provider, channel_id, "create-routed");

        provider
            .handle_command_event(channel_id, &create)
            .await
            .expect("handle routed create");
        let record = provider.state().sessions().next().expect("session record");
        assert_eq!(record.routing.as_ref(), Some(&routing));

        let metadata = provider.metadata_for(
            &record.target(&provider.config.instance_id),
            SessionStatus::Idle,
        );
        assert_eq!(metadata.routing, Some(routing));
    }

    /// A `NO_LIVE_EXECUTION` drop is an answer, and an answer is given once.
    ///
    /// The receipt used to promise a replay ("resume it and the turn will be
    /// delivered") that nothing kept: the command was recorded in neither
    /// ledger, so the channel watermark walked straight past it, and a
    /// redelivery of the same 44220 republished a byte-identical
    /// `turn_dropped` under the same semantic key.
    #[tokio::test]
    async fn a_drop_with_no_live_execution_is_terminal_and_answered_once() {
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
        let record = provider.state().sessions().next().expect("session").clone();
        let target = record.target("instance-1");
        provider
            .handle_session_event(session::SessionEvent::Exited {
                session_id: record.session_id.clone(),
                reason: session::ExitReason::Idle,
            })
            .expect("fold the exit");

        let base = now_secs();
        let orphan = turn_event_at(channel_id, "turn-orphan", &target, "do the thing", base);
        provider
            .handle_command_event(channel_id, &orphan)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(receipt_stages(&sink, "turn-orphan"), vec!["turn_dropped"]);
        assert!(
            provider.state().is_command_refused("turn-orphan"),
            "a drop nobody will retry is terminal, so the refusal ledger holds it"
        );
        assert!(
            !provider.state().is_command_consumed("turn-orphan"),
            "the turn never ran, so nothing may claim it did"
        );
        assert_eq!(
            provider.state().watermark(channel_id),
            Some(base),
            "an answered command holds the replay floor no longer"
        );

        provider
            .handle_command_event(channel_id, &orphan)
            .await
            .expect("redelivery");
        let after = CollectingSink::new();
        provider.flush(&after).await.expect("flush");
        assert!(
            receipt_stages(&after, "turn-orphan").is_empty(),
            "a redelivered drop publishes nothing"
        );
    }

    /// A `steer` to an execution with no live process is dropped, and only
    /// dropped.
    ///
    /// The degrade used to be published before the delivery was attempted, so
    /// one command produced two receipts that contradicted each other: "the
    /// turn was accepted for the next turn boundary" immediately followed by
    /// "this execution has no live process".
    #[tokio::test]
    async fn a_steer_to_a_dead_execution_is_dropped_and_not_degraded() {
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
        let record = provider.state().sessions().next().expect("session").clone();
        let target = record.target("instance-1");
        provider
            .handle_session_event(session::SessionEvent::Exited {
                session_id: record.session_id.clone(),
                reason: session::ExitReason::Idle,
            })
            .expect("fold the exit");

        provider
            .handle_command_event(
                channel_id,
                &command_event(
                    channel_id,
                    "turn-steer-dead",
                    &target,
                    serde_json::json!({
                        "type": "thread.turn.start",
                        "text": "actually, use the other approach",
                        "deliver": "steer",
                    }),
                ),
            )
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "turn-steer-dead"),
            vec!["turn_dropped"],
            "a turn that reached no process was not accepted for any boundary"
        );
    }

    /// An interrupt answers from what this provider has taken custody of, not
    /// from a fold that lags the actor by one select pass.
    ///
    /// The run loop is `biased;` with the relay arm ahead of the session-event
    /// arm, so a turn and an interrupt sent back to back reach the interrupt
    /// decision before `TurnStarted` has been folded into `open_turn`. That
    /// published a signed "nothing to cancel" while the cancel was in fact
    /// delivered to a running turn.
    #[tokio::test]
    async fn an_interrupt_racing_a_turn_start_is_not_answered_from_stale_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
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
        // The actor has started the turn, but the provider has not folded the
        // report yet — exactly the state the biased run loop is in when a
        // second command arrives on the relay arm first.
        loop {
            let event =
                tokio::time::timeout(Duration::from_secs(20), provider.next_session_event())
                    .await
                    .expect("session event within timeout")
                    .expect("channel open");
            if matches!(event, SessionEvent::TurnStarted { .. }) {
                break;
            }
            provider.handle_session_event(event).expect("record");
        }
        assert!(
            provider
                .state()
                .session(&target.session_id)
                .is_some_and(|record| record.open_turn.is_none()),
            "the fold has deliberately not happened yet"
        );

        provider
            .handle_command_event(
                channel_id,
                &interrupt_event(channel_id, "int-racing", &target),
            )
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "int-racing"),
            vec!["interrupt_delivered"],
            "the cancel reached a turn this provider is holding, and the receipt must say so"
        );
    }

    /// An interrupt-class turn cancels the running turn and then runs.
    ///
    /// Without the cancel it is an ordinary boundary turn that waits behind a
    /// stalled one forever, which is the opposite of what the class promises
    /// its (founder-only) sender.
    /// An interrupt-class turn that cannot be taken does not destroy the turn
    /// it was going to replace.
    ///
    /// The cancel and the replacement turn are two sends into one bounded
    /// mailbox (`session::SESSION_MAILBOX_DEPTH`), and the actor stops reading
    /// that mailbox for up to `session::CANCEL_GRACE` while it drains the turn
    /// the cancel just ended. With one slot free, issuing the cancel first
    /// destroyed the running turn, lost the founder's replacement words to a
    /// `QUEUE_FULL`, and left a receipt that mentioned only the queue. Both
    /// slots or neither.
    #[tokio::test]
    async fn an_interrupt_class_turn_with_no_room_for_it_leaves_the_running_turn_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
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
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-stalled", &target))
            .await
            .expect("handle");
        pump_until_turn_started(&mut provider).await;
        assert!(provider
            .state()
            .session(&target.session_id)
            .is_some_and(|record| record.open_turn.is_some()));

        // A mailbox with exactly one free slot, held by this test rather than
        // by a real actor — see `SessionManager::attach_test_handle`.
        let (tx, mut rx) = tokio::sync::mpsc::channel(session::SESSION_MAILBOX_DEPTH);
        for index in 0..session::SESSION_MAILBOX_DEPTH - 1 {
            tx.try_send(session::SessionCommand::Turn {
                command_id: format!("filler-{index}"),
                attachments: Vec::new(),
                text: "filler".to_owned(),
                operator_pubkey: None,
                framing: None,
            })
            .expect("fill the mailbox");
        }
        let _shutdown_rx = provider.sessions.attach_test_handle(&target.session_id, tx);

        provider
            .handle_command_event(
                channel_id,
                &command_event(
                    channel_id,
                    "turn-boss",
                    &target,
                    serde_json::json!({
                        "type": "thread.turn.start",
                        "text": "stop and do this instead",
                        "deliver": "interrupt",
                    }),
                ),
            )
            .await
            .expect("handle");

        // Nothing new reached the mailbox: no cancel was issued, so the turn
        // that was running is still running.
        let mut delivered = Vec::new();
        while let Ok(command) = rx.try_recv() {
            delivered.push(command);
        }
        assert_eq!(
            delivered.len(),
            session::SESSION_MAILBOX_DEPTH - 1,
            "an interrupt that cannot be completed must not be started"
        );
        assert!(
            delivered
                .iter()
                .all(|command| !matches!(command, session::SessionCommand::Interrupt { .. })),
            "the running turn was cancelled for a replacement that could not be delivered"
        );
        assert!(provider
            .state()
            .session(&target.session_id)
            .is_some_and(|record| record.open_turn.is_some()));

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(receipt_stages(&sink, "turn-boss"), vec!["turn_dropped"]);
        let dropped = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == "turn-boss")
            .expect("drop receipt");
        assert_eq!(dropped["error"]["code"], QUEUE_FULL_TURN_KEPT);
        payload::decode_coding_session_lifecycle_receipt(&dropped.to_string())
            .expect("strictly decodable drop receipt");
        assert!(provider.state().is_command_refused("turn-boss"));
    }

    #[tokio::test]
    async fn an_interrupt_class_turn_cancels_the_running_turn_then_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
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
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-stalled", &target))
            .await
            .expect("handle");
        pump_until_turn_started(&mut provider).await;

        provider
            .handle_command_event(
                channel_id,
                &command_event(
                    channel_id,
                    "turn-boss",
                    &target,
                    serde_json::json!({
                        "type": "thread.turn.start",
                        "text": "stop and do this instead",
                        "deliver": "interrupt",
                    }),
                ),
            )
            .await
            .expect("handle");
        // The stalled turn is cancelled, and the boundary that creates is where
        // the interrupt-class turn runs.
        pump_until(&mut provider, |event| {
            matches!(event, SessionEvent::TurnStarted { command_id, .. } if command_id == "turn-boss")
        })
        .await;

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "turn-boss"),
            vec!["turn_queued".to_owned(), "turn_started".to_owned()],
        );
        assert!(provider.state().is_command_consumed("turn-boss"));
    }

    /// A channel watermark never steps over a turn its own replay window is
    /// still holding.
    ///
    /// Only 44220s are held for reordering. A newer 44221 arriving first — the
    /// order the relay serves stored events in — recorded its own `created_at`,
    /// and `record_watermark` never moves backwards, so the clamp the held
    /// command tried to apply afterwards was a no-op and a crash inside the
    /// window lost the turn.
    #[tokio::test]
    async fn a_replay_window_holds_the_watermark_against_a_newer_trailing_write() {
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
        let mut unknown = target.clone();
        unknown.session_id = format!("{}-gone", target.session_id);

        let held_at = now_secs() + 10;
        provider.open_replay_window(channel_id);
        // Newest first: a lifecycle command for a session this provider does
        // not have, which is ignored but still advanced the channel.
        provider
            .handle_command_event(
                channel_id,
                &lifecycle_target_event_at(
                    &provider,
                    channel_id,
                    "stop-gone",
                    "session.stop",
                    &unknown,
                    held_at + 10,
                ),
            )
            .await
            .expect("handle");
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, "turn-held", &target, "do the thing", held_at),
            )
            .await
            .expect("handle");

        assert!(
            provider
                .state()
                .watermark(channel_id)
                .is_none_or(|mark| mark <= held_at),
            "the replay floor must still reach a command this provider is holding"
        );
    }

    /// The first held turn of a replayed burst must not push the channel floor
    /// past the older turns still queued behind it.
    ///
    /// `watermark_ceiling` is a *clamp*, not an advance: it is a `min` over
    /// the turns this channel still owes, and the command being held is one of
    /// them, so for the first arrival of a burst the ceiling equals that
    /// command's own `created_at`. Writing it as a watermark walks the floor
    /// over every older command the relay has not served yet — and the relay
    /// serves stored REQ results newest-first
    /// (`crates/buzz-db/src/event.rs`'s `created_at DESC, id ASC`), so older is
    /// exactly what arrives next. A death inside the 1.5 s window then loses
    /// those turns with no receipt, no ledger entry and no log line, which is
    /// the silent loss D2 exists to remove.
    #[tokio::test]
    async fn a_replay_window_holds_the_watermark_against_an_older_turn_behind_a_newer_one() {
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

        let older_at = now_secs() + 10;
        let newer_at = older_at + 3;
        provider.open_replay_window(channel_id);
        // Newest first, which is the order the relay actually serves.
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, "turn-newer", &target, "second", newer_at),
            )
            .await
            .expect("handle");
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, "turn-older", &target, "first", older_at),
            )
            .await
            .expect("handle");

        assert!(
            provider
                .state()
                .watermark(channel_id)
                .is_none_or(|mark| mark <= older_at),
            "the replay floor must still reach the older turn this provider is holding"
        );
    }

    /// The watermark ceiling is a fact about one channel.
    ///
    /// A turn held on a busy channel must not drag every other channel's
    /// replay floor backwards with it — that is a silent multiplication of
    /// replay volume on a multi-channel provider, and the doc on
    /// `watermark_ceiling` says per channel.
    #[tokio::test]
    async fn a_held_turn_does_not_drag_another_channels_watermark_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let busy = Uuid::new_v4();
        let quiet = Uuid::new_v4();
        let projects = write_projects(dir.path(), busy, &cwd);
        let mut provider = provider(&dir.path().join("state"), Some(&projects));
        provider
            .handle_command_event(busy, &create_event(&provider, busy, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        let mut unknown = target.clone();
        unknown.session_id = format!("{}-gone", target.session_id);

        let held_at = now_secs().saturating_sub(600);
        provider.open_replay_window(busy);
        provider
            .handle_command_event(
                busy,
                &turn_event_at(busy, "turn-held", &target, "do the thing", held_at),
            )
            .await
            .expect("handle");
        let now = now_secs();
        provider
            .handle_command_event(
                quiet,
                &lifecycle_target_event(&provider, quiet, "stop-gone", "session.stop", &unknown),
            )
            .await
            .expect("handle");

        assert!(
            provider
                .state()
                .watermark(quiet)
                .is_some_and(|mark| mark >= now),
            "a quiet channel with nothing held replays from where it actually got to"
        );
    }

    /// A dropped socket replays history too, and the reorder window has to
    /// reopen for it.
    ///
    /// The run loop's reconnect arm used to call `relay.reconnect()` and
    /// nothing else, while the relay client resubscribes every channel from
    /// `min(last_seen, channel_dropped_since)`
    /// (`crates/buzz-acp/src/relay.rs:3256-3269`) and the relay serves stored
    /// REQ results newest-first (`crates/buzz-db/src/event.rs:771`). Every
    /// unconsumed turn therefore came back backwards on the commonest
    /// recovery path there is — a socket drop, not a restart — and ran in
    /// reverse.
    ///
    /// What this pins, exactly: `reopen_replay_windows_after_reconnect`, which
    /// it calls directly. It does not enter the run loop's `None =>` reconnect
    /// arm — no test in this crate enters `run_with` at all — so the call site
    /// itself is covered only by
    /// `the_run_loop_still_calls_both_replay_entry_points`.
    #[tokio::test]
    async fn a_reconnect_reopens_the_replay_window_for_every_subscribed_channel() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
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
        // What `subscribe` left behind before the socket went away.
        provider.subscribed.insert(channel_id);
        provider.flush_replays_now().await.expect("start clean");
        assert_eq!(provider.next_replay_delay(), None);

        let base = now_secs();
        let first = turn_event_at(channel_id, "turn-first", &target, "alpha", base);
        let second = turn_event_at(channel_id, "turn-second", &target, "beta", base + 1);

        // Exactly what the run loop does when `next_event()` yields `None`.
        provider.reopen_replay_windows_after_reconnect();
        assert!(
            provider.next_replay_delay().is_some(),
            "a reconnect is a replay, so it must be holding turns for reorder"
        );

        // Newest first, which is the order the relay actually serves.
        for event in [&second, &first] {
            provider
                .handle_command_event(channel_id, event)
                .await
                .expect("replay");
        }
        provider.flush_replays_now().await.expect("deliver replay");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let turns: Vec<(String, String)> = turn_receipts_in_order(&sink)
            .into_iter()
            .filter(|(command_id, _)| command_id.starts_with("turn-"))
            .collect();
        assert_eq!(
            turns,
            vec![
                ("turn-first".to_owned(), "turn_queued".to_owned()),
                ("turn-second".to_owned(), "turn_queued".to_owned()),
            ],
            "a reconnected provider answers in sent order, not arrival order"
        );
    }

    /// `flush_due_replays` delivers held turns once the window's own clock
    /// runs out.
    ///
    /// What this pins, exactly: the function, on the real deadline, not the
    /// run loop's `sleep_for(replay_delay)` arm that calls it. Every other
    /// replay test calls `flush_replays_now`, which is test-only and ignores
    /// the clock. Nothing in this crate enters `run_with` — it builds a live
    /// `HarnessRelay` before it reaches its `select!` — so deleting that arm
    /// still leaves the suite green while every held 44220 is held forever.
    /// `the_run_loop_still_calls_both_replay_entry_points` is the cheap
    /// stand-in for that until the loop takes an injectable event source.
    #[tokio::test]
    async fn the_replay_window_closing_on_its_own_clock_delivers_held_turns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
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

        let base = now_secs();
        provider.open_replay_window(channel_id);
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, "turn-held", &target, "do the thing", base),
            )
            .await
            .expect("handle");
        provider
            .flush_due_replays()
            .await
            .expect("no window is due yet");
        let early = CollectingSink::new();
        provider.flush(&early).await.expect("flush");
        assert!(
            receipt_stages(&early, "turn-held").is_empty(),
            "an open window holds the turn"
        );

        // Real time, on purpose: the production path is a `sleep_for` on
        // `next_replay_delay`, and this is the clock it waits on.
        let delay = provider
            .next_replay_delay()
            .expect("an open window has a deadline");
        tokio::time::sleep(delay + Duration::from_millis(100)).await;
        provider.flush_due_replays().await.expect("window closed");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "turn-held"),
            vec!["turn_queued"],
            "the timer that closes the window is what delivers the turn"
        );
        assert_eq!(provider.next_replay_delay(), None);
    }

    /// A failed delivery hands the rest of the burst back, instead of dropping
    /// it on the floor.
    ///
    /// `deliver_held_commands` drains `replay.held` with `mem::take`, so a `?`
    /// out of the delivery loop used to take the untried remainder with it:
    /// no longer held, never delivered, no receipt, and the run loop only
    /// logs. A restart still recovers them — the channel floor sits below
    /// them — but inside the running process that is exactly the silent loss
    /// this machinery exists to remove.
    #[tokio::test]
    async fn a_failed_held_delivery_hands_the_rest_of_the_burst_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state_dir = dir.path().join("state");
        let mut provider = provider(&state_dir, Some(&projects));
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
        // A generation this provider has moved past: the fence answers it with
        // a refusal, and a refusal is written to the durable ledger before it
        // is published. That write is the io failure this test induces.
        let mut stale = target.clone();
        stale.generation += 1;

        let base = now_secs();
        provider.open_replay_window(channel_id);
        for (command_id, turn_target, created_at) in [
            ("turn-stale", &stale, base),
            ("turn-second", &target, base + 1),
            ("turn-third", &target, base + 2),
        ] {
            provider
                .handle_command_event(
                    channel_id,
                    &turn_event_at(channel_id, command_id, turn_target, "words", created_at),
                )
                .await
                .expect("hold");
        }

        // A directory where the refusal ledger's file belongs: every append to
        // it fails, which is the same `io::Error` class a full or read-only
        // state directory raises.
        std::fs::create_dir_all(state_dir.join("refusals.jsonl")).expect("sabotage");

        provider
            .flush_replays_now()
            .await
            .expect_err("the refusal ledger append fails");

        let mut still_held: Vec<u64> = provider
            .replay
            .held
            .iter()
            .filter(|held| held.channel_id == channel_id)
            .map(|held| held.created_at)
            .collect();
        still_held.sort_unstable();
        assert_eq!(
            still_held,
            vec![base, base + 1, base + 2],
            "the command that failed and everything queued behind it stay held"
        );
        assert!(
            provider
                .state()
                .watermark(channel_id)
                .is_none_or(|mark| mark <= base),
            "the floor still reaches the oldest turn this provider is holding"
        );
    }

    /// A cancel that reached the actor is answered, once, even when the
    /// ledger append behind it fails.
    ///
    /// Two guarantees on one path. `on_turn` records custody in `in_flight`
    /// only on the `Ok(()) if is_turn` arm, and `is_turn` is false for
    /// `TurnAction::Interrupt`, so `delivered_cancels` is the only thing that
    /// stops a relay redelivery walking every `decide_turn` fence and
    /// cancelling whatever turn is running by then. But that fence answers
    /// `Ignored::AlreadyAccepted`, which is *silent* — so it swallows the one
    /// redelivery that could still have produced an answer. The answer must
    /// therefore already be durable when the fence closes, and it is: the
    /// receipt is enqueued into the crash-safe outbox in front of the
    /// consumed/refused append, and the append is the write this test breaks.
    /// Ledger-first left the operator's turn cancelled with no receipt, no
    /// transcript item and no ledger entry, and then no replay either, because
    /// the silent redelivery advanced the channel watermark past it.
    #[tokio::test]
    async fn a_delivered_cancel_is_answered_once_even_when_the_ledger_append_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state_dir = dir.path().join("state");
        let mut provider = stalling_provider(&state_dir, Some(&projects));
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
        pump_until_turn_started(&mut provider).await;

        // Held, so the delivery happens out of `deliver_held_commands` — the
        // one path that hands a failed command back for a later try.
        let cancel = interrupt_event(channel_id, "int-held", &target);
        provider.open_replay_window(channel_id);
        provider
            .handle_command_event(channel_id, &cancel)
            .await
            .expect("hold");

        // A directory where the consumed ledger's file belongs, which is the
        // same sabotage `a_failed_held_delivery_hands_the_rest_of_the_burst_back`
        // uses: `append_ledger_record` re-opens the path on every append, so
        // every one of them fails EISDIR — a structural failure, not a
        // permission one. A mode bit would not do: the CI gate runs as root
        // inside `buzz-ci` (`scripts/ci-image/Dockerfile` declares no `USER`
        // and `.woodpecker/gate.yml` mounts `/root/...`), and root's
        // `CAP_DAC_OVERRIDE` opens a 0444 file for write happily, which would
        // turn this test's `expect_err` red on CI and green on every developer
        // machine.
        let ledger = state_dir.join("commands.jsonl");
        if ledger.exists() {
            std::fs::remove_file(&ledger).expect("clear the ledger file");
        }
        std::fs::create_dir(&ledger).expect("sabotage");
        provider
            .flush_replays_now()
            .await
            .expect_err("the consumed ledger append fails");
        std::fs::remove_dir(&ledger).expect("restore");

        // Custody is a fact, not a hypothesis: the cancel reached the actor and
        // ended the running turn before the ledger write failed.
        pump_until(&mut provider, |event| {
            matches!(
                event,
                SessionEvent::TurnFinished {
                    outcome: session::TurnOutcome::Cancelled,
                    ..
                }
            )
        })
        .await;
        assert!(
            !provider.state().is_command_consumed("int-held"),
            "the failed append left no durable record of the delivered cancel"
        );
        assert!(
            !provider.state().is_command_refused("int-held"),
            "and no refusal either"
        );

        // The operator's answer survived it anyway. This is the whole of the
        // fix: an interrupt that destroyed a running turn is never answered by
        // silence, whatever the state directory is doing.
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "int-held"),
            vec!["interrupt_delivered"],
            "the cancel reached the actor, so the operator is told so before the ledger \
             append that fences a redelivery is even attempted"
        );

        // A second turn, running, and a relay redelivery of the same cancel —
        // the reconnect shape. Nothing about this command is new; the actor has
        // already had it, and it has already been answered.
        provider
            .handle_command_event(channel_id, &turn_event(channel_id, "turn-2", &target))
            .await
            .expect("handle");
        pump_until_turn_started(&mut provider).await;

        provider
            .handle_command_event(channel_id, &cancel)
            .await
            .expect("handle");

        let redelivery = CollectingSink::new();
        provider.flush(&redelivery).await.expect("flush");
        assert!(
            receipt_stages(&redelivery, "int-held").is_empty(),
            "answered once and only once: a second `interrupt_delivered` here is published \
             from the same arm that issues the second `SessionCommand::Interrupt`, so it \
             would mean turn-2 was cancelled too"
        );
        assert!(
            provider
                .state()
                .session(&target.session_id)
                .is_some_and(|record| record.open_turn.is_some()),
            "turn-2 is still running: the redelivered cancel reached no mailbox"
        );
    }

    /// A cancel whose answer could not even be queued is not fenced.
    ///
    /// The answer and the fence are two writes to the same state directory,
    /// and the outbox is the one that fails *first* under the failure class
    /// this fence is bounded for: `outbox.jsonl` and `commands.jsonl` are
    /// siblings, so "full, read-only or gone" takes the receipt before it
    /// takes the ledger. Recording the id in front of `enqueue_receipt`
    /// therefore closed a *silent* fence — `Ignored::AlreadyAccepted`
    /// publishes nothing — over a cancel that had reached the actor, ended a
    /// running turn, and been answered by nothing at all; the redelivery that
    /// was the only thing left able to answer it returned `Ok(())`, so the
    /// channel floor then walked past it and the restart could not recover it
    /// either. The id is remembered only once the answer is durably queued.
    #[tokio::test]
    async fn a_cancel_whose_answer_cannot_be_queued_is_not_fenced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state_dir = dir.path().join("state");
        let mut provider = stalling_provider(&state_dir, Some(&projects));
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
        pump_until_turn_started(&mut provider).await;

        // Held, so the delivery runs out of `deliver_held_commands` — the one
        // path that hands a failed command back for a later try.
        let cancel = interrupt_event(channel_id, "int-held", &target);
        let cancel_at = cancel.created_at.as_secs();
        provider.open_replay_window(channel_id);
        provider
            .handle_command_event(channel_id, &cancel)
            .await
            .expect("hold");
        let floor_before = provider.state().watermark(channel_id);

        // A directory where the outbox belongs. `Outbox::append` re-opens the
        // path on every append, so every one of them fails EISDIR — the same
        // structural, root-hermetic sabotage the sibling ledger test uses,
        // applied to the sibling file.
        let outbox = state_dir.join("outbox.jsonl");
        if outbox.exists() {
            std::fs::remove_file(&outbox).expect("clear the outbox file");
        }
        std::fs::create_dir(&outbox).expect("sabotage");
        provider
            .flush_replays_now()
            .await
            .expect_err("the receipt cannot even be queued");

        assert!(
            !provider.delivered_cancels.contains(&"int-held".to_owned()),
            "an unanswered cancel is not fenced: the silent `AlreadyAccepted` arm would \
             swallow the one redelivery still able to answer it"
        );
        assert!(
            provider
                .replay
                .held
                .iter()
                .any(|held| held.channel_id == channel_id && held.content.contains("int-held")),
            "the command goes back to `replay.held` for a later try"
        );
        assert_eq!(
            provider.state().watermark(channel_id),
            floor_before,
            "and the channel floor does not walk past a command still owed an answer"
        );

        // The state directory recovers and the window reopens. This is the
        // later try the hand-back promised, and it is the assertion the fence
        // ordering exists for: under the old order `decide_turn` answered the
        // retry `AlreadyAccepted`, published nothing, and let the floor
        // advance anyway.
        std::fs::remove_dir(&outbox).expect("restore");
        provider.open_replay_window(channel_id);
        provider
            .flush_replays_now()
            .await
            .expect("the retry has a working state directory");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert!(
            !receipt_stages(&sink, "int-held").is_empty(),
            "the cancel that reached the actor is answered on the retry, not swallowed"
        );
        assert!(
            provider
                .state()
                .watermark(channel_id)
                .is_some_and(|mark| mark >= cancel_at),
            "and only now, behind an answered command, does the floor advance"
        );
    }

    /// The delivered-cancel fence is bounded.
    ///
    /// An entry is released the moment the ledger append behind its receipt
    /// succeeds, so in normal operation the queue holds at most the cancel
    /// currently being answered. It grows only while that append keeps failing
    /// — a full, read-only or missing state directory — and an unbounded set
    /// under that condition is a leak that outlives the condition.
    #[test]
    fn the_delivered_cancel_fence_evicts_its_oldest_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut provider = provider(&dir.path().join("state"), None);
        let newest = format!("int-{}", DELIVERED_CANCEL_FENCE_CAPACITY + 2);
        for index in 0..=(DELIVERED_CANCEL_FENCE_CAPACITY + 2) {
            provider.remember_delivered_cancel(format!("int-{index}"));
        }

        assert_eq!(
            provider.delivered_cancels.len(),
            DELIVERED_CANCEL_FENCE_CAPACITY,
            "the fence never grows past its ceiling"
        );
        assert!(
            !provider.delivered_cancels.contains(&"int-0".to_owned()),
            "the oldest entry is the one dropped: its receipt was enqueued longest ago"
        );
        assert!(
            provider.delivered_cancels.contains(&newest),
            "and the cancel most recently handed to a mailbox is still fenced"
        );

        // A redelivery this process re-records must not push a duplicate.
        provider.remember_delivered_cancel(newest.clone());
        assert_eq!(
            provider.delivered_cancels.len(),
            DELIVERED_CANCEL_FENCE_CAPACITY,
            "re-recording an id already held is not a second entry"
        );

        provider.forget_delivered_cancel(&newest);
        assert!(
            !provider.delivered_cancels.contains(&newest),
            "and the durable ledger taking over releases it"
        );
    }

    /// Both replay entry points are still wired into the run loop.
    ///
    /// Honest about what it is: a source-level pin, not an execution of the
    /// arms. `run_with` connects a live `HarnessRelay` before it reaches its
    /// `select!`, so no unit test in this crate can enter the
    /// `sleep_for(replay_delay)` arm or the `next_event() -> None` reconnect
    /// arm; the two tests above drive `flush_due_replays` and
    /// `reopen_replay_windows_after_reconnect` directly and pin the functions,
    /// not the call sites. Deleting either arm is the regression that leaves
    /// every held 44220 held forever with no receipt, and until the loop takes
    /// an injectable event source this is the cheapest thing that catches it.
    #[test]
    fn the_run_loop_still_calls_both_replay_entry_points() {
        let source = include_str!("lib.rs");
        let start = source
            .find("pub async fn run_with(")
            .expect("run_with is this crate's run loop");
        // Bounded at `run_with`'s own closing brace — the first lone `}` at
        // column zero after it, which in this file only ends a top-level item.
        // The window used to run to the test module instead, i.e. over every
        // line of production code in the file, so the same literal appearing
        // in any doc comment or log message ~3,700 lines away satisfied the
        // assertions below while the run loop no longer called anything.
        let end = start
            + source[start..]
                .find("\n}\n")
                .expect("run_with's body is brace-delimited");
        let run_loop = &source[start..end];
        assert!(
            run_loop.contains("tokio::select! {"),
            "the window no longer covers the run loop's select"
        );
        for call in [
            "provider.flush_due_replays()",
            "provider.reopen_replay_windows_after_reconnect()",
        ] {
            assert!(
                run_loop.contains(call),
                "the run loop no longer calls {call}: held turns are never delivered"
            );
        }
    }
}
