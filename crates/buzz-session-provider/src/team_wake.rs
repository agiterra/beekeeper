//! Durable provider-owned wake intents for team transaction progress.
//!
//! An intent is persisted before any kind-44220 publication. Unlike the
//! provider outbox it stores the semantic source and current routing attempt,
//! so a command that outlives the protocol horizon or a lead generation that
//! is superseded can be re-resolved and freshly signed without forgetting why
//! it exists. Relay acceptance is not completion: only a verified provider
//! receipt or prompt echo retires the intent.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
    fetch_and_project_session_context, query_complete_kind_partition, ContextProjectionLimits,
    ContextProjectionRequest,
};
use crate::state::atomic_write;

const STORE_FILE: &str = "team-wake-intents.json";
const STORE_SCHEMA: &str = "buzz-provider-team-wake-intents/v1";
const MAX_PENDING_INTENTS: usize = 256;
const MAX_COMPLETED_SOURCES: usize = 4_096;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    schema: String,
    pending: Vec<WakeIntent>,
    #[serde(default)]
    completed_source_ids: Vec<String>,
}

/// Atomic crash-safe store for unresolved provider wake intents.
#[derive(Debug)]
pub struct WakeIntentStore {
    path: PathBuf,
    pending: Vec<WakeIntent>,
    completed_source_ids: Vec<String>,
}

impl WakeIntentStore {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(STORE_FILE);
        let (pending, completed_source_ids) = match fs::read(&path) {
            Ok(body) => {
                let snapshot: Snapshot = serde_json::from_slice(&body).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unreadable wake store: {error}"),
                    )
                })?;
                if snapshot.schema != STORE_SCHEMA
                    || snapshot.pending.len() > MAX_PENDING_INTENTS
                    || snapshot.completed_source_ids.len() > MAX_COMPLETED_SOURCES
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unsupported or oversized wake store",
                    ));
                }
                (snapshot.pending, snapshot.completed_source_ids)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            pending,
            completed_source_ids,
        })
    }

    pub fn pending(&self) -> &[WakeIntent] {
        &self.pending
    }

    /// Whether recovery has already durably captured this exact in-flight turn.
    pub fn has_terminal_command(
        &self,
        command_id: &str,
        source_target: &CodingSessionTarget,
    ) -> bool {
        self.pending.iter().any(|intent| {
            matches!(
                &intent.source,
                WakeSource::Terminal {
                    caused_by_command_id,
                    source_target: stored_target,
                    ..
                } if caused_by_command_id == command_id && stored_target == source_target
            )
        })
    }

    pub fn enqueue(&mut self, scope: WakeScope, source: WakeSource) -> io::Result<bool> {
        if self
            .pending
            .iter()
            .any(|intent| intent.source.event_id() == source.event_id())
            || self
                .completed_source_ids
                .iter()
                .any(|event_id| event_id == source.event_id())
        {
            return Ok(false);
        }
        if self.pending.len() >= MAX_PENDING_INTENTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "provider team-wake intent bound reached",
            ));
        }
        let previous = (self.pending.clone(), self.completed_source_ids.clone());
        self.pending.push(WakeIntent {
            scope,
            source,
            target: None,
            command_id: None,
            signed_event: None,
            relay_accepted_at: None,
            attempt: 0,
            last_reason: None,
        });
        self.persist_or_restore(previous)?;
        Ok(true)
    }

    pub fn replace(&mut self, index: usize, intent: WakeIntent) -> io::Result<()> {
        let previous = (self.pending.clone(), self.completed_source_ids.clone());
        let Some(slot) = self.pending.get_mut(index) else {
            return Ok(());
        };
        *slot = intent;
        self.persist_or_restore(previous)
    }

    /// Persist the current first intent and yield to the next pending source.
    pub fn defer_first(&mut self, intent: WakeIntent) -> io::Result<()> {
        let previous = (self.pending.clone(), self.completed_source_ids.clone());
        let Some(first) = self.pending.first_mut() else {
            return Ok(());
        };
        *first = intent;
        if self.pending.len() > 1 {
            self.pending.rotate_left(1);
        }
        self.persist_or_restore(previous)
    }

    pub fn retire(&mut self, index: usize) -> io::Result<()> {
        if index >= self.pending.len() {
            return Ok(());
        }
        let previous = (self.pending.clone(), self.completed_source_ids.clone());
        let retired = self.pending.remove(index);
        self.completed_source_ids
            .push(retired.source.event_id().to_owned());
        if self.completed_source_ids.len() > MAX_COMPLETED_SOURCES {
            let dropped = self.completed_source_ids.len() - MAX_COMPLETED_SOURCES;
            self.completed_source_ids.drain(..dropped);
        }
        self.persist_or_restore(previous)
    }

    fn persist_or_restore(&mut self, previous: (Vec<WakeIntent>, Vec<String>)) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&Snapshot {
            schema: STORE_SCHEMA.to_owned(),
            pending: self.pending.clone(),
            completed_source_ids: self.completed_source_ids.clone(),
        })?;
        if let Err(error) = atomic_write(&self.path, &body) {
            self.pending = previous.0;
            self.completed_source_ids = previous.1;
            return Err(error);
        }
        Ok(())
    }
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

/// One canonical included report and the causal assignment it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludedReport {
    pub event_id: String,
    pub assignment_ref: String,
    pub author_pubkey: String,
    pub created_at: u64,
}

/// Validate the complete transaction graph and return included reports.
pub fn included_reports(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
) -> Result<Vec<IncludedReport>, String> {
    let fold = fold_coding_session_team_transactions(events, context)?;
    let included: std::collections::HashSet<&str> =
        fold.included_event_ids.iter().map(String::as_str).collect();
    let mut reports = Vec::new();
    for event in events {
        let id = event.id.to_hex();
        if !included.contains(id.as_str()) {
            continue;
        }
        let payload = buzz_core::coding_session_team_transaction::validate_coding_session_team_transaction_envelope(event)?;
        if let CodingSessionTeamTransactionBody::Report(report) = payload.body {
            reports.push(IncludedReport {
                event_id: id,
                assignment_ref: report.assignment_ref,
                author_pubkey: event.pubkey.to_hex(),
                created_at: event.created_at.as_secs(),
            });
        }
    }
    Ok(reports)
}

pub fn report_suppresses_terminal(
    reports: &[IncludedReport],
    assignment_ref: &str,
    actor: &str,
    prompt_at_ms: Option<i64>,
    terminal_at_ms: i64,
) -> bool {
    reports.iter().any(|report| {
        let created_at_ms = i64::try_from(report.created_at)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1_000));
        report.assignment_ref == assignment_ref
            && report.author_pubkey == actor
            && created_at_ms.is_some_and(|created| {
                created <= terminal_at_ms && prompt_at_ms.is_none_or(|prompt| created >= prompt)
            })
    })
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
            "schema": "buzz-team-wake/v1",
            "type": "turn_ended_without_required_operation",
            "terminalEventId": terminal_event_id,
            "seatRole": role,
            "causedByCommandId": caused_by_command_id,
        }),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
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

/// Fetch and verify the authority, provider-routing, and transaction facts for
/// one pending intent. Every query is exact-channel and complete-or-error.
pub async fn fetch_verified_snapshot(
    rest: &buzz_acp::relay::RestClient,
    relay_self_pubkey: &str,
    scope: &WakeScope,
) -> Result<VerifiedWakeSnapshot, String> {
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
        return Err("canonical genesis does not match the wake scope".into());
    }
    let founder_pubkey = genesis.pubkey.to_hex();

    let transitions = query_complete_kind_partition(
        rest,
        scope.channel_ref,
        KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    )
    .await
    .map_err(|error| error.to_string())?;
    let by_id: std::collections::HashMap<String, Event> = transitions
        .into_iter()
        .map(|event| (event.id.to_hex(), event))
        .collect();
    let receipts = query_complete_kind_partition(rest, scope.channel_ref, KIND_SYSTEM_MESSAGE)
        .await
        .map_err(|error| error.to_string())?;
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
    .await
    .map_err(|error| error.to_string())?;

    let team_events = query_complete_kind_partition(
        rest,
        scope.channel_ref,
        KIND_CODING_SESSION_TEAM_TRANSACTION,
    )
    .await
    .map_err(|error| error.to_string())?
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
