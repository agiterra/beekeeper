//! Admission budgets: what a client frame costs, and where it is charged.
//!
//! Three per-**connection** budgets live in process memory — reads (REQ +
//! COUNT), durable EVENTs, and ephemeral EVENTs — so one device's read storm
//! never spends another device's write budget, and a read never touches
//! Redis. The per-(community, pubkey) message quota stays shared in Redis,
//! because it is the one limit that must hold across every socket a key
//! holds and every pod that key lands on.
//!
//! Every limiter here is a fixed window, which admits up to 2× the limit
//! across a window boundary. That is unchanged from before the split.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use buzz_auth::{LimitType, RateLimiter};
use buzz_core::kind::{is_ephemeral, KIND_PRESENCE_UPDATE, KIND_TYPING_INDICATOR};
use buzz_core::{CommunityId, TenantContext};
use nostr::PublicKey;

use crate::config::ConfigError;
use crate::protocol::ClientMessage;
use crate::state::ScopedRateLimiter;

/// Length of every per-connection burst window, in seconds.
///
/// Desktop and mobile startup each establish dozens of subscriptions at
/// once. The configured per-second rate is preserved as an average while a
/// bounded burst of `rate × window` is admitted at once.
pub(crate) const WS_BURST_WINDOW_SECS: u64 = 5;

/// Presence updates (kind 20001) admitted per second per (community, pubkey).
pub(crate) const EPHEMERAL_PRESENCE_PER_SEC: u32 = 5;
/// Typing indicators (kind 20002) admitted per second per (community, pubkey).
pub(crate) const EPHEMERAL_TYPING_PER_SEC: u32 = 5;
/// Any other generic ephemeral kind admitted per second per (community,
/// pubkey). NIP-ST shared-terminal kinds and agent observer frames take
/// their own branches in `handlers::event` and are not charged here.
pub(crate) const EPHEMERAL_OTHER_PER_SEC: u32 = 10;
/// Window of the per-kind ephemeral limiters, in seconds.
pub(crate) const EPHEMERAL_KIND_WINDOW_SECS: u64 = 1;

/// The rejection text the per-kind ephemeral limiter sends. The `retry in`
/// hint names [`EPHEMERAL_KIND_WINDOW_SECS`].
pub(crate) const EPHEMERAL_KIND_REJECTION: &str =
    "rate-limited: ephemeral kind rate exceeded; retry in 1s";

/// Which per-connection budget a client frame draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Budget {
    /// REQ and COUNT. Local only; never consults Redis.
    Reads,
    /// EVENT of a stored kind. Local burst, then the shared message quota.
    Durable,
    /// EVENT of an ephemeral kind (20000–29999). Local burst, then the
    /// shared message quota — ephemeral frames stay counted against it.
    Ephemeral,
}

impl Budget {
    /// The word a rejection names this budget by:
    /// `rate-limited: {label} quota exceeded; retry in {n}s`.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Reads => "read",
            Self::Durable => "message",
            Self::Ephemeral => "ephemeral",
        }
    }
}

/// The budget `msg` is charged to, or `None` for AUTH and CLOSE, which are
/// free: refusing either would strand a connection that is trying to
/// authenticate or to release load.
pub(crate) fn budget_for(msg: &ClientMessage) -> Option<Budget> {
    match msg {
        ClientMessage::Req { .. } | ClientMessage::Count { .. } => Some(Budget::Reads),
        ClientMessage::Event(event) => {
            let kind = buzz_core::kind::event_kind_u32(event);
            Some(if is_ephemeral(kind) {
                Budget::Ephemeral
            } else {
                Budget::Durable
            })
        }
        ClientMessage::Auth(_) | ClientMessage::Close(_) => None,
    }
}

/// Why a process-local budget refused a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalRejection {
    /// Whole seconds until the window resets — the `retry in {n}s` hint.
    /// Never zero: a client told to retry in 0 s retries immediately.
    pub(crate) reset_in_secs: u64,
    /// True for the first refusal of a window, so callers can log once per
    /// window instead of once per refused frame.
    pub(crate) first_in_window: bool,
}

#[derive(Debug, Default)]
struct WindowState {
    count: u64,
    started: Option<Instant>,
}

/// A fixed-window counter local to one connection.
///
/// `admit` takes the clock as an argument so the window logic is testable
/// without sleeping. Poisoning is tolerated: the counter is plain data and
/// a panic elsewhere must not turn admission into a permanent refusal.
#[derive(Debug, Default)]
pub struct WindowBudget {
    inner: Mutex<WindowState>,
}

impl WindowBudget {
    /// Charges one unit against `limit` per `window`, starting a window on
    /// first use and on expiry.
    pub(crate) fn admit(
        &self,
        limit: u64,
        window: Duration,
        now: Instant,
    ) -> Result<(), LocalRejection> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let started = match state.started {
            Some(started) if now.saturating_duration_since(started) < window => started,
            _ => {
                state.started = Some(now);
                state.count = 0;
                now
            }
        };
        state.count = state.count.saturating_add(1);
        if state.count <= limit {
            return Ok(());
        }
        let elapsed = now.saturating_duration_since(started);
        let remaining = window.saturating_sub(elapsed).as_secs_f64().ceil() as u64;
        Err(LocalRejection {
            reset_in_secs: remaining.max(1),
            first_in_window: state.count == limit.saturating_add(1),
        })
    }
}

/// The three per-connection budgets, one [`WindowBudget`] each. Held by
/// `ConnectionState`; the relay charges frames through `crate::rejection`.
#[derive(Debug, Default)]
pub struct ConnectionBudgets {
    reads: WindowBudget,
    durable: WindowBudget,
    ephemeral: WindowBudget,
}

impl ConnectionBudgets {
    /// The counter behind `budget`.
    pub(crate) fn get(&self, budget: Budget) -> &WindowBudget {
        match budget {
            Budget::Reads => &self.reads,
            Budget::Durable => &self.durable,
            Budget::Ephemeral => &self.ephemeral,
        }
    }
}

/// The per-second rate configured for `budget`.
pub(crate) fn per_second_rate(limits: &buzz_auth::RateLimitConfig, budget: Budget) -> u64 {
    match budget {
        Budget::Reads => limits.ws_reads_per_sec,
        Budget::Durable => limits.human_ws_events_per_sec,
        Budget::Ephemeral => limits.ws_ephemeral_events_per_sec,
    }
}

/// Turns a per-second rate into `(window_secs, limit)` over the burst window.
pub(crate) fn ws_admission_budget(per_second_limit: u64) -> (u64, u64) {
    (
        WS_BURST_WINDOW_SECS,
        per_second_limit.saturating_mul(WS_BURST_WINDOW_SECS),
    )
}

/// Why the shared, Redis-backed quota refused a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    /// The counter is over its limit.
    Exceeded {
        /// Seconds until the Redis key expires.
        reset_in_secs: u64,
        /// True when this call was the one that crossed the limit, so
        /// callers can log once per window.
        first_in_window: bool,
    },
    /// Redis did not answer; the shared quota fails closed.
    Unavailable,
}

/// Charges one unit against the shared per-(community, pubkey) quota.
pub(crate) async fn check_principal<L: RateLimiter>(
    limiter: &L,
    tenant: &TenantContext,
    pubkey: &PublicKey,
    limit_type: LimitType,
    window_secs: u64,
    limit: u64,
) -> Result<(), AdmissionError> {
    match limiter
        .check_and_increment(tenant, pubkey, limit_type, window_secs, limit)
        .await
    {
        Ok(result) if result.allowed => Ok(()),
        Ok(result) => Err(AdmissionError::Exceeded {
            // A window that expires this very second still reads `retry in
            // 1s`: every client parses the hint as a wait, and 0 is "now".
            reset_in_secs: result.reset_in_secs.max(1),
            first_in_window: result.current == result.limit.saturating_add(1),
        }),
        Err(error) => {
            tracing::warn!(error = %error, "shared rate-limit admission unavailable");
            Err(AdmissionError::Unavailable)
        }
    }
}

/// Check + bump a per-key fixed one-second window in a process-local map.
///
/// The shape every per-kind limiter in the relay shares (NIP-ST frames,
/// watches and input; agent observer frames; generic ephemeral kinds): the
/// first `limit` calls in a second are admitted, the rest refused, and the
/// window restarts one second after it began. `now` is injected so the
/// boundary is testable without sleeping.
pub(crate) fn local_kind_window_limited<K>(
    limiter: &dashmap::DashMap<K, (u32, Instant)>,
    key: K,
    limit: u32,
    now: Instant,
) -> bool
where
    K: std::hash::Hash + Eq,
{
    local_kind_window_check(limiter, key, limit, now).limited
}

/// What one per-kind window said about a frame: refused or not, and whether
/// this refusal is the first of its window — the one worth a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KindWindowVerdict {
    /// The frame is over the kind's ceiling for this window.
    pub limited: bool,
    /// `limited`, and no earlier frame in this window was.
    pub first_over: bool,
}

/// [`local_kind_window_limited`] with the first-refusal flag, for callers
/// that log.
pub(crate) fn local_kind_window_check<K>(
    limiter: &dashmap::DashMap<K, (u32, Instant)>,
    key: K,
    limit: u32,
    now: Instant,
) -> KindWindowVerdict
where
    K: std::hash::Hash + Eq,
{
    let mut entry = limiter.entry(key).or_insert((0, now));
    let (count, window_start) = entry.value_mut();
    if now.saturating_duration_since(*window_start).as_secs() >= EPHEMERAL_KIND_WINDOW_SECS {
        *count = 1;
        *window_start = now;
        KindWindowVerdict {
            limited: false,
            first_over: false,
        }
    } else {
        *count = count.saturating_add(1);
        KindWindowVerdict {
            limited: *count > limit,
            first_over: *count == limit.saturating_add(1),
        }
    }
}

/// The per-second ceiling for a generic ephemeral `kind` per (community, pubkey).
pub(crate) fn ephemeral_kind_limit(kind: u32) -> u32 {
    match kind {
        KIND_PRESENCE_UPDATE => EPHEMERAL_PRESENCE_PER_SEC,
        KIND_TYPING_INDICATOR => EPHEMERAL_TYPING_PER_SEC,
        _ => EPHEMERAL_OTHER_PER_SEC,
    }
}

/// Convenience over [`local_kind_window_limited`] for the (community, pubkey)
/// limiters on `AppState`.
pub(crate) fn scoped_kind_window_limited(
    limiter: &ScopedRateLimiter,
    community_id: CommunityId,
    pubkey: [u8; 32],
    limit: u32,
) -> bool {
    local_kind_window_limited(limiter, (community_id, pubkey), limit, Instant::now())
}

/// Whether a `pubkey` holding `live` sockets in one community (this one
/// included) is over `max`.
pub(crate) fn connection_cap_exceeded(live: usize, max: u64) -> bool {
    u64::try_from(live).map_or(true, |live| live > max)
}

/// The NOTICE a socket over the per-key cap is refused with.
pub(crate) const TOO_MANY_CONNECTIONS: &str = "rate-limited: too many connections for this key";

/// Reads a positive integer from `name`, or `default` when unset.
pub(crate) fn positive_u64_from_env(name: &str, default: u64) -> Result<u64, ConfigError> {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| ConfigError::InvalidValue(format!("{name} must be a positive integer"))),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidValue(format!(
            "{name} must be valid Unicode"
        ))),
    }
}

/// Builds the rate-limit configuration from `BUZZ_RATE_LIMIT_*` and
/// `BUZZ_MAX_WS_CONNECTIONS_PER_PUBKEY`, falling back to
/// [`buzz_auth::RateLimitConfig::default`] per field. Lives beside the
/// budgets it configures; `Config::from_env` calls it.
pub(crate) fn rate_limit_config_from_env() -> Result<buzz_auth::RateLimitConfig, ConfigError> {
    let defaults = buzz_auth::RateLimitConfig::default();
    Ok(buzz_auth::RateLimitConfig {
        human_messages_per_min: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_HUMAN_MESSAGES_PER_MIN",
            defaults.human_messages_per_min,
        )?,
        human_api_calls_per_min: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_HUMAN_API_CALLS_PER_MIN",
            defaults.human_api_calls_per_min,
        )?,
        human_ws_events_per_sec: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_HUMAN_WS_EVENTS_PER_SEC",
            defaults.human_ws_events_per_sec,
        )?,
        ws_reads_per_sec: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_WS_READS_PER_SEC",
            defaults.ws_reads_per_sec,
        )?,
        ws_ephemeral_events_per_sec: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_WS_EPHEMERAL_PER_SEC",
            defaults.ws_ephemeral_events_per_sec,
        )?,
        max_ws_connections_per_pubkey: positive_u64_from_env(
            "BUZZ_MAX_WS_CONNECTIONS_PER_PUBKEY",
            defaults.max_ws_connections_per_pubkey,
        )?,
        agent_standard_messages_per_min: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_AGENT_STANDARD_MESSAGES_PER_MIN",
            defaults.agent_standard_messages_per_min,
        )?,
        agent_standard_api_calls_per_min: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_AGENT_STANDARD_API_CALLS_PER_MIN",
            defaults.agent_standard_api_calls_per_min,
        )?,
        agent_elevated_messages_per_min: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_AGENT_ELEVATED_MESSAGES_PER_MIN",
            defaults.agent_elevated_messages_per_min,
        )?,
        agent_platform_messages_per_min: positive_u64_from_env(
            "BUZZ_RATE_LIMIT_AGENT_PLATFORM_MESSAGES_PER_MIN",
            defaults.agent_platform_messages_per_min,
        )?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::net::IpAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use buzz_auth::{AuthError, RateLimitResult, RateLimiter};
    use nostr::{EventBuilder, Keys, Kind};
    use uuid::Uuid;

    use super::*;

    /// What the stub answers every shared check with.
    pub(crate) enum StubOutcome {
        /// Admitted; `current` is the call's ordinal so first-in-window is
        /// observable.
        Allowed,
        /// Refused as the first call over a limit of 10.
        Denied,
        /// Redis unreachable.
        Failed,
    }

    /// A [`RateLimiter`] that records every call it receives. Shared with
    /// `crate::rejection`'s tests, which need to prove which frames reach
    /// the shared quota and which never do.
    pub(crate) struct StubLimiter {
        pub(crate) outcome: StubOutcome,
        pub(crate) calls: AtomicUsize,
        /// Every `(limit_type, window_secs, limit)` the relay asked for.
        pub(crate) requests: Mutex<Vec<(LimitType, u64, u64)>>,
    }

    impl StubLimiter {
        pub(crate) fn new(outcome: StubOutcome) -> Self {
            Self {
                outcome,
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
            }
        }

        pub(crate) fn call_count(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }

        pub(crate) fn requests(&self) -> Vec<(LimitType, u64, u64)> {
            self.requests.lock().expect("requests").clone()
        }
    }

    impl RateLimiter for StubLimiter {
        async fn check_and_increment(
            &self,
            _ctx: &TenantContext,
            _pubkey: &PublicKey,
            limit_type: LimitType,
            window_secs: u64,
            limit: u64,
        ) -> Result<RateLimitResult, AuthError> {
            let ordinal = self.calls.fetch_add(1, Ordering::Relaxed) as u64 + 1;
            self.requests
                .lock()
                .expect("requests")
                .push((limit_type, window_secs, limit));
            match self.outcome {
                StubOutcome::Allowed => Ok(RateLimitResult::allowed(ordinal, limit, window_secs)),
                StubOutcome::Denied => Ok(RateLimitResult::denied(11, 10, 1)),
                StubOutcome::Failed => Err(AuthError::Internal("redis unavailable".to_owned())),
            }
        }

        async fn check_ip_connection(
            &self,
            _ip: &IpAddr,
            _window_secs: u64,
            _limit: u64,
        ) -> Result<RateLimitResult, AuthError> {
            match self.outcome {
                StubOutcome::Allowed => Ok(RateLimitResult::allowed(1, 10, 1)),
                StubOutcome::Denied => Ok(RateLimitResult::denied(11, 10, 1)),
                StubOutcome::Failed => Err(AuthError::Internal("redis unavailable".to_owned())),
            }
        }
    }

    fn tenant() -> TenantContext {
        TenantContext::resolved(
            CommunityId::from_uuid(Uuid::from_u128(1)),
            "relay.example.com",
        )
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn websocket_budget_preserves_rate_with_a_bounded_burst() {
        assert_eq!(ws_admission_budget(10), (5, 50));
        assert_eq!(ws_admission_budget(30), (5, 150));
        assert_eq!(ws_admission_budget(100), (5, 500));
    }

    #[test]
    fn websocket_budget_saturates_on_overflow() {
        assert_eq!(ws_admission_budget(u64::MAX), (5, u64::MAX));
    }

    #[test]
    fn window_budget_admits_up_to_the_limit_then_rejects() {
        let budget = WindowBudget::default();
        let t0 = Instant::now();
        for _ in 0..3 {
            assert_eq!(budget.admit(3, secs(5), t0), Ok(()));
        }
        let rejection = budget.admit(3, secs(5), t0).expect_err("4th is over");
        assert_eq!(rejection.reset_in_secs, 5);
        assert!(rejection.first_in_window, "the first refusal is flagged");
    }

    #[test]
    fn window_budget_flags_only_the_first_rejection_in_a_window() {
        let budget = WindowBudget::default();
        let t0 = Instant::now();
        assert_eq!(budget.admit(1, secs(5), t0), Ok(()));
        let first = budget.admit(1, secs(5), t0).expect_err("over");
        let second = budget.admit(1, secs(5), t0 + secs(1)).expect_err("over");
        assert!(first.first_in_window);
        assert!(!second.first_in_window, "later refusals are not re-logged");
        assert_eq!(second.reset_in_secs, 4, "retry hint counts down");
    }

    #[test]
    fn window_budget_retry_hint_is_never_zero() {
        let budget = WindowBudget::default();
        let t0 = Instant::now();
        assert_eq!(budget.admit(1, secs(5), t0), Ok(()));
        let late = budget
            .admit(1, secs(5), t0 + Duration::from_millis(4_990))
            .expect_err("over");
        assert_eq!(late.reset_in_secs, 1);
    }

    #[test]
    fn window_budget_resets_when_the_window_expires() {
        let budget = WindowBudget::default();
        let t0 = Instant::now();
        assert_eq!(budget.admit(1, secs(5), t0), Ok(()));
        assert!(budget.admit(1, secs(5), t0 + secs(4)).is_err());
        assert_eq!(
            budget.admit(1, secs(5), t0 + secs(5)),
            Ok(()),
            "a new window starts at the boundary"
        );
        assert!(
            budget.admit(1, secs(5), t0 + secs(5)).is_err(),
            "and the new window counts from zero, not from the old overflow"
        );
    }

    #[test]
    fn budget_for_maps_reads_events_and_free_frames() {
        let keys = Keys::generate();
        let req = ClientMessage::parse(
            &serde_json::json!(["REQ", "s", {"kinds": [1]}, {"kinds": [2]}]).to_string(),
        )
        .expect("REQ");
        let count =
            ClientMessage::parse(&serde_json::json!(["COUNT", "c", {"kinds": [1]}]).to_string())
                .expect("COUNT");
        let close =
            ClientMessage::parse(&serde_json::json!(["CLOSE", "s"]).to_string()).expect("CLOSE");
        let durable = EventBuilder::new(Kind::TextNote, "hi")
            .sign_with_keys(&keys)
            .expect("sign");
        let ephemeral = EventBuilder::new(Kind::Custom(KIND_TYPING_INDICATOR as u16), "")
            .sign_with_keys(&keys)
            .expect("sign");
        let auth = EventBuilder::new(Kind::Authentication, "")
            .sign_with_keys(&keys)
            .expect("sign");

        assert_eq!(budget_for(&req), Some(Budget::Reads));
        assert_eq!(budget_for(&count), Some(Budget::Reads));
        assert_eq!(budget_for(&close), None);
        assert_eq!(
            budget_for(&ClientMessage::Event(durable)),
            Some(Budget::Durable)
        );
        assert_eq!(
            budget_for(&ClientMessage::Event(ephemeral)),
            Some(Budget::Ephemeral)
        );
        assert_eq!(budget_for(&ClientMessage::Auth(auth)), None);
    }

    #[test]
    fn budget_labels_are_the_words_rejections_use() {
        assert_eq!(Budget::Reads.label(), "read");
        assert_eq!(Budget::Durable.label(), "message");
        assert_eq!(Budget::Ephemeral.label(), "ephemeral");
    }

    #[test]
    fn ephemeral_kind_limits_follow_the_consts() {
        assert_eq!(
            ephemeral_kind_limit(KIND_PRESENCE_UPDATE),
            EPHEMERAL_PRESENCE_PER_SEC
        );
        assert_eq!(
            ephemeral_kind_limit(KIND_TYPING_INDICATOR),
            EPHEMERAL_TYPING_PER_SEC
        );
        assert_eq!(ephemeral_kind_limit(24223), EPHEMERAL_OTHER_PER_SEC);
    }

    #[test]
    fn local_kind_window_admits_limit_then_refuses_then_resets() {
        let limiter: dashmap::DashMap<u8, (u32, Instant)> = dashmap::DashMap::new();
        let t0 = Instant::now();
        for _ in 0..5 {
            assert!(!local_kind_window_limited(&limiter, 1, 5, t0));
        }
        assert!(local_kind_window_limited(&limiter, 1, 5, t0));
        assert!(
            !local_kind_window_limited(&limiter, 2, 5, t0),
            "another key is unaffected"
        );
        assert!(
            !local_kind_window_limited(&limiter, 1, 5, t0 + secs(1)),
            "the window restarts after one second"
        );
    }

    #[test]
    fn connection_cap_counts_this_socket() {
        assert!(!connection_cap_exceeded(8, 8), "the 8th socket is allowed");
        assert!(connection_cap_exceeded(9, 8), "the 9th is refused");
        assert!(!connection_cap_exceeded(1, 1));
    }

    /// The per-key socket cap, driven through the real `ConnectionManager`
    /// count the auth handler consults: with eight sockets live on one key
    /// in one community the ninth is over, while a ninth in another
    /// community is not (the cap is inside the tenant fence).
    #[test]
    fn ninth_socket_on_one_key_in_one_community_is_over_the_cap() {
        use std::collections::HashMap;
        use std::sync::atomic::AtomicU8;
        use std::sync::Arc;

        let mgr = crate::state::ConnectionManager::new();
        let pubkey = Keys::generate().public_key().to_bytes().to_vec();
        let community_a = buzz_core::CommunityId::from_uuid(uuid::Uuid::from_u128(0xA));
        let community_b = buzz_core::CommunityId::from_uuid(uuid::Uuid::from_u128(0xB));
        let max = buzz_auth::RateLimitConfig::default().max_ws_connections_per_pubkey;
        assert_eq!(max, 8);

        let register = |community| {
            let id = uuid::Uuid::new_v4();
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let (ctrl_tx, _ctrl_rx) = tokio::sync::mpsc::channel(1);
            mgr.register(
                id,
                tx,
                ctrl_tx,
                None,
                tokio_util::sync::CancellationToken::new(),
                community,
                Arc::new(AtomicU8::new(0)),
                Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                3,
            );
            mgr.set_authenticated_pubkey(id, pubkey.clone());
            mgr.connection_ids_for_pubkey_in_community(community, &pubkey)
                .len()
        };

        for n in 1..=8 {
            let live = register(community_a);
            assert_eq!(live, n);
            assert!(
                !crate::admission::connection_cap_exceeded(live, max),
                "socket {n} is within the cap"
            );
        }
        let ninth = register(community_a);
        assert_eq!(ninth, 9);
        assert!(
            crate::admission::connection_cap_exceeded(ninth, max),
            "the ninth socket on one key is refused"
        );
        let in_b = register(community_b);
        assert_eq!(in_b, 1);
        assert!(
            !crate::admission::connection_cap_exceeded(in_b, max),
            "the same key in another community starts its own count"
        );
    }

    #[tokio::test]
    async fn denied_shared_counter_rejects_admission() {
        let limiter = StubLimiter::new(StubOutcome::Denied);
        let keys = Keys::generate();

        let result = check_principal(
            &limiter,
            &tenant(),
            &keys.public_key(),
            LimitType::Messages,
            60,
            10,
        )
        .await;

        assert_eq!(
            result,
            Err(AdmissionError::Exceeded {
                reset_in_secs: 1,
                first_in_window: true,
            })
        );
        assert_eq!(limiter.call_count(), 1);
    }

    #[tokio::test]
    async fn shared_counter_failure_rejects_admission() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let keys = Keys::generate();

        let result = check_principal(
            &limiter,
            &tenant(),
            &keys.public_key(),
            LimitType::ApiCalls,
            60,
            300,
        )
        .await;

        assert_eq!(result, Err(AdmissionError::Unavailable));
        assert_eq!(limiter.call_count(), 1);
    }
}
