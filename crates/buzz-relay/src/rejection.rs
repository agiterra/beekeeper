//! How a rejected client frame is addressed back to the client, and the
//! admission flow that decides whether to reject it.
//!
//! NIP-01 gives every request type its own acknowledgement channel, and a
//! rejection is only actionable if it travels on the same one: a REQ or COUNT
//! refusal settles on `CLOSED`, an EVENT on `OK`. Rejecting an EVENT with a bare
//! `NOTICE` leaves a client that tracks pending publishes by event id with
//! nothing to key on, so the send cannot fail — it can only time out.
//!
//! The admission flow, per authenticated frame:
//!
//! | frame        | local per-connection budget | shared per-key quota (Redis) |
//! |--------------|-----------------------------|------------------------------|
//! | REQ, COUNT   | `read`                      | none                         |
//! | EVENT stored | `message`                   | `Messages` 60/min (120 agent)|
//! | EVENT 2xxxx  | `ephemeral`                 | `Messages` 60/min (120 agent)|
//! | AUTH, CLOSE  | free                        | free                         |
//!
//! Every rejection reads `rate-limited: {budget} quota exceeded; retry in {n}s`;
//! the `rate-limited:` prefix and the `retry in {n}s` hint are what every
//! client parser keys on. The word between them names the quota that tripped.

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use buzz_auth::{LimitType, RateLimitConfig, RateLimiter};

use crate::admission::{self, AdmissionError, Budget, LocalRejection};
use crate::connection::{AuthState, ConnectionState};
use crate::protocol::{ClientMessage, RelayMessage};
use crate::state::AppState;

/// The refusal sent when the shared quota could not be consulted at all.
pub(crate) const SHARED_ADMISSION_UNAVAILABLE: &str = "rate-limited: shared admission unavailable";

/// What a rejected client frame is correlated back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RejectionTarget<'a> {
    /// A REQ or COUNT names the query it opened.
    Subscription(&'a str),
    /// An EVENT names the event it submitted.
    Event(nostr::EventId),
    /// No per-request correlation exists — connection-scoped notice.
    Connection,
}

/// Picks the acknowledgement channel a rejection of `msg` must travel on.
pub(crate) fn rejection_target_for(msg: &ClientMessage) -> RejectionTarget<'_> {
    match msg {
        ClientMessage::Req { sub_id, .. } | ClientMessage::Count { sub_id, .. } => {
            RejectionTarget::Subscription(sub_id.as_str())
        }
        ClientMessage::Event(event) => RejectionTarget::Event(event.id),
        _ => RejectionTarget::Connection,
    }
}

/// Renders `reason` as the rejection frame `target`'s acknowledgement channel
/// expects.
pub(crate) fn request_rejection_message(target: RejectionTarget<'_>, reason: &str) -> String {
    match target {
        RejectionTarget::Subscription(sub_id) => RelayMessage::closed(sub_id, reason),
        RejectionTarget::Event(event_id) => RelayMessage::ok(&event_id.to_hex(), false, reason),
        RejectionTarget::Connection => RelayMessage::notice(reason),
    }
}

/// The rejection text for a tripped quota. `label` is the budget's word.
pub(crate) fn quota_exceeded_message(label: &str, reset_in_secs: u64) -> String {
    format!("rate-limited: {label} quota exceeded; retry in {reset_in_secs}s")
}

/// The NIP-01 verb of `msg`, for log lines.
fn frame_name(msg: &ClientMessage) -> &'static str {
    match msg {
        ClientMessage::Event(_) => "EVENT",
        ClientMessage::Req { .. } => "REQ",
        ClientMessage::Count { .. } => "COUNT",
        ClientMessage::Close(_) => "CLOSE",
        ClientMessage::Auth(_) => "AUTH",
    }
}

fn frame_kind(msg: &ClientMessage) -> Option<u32> {
    match msg {
        ClientMessage::Event(event) => Some(buzz_core::kind::event_kind_u32(event)),
        _ => None,
    }
}

/// The first eight hex characters of `pubkey` — enough to correlate a log
/// line with a user without printing the whole key on every refusal.
fn pubkey_prefix(pubkey: &nostr::PublicKey) -> String {
    pubkey.to_hex().chars().take(8).collect()
}

/// Applies the WebSocket admission quotas to `msg`, returning whether it may be
/// handled. A rejection is addressed to the frame's own acknowledgement channel.
pub(crate) async fn enforce_ws_admission(
    msg: &ClientMessage,
    conn: &ConnectionState,
    state: &AppState,
) -> bool {
    enforce_ws_admission_with(
        msg,
        conn,
        &state.auth.config().rate_limits,
        state.admission_rate_limiter.as_ref(),
        Instant::now(),
    )
    .await
}

/// [`enforce_ws_admission`] with its collaborators injected, so the flow is
/// testable against a stub limiter and a chosen clock.
///
/// Unauthenticated frames are admitted here (the handlers refuse them on
/// their own terms). Reads never reach `limiter`, so a Redis outage rejects
/// only EVENTs.
pub(crate) async fn enforce_ws_admission_with<L: RateLimiter>(
    msg: &ClientMessage,
    conn: &ConnectionState,
    limits: &RateLimitConfig,
    limiter: &L,
    now: Instant,
) -> bool {
    let Some(budget) = admission::budget_for(msg) else {
        return true;
    };

    let (pubkey, is_agent) = {
        let auth = conn.auth_state.read().await;
        match &*auth {
            AuthState::Authenticated(ctx) => (ctx.pubkey, ctx.agent_owner_pubkey.is_some()),
            _ => return true,
        }
    };

    let (window_secs, limit) =
        admission::ws_admission_budget(admission::per_second_rate(limits, budget));
    if let Err(rejection) =
        conn.budgets
            .get(budget)
            .admit(limit, Duration::from_secs(window_secs), now)
    {
        send_local_rejection(conn, msg, &pubkey, budget, window_secs, limit, rejection);
        return false;
    }

    if budget == Budget::Reads {
        return true;
    }

    // Both EVENT budgets fall through to the shared per-key message quota:
    // ephemeral frames stay counted against it (decision 2026-09-07), so a
    // streaming terminal spends the same pool as chat.
    let message_limit = if is_agent {
        limits.agent_standard_messages_per_min
    } else {
        limits.human_messages_per_min
    };
    let message_result = admission::check_principal(
        limiter,
        &conn.tenant,
        &pubkey,
        LimitType::Messages,
        60,
        message_limit,
    )
    .await;
    send_admission_result(conn, message_result, msg, &pubkey, message_limit)
}

/// Rejects `msg` for a tripped per-connection budget, logging the first
/// refusal of each window.
fn send_local_rejection(
    conn: &ConnectionState,
    msg: &ClientMessage,
    pubkey: &nostr::PublicKey,
    budget: Budget,
    window_secs: u64,
    limit: u64,
    rejection: LocalRejection,
) {
    let label = budget.label();
    metrics::counter!(
        "buzz_admission_rejections_total",
        "transport" => "websocket",
        "reason" => "quota",
        "budget" => label,
        "scope" => "connection"
    )
    .increment(1);
    if rejection.first_in_window {
        tracing::warn!(
            conn_id = %conn.conn_id,
            pubkey = %pubkey_prefix(pubkey),
            frame = frame_name(msg),
            kind = frame_kind(msg),
            budget = label,
            scope = "connection",
            window_secs,
            limit,
            retry_in_secs = rejection.reset_in_secs,
            "admission quota exceeded"
        );
    }
    conn.send(request_rejection_message(
        rejection_target_for(msg),
        &quota_exceeded_message(label, rejection.reset_in_secs),
    ));
}

/// Forwards the shared message-quota verdict to the client, returning whether
/// the frame was admitted.
///
/// The rejection target is derived from `msg` here rather than supplied by the
/// caller: every quota check in this module must address its rejection to the
/// rejected frame's own acknowledgement channel, so there is deliberately no way
/// for a call site to name a different one.
fn send_admission_result(
    conn: &ConnectionState,
    result: Result<(), AdmissionError>,
    msg: &ClientMessage,
    pubkey: &nostr::PublicKey,
    limit: u64,
) -> bool {
    let target = rejection_target_for(msg);
    // The shared quota is the message pool whichever EVENT budget led here.
    let label = Budget::Durable.label();
    match result {
        Ok(()) => true,
        Err(AdmissionError::Exceeded {
            reset_in_secs,
            first_in_window,
        }) => {
            metrics::counter!(
                "buzz_admission_rejections_total",
                "transport" => "websocket",
                "reason" => "quota",
                "budget" => label,
                "scope" => "key"
            )
            .increment(1);
            if first_in_window {
                tracing::warn!(
                    conn_id = %conn.conn_id,
                    pubkey = %pubkey_prefix(pubkey),
                    frame = frame_name(msg),
                    kind = frame_kind(msg),
                    budget = label,
                    scope = "key",
                    window_secs = 60,
                    limit,
                    retry_in_secs = reset_in_secs,
                    "admission quota exceeded"
                );
            }
            conn.send(request_rejection_message(
                target,
                &quota_exceeded_message(label, reset_in_secs),
            ));
            false
        }
        Err(AdmissionError::Unavailable) => {
            metrics::counter!(
                "buzz_admission_rejections_total",
                "transport" => "websocket",
                "reason" => "unavailable",
                "budget" => label,
                "scope" => "key"
            )
            .increment(1);
            conn.send(request_rejection_message(
                target,
                SHARED_ADMISSION_UNAVAILABLE,
            ));
            false
        }
    }
}

/// The HTTP status and body for a bridge call the shared `api` quota refused.
/// Logs the first refusal of each window, like the WebSocket path.
pub(crate) fn http_rejection(
    error: AdmissionError,
    pubkey: &nostr::PublicKey,
    limit: u64,
) -> (StatusCode, String) {
    match error {
        AdmissionError::Exceeded {
            reset_in_secs,
            first_in_window,
        } => {
            metrics::counter!(
                "buzz_admission_rejections_total",
                "transport" => "http",
                "reason" => "quota",
                "budget" => "api",
                "scope" => "key"
            )
            .increment(1);
            if first_in_window {
                tracing::warn!(
                    pubkey = %pubkey_prefix(pubkey),
                    frame = "http",
                    budget = "api",
                    scope = "key",
                    window_secs = 60,
                    limit,
                    retry_in_secs = reset_in_secs,
                    "admission quota exceeded"
                );
            }
            (
                StatusCode::TOO_MANY_REQUESTS,
                quota_exceeded_message("api", reset_in_secs),
            )
        }
        AdmissionError::Unavailable => {
            metrics::counter!(
                "buzz_admission_rejections_total",
                "transport" => "http",
                "reason" => "unavailable",
                "budget" => "api",
                "scope" => "key"
            )
            .increment(1);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                SHARED_ADMISSION_UNAVAILABLE.to_owned(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    //! A rejected frame must be answerable on the acknowledgement channel the
    //! client is actually waiting on, and each frame must be charged to the
    //! budget the design says — and to no other.
    //!
    //! History: an over-quota EVENT used to be rejected with a bare
    //! `["NOTICE", reason]`. A NOTICE carries no event id, and desktop/mobile
    //! settle pending publishes only from an `OK` keyed by event id, so the
    //! rejection was unaddressable: the send could not fail, it could only time
    //! out (25s in Desktop, `PUBLISH_TIMEOUT_MS`) and surface as a message stuck
    //! on "Sending…". Startup quota exhaustion made it routine in the first
    //! seconds after launch.
    //!
    //! Second history: every frame used to be charged to one burst counter
    //! per (community, pubkey) in Redis, so a phone and a desktop on one key
    //! spent each other's budget at reconnect, and a Redis outage refused
    //! reads. These tests drive the production flow through
    //! `enforce_ws_admission_with` with a stub limiter that records every
    //! shared call, so "reads never touch Redis" is asserted, not assumed.

    use std::sync::Arc;

    use axum::extract::ws::Message as WsMessage;
    use buzz_auth::{AuthContext, AuthMethod};
    use buzz_core::kind::KIND_TYPING_INDICATOR;
    use nostr::{EventBuilder, Keys, Kind};
    use tokio::sync::mpsc;

    use crate::admission::tests::{StubLimiter, StubOutcome};
    use crate::connection::tests::{authenticated_state, read_frame, test_conn_with_auth};
    use crate::connection::AuthState;

    use super::*;

    fn sent_frame(rx: &mut mpsc::Receiver<WsMessage>) -> serde_json::Value {
        read_frame(rx)
    }

    fn no_frame(rx: &mut mpsc::Receiver<WsMessage>) {
        assert!(
            rx.try_recv().is_err(),
            "an admitted frame must not be answered by the admission gate"
        );
    }

    fn test_conn() -> (Arc<ConnectionState>, mpsc::Receiver<WsMessage>) {
        test_conn_with_auth(AuthState::Failed)
    }

    fn authenticated_as(keys: &Keys, agent: bool) -> AuthState {
        AuthState::Authenticated(AuthContext {
            pubkey: keys.public_key(),
            scopes: Vec::new(),
            channel_ids: None,
            auth_method: AuthMethod::Nip42,
            agent_owner_pubkey: agent.then(|| Keys::generate().public_key()),
        })
    }

    fn limits() -> RateLimitConfig {
        RateLimitConfig::default()
    }

    fn req(sub_id: &str) -> ClientMessage {
        ClientMessage::parse(&serde_json::json!(["REQ", sub_id, {"kinds": [1]}]).to_string())
            .expect("parse REQ")
    }

    fn req_with_filters(sub_id: &str, filters: usize) -> ClientMessage {
        let mut frame = vec![serde_json::json!("REQ"), serde_json::json!(sub_id)];
        frame.extend((0..filters).map(|i| serde_json::json!({"kinds": [i]})));
        ClientMessage::parse(&serde_json::Value::Array(frame).to_string()).expect("parse REQ")
    }

    fn durable_event(keys: &Keys) -> (ClientMessage, String) {
        let event = EventBuilder::new(Kind::TextNote, "hello")
            .sign_with_keys(keys)
            .expect("sign event");
        let id = event.id.to_hex();
        (ClientMessage::Event(event), id)
    }

    fn ephemeral_event(keys: &Keys) -> (ClientMessage, String) {
        let event = EventBuilder::new(Kind::Custom(KIND_TYPING_INDICATOR as u16), "")
            .sign_with_keys(keys)
            .expect("sign event");
        let id = event.id.to_hex();
        (ClientMessage::Event(event), id)
    }

    /// Parses a real EVENT frame exactly as the recv loop does, so the test is
    /// coupled to production parsing and not to a hand-built target.
    fn parsed_event_message() -> (ClientMessage, String) {
        let event = EventBuilder::new(Kind::TextNote, "hello")
            .sign_with_keys(&Keys::generate())
            .expect("sign event");
        let event_id = event.id.to_hex();
        let frame = serde_json::json!(["EVENT", event]).to_string();
        (ClientMessage::parse(&frame).expect("parse EVENT"), event_id)
    }

    fn exceeded(reset_in_secs: u64) -> Result<(), AdmissionError> {
        Err(AdmissionError::Exceeded {
            reset_in_secs,
            first_in_window: true,
        })
    }

    async fn enforce<L: RateLimiter>(
        msg: &ClientMessage,
        conn: &ConnectionState,
        limits: &RateLimitConfig,
        limiter: &L,
    ) -> bool {
        enforce_ws_admission_with(msg, conn, limits, limiter, Instant::now()).await
    }

    /// The regression: an over-quota EVENT must be rejected with
    /// `OK(event_id, false, reason)` so the client can settle the exact pending
    /// publish it belongs to. A NOTICE here reintroduces the 25s send stall.
    #[test]
    fn over_quota_event_is_rejected_with_a_correlated_ok() {
        let (conn, mut rx) = test_conn();
        let (msg, event_id) = parsed_event_message();
        let pubkey = Keys::generate().public_key();

        let admitted = send_admission_result(&conn, exceeded(7), &msg, &pubkey, 60);

        assert!(!admitted, "an over-quota frame is not admitted");
        let frame = sent_frame(&mut rx);
        assert_eq!(
            frame[0], "OK",
            "an EVENT rejection must travel on the OK channel — a NOTICE cannot \
             be correlated to a pending publish, so the send hangs until the \
             client's publish timeout instead of failing"
        );
        assert_eq!(
            frame[1], event_id,
            "the OK must name the rejected event id, which is what the client's \
             pending-publish map is keyed by"
        );
        assert_eq!(frame[2], false, "and must be an explicit rejection");
        assert_eq!(
            frame[3], "rate-limited: message quota exceeded; retry in 7s",
            "the retry hint must survive so the client can arm its gate"
        );
    }

    /// The same correlation is required when admission is unavailable rather
    /// than exceeded — both branches strand a send if they emit a NOTICE.
    #[test]
    fn event_rejected_for_unavailable_admission_is_also_correlated() {
        let (conn, mut rx) = test_conn();
        let (msg, event_id) = parsed_event_message();
        let pubkey = Keys::generate().public_key();

        send_admission_result(&conn, Err(AdmissionError::Unavailable), &msg, &pubkey, 60);

        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "OK");
        assert_eq!(frame[1], event_id);
        assert_eq!(frame[2], false);
        assert_eq!(frame[3], SHARED_ADMISSION_UNAVAILABLE);
    }

    /// A REQ still settles on CLOSED, which carries the subscription id. This
    /// pins the pre-existing behavior the split must not disturb.
    #[test]
    fn over_quota_req_still_closes_the_subscription() {
        let (conn, mut rx) = test_conn();
        let msg = req("history-abc");
        let pubkey = Keys::generate().public_key();

        send_local_rejection(
            &conn,
            &msg,
            &pubkey,
            Budget::Reads,
            5,
            150,
            LocalRejection {
                reset_in_secs: 7,
                first_in_window: true,
            },
        );

        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "CLOSED");
        assert_eq!(
            frame[1], "history-abc",
            "a REQ rejection must name the subscription it rejected"
        );
        assert_eq!(frame[2], "rate-limited: read quota exceeded; retry in 7s");
    }

    /// NIP-45 uses `CLOSED(query_id, reason)` when a relay refuses a COUNT.
    #[test]
    fn over_quota_count_closes_the_query() {
        let (conn, mut rx) = test_conn();
        let raw = serde_json::json!(["COUNT", "count-abc", {"kinds": [1]}]).to_string();
        let msg = ClientMessage::parse(&raw).expect("parse COUNT");
        let pubkey = Keys::generate().public_key();

        send_local_rejection(
            &conn,
            &msg,
            &pubkey,
            Budget::Reads,
            5,
            150,
            LocalRejection {
                reset_in_secs: 7,
                first_in_window: false,
            },
        );

        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "CLOSED");
        assert_eq!(frame[1], "count-abc");
        assert_eq!(frame[2], "rate-limited: read quota exceeded; retry in 7s");
    }

    /// The HTTP bridge names its own budget and keeps the same grammar.
    #[test]
    fn http_rejection_names_the_api_budget() {
        let pubkey = Keys::generate().public_key();
        let (status, message) = http_rejection(
            AdmissionError::Exceeded {
                reset_in_secs: 12,
                first_in_window: true,
            },
            &pubkey,
            300,
        );
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(message, "rate-limited: api quota exceeded; retry in 12s");

        let (status, message) = http_rejection(AdmissionError::Unavailable, &pubkey, 300);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(message, SHARED_ADMISSION_UNAVAILABLE);
    }

    /// Unauthenticated frames are not the admission gate's to refuse.
    #[tokio::test]
    async fn unauthenticated_frames_are_admitted_without_charge() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let (conn, mut rx) = test_conn();
        let (event, _) = durable_event(&Keys::generate());

        assert!(enforce(&req("s"), &conn, &limits(), &limiter).await);
        assert!(enforce(&event, &conn, &limits(), &limiter).await);
        no_frame(&mut rx);
        assert_eq!(limiter.call_count(), 0);
    }

    /// Reads never consult the shared limiter, so a Redis outage — which
    /// used to refuse every REQ — no longer touches them.
    #[tokio::test]
    async fn req_is_admitted_locally_even_when_shared_admission_is_down() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let (conn, mut rx) = test_conn_with_auth(authenticated_state());

        assert!(enforce(&req("history-abc"), &conn, &limits(), &limiter).await);
        let raw = serde_json::json!(["COUNT", "count-abc", {"kinds": [1]}]).to_string();
        let count = ClientMessage::parse(&raw).expect("parse COUNT");
        assert!(enforce(&count, &conn, &limits(), &limiter).await);

        no_frame(&mut rx);
        assert_eq!(
            limiter.call_count(),
            0,
            "a read must never reach the shared limiter"
        );
    }

    /// Exhausting the local read budget (150 per 5 s by default) is the only
    /// way a REQ is refused, and the refusal settles on CLOSED with the
    /// `read` label.
    #[tokio::test]
    async fn the_151st_req_in_a_window_is_closed_with_the_read_label() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let (conn, mut rx) = test_conn_with_auth(authenticated_state());
        let t0 = Instant::now();

        for i in 0..150 {
            let msg = req(&format!("sub-{i}"));
            assert!(
                enforce_ws_admission_with(&msg, &conn, &limits(), &limiter, t0).await,
                "REQ {i} is within the read budget"
            );
        }
        no_frame(&mut rx);

        let msg = req("history-abc");
        assert!(!enforce_ws_admission_with(&msg, &conn, &limits(), &limiter, t0).await);
        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "CLOSED");
        assert_eq!(frame[1], "history-abc");
        assert_eq!(frame[2], "rate-limited: read quota exceeded; retry in 5s");
        assert_eq!(limiter.call_count(), 0);

        // COUNT shares the read budget.
        let raw = serde_json::json!(["COUNT", "count-abc", {"kinds": [1]}]).to_string();
        let count = ClientMessage::parse(&raw).expect("parse COUNT");
        assert!(!enforce_ws_admission_with(&count, &conn, &limits(), &limiter, t0).await);
        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "CLOSED");
        assert_eq!(frame[1], "count-abc");

        // And the window passes.
        let later = t0 + Duration::from_secs(5);
        assert!(enforce_ws_admission_with(&req("again"), &conn, &limits(), &limiter, later).await);
    }

    /// The regression guard for the whole change: two sockets on one key
    /// no longer share a burst. Exhausting A's reads leaves B untouched.
    #[tokio::test]
    async fn two_connections_on_one_pubkey_have_independent_read_budgets() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let keys = Keys::generate();
        let (conn_a, mut rx_a) = test_conn_with_auth(authenticated_as(&keys, false));
        let (conn_b, mut rx_b) = test_conn_with_auth(authenticated_as(&keys, false));
        let t0 = Instant::now();

        for i in 0..150 {
            let msg = req(&format!("a-{i}"));
            assert!(enforce_ws_admission_with(&msg, &conn_a, &limits(), &limiter, t0).await);
        }
        assert!(!enforce_ws_admission_with(&req("a-over"), &conn_a, &limits(), &limiter, t0).await);
        assert_eq!(sent_frame(&mut rx_a)[0], "CLOSED");

        assert!(
            enforce_ws_admission_with(&req("b-1"), &conn_b, &limits(), &limiter, t0).await,
            "B's first REQ is admitted although A, on the same key, is exhausted"
        );
        no_frame(&mut rx_b);
    }

    /// A REQ carrying ten filters costs one unit, which is what makes
    /// bundling on the client side pay.
    #[tokio::test]
    async fn a_req_with_ten_filters_costs_one_read() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let (conn, mut rx) = test_conn_with_auth(authenticated_state());
        let mut limits = limits();
        limits.ws_reads_per_sec = 1; // budget of 5 per window
        let t0 = Instant::now();

        for i in 0..5 {
            let msg = req_with_filters(&format!("bundle-{i}"), 10);
            assert!(enforce_ws_admission_with(&msg, &conn, &limits, &limiter, t0).await);
        }
        no_frame(&mut rx);
        assert!(!enforce_ws_admission_with(&req("sixth"), &conn, &limits, &limiter, t0).await);
        assert_eq!(sent_frame(&mut rx)[0], "CLOSED");
    }

    /// A durable EVENT makes exactly one shared call — `Messages`, 60 s,
    /// the human limit — and none against the old burst key.
    #[tokio::test]
    async fn durable_event_makes_one_shared_messages_call() {
        let limiter = StubLimiter::new(StubOutcome::Allowed);
        let keys = Keys::generate();
        let (conn, mut rx) = test_conn_with_auth(authenticated_as(&keys, false));
        let (event, _) = durable_event(&keys);

        assert!(enforce(&event, &conn, &limits(), &limiter).await);
        no_frame(&mut rx);
        assert_eq!(limiter.requests(), vec![(LimitType::Messages, 60, 60)]);
    }

    /// An agent login is charged to the agent tier of the same quota.
    #[tokio::test]
    async fn agent_durable_event_is_charged_to_the_agent_message_limit() {
        let limiter = StubLimiter::new(StubOutcome::Allowed);
        let keys = Keys::generate();
        let (conn, _rx) = test_conn_with_auth(authenticated_as(&keys, true));
        let (event, _) = durable_event(&keys);

        assert!(enforce(&event, &conn, &limits(), &limiter).await);
        assert_eq!(limiter.requests(), vec![(LimitType::Messages, 60, 120)]);
    }

    /// Ephemeral EVENTs stay counted against the shared message quota — the
    /// decision of 2026-09-07 — after their own local burst.
    #[tokio::test]
    async fn ephemeral_event_is_also_charged_to_the_shared_message_quota() {
        let limiter = StubLimiter::new(StubOutcome::Allowed);
        let keys = Keys::generate();
        let (conn, _rx) = test_conn_with_auth(authenticated_as(&keys, false));
        let (event, _) = ephemeral_event(&keys);

        assert!(enforce(&event, &conn, &limits(), &limiter).await);
        assert_eq!(limiter.requests(), vec![(LimitType::Messages, 60, 60)]);
    }

    /// The shared verdict is forwarded on the OK channel with its own
    /// retry hint and the `message` label.
    #[tokio::test]
    async fn shared_message_quota_refusal_is_forwarded_on_the_ok_channel() {
        let limiter = StubLimiter::new(StubOutcome::Denied);
        let keys = Keys::generate();
        let (conn, mut rx) = test_conn_with_auth(authenticated_as(&keys, false));
        let (event, event_id) = durable_event(&keys);

        assert!(!enforce(&event, &conn, &limits(), &limiter).await);
        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "OK");
        assert_eq!(frame[1], event_id);
        assert_eq!(frame[2], false);
        assert_eq!(
            frame[3],
            "rate-limited: message quota exceeded; retry in 1s"
        );
    }

    /// The local durable burst is checked before the shared quota: the 51st
    /// EVENT in a window is refused without a Redis round trip.
    #[tokio::test]
    async fn local_durable_burst_is_checked_before_the_shared_quota() {
        let limiter = StubLimiter::new(StubOutcome::Allowed);
        let keys = Keys::generate();
        let (conn, mut rx) = test_conn_with_auth(authenticated_as(&keys, false));
        let t0 = Instant::now();

        for _ in 0..50 {
            let (event, _) = durable_event(&keys);
            assert!(enforce_ws_admission_with(&event, &conn, &limits(), &limiter, t0).await);
        }
        assert_eq!(limiter.call_count(), 50);

        let (event, event_id) = durable_event(&keys);
        assert!(!enforce_ws_admission_with(&event, &conn, &limits(), &limiter, t0).await);
        assert_eq!(limiter.call_count(), 50, "the 51st never reached Redis");
        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "OK");
        assert_eq!(frame[1], event_id);
        assert_eq!(
            frame[3],
            "rate-limited: message quota exceeded; retry in 5s"
        );
    }

    /// Ephemeral and durable bursts are separate pools on one connection.
    #[tokio::test]
    async fn ephemeral_burst_does_not_spend_the_durable_burst() {
        let limiter = StubLimiter::new(StubOutcome::Allowed);
        let keys = Keys::generate();
        let (conn, mut rx) = test_conn_with_auth(authenticated_as(&keys, false));
        let mut limits = limits();
        limits.ws_ephemeral_events_per_sec = 1; // 5 per window
        let t0 = Instant::now();

        for _ in 0..5 {
            let (event, _) = ephemeral_event(&keys);
            assert!(enforce_ws_admission_with(&event, &conn, &limits, &limiter, t0).await);
        }
        let (event, _) = ephemeral_event(&keys);
        assert!(!enforce_ws_admission_with(&event, &conn, &limits, &limiter, t0).await);
        assert_eq!(
            sent_frame(&mut rx)[3],
            "rate-limited: ephemeral quota exceeded; retry in 5s"
        );

        let (event, _) = durable_event(&keys);
        assert!(
            enforce_ws_admission_with(&event, &conn, &limits, &limiter, t0).await,
            "a chat message still goes out while typing indicators are throttled"
        );
    }

    /// A Redis outage now refuses only EVENTs, on their OK channel.
    #[tokio::test]
    async fn event_is_refused_on_the_ok_channel_when_shared_admission_is_down() {
        let limiter = StubLimiter::new(StubOutcome::Failed);
        let keys = Keys::generate();
        let (conn, mut rx) = test_conn_with_auth(authenticated_as(&keys, false));
        let (event, event_id) = durable_event(&keys);

        assert!(!enforce(&event, &conn, &limits(), &limiter).await);
        let frame = sent_frame(&mut rx);
        assert_eq!(frame[0], "OK");
        assert_eq!(frame[1], event_id);
        assert_eq!(frame[2], false);
        assert_eq!(frame[3], SHARED_ADMISSION_UNAVAILABLE);
    }
}
