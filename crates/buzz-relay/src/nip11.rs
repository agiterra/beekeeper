//! NIP-11 relay information document.

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::config::DEFAULT_MAX_FRAME_BYTES;

/// NIPs unconditionally supported by this relay, advertised in the NIP-11
/// document. Kept as a module-level constant so tests can verify it without
/// constructing a full `Config` (which reads env vars and races with
/// config.rs tests).
///
/// NIP-43 (relay membership) is advertised separately by [`RelayInfo::build`]
/// only when membership enforcement is actually enabled — see that function.
pub(crate) const SUPPORTED_NIPS: &[u32] = &[1, 2, 10, 11, 16, 17, 23, 25, 29, 33, 38, 42, 50, 56];

/// NIP-43 (relay membership). Advertised only when the relay actually
/// enforces membership (`BUZZ_REQUIRE_RELAY_MEMBERSHIP=true`) AND has a
/// stable signing key — both are required for kind 13534/8000/8001 events
/// to be verifiable by clients.
pub(crate) const NIP_RELAY_MEMBERSHIP: u32 = 43;

/// Relay information document served at `GET /` with `Accept: application/nostr+json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInfo {
    /// Human-readable relay name.
    pub name: String,
    /// Human-readable relay description.
    pub description: String,
    /// Workspace icon URL (NIP-11 `icon`), per-community, set by relay
    /// admins/owners via the kind:9033 command. Omitted when no icon is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Relay operator's public key (hex), if published.
    pub pubkey: Option<String>,
    /// Contact address for the relay operator.
    pub contact: Option<String>,
    /// NIPs supported by this relay.
    pub supported_nips: Vec<u32>,
    /// Draft/extension protocol identifiers supported by this relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_extensions: Option<Vec<String>>,
    /// NIP-PL executor descriptor. Present only when push delivery is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<serde_json::Value>,
    /// URL of the relay software repository.
    pub software: String,
    /// Relay software version string.
    pub version: String,
    /// Protocol and resource limits advertised to clients.
    pub limitation: Option<RelayLimitation>,
    /// Public WebSocket URL of the dedicated NIP-AB device-pairing relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing_relay_url: Option<String>,
    /// Relay's own signing pubkey (NIP-11 `self` field, NIP-43).
    #[serde(rename = "self", skip_serializing_if = "Option::is_none")]
    pub relay_self: Option<String>,
    /// Full 40-hex git commit this binary was built from, or `unknown` when
    /// none could be determined at compile time. Additive field (finding 32,
    /// `review-2026-09-01/LIVE-RUN-TeamRolesV1.md`): whether a push had
    /// redeployed hive could not be observed by mechanism before this. See
    /// `build_info::source_sha` and `docs/INTEGRATION.md` § NIP-11.
    pub software_commit: String,
    /// `git rev-list --count` of [`Self::software_commit`] — the number of
    /// commits reachable from it, inclusive — or `null`.
    ///
    /// Always describes the *same* commit as `software_commit`: `build.rs`
    /// resolves the pair from one source or yields no count, so the two can
    /// never come from different histories.
    ///
    /// `null` is the disclosed non-answer, exactly as `unknown` is for
    /// `software_commit`: never omitted from the document, never guessed,
    /// and never `0` — `rev-list --count` of a real commit is at least 1, so
    /// a `0` could only come from a broken pipeline, and unlike `null` it
    /// would silently take part in a consumer's subtraction.
    ///
    /// A count is comparable **only** against a count from the same linear
    /// history, and a difference of two counts is a distance only when one
    /// commit is an ancestor of the other. See `docs/INTEGRATION.md` § NIP-11.
    #[serde(default)]
    pub software_commit_count: Option<u32>,
    /// RFC 3339 UTC timestamp this binary was compiled, or `unknown`.
    /// See `build_info::build_time`.
    pub build_time: String,
}

/// Protocol and resource limits advertised in the NIP-11 document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayLimitation {
    /// Maximum WebSocket frame size in bytes.
    pub max_message_length: Option<u64>,
    /// Maximum number of concurrent subscriptions per connection.
    pub max_subscriptions: Option<u32>,
    /// Maximum number of filters per subscription.
    pub max_filters: Option<u32>,
    /// Maximum value of the `limit` field in a filter.
    pub max_limit: Option<u32>,
    /// Maximum length of a subscription ID string.
    pub max_subid_length: Option<u32>,
    /// Minimum proof-of-work difficulty required for events.
    pub min_pow_difficulty: Option<u32>,
    /// Whether NIP-42 authentication is required before subscribing or
    /// publishing events.
    pub auth_required: bool,
    /// Whether payment is required to use the relay.
    pub payment_required: bool,
    /// Whether writes are restricted to authorized pubkeys.
    pub restricted_writes: bool,
    /// NIP-ER: how the relay delivers due reminders ("push" or "lazy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_delivery_mode: Option<String>,
    /// NIP-ER: maximum allowed `not_before` horizon in seconds from now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_not_before_delta: Option<u64>,
    /// The admission limits this relay enforces, so a client can pace itself
    /// instead of discovering them from `rate-limited:` refusals. Absent only
    /// in documents from relays that predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limits: Option<RelayRateLimits>,
}

/// The admission limits advertised under `limitation.rate_limits`.
///
/// Every value is the number the relay actually enforces: the
/// per-connection figures are the configured per-second rates multiplied
/// by the burst window, the per-key figures are the shared Redis quotas,
/// and the per-kind ceilings are the relay's own constants. Built by
/// [`Self::from_config`] from the live configuration — never retyped — so
/// the document cannot drift from enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayRateLimits {
    /// Length of the per-connection burst windows, in seconds.
    pub window_secs: u64,
    /// REQ + COUNT frames admitted per connection per window. A REQ carrying
    /// several filters costs one.
    pub reads_per_connection: u64,
    /// Durable (stored) EVENTs admitted per connection per window, before
    /// the shared per-key quota.
    pub messages_per_connection: u64,
    /// Ephemeral EVENTs (kinds 20000–29999) admitted per connection per
    /// window, before the shared per-key quota.
    pub ephemeral_per_connection: u64,
    /// Durable **and** ephemeral EVENTs admitted per (community, pubkey) per
    /// minute, shared across every connection the key holds.
    pub messages_per_key_per_min: u64,
    /// The same quota for agent logins.
    pub agent_messages_per_key_per_min: u64,
    /// HTTP bridge calls (`/events`, `/query`, `/count`) admitted per
    /// (community, pubkey) per minute — a separate pool from the WebSocket.
    pub api_calls_per_key_per_min: u64,
    /// Concurrent WebSocket connections one pubkey may hold in one community.
    pub max_connections_per_key: u64,
    /// Presence updates (kind 20001) admitted per pubkey per second.
    pub presence_per_key_per_sec: u32,
    /// Typing indicators (kind 20002) admitted per pubkey per second.
    pub typing_per_key_per_sec: u32,
    /// Any other generic ephemeral kind admitted per pubkey per second.
    pub ephemeral_kind_per_key_per_sec: u32,
}

impl RelayRateLimits {
    /// Derives the advertised figures from the enforced configuration and
    /// the per-kind constants in `crate::admission`.
    pub fn from_config(limits: &buzz_auth::RateLimitConfig) -> Self {
        use crate::admission::{ws_admission_budget, Budget};
        let (window_secs, reads_per_connection) =
            ws_admission_budget(crate::admission::per_second_rate(limits, Budget::Reads));
        let (_, messages_per_connection) =
            ws_admission_budget(crate::admission::per_second_rate(limits, Budget::Durable));
        let (_, ephemeral_per_connection) =
            ws_admission_budget(crate::admission::per_second_rate(limits, Budget::Ephemeral));
        Self {
            window_secs,
            reads_per_connection,
            messages_per_connection,
            ephemeral_per_connection,
            messages_per_key_per_min: limits.human_messages_per_min,
            agent_messages_per_key_per_min: limits.agent_standard_messages_per_min,
            api_calls_per_key_per_min: limits.human_api_calls_per_min,
            max_connections_per_key: limits.max_ws_connections_per_pubkey,
            presence_per_key_per_sec: crate::admission::EPHEMERAL_PRESENCE_PER_SEC,
            typing_per_key_per_sec: crate::admission::EPHEMERAL_TYPING_PER_SEC,
            ephemeral_kind_per_key_per_sec: crate::admission::EPHEMERAL_OTHER_PER_SEC,
        }
    }
}

/// Canonical `RelayLimitation` advertised by this relay.
///
/// `max_limit` is [`buzz_db::DEFAULT_MAX_PAGE_LIMIT`], the same constant the
/// REQ path clamps filter limits to, so the advertised ceiling and the
/// enforced one cannot drift (see
/// `handlers::req::tests::req_filter_limit_clamps_to_advertised_nip11_max_limit`).
///
/// `auth_required` is always `true`: the REQ, EVENT, and COUNT handlers
/// unconditionally reject connections that are not in
/// `AuthState::Authenticated`. This is independent of the REST API token
/// toggle (`config.require_auth_token`).
fn relay_limitation(max_message_length: usize, rate_limits: RelayRateLimits) -> RelayLimitation {
    let max_not_before_delta: u64 = std::env::var("SPROUT_MAX_NOT_BEFORE_DELTA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(31_536_000); // 1 year default

    RelayLimitation {
        max_message_length: Some(max_message_length as u64),
        max_subscriptions: Some(1024),
        max_filters: Some(10),
        max_limit: Some(buzz_db::DEFAULT_MAX_PAGE_LIMIT as u32),
        max_subid_length: Some(256),
        min_pow_difficulty: None,
        auth_required: true,
        payment_required: false,
        restricted_writes: true,
        due_delivery_mode: Some("push".to_string()),
        max_not_before_delta: Some(max_not_before_delta),
        rate_limits: Some(rate_limits),
    }
}

impl RelayInfo {
    /// Builds the relay's NIP-11 information document.
    ///
    /// `relay_self` is the relay's own signing pubkey (hex), advertised as the
    /// NIP-11 `self` field. NIP-11 defines `self` generically as the relay's
    /// identity key; other NIPs reference it. Notably NIP-29 (group metadata
    /// kinds 39000/39001/39002, which Buzz signs with `state.relay_keypair`
    /// unconditionally) requires clients to verify those events against
    /// `self`. Pass `Some` whenever the relay has a stable signing key.
    ///
    /// `icon` is the community's workspace icon (see
    /// [`workspace_icon_for_host`]) — a host-scoped scalar, pre-fetched by
    /// the caller so `build` itself stays static-input.
    ///
    /// `advertise_nip43` controls whether NIP-43 (relay membership) is added
    /// to `supported_nips`. Set `true` only when the relay actually emits and
    /// gates on NIP-43 events — i.e. has a stable key AND enforces
    /// membership. NIP-43 events are verified against `self`, so it is a
    /// programmer error to advertise NIP-43 without a `relay_self`.
    ///
    /// `rate_limits` is the enforced admission configuration, pre-derived
    /// by [`RelayRateLimits::from_config`] — a config scalar, not a lookup.
    pub fn build(
        relay_self: Option<&str>,
        icon: Option<&str>,
        advertise_nip43: bool,
        max_message_length: usize,
        pairing_relay_url: Option<&str>,
        rate_limits: RelayRateLimits,
    ) -> Self {
        debug_assert!(
            !advertise_nip43 || relay_self.is_some(),
            "advertise_nip43=true requires relay_self=Some — NIP-43 events are verified against `self`"
        );

        let mut supported_nips = SUPPORTED_NIPS.to_vec();
        if advertise_nip43 {
            supported_nips.push(NIP_RELAY_MEMBERSHIP);
        }

        Self {
            name: "Beekeeper Relay".to_string(),
            description: "Beekeeper — private team communication relay".to_string(),
            icon: icon.filter(|s| !s.is_empty()).map(|s| s.to_string()),
            pubkey: None,
            contact: None,
            supported_nips,
            supported_extensions: Some(vec!["nip-er".to_string()]),
            push: None,
            software: "https://github.com/agiterra/beekeeper".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            limitation: Some(relay_limitation(max_message_length, rate_limits)),
            pairing_relay_url: pairing_relay_url.map(str::to_string),
            relay_self: relay_self.map(|s| s.to_string()),
            software_commit: crate::build_info::source_sha().to_string(),
            software_commit_count: crate::build_info::source_commit_count(),
            build_time: crate::build_info::build_time().to_string(),
        }
    }
}

/// Axum handler that returns the NIP-11 relay information document as JSON.
pub async fn relay_info_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Json<RelayInfo> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    axum::response::Json(nip11_document(&state, raw_host).await)
}

fn push_descriptor(
    push_configured: bool,
    relay_url: &str,
    executor_key_id: &str,
    relay_keypair: &nostr::Keys,
    tenant_host: Option<&str>,
) -> Option<serde_json::Value> {
    let host = tenant_host?;
    push_configured.then_some(())?;
    let scheme = if relay_url.starts_with("wss://") {
        "wss"
    } else {
        "ws"
    };
    Some(serde_json::json!({
        "origin": format!("{scheme}://{host}"),
        "keys": [{
            "id": executor_key_id,
            "pubkey": relay_keypair.public_key().to_hex(),
            "current": true
        }],
        "app_profiles": [
            {"id": "buzz-ios-production", "transport": "apns"},
            {"id": "buzz-ios-sandbox", "transport": "apns"}
        ],
        "push_kinds": crate::handlers::push_lease::PUSH_KINDS,
        "urgent_kinds": crate::handlers::push_lease::URGENT_KINDS,
        "h_grammar": "uuid-v4-lowercase",
        "class_support": {"apns": ["silent", "default", "time_sensitive"]},
        "limitation": {
            "max_lease_ttl": 2592000,
            "max_leases_per_pubkey": 16,
            "max_subscriptions_per_lease": 16,
            "max_kinds": 16,
            "max_authors": 20,
            "max_h": 50,
            "max_tag_values": 20,
            "max_ignore": 8,
            "max_content_len": 65536,
            "max_plaintext_len": 32768,
            "max_endpoint_len": 4096,
            "max_string_len": 512
        }
    }))
}

/// Builds the served NIP-11 document for a request arriving on `raw_host`.
///
/// Centralised so the content-negotiated root handler and the dedicated
/// `/info` endpoint can't drift apart. Every input to `RelayInfo::build`
/// stays a pre-derived scalar: [`nip11_facts`] (config + keypair), the
/// enforced rate limits ([`RelayRateLimits::from_config`], config only), plus
/// the host-scoped workspace icon.
pub(crate) async fn nip11_document(state: &crate::state::AppState, raw_host: &str) -> RelayInfo {
    let (relay_self, advertise_nip43) = nip11_facts(state);
    let icon = workspace_icon_for_host(state, raw_host).await;
    let mut info = RelayInfo::build(
        relay_self.as_deref(),
        icon.as_deref(),
        advertise_nip43,
        state.config.max_frame_bytes,
        state.config.pairing_relay_url.as_deref(),
        RelayRateLimits::from_config(&state.auth.config().rate_limits),
    );
    let tenant_host = if state.config.push_gateway_delivery_url.is_some() {
        crate::tenant::bind_community(&state.db, raw_host)
            .await
            .ok()
            .map(|tenant| tenant.host().to_owned())
    } else {
        None
    };
    if let Some(push) = push_descriptor(
        state.config.push_gateway_delivery_url.is_some(),
        &state.config.relay_url,
        &state.config.push_executor_key_id,
        &state.relay_keypair,
        tenant_host.as_deref(),
    ) {
        info.supported_extensions
            .get_or_insert_default()
            .push("nip-pl".to_string());
        info.push = Some(push);
    }
    info
}

/// Fetches the workspace icon for the community bound to `raw_host`, as the
/// host-scoped scalar consumed by [`RelayInfo::build`].
///
/// The icon is per-community state (`communities.icon`, set by relay
/// admins/owners via the kind:9033 command) served in the standard NIP-11
/// `icon` field. The lookup is scoped through
/// [`crate::tenant::bind_community`] — never an unscoped query. Fails open to
/// `None` (no `icon` field): NIP-11 is intentionally served to unmapped hosts
/// too, and an icon lookup failure must not break that.
async fn workspace_icon_for_host(state: &crate::state::AppState, raw_host: &str) -> Option<String> {
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .ok()?;
    state
        .db
        .get_community_icon(tenant.community())
        .await
        .ok()
        .flatten()
}

/// Derives the two NIP-11 facts that depend on runtime config:
///
/// - `relay_self`: the NIP-11 `self` pubkey, set whenever the relay has a
///   stable signing key. Consumed by NIP-29 (group metadata verification)
///   and NIP-43, among others. Ephemeral keys are excluded because they
///   change on restart, leaving previously-signed events unverifiable.
/// - `advertise_nip43`: whether to list NIP-43 in `supported_nips`. True
///   only when membership is actually enforced AND we have a stable key
///   (NIP-43 events must be verifiable against `self`).
///
/// Centralised so the content-negotiated root handler and the dedicated
/// `/info` endpoint can't drift apart.
pub(crate) fn nip11_facts(state: &crate::state::AppState) -> (Option<String>, bool) {
    let has_stable_key = state.config.relay_private_key.is_some();
    let relay_self = has_stable_key.then(|| state.relay_keypair.public_key().to_hex());
    let advertise_nip43 = has_stable_key && state.config.require_relay_membership;
    (relay_self, advertise_nip43)
}

/// Multi-tenant conformance static-input fence (surface row "NIP-11 relay info
/// and relay `self`").
///
/// The conformance obligation: `RelayInfo::build` "must not grow unscoped
/// DB/search/audit inputs", so an unauthenticated NIP-11 read can never become
/// an enumeration oracle for *other* communities. `build` takes only static
/// and scalar inputs — the per-deployment facts arrive pre-derived through
/// [`nip11_facts`] (config + relay keypair), and the one host-scoped fact
/// (the workspace `icon`) arrives as a scalar from
/// [`workspace_icon_for_host`], whose DB lookup is scoped through
/// [`crate::tenant::bind_community`] and can therefore only ever surface the
/// requesting host's own community state. `RelayRateLimits` is a `Copy`
/// struct of numbers derived from the process configuration — the same for
/// every host, so it can name no community.
///
/// This const binds `RelayInfo::build` to its **exact** allowed signature. The
/// moment someone adds a `&Db`, `&AppState`, a search handle, an audit handle,
/// or any other unscoped input, the function pointer's type stops matching and
/// **this file fails to compile** — turning a silent cross-tenant leak into a
/// hard build break, the same way a deny-lint would. If you must change this
/// signature, you are changing the conformance contract: update the conformance
/// doc and prove the new input is host-scoped, not unscoped, first.
#[allow(clippy::type_complexity)]
const _RELAY_INFO_BUILD_STATIC_INPUT_FENCE: fn(
    Option<&str>,
    Option<&str>,
    bool,
    usize,
    Option<&str>,
    RelayRateLimits,
) -> RelayInfo = RelayInfo::build;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rate_limits() -> RelayRateLimits {
        RelayRateLimits::from_config(&buzz_auth::RateLimitConfig::default())
    }

    /// The advertised figures are the enforced ones: config × burst window
    /// for the per-connection budgets, the per-kind consts for the ceilings.
    #[test]
    fn advertised_rate_limits_equal_the_enforced_config_and_consts() {
        let config = buzz_auth::RateLimitConfig {
            ws_reads_per_sec: 7,
            human_ws_events_per_sec: 3,
            ws_ephemeral_events_per_sec: 11,
            human_messages_per_min: 61,
            agent_standard_messages_per_min: 121,
            human_api_calls_per_min: 301,
            max_ws_connections_per_pubkey: 9,
            ..buzz_auth::RateLimitConfig::default()
        };

        let limits = RelayRateLimits::from_config(&config);
        assert_eq!(limits.window_secs, crate::admission::WS_BURST_WINDOW_SECS);
        assert_eq!(
            limits.reads_per_connection,
            7 * crate::admission::WS_BURST_WINDOW_SECS
        );
        assert_eq!(
            limits.messages_per_connection,
            3 * crate::admission::WS_BURST_WINDOW_SECS
        );
        assert_eq!(
            limits.ephemeral_per_connection,
            11 * crate::admission::WS_BURST_WINDOW_SECS
        );
        assert_eq!(limits.messages_per_key_per_min, 61);
        assert_eq!(limits.agent_messages_per_key_per_min, 121);
        assert_eq!(limits.api_calls_per_key_per_min, 301);
        assert_eq!(limits.max_connections_per_key, 9);
        assert_eq!(
            limits.presence_per_key_per_sec,
            crate::admission::EPHEMERAL_PRESENCE_PER_SEC
        );
        assert_eq!(
            limits.typing_per_key_per_sec,
            crate::admission::EPHEMERAL_TYPING_PER_SEC
        );
        assert_eq!(
            limits.ephemeral_kind_per_key_per_sec,
            crate::admission::EPHEMERAL_OTHER_PER_SEC
        );

        // Defaults, as the document will read on an unconfigured relay.
        let defaults = test_rate_limits();
        assert_eq!(defaults.reads_per_connection, 150);
        assert_eq!(defaults.messages_per_connection, 50);
        assert_eq!(defaults.ephemeral_per_connection, 500);
        assert_eq!(defaults.messages_per_key_per_min, 60);
        assert_eq!(defaults.max_connections_per_key, 8);
    }

    /// The field ships under `limitation.rate_limits` with stable names.
    #[test]
    fn rate_limits_are_serialized_under_limitation() {
        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        let json = serde_json::to_value(&info).expect("serialize");
        let limits = &json["limitation"]["rate_limits"];
        assert_eq!(limits["window_secs"], 5);
        assert_eq!(limits["reads_per_connection"], 150);
        assert_eq!(limits["messages_per_connection"], 50);
        assert_eq!(limits["ephemeral_per_connection"], 500);
        assert_eq!(limits["messages_per_key_per_min"], 60);
        assert_eq!(limits["agent_messages_per_key_per_min"], 120);
        assert_eq!(limits["api_calls_per_key_per_min"], 300);
        assert_eq!(limits["max_connections_per_key"], 8);
        assert_eq!(limits["presence_per_key_per_sec"], 5);
        assert_eq!(limits["typing_per_key_per_sec"], 5);
        assert_eq!(limits["ephemeral_kind_per_key_per_sec"], 10);
    }

    #[test]
    fn push_descriptor_is_gated_by_gateway_configuration_and_tenant_binding() {
        let keys = nostr::Keys::generate();
        assert!(
            push_descriptor(false, "ws://relay", "key", &keys, Some("tenant.example")).is_none()
        );
        assert!(push_descriptor(true, "ws://relay", "key", &keys, None).is_none());
        let descriptor = push_descriptor(true, "ws://relay", "key", &keys, Some("tenant.example"))
            .expect("configured push descriptor");
        assert_eq!(descriptor["origin"], "ws://tenant.example");
        assert_eq!(
            descriptor["push_kinds"],
            serde_json::json!(crate::handlers::push_lease::PUSH_KINDS)
        );
    }

    #[test]
    fn supported_nips_includes_nip23_and_nip33() {
        // Tests the production SUPPORTED_NIPS constant directly — no Config::from_env()
        // needed, avoiding the env-var race with config.rs tests.
        assert!(
            SUPPORTED_NIPS.contains(&23),
            "NIP-23 (long-form content) must be advertised"
        );
        assert!(
            SUPPORTED_NIPS.contains(&33),
            "NIP-33 (parameterized replaceable) must be advertised"
        );
    }

    #[test]
    fn supported_nips_includes_nip38() {
        assert!(
            SUPPORTED_NIPS.contains(&38),
            "NIP-38 (user statuses) must be advertised"
        );
    }

    #[test]
    fn supported_nips_includes_nip56() {
        assert!(
            SUPPORTED_NIPS.contains(&56),
            "NIP-56 (reporting) must be advertised — kind:1984 ingest is live"
        );
    }

    #[test]
    fn build_advertises_buzz_repository_url() {
        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        assert_eq!(info.software, "https://github.com/agiterra/beekeeper");
    }

    /// Finding 32: NIP-11 must carry the relay's build identity, not just its
    /// crate version, so a redeploy can be confirmed by mechanism. Both
    /// fields are compile-time constants from `build_info` (in turn set by
    /// `build.rs`), so they are present unconditionally — never `Option`,
    /// because `unknown` is itself the disclosed answer, not an absence.
    #[test]
    fn build_advertises_software_commit_and_build_time() {
        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        assert_eq!(info.software_commit, crate::build_info::source_sha());
        assert_eq!(info.build_time, crate::build_info::build_time());
        // In this dev/test build the checkout's own `.git` is present, so
        // build.rs resolves a real commit rather than falling back to
        // `unknown` — pin the *shape* of both fields rather than an exact
        // value, since the exact commit changes on every commit that touches
        // this file.
        assert!(
            crate::build_provenance::is_full_sha(&info.software_commit)
                || info.software_commit == "unknown",
            "software_commit must be a full 40-hex commit or the literal unknown, got {:?}",
            info.software_commit
        );
        assert!(
            info.build_time.ends_with('Z') && info.build_time.contains('T'),
            "build_time must be RFC 3339 UTC, got {:?}",
            info.build_time
        );
        assert_eq!(
            info.software_commit_count,
            crate::build_info::source_commit_count()
        );
        assert!(
            info.software_commit_count.is_none_or(|count| count >= 1),
            "a count is absent or at least 1, never 0: {:?}",
            info.software_commit_count
        );
        // The coupling `build.rs::resolve_stamp` exists to guarantee: a
        // count is only ever emitted alongside a commit it actually
        // describes, so a count with an `unknown` commit would mean the two
        // came from different sources.
        assert!(
            info.software_commit_count.is_none() || info.software_commit != "unknown",
            "a count must never be paired with an unknown commit"
        );
    }

    #[test]
    fn software_commit_and_build_time_are_additive_and_always_present_in_json() {
        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        let json = serde_json::to_value(&info).expect("serialize");
        assert!(
            json.get("software_commit")
                .and_then(|v| v.as_str())
                .is_some(),
            "software_commit must always serialize, never be omitted"
        );
        assert!(
            json.get("build_time").and_then(|v| v.as_str()).is_some(),
            "build_time must always serialize, never be omitted"
        );
        let count = json
            .get("software_commit_count")
            .expect("software_commit_count must always serialize, never be omitted");
        // Omitting it would make "this relay could not determine a count"
        // indistinguishable from "this relay predates the field", which is
        // the disclosure distinction finding 32 exists to preserve.
        assert!(
            count.is_null() || count.as_u64().is_some_and(|n| n >= 1),
            "software_commit_count is a positive integer or null — never 0, \
             never a string: {count:?}"
        );
        // Existing fields untouched — additive means additive.
        assert_eq!(json["software"], "https://github.com/agiterra/beekeeper");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn configured_pairing_relay_is_advertised_and_unset_value_is_omitted() {
        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            Some("wss://pairing.buzz.xyz"),
            test_rate_limits(),
        );
        let json = serde_json::to_value(&info).expect("serialize");
        assert_eq!(
            json.get("pairing_relay_url")
                .and_then(|value| value.as_str()),
            Some("wss://pairing.buzz.xyz")
        );

        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        let json = serde_json::to_value(&info).expect("serialize");
        assert!(json.get("pairing_relay_url").is_none());
    }

    /// NIP-WP → NIP-11 mirror: a set workspace icon is served in the standard
    /// `icon` field; no icon (or a cleared, empty icon) omits the field
    /// entirely so the JSON matches pre-icon documents byte-for-byte.
    #[test]
    fn icon_is_mirrored_and_empty_or_absent_is_omitted() {
        let info = RelayInfo::build(
            None,
            Some("data:image/webp;base64,UklGRg=="),
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        assert_eq!(
            info.icon.as_deref(),
            Some("data:image/webp;base64,UklGRg==")
        );
        let json = serde_json::to_value(&info).expect("serialize");
        assert_eq!(
            json.get("icon").and_then(|v| v.as_str()),
            Some("data:image/webp;base64,UklGRg==")
        );

        for icon in [None, Some("")] {
            let info = RelayInfo::build(
                None,
                icon,
                false,
                DEFAULT_MAX_FRAME_BYTES,
                None,
                test_rate_limits(),
            );
            assert!(info.icon.is_none());
            let json = serde_json::to_value(&info).expect("serialize");
            assert!(
                json.get("icon").is_none(),
                "unset/cleared icon must omit the `icon` field, not serialize null/empty"
            );
        }
    }

    #[test]
    fn auth_required_is_advertised_true() {
        // REQ, EVENT, and COUNT all unconditionally require
        // `AuthState::Authenticated` (see `crates/buzz-relay/src/handlers/`),
        // so the NIP-11 doc must advertise it.
        assert!(relay_limitation(DEFAULT_MAX_FRAME_BYTES, test_rate_limits()).auth_required);
    }

    #[test]
    fn max_message_length_uses_configured_frame_limit() {
        let info = RelayInfo::build(None, None, false, 262_144, None, test_rate_limits());
        let limitation = info.limitation.expect("limitation");
        assert_eq!(limitation.max_message_length, Some(262_144));
    }

    #[test]
    fn supported_nips_are_sorted() {
        let mut sorted = SUPPORTED_NIPS.to_vec();
        sorted.sort();
        assert_eq!(
            SUPPORTED_NIPS,
            &sorted[..],
            "supported_nips should be sorted"
        );
    }

    #[test]
    fn nip43_not_in_static_supported_nips() {
        // NIP-43 advertisement is conditional on runtime config (stable signing
        // key + membership enforcement) and must NOT live in the static list.
        // The desktop pairing probe keys off this NIP — advertising it on
        // open relays misroutes pairing peers to a non-existent /pair sidecar.
        assert!(
            !SUPPORTED_NIPS.contains(&NIP_RELAY_MEMBERSHIP),
            "NIP-43 must be advertised only when advertise_nip43=true is passed to RelayInfo::build"
        );
    }

    /// Open relay, ephemeral key — both `self` and NIP-43 are absent.
    #[test]
    fn build_open_relay_ephemeral_key_omits_self_and_nip43() {
        let info = RelayInfo::build(
            None,
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        assert!(info.relay_self.is_none());
        assert!(!info.supported_nips.contains(&NIP_RELAY_MEMBERSHIP));
    }

    /// Open relay with a stable signing key (e.g. for NIP-29 group metadata
    /// signing): `self` MUST be advertised so clients can verify those
    /// events; NIP-43 must NOT be, because the relay isn't enforcing
    /// membership. This is the staging-default shape — the bug we're
    /// fixing — and the regression we must not reintroduce.
    #[test]
    fn build_open_relay_stable_key_advertises_self_but_not_nip43() {
        let pk = "0000000000000000000000000000000000000000000000000000000000000001";
        let info = RelayInfo::build(
            Some(pk),
            None,
            false,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        assert_eq!(info.relay_self.as_deref(), Some(pk));
        assert!(!info.supported_nips.contains(&NIP_RELAY_MEMBERSHIP));
    }

    /// Membership-enforcing relay: both `self` and NIP-43 advertised.
    #[test]
    fn build_membership_relay_advertises_self_and_nip43() {
        let pk = "0000000000000000000000000000000000000000000000000000000000000001";
        let info = RelayInfo::build(
            Some(pk),
            None,
            true,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
        assert_eq!(info.relay_self.as_deref(), Some(pk));
        assert!(info.supported_nips.contains(&NIP_RELAY_MEMBERSHIP));
    }

    /// NIP-43 events are verified against `self`; advertising NIP-43 without
    /// `self` would give clients no way to verify membership events. The
    /// debug_assert in `build` catches this in tests/debug builds.
    #[test]
    #[should_panic(expected = "advertise_nip43=true requires relay_self=Some")]
    fn build_nip43_without_self_panics_in_debug() {
        let _ = RelayInfo::build(
            None,
            None,
            true,
            DEFAULT_MAX_FRAME_BYTES,
            None,
            test_rate_limits(),
        );
    }
}
