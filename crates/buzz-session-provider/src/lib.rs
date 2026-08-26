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
pub mod authority;
pub mod catalog;
pub mod commands;
pub mod config;
pub mod context_projector;
mod context_store;
mod git_probe;
mod lease;
mod model_catalog;
pub mod payload;
pub mod publish;
mod reachability;
pub mod session;
pub mod state;
pub mod transcript;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use nostr::{Event, Kind};
use tokio::sync::mpsc;
use uuid::Uuid;

use buzz_acp::relay::{HarnessRelay, RelayEventPublisher, RestClient};
use buzz_acp::{ChannelFilter, TurnUsage};
use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionDelivery, CodingSessionTarget,
};
use buzz_core::coding_session_context::coding_session_first_turn_brief;
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::coding_session_lease::CodingSessionLeaseState;
use buzz_core::kind::{
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_COMMAND,
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_PROVIDER_CATALOG, KIND_CODING_SESSION_TRANSCRIPT,
    KIND_MEMBER_ADDED_NOTIFICATION, KIND_MEMBER_REMOVED_NOTIFICATION, KIND_SYSTEM_MESSAGE,
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

use commands::{
    decide_lifecycle, CommandContext, CreatePlan, Ignored, LifecycleDecision, ProjectsFile,
    ResumePlan, StopPlan, TurnDecision,
};
use config::Config;
use context_projector::{ContextProjectionLimits, ContextProjectionRequest};
use payload::{
    Capabilities, LifecycleReceipt, SessionMetadata, SessionStatus, TranscriptEnvelope,
    GENESIS_NOT_FOUND, METADATA_SCHEMA, PROVIDER_UNAVAILABLE, SESSION_ALREADY_ATTACHED,
    UNAUTHORIZED_OPERATOR,
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

/// The operator-facing sentence a `turn_degraded` receipt carries.
///
/// One sentence, not a per-execution explanation: see
/// [`Provider::native_steer_deliverable`] for why the payload under a given
/// `(commandId, turn_degraded)` key must not depend on what a particular
/// process learned at `initialize`.
const STEER_DOWNGRADED: &str = "this execution cannot take a mid-turn steer; the turn was \
                                accepted for the next turn boundary instead";

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
    provider.set_rest_client(relay.rest_client());

    // Witness the relay identity (NIP-11 `self`) once, over the same origin
    // the whole authenticated command stream already trusts. Without it,
    // authority-acceptance receipts cannot verify and genesis-bearing
    // sessions stay founder-only — fail closed, never guessed.
    match relay.rest_client().fetch_relay_self_verified().await {
        Ok(Some(relay_self)) => {
            tracing::info!(target: "csp::authority", %relay_self, "witnessed relay identity");
            provider.set_relay_self(relay_self);
        }
        Ok(None) => tracing::warn!(
            target: "csp::authority",
            "relay advertises no stable identity (NIP-11 self) — authority chains cannot be \
             verified; genesis-bearing sessions stay founder-only"
        ),
        Err(error) => tracing::warn!(
            target: "csp::authority",
            "could not witness the relay identity: {error} — genesis-bearing sessions stay \
             founder-only until the provider restarts"
        ),
    }

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

    // Grants accepted while this provider was down are re-verified and folded
    // in before the first command is served; live receipts extend from here.
    let rest = relay.rest_client();
    provider.backfill_authority_chains(&rest).await;

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
    /// Last relay-confirmed reachability fact per session id, keyed and
    /// applied under the exact same generation fence as `git_probes` — see
    /// [`session::SessionEvent::WorktreeObserved`]. Absent, like a `None`
    /// reachability result, means "not checked."
    git_reachability: HashMap<String, reachability::ReachabilityFact>,
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
    /// Whether each live execution's runtime advertised native mid-turn
    /// steering at `initialize`, keyed by session id.
    ///
    /// Per execution, never per driver: this is what the process behind one
    /// generation actually answered. Absent means "no live process was
    /// witnessed", which publishes as `threadSteer: false` — an unwitnessed
    /// capability is not a capability.
    steering: HashMap<String, bool>,
    /// Turn commands held for reordering while a channel's subscription is
    /// replaying history. See [`ReplayWindow`].
    replay: ReplayWindow,
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
            git_reachability: HashMap::new(),
            rest_client: None,
            relay_self: None,
            context_refresh: HashMap::new(),
            subscribed: BTreeSet::new(),
            projects_fingerprint: None,
            pending_leases: HashMap::new(),
            established_leases: HashSet::new(),
            first_lease_prerequisites: HashMap::new(),
            in_flight: HashMap::new(),
            steering: HashMap::new(),
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
    pub fn recover(&mut self) -> anyhow::Result<()> {
        // Best-effort: a state directory that refuses the sweep must not stop
        // the provider from coming back up. The consequence is disk, not
        // correctness — the reader of those files is already gone.
        if let Err(error) = context_store::remove_all_context_packages(&self.config.state_dir) {
            tracing::warn!(
                target: "csp::context",
                "leftover verified-context packages could not be swept: {error}"
            );
        }
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
                KIND_SYSTEM_MESSAGE,
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
                    &command.content,
                )
                .await;
            if let Err(error) = delivered {
                // Back it goes with the rest — but not because it "never
                // reached a mailbox": `on_turn` can fail *after*
                // `SessionHandle::deliver` took the turn, because the
                // `turn_queued` (or `turn_degraded`) `enqueue_receipt` right
                // behind the delivery is itself fallible. What makes the
                // redelivery safe on that path is the process-local
                // `in_flight` fence — `decide_turn` answers
                // `Ignored::AlreadyAccepted` for a command this provider has
                // already handed to a session (`commands.rs`) — not the
                // ledgers, which are written when a turn *starts* or when it
                // is answered terminally and so say nothing about a turn
                // sitting in a session's mailbox. The ledgers cover the other
                // two shapes; the fence covers this one, and it dies with the
                // process, where replay from the channel floor takes over.
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
                self.on_turn(channel_id, created_at, &operator_pubkey, &event.content)
                    .await?;
            }
            KIND_SYSTEM_MESSAGE => {
                self.on_authority_receipt(channel_id, event, relay).await?;
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
                self.create_session(plan, relay).await
            }
            LifecycleDecision::Resume(plan) => self.resume_session(plan, relay).await,
            LifecycleDecision::Stop(plan) => self.stop_session(plan),
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

        let target = CodingSessionTarget {
            driver: descriptor.driver.clone(),
            instance_id: self.config.instance_id.clone(),
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
        let rehydration = self
            .prepare_rehydration_context(
                &plan.command_id,
                plan.channel_id,
                plan.session_ref.as_deref(),
                plan.genesis_ref.as_deref(),
                &target.session_id,
                relay,
            )
            .await;
        let context_package_id = rehydration
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.package_id.clone());
        let unavailable_reason = rehydration.unavailable_reason;
        let rehydration_mcp = rehydration.descriptor;
        let request = CreateRequest {
            target: target.clone(),
            channel_id: plan.channel_id,
            cwd: plan.cwd.clone(),
            title: plan.title.clone(),
            model: plan.model.clone(),
            resume_cursor: None,
            rehydration_mcp,
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

        let events = self.sessions.event_sender();
        let startup_future = SessionManager::start(request, events);
        let started = match self
            .await_with_lease_maintenance(startup_future, relay.map(HarnessRelay::event_publisher))
            .await
        {
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

        let record = SessionRecord {
            session_id: target.session_id.clone(),
            generation: target.generation,
            channel_id: plan.channel_id,
            command_id: plan.command_id.clone(),
            generation_command_id: Some(plan.command_id.clone()),
            provider_instance_ref: descriptor.instance_ref.clone(),
            runtime: descriptor.runtime.clone(),
            driver: descriptor.driver.clone(),
            cwd: plan.cwd,
            project_ref: plan.project_ref.clone(),
            repo_ref: plan.repo_ref.clone(),
            session_ref: plan.session_ref.clone(),
            genesis_ref: plan.genesis_ref.clone(),
            founder_pubkey: Some(plan.founder_pubkey.clone()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: startup.model.clone().or_else(|| plan.model.clone()),
            resume_cursor: Some(startup.acp_session_id.clone()),
            title: plan.title.clone(),
            created_at_ms: now_ms(),
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: startup.bootstrap_transport,
            open_turn: None,
            closed: false,
        };
        self.state.insert_session(record)?;

        if let Some(package_id) = context_package_id {
            self.context_refresh.insert(
                target.session_id.clone(),
                ContextRefreshState::opened(package_id),
            );
        }

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
        if plan.genesis_ref.is_some() {
            if let Some(relay) = relay {
                let rest = relay.rest_client();
                if let Err(error) = self
                    .backfill_session_authority(&target.session_id, &rest)
                    .await
                {
                    tracing::warn!(
                        target: "csp::authority",
                        session_id = %target.session_id,
                        "authority backfill at create failed: {error}"
                    );
                }
            }
        }

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
                    // The create's own `commandId`, not a derived one: the
                    // first turn's `user_prompt` echo has to name a command
                    // the operator actually sent, or the prompt they typed
                    // into the create dialog is the one prompt in the session
                    // they cannot join to anything.
                    command_id: plan.command_id.clone(),
                    text: text.clone(),
                    // The create's verified signer *is* the operator driving
                    // this first turn — the same fact that made them founder.
                    operator_pubkey: Some(plan.founder_pubkey.clone()),
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
        command_id: &str,
        channel_id: Uuid,
        session_ref: Option<&str>,
        genesis_ref: Option<&str>,
        package_id: &str,
        relay: Option<&HarnessRelay>,
    ) -> RehydrationOutcome {
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
        })
    }

    async fn resume_session(
        &mut self,
        plan: ResumePlan,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        let outbox_before = self.outbox.pending_keys();
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
        let previous_semantic_key = coding_session_target_key(&self.target_for(&record));
        let package_id = Uuid::new_v4().to_string();
        let rehydration = self
            .prepare_rehydration_context(
                &plan.command_id,
                record.channel_id,
                record.session_ref.as_deref(),
                record.genesis_ref.as_deref(),
                &package_id,
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
        let request = CreateRequest {
            target: target.clone(),
            channel_id: record.channel_id,
            cwd: record.cwd.clone(),
            title: record.title.clone(),
            model: record.model.clone(),
            resume_cursor: record.resume_cursor.clone(),
            rehydration_mcp,
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
        let events = self.sessions.event_sender();
        let startup_future = SessionManager::start(request, events);
        let started = match self
            .await_with_lease_maintenance(startup_future, relay.map(HarnessRelay::event_publisher))
            .await
        {
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

        if let Err(error) = self.state.update_session(&record.session_id, |record| {
            record.generation = generation;
            record.generation_command_id = Some(plan.command_id.clone());
            record.next_seq = 1;
            record.next_lease_sequence = 1;
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
        let mut completion_errors = Vec::new();
        if let Err(error) = self.state.consume_command(&plan.command_id, now_secs()) {
            completion_errors.push(format!("consume stop command: {error}"));
        }
        self.sessions.shutdown(&plan.target.session_id);
        self.discard_context_packages(&plan.target.session_id);
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
        content: &str,
    ) -> anyhow::Result<()> {
        let projects = ProjectsFile::default();
        let decision = commands::decide_turn(
            &self.context(&projects, operator_pubkey),
            created_at,
            content,
        );
        let (command_id, target, deliver, message) = match decision {
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
                    tracing::warn!(
                        target: "csp",
                        %command_id,
                        code = refusal.code,
                        "turn command refused: {}",
                        refusal.message
                    );
                    // Durable *before* the publish: a refusal is an answer,
                    // and an answer given twice under the same semantic key
                    // after a restart is a stutter, not a second fact.
                    self.state.record_refusal(&command_id, now_secs())?;
                    return self.enqueue_receipt(channel_id, &command_id, &receipt);
                }
                return Ok(());
            }
            TurnDecision::Fail {
                command_id,
                target,
                message,
            } => {
                // Refused, not consumed. Consuming would claim this command
                // ran; it did not, and never will.
                self.state.record_refusal(&command_id, now_secs())?;
                let receipt = LifecycleReceipt::turn_refused(
                    &command_id,
                    &target,
                    UNAUTHORIZED_OPERATOR,
                    &message,
                );
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
                deliver,
            } => (
                command_id.clone(),
                target,
                deliver,
                // `decide_turn` returns `Start` only after checking this exact
                // signer against the session's founder/granted-operator set,
                // so attributing the turn to them is a witnessed fact.
                SessionCommand::Turn {
                    command_id,
                    text,
                    operator_pubkey: Some(operator_pubkey.to_owned()),
                },
            ),
            TurnDecision::Interrupt { command_id, target } => (
                command_id.clone(),
                target,
                CodingSessionDelivery::Boundary,
                SessionCommand::Interrupt { command_id },
            ),
        };
        let session_id = target.session_id.clone();

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
                return self.enqueue_receipt(channel_id, &command_id, &receipt);
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

        match self.sessions.handle(&session_id) {
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
                    let receipt = LifecycleReceipt::turn_queued(&command_id, &target);
                    self.enqueue_receipt(channel_id, &command_id, &receipt)?;
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
                        // Consumed: the cancel was issued, which is the whole
                        // of what an interrupt does.
                        self.state.consume_command(&command_id, now_secs())?;
                        LifecycleReceipt::interrupt_delivered(&command_id, &target)
                    } else {
                        // Refused, not consumed — the two ledgers answer
                        // different questions and a command belongs in exactly
                        // one of them.
                        self.state.record_refusal(&command_id, now_secs())?;
                        LifecycleReceipt::turn_refused(
                            &command_id,
                            &target,
                            payload::NO_TURN_IN_FLIGHT,
                            "the execution is live but had no turn in flight to cancel",
                        )
                    };
                    self.enqueue_receipt(channel_id, &command_id, &receipt)?;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "csp",
                        %command_id,
                        %session_id,
                        "could not deliver command to session: {error:?}"
                    );
                    self.report_undelivered_turn(
                        channel_id,
                        &command_id,
                        &target,
                        is_turn,
                        &error,
                    )?;
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
                self.report_no_live_execution(channel_id, &command_id, &target, is_turn)?;
            }
        }
        Ok(())
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
    ) -> anyhow::Result<()> {
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
                self.enqueue_receipt(channel_id, command_id, &receipt)
            }
            DeliverError::QueueFull => {
                self.state.record_refusal(command_id, now_secs())?;
                let receipt = LifecycleReceipt::turn_refused(
                    command_id,
                    target,
                    payload::QUEUE_FULL,
                    "the execution's queue is full, so the interrupt could not be delivered",
                );
                self.enqueue_receipt(channel_id, command_id, &receipt)
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
    fn report_no_live_execution(
        &mut self,
        channel_id: Uuid,
        command_id: &str,
        target: &CodingSessionTarget,
        is_turn: bool,
    ) -> anyhow::Result<()> {
        let receipt = if is_turn {
            // Refused, not consumed: the turn never ran, and it never will.
            // Recording it terminally is also what lets the channel watermark
            // move — an unanswered command holds the replay floor, an answered
            // one does not.
            self.state.record_refusal(command_id, now_secs())?;
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
            self.state.record_refusal(command_id, now_secs())?;
            LifecycleReceipt::turn_refused(
                command_id,
                target,
                payload::NO_LIVE_EXECUTION,
                "this execution has no live process, so there is no turn to interrupt",
            )
        };
        self.enqueue_receipt(channel_id, command_id, &receipt)
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
            if accepted.seq == applied_seq + 1 {
                self.resolve_and_apply_grant(&session_id, &accepted, &rest)
                    .await?;
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
                        .await?;
                }
            }
        }
        Ok(())
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
            }
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
    pub async fn backfill_session_authority(
        &mut self,
        session_id: &str,
        rest: &RestClient,
    ) -> anyhow::Result<()> {
        use nostr::{Alphabet, SingleLetterTag};

        let Some(record) = self.state.session(session_id) else {
            return Ok(());
        };
        let Some(genesis_ref) = record.genesis_ref.clone() else {
            return Ok(()); // Legacy sessions have no chain (R20).
        };
        let channel_id = record.channel_id;
        let mut applied_seq = record.authority_seq;
        let Some(relay_self) = self.relay_self.clone() else {
            tracing::debug!(
                target: "csp::authority",
                "no relay identity witnessed — skipping authority backfill"
            );
            return Ok(());
        };
        let Ok(relay_author) = nostr::PublicKey::from_hex(&relay_self) else {
            tracing::warn!(
                target: "csp::authority",
                "witnessed relay identity is not a valid pubkey — skipping authority backfill"
            );
            return Ok(());
        };

        let filter = nostr::Filter::new()
            .kind(Kind::Custom(KIND_SYSTEM_MESSAGE as u16))
            .author(relay_author)
            .custom_tags(
                SingleLetterTag::lowercase(Alphabet::H),
                [channel_id.to_string()],
            )
            .limit(AUTHORITY_BACKFILL_QUERY_LIMIT);
        let rows = match rest.query(&[filter]).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    "authority backfill query failed: {error}"
                );
                return Ok(());
            }
        };
        let Some(rows) = rows.as_array() else {
            tracing::warn!(
                target: "csp::authority",
                "authority backfill query returned a non-array response"
            );
            return Ok(());
        };

        let mut accepted: Vec<authority::AcceptedTransition> = rows
            .iter()
            .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
            .filter(|event| authority::looks_like_acceptance_receipt(&event.content))
            .filter_map(|event| {
                authority::verify_acceptance_receipt(&event, &relay_self, channel_id)
                    .map_err(|error| {
                        tracing::warn!(
                            target: "csp::authority",
                            "skipping unverifiable stored acceptance receipt: {error}"
                        );
                    })
                    .ok()
            })
            .filter(|accepted| accepted.genesis_ref == genesis_ref)
            .collect();
        accepted.sort_by_key(|accepted| accepted.seq);

        for accepted in accepted {
            if accepted.seq <= applied_seq {
                continue;
            }
            if accepted.seq != applied_seq + 1 {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    applied_seq,
                    next_seq = accepted.seq,
                    "authority chain has a receipt gap — later grants stay unapplied"
                );
                break;
            }
            if self
                .resolve_and_apply_grant(session_id, &accepted, rest)
                .await?
            {
                applied_seq = accepted.seq;
            } else {
                // An unverifiable link is never skipped over: everything past
                // it waits until it can be verified.
                break;
            }
        }
        Ok(())
    }

    /// Backfill every live genesis-bearing session's authority chain.
    /// Best-effort: called at startup so grants accepted while the provider
    /// was down are folded in before the first command is served.
    pub async fn backfill_authority_chains(&mut self, rest: &RestClient) {
        let session_ids: Vec<String> = self
            .state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.is_some())
            .map(|record| record.session_id.clone())
            .collect();
        for session_id in session_ids {
            if let Err(error) = self.backfill_session_authority(&session_id, rest).await {
                tracing::warn!(
                    target: "csp::authority",
                    %session_id,
                    "authority backfill failed: {error}"
                );
            }
        }
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
            in_flight: &self.in_flight,
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
        }
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
            &semantic_key,
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
        let item = match workspace_root {
            Some(root) => transcript::fit_item_for_workspace(
                item,
                overhead,
                MAX_TRANSCRIPT_CONTENT_BYTES,
                &root,
            ),
            None => transcript::fit_item(item, overhead, MAX_TRANSCRIPT_CONTENT_BYTES),
        };
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
                // **This** is where a turn command is consumed — the moment it
                // actually begins, not the moment it was accepted. Everything
                // between accept and here is recoverable: a crash in that
                // window leaves the command unconsumed, the channel watermark
                // still behind it, and the replay runs it exactly once. The
                // durable write happens before the receipt so a crash between
                // the two costs a receipt, never a duplicate turn.
                self.state.consume_command(&command_id, now_secs())?;
                self.in_flight.remove(&command_id);
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
                self.in_flight.remove(&command_id);
                let Some((channel_id, target)) = self.locate(&session_id) else {
                    return Ok(());
                };
                // Terminal: the operator is told the turn was dropped, so a
                // redelivery must not quietly run it later and make that
                // receipt a lie.
                self.state.record_refusal(&command_id, now_secs())?;
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
                // The transcript item says it to a reader of the session; the
                // receipt says it to the operator whose turn it was, keyed by
                // the command they sent.
                let receipt = LifecycleReceipt::turn_dropped(
                    &command_id,
                    &target,
                    payload::QUEUE_FULL,
                    "the execution's queue is full",
                );
                self.enqueue_receipt(channel_id, &command_id, &receipt)?;
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
        Ok(self.outbox.flush(sink).await?)
    }

    async fn flush_one<S: EventSink>(&mut self, sink: &S) -> anyhow::Result<usize> {
        Ok(self.outbox.flush_one(sink).await?)
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
        events: Vec<Event>,
        queries: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    /// Minimal NIP-01 filter matching for the fake relay's `/query` bridge:
    /// `ids`, `kinds`, `authors`, and `#h` — the fields the provider's
    /// genesis resolution, transition resolution, and authority backfill use.
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
        true
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
        state
            .queries
            .lock()
            .expect("queries lock")
            .push(query.clone());
        let filters: Vec<serde_json::Value> = query.as_array().cloned().unwrap_or_default();
        let matched: Vec<&Event> = state
            .events
            .iter()
            .filter(|event| {
                filters
                    .iter()
                    .any(|filter| test_filter_matches(filter, event))
            })
            .collect();
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
        let queries = Arc::new(Mutex::new(Vec::new()));
        let state = TestRelayState {
            events,
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
            context_mcp_command: None,
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

    /// A `grant-operator` transition signed by the founder (the session
    /// owner), as the desktop would publish it.
    fn grant_transition_event(
        channel_id: Uuid,
        genesis_ref: &str,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_hex: &str,
    ) -> Event {
        let payload = buzz_core::coding_session_authority_transition::
            CodingSessionAuthorityTransitionPayload::new_grant_operator(
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
            founder_pubkey: Some(test_operator_keys().public_key().to_hex()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: None,
            resume_cursor: None,
            title: None,
            created_at_ms: now_ms(),
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
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
        let app = Router::new().route("/", get(test_relay_ws)).route(
            "/query",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(120)).await;
                axum::Json(serde_json::json!({ "events": [] }))
            }),
        );
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
            provider.recover().expect("first recover");
            context_store::write_context_package(&state_dir, &first, &empty_context_package())
                .expect("write first");
            context_store::write_context_package(&state_dir, &second, &empty_context_package())
                .expect("write second");
        }

        let mut restarted = provider_with_sidecar(&state_dir, None);
        restarted.recover().expect("recover");
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
                    "cmd-1",
                    channel_id,
                    Some(session_ref),
                    Some(&genesis_ref),
                    &package_id,
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
                    "cmd-2",
                    channel_id,
                    Some(session_ref),
                    Some(&genesis_ref),
                    &package_id,
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
                    "cmd-3",
                    channel_id,
                    None,
                    Some(&genesis_ref),
                    &package_id,
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
                    "cmd-4",
                    channel_id,
                    Some(session_ref),
                    None,
                    &package_id,
                    None,
                )
                .await
                .unavailable_reason,
            Some("no_prior_execution")
        );
        assert_eq!(
            provider
                .prepare_rehydration_context(
                    "cmd-5",
                    channel_id,
                    Some(session_ref),
                    Some(&genesis_ref),
                    &package_id,
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
                "cmd-6",
                channel_id,
                Some(session_ref),
                Some(&genesis_ref),
                &package_id,
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
    /// restart — while stop/resume stay owner-only for the same grantee.
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
                    generation_command_id: None,
                    provider_instance_ref: "codex-primary".into(),
                    runtime: "codex".into(),
                    driver: "codex-acp".into(),
                    cwd: dir.path().join("checkout"),
                    project_ref: None,
                    repo_ref: None,
                    session_ref: None,
                    genesis_ref: None,
                    founder_pubkey: Some("ab".repeat(32)),
                    granted_operators: std::collections::BTreeSet::new(),
                    granted_viewers: std::collections::BTreeSet::new(),
                    authority_seq: 0,
                    model: None,
                    resume_cursor: Some("private-acp-cursor".into()),
                    title: None,
                    created_at_ms: now_ms(),
                    next_seq: 3,
                    next_lease_sequence: 1,
                    bootstrap_transport: None,
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
        restarted.recover().expect("recover");
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
                text: "filler".to_owned(),
                operator_pubkey: None,
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
        // Bounded at the test module so this test's own doc comment, which
        // names both functions, cannot satisfy the assertions below.
        let end = start
            + source[start..]
                .find("\n#[cfg(test)]")
                .expect("the test module follows the run loop");
        let run_loop = &source[start..end];
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
