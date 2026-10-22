//! The `webhook-plugin@1` world's host-side checks: what a plugin's auth plan
//! may say, the MAC the host verifies with it, and how the host builds an
//! application event from what a plugin proposed.
//!
//! **The host verifies, never the embedder or the plugin.**
//! [`crate::WebhookCall::verify`] takes the plugin's [`AuthPlan`], the installation's secret, the raw body
//! and the receive time, computes the MAC over `signed-prefix || body` with
//! the `hmac` crate and compares it with `Mac::verify_slice` (constant time).
//! Only it builds a [`VerifiedDelivery`], which owns the body, the plan's
//! vendor event and delivery id, the vendor timestamp parsed from the signed
//! prefix, and the receive time; `project` takes one by value, so an
//! unverified body cannot reach a plugin's `project` at all — that is a
//! compile error, not a rule — and the event's vendor name and `created_at`
//! can only be the ones that went through verification.
//!
//! **The MAC covers `signed-prefix || body` and nothing else.** The plan's
//! delivery id and vendor event are the plugin's reading of the headers; the
//! host carries them but cannot authenticate them, and does not. The vendor
//! timestamp is the one plan field the host binds: the plan names where it
//! sits inside the signed prefix ([`ByteSpan`]) and the host parses it from
//! those bytes, so it is exactly as authentic as the MAC.
//!
//! The plugin proposes an event type, a schema number, a data object and
//! (allowlisted) data tags. Everything else is the host's: the channel scope
//! (`h`), the namespace and type labels (`L`, `l`), the content envelope, the
//! `created_at` (the receive time the embedder passes in, never a plugin
//! value), and the signature. The embedder's [`DraftKind`] for kind 50000 —
//! the relay passes ingest's own application-event validator — runs on the
//! finished event before anything is returned. **This is the only check in
//! the webhook world**: the relay signs what comes out with its own key, so
//! no member's client re-seals it afterwards the way a Planning draft is
//! re-sealed. An embedder must not store an event this module did not build.

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::drafts::{check_constructed_caps, seal_scoped_tags, DraftKind, MAX_DRAFT_CONTENT_BYTES};
use crate::Scope;

/// Kind of an application event.
pub const KIND_APPLICATION_EVENT: u32 = 50000;
/// The NIP-32 namespace an application event's `L` and `l` labels carry.
/// Ingest requires exactly this value; its validator is the authority.
pub const APPLICATION_EVENT_NAMESPACE: &str = "org.agiterra.application-event";
/// Bytes of a projected `data` object.
pub const MAX_DATA_BYTES: usize = 64 * 1024;
/// Bytes of an application-event type.
pub const MAX_EVENT_TYPE_BYTES: usize = 128;
/// Bytes of a delivery id.
pub const MAX_DELIVERY_ID_BYTES: usize = 128;
/// Bytes of a vendor event name.
pub const MAX_VENDOR_EVENT_BYTES: usize = 64;
/// Bytes of an HMAC-SHA256 signature: exactly this many, never a prefix.
pub const HMAC_SHA256_BYTES: usize = 32;
/// Bytes of a signed prefix.
pub const MAX_SIGNED_PREFIX_BYTES: usize = 4 * 1024;
/// How far a vendor-asserted timestamp may be from the receive time.
pub const MAX_TIMESTAMP_SKEW_SECS: u64 = 300;
/// Headers handed to a plugin, and their total bytes (names and values).
pub const MAX_HEADERS: usize = 128;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Bytes of an installation's settings JSON.
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// Bytes of an `ignore`/`reject` reason kept.
pub const MAX_REASON_BYTES: usize = 512;

/// One request header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Lowercase.
    pub name: String,
    pub value: Vec<u8>,
}

/// What a webhook plugin says about itself, checked at load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookManifest {
    pub name: String,
    pub version: String,
    pub type_prefix: String,
    pub max_body_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MacAlgorithm {
    HmacSha256,
}

/// Where, inside an [`AuthPlan`]'s signed prefix, the vendor's unix
/// timestamp sits: `len` ASCII decimal digits starting at byte `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    pub start: u32,
    pub len: u32,
}

/// Digits of a span-held timestamp: enough for any `u64`.
pub const MAX_TIMESTAMP_DIGITS: u32 = 20;

/// What the plugin says the host must verify, read from the headers. Checked
/// for shape by `authenticate`; [`crate::WebhookCall::verify`] checks it again,
/// with the timestamp skew, and computes the MAC. A plan is a claim, not a
/// verification: nothing here lets a body reach `project`.
///
/// Only `signed_prefix` is covered by the MAC (with the body). There is no
/// timestamp field: `timestamp_span` points into `signed_prefix`, and the
/// host reads the value from there. `delivery_id` and `vendor_event` are
/// unauthenticated unless the vendor signs them — see [`VerifiedDelivery`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPlan {
    pub algorithm: MacAlgorithm,
    pub signature: Vec<u8>,
    pub signed_prefix: Vec<u8>,
    pub timestamp_span: Option<ByteSpan>,
    pub delivery_id: String,
    pub vendor_event: String,
}

impl AuthPlan {
    /// The vendor timestamp the signed prefix holds at `timestamp_span`, or
    /// why it cannot be read: a span that is empty, longer than
    /// [`MAX_TIMESTAMP_DIGITS`] or past the prefix's end, or bytes that are
    /// not a decimal `u64`. `None` when the plan names no span.
    ///
    /// This proves the digits were signed, not that they are the vendor's
    /// timestamp field: choosing the span (or naming none, which skips the
    /// skew check) is the plugin's, trusted as part of its pinned code.
    pub fn signed_timestamp(&self) -> Result<Option<u64>, String> {
        let Some(span) = self.timestamp_span else {
            return Ok(None);
        };
        if span.len == 0 || span.len > MAX_TIMESTAMP_DIGITS {
            return Err(format!(
                "timestamp span is {} bytes; it must be 1-{MAX_TIMESTAMP_DIGITS}",
                span.len
            ));
        }
        let start = span.start as usize;
        let bytes = start
            .checked_add(span.len as usize)
            .and_then(|end| self.signed_prefix.get(start..end))
            .ok_or_else(|| {
                format!(
                    "timestamp span {}..+{} is outside the {}-byte signed prefix",
                    span.start,
                    span.len,
                    self.signed_prefix.len()
                )
            })?;
        if !bytes.iter().all(u8::is_ascii_digit) {
            return Err("timestamp span does not hold ASCII decimal digits".to_owned());
        }
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|digits| digits.parse::<u64>().ok())
            .map(Some)
            .ok_or_else(|| "timestamp span does not hold a u64".to_owned())
    }
}

/// Why a delivery could not be authenticated, as the plugin or the host's
/// shape check put it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "message", rename_all = "kebab-case")]
pub enum AuthFailure {
    MissingHeader(String),
    Malformed(String),
    Unsupported(String),
}

/// Why the host refused to verify a delivery. Never names the secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    /// The body is past the plugin's cap; nothing was hashed.
    #[error("body is {size} bytes; this plugin accepts at most {limit}")]
    BodyTooLarge { size: usize, limit: usize },
    /// The plan, the receive time or the secret cannot be verified against.
    #[error("{0}")]
    Malformed(String),
    /// The MAC over the signed prefix and the body is not the plan's signature.
    #[error("the signature does not match the body")]
    BadSignature,
}

/// A delivery whose MAC the host verified. Only
/// [`crate::WebhookCall::verify`] builds one — its fields are private and it is neither `Default`, `Clone` nor
/// `Deserialize` — so a body reaches a plugin's `project` only after its MAC
/// matched. It is bound to the plugin and installation it was verified for.
///
/// `project` borrows it: after [`crate::InvokeError::Busy`] the caller
/// retries the same delivery, with the same [`VerifiedDelivery::received_at`]
/// and so the same event. Nothing here limits a delivery to one projection;
/// that is the B1 store's (`beekeeper_db::vendor_ingress::record_delivery`),
/// which dedupes on `(community, installation, delivery id)`.
///
/// **What it authenticates:** that the holder of the installation's secret
/// produced the MAC over `signed_prefix || body` — those bytes and nothing
/// else. So [`VerifiedDelivery::body`] and [`VerifiedDelivery::vendor_timestamp`]
/// (parsed by the host from inside the signed prefix) are authenticated;
/// [`VerifiedDelivery::received_at`] is the host's own clock.
/// [`VerifiedDelivery::delivery_id`] and [`VerifiedDelivery::vendor_event`]
/// are NOT: the plugin read them from headers, and they are authentic only
/// when the vendor signs them, i.e. when the plugin took them from bytes in
/// the signed prefix or body — which the host does not check. GitHub signs
/// the body alone, so its `X-GitHub-Event` and `X-GitHub-Delivery` can be
/// changed in transit without failing the MAC. Treat both as routing hints:
/// never as an authorization input, and dedupe on a delivery id only with
/// that in mind.
///
/// **What B1's dedup does with an unsigned id.** The key is scoped to one
/// community and one installation. Within it, a delivery id already recorded
/// `accepted` or `ignored` is a duplicate and writes nothing; one recorded
/// `rejected` is projected again. Another installation's key is separate,
/// but a body reaches it only under that installation's secret. An id alone
/// grants nothing: a request that fails the MAC never writes a delivery row,
/// so it cannot take or pre-poison a key. The dedup stops a vendor
/// redelivery and a replay that keeps its id — not a replay of a captured
/// signed body under a fresh id: GitHub signs no id and no time, so that
/// replay is a new delivery and publishes a second event, and with no signed
/// time there is no skew window to bound it. Only TLS and the secrecy of the
/// installation secret keep a signed body from a replayer; event consumers
/// must tolerate a duplicate. Once a delivery row is pruned (30 days) its id
/// is new again too.
pub struct VerifiedDelivery {
    plugin: String,
    installation: String,
    body: Vec<u8>,
    vendor_event: String,
    delivery_id: String,
    vendor_timestamp: Option<u64>,
    received_at: u64,
}

impl VerifiedDelivery {
    /// The vendor's delivery id, as the plugin read it. Not authenticated by
    /// the MAC unless the vendor signs it (see the type's doc).
    pub fn delivery_id(&self) -> &str {
        &self.delivery_id
    }

    /// The vendor's event name, as the plugin read it. Not authenticated by
    /// the MAC unless the vendor signs it (see the type's doc). `project`
    /// hands the plugin this, never a value the embedder passes.
    pub fn vendor_event(&self) -> &str {
        &self.vendor_event
    }

    /// The vendor's timestamp, parsed by the host from the plan's span of the
    /// signed prefix — so covered by the MAC — and within
    /// [`MAX_TIMESTAMP_SKEW_SECS`] of [`VerifiedDelivery::received_at`].
    /// `None` when the scheme signs none (GitHub).
    pub fn vendor_timestamp(&self) -> Option<u64> {
        self.vendor_timestamp
    }

    /// The host's receive time the MAC and skew were checked at; it becomes
    /// the event's `created_at`.
    pub fn received_at(&self) -> u64 {
        self.received_at
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn is_for(&self, plugin: &str, installation: &str) -> bool {
        self.plugin == plugin && self.installation == installation
    }
}

impl std::fmt::Debug for VerifiedDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedDelivery")
            .field("plugin", &self.plugin)
            .field("installation", &self.installation)
            .field("body_bytes", &self.body.len())
            .field("vendor_event", &self.vendor_event)
            .field("delivery_id", &self.delivery_id)
            .field("vendor_timestamp", &self.vendor_timestamp)
            .field("received_at", &self.received_at)
            .finish()
    }
}

/// An application event the host built and checked, ready for the embedder
/// to sign with the relay's key and store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedEvent {
    pub kind: u32,
    /// The receive time the embedder passed in.
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// What a delivery came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookOutcome {
    Accept(SealedEvent),
    Ignore(String),
    Reject(String),
}

/// An application event as the plugin proposed it.
#[derive(Debug, Clone)]
pub(crate) struct ProposedEvent {
    pub event_type: String,
    pub schema: u32,
    pub data_json: String,
    pub tags: Vec<Vec<String>>,
}

pub(crate) fn check_headers(headers: &[Header]) -> Result<(), String> {
    if headers.len() > MAX_HEADERS {
        return Err(format!(
            "{} headers; at most {MAX_HEADERS} are passed to a plugin",
            headers.len()
        ));
    }
    let bytes: usize = headers.iter().map(|h| h.name.len() + h.value.len()).sum();
    if bytes > MAX_HEADER_BYTES {
        return Err(format!(
            "headers are {bytes} bytes; at most {MAX_HEADER_BYTES} are passed to a plugin"
        ));
    }
    Ok(())
}

pub(crate) fn check_settings(settings_json: &str) -> Result<(), String> {
    if settings_json.len() > MAX_SETTINGS_BYTES {
        return Err(format!("settings are over {MAX_SETTINGS_BYTES} bytes"));
    }
    match serde_json::from_str::<serde_json::Value>(settings_json) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        _ => Err("settings must be a JSON object".to_owned()),
    }
}

/// Check a plugin's auth plan for shape. A failure here is the plugin's
/// malformed plan, reported like a malformed request (the delivery is not
/// authenticated).
pub(crate) fn check_auth_plan(plan: &AuthPlan) -> Result<(), String> {
    let id = &plan.delivery_id;
    if id.is_empty()
        || id.len() > MAX_DELIVERY_ID_BYTES
        || !id.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(format!(
            "delivery id must be 1-{MAX_DELIVERY_ID_BYTES} printable ASCII bytes"
        ));
    }
    let event = &plan.vendor_event;
    if event.is_empty()
        || event.len() > MAX_VENDOR_EVENT_BYTES
        || !event
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
    {
        return Err(format!(
            "vendor event must be 1-{MAX_VENDOR_EVENT_BYTES} bytes of [A-Za-z0-9_.:-]"
        ));
    }
    match plan.algorithm {
        MacAlgorithm::HmacSha256 if plan.signature.len() != HMAC_SHA256_BYTES => {
            return Err(format!(
                "an hmac-sha256 signature is exactly {HMAC_SHA256_BYTES} bytes, not {}",
                plan.signature.len()
            ));
        }
        MacAlgorithm::HmacSha256 => {}
    }
    if plan.signed_prefix.len() > MAX_SIGNED_PREFIX_BYTES {
        return Err(format!(
            "signed prefix is over {MAX_SIGNED_PREFIX_BYTES} bytes"
        ));
    }
    plan.signed_timestamp()?;
    Ok(())
}

/// Verify one delivery to `installation` of `plugin`: the plan's shape again,
/// a real receive time, the skew of the vendor timestamp the host parses from
/// the signed prefix against that same time, and the MAC of
/// `signed-prefix || body` under `secret`. The secret is the embedder's to
/// pass and is never shown to a plugin or put in an error.
pub(crate) fn verify_delivery(
    plugin: &str,
    installation: &str,
    plan: AuthPlan,
    secret: &[u8],
    body: Vec<u8>,
    received_at: u64,
) -> Result<VerifiedDelivery, VerifyError> {
    if received_at == 0 {
        return Err(VerifyError::Malformed(
            "the receive time is 0; the host's clock must be passed in".to_owned(),
        ));
    }
    if secret.is_empty() {
        return Err(VerifyError::Malformed(
            "the installation has an empty webhook secret".to_owned(),
        ));
    }
    check_auth_plan(&plan).map_err(VerifyError::Malformed)?;
    let vendor_timestamp = plan.signed_timestamp().map_err(VerifyError::Malformed)?;
    if let Some(at) = vendor_timestamp {
        if at.abs_diff(received_at) > MAX_TIMESTAMP_SKEW_SECS {
            return Err(VerifyError::Malformed(format!(
                "vendor timestamp {at} is more than {MAX_TIMESTAMP_SKEW_SECS} s from the receive time {received_at}"
            )));
        }
    }
    match plan.algorithm {
        MacAlgorithm::HmacSha256 => {
            let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret)
                .map_err(|error| VerifyError::Malformed(format!("hmac key: {error}")))?;
            mac.update(&plan.signed_prefix);
            mac.update(&body);
            mac.verify_slice(&plan.signature)
                .map_err(|_| VerifyError::BadSignature)?;
        }
    }
    Ok(VerifiedDelivery {
        plugin: plugin.to_owned(),
        installation: installation.to_owned(),
        body,
        vendor_event: plan.vendor_event,
        delivery_id: plan.delivery_id,
        vendor_timestamp,
        received_at,
    })
}

/// Build the application event for `proposed` in `channel`, stamped
/// `received_at`, and run `kind`'s validator on it. Any failure refuses the
/// whole delivery's output; nothing is ever repaired.
pub(crate) fn seal_application_event(
    kind: &DraftKind,
    type_prefix: &str,
    channel: &str,
    received_at: u64,
    proposed: ProposedEvent,
) -> Result<SealedEvent, String> {
    let event_type = proposed.event_type;
    let name = event_type
        .strip_prefix(type_prefix)
        .and_then(|rest| rest.strip_prefix('.'));
    if name.is_none_or(str::is_empty) {
        return Err(format!(
            "event type {event_type:?} does not start with the granted prefix {type_prefix:?} and a '.'"
        ));
    }
    if event_type.len() > MAX_EVENT_TYPE_BYTES {
        return Err(format!("event type is over {MAX_EVENT_TYPE_BYTES} bytes"));
    }
    if proposed.schema == 0 {
        return Err("schema must be 1 or more".to_owned());
    }
    if proposed.data_json.len() > MAX_DATA_BYTES {
        return Err(format!("data is over {MAX_DATA_BYTES} bytes"));
    }
    let data: serde_json::Value = serde_json::from_str(&proposed.data_json)
        .map_err(|error| format!("data is not JSON: {error}"))?;
    if !data.is_object() {
        return Err("data must be a JSON object".to_owned());
    }
    let content = serde_json::json!({ "schema": proposed.schema, "data": data }).to_string();
    if content.len() > MAX_DRAFT_CONTENT_BYTES + 64 {
        return Err("content is too large".to_owned());
    }
    let mut tags = seal_scoped_tags(
        kind.data_tags(),
        &Scope::Channel(channel.to_owned()),
        &proposed.tags,
    )?;
    tags.push(vec!["L".to_owned(), APPLICATION_EVENT_NAMESPACE.to_owned()]);
    tags.push(vec![
        "l".to_owned(),
        event_type,
        APPLICATION_EVENT_NAMESPACE.to_owned(),
    ]);
    check_constructed_caps(&tags)?;
    kind.validate(&tags, &content)?;
    Ok(SealedEvent {
        kind: KIND_APPLICATION_EVENT,
        created_at: received_at,
        tags,
        content,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::drafts::DraftReferences;

    const CHANNEL: &str = "6f1c2b9e-3d4a-4f5b-8c7d-9e0f1a2b3c4d";

    fn kind(data_tags: &[&str]) -> DraftKind {
        DraftKind::new(data_tags, Arc::new(|_, _| Ok(DraftReferences::default())))
    }

    fn event(event_type: &str, tags: Vec<Vec<String>>) -> ProposedEvent {
        ProposedEvent {
            event_type: event_type.to_owned(),
            schema: 1,
            data_json: r#"{"action":"opened"}"#.to_owned(),
            tags,
        }
    }

    fn tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn the_host_builds_scope_labels_content_and_time() {
        let sealed = seal_application_event(
            &kind(&[]),
            "com.github",
            CHANNEL,
            1_800_000_000,
            event("com.github.pull-request.opened", vec![tag(&["h", CHANNEL])]),
        )
        .expect("seals");
        assert_eq!(sealed.kind, KIND_APPLICATION_EVENT);
        assert_eq!(sealed.created_at, 1_800_000_000);
        assert_eq!(
            sealed.tags,
            vec![
                tag(&["h", CHANNEL]),
                tag(&["L", APPLICATION_EVENT_NAMESPACE]),
                tag(&[
                    "l",
                    "com.github.pull-request.opened",
                    APPLICATION_EVENT_NAMESPACE
                ]),
            ]
        );
        assert_eq!(sealed.content, r#"{"data":{"action":"opened"},"schema":1}"#);
    }

    #[test]
    fn a_type_outside_the_granted_prefix_is_refused() {
        for bad in [
            "com.gitlab.push",
            "com.githubx.push",
            "com.github",
            "com.github.",
        ] {
            let error =
                seal_application_event(&kind(&[]), "com.github", CHANNEL, 1, event(bad, vec![]))
                    .unwrap_err();
            assert!(error.contains("prefix"), "{bad}: {error}");
        }
    }

    #[test]
    fn data_must_be_a_bounded_object_and_schema_positive() {
        let mut e = event("com.github.push", vec![]);
        e.schema = 0;
        assert!(seal_application_event(&kind(&[]), "com.github", CHANNEL, 1, e).is_err());
        for data in ["[]", "1", "not json"] {
            let mut e = event("com.github.push", vec![]);
            e.data_json = data.to_owned();
            assert!(seal_application_event(&kind(&[]), "com.github", CHANNEL, 1, e).is_err());
        }
        let mut e = event("com.github.push", vec![]);
        e.data_json = format!("{{\"x\":\"{}\"}}", "y".repeat(MAX_DATA_BYTES));
        assert!(
            seal_application_event(&kind(&[]), "com.github", CHANNEL, 1, e)
                .unwrap_err()
                .contains("data is over")
        );
    }

    #[test]
    fn a_plugin_cannot_propose_routing_or_provenance() {
        for proposed in [
            tag(&["h", "00000000-0000-4000-8000-000000000000"]),
            tag(&["a", "30621:aa:x"]),
            tag(&["p", "aa"]),
            tag(&["e", "aa"]),
            tag(&["L", APPLICATION_EVENT_NAMESPACE]),
            tag(&["l", "com.github.other", APPLICATION_EVENT_NAMESPACE]),
        ] {
            assert!(
                seal_application_event(
                    &kind(&[]),
                    "com.github",
                    CHANNEL,
                    1,
                    event("com.github.push", vec![proposed.clone()])
                )
                .is_err(),
                "{proposed:?}"
            );
        }
        // An allowlisted data tag passes, after the scope and before the labels.
        let sealed = seal_application_event(
            &kind(&["r"]),
            "com.github",
            CHANNEL,
            1,
            event("com.github.push", vec![tag(&["r", "https://example"])]),
        )
        .unwrap();
        assert_eq!(sealed.tags[1], tag(&["r", "https://example"]));
    }

    #[test]
    fn the_embedders_validator_has_the_last_word() {
        let refusing = DraftKind::new(&[], Arc::new(|_, _| Err("ingest says no".to_owned())));
        let error = seal_application_event(
            &refusing,
            "com.github",
            CHANNEL,
            1,
            event("com.github.push", vec![]),
        )
        .unwrap_err();
        assert_eq!(error, "ingest says no");
    }

    fn plan() -> AuthPlan {
        AuthPlan {
            algorithm: MacAlgorithm::HmacSha256,
            signature: vec![1; HMAC_SHA256_BYTES],
            signed_prefix: vec![],
            timestamp_span: None,
            delivery_id: "72d3162e-cc78-11e3-81ab-4c9367dc0958".to_owned(),
            vendor_event: "pull_request".to_owned(),
        }
    }

    #[test]
    fn an_auth_plan_is_shape_checked() {
        assert!(check_auth_plan(&plan()).is_ok());
        for edit in [
            (|p: &mut AuthPlan| p.delivery_id = String::new()) as fn(&mut AuthPlan),
            |p| p.delivery_id = "has space".to_owned(),
            |p| p.delivery_id = "x".repeat(MAX_DELIVERY_ID_BYTES + 1),
            |p| p.vendor_event = "bad/event".to_owned(),
            |p| p.signature = vec![],
            |p| p.signed_prefix = vec![0; MAX_SIGNED_PREFIX_BYTES + 1],
        ] {
            let mut p = plan();
            edit(&mut p);
            assert!(check_auth_plan(&p).is_err(), "{p:?}");
        }
    }

    #[test]
    fn an_hmac_sha256_signature_is_exactly_32_bytes() {
        for len in [1, 16, 31, 33, 64, 512] {
            let mut p = plan();
            p.signature = vec![1; len];
            let error = check_auth_plan(&p).unwrap_err();
            assert!(error.contains("exactly 32 bytes"), "{len}: {error}");
        }
        let mut p = plan();
        p.signature = vec![1; 32];
        assert!(check_auth_plan(&p).is_ok());
    }

    const SECRET: &[u8] = b"installation secret";
    const BODY: &[u8] = br#"{"action":"opened"}"#;
    const PREFIX: &[u8] = b"v0:1800000000:";

    fn mac(secret: &[u8], message: &[&[u8]]) -> Vec<u8> {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).unwrap();
        for part in message {
            mac.update(part);
        }
        mac.finalize().into_bytes().to_vec()
    }

    /// A plan signed over `PREFIX || BODY` with `SECRET`.
    fn signed_plan() -> AuthPlan {
        AuthPlan {
            signature: mac(SECRET, &[PREFIX, BODY]),
            signed_prefix: PREFIX.to_vec(),
            ..plan()
        }
    }

    fn verify(plan: AuthPlan, secret: &[u8], body: &[u8]) -> Result<VerifiedDelivery, VerifyError> {
        verify_delivery("p", "inst", plan, secret, body.to_vec(), 1_800_000_000)
    }

    #[test]
    fn the_host_verifies_the_mac_over_the_signed_prefix_and_the_body() {
        let verified = verify(signed_plan(), SECRET, BODY).expect("the right secret passes");
        assert_eq!(verified.body(), BODY);
        assert_eq!(verified.vendor_event(), "pull_request");
        assert_eq!(
            verified.delivery_id(),
            "72d3162e-cc78-11e3-81ab-4c9367dc0958"
        );
        assert_eq!(verified.received_at(), 1_800_000_000);
        assert!(verified.is_for("p", "inst"));
        assert!(!verified.is_for("p", "other"));
        // With no prefix the MAC is over the body alone (GitHub's scheme).
        let bare = AuthPlan {
            signature: mac(SECRET, &[BODY]),
            ..plan()
        };
        assert!(verify(bare, SECRET, BODY).is_ok());
    }

    #[test]
    fn a_wrong_secret_or_any_flipped_byte_fails_the_mac() {
        assert_eq!(
            verify(signed_plan(), b"another secret", BODY).unwrap_err(),
            VerifyError::BadSignature
        );
        let mut body = BODY.to_vec();
        body[3] ^= 0x01;
        assert_eq!(
            verify(signed_plan(), SECRET, &body).unwrap_err(),
            VerifyError::BadSignature
        );
        let mut p = signed_plan();
        p.signed_prefix[0] ^= 0x01;
        assert_eq!(
            verify(p, SECRET, BODY).unwrap_err(),
            VerifyError::BadSignature
        );
        let mut p = signed_plan();
        p.signature[31] ^= 0x01;
        assert_eq!(
            verify(p, SECRET, BODY).unwrap_err(),
            VerifyError::BadSignature
        );
        // The prefix is signed, not merely compared: dropping it fails too.
        let mut p = signed_plan();
        p.signed_prefix.clear();
        assert_eq!(
            verify(p, SECRET, BODY).unwrap_err(),
            VerifyError::BadSignature
        );
    }

    #[test]
    fn a_truncated_signature_is_refused_before_any_compare() {
        let mut p = signed_plan();
        p.signature.truncate(16);
        assert!(matches!(
            verify(p, SECRET, BODY).unwrap_err(),
            VerifyError::Malformed(m) if m.contains("exactly 32 bytes")
        ));
    }

    #[test]
    fn a_zero_receive_time_or_an_empty_secret_is_refused() {
        let error =
            verify_delivery("p", "inst", signed_plan(), SECRET, BODY.to_vec(), 0).unwrap_err();
        assert!(
            matches!(error, VerifyError::Malformed(ref m) if m.contains("receive time is 0")),
            "{error:?}"
        );
        let unsigned = AuthPlan {
            signature: mac(b"", &[PREFIX, BODY]),
            ..signed_plan()
        };
        assert!(matches!(
            verify(unsigned, b"", BODY).unwrap_err(),
            VerifyError::Malformed(m) if m.contains("empty webhook secret")
        ));
    }

    /// A plan whose signed prefix is `v0:<timestamp>:` (Slack's shape), signed
    /// with the body, its span on the digits.
    fn stamped_plan(timestamp: &str) -> AuthPlan {
        let prefix = format!("v0:{timestamp}:").into_bytes();
        AuthPlan {
            signature: mac(SECRET, &[&prefix, BODY]),
            signed_prefix: prefix,
            timestamp_span: Some(ByteSpan {
                start: 3,
                len: timestamp.len() as u32,
            }),
            ..plan()
        }
    }

    #[test]
    fn the_vendor_timestamp_skew_is_checked_against_the_verified_receive_time() {
        let at = |timestamp: u64, received_at: u64| {
            verify_delivery(
                "p",
                "inst",
                stamped_plan(&timestamp.to_string()),
                SECRET,
                BODY.to_vec(),
                received_at,
            )
        };
        assert!(at(1_000 + MAX_TIMESTAMP_SKEW_SECS, 1_000).is_ok());
        assert!(at(1_000 - MAX_TIMESTAMP_SKEW_SECS, 1_000).is_ok());
        assert!(matches!(
            at(1_000 + MAX_TIMESTAMP_SKEW_SECS + 1, 1_000).unwrap_err(),
            VerifyError::Malformed(m) if m.contains("timestamp")
        ));
        assert!(at(1_000 - MAX_TIMESTAMP_SKEW_SECS - 1, 1_000).is_err());
    }

    #[test]
    fn the_timestamp_is_the_signed_one_never_a_plugin_claim() {
        const RECEIVED: u64 = 1_800_000_000;
        // The vendor signed a stale time. A plugin has no field to claim a
        // fresh one in; the host reads the signed digits and refuses.
        let error = verify_delivery(
            "p",
            "inst",
            stamped_plan("1799990000"),
            SECRET,
            BODY.to_vec(),
            RECEIVED,
        )
        .unwrap_err();
        assert!(
            matches!(error, VerifyError::Malformed(ref m) if m.contains("1799990000")),
            "{error:?}"
        );
        // Rewriting the signed digits to a fresh time breaks the MAC.
        let mut forged = stamped_plan("1799990000");
        forged.signed_prefix = b"v0:1800000000:".to_vec();
        assert_eq!(
            verify_delivery("p", "inst", forged, SECRET, BODY.to_vec(), RECEIVED).unwrap_err(),
            VerifyError::BadSignature
        );
        // A fresh signed time passes, and is the value the delivery carries.
        let verified = verify_delivery(
            "p",
            "inst",
            stamped_plan("1800000123"),
            SECRET,
            BODY.to_vec(),
            RECEIVED,
        )
        .expect("fresh");
        assert_eq!(verified.vendor_timestamp(), Some(1_800_000_123));
        assert_eq!(verified.received_at(), RECEIVED);
        // No span, no vendor timestamp.
        let bare = verify(signed_plan(), SECRET, BODY).expect("no span");
        assert_eq!(bare.vendor_timestamp(), None);
    }

    #[test]
    fn a_timestamp_span_outside_the_signed_prefix_is_refused() {
        let prefix_len = stamped_plan("1800000000").signed_prefix.len() as u32;
        for (start, len) in [
            (prefix_len, 1),
            (prefix_len - 1, 2),
            (3, prefix_len),
            (u32::MAX, 10),
            (3, 0),
            (0, MAX_TIMESTAMP_DIGITS + 1),
        ] {
            let mut p = stamped_plan("1800000000");
            p.timestamp_span = Some(ByteSpan { start, len });
            let error = check_auth_plan(&p).unwrap_err();
            assert!(
                error.contains("outside") || error.contains("must be 1-"),
                "{start}+{len}: {error}"
            );
            assert!(matches!(
                verify_delivery("p", "inst", p, SECRET, BODY.to_vec(), 1_800_000_000),
                Err(VerifyError::Malformed(_))
            ));
        }
    }

    #[test]
    fn an_unparseable_timestamp_span_is_refused() {
        for (prefix, start, len) in [
            ("v0:1800000000:", 0, 2),            // "v0"
            ("v0:1800000000:", 2, 11),           // ":1800000000"
            ("v0:18000000x0:", 3, 10),           // a letter among the digits
            ("v0:+1800000000:", 3, 11),          // a sign
            ("v0:99999999999999999999:", 3, 20), // past u64::MAX
        ] {
            let p = AuthPlan {
                signature: mac(SECRET, &[prefix.as_bytes(), BODY]),
                signed_prefix: prefix.as_bytes().to_vec(),
                timestamp_span: Some(ByteSpan { start, len }),
                ..plan()
            };
            assert!(check_auth_plan(&p).is_err(), "{prefix} {start}+{len}");
            let error =
                verify_delivery("p", "inst", p, SECRET, BODY.to_vec(), 1_800_000_000).unwrap_err();
            assert!(
                matches!(error, VerifyError::Malformed(ref m) if m.contains("timestamp span")),
                "{prefix} {start}+{len}: {error:?}"
            );
        }
    }

    #[test]
    fn settings_must_be_a_json_object() {
        assert!(check_settings("{}").is_ok());
        assert!(check_settings("[]").is_err());
        assert!(check_settings("nope").is_err());
    }
}
