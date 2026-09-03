//! Durable provider-owned wake intents for team transaction progress.
//!
//! Report discovery and crash-safe queueing live in the schema-v3 store. This
//! module owns signed source shapes and the verified routing/delivery protocol.

use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, CodingSessionTarget, CODING_SESSION_COMMAND_SCHEMA,
};
use buzz_core::coding_session_context::{
    CodingSessionContextPackage, CodingSessionContextSeatStatus,
};
use buzz_core::coding_session_genesis::decode_coding_session_genesis;
use buzz_core::coding_session_payload::ReceiptStatus;
use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamActiveGrant,
    CodingSessionTeamActiveSeat, CodingSessionTeamFoldContext, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionType,
};
use nostr::{Event, Keys};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::authority::CurrentAuthority;
use crate::context_projector::{
    fetch_and_project_session_context, query_complete_kind_partition, ContextProjectionError,
    ContextProjectionLimits, ContextProjectionRequest,
};

#[path = "team_wake_report_order.rs"]
mod report_order;
pub use report_order::{included_reports, report_suppresses_terminal, IncludedReport};

#[path = "team_wake_store.rs"]
mod store;
pub use store::{ChannelRefusalCode, DiscoveryCapture, WakeIntentStore};

/// Exact durable scope shared by one team transaction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeScope {
    pub channel_ref: Uuid,
    pub session_ref: String,
    pub genesis_ref: String,
}

/// Signed fact that made the provider owe a lead wake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WakeSource {
    Report {
        operation_id: String,
        operation_type: String,
        author_pubkey: String,
        created_at: u64,
    },
    Terminal {
        terminal_event_id: String,
        actor_pubkey: String,
        role: String,
        caused_by_command_id: String,
        source_target: CodingSessionTarget,
        prompt_at_ms: Option<i64>,
        terminal_at_ms: i64,
    },
}

impl WakeSource {
    pub fn event_id(&self) -> &str {
        match self {
            Self::Report { operation_id, .. } => operation_id,
            Self::Terminal {
                terminal_event_id, ..
            } => terminal_event_id,
        }
    }
}

/// One pending wake and its current exact delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeIntent {
    pub scope: WakeScope,
    pub source: WakeSource,
    pub target: Option<CodingSessionTarget>,
    pub command_id: Option<String>,
    pub signed_event: Option<Event>,
    pub relay_accepted_at: Option<u64>,
    pub attempt: u32,
    pub last_reason: Option<String>,
    /// Bounded, control-free cause for the stable reason above.
    ///
    /// This lives with the durable intent because logs can be absent or
    /// rotated while the store is the artifact an operator can still inspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reason_detail: Option<String>,
}

/// Keep one diagnostic useful without letting relay/error text grow the store.
pub fn bounded_reason_detail(detail: &str) -> Option<String> {
    const MAX_BYTES: usize = 1_024;
    let cleaned: String = detail
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        return None;
    }
    let mut end = cleaned.len().min(MAX_BYTES);
    while !cleaned.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    Some(cleaned[..end].to_owned())
}

/// Build the exact core fold context from verified genesis and authority facts.
pub fn fold_context(
    scope: &WakeScope,
    founder_pubkey: &str,
    authority: &CurrentAuthority,
) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: scope.channel_ref.to_string(),
        session_ref: scope.session_ref.clone(),
        genesis_ref: scope.genesis_ref.clone(),
        founder_pubkey: founder_pubkey.to_owned(),
        // This projection reads kind 44228 and 44244, never 44245, so it makes
        // no claim about `gates.verifierRequired`. `false` keeps the wake fold
        // behaving exactly as it did before the field existed; a wake is a
        // delivery decision, not a governance verdict.
        verifier_required: false,
        active_seats: authority
            .seats
            .iter()
            .map(|(actor_pubkey, (role, _))| CodingSessionTeamActiveSeat {
                actor_pubkey: actor_pubkey.clone(),
                role: role.clone(),
            })
            .collect(),
        active_grants: authority
            .operator_grants
            .iter()
            .map(
                |(actor_pubkey, grant_event_ref)| CodingSessionTeamActiveGrant {
                    actor_pubkey: actor_pubkey.clone(),
                    grant_event_ref: grant_event_ref.clone(),
                    may_steer: true,
                },
            )
            .collect(),
    }
}

/// Return the one exact active lead target, including remote provider targets.
pub fn resolve_lead_target(
    package: &CodingSessionContextPackage,
    authority: &CurrentAuthority,
) -> Result<CodingSessionTarget, String> {
    let lead_actors: Vec<&str> = authority
        .seats
        .iter()
        .filter_map(|(actor, (role, _))| (role == "lead").then_some(actor.as_str()))
        .collect();
    if lead_actors.len() != 1 {
        return Err(format!(
            "expected exactly one active accepted lead seat, found {}",
            lead_actors.len()
        ));
    }
    let actor = lead_actors[0];
    let candidates: Vec<_> = package
        .roster
        .iter()
        .filter(|seat| {
            seat.actor.as_deref() == Some(actor)
                && seat.role.as_deref() == Some("lead")
                && seat.status == CodingSessionContextSeatStatus::Active
        })
        .collect();
    if candidates.len() != 1 {
        return Err(format!(
            "accepted lead has {} active receipt-backed provider generations",
            candidates.len()
        ));
    }
    Ok(candidates[0].target.clone())
}

/// Resolve one actor to one active receipt-backed provider generation.
pub fn resolve_actor_target(
    package: &CodingSessionContextPackage,
    actor: &str,
) -> Result<CodingSessionTarget, String> {
    let candidates: Vec<_> = package
        .roster
        .iter()
        .filter(|seat| {
            seat.actor.as_deref() == Some(actor)
                && seat.status == CodingSessionContextSeatStatus::Active
        })
        .collect();
    if candidates.len() != 1 {
        return Err(format!(
            "report actor has {} active receipt-backed provider generations",
            candidates.len()
        ));
    }
    Ok(candidates[0].target.clone())
}

/// Prove that the turn which ended was initiated by an exact operation pointer
/// to a canonical assignment for this actor and role.
///
/// READY and ordinary prose are definitively [`TurnReportRequirement::NotRequired`].
/// Once a strict assignment pointer is present, however, missing or currently
/// unverifiable transaction facts are [`TurnReportRequirement::Unknown`] so a
/// delayed relay query cannot permanently suppress the required diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnReportRequirement {
    NotRequired,
    Required { assignment_ref: String },
    Unknown(&'static str),
}

pub fn turn_requires_report(
    package: &CodingSessionContextPackage,
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
    command_id: &str,
    source_target: &CodingSessionTarget,
    actor: &str,
    role: &str,
) -> TurnReportRequirement {
    let matching: Vec<_> = package
        .inbox
        .iter()
        .filter(|item| item.command_id == command_id && &item.target == source_target)
        .collect();
    if matching.is_empty() {
        return TurnReportRequirement::Unknown("initiating_command_not_query_visible");
    }
    if matching.len() != 1 {
        return TurnReportRequirement::Unknown("assignment_command_conflict");
    }
    let pointer: serde_json::Value = match serde_json::from_str(&matching[0].content) {
        Ok(pointer) => pointer,
        Err(_) => return TurnReportRequirement::NotRequired,
    };
    let Some(object) = pointer.as_object() else {
        return TurnReportRequirement::NotRequired;
    };
    if object.len() != 2
        || object.get("type").and_then(serde_json::Value::as_str) != Some("assignment")
    {
        return TurnReportRequirement::NotRequired;
    }
    let Some(operation_id) = object
        .get("operationId")
        .and_then(serde_json::Value::as_str)
    else {
        return TurnReportRequirement::NotRequired;
    };
    let fold = match fold_coding_session_team_transactions(events, context) {
        Ok(fold) => fold,
        Err(_) => return TurnReportRequirement::Unknown("assignment_fold_unavailable"),
    };
    if !fold.included_event_ids.iter().any(|id| id == operation_id) {
        return TurnReportRequirement::Unknown("assignment_not_canonical");
    }
    let Some(event) = events
        .iter()
        .find(|event| event.id.to_hex() == operation_id)
    else {
        return TurnReportRequirement::Unknown("assignment_not_query_visible");
    };
    let payload = match buzz_core::coding_session_team_transaction::validate_coding_session_team_transaction_envelope(event) {
        Ok(payload) => payload,
        Err(_) => return TurnReportRequirement::Unknown("assignment_invalid"),
    };
    let CodingSessionTeamTransactionBody::Assignment(assignment) = payload.body else {
        return TurnReportRequirement::Unknown("assignment_type_mismatch");
    };
    if payload.delivery_command_id.as_deref() == Some(command_id)
        && assignment.assignee_actor == actor
        && assignment.assignee_role == role
    {
        TurnReportRequirement::Required {
            assignment_ref: operation_id.to_owned(),
        }
    } else {
        TurnReportRequirement::Unknown("assignment_binding_mismatch")
    }
}

pub fn provider_may_wake(
    provider_pubkey: &str,
    founder_pubkey: &str,
    authority: &CurrentAuthority,
) -> bool {
    provider_pubkey == founder_pubkey || authority.operator_grants.contains_key(provider_pubkey)
}

pub fn command_id(source_event_id: &str, target: &CodingSessionTarget, attempt: u32) -> String {
    let mut digest = Sha256::new();
    digest.update(b"buzz-provider-team-wake/v1\0");
    digest.update(source_event_id.as_bytes());
    digest.update(b"\0");
    digest.update(coding_session_target_key(target).as_bytes());
    digest.update(b"\0");
    digest.update(attempt.to_be_bytes());
    format!("team-wake-{}", hex::encode(digest.finalize()))
}

pub fn wake_text(source: &WakeSource) -> Result<String, String> {
    let value = match source {
        WakeSource::Report {
            operation_id,
            operation_type,
            ..
        } => serde_json::json!({"operationId": operation_id, "type": operation_type}),
        WakeSource::Terminal {
            terminal_event_id,
            role,
            caused_by_command_id,
            ..
        } => serde_json::json!({
            "schema": TEAM_WAKE_POINTER_SCHEMA,
            "type": "turn_ended_without_required_operation",
            "terminalEventId": terminal_event_id,
            "seatRole": role,
            "causedByCommandId": caused_by_command_id,
        }),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

/// Schema string carried by the terminal-diagnostic wake pointer.
const TEAM_WAKE_POINTER_SCHEMA: &str = "buzz-team-wake/v1";

/// Whether a JSON object is the identifier-only *report* pointer.
///
/// Exactly two keys, an event-id-shaped `operationId`, and a non-blank string
/// `type`. Anything looser would let ordinary operator JSON claim an
/// operation identity it does not have.
fn is_report_pointer(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    let Some(operation_id) = object
        .get("operationId")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let Some(operation_type) = object.get("type").and_then(serde_json::Value::as_str) else {
        return false;
    };
    object.len() == 2
        && operation_id.len() == 64
        && operation_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !operation_type.is_empty()
}

/// Whether a JSON object is the *terminal diagnostic* pointer.
///
/// Exactly the five keys the terminal arm of [`wake_text`] emits, every value
/// a string, and this crate's own schema.
fn is_terminal_pointer(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    const KEYS: [&str; 5] = [
        "schema",
        "type",
        "terminalEventId",
        "seatRole",
        "causedByCommandId",
    ];
    object.len() == KEYS.len()
        && KEYS
            .iter()
            .all(|key| object.get(*key).is_some_and(serde_json::Value::is_string))
        && object.get("schema").and_then(serde_json::Value::as_str)
            == Some(TEAM_WAKE_POINTER_SCHEMA)
}

/// Whether a parsed JSON value is one of the two pointer shapes [`wake_text`]
/// produces.
///
/// The single recogniser behind both [`operation_fence_key`] and
/// [`is_team_wake_pointer`]: a second one would be a second opinion about what
/// a wake is, and the fence and the framing must never disagree about that.
fn is_wake_pointer_value(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| is_report_pointer(object) || is_terminal_pointer(object))
}

/// Whether `text` is a team-wake pointer this provider itself mints.
///
/// Used by the delivery path to tell a provider-minted wake from words an
/// operator typed. Prose is never a pointer, so a founder who types JSON-shaped
/// prose that is not one of the two exact shapes is still treated as prose.
pub fn is_team_wake_pointer(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok_and(|value| is_wake_pointer_value(&value))
}

/// The durable identity of one team-wake *operation* addressed to one exact
/// execution generation, or `None` when `text` is not a wake pointer.
///
/// The runner spends model context per `commandId`, but the thing that must
/// happen at most once is the *operation*: the provider's wake sender and the
/// founder's Desktop fallback deliberately mint byte-identical pointer text
/// under different command ids, so a producer bug on either side would
/// otherwise buy a second lead turn on a fact the lead already has.
///
/// Only the two shapes [`wake_text`] produces are fenced. Prose is never
/// fenced: an operator who sends the same sentence twice meant to, and
/// swallowing the second one would be a control lying about what it does.
///
/// The key is `coding_session_target_key(target)`, a NUL, then the pointer
/// re-serialised through [`serde_json::Value`] — whose object is a `BTreeMap`
/// in this build, so key order and whitespace are canonicalised away while
/// two different pointers can never collide. The NUL cannot occur in either
/// half's JSON or in the length-prefixed target key, so the two components
/// cannot be confused for one another.
pub fn operation_fence_key(target: &CodingSessionTarget, text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if !is_wake_pointer_value(&value) {
        return None;
    }
    let canonical = serde_json::to_string(&value).ok()?;
    Some(format!(
        "{}\u{0}{canonical}",
        coding_session_target_key(target)
    ))
}

pub fn build_wake_event(
    keys: &Keys,
    scope: &WakeScope,
    source: &WakeSource,
    target: CodingSessionTarget,
    command_id: String,
) -> Result<Event, String> {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id,
        target,
        action: CodingSessionAction::ThreadTurnStart {
            text: wake_text(source)?,
            // A wake is provider-minted text; there is no operator draft to
            // carry images from.
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        },
    };
    buzz_sdk::builders::build_coding_session_command(scope.channel_ref, &payload)
        .map_err(|error| error.to_string())?
        .sign_with_keys(keys)
        .map_err(|error| error.to_string())
}

/// Return a verified settling provider outcome for this exact command and target.
///
/// Delivery degradation and interrupt delivery are progress facts, not proof
/// that the boundary prompt was queued, started, or refused. They therefore
/// leave the durable wake intent pending.
pub fn command_outcome(
    package: &CodingSessionContextPackage,
    command_id: &str,
    target: &CodingSessionTarget,
) -> Option<ReceiptStatus> {
    package
        .inbox
        .iter()
        .filter(|item| item.command_id == command_id && &item.target == target)
        .filter_map(|item| match item.stage {
            Some(
                status @ (ReceiptStatus::TurnQueued
                | ReceiptStatus::TurnStarted
                | ReceiptStatus::TurnDropped
                | ReceiptStatus::TurnRefused),
            ) => Some(status),
            _ => None,
        })
        .max_by_key(|status| match status {
            ReceiptStatus::TurnQueued => 1,
            ReceiptStatus::TurnStarted => 2,
            ReceiptStatus::TurnDropped | ReceiptStatus::TurnRefused => 3,
            _ => 0,
        })
}

pub fn observed_command_at(
    package: &CodingSessionContextPackage,
    command_id: &str,
    target: &CodingSessionTarget,
) -> Option<u64> {
    package
        .inbox
        .iter()
        .filter(|item| item.command_id == command_id && &item.target == target)
        .map(|item| item.created_at)
        .max()
}

pub fn command_echoed(
    package: &CodingSessionContextPackage,
    command_id: &str,
    target: &CodingSessionTarget,
    expected_content: &str,
) -> bool {
    package.history.iter().any(|item| {
        &item.target == target
            && item.item_kind == "user_prompt"
            && item
                .content
                .get("commandId")
                .and_then(serde_json::Value::as_str)
                == Some(command_id)
            && item
                .content
                .get("content")
                .and_then(serde_json::Value::as_str)
                == Some(expected_content)
    })
}

/// Whether any verified command delivered this exact identifier-only wake.
///
/// This deliberately ignores the producer-specific command id. The provider
/// and Desktop fallback mint different deterministic ids; binding instead to
/// exact target + exact pointer text lets either producer prove delivery and
/// prevents the other from spending a second lead turn on the same fact.
pub fn operation_wake_delivered(
    package: &CodingSessionContextPackage,
    target: &CodingSessionTarget,
    expected_content: &str,
) -> bool {
    let echoed = package.history.iter().any(|item| {
        &item.target == target
            && item.item_kind == "user_prompt"
            && item
                .content
                .get("content")
                .and_then(serde_json::Value::as_str)
                == Some(expected_content)
    });
    echoed
        || package.inbox.iter().any(|item| {
            &item.target == target
                && item.content == expected_content
                && (matches!(
                    item.stage,
                    Some(ReceiptStatus::TurnQueued | ReceiptStatus::TurnStarted)
                ) || command_echoed(package, &item.command_id, target, expected_content))
        })
}

pub const fn report_operation_type() -> CodingSessionTeamTransactionType {
    CodingSessionTeamTransactionType::Report
}

/// Complete verified inputs used for one routing/delivery decision.
pub struct VerifiedWakeSnapshot {
    pub founder_pubkey: String,
    pub authority: CurrentAuthority,
    pub package: CodingSessionContextPackage,
    pub team_events: Vec<Event>,
    pub included_reports: Vec<IncludedReport>,
}

/// Typed wake verification failure. Structural projection bounds must remain
/// distinguishable from retryable relay or fact failures.
#[derive(Debug, thiserror::Error)]
pub enum WakeSnapshotError {
    #[error(transparent)]
    Projection(#[from] ContextProjectionError),
    #[error("{0}")]
    Other(String),
}

impl From<String> for WakeSnapshotError {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

impl WakeSnapshotError {
    pub fn is_bound(&self) -> bool {
        matches!(self, Self::Projection(ContextProjectionError::Bound(_)))
    }
}

/// Fetch and verify the authority, provider-routing, and transaction facts for
/// one pending intent. Every query is exact-channel and complete-or-error.
pub async fn fetch_verified_snapshot(
    rest: &buzz_acp::relay::RestClient,
    relay_self_pubkey: &str,
    scope: &WakeScope,
) -> Result<VerifiedWakeSnapshot, WakeSnapshotError> {
    use buzz_core::kind::{
        KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_GENESIS,
        KIND_CODING_SESSION_TEAM_TRANSACTION, KIND_SYSTEM_MESSAGE,
    };
    use nostr::Kind;

    let genesis = rest
        .query_event_by_id(
            &scope.genesis_ref,
            Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
        )
        .await
        .map_err(|error| format!("genesis query failed: {error}"))?
        .ok_or_else(|| "canonical genesis is missing".to_owned())?;
    buzz_core::verify_event(&genesis)
        .map_err(|error| format!("canonical genesis signature is invalid: {error}"))?;
    let genesis_payload = decode_coding_session_genesis(&genesis.content)?;
    if genesis.id.to_hex() != scope.genesis_ref
        || genesis_payload.session_ref != scope.session_ref
        || tag_value(&genesis, "h") != Some(scope.channel_ref.to_string())
    {
        return Err("canonical genesis does not match the wake scope"
            .to_owned()
            .into());
    }
    let founder_pubkey = genesis.pubkey.to_hex();

    let transitions = query_complete_kind_partition(
        rest,
        scope.channel_ref,
        KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    )
    .await?;
    let by_id: std::collections::HashMap<String, Event> = transitions
        .into_iter()
        .map(|event| (event.id.to_hex(), event))
        .collect();
    let receipts =
        query_complete_kind_partition(rest, scope.channel_ref, KIND_SYSTEM_MESSAGE).await?;
    let mut links = Vec::new();
    for receipt in receipts
        .into_iter()
        .filter(|event| crate::authority::looks_like_acceptance_receipt(&event.content))
    {
        let claims_scope = serde_json::from_str::<serde_json::Value>(&receipt.content)
            .ok()
            .and_then(|value| value.get("genesisRef")?.as_str().map(str::to_owned))
            .as_deref()
            == Some(scope.genesis_ref.as_str());
        if !claims_scope {
            continue;
        }
        let accepted = crate::authority::verify_acceptance_receipt(
            &receipt,
            relay_self_pubkey,
            scope.channel_ref,
        )?;
        let transition = by_id
            .get(&accepted.accepted_event_id)
            .ok_or_else(|| {
                format!(
                    "accepted authority transition {} is missing",
                    accepted.accepted_event_id
                )
            })?
            .clone();
        crate::authority::verify_accepted_transition(
            &transition,
            &accepted,
            scope.channel_ref,
            &founder_pubkey,
        )?;
        links.push((accepted, transition));
    }
    let authority = crate::authority::fold_current_authority(&links)?;

    let package = fetch_and_project_session_context(
        rest,
        &ContextProjectionRequest {
            channel_id: scope.channel_ref,
            session_ref: scope.session_ref.clone(),
            genesis_ref: scope.genesis_ref.clone(),
            relay_self_pubkey: Some(relay_self_pubkey.to_owned()),
            allow_no_executions: false,
            generated_at: crate::state::now_ms(),
            limits: ContextProjectionLimits::default(),
        },
    )
    .await?;

    let team_events = query_complete_kind_partition(
        rest,
        scope.channel_ref,
        KIND_CODING_SESSION_TEAM_TRANSACTION,
    )
    .await?
    .into_iter()
    .filter(|event| {
        tag_value(event, "d").as_deref() == Some(scope.session_ref.as_str())
            && tag_value(event, "cstx-genesis").as_deref() == Some(scope.genesis_ref.as_str())
    })
    .collect::<Vec<_>>();
    let context = fold_context(scope, &founder_pubkey, &authority);
    let included_reports = included_reports(&team_events, &context)?;
    Ok(VerifiedWakeSnapshot {
        founder_pubkey,
        authority,
        package,
        team_events,
        included_reports,
    })
}

fn tag_value(event: &Event, name: &str) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        let tag = tag.as_slice();
        (tag.len() == 2 && tag[0] == name).then(|| tag[1].clone())
    })
}

#[cfg(test)]
#[path = "team_wake_tests.rs"]
mod tests;
