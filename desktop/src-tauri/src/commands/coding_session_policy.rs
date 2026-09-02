//! Native adapter for NIP-CSP session policy records (kind 44245).
//!
//! Desktop never encodes or decodes a policy itself. The launch form collects
//! the answers, hands them here as one object, and gets back the exact bytes
//! `buzz-sdk` would sign plus a flattened record for rendering; the Inspector
//! hands a signed event here and gets back the same record. Every bound,
//! closed vocabulary, exact-key rule and refusal sentence belongs to
//! `buzz_core::coding_session_policy` — a TypeScript copy of any of them would
//! be a second implementation of a wire contract, which is the one thing the
//! batch rules forbid outright.
//!
//! Two commands rather than one because they answer different questions and a
//! caller should not have to pretend to hold a signed event in order to check
//! a draft:
//!
//! * [`build_coding_session_policy_event`] — a draft in, unsigned event out.
//!   The `content` it returns is **Rust's** serialization, not the caller's,
//!   so what gets signed is what the decoder read.
//! * [`decode_coding_session_policy_record`] — a signed event in, the same
//!   flattened record out, after the envelope validator has run.
//!
//! The record's fields are all **present**, with `null` where the policy sets
//! nothing. A key that vanished when unset would let a reader that has not
//! shipped a field and a policy that does not set it look identical, and
//! "unknown ≠ empty" is exactly the distinction the 44245 record exists to
//! keep.

use buzz_core_pkg::coding_session_policy::{
    decode_coding_session_policy, validate_coding_session_policy_envelope,
    CodingSessionPolicyPayload,
};
use buzz_sdk_pkg::coding_session_policy::build_coding_session_policy;
use nostr::Event;
use serde::{Deserialize, Serialize};

/// Closed wire-schema identifier accepted by the build boundary.
pub const CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA: &str =
    "buzz-coding-session-policy-build-request/v1";
/// Closed wire-schema identifier accepted by the decode boundary.
pub const CODING_SESSION_POLICY_READ_REQUEST_SCHEMA: &str =
    "buzz-coding-session-policy-read-request/v1";
/// Closed wire-schema identifier this native adapter answers with.
pub const CODING_SESSION_POLICY_ADAPTER_SCHEMA: &str = "buzz-coding-session-policy-adapter/v1";

/// A draft policy on its way to a signer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyBuildRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// Canonical lowercase channel UUID for the `h` tag.
    pub channel_ref: String,
    /// The policy content object exactly as NIP-CSP defines it.
    ///
    /// Deliberately untyped at this boundary: it is handed straight to
    /// `buzz-core`'s strict decoder, so an unknown key, an explicit null or a
    /// zero limit is refused by the one implementation of those rules rather
    /// than by a shape check written twice.
    pub policy: serde_json::Value,
}

/// A signed kind-44245 event on its way to a reader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyReadRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// The raw signed event, verified here before anything is read off it.
    pub event: serde_json::Value,
}

/// Spending ceilings, every key present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyAdapterBudget {
    /// Ceiling on turns across the umbrella, or null.
    pub turns: Option<u32>,
    /// Ceiling on tokens any one seat may spend, or null.
    pub tokens_per_seat: Option<u64>,
    /// Ceiling on tokens the umbrella may spend, or null.
    pub tokens_per_session: Option<u64>,
    /// Ceiling on dollars the umbrella may spend, or null.
    pub cost_usd_per_session: Option<f64>,
    /// `standard` or `long`, or null.
    pub context_tier: Option<String>,
}

/// What a lane owes before its work counts, every key present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyAdapterGates {
    /// Acceptance tests written failing first, or null.
    pub red_first: Option<bool>,
    /// Every lane reviewed by someone who did not write it, or null.
    pub review_every_lane: Option<bool>,
    /// Named gates every lane must run, or null.
    pub required_gates: Option<Vec<String>>,
    /// A verifier must rule before the mission may settle, or null.
    pub verifier_required: Option<bool>,
}

/// Who the lead may hire from, every key present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyAdapterBench {
    /// Identities eligible for the bench, or null.
    pub identities: Option<Vec<String>>,
    /// Provider **aliases** eligible for the bench, or null.
    pub providers: Option<Vec<String>>,
    /// Fraction of eligible jobs given to a challenger, or null.
    pub challenger_sample_rate: Option<f64>,
}

/// When the mission stops, every key present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyAdapterStop {
    /// Wall-clock seconds after which the lead stops opening work, or null.
    pub time_box_secs: Option<u64>,
    /// The milestone whose arrival ends the mission, or null.
    pub on_milestone: Option<String>,
}

/// One decoded policy, flattened for rendering.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyAdapterRecord {
    /// Umbrella this policy addresses.
    pub session_ref: String,
    /// The umbrella's immutable authority anchor.
    pub genesis_ref: String,
    /// `spike` / `ship` / `investigate` / `overnight`, or null.
    pub posture: Option<String>,
    /// Spending ceilings, or null when the record sets none.
    pub budget: Option<CodingSessionPolicyAdapterBudget>,
    /// `decisions` / `decisions-and-milestones` / `everything`, or null.
    pub attention: Option<String>,
    /// Gate requirements, or null when the record sets none.
    pub gates: Option<CodingSessionPolicyAdapterGates>,
    /// Bench eligibility, or null when the record sets none.
    pub bench: Option<CodingSessionPolicyAdapterBench>,
    /// Acts that need the founder's word, or null.
    pub irreversible: Option<Vec<String>>,
    /// Stop conditions, or null when the record sets none.
    pub stop: Option<CodingSessionPolicyAdapterStop>,
    /// False exactly for the withdrawal record — a decision, never "unknown".
    pub sets_any_policy: bool,
}

/// The unsigned kind-44245 event, plus what it says.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyBuildResponse {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose rules produced this, never "desktop".
    pub implementation: String,
    /// Kind integer as `buzz-core` allocated it; the caller never hard-codes it.
    pub kind: u16,
    /// Rust's own serialization of the policy — the bytes that get signed.
    pub content: String,
    /// The exact four-tag envelope, in NIP-CSP's order.
    pub tags: Vec<Vec<String>>,
    /// What those bytes say, already decoded.
    pub record: CodingSessionPolicyAdapterRecord,
}

/// One signed policy, read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyReadResponse {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose rules produced this, never "desktop".
    pub implementation: String,
    /// The event this record was read from.
    pub event_id: String,
    /// Who signed it. Standing is the fold's question, not this boundary's.
    pub author_pubkey: String,
    /// Seconds since the epoch, as the signer stamped it.
    pub created_at: u64,
    /// What the event says.
    pub record: CodingSessionPolicyAdapterRecord,
}

/// Read a closed-vocabulary word back out as the string the wire carries.
///
/// Going through serde rather than a hand-written match is deliberate: a
/// fifth posture added in `buzz-core` reaches this adapter with no edit, and
/// cannot arrive here spelled differently from the way it is signed.
fn vocabulary_word<T: Serialize>(label: &str, value: &T) -> Result<String, String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => Ok(word),
        _ => Err(format!("{label} is not a closed-vocabulary word")),
    }
}

fn flatten(
    payload: &CodingSessionPolicyPayload,
) -> Result<CodingSessionPolicyAdapterRecord, String> {
    let posture = match &payload.posture {
        Some(value) => Some(vocabulary_word("posture", value)?),
        None => None,
    };
    let attention = match &payload.attention {
        Some(value) => Some(vocabulary_word("attention", value)?),
        None => None,
    };
    let budget = match &payload.budget {
        Some(budget) => {
            let context_tier = match &budget.context_tier {
                Some(value) => Some(vocabulary_word("budget.contextTier", value)?),
                None => None,
            };
            Some(CodingSessionPolicyAdapterBudget {
                turns: budget.turns,
                tokens_per_seat: budget.tokens_per_seat,
                tokens_per_session: budget.tokens_per_session,
                cost_usd_per_session: budget.cost_usd_per_session,
                context_tier,
            })
        }
        None => None,
    };
    let irreversible = match &payload.irreversible {
        Some(acts) => Some(
            acts.iter()
                .map(|act| vocabulary_word("irreversible", act))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        None => None,
    };
    Ok(CodingSessionPolicyAdapterRecord {
        session_ref: payload.session_ref.clone(),
        genesis_ref: payload.genesis_ref.clone(),
        posture,
        budget,
        attention,
        gates: payload
            .gates
            .as_ref()
            .map(|gates| CodingSessionPolicyAdapterGates {
                red_first: gates.red_first,
                review_every_lane: gates.review_every_lane,
                required_gates: gates.required_gates.clone(),
                verifier_required: gates.verifier_required,
            }),
        bench: payload
            .bench
            .as_ref()
            .map(|bench| CodingSessionPolicyAdapterBench {
                identities: bench.identities.clone(),
                providers: bench.providers.clone(),
                challenger_sample_rate: bench.challenger_sample_rate,
            }),
        irreversible,
        stop: payload
            .stop
            .as_ref()
            .map(|stop| CodingSessionPolicyAdapterStop {
                time_box_secs: stop.time_box_secs,
                on_milestone: stop.on_milestone.clone(),
            }),
        sets_any_policy: payload.sets_any_policy(),
    })
}

fn build_adapter(
    request: CodingSessionPolicyBuildRequest,
) -> Result<CodingSessionPolicyBuildResponse, String> {
    if request.schema != CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA}"
        ));
    }
    // The caller's object is turned into bytes and read back by the strict
    // decoder before anything else happens, so the refusal a person sees is
    // the decoder's own sentence naming the offending key.
    let drafted = serde_json::to_string(&request.policy)
        .map_err(|error| format!("policy could not be serialized: {error}"))?;
    let payload = decode_coding_session_policy(&drafted)?;
    let builder = build_coding_session_policy(&request.channel_ref, payload)
        .map_err(|error| error.to_string())?;
    // `EventBuilder` does not expose its parts, so it is sealed against a
    // throwaway key and only the *kind, content and tags* are read back. That
    // signature never leaves this function and is never published: the desktop
    // keyring signs the real event, exactly as it does for every other
    // coding-session kind.
    let scratch = builder
        .sign_with_keys(&nostr::Keys::generate())
        .map_err(|error| format!("policy event could not be sealed: {error}"))?;
    let record = flatten(&decode_coding_session_policy(&scratch.content)?)?;
    Ok(CodingSessionPolicyBuildResponse {
        schema: CODING_SESSION_POLICY_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        kind: scratch.kind.as_u16(),
        content: scratch.content.clone(),
        tags: scratch
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        record,
    })
}

fn read_adapter(
    request: CodingSessionPolicyReadRequest,
) -> Result<CodingSessionPolicyReadResponse, String> {
    if request.schema != CODING_SESSION_POLICY_READ_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_POLICY_READ_REQUEST_SCHEMA}"
        ));
    }
    let event: Event = serde_json::from_value(request.event)
        .map_err(|error| format!("event is not a signed Nostr event: {error}"))?;
    // Signature first. A record read off an unverified event would be a claim
    // about who set a policy, made by nobody.
    event
        .verify()
        .map_err(|error| format!("event signature is invalid: {error}"))?;
    let payload = validate_coding_session_policy_envelope(&event)?;
    Ok(CodingSessionPolicyReadResponse {
        schema: CODING_SESSION_POLICY_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        event_id: event.id.to_hex(),
        author_pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        record: flatten(&payload)?,
    })
}

/// Validate a draft policy and return the unsigned kind-44245 event.
#[tauri::command]
pub async fn build_coding_session_policy_event(
    request: CodingSessionPolicyBuildRequest,
) -> Result<CodingSessionPolicyBuildResponse, String> {
    tauri::async_runtime::spawn_blocking(move || build_adapter(request))
        .await
        .map_err(|error| format!("session-policy build task failed: {error}"))?
}

/// Verify a signed kind-44245 event and return what it says.
#[tauri::command]
pub async fn decode_coding_session_policy_record(
    request: CodingSessionPolicyReadRequest,
) -> Result<CodingSessionPolicyReadResponse, String> {
    tauri::async_runtime::spawn_blocking(move || read_adapter(request))
        .await
        .map_err(|error| format!("session-policy read task failed: {error}"))?
}

#[cfg(test)]
#[path = "coding_session_policy_tests.rs"]
mod tests;
