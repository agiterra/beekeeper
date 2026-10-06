//! NIP-CSP: the signed session policy record (kind 44245).
//!
//! One record says how a mission is meant to be run — the posture, the budget,
//! how much the founder wants to be told, which gates a lane owes, which
//! identities and providers may be benched against each other, which acts stay
//! irreversible without the founder's word, and when to stop. It is
//! **addressable per umbrella**: `d = sessionRef`, and the newest accepted
//! record wins, exactly as kinds 44227/44229/44230 do. Like them it is a
//! *regular* stored event — the `d` tag groups a session, and never opts this
//! kind into NIP-33 replacement, so every revision stays on the record.
//!
//! Founder- or lead-signed. Authority is **not** checked here: the signature is
//! the author, and whether that author held the standing to set policy is the
//! consumer's fold to answer against the accepted NIP-CSAT chain — the same
//! division kind 44244 draws. This module validates only what is self-contained
//! in one event: exact JSON and tag shape, closed vocabularies, bounds, and
//! tag-to-content parity.
//!
//! # Every field is optional, and absent is not null
//!
//! Only `schema`, `sessionRef` and `genesisRef` are required. Every policy
//! field is **omitted** when it is not set, never written as an explicit
//! `null`, and the decoder rejects the null (the item-102 additive-shape rule:
//! absent and null are different claims, and a reader that silently reads one
//! as the other is a reader that disagrees with its peer about the same signed
//! bytes).
//!
//! A record carrying no policy field at all is **accepted**, and means exactly
//! what it says: this umbrella's policy is now empty. That is the honest way to
//! clear a policy under a newest-wins fold — a deletion nobody can express is a
//! policy nobody can withdraw.
//!
//! # Unknown fields are rejected in v1
//!
//! A consumer that tolerated an unknown key would be claiming to enforce a
//! policy it cannot read. See `docs/design/portable-team-loop/POLICY.md` for
//! each field's consumer and the reason v1 refuses rather than ignores.

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::coding_session_command::MAX_IDENTIFIER_BYTES;
use crate::coding_session_identity::ProviderInstanceAlias;
use crate::kind::KIND_CODING_SESSION_POLICY;

// The newest-accepted-wins fold, and the standing rule every consumer shares,
// live in a sibling file so no file here passes 1,000 lines.
#[path = "coding_session_policy_fold.rs"]
mod fold;
pub use fold::*;

/// Exact v1 schema identifier, carried in content and in the `csp-v` tag.
pub const CODING_SESSION_POLICY_SCHEMA: &str = "buzz-coding-session-policy/v1";

/// Maximum UTF-8 byte length of a complete signed policy payload.
///
/// Sized to the worst legal record rather than guessed: 64 identities
/// (66 B each), 16 provider aliases (258 B each), 32 required gates (66 B
/// each), an 8 KiB milestone, and the keys around them come to roughly 19 KiB.
/// The same 32 KiB the relay already caps kind 44223 at leaves room without
/// leaving room for prose.
pub const MAX_CODING_SESSION_POLICY_CONTENT_BYTES: usize = 32 * 1024;

/// Maximum number of named gates a policy may require.
pub const MAX_POLICY_REQUIRED_GATES: usize = 32;
/// Maximum UTF-8 byte length of one required-gate name.
pub const MAX_POLICY_REQUIRED_GATE_BYTES: usize = 64;
/// Maximum number of identities a policy may put on the bench.
pub const MAX_POLICY_BENCH_IDENTITIES: usize = 64;
/// Maximum number of provider instances a policy may put on the bench.
pub const MAX_POLICY_BENCH_PROVIDERS: usize = 16;
/// Maximum UTF-8 byte length of one benched provider alias.
///
/// Deliberately tighter than a
/// [`ProviderInstanceAlias`]'s own 2 KiB ceiling. Nothing on an existing wire
/// is affected — this kind is new — and sixteen two-kilobyte aliases would be
/// half the record's whole budget spent on names.
pub const MAX_POLICY_BENCH_PROVIDER_BYTES: usize = MAX_IDENTIFIER_BYTES;
/// Maximum UTF-8 byte length of the milestone a policy stops on.
pub const MAX_POLICY_MILESTONE_BYTES: usize = 8 * 1024;

/// How the founder means this mission to be run.
///
/// Consumed by the router: posture selects the risk tier a class is routed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionPosture {
    /// Learn fast, throw away freely.
    Spike,
    /// Land it; correctness and review carry the weight.
    Ship,
    /// Find out what is true; no landing expected.
    Investigate,
    /// Long unattended run; nobody is watching the screen.
    Overnight,
}

/// How much of the mission the founder wants surfaced.
///
/// Consumed by the UI, never by the router: attention changes what a person is
/// shown, never what the machine does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionAttention {
    /// Only decisions that need a person.
    Decisions,
    /// Decisions, plus each milestone as it settles.
    DecisionsAndMilestones,
    /// Every transaction, unfiltered.
    Everything,
}

/// Which context window a seat is meant to run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionContextTier {
    /// The model's ordinary window.
    Standard,
    /// The model's extended window, where it offers one.
    Long,
}

/// An act that stays the founder's to authorize.
///
/// Consumed by the fence. The list is closed in v1 for the same reason the
/// fence is: an act nobody named is an act nobody agreed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionIrreversibleAct {
    /// Pushing commits to a remote.
    Push,
    /// Deploying anything anywhere.
    Deploy,
    /// Deleting data, branches, or history.
    Delete,
    /// Sending a message that reaches a person outside this machine.
    ExternalMessage,
}

impl CodingSessionIrreversibleAct {
    /// The exact wire token for this act.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::Deploy => "deploy",
            Self::Delete => "delete",
            Self::ExternalMessage => "external-message",
        }
    }
}

/// What the mission may spend.
///
/// Consumed by the router. Every field is omitted when unset; a limit of zero
/// is refused rather than stored, because "zero turns" and "no turn limit"
/// would otherwise be the same record read two ways.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyBudget {
    /// Ceiling on turns across the umbrella.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<u32>,
    /// Ceiling on tokens any one seat may spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_seat: Option<u64>,
    /// Ceiling on tokens the umbrella may spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_session: Option<u64>,
    /// Ceiling on US dollars the umbrella may spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd_per_session: Option<f64>,
    /// Which context window seats run in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tier: Option<CodingSessionContextTier>,
}

/// What a lane owes before its work counts as done.
///
/// Consumed by the lead pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyGates {
    /// Every acceptance test is written failing first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_first: Option<bool>,
    /// Every lane's work is reviewed by someone who did not write it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_every_lane: Option<bool>,
    /// Named gates every lane must run, e.g. `just ci`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_gates: Option<Vec<String>>,
    /// A verifier seat must rule before the mission may settle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_required: Option<bool>,
}

/// Who may be compared against whom.
///
/// Consumed by the router. `identities` and `providers` are the *permitted*
/// bench, never a claim that any of them is seated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyBench {
    /// Lowercase 64-hex pubkeys eligible for the bench.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identities: Option<Vec<String>>,
    /// Provider instance **aliases** eligible for the bench — never instance
    /// ids; see [`crate::coding_session_identity`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<String>>,
    /// Fraction of eligible jobs given to a challenger, `0.0..=1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenger_sample_rate: Option<f64>,
}

/// When the mission is meant to stop.
///
/// Consumed by the lead pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyStop {
    /// Wall-clock seconds after which the lead stops opening work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_box_secs: Option<u64>,
    /// The milestone whose arrival ends the mission, in the founder's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_milestone: Option<String>,
}

/// Strict public JSON carried by a kind 44245 event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionPolicyPayload {
    /// Exact schema identifier; must equal [`CODING_SESSION_POLICY_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the umbrella this policy governs.
    pub session_ref: String,
    /// Lowercase 64-hex event id of that umbrella's genesis.
    pub genesis_ref: String,
    /// How the mission is meant to be run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posture: Option<CodingSessionPosture>,
    /// What the mission may spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<CodingSessionPolicyBudget>,
    /// How much the founder wants surfaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<CodingSessionAttention>,
    /// What a lane owes before its work counts as done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<CodingSessionPolicyGates>,
    /// Who may be compared against whom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bench: Option<CodingSessionPolicyBench>,
    /// Acts that stay the founder's to authorize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irreversible: Option<Vec<CodingSessionIrreversibleAct>>,
    /// When the mission is meant to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<CodingSessionPolicyStop>,
}

/// The three keys every policy record carries.
pub const POLICY_REQUIRED_KEYS: &[&str] = &["schema", "sessionRef", "genesisRef"];

/// Every policy key, in wire order. Present exactly when set; never `null`.
pub const POLICY_OPTIONAL_KEYS: &[&str] = &[
    "posture",
    "budget",
    "attention",
    "gates",
    "bench",
    "irreversible",
    "stop",
];

impl CodingSessionPolicyPayload {
    /// A policy that governs `session_ref` under `genesis_ref` and says
    /// nothing else.
    ///
    /// Under a newest-wins fold this is the explicit withdrawal of a policy,
    /// which is why it is a legal record rather than an error.
    pub fn empty(session_ref: impl Into<String>, genesis_ref: impl Into<String>) -> Self {
        Self {
            schema: CODING_SESSION_POLICY_SCHEMA.to_owned(),
            session_ref: session_ref.into(),
            genesis_ref: genesis_ref.into(),
            posture: None,
            budget: None,
            attention: None,
            gates: None,
            bench: None,
            irreversible: None,
            stop: None,
        }
    }

    /// Whether this record sets any policy at all.
    ///
    /// `false` is the withdrawal case above. A consumer that rendered it as
    /// "unknown" rather than "no policy" would be guessing.
    ///
    /// The question is **whether a value is set**, not whether a key is
    /// present. An earlier draft asked `is_some()` on each `Option`, so a
    /// record whose every collection was empty answered `true` — a record that
    /// set nothing claiming to set something, and a withdrawal that could be
    /// silently impersonated (REVIEW-B1 F4). Empty collections are refused by
    /// [`validate`](Self::validate) now, so this and the validator agree; the
    /// checks below are belt and braces for a payload built in memory and
    /// never validated.
    pub fn sets_any_policy(&self) -> bool {
        self.posture.is_some()
            || self.budget.as_ref().is_some_and(|it| it.sets_any_value())
            || self.attention.is_some()
            || self.gates.as_ref().is_some_and(|it| it.sets_any_value())
            || self.bench.as_ref().is_some_and(|it| it.sets_any_value())
            || self
                .irreversible
                .as_ref()
                .is_some_and(|acts| !acts.is_empty())
            || self.stop.as_ref().is_some_and(|it| it.sets_any_value())
    }

    /// Validate field bounds, closed vocabularies, and reference syntax.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_POLICY_SCHEMA {
            return Err("unsupported coding-session policy schema".into());
        }
        validate_canonical_uuid("sessionRef", &self.session_ref)?;
        validate_event_id("genesisRef", &self.genesis_ref)?;
        if let Some(budget) = &self.budget {
            budget.validate()?;
        }
        if let Some(gates) = &self.gates {
            gates.validate()?;
        }
        if let Some(bench) = &self.bench {
            bench.validate()?;
        }
        if let Some(irreversible) = &self.irreversible {
            validate_irreversible(irreversible)?;
        }
        if let Some(stop) = &self.stop {
            stop.validate()?;
        }
        Ok(())
    }
}

impl CodingSessionPolicyBudget {
    /// Whether this sub-object actually sets a value (see
    /// [`CodingSessionPolicyPayload::sets_any_policy`]).
    fn sets_any_value(&self) -> bool {
        self.turns.is_some()
            || self.tokens_per_seat.is_some()
            || self.tokens_per_session.is_some()
            || self.cost_usd_per_session.is_some()
            || self.context_tier.is_some()
    }

    fn validate(&self) -> Result<(), String> {
        if !self.sets_any_value() {
            return Err("budget must carry at least one field".into());
        }
        if self.turns == Some(0) {
            return Err("budget.turns must be at least 1".into());
        }
        if self.tokens_per_seat == Some(0) {
            return Err("budget.tokensPerSeat must be at least 1".into());
        }
        if self.tokens_per_session == Some(0) {
            return Err("budget.tokensPerSession must be at least 1".into());
        }
        if let (Some(seat), Some(session)) = (self.tokens_per_seat, self.tokens_per_session) {
            if seat > session {
                return Err(
                    "budget.tokensPerSeat must not exceed budget.tokensPerSession: a per-seat \
                     ceiling above the whole session's is a limit that cannot bind"
                        .into(),
                );
            }
        }
        if let Some(cost) = self.cost_usd_per_session {
            if !cost.is_finite() || cost <= 0.0 {
                return Err("budget.costUsdPerSession must be a finite amount above zero".into());
            }
        }
        Ok(())
    }
}

impl CodingSessionPolicyGates {
    /// Whether this sub-object actually sets a value. An empty
    /// `requiredGates` sets nothing, so it does not count.
    fn sets_any_value(&self) -> bool {
        self.red_first.is_some()
            || self.review_every_lane.is_some()
            || self
                .required_gates
                .as_ref()
                .is_some_and(|gates| !gates.is_empty())
            || self.verifier_required.is_some()
    }

    fn validate(&self) -> Result<(), String> {
        // The collection check runs *before* the sub-object guard so the
        // author reads the specific sentence -- "gates.requiredGates must not
        // be empty" -- rather than the generic "gates must carry at least one
        // field", which is true but names nothing. An empty array is an empty
        // sub-object one level down, refused with the sentence `irreversible`
        // already uses, so the same argument gets the same answer at every
        // depth (REVIEW-B1 F4).
        if let Some(required) = &self.required_gates {
            validate_non_empty_collection(
                "gates.requiredGates",
                required.len(),
                "require no gate",
            )?;
        }
        if !self.sets_any_value() {
            return Err("gates must carry at least one field".into());
        }
        if let Some(required) = &self.required_gates {
            if required.len() > MAX_POLICY_REQUIRED_GATES {
                return Err(format!(
                    "gates.requiredGates exceeds {MAX_POLICY_REQUIRED_GATES} entries"
                ));
            }
            for gate in required {
                validate_text("gates.requiredGates", gate, MAX_POLICY_REQUIRED_GATE_BYTES)?;
            }
            validate_unique("gates.requiredGates", required)?;
        }
        Ok(())
    }
}

impl CodingSessionPolicyBench {
    /// Whether this sub-object actually sets a value. Empty `identities` and
    /// empty `providers` set nothing, so they do not count.
    fn sets_any_value(&self) -> bool {
        self.identities
            .as_ref()
            .is_some_and(|values| !values.is_empty())
            || self
                .providers
                .as_ref()
                .is_some_and(|values| !values.is_empty())
            || self.challenger_sample_rate.is_some()
    }

    fn validate(&self) -> Result<(), String> {
        // Specific before generic -- see the note in `CodingSessionPolicyGates`.
        if let Some(identities) = &self.identities {
            validate_non_empty_collection(
                "bench.identities",
                identities.len(),
                "bench no identity",
            )?;
        }
        if let Some(providers) = &self.providers {
            validate_non_empty_collection("bench.providers", providers.len(), "bench no provider")?;
        }
        if !self.sets_any_value() {
            return Err("bench must carry at least one field".into());
        }
        if let Some(identities) = &self.identities {
            if identities.len() > MAX_POLICY_BENCH_IDENTITIES {
                return Err(format!(
                    "bench.identities exceeds {MAX_POLICY_BENCH_IDENTITIES} entries"
                ));
            }
            for identity in identities {
                validate_event_id("bench.identities", identity)?;
            }
            validate_unique("bench.identities", identities)?;
        }
        if let Some(providers) = &self.providers {
            if providers.len() > MAX_POLICY_BENCH_PROVIDERS {
                return Err(format!(
                    "bench.providers exceeds {MAX_POLICY_BENCH_PROVIDERS} entries"
                ));
            }
            for provider in providers {
                if provider.len() > MAX_POLICY_BENCH_PROVIDER_BYTES {
                    return Err(format!(
                        "bench.providers exceeds {MAX_POLICY_BENCH_PROVIDER_BYTES} bytes per alias"
                    ));
                }
                // The bench names provider **aliases**, never instance ids.
                ProviderInstanceAlias::from_wire(provider.as_str())
                    .map_err(|error| error.replace("providerInstanceRef", "bench.providers"))?;
            }
            validate_unique("bench.providers", providers)?;
        }
        if let Some(rate) = self.challenger_sample_rate {
            if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
                return Err("bench.challengerSampleRate must be within 0.0..=1.0".into());
            }
        }
        Ok(())
    }
}

impl CodingSessionPolicyStop {
    /// Whether this sub-object actually sets a value.
    fn sets_any_value(&self) -> bool {
        self.time_box_secs.is_some() || self.on_milestone.is_some()
    }

    fn validate(&self) -> Result<(), String> {
        if !self.sets_any_value() {
            return Err("stop must carry at least one field".into());
        }
        if self.time_box_secs == Some(0) {
            return Err("stop.timeBoxSecs must be at least 1".into());
        }
        if let Some(milestone) = &self.on_milestone {
            validate_text("stop.onMilestone", milestone, MAX_POLICY_MILESTONE_BYTES)?;
        }
        Ok(())
    }
}

fn validate_irreversible(acts: &[CodingSessionIrreversibleAct]) -> Result<(), String> {
    if acts.is_empty() {
        return Err(
            "irreversible must not be empty: omit the key to name no irreversible act".into(),
        );
    }
    for (index, act) in acts.iter().enumerate() {
        if acts[..index].contains(act) {
            return Err("irreversible must not contain duplicates".into());
        }
    }
    Ok(())
}

/// Strictly decode and validate signed kind 44245 content.
///
/// Exact keys at every level, unknown keys refused, and an explicit `null` on
/// any optional key refused by name. A second pass through `serde_json` after
/// the shape check preserves serde's duplicate-key detection, which a `Value`
/// map cannot represent — the same two-pass pattern kinds 44221 and 44244 use.
pub fn decode_coding_session_policy(content: &str) -> Result<CodingSessionPolicyPayload, String> {
    if content.len() > MAX_CODING_SESSION_POLICY_CONTENT_BYTES {
        return Err(format!(
            "coding-session policy content exceeds {MAX_CODING_SESSION_POLICY_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session policy payload: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session policy payload must be an object".to_owned())?;
    validate_exact_keys(
        object,
        POLICY_REQUIRED_KEYS,
        POLICY_OPTIONAL_KEYS,
        "policy payload",
    )?;
    for key in POLICY_OPTIONAL_KEYS {
        reject_explicit_null(object, key, "policy payload")?;
    }
    validate_nested_keys(
        object,
        "budget",
        &[
            "turns",
            "tokensPerSeat",
            "tokensPerSession",
            "costUsdPerSession",
            "contextTier",
        ],
    )?;
    validate_nested_keys(
        object,
        "gates",
        &[
            "redFirst",
            "reviewEveryLane",
            "requiredGates",
            "verifierRequired",
        ],
    )?;
    validate_nested_keys(
        object,
        "bench",
        &["identities", "providers", "challengerSampleRate"],
    )?;
    validate_nested_keys(object, "stop", &["timeBoxSecs", "onMilestone"])?;

    let payload: CodingSessionPolicyPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session policy payload: {error}"))?;
    payload.validate()?;
    Ok(payload)
}

/// Validate the exact ordered event envelope and return its decoded payload.
///
/// Four two-field tags, in order: `h` (channel UUID), `d` (the umbrella this
/// policy is addressed by), `csp-v` (the schema), `csp-genesis` (the founding
/// event). The `d` and `csp-genesis` tags must agree with the content, so a
/// policy cannot be filed under one umbrella while claiming another.
pub fn validate_coding_session_policy_envelope(
    event: &Event,
) -> Result<CodingSessionPolicyPayload, String> {
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_POLICY {
        return Err("coding-session policy has the wrong event kind".into());
    }
    let payload = decode_coding_session_policy(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 4 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session policy requires exactly four two-field tags".into());
    }
    if tags[0][0] != "h" {
        return Err("coding-session policy first tag must be h=channel UUID".into());
    }
    validate_canonical_uuid("h", &tags[0][1])?;
    if tags[1][0] != "d" || tags[1][1] != payload.session_ref {
        return Err("coding-session policy d tag does not match payload sessionRef".into());
    }
    if tags[2][0] != "csp-v" || tags[2][1] != CODING_SESSION_POLICY_SCHEMA {
        return Err("unsupported coding-session policy tag version".into());
    }
    if tags[3][0] != "csp-genesis" || tags[3][1] != payload.genesis_ref {
        return Err("coding-session policy genesis tag does not match payload genesisRef".into());
    }
    Ok(payload)
}

fn validate_exact_keys(
    object: &serde_json::Map<String, Value>,
    required: &[&str],
    optional: &[&str],
    field: &str,
) -> Result<(), String> {
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("coding-session {field} is missing {key:?}"));
        }
    }
    for key in object.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(format!(
                "coding-session {field} carries unsupported field {key:?}: v1 rejects unknown \
                 fields rather than ignoring them, because a consumer that ignores a policy key \
                 is claiming to enforce a policy it cannot read"
            ));
        }
    }
    Ok(())
}

fn reject_explicit_null(
    object: &serde_json::Map<String, Value>,
    key: &str,
    field: &str,
) -> Result<(), String> {
    if object.get(key).is_some_and(Value::is_null) {
        return Err(format!(
            "coding-session {field} field {key:?} must be omitted when it is not set, never \
             written as an explicit null"
        ));
    }
    Ok(())
}

fn validate_nested_keys(
    object: &serde_json::Map<String, Value>,
    field: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let Some(nested) = object.get(field) else {
        return Ok(());
    };
    let nested = nested
        .as_object()
        .ok_or_else(|| format!("coding-session policy {field} must be an object"))?;
    validate_exact_keys(nested, &[], allowed, &format!("policy {field}"))?;
    for key in allowed {
        reject_explicit_null(nested, key, &format!("policy {field}"))?;
    }
    Ok(())
}

fn validate_canonical_uuid(field: &str, value: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("{field} must be a UUID"))?;
    if parsed.to_string() != value {
        return Err(format!("{field} must be a lowercase canonical UUID"));
    }
    Ok(())
}

fn validate_event_id(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase 64-hex event id"));
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

/// Refuse an empty collection, telling the author to omit the key instead.
///
/// An empty array is an empty sub-object one level down. `irreversible: []`
/// was already refused this way; `gates.requiredGates`, `bench.identities` and
/// `bench.providers` were not, so a record whose every collection was empty
/// passed the "must carry at least one field" guard and answered
/// `sets_any_policy() == true` — a record that set nothing claiming to set
/// something, and a withdrawal that could be impersonated (REVIEW-B1 F4).
fn validate_non_empty_collection(field: &str, len: usize, omission: &str) -> Result<(), String> {
    if len == 0 {
        return Err(format!(
            "{field} must not be empty: omit the key to {omission}"
        ));
    }
    Ok(())
}

fn validate_unique(field: &str, values: &[String]) -> Result<(), String> {
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Err(format!("{field} must not contain duplicates"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "coding_session_policy_tests.rs"]
mod tests;
