//! Native builder for the NIP-CSTX team transactions the founder signs in the app.
//!
//! Live run 2 ended with the founder answering three rulings from a terminal
//! while the app that *showed* the question could not take the answer. This is
//! the boundary that closes that gap, and it is deliberately the same shape as
//! [`super::coding_session_policy::build_coding_session_policy_event`]: a
//! caller hands over one content object, `buzz-core` decodes it under its own
//! exact-key rules, `buzz-sdk` builds the exact five-tag envelope, and what
//! comes back is **Rust's own serialization** — the bytes the desktop keyring
//! then signs. TypeScript never serializes a 44244 body, so a producer and a
//! consumer that disagree about those bytes cannot survive one answer.
//!
//! Two commands, because a form has to know what the wire accepts *before* it
//! offers a field:
//!
//! * [`build_coding_session_team_transaction_event`] — a body in, an unsigned
//!   event out.
//! * [`coding_session_team_transaction_capabilities`] — which optional keys
//!   this build's `buzz-core` actually accepts, measured by **running its
//!   decoder**, never by a version number or a hard-coded list. `condition`
//!   (batch 3 §1k, lane L7) reaches the wire the day core carries it and is
//!   omitted before that, with no edit here and no edit in TypeScript.

use buzz_core_pkg::coding_session_team_transaction::{
    decode_coding_session_team_transaction, CodingSessionTeamTransactionPayload,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_sdk_pkg::coding_session_team_transaction::build_coding_session_team_transaction;
use serde::{Deserialize, Serialize};

/// Closed wire-schema identifier accepted by the build boundary.
pub const CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA: &str =
    "buzz-coding-session-team-transaction-build-request/v1";
/// Closed wire-schema identifier this native adapter answers with.
pub const CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA: &str =
    "buzz-coding-session-team-transaction-adapter/v1";

/// A caller's draft transaction body, before anything has validated it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamTransactionBuildRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// Canonical channel UUID for the `h` tag.
    pub channel_ref: String,
    /// The whole NIP-CSTX content object, exactly as it should be signed.
    pub transaction: serde_json::Value,
}

/// What the built bytes say, read back out of the bytes themselves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamTransactionBuildRecord {
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Immutable session-genesis event id.
    pub genesis_ref: String,
    /// The signed operation word, e.g. `decision.answer`.
    #[serde(rename = "type")]
    pub transaction_type: String,
    /// The operation body as **Rust** serialized it.
    ///
    /// Echoed as JSON rather than a typed struct on purpose: an optional key
    /// core gains later (§1k's `condition`) shows up here the day it lands,
    /// without this adapter having a field for it first.
    pub body: serde_json::Value,
}

/// The unsigned kind-44244 event, plus what it says.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamTransactionBuildResponse {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose rules produced this, never "desktop".
    pub implementation: String,
    /// Kind integer as `buzz-core` allocated it; the caller never hard-codes it.
    pub kind: u16,
    /// Rust's own serialization of the transaction — the bytes that get signed.
    pub content: String,
    /// The exact five-tag envelope, in NIP-CSTX's order.
    pub tags: Vec<Vec<String>>,
    /// What those bytes say, already decoded.
    pub record: CodingSessionTeamTransactionBuildRecord,
}

/// Which optional keys this build's `buzz-core` accepts on a `decision.answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionTeamTransactionCapabilities {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose decoder was measured, never "desktop".
    pub implementation: String,
    /// Longest free-text `choice` this build's core accepts, in bytes.
    pub choice_max_bytes: usize,
    /// Longest `note` this build's core accepts, in bytes.
    pub note_max_bytes: usize,
    /// Longest `condition` §1k allows, in bytes.
    pub condition_max_bytes: usize,
    /// Whether a `decision.answer` may carry §1k's `condition` key.
    ///
    /// **Measured, not declared.** The decoder is handed a canonical answer
    /// carrying the key; `true` means it accepted it. A form that offered the
    /// field against a core that refuses it would build an answer the relay
    /// rejects — which is worse than not offering it, because the ruling then
    /// does not reach the wire at all.
    pub supports_decision_answer_condition: bool,
}

/// Longest free-text choice `buzz-core` accepts, echoed for a form's counter.
///
/// Read from the crate's own constant rather than repeated: a bound this side
/// spells for itself is a second implementation of the rule that refuses it.
pub const CODING_SESSION_DECISION_CHOICE_MAX_BYTES: usize =
    buzz_core_pkg::coding_session_team_transaction::MAX_TEAM_TRANSACTION_SHORT_TEXT_BYTES;
/// Longest `note` `buzz-core` accepts on a `decision.answer`.
pub const CODING_SESSION_DECISION_NOTE_MAX_BYTES: usize =
    buzz_core_pkg::coding_session_team_transaction::MAX_TEAM_TRANSACTION_TEXT_BYTES;
/// Longest `condition` §1k allows on a `decision.answer`.
///
/// JOIN(L7): when `buzz-core` gains
/// `MAX_TEAM_TRANSACTION_DECISION_CONDITION_BYTES`, replace the literal below
/// with that constant, exactly as the two constants above already alias
/// core's. Until then the number is only ever used to refuse *before*
/// signing; core's own decoder is still the thing that decides, and while it
/// refuses the key outright the field is never offered at all.
pub const CODING_SESSION_DECISION_CONDITION_MAX_BYTES: usize = 512;

/// A canonical `decision.answer` used only to ask the decoder a question.
///
/// Every value is a well-formed placeholder, so the *only* reason the decoder
/// can refuse it is the key under test.
fn condition_probe(with_condition: bool) -> String {
    let mut body = serde_json::json!({
        "requestRef": "22".repeat(32),
        "choice": 0,
        "note": serde_json::Value::Null,
    });
    if with_condition {
        body["condition"] = serde_json::Value::String("while the branch is red".to_owned());
    }
    serde_json::json!({
        "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
        "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        "genesisRef": "ab".repeat(32),
        "type": "decision.answer",
        "supersedes": serde_json::Value::Null,
        "deliveryCommandId": serde_json::Value::Null,
        "body": body,
    })
    .to_string()
}

/// Ask this build's decoder whether §1k's `condition` key is accepted yet.
pub fn decision_answer_condition_is_supported() -> bool {
    // Both probes are run: a core that refuses the *six*-key body too would be
    // a broken build, and reporting `false` from that would look like an
    // ordinary "not landed yet" rather than the failure it is.
    decode_coding_session_team_transaction(&condition_probe(false)).is_ok()
        && decode_coding_session_team_transaction(&condition_probe(true)).is_ok()
}

fn build_adapter(
    request: CodingSessionTeamTransactionBuildRequest,
) -> Result<CodingSessionTeamTransactionBuildResponse, String> {
    if request.schema != CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA}"
        ));
    }
    // The caller's object becomes bytes and is read back by the strict decoder
    // before anything else happens, so the refusal a person sees is the
    // decoder's own sentence naming the offending key.
    let drafted = serde_json::to_string(&request.transaction)
        .map_err(|error| format!("transaction could not be serialized: {error}"))?;
    let payload: CodingSessionTeamTransactionPayload =
        decode_coding_session_team_transaction(&drafted)?;
    let builder = build_coding_session_team_transaction(&request.channel_ref, payload)
        .map_err(|error| error.to_string())?;
    // `EventBuilder` does not expose its parts, so it is sealed against a
    // throwaway key and only the *kind, content and tags* are read back. That
    // signature never leaves this function and is never published: the desktop
    // keyring signs the real event, exactly as it does for a 44245.
    let scratch = builder
        .sign_with_keys(&nostr::Keys::generate())
        .map_err(|error| format!("team transaction could not be sealed: {error}"))?;
    let signed: serde_json::Value = serde_json::from_str(&scratch.content)
        .map_err(|error| format!("built content is not JSON: {error}"))?;
    let read = |key: &str| -> Result<String, String> {
        signed
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("built content is missing {key}"))
    };
    Ok(CodingSessionTeamTransactionBuildResponse {
        schema: CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        kind: scratch.kind.as_u16(),
        record: CodingSessionTeamTransactionBuildRecord {
            session_ref: read("sessionRef")?,
            genesis_ref: read("genesisRef")?,
            transaction_type: read("type")?,
            body: signed
                .get("body")
                .cloned()
                .ok_or_else(|| "built content is missing body".to_owned())?,
        },
        content: scratch.content.clone(),
        tags: scratch
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
    })
}

/// Validate a draft team transaction and return the unsigned kind-44244 event.
#[tauri::command]
pub async fn build_coding_session_team_transaction_event(
    request: CodingSessionTeamTransactionBuildRequest,
) -> Result<CodingSessionTeamTransactionBuildResponse, String> {
    tauri::async_runtime::spawn_blocking(move || build_adapter(request))
        .await
        .map_err(|error| format!("team-transaction build task failed: {error}"))?
}

/// Report which optional `decision.answer` keys this build's core accepts.
#[tauri::command]
pub async fn coding_session_team_transaction_capabilities(
) -> Result<CodingSessionTeamTransactionCapabilities, String> {
    tauri::async_runtime::spawn_blocking(|| CodingSessionTeamTransactionCapabilities {
        schema: CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        choice_max_bytes: CODING_SESSION_DECISION_CHOICE_MAX_BYTES,
        note_max_bytes: CODING_SESSION_DECISION_NOTE_MAX_BYTES,
        condition_max_bytes: CODING_SESSION_DECISION_CONDITION_MAX_BYTES,
        supports_decision_answer_condition: decision_answer_condition_is_supported(),
    })
    .await
    .map_err(|error| format!("team-transaction capability probe failed: {error}"))
}

#[cfg(test)]
#[path = "coding_session_team_transaction_tests.rs"]
mod tests;
