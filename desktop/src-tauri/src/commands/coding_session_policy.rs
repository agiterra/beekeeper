//! Native adapter for NIP-CSP session policy records (kind 44245).
//!
//! Desktop never encodes or decodes a policy itself. The launch form collects
//! the answers, hands them here as one object, and gets back the exact bytes
//! `buzz-sdk` would sign plus a flattened record for rendering; the Inspector
//! hands a signed event here and gets back the same record. Every bound,
//! closed vocabulary, exact-key rule and refusal sentence belongs to
//! `beekeeper_core::coding_session_policy` — a TypeScript copy of any of them would
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

use beekeeper_core_pkg::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use beekeeper_core_pkg::coding_session_policy::{
    decode_coding_session_policy, fold_coding_session_policies, signer_may_steer_at,
    validate_coding_session_policy_envelope, CodingSessionPolicyGrant, CodingSessionPolicyPayload,
};
use beekeeper_core_pkg::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;
use beekeeper_sdk_pkg::coding_session_policy::build_coding_session_policy;
use nostr::Event;
use serde::{Deserialize, Serialize};

/// Closed wire-schema identifier accepted by the build boundary.
pub const CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA: &str =
    "buzz-coding-session-policy-build-request/v1";
/// Closed wire-schema identifier accepted by the decode boundary.
pub const CODING_SESSION_POLICY_READ_REQUEST_SCHEMA: &str =
    "buzz-coding-session-policy-read-request/v1";
/// Closed wire-schema identifier accepted by the fold boundary.
pub const CODING_SESSION_POLICY_FOLD_REQUEST_SCHEMA: &str =
    "buzz-coding-session-policy-fold-request/v1";
/// Closed wire-schema identifier this native adapter answers with.
pub const CODING_SESSION_POLICY_ADAPTER_SCHEMA: &str = "buzz-coding-session-policy-adapter/v1";
/// Closed wire-schema identifier the fold boundary answers with.
pub const CODING_SESSION_POLICY_FOLD_ADAPTER_SCHEMA: &str =
    "buzz-coding-session-policy-fold-adapter/v1";

/// The one sentence every surface rendering a policy owes its reader.
///
/// Byte-identical to `bee sessions policy get`'s own `enforcement` field
/// (`crates/beekeeper-cli/src/commands/sessions/policy.rs`). Repeated here rather
/// than imported because Desktop does not depend on the CLI crate; the test
/// `the_enforcement_sentence_is_the_clis_own` holds the two together.
pub const POLICY_ENFORCEMENT_DISCLOSURE: &str =
    "Enforced: budget.turns at the provider's turn gate, gates.verifierRequired at the fold's \
     completion check and at the relay's verdict-gated push, and gates.requiredGates at that \
     push. Every other field is read and shown, never counted.";

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

/// One accepted authority transition, as the caller's own projection has it.
///
/// `accepted_at` is the **relay receipt's** `created_at`, not the transition's
/// own: acceptance is what put the grant in the canonical chain, and standing
/// is evaluated at the policy record's time against that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyFoldGrant {
    /// Event id of the signed kind-44228 this projection read the grant from.
    ///
    /// Required, and the whole point of REVIEW-L2 F15: it is what lets this
    /// boundary check a claimed grant against a **signed** transition instead
    /// of trusting a TypeScript projection's word for it.
    pub transition_event_id: String,
    /// Pubkey the transition grants or revokes.
    pub grantee: String,
    /// Seconds since the epoch at which the relay accepted it.
    pub accepted_at: u64,
    /// The transition's own wire word.
    pub transition_type: String,
}

/// One claimed grant this boundary refused, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyRefusedGrant {
    /// The kind-44228 the caller's projection named.
    pub transition_event_id: String,
    /// One sentence naming the rule it failed.
    pub reason: String,
}

/// Every published 44245 for one umbrella, on its way to the fold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyFoldRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// Umbrella this fold is scoped to.
    pub session_ref: String,
    /// The umbrella's immutable authority anchor.
    pub genesis_ref: String,
    /// The umbrella's founder, who may always set policy.
    pub founder_pubkey: String,
    /// The accepted authority chain, in accepted order.
    pub grants: Vec<CodingSessionPolicyFoldGrant>,
    /// The signed kind-44228 events those grants were projected from.
    ///
    /// REVIEW-L2 F15, corrected by REVIEW-L5 F3. Every entry in `grants` must
    /// be supported by one of these — right id, right grantee, right verb,
    /// **and the right umbrella** — verified here.
    ///
    /// What that buys, stated exactly: this boundary **cannot invent a grant**.
    /// It can still be handed a chain that *omits* one, so any refusal at all
    /// drops the whole projection rather than applying a partial chain whose
    /// missing link might have been a revoke. `accepted_at` remains the
    /// caller's unverified word — acceptance is a fact about a relay receipt
    /// this boundary is not given. **Lifting
    /// `crates/beekeeper-session-provider/src/authority.rs` into `buzz-core`, so
    /// provider, CLI and Desktop share one chain, is the real fix**; this is a
    /// narrowing, not a closure.
    pub transitions: Vec<serde_json::Value>,
    /// Raw signed kind-44245 events, verified here before any is read.
    pub events: Vec<serde_json::Value>,
}

/// The policy in force, with its provenance.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyFoldSelected {
    /// The event the record was read from.
    pub event_id: String,
    /// Who signed it.
    pub author_pubkey: String,
    /// Whether that signer is the umbrella's founder.
    pub author_is_founder: bool,
    /// Seconds since the epoch, as the signer stamped it.
    pub created_at: u64,
    /// What it says.
    pub record: CodingSessionPolicyAdapterRecord,
}

/// One published record this fold refused, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyFoldExclusion {
    /// The refused event.
    pub event_id: String,
    /// Who signed it.
    pub author_pubkey: String,
    /// Seconds since the epoch, as the signer stamped it.
    pub created_at: u64,
    /// The fold's own closed code word.
    pub code: String,
    /// One sentence naming the rule it failed.
    pub reason: String,
}

/// Newest-accepted-wins, plus everything it refused.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyFoldResponse {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose rules produced this, never "desktop".
    pub implementation: String,
    /// The policy in force, or null when nobody with standing set one.
    pub selected: Option<CodingSessionPolicyFoldSelected>,
    /// Every refused record, newest first. Never empty-by-omission.
    pub excluded: Vec<CodingSessionPolicyFoldExclusion>,
    /// Claimed grants no verified transition supported. Present even when
    /// empty: a caller must be able to tell "none were refused" from "this
    /// build does not check".
    pub refused_grants: Vec<CodingSessionPolicyRefusedGrant>,
    /// The enforcement sentence, byte-identical to the CLI's.
    pub enforcement: String,
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

/// One verified kind-44228, reduced to the two facts a claimed grant asserts.
struct SignedTransition {
    grantee: String,
    transition_type: CodingSessionAuthorityTransitionType,
    /// The umbrella the transition itself names (REVIEW-L5 F3).
    genesis_ref: String,
}

/// Verify every supplied 44228 and index it by event id.
///
/// Signature first, then the envelope validator `buzz-core` already owns. A
/// transition that fails either is simply not in the index, so any grant
/// claiming it is refused by name rather than quietly believed.
fn verified_transitions(
    values: &[serde_json::Value],
) -> Result<std::collections::HashMap<String, SignedTransition>, String> {
    let mut index = std::collections::HashMap::new();
    for value in values {
        let event: Event = serde_json::from_value(value.clone())
            .map_err(|error| format!("transitions[] is not a signed Nostr event: {error}"))?;
        // Wrong kind, bad signature or unreadable content: not in the index,
        // so a grant claiming it is refused by name rather than believed.
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_AUTHORITY_TRANSITION
            || event.verify().is_err()
        {
            continue;
        }
        let Ok(payload) = decode_coding_session_authority_transition(&event.content) else {
            continue;
        };
        index.insert(
            event.id.to_hex(),
            SignedTransition {
                grantee: payload.grantee_pubkey.clone(),
                transition_type: payload.transition_type,
                genesis_ref: payload.genesis_ref.clone(),
            },
        );
    }
    Ok(index)
}

fn transition_type(word: &str) -> Result<CodingSessionAuthorityTransitionType, String> {
    // Through serde, so the words this adapter accepts are exactly the words
    // `buzz-core` signs — a hand-written match here would be a second spelling
    // of a closed vocabulary.
    serde_json::from_value(serde_json::Value::String(word.to_owned()))
        .map_err(|_| format!("grants[].transitionType is not an authority transition: {word}"))
}

fn fold_adapter(
    request: CodingSessionPolicyFoldRequest,
) -> Result<CodingSessionPolicyFoldResponse, String> {
    if request.schema != CODING_SESSION_POLICY_FOLD_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_POLICY_FOLD_REQUEST_SCHEMA}"
        ));
    }
    // REVIEW-L2 F15. The standing rule is applied in Rust but was handed a
    // grant list derived by `lib/codingSessionMissionAuthority.ts`, so a drift
    // there made Desktop disagree with the provider about whose ceiling counts.
    // Each claimed grant is now held against the **signed** 44228 it names.
    let signed = verified_transitions(&request.transitions)?;
    let mut refused_grants: Vec<CodingSessionPolicyRefusedGrant> = Vec::new();
    let mut grants: Vec<CodingSessionPolicyGrant> = Vec::new();
    for grant in &request.grants {
        let transition = transition_type(&grant.transition_type)?;
        match signed.get(&grant.transition_event_id) {
            None => refused_grants.push(CodingSessionPolicyRefusedGrant {
                transition_event_id: grant.transition_event_id.clone(),
                reason: "no verified kind-44228 with this id was supplied, so this grant is a \
                         claim no signature supports"
                    .to_owned(),
            }),
            Some(signed_grant) if signed_grant.genesis_ref != request.genesis_ref => {
                // REVIEW-L5 F3. Kind, signature and decodability said nothing
                // about *which umbrella* the transition belongs to, and the
                // projection and the transition list come from the same caller
                // — so a real 44228 from another session used to support a
                // grant here.
                refused_grants.push(CodingSessionPolicyRefusedGrant {
                    transition_event_id: grant.transition_event_id.clone(),
                    reason: format!(
                        "the signed transition belongs to another umbrella ({}), not this one",
                        signed_grant.genesis_ref
                    ),
                });
            }
            Some(signed_grant) => {
                if signed_grant.grantee != grant.grantee {
                    refused_grants.push(CodingSessionPolicyRefusedGrant {
                        transition_event_id: grant.transition_event_id.clone(),
                        reason: format!(
                            "the signed transition grants {}, not {}",
                            signed_grant.grantee, grant.grantee
                        ),
                    });
                } else if signed_grant.transition_type != transition {
                    refused_grants.push(CodingSessionPolicyRefusedGrant {
                        transition_event_id: grant.transition_event_id.clone(),
                        reason: format!(
                            "the signed transition is a {}, not a {}",
                            vocabulary_word("transitionType", &signed_grant.transition_type)?,
                            grant.transition_type
                        ),
                    });
                } else {
                    grants.push(CodingSessionPolicyGrant {
                        grantee: grant.grantee.clone(),
                        // The relay receipt's `created_at` is the caller's:
                        // acceptance is a fact about a receipt this boundary
                        // was not given, and re-deriving it here would be the
                        // second chain implementation F15 forbids.
                        accepted_at: grant.accepted_at,
                        transition_type: transition,
                    });
                }
            }
        }
    }
    // REVIEW-L5 F3, second half: **standing needs the whole chain**.
    // `fold_coding_session_policies` turns standing *off* on a `Revoke`, so a
    // projection that omits a revoke link would leave its grantee steering.
    // A chain with a hole in it cannot be evaluated at all, so none of it is
    // applied: the founder keeps standing (which needs no grant) and everybody
    // else loses it. Failing closed is the only direction that cannot be used.
    if !refused_grants.is_empty() {
        grants.clear();
    }

    // Signature first, and separately from the fold: `fold_coding_session_policies`
    // adjudicates *standing*, not authenticity, so an unverified event reaching
    // it would be a claim about who set a policy made by nobody. A record that
    // fails here is listed as refused with the fold's own `undecodable` word
    // rather than dropped — a forged ceiling in a channel is a fact its reader
    // needs, and silence would make it look like no record at all.
    let mut verified: Vec<Event> = Vec::new();
    let mut excluded: Vec<CodingSessionPolicyFoldExclusion> = Vec::new();
    for value in request.events {
        let event: Event = match serde_json::from_value(value) {
            Ok(event) => event,
            Err(error) => {
                return Err(format!("events[] is not a signed Nostr event: {error}"));
            }
        };
        match event.verify() {
            Ok(()) => verified.push(event),
            Err(error) => excluded.push(CodingSessionPolicyFoldExclusion {
                event_id: event.id.to_hex(),
                author_pubkey: event.pubkey.to_hex(),
                created_at: event.created_at.as_secs(),
                code: "undecodable".to_owned(),
                reason: format!("event signature is invalid: {error}"),
            }),
        }
    }
    let founder = request.founder_pubkey.clone();
    let fold = fold_coding_session_policies(
        &verified,
        &request.session_ref,
        &request.genesis_ref,
        &founder,
        &|author, created_at| signer_may_steer_at(author, created_at, &founder, &grants),
    );
    let selected = match fold.selected.as_ref() {
        Some(selected) => Some(CodingSessionPolicyFoldSelected {
            event_id: selected.event_id.clone(),
            author_pubkey: selected.author.clone(),
            author_is_founder: selected.author_is_founder,
            created_at: selected.created_at,
            record: flatten(&selected.record)?,
        }),
        None => None,
    };
    excluded.extend(
        fold.excluded
            .iter()
            .map(|item| CodingSessionPolicyFoldExclusion {
                event_id: item.event_id.clone(),
                author_pubkey: item.author.clone(),
                created_at: item.created_at,
                code: item.code.as_str().to_owned(),
                reason: item.reason.clone(),
            }),
    );
    excluded.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.event_id.cmp(&left.event_id))
    });
    refused_grants.sort_by(|left, right| left.transition_event_id.cmp(&right.transition_event_id));
    Ok(CodingSessionPolicyFoldResponse {
        schema: CODING_SESSION_POLICY_FOLD_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        selected,
        excluded,
        refused_grants,
        enforcement: POLICY_ENFORCEMENT_DISCLOSURE.to_owned(),
    })
}

/// Fold an umbrella's published kind-44245 records into the one in force.
///
/// The same `buzz-core` fold and the same standing rule the session provider
/// and `bee sessions policy get` use, so Desktop cannot give a third answer
/// about whose ceiling is real (REVIEW-B2 F1).
#[tauri::command]
pub async fn fold_coding_session_policies_command(
    request: CodingSessionPolicyFoldRequest,
) -> Result<CodingSessionPolicyFoldResponse, String> {
    tauri::async_runtime::spawn_blocking(move || fold_adapter(request))
        .await
        .map_err(|error| format!("session-policy fold task failed: {error}"))?
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
