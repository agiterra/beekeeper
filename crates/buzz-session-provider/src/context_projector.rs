//! Verified relay-fact projection for cross-machine session rehydration.
//!
//! The projector consumes explicit fact bundles rather than an untyped event
//! bag. Every retained transcript must prove this chain:
//!
//! `genesis -> authorized create/resume -> provider-signed receipt -> exact
//! generation metadata -> provider-signed transcript sequence`.
//!
//! A valid Nostr signature alone is insufficient. Channel, session, genesis,
//! operator authority, provider authority, semantic tags, and generation
//! linkage are checked before content enters the private package. Any conflict
//! fails the whole projection; bounds cause an explicit, deterministic
//! truncation instead of an implicit partial history.

use std::collections::{HashMap, HashSet};

use nostr::Event;
use serde_json::Value;
use uuid::Uuid;

use buzz_acp::relay::RestClient;

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_context::{
    coding_session_context_role_for_item_kind, CodingSessionContextHistoryItem,
    CodingSessionContextIdentity, CodingSessionContextPackage, CodingSessionContextProvenance,
    CODING_SESSION_CONTEXT_PACKAGE_VERSION, MAX_CONTEXT_HISTORY_ITEMS, MAX_CONTEXT_PACKAGE_BYTES,
    MAX_CONTEXT_PROVENANCE_NOTES, MAX_CONTEXT_PROVENANCE_NOTE_BYTES,
};
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::coding_session_goal::{
    latest_coding_session_goal, validate_coding_session_goal_envelope,
};
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CodingSessionLifecycleCommandPayload, CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
};
use buzz_core::coding_session_name::{
    latest_coding_session_name, validate_coding_session_name_envelope,
};
use buzz_core::coding_session_payload::{
    decode_coding_session_metadata, LifecycleReceipt, ReceiptStatus, SessionMetadata,
    TranscriptEnvelope, LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA, TRANSCRIPT_SCHEMA,
};
use buzz_core::kind::{
    event_kind_u32, KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_GOAL, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_TRANSCRIPT, KIND_SYSTEM_MESSAGE,
};
use buzz_core::verify_event;
use buzz_sdk::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, coding_session_metadata_semantic_key,
    coding_session_transcript_semantic_key, CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    CODING_SESSION_METADATA_TAG_VERSION, CODING_SESSION_TRANSCRIPT_TAG_VERSION,
    MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES, MAX_METADATA_CONTENT_BYTES, MAX_TRANSCRIPT_CONTENT_BYTES,
};

use crate::authority::{verify_acceptance_receipt, verify_accepted_transition};

/// Maximum signed source events one projection call will examine.
pub const MAX_CONTEXT_SOURCE_EVENTS: usize = 16_384;
/// Maximum total signed-content bytes one projection call will examine.
pub const MAX_CONTEXT_SOURCE_CONTENT_BYTES: usize = 32 * 1024 * 1024;
/// Current relay-side maximum rows returned for one standard Nostr filter.
const RELAY_QUERY_PAGE_LIMIT: usize = 1_000;

/// One accepted authority-chain link, including both cryptographic witnesses.
#[derive(Debug, Clone)]
pub struct ContextAuthorityLink {
    /// Relay-signed kind-40099 acceptance receipt.
    pub receipt: Event,
    /// Owner-signed kind-44228 transition named by the receipt.
    pub transition: Event,
}

/// Facts proving one exact provider generation and its transcript.
#[derive(Debug, Clone)]
pub struct ContextGenerationFacts {
    /// Founder/operator-signed create (first generation) or resume command.
    pub lifecycle_command: Event,
    /// Provider-signed outcome binding the command to this generation target.
    pub receipt: Event,
    /// Provider-signed immutable facts for the exact generation.
    pub metadata: Event,
    /// Provider-signed, sequenced transcript facts for the exact generation.
    pub transcript: Vec<Event>,
}

/// The complete generation chain of one provider execution.
#[derive(Debug, Clone)]
pub struct ContextExecutionFacts {
    /// Generation one followed by every verified resume generation.
    pub generations: Vec<ContextGenerationFacts>,
}

/// Source-query coverage stated by the caller that fetched the relay facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceCoverage {
    /// True only when every relevant relay query proved an uncapped result.
    pub complete: bool,
    /// Total relevant transcript items when source completeness established it.
    pub total_history_items: Option<u64>,
    /// Bounded explanatory notes copied into package provenance.
    pub notes: Vec<String>,
}

/// Deterministic local package limits, each capped by the shared wire ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextProjectionLimits {
    /// Maximum most-recent transcript items retained.
    pub max_history_items: usize,
    /// Maximum serialized package bytes retained.
    pub max_package_bytes: usize,
}

impl Default for ContextProjectionLimits {
    fn default() -> Self {
        Self {
            max_history_items: MAX_CONTEXT_HISTORY_ITEMS,
            max_package_bytes: MAX_CONTEXT_PACKAGE_BYTES,
        }
    }
}

/// Explicit facts and authority witnesses for one package projection.
#[derive(Debug, Clone)]
pub struct ContextProjectionInput {
    /// Session channel all facts must inhabit.
    pub channel_id: Uuid,
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Exact canonical genesis event id.
    pub genesis_ref: String,
    /// Relay identity witnessed at connect time for acceptance receipts.
    ///
    /// A relay that does not advertise a stable identity can still project a
    /// founder-only session. It cannot prove accepted authority transitions.
    pub relay_self_pubkey: Option<String>,
    /// Directly resolved genesis event.
    pub genesis: Event,
    /// Contiguous accepted authority chain, if any.
    pub authority_links: Vec<ContextAuthorityLink>,
    /// One or more exact provider execution-generation chains.
    pub executions: Vec<ContextExecutionFacts>,
    /// Candidate founder-authored name revisions for this session only.
    pub name_revisions: Vec<Event>,
    /// Candidate founder-authored goal revisions for this session only.
    pub goal_revisions: Vec<Event>,
    /// Source-query coverage and explanation.
    pub coverage: ContextSourceCoverage,
    /// Epoch milliseconds stamped on the private package.
    pub generated_at: i64,
    /// Local package bounds.
    pub limits: ContextProjectionLimits,
}

/// Coordinates and local bounds for fetching one package from the relay.
#[derive(Debug, Clone)]
pub struct ContextProjectionRequest {
    /// Session channel all queried facts must inhabit.
    pub channel_id: Uuid,
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Exact canonical genesis event id.
    pub genesis_ref: String,
    /// Relay identity witnessed during connection setup, when advertised.
    pub relay_self_pubkey: Option<String>,
    /// Epoch milliseconds stamped on the private package.
    pub generated_at: i64,
    /// Local package bounds.
    pub limits: ContextProjectionLimits,
}

/// A projection failure. No partial package is returned.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContextProjectionError {
    /// The bounded relay query failed or returned a malformed response.
    #[error("context relay query failed: {0}")]
    Relay(String),
    /// A purported source fact failed signature, shape, scope, or linkage.
    #[error("invalid context fact: {0}")]
    InvalidFact(String),
    /// Two individually plausible facts make incompatible immutable claims.
    #[error("context fact conflict: {0}")]
    Conflict(String),
    /// Input or output exceeded a hard processing bound.
    #[error("context projection bound exceeded: {0}")]
    Bound(String),
}

#[derive(Debug)]
struct VerifiedGeneration {
    target: CodingSessionTarget,
    provider_authority: String,
    provider_instance_ref: String,
    project_ref: Option<String>,
    title: Option<String>,
}

#[derive(Debug)]
struct CandidateHistory {
    timestamp_ms: i64,
    item: CodingSessionContextHistoryItem,
}

#[derive(Debug)]
struct Grant {
    grantee: String,
    accepted_at: u64,
}

/// Fetch, group, and verify the complete bounded relay fact set for a session.
///
/// This is the create-flow entry point. Control facts are queried in separate
/// kind partitions so transcript volume cannot starve genesis or authority
/// links. A saturated control partition fails closed. A saturated transcript
/// partition is retained only with explicit incomplete-source provenance.
/// Callers may serialize the result for a private MCP adapter; this function
/// performs no filesystem writes.
pub async fn fetch_and_project_session_context(
    rest: &RestClient,
    request: &ContextProjectionRequest,
) -> Result<CodingSessionContextPackage, ContextProjectionError> {
    use nostr::Kind;

    let genesis = rest
        .query_event_by_id(
            &request.genesis_ref,
            Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
        )
        .await
        .map_err(|error| ContextProjectionError::Relay(error.to_string()))?
        .ok_or_else(|| ContextProjectionError::InvalidFact("missing canonical genesis".into()))?;
    let mut events = vec![genesis];
    let mut ids = HashSet::new();
    ids.insert(events[0].id);
    let mut transcript_complete = true;
    for kind in [
        KIND_CODING_SESSION_AUTHORITY_TRANSITION,
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        KIND_CODING_SESSION_METADATA,
        KIND_CODING_SESSION_TRANSCRIPT,
        KIND_CODING_SESSION_NAME,
        KIND_CODING_SESSION_GOAL,
        KIND_SYSTEM_MESSAGE,
    ] {
        let partition = query_kind_partition(rest, request.channel_id, kind).await?;
        if partition.len() == RELAY_QUERY_PAGE_LIMIT {
            if kind == KIND_CODING_SESSION_TRANSCRIPT {
                transcript_complete = false;
            } else {
                return Err(ContextProjectionError::Bound(format!(
                    "relay kind-{kind} partition reached its {RELAY_QUERY_PAGE_LIMIT}-row clamp; refusing potentially incomplete authority/linkage facts"
                )));
            }
        }
        for event in partition {
            if ids.insert(event.id) {
                events.push(event);
            }
        }
    }
    let coverage = if transcript_complete {
        ContextSourceCoverage {
            complete: true,
            total_history_items: None,
            notes: vec![
                "All kind-partitioned relay queries completed below their row clamp".into(),
            ],
        }
    } else {
        ContextSourceCoverage {
            complete: false,
            total_history_items: None,
            notes: vec![format!(
                "Transcript query reached the relay's {RELAY_QUERY_PAGE_LIMIT}-row clamp; retained history is verified but source coverage is incomplete"
            )],
        }
    };
    group_and_project_session_context_events(request, &events, coverage)
}

/// Group an already-fetched complete channel fact set, then run strict proof.
///
/// This helper makes the network boundary testable and is also suitable for a
/// caller that already owns an authenticated relay query. `events` must be the
/// complete union of the explicit-kind, exact-`h` partitions plus the genesis
/// resolved by exact event id.
pub fn project_session_context_events(
    request: &ContextProjectionRequest,
    events: &[Event],
) -> Result<CodingSessionContextPackage, ContextProjectionError> {
    group_and_project_session_context_events(
        request,
        events,
        ContextSourceCoverage {
            complete: true,
            total_history_items: None,
            notes: vec!["Caller supplied a complete relay fact set".into()],
        },
    )
}

async fn query_kind_partition(
    rest: &RestClient,
    channel_id: Uuid,
    kind: u32,
) -> Result<Vec<Event>, ContextProjectionError> {
    use nostr::{Alphabet, Filter, Kind, SingleLetterTag};

    let filter = Filter::new()
        .kind(Kind::Custom(kind as u16))
        .custom_tags(
            SingleLetterTag::lowercase(Alphabet::H),
            [channel_id.to_string()],
        )
        .limit(RELAY_QUERY_PAGE_LIMIT);
    let value = rest
        .query(&[filter])
        .await
        .map_err(|error| ContextProjectionError::Relay(error.to_string()))?;
    let rows = value.as_array().ok_or_else(|| {
        ContextProjectionError::Relay(format!(
            "relay kind-{kind} context query returned a non-array response"
        ))
    })?;
    rows.iter()
        .map(|row| {
            serde_json::from_value(row.clone()).map_err(|error| {
                ContextProjectionError::Relay(format!(
                    "relay kind-{kind} query returned a malformed event: {error}"
                ))
            })
        })
        .collect()
}

fn group_and_project_session_context_events(
    request: &ContextProjectionRequest,
    events: &[Event],
    mut coverage: ContextSourceCoverage,
) -> Result<CodingSessionContextPackage, ContextProjectionError> {
    if events.len() > MAX_CONTEXT_SOURCE_EVENTS {
        return Err(ContextProjectionError::Bound(format!(
            "relay fact set has {} events, max {MAX_CONTEXT_SOURCE_EVENTS}",
            events.len()
        )));
    }
    let genesis = unique_event(
        events.iter().filter(|event| {
            event_kind_u32(event) == KIND_CODING_SESSION_GENESIS
                && event.id.to_hex() == request.genesis_ref
        }),
        "canonical genesis",
    )?;

    let authority_links = group_authority_links(request, events)?;
    let executions = group_executions(request, events)?;
    let name_revisions = events
        .iter()
        .filter(|event| {
            event_kind_u32(event) == KIND_CODING_SESSION_NAME
                && tag_value(event, "d").as_deref() == Some(request.session_ref.as_str())
        })
        .cloned()
        .collect();
    let goal_revisions = events
        .iter()
        .filter(|event| {
            event_kind_u32(event) == KIND_CODING_SESSION_GOAL
                && tag_value(event, "d").as_deref() == Some(request.session_ref.as_str())
        })
        .cloned()
        .collect();
    let total_history_items = executions
        .iter()
        .flat_map(|execution| &execution.generations)
        .map(|generation| generation.transcript.len() as u64)
        .sum();
    if coverage.complete {
        coverage.total_history_items = Some(total_history_items);
    }
    project_session_context(&ContextProjectionInput {
        channel_id: request.channel_id,
        session_ref: request.session_ref.clone(),
        genesis_ref: request.genesis_ref.clone(),
        relay_self_pubkey: request.relay_self_pubkey.clone(),
        genesis: genesis.clone(),
        authority_links,
        executions,
        name_revisions,
        goal_revisions,
        coverage,
        generated_at: request.generated_at,
        limits: request.limits,
    })
}

fn unique_event<'a>(
    events: impl Iterator<Item = &'a Event>,
    label: &str,
) -> Result<&'a Event, ContextProjectionError> {
    let mut matches = events;
    let event = matches
        .next()
        .ok_or_else(|| ContextProjectionError::InvalidFact(format!("missing {label}")))?;
    if matches.next().is_some() {
        return Err(ContextProjectionError::Conflict(format!(
            "more than one {label} was returned"
        )));
    }
    Ok(event)
}

fn group_authority_links(
    request: &ContextProjectionRequest,
    events: &[Event],
) -> Result<Vec<ContextAuthorityLink>, ContextProjectionError> {
    let by_id: HashMap<String, &Event> = events
        .iter()
        .map(|event| (event.id.to_hex(), event))
        .collect();
    let mut accepted = Vec::new();
    for receipt in events.iter().filter(|event| {
        event_kind_u32(event) == KIND_SYSTEM_MESSAGE
            && crate::authority::looks_like_acceptance_receipt(&event.content)
    }) {
        let claims_requested_genesis = serde_json::from_str::<Value>(&receipt.content)
            .ok()
            .and_then(|content| {
                content
                    .get("genesisRef")
                    .and_then(Value::as_str)
                    .map(|genesis_ref| genesis_ref == request.genesis_ref)
            })
            .unwrap_or(false);
        if !claims_requested_genesis {
            continue;
        }
        let relay_self_pubkey = request.relay_self_pubkey.as_deref().ok_or_else(|| {
            ContextProjectionError::InvalidFact(
                "relay has no stable identity needed to verify a candidate authority receipt"
                    .into(),
            )
        })?;
        let proof = match verify_acceptance_receipt(receipt, relay_self_pubkey, request.channel_id)
        {
            Ok(proof) if proof.genesis_ref == request.genesis_ref => proof,
            Ok(_) => continue,
            Err(error) => {
                return Err(ContextProjectionError::InvalidFact(format!(
                    "candidate authority acceptance receipt failed verification: {error}"
                )));
            }
        };
        let transition = by_id.get(&proof.accepted_event_id).ok_or_else(|| {
            ContextProjectionError::InvalidFact(format!(
                "accepted authority transition {} is absent from the complete relay fact set",
                proof.accepted_event_id
            ))
        })?;
        accepted.push((
            proof.seq,
            ContextAuthorityLink {
                receipt: receipt.clone(),
                transition: (*transition).clone(),
            },
        ));
    }
    accepted.sort_by_key(|(seq, _)| *seq);
    Ok(accepted.into_iter().map(|(_, link)| link).collect())
}

fn group_executions(
    request: &ContextProjectionRequest,
    events: &[Event],
) -> Result<Vec<ContextExecutionFacts>, ContextProjectionError> {
    let commands: Vec<(&Event, CodingSessionLifecycleCommandPayload)> = events
        .iter()
        .filter(|event| event_kind_u32(event) == KIND_CODING_SESSION_LIFECYCLE_COMMAND)
        .filter_map(|event| {
            decode_coding_session_lifecycle_command(&event.content)
                .ok()
                .map(|payload| (event, payload))
        })
        .collect();
    let mut executions = Vec::new();
    let mut consumed_commands = HashSet::new();
    let mut consumed_targets = HashSet::new();

    for (create_event, create) in commands.iter().filter(|(_, command)| {
        matches!(
            &command.action,
            CodingSessionLifecycleAction::SessionCreate {
                session_ref,
                genesis_ref,
                ..
            } if session_ref.as_deref() == Some(request.session_ref.as_str())
                && genesis_ref.as_deref() == Some(request.genesis_ref.as_str())
        )
    }) {
        let CodingSessionLifecycleAction::SessionCreate {
            project_ref,
            provider_instance_ref,
            provider_authority_pubkey,
            ..
        } = &create.action
        else {
            continue;
        };
        let Some((receipt_event, receipt)) =
            receipt_for_command(events, &create.command_id, provider_authority_pubkey)?
        else {
            continue;
        };
        if !matches!(
            receipt.status,
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
        ) {
            continue;
        }
        let target = receipt.session.ok_or_else(|| {
            ContextProjectionError::InvalidFact(
                "successful create receipt is missing its session target".into(),
            )
        })?;
        if !consumed_commands.insert(create_event.id) {
            return Err(ContextProjectionError::Conflict(
                "one create command was grouped more than once".into(),
            ));
        }
        let mut generations = vec![group_generation(
            request,
            events,
            create_event,
            receipt_event,
            &target,
            provider_authority_pubkey,
            provider_instance_ref,
            project_ref,
        )?];
        if !consumed_targets.insert(coding_session_target_key(&target)) {
            return Err(ContextProjectionError::Conflict(format!(
                "multiple creates established target {}",
                coding_session_target_key(&target)
            )));
        }

        let mut current = target;
        loop {
            let mut established_resumes = Vec::new();
            for (resume_event, resume) in commands.iter().filter(|(_, command)| {
                matches!(
                    &command.action,
                    CodingSessionLifecycleAction::SessionResume {
                        session,
                        provider_authority_pubkey: authority,
                    } if session == &current && authority == provider_authority_pubkey
                )
            }) {
                let Some((resume_receipt_event, resume_receipt)) =
                    receipt_for_command(events, &resume.command_id, provider_authority_pubkey)?
                else {
                    continue;
                };
                if matches!(
                    resume_receipt.status,
                    ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext
                ) {
                    let next = resume_receipt.session.ok_or_else(|| {
                        ContextProjectionError::InvalidFact(
                            "successful resume receipt is missing its session target".into(),
                        )
                    })?;
                    established_resumes.push((*resume_event, resume_receipt_event, next));
                }
            }
            if established_resumes.is_empty() {
                break;
            }
            if established_resumes.len() != 1 {
                return Err(ContextProjectionError::Conflict(format!(
                    "target {} has {} successful resume branches",
                    coding_session_target_key(&current),
                    established_resumes.len()
                )));
            }
            let (resume_event, resume_receipt, next) = established_resumes.remove(0);
            if !consumed_commands.insert(resume_event.id) {
                return Err(ContextProjectionError::Conflict(
                    "one resume command was grouped more than once".into(),
                ));
            }
            if !consumed_targets.insert(coding_session_target_key(&next)) {
                return Err(ContextProjectionError::Conflict(format!(
                    "resume chain repeats target {}",
                    coding_session_target_key(&next)
                )));
            }
            generations.push(group_generation(
                request,
                events,
                resume_event,
                resume_receipt,
                &next,
                provider_authority_pubkey,
                provider_instance_ref,
                project_ref,
            )?);
            current = next;
        }
        executions.push(ContextExecutionFacts { generations });
    }
    if executions.is_empty() {
        return Err(ContextProjectionError::InvalidFact(
            "no successful create chain links this session and genesis".into(),
        ));
    }
    Ok(executions)
}

fn receipt_for_command<'a>(
    events: &'a [Event],
    command_id: &str,
    provider_authority: &str,
) -> Result<Option<(&'a Event, LifecycleReceipt)>, ContextProjectionError> {
    let mut matches = Vec::new();
    for event in events.iter().filter(|event| {
        event_kind_u32(event) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT
            && event.pubkey.to_hex() == provider_authority
            && tag_value(event, "csl-command").as_deref() == Some(command_id)
    }) {
        let receipt: LifecycleReceipt = serde_json::from_str(&event.content).map_err(|_| {
            ContextProjectionError::InvalidFact(format!(
                "provider receipt for command {command_id} is malformed"
            ))
        })?;
        if receipt.command_id != command_id {
            return Err(ContextProjectionError::Conflict(format!(
                "receipt tag and content disagree for command {command_id}"
            )));
        }
        matches.push((event, receipt));
    }
    if matches.len() > 1 {
        return Err(ContextProjectionError::Conflict(format!(
            "command {command_id} has more than one provider receipt"
        )));
    }
    Ok(matches.pop())
}

#[allow(clippy::too_many_arguments)]
fn group_generation(
    request: &ContextProjectionRequest,
    events: &[Event],
    command: &Event,
    receipt: &Event,
    target: &CodingSessionTarget,
    provider_authority: &str,
    provider_instance_ref: &str,
    project_ref: &Option<String>,
) -> Result<ContextGenerationFacts, ContextProjectionError> {
    let target_key = coding_session_target_key(target);
    let mut metadata = Vec::new();
    let mut transcript = Vec::new();
    for event in events.iter().filter(|event| {
        event.pubkey.to_hex() == provider_authority
            && tag_value(event, "cs-target").as_deref() == Some(target_key.as_str())
    }) {
        match event_kind_u32(event) {
            KIND_CODING_SESSION_METADATA => {
                let payload = decode_coding_session_metadata(&event.content)
                    .map_err(ContextProjectionError::InvalidFact)?;
                if payload.session != *target
                    || payload.session_ref.as_deref() != Some(request.session_ref.as_str())
                    || payload.provider.as_deref() != Some(provider_instance_ref)
                    || &payload.project_ref != project_ref
                {
                    return Err(ContextProjectionError::Conflict(format!(
                        "provider metadata for target {target_key} changes immutable linkage"
                    )));
                }
                metadata.push(event);
            }
            KIND_CODING_SESSION_TRANSCRIPT => {
                let payload: TranscriptEnvelope =
                    serde_json::from_str(&event.content).map_err(|_| {
                        ContextProjectionError::InvalidFact(format!(
                            "provider transcript for target {target_key} is malformed"
                        ))
                    })?;
                if payload.session != *target {
                    return Err(ContextProjectionError::Conflict(format!(
                        "transcript tag and content target disagree for {target_key}"
                    )));
                }
                transcript.push(event.clone());
            }
            _ => {}
        }
    }
    metadata.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let metadata = metadata.pop().ok_or_else(|| {
        ContextProjectionError::InvalidFact(format!("target {target_key} has no provider metadata"))
    })?;
    Ok(ContextGenerationFacts {
        lifecycle_command: command.clone(),
        receipt: receipt.clone(),
        metadata: metadata.clone(),
        transcript,
    })
}

/// Project a bounded private package from fully verified relay facts.
pub fn project_session_context(
    input: &ContextProjectionInput,
) -> Result<CodingSessionContextPackage, ContextProjectionError> {
    validate_limits(input.limits)?;
    validate_source_bound(input)?;
    validate_coverage(&input.coverage)?;

    let founder = verify_genesis(input)?;
    let grants = verify_authority_chain(input, &founder)?;
    let name = verify_latest_text_revisions(
        &input.name_revisions,
        input,
        &founder,
        KIND_CODING_SESSION_NAME,
    )?;
    let goal = verify_latest_text_revisions(
        &input.goal_revisions,
        input,
        &founder,
        KIND_CODING_SESSION_GOAL,
    )?;

    if input.executions.is_empty() {
        return Err(ContextProjectionError::InvalidFact(
            "at least one provider execution chain is required".into(),
        ));
    }

    let mut candidates = Vec::new();
    let mut project_ref: Option<Option<String>> = None;
    let mut fallback_title: Option<String> = None;
    let mut transcript_keys: HashMap<(String, u64), String> = HashMap::new();
    let mut generation_targets = HashSet::new();

    for execution in &input.executions {
        if execution.generations.is_empty() {
            return Err(ContextProjectionError::InvalidFact(
                "provider execution has no generation facts".into(),
            ));
        }
        let mut previous: Option<VerifiedGeneration> = None;
        for (index, facts) in execution.generations.iter().enumerate() {
            let generation = verify_generation(input, &founder, &grants, previous.as_ref(), facts)?;
            let target_key = coding_session_target_key(&generation.target);
            if !generation_targets.insert(target_key.clone()) {
                return Err(ContextProjectionError::Conflict(format!(
                    "generation target {target_key} appears in more than one execution bundle"
                )));
            }

            match &project_ref {
                None => project_ref = Some(generation.project_ref.clone()),
                Some(expected) if expected == &generation.project_ref => {}
                Some(_) => {
                    return Err(ContextProjectionError::Conflict(
                        "linked creates disagree on projectRef".into(),
                    ));
                }
            }
            if index == 0 && fallback_title.is_none() {
                fallback_title = generation.title.clone();
            }

            for transcript in &facts.transcript {
                let candidate = verify_transcript(
                    transcript,
                    input.channel_id,
                    &generation.target,
                    &generation.provider_authority,
                )?;
                let key = (target_key.clone(), candidate.item.event_seq);
                if let Some(existing_id) =
                    transcript_keys.insert(key.clone(), candidate.item.event_id.clone())
                {
                    return Err(ContextProjectionError::Conflict(format!(
                        "target {} sequence {} has divergent events {existing_id} and {}",
                        key.0, key.1, candidate.item.event_id
                    )));
                }
                candidates.push(candidate);
            }
            previous = Some(generation);
        }
    }

    candidates.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| {
                coding_session_target_key(&left.item.target)
                    .cmp(&coding_session_target_key(&right.item.target))
            })
            .then_with(|| left.item.event_seq.cmp(&right.item.event_seq))
            .then_with(|| left.item.event_id.cmp(&right.item.event_id))
    });

    let verified_total = candidates.len() as u64;
    if input.coverage.complete && input.coverage.total_history_items != Some(verified_total) {
        return Err(ContextProjectionError::Conflict(format!(
            "complete source claimed {:?} history items but {} verified",
            input.coverage.total_history_items, verified_total
        )));
    }

    let retain_from = candidates
        .len()
        .saturating_sub(input.limits.max_history_items);
    let mut history: Vec<CodingSessionContextHistoryItem> = candidates
        .into_iter()
        .skip(retain_from)
        .map(|candidate| candidate.item)
        .collect();
    let mut omitted = retain_from as u64;
    let mut notes = input.coverage.notes.clone();
    if omitted > 0 {
        record_omission_note(
            &mut notes,
            format!("Omitted {omitted} oldest verified history items to satisfy the item bound"),
        )?;
    }

    let identity = CodingSessionContextIdentity {
        session_ref: input.session_ref.clone(),
        genesis_ref: input.genesis_ref.clone(),
        channel_id: input.channel_id,
        name: name.or(fallback_title),
        goal,
        project_ref: project_ref.flatten(),
    };

    loop {
        let package = CodingSessionContextPackage {
            v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
            session: identity.clone(),
            provenance: CodingSessionContextProvenance {
                generated_at: input.generated_at,
                complete: input.coverage.complete,
                truncated: omitted > 0,
                source_event_count: source_event_count(input) as u64,
                included_history_items: history.len() as u64,
                omitted_history_items: omitted,
                total_history_items: input.coverage.complete.then_some(verified_total),
                notes: notes.clone(),
            },
            history,
        };
        let encoded_len = serde_json::to_vec(&package)
            .map_err(|error| ContextProjectionError::InvalidFact(error.to_string()))?
            .len();
        if encoded_len <= input.limits.max_package_bytes {
            package
                .validate()
                .map_err(ContextProjectionError::InvalidFact)?;
            return Ok(package);
        }
        history = package.history;
        if history.is_empty() {
            return Err(ContextProjectionError::Bound(format!(
                "identity and provenance alone exceed {} package bytes",
                input.limits.max_package_bytes
            )));
        }
        history.remove(0);
        omitted += 1;
        record_omission_note(
            &mut notes,
            format!("Omitted {omitted} oldest verified history items to satisfy package bounds"),
        )?;
    }
}

fn record_omission_note(
    notes: &mut Vec<String>,
    note: String,
) -> Result<(), ContextProjectionError> {
    notes.retain(|existing| !existing.starts_with("Omitted "));
    if notes.len() >= MAX_CONTEXT_PROVENANCE_NOTES {
        return Err(ContextProjectionError::Bound(
            "no provenance-note capacity remains to disclose package truncation".into(),
        ));
    }
    notes.push(note);
    Ok(())
}

fn validate_limits(limits: ContextProjectionLimits) -> Result<(), ContextProjectionError> {
    if limits.max_history_items == 0 || limits.max_history_items > MAX_CONTEXT_HISTORY_ITEMS {
        return Err(ContextProjectionError::Bound(format!(
            "max_history_items must be 1..={MAX_CONTEXT_HISTORY_ITEMS}"
        )));
    }
    if limits.max_package_bytes == 0 || limits.max_package_bytes > MAX_CONTEXT_PACKAGE_BYTES {
        return Err(ContextProjectionError::Bound(format!(
            "max_package_bytes must be 1..={MAX_CONTEXT_PACKAGE_BYTES}"
        )));
    }
    Ok(())
}

fn validate_coverage(coverage: &ContextSourceCoverage) -> Result<(), ContextProjectionError> {
    if coverage.complete != coverage.total_history_items.is_some() {
        return Err(ContextProjectionError::InvalidFact(
            "complete source coverage requires totalHistoryItems, and incomplete coverage must not claim a total"
                .into(),
        ));
    }
    if coverage.notes.len() > MAX_CONTEXT_PROVENANCE_NOTES {
        return Err(ContextProjectionError::Bound(format!(
            "coverage exceeds {MAX_CONTEXT_PROVENANCE_NOTES} provenance notes"
        )));
    }
    for note in &coverage.notes {
        if note.trim().is_empty() || note.len() > MAX_CONTEXT_PROVENANCE_NOTE_BYTES {
            return Err(ContextProjectionError::InvalidFact(format!(
                "coverage notes must contain text and be at most {MAX_CONTEXT_PROVENANCE_NOTE_BYTES} bytes"
            )));
        }
    }
    Ok(())
}

fn source_event_count(input: &ContextProjectionInput) -> usize {
    1 + input.authority_links.len() * 2
        + input.name_revisions.len()
        + input.goal_revisions.len()
        + input
            .executions
            .iter()
            .flat_map(|execution| &execution.generations)
            .map(|generation| 3 + generation.transcript.len())
            .sum::<usize>()
}

fn validate_source_bound(input: &ContextProjectionInput) -> Result<(), ContextProjectionError> {
    let count = source_event_count(input);
    if count > MAX_CONTEXT_SOURCE_EVENTS {
        return Err(ContextProjectionError::Bound(format!(
            "source has {count} events, max {MAX_CONTEXT_SOURCE_EVENTS}"
        )));
    }
    let mut bytes = input.genesis.content.len();
    for event in all_events(input).skip(1) {
        bytes = bytes.checked_add(event.content.len()).ok_or_else(|| {
            ContextProjectionError::Bound("source content byte count overflow".into())
        })?;
    }
    if bytes > MAX_CONTEXT_SOURCE_CONTENT_BYTES {
        return Err(ContextProjectionError::Bound(format!(
            "source has {bytes} content bytes, max {MAX_CONTEXT_SOURCE_CONTENT_BYTES}"
        )));
    }
    Ok(())
}

fn all_events(input: &ContextProjectionInput) -> impl Iterator<Item = &Event> {
    std::iter::once(&input.genesis)
        .chain(
            input
                .authority_links
                .iter()
                .flat_map(|link| [&link.receipt, &link.transition]),
        )
        .chain(input.name_revisions.iter())
        .chain(input.goal_revisions.iter())
        .chain(input.executions.iter().flat_map(|execution| {
            execution.generations.iter().flat_map(|generation| {
                [
                    &generation.lifecycle_command,
                    &generation.receipt,
                    &generation.metadata,
                ]
                .into_iter()
                .chain(generation.transcript.iter())
            })
        }))
}

fn verify_signed(event: &Event, label: &str) -> Result<(), ContextProjectionError> {
    verify_event(event).map_err(|error| {
        ContextProjectionError::InvalidFact(format!("{label} signature/id verification: {error}"))
    })
}

fn verify_genesis(input: &ContextProjectionInput) -> Result<String, ContextProjectionError> {
    verify_signed(&input.genesis, "genesis")?;
    if event_kind_u32(&input.genesis) != KIND_CODING_SESSION_GENESIS {
        return Err(ContextProjectionError::InvalidFact(
            "genesis has the wrong kind".into(),
        ));
    }
    if input.genesis.id.to_hex() != input.genesis_ref {
        return Err(ContextProjectionError::InvalidFact(
            "resolved genesis id disagrees with genesisRef".into(),
        ));
    }
    let payload = decode_coding_session_genesis(&input.genesis.content)
        .map_err(ContextProjectionError::InvalidFact)?;
    if payload.session_ref != input.session_ref {
        return Err(ContextProjectionError::InvalidFact(
            "genesis sessionRef disagrees with projection request".into(),
        ));
    }
    require_exact_tags(
        &input.genesis,
        &[
            ("h", input.channel_id.to_string()),
            ("csg-v", CODING_SESSION_GENESIS_TAG_VERSION.into()),
            ("csg-session", input.session_ref.clone()),
        ],
        "genesis",
    )?;
    Ok(input.genesis.pubkey.to_hex())
}

fn verify_authority_chain(
    input: &ContextProjectionInput,
    founder: &str,
) -> Result<Vec<Grant>, ContextProjectionError> {
    let mut links = Vec::with_capacity(input.authority_links.len());
    for link in &input.authority_links {
        verify_signed(&link.receipt, "authority acceptance receipt")?;
        verify_signed(&link.transition, "accepted authority transition")?;
        let relay_self_pubkey = input.relay_self_pubkey.as_deref().ok_or_else(|| {
            ContextProjectionError::InvalidFact(
                "relay has no stable identity needed to verify an authority chain".into(),
            )
        })?;
        let accepted =
            verify_acceptance_receipt(&link.receipt, relay_self_pubkey, input.channel_id)
                .map_err(ContextProjectionError::InvalidFact)?;
        if accepted.genesis_ref != input.genesis_ref {
            return Err(ContextProjectionError::InvalidFact(
                "authority receipt names a different genesis".into(),
            ));
        }
        verify_accepted_transition(&link.transition, &accepted, input.channel_id, founder)
            .map_err(ContextProjectionError::InvalidFact)?;
        let payload = buzz_core::coding_session_authority_transition::decode_coding_session_authority_transition(
            &link.transition.content,
        )
        .map_err(ContextProjectionError::InvalidFact)?;
        links.push((
            accepted,
            payload.prev_accepted,
            link.receipt.created_at.as_secs(),
        ));
    }
    links.sort_by_key(|(accepted, _, _)| accepted.seq);

    let mut previous: Option<String> = None;
    let mut grants = Vec::with_capacity(links.len());
    for (index, (accepted, prev_accepted, accepted_at)) in links.into_iter().enumerate() {
        let expected_seq = (index + 1) as u32;
        if accepted.seq != expected_seq {
            return Err(ContextProjectionError::Conflict(format!(
                "authority chain is not contiguous at seq {expected_seq}"
            )));
        }
        if prev_accepted != previous {
            return Err(ContextProjectionError::Conflict(format!(
                "authority transition {} does not extend the previous accepted head",
                accepted.accepted_event_id
            )));
        }
        previous = Some(accepted.accepted_event_id);
        grants.push(Grant {
            grantee: accepted.grantee_pubkey,
            accepted_at,
        });
    }
    Ok(grants)
}

fn verify_latest_text_revisions(
    events: &[Event],
    input: &ContextProjectionInput,
    founder: &str,
    kind: u32,
) -> Result<Option<String>, ContextProjectionError> {
    for event in events {
        verify_signed(event, "session text revision")?;
        if event_kind_u32(event) != kind
            || event.pubkey.to_hex() != founder
            || tag_value(event, "h") != Some(input.channel_id.to_string())
            || tag_value(event, "d") != Some(input.session_ref.clone())
        {
            return Err(ContextProjectionError::InvalidFact(
                "session text revision has wrong kind, signer, channel, or sessionRef".into(),
            ));
        }
        match kind {
            KIND_CODING_SESSION_NAME => validate_coding_session_name_envelope(event),
            KIND_CODING_SESSION_GOAL => validate_coding_session_goal_envelope(event),
            _ => Err("unsupported session text revision kind".into()),
        }
        .map_err(ContextProjectionError::InvalidFact)?;
    }
    let latest = match kind {
        KIND_CODING_SESSION_NAME => latest_coding_session_name(events.iter()),
        KIND_CODING_SESSION_GOAL => latest_coding_session_goal(events.iter()),
        _ => None,
    };
    Ok(latest.map(|event| event.content.clone()))
}

fn verify_generation(
    input: &ContextProjectionInput,
    founder: &str,
    grants: &[Grant],
    previous: Option<&VerifiedGeneration>,
    facts: &ContextGenerationFacts,
) -> Result<VerifiedGeneration, ContextProjectionError> {
    let command =
        verify_lifecycle_command(&facts.lifecycle_command, input.channel_id, founder, grants)?;
    let (provider_authority, provider_instance_ref, project_ref, title) =
        match (&command.action, previous) {
            (
                CodingSessionLifecycleAction::SessionCreate {
                    project_ref,
                    session_ref,
                    genesis_ref,
                    provider_instance_ref,
                    provider_authority_pubkey,
                    title,
                    ..
                },
                None,
            ) => {
                if session_ref.as_deref() != Some(input.session_ref.as_str())
                    || genesis_ref.as_deref() != Some(input.genesis_ref.as_str())
                {
                    return Err(ContextProjectionError::InvalidFact(
                        "create does not explicitly link the requested session and genesis".into(),
                    ));
                }
                (
                    provider_authority_pubkey.clone(),
                    provider_instance_ref.clone(),
                    project_ref.clone(),
                    title.clone(),
                )
            }
            (
                CodingSessionLifecycleAction::SessionResume {
                    session,
                    provider_authority_pubkey,
                },
                Some(previous),
            ) => {
                if session != &previous.target
                    || provider_authority_pubkey != &previous.provider_authority
                {
                    return Err(ContextProjectionError::InvalidFact(
                        "resume does not address the previous generation and provider authority"
                            .into(),
                    ));
                }
                (
                    previous.provider_authority.clone(),
                    previous.provider_instance_ref.clone(),
                    previous.project_ref.clone(),
                    previous.title.clone(),
                )
            }
            (CodingSessionLifecycleAction::SessionCreate { .. }, Some(_)) => {
                return Err(ContextProjectionError::Conflict(
                    "a provider execution contains a second create".into(),
                ));
            }
            _ => {
                return Err(ContextProjectionError::InvalidFact(
                    "first generation must be create and later generations must be resume".into(),
                ));
            }
        };

    let receipt = verify_receipt(
        &facts.receipt,
        input.channel_id,
        &provider_authority,
        &command,
        previous,
    )?;
    let target = receipt.session.ok_or_else(|| {
        ContextProjectionError::InvalidFact("successful lifecycle receipt has no target".into())
    })?;
    if let Some(previous) = previous {
        if target.driver != previous.target.driver
            || target.instance_id != previous.target.instance_id
            || target.session_id != previous.target.session_id
            || target.generation != previous.target.generation + 1
        {
            return Err(ContextProjectionError::Conflict(
                "resume receipt does not advance the same target by one generation".into(),
            ));
        }
    } else if target.generation != 1 {
        return Err(ContextProjectionError::Conflict(
            "create receipt must establish generation one".into(),
        ));
    }

    let metadata = verify_metadata(
        &facts.metadata,
        input.channel_id,
        &provider_authority,
        &target,
        &provider_instance_ref,
        &project_ref,
        &input.session_ref,
    )?;
    Ok(VerifiedGeneration {
        target,
        provider_authority,
        provider_instance_ref,
        project_ref,
        title: metadata.title.or(title),
    })
}

fn verify_lifecycle_command(
    event: &Event,
    channel_id: Uuid,
    founder: &str,
    grants: &[Grant],
) -> Result<CodingSessionLifecycleCommandPayload, ContextProjectionError> {
    verify_signed(event, "lifecycle command")?;
    if event_kind_u32(event) != KIND_CODING_SESSION_LIFECYCLE_COMMAND {
        return Err(ContextProjectionError::InvalidFact(
            "lifecycle command has wrong kind".into(),
        ));
    }
    let command = decode_coding_session_lifecycle_command(&event.content)
        .map_err(ContextProjectionError::InvalidFact)?;
    require_exact_tags(
        event,
        &[
            ("h", channel_id.to_string()),
            ("csl-v", CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION.into()),
            ("csl-command", command.command_id.clone()),
        ],
        "lifecycle command",
    )?;
    let signer = event.pubkey.to_hex();
    let authorized = signer == founder
        || grants.iter().any(|grant| {
            grant.grantee == signer && grant.accepted_at <= event.created_at.as_secs()
        });
    if !authorized {
        return Err(ContextProjectionError::InvalidFact(format!(
            "lifecycle command signer {signer} was not an accepted operator at publication time"
        )));
    }
    Ok(command)
}

fn verify_receipt(
    event: &Event,
    channel_id: Uuid,
    provider_authority: &str,
    command: &CodingSessionLifecycleCommandPayload,
    previous: Option<&VerifiedGeneration>,
) -> Result<LifecycleReceipt, ContextProjectionError> {
    verify_signed(event, "lifecycle receipt")?;
    require_kind_signer_size(
        event,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        provider_authority,
        MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES,
        "lifecycle receipt",
    )?;
    require_exact_json_fields(
        &event.content,
        &["schema", "commandId", "status", "session", "error"],
        "lifecycle receipt",
    )?;
    let receipt: LifecycleReceipt = serde_json::from_str(&event.content).map_err(|_| {
        ContextProjectionError::InvalidFact("lifecycle receipt content is malformed".into())
    })?;
    if receipt.schema != LIFECYCLE_RECEIPT_SCHEMA || receipt.command_id != command.command_id {
        return Err(ContextProjectionError::InvalidFact(
            "lifecycle receipt schema or commandId disagrees with command".into(),
        ));
    }
    let valid_status = matches!(
        (&command.action, receipt.status, previous),
        (
            CodingSessionLifecycleAction::SessionCreate { .. },
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn,
            None,
        ) | (
            CodingSessionLifecycleAction::SessionResume { .. },
            ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext,
            Some(_),
        )
    );
    if !valid_status || receipt.session.is_none() {
        return Err(ContextProjectionError::InvalidFact(
            "lifecycle receipt is not a successful outcome for this command".into(),
        ));
    }
    require_exact_tags(
        event,
        &[
            ("h", channel_id.to_string()),
            (
                "cslr-v",
                CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION.into(),
            ),
            ("csl-command", command.command_id.clone()),
            (
                "csl-key",
                coding_session_lifecycle_receipt_semantic_key(&command.command_id),
            ),
        ],
        "lifecycle receipt",
    )?;
    Ok(receipt)
}

#[allow(clippy::too_many_arguments)]
fn verify_metadata(
    event: &Event,
    channel_id: Uuid,
    provider_authority: &str,
    target: &CodingSessionTarget,
    provider_instance_ref: &str,
    project_ref: &Option<String>,
    session_ref: &str,
) -> Result<SessionMetadata, ContextProjectionError> {
    verify_signed(event, "session metadata")?;
    require_kind_signer_size(
        event,
        KIND_CODING_SESSION_METADATA,
        provider_authority,
        MAX_METADATA_CONTENT_BYTES,
        "session metadata",
    )?;
    let metadata = decode_coding_session_metadata(&event.content)
        .map_err(ContextProjectionError::InvalidFact)?;
    if metadata.schema != METADATA_SCHEMA
        || &metadata.session != target
        || metadata.session_ref.as_deref() != Some(session_ref)
        || &metadata.project_ref != project_ref
        || metadata.provider.as_deref() != Some(provider_instance_ref)
    {
        return Err(ContextProjectionError::InvalidFact(
            "session metadata disagrees with its create/provider/target chain".into(),
        ));
    }
    require_exact_tags(
        event,
        &[
            ("h", channel_id.to_string()),
            ("csm-v", CODING_SESSION_METADATA_TAG_VERSION.into()),
            ("cs-target", coding_session_target_key(target)),
            ("csm-key", coding_session_metadata_semantic_key(target)),
        ],
        "session metadata",
    )?;
    Ok(metadata)
}

fn verify_transcript(
    event: &Event,
    channel_id: Uuid,
    target: &CodingSessionTarget,
    provider_authority: &str,
) -> Result<CandidateHistory, ContextProjectionError> {
    verify_signed(event, "transcript item")?;
    require_kind_signer_size(
        event,
        KIND_CODING_SESSION_TRANSCRIPT,
        provider_authority,
        MAX_TRANSCRIPT_CONTENT_BYTES,
        "transcript item",
    )?;
    require_exact_json_fields(
        &event.content,
        &[
            "schema",
            "session",
            "eventSeq",
            "timestamp",
            "turnId",
            "item",
        ],
        "transcript item",
    )?;
    let envelope: TranscriptEnvelope = serde_json::from_str(&event.content).map_err(|_| {
        ContextProjectionError::InvalidFact("transcript content is malformed".into())
    })?;
    if envelope.schema != TRANSCRIPT_SCHEMA
        || &envelope.session != target
        || envelope.event_seq == 0
        || envelope.timestamp < 0
    {
        return Err(ContextProjectionError::InvalidFact(
            "transcript schema, target, sequence, or timestamp is invalid".into(),
        ));
    }
    let item_kind = envelope
        .item
        .as_object()
        .and_then(|item| item.get("kind"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ContextProjectionError::InvalidFact("transcript item must carry a string kind".into())
        })?
        .to_owned();
    let role = coding_session_context_role_for_item_kind(&item_kind).ok_or_else(|| {
        ContextProjectionError::InvalidFact(format!(
            "transcript item kind {item_kind:?} is not supported for rehydration"
        ))
    })?;
    require_exact_tags(
        event,
        &[
            ("h", channel_id.to_string()),
            ("cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION.into()),
            ("cs-target", coding_session_target_key(target)),
            ("cst-seq", envelope.event_seq.to_string()),
            (
                "cst-key",
                coding_session_transcript_semantic_key(target, envelope.event_seq),
            ),
        ],
        "transcript item",
    )?;
    Ok(CandidateHistory {
        timestamp_ms: envelope.timestamp,
        item: CodingSessionContextHistoryItem {
            event_id: event.id.to_hex(),
            created_at: event.created_at.as_secs(),
            author: event.pubkey.to_hex(),
            source_kind: KIND_CODING_SESSION_TRANSCRIPT,
            target: target.clone(),
            event_seq: envelope.event_seq,
            turn_id: envelope.turn_id,
            role,
            item_kind,
            content: envelope.item,
        },
    })
}

fn require_kind_signer_size(
    event: &Event,
    kind: u32,
    signer: &str,
    max_content_bytes: usize,
    label: &str,
) -> Result<(), ContextProjectionError> {
    if event_kind_u32(event) != kind
        || event.pubkey.to_hex() != signer
        || event.content.len() > max_content_bytes
    {
        return Err(ContextProjectionError::InvalidFact(format!(
            "{label} has wrong kind, signer, or content size"
        )));
    }
    Ok(())
}

fn require_exact_tags(
    event: &Event,
    expected: &[(&str, String)],
    label: &str,
) -> Result<(), ContextProjectionError> {
    let actual: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if actual.len() != expected.len()
        || actual.iter().zip(expected).any(|(actual, expected)| {
            actual.len() != 2 || actual[0] != expected.0 || actual[1] != expected.1
        })
    {
        return Err(ContextProjectionError::InvalidFact(format!(
            "{label} has a malformed or substituted tag envelope"
        )));
    }
    Ok(())
}

fn require_exact_json_fields(
    content: &str,
    expected: &[&str],
    label: &str,
) -> Result<(), ContextProjectionError> {
    let value: Value = serde_json::from_str(content).map_err(|_| {
        ContextProjectionError::InvalidFact(format!("{label} content is malformed"))
    })?;
    let object = value.as_object().ok_or_else(|| {
        ContextProjectionError::InvalidFact(format!("{label} content must be an object"))
    })?;
    if object.len() != expected.len()
        || !expected.iter().all(|field| object.contains_key(*field))
        || !object
            .keys()
            .all(|field| expected.contains(&field.as_str()))
    {
        return Err(ContextProjectionError::InvalidFact(format!(
            "{label} has missing or unsupported fields"
        )));
    }
    Ok(())
}

fn tag_value(event: &Event, name: &str) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        let parts = tag.as_slice();
        (parts.len() == 2 && parts[0] == name).then(|| parts[1].clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Timestamp};

    use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
    use buzz_core::coding_session_lifecycle_command::{
        CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
        CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    };
    use buzz_core::coding_session_payload::{Capabilities, SessionStatus, TranscriptEnvelope};
    use buzz_sdk::builders::{
        build_coding_session_genesis, build_coding_session_lifecycle_command,
        build_coding_session_lifecycle_receipt, build_coding_session_metadata,
        build_coding_session_transcript_item,
    };

    const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    struct Fixture {
        input: ContextProjectionInput,
        provider: Keys,
    }

    fn fixture(items: usize) -> Fixture {
        let founder = Keys::generate();
        let provider = Keys::generate();
        let relay = Keys::generate();
        let channel_id = Uuid::new_v4();
        let genesis = build_coding_session_genesis(
            channel_id,
            &CodingSessionGenesisPayload::new(SESSION_REF),
        )
        .unwrap()
        .sign_with_keys(&founder)
        .unwrap();
        let genesis_ref = genesis.id.to_hex();
        let create = CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: "create-1".into(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref: None,
                repo_ref: None,
                session_ref: Some(SESSION_REF.into()),
                genesis_ref: Some(genesis_ref.clone()),
                provider_instance_ref: "codex-primary".into(),
                provider_authority_pubkey: provider.public_key().to_hex(),
                model: Some("default".into()),
                title: Some("Rehydrate me".into()),
                initial_turn: None,
            },
        };
        let create_event = build_coding_session_lifecycle_command(channel_id, &create)
            .unwrap()
            .sign_with_keys(&founder)
            .unwrap();
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "provider-host-1".into(),
            session_id: "session-1".into(),
            generation: 1,
        };
        let receipt = LifecycleReceipt::created(&create.command_id, &target);
        let receipt_content = serde_json::to_string(&receipt).unwrap();
        let receipt_event = build_coding_session_lifecycle_receipt(
            channel_id,
            &create.command_id,
            &receipt_content,
        )
        .unwrap()
        .sign_with_keys(&provider)
        .unwrap();
        let metadata = SessionMetadata {
            schema: METADATA_SCHEMA.into(),
            session: target.clone(),
            project_ref: None,
            repo_ref: None,
            title: Some("Rehydrate me".into()),
            agent_ref: None,
            provider: Some("codex-primary".into()),
            runtime: Some("codex".into()),
            model: Some("default".into()),
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_baseline(),
            session_ref: Some(SESSION_REF.into()),
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
        };
        let metadata_content = serde_json::to_string(&metadata).unwrap();
        let metadata_event = build_coding_session_metadata(channel_id, &target, &metadata_content)
            .unwrap()
            .sign_with_keys(&provider)
            .unwrap();
        let transcript = (1..=items)
            .map(|seq| {
                let kind = if seq % 2 == 1 {
                    "user_prompt"
                } else {
                    "assistant_text"
                };
                let item = if kind == "user_prompt" {
                    serde_json::json!({"kind": kind, "content": format!("prompt {seq}"), "steered": false})
                } else {
                    serde_json::json!({"kind": kind, "text": format!("answer {seq}")})
                };
                let envelope = TranscriptEnvelope::new(
                    &target,
                    seq as u64,
                    seq as i64 * 1_000,
                    Some("turn-1"),
                    item,
                );
                let content = serde_json::to_string(&envelope).unwrap();
                build_coding_session_transcript_item(channel_id, &target, seq as u64, &content)
                    .unwrap()
                    .custom_created_at(Timestamp::from(seq as u64))
                    .sign_with_keys(&provider)
                    .unwrap()
            })
            .collect();
        Fixture {
            input: ContextProjectionInput {
                channel_id,
                session_ref: SESSION_REF.into(),
                genesis_ref,
                relay_self_pubkey: Some(relay.public_key().to_hex()),
                genesis,
                authority_links: Vec::new(),
                executions: vec![ContextExecutionFacts {
                    generations: vec![ContextGenerationFacts {
                        lifecycle_command: create_event,
                        receipt: receipt_event,
                        metadata: metadata_event,
                        transcript,
                    }],
                }],
                name_revisions: Vec::new(),
                goal_revisions: Vec::new(),
                coverage: ContextSourceCoverage {
                    complete: true,
                    total_history_items: Some(items as u64),
                    notes: vec!["Relay query reached EOSE".into()],
                },
                generated_at: 5_000,
                limits: ContextProjectionLimits::default(),
            },
            provider,
        }
    }

    #[test]
    fn projects_only_the_exact_verified_provider_chain() {
        let fixture = fixture(4);
        let package = project_session_context(&fixture.input).unwrap();
        package.validate().unwrap();
        assert_eq!(package.session.session_ref, SESSION_REF);
        assert_eq!(package.session.name.as_deref(), Some("Rehydrate me"));
        assert_eq!(package.history.len(), 4);
        assert!(package.provenance.complete);
        assert!(!package.provenance.truncated);
        assert_eq!(
            package.history[0].role,
            buzz_core::coding_session_context::CodingSessionContextRole::User
        );
        assert_eq!(
            package.history[1].role,
            buzz_core::coding_session_context::CodingSessionContextRole::Assistant
        );
    }

    #[test]
    fn founder_only_session_projects_without_a_stable_relay_identity() {
        let mut fixture = fixture(4);
        fixture.input.relay_self_pubkey = None;

        let package = project_session_context(&fixture.input).unwrap();

        assert_eq!(package.history.len(), 4);
        assert!(package.provenance.complete);
    }

    #[test]
    fn groups_a_complete_relay_fact_set_before_projection() {
        let fixture = fixture(4);
        let generation = &fixture.input.executions[0].generations[0];
        let mut events = vec![
            fixture.input.genesis.clone(),
            generation.lifecycle_command.clone(),
            generation.receipt.clone(),
            generation.metadata.clone(),
        ];
        events.extend(generation.transcript.iter().cloned());
        let request = ContextProjectionRequest {
            channel_id: fixture.input.channel_id,
            session_ref: fixture.input.session_ref.clone(),
            genesis_ref: fixture.input.genesis_ref.clone(),
            relay_self_pubkey: fixture.input.relay_self_pubkey.clone(),
            generated_at: fixture.input.generated_at,
            limits: ContextProjectionLimits::default(),
        };

        let package = project_session_context_events(&request, &events).unwrap();

        assert_eq!(package.history.len(), 4);
        assert!(package.provenance.complete);
        assert_eq!(package.provenance.total_history_items, Some(4));
        assert_eq!(package.provenance.source_event_count, 8);
    }

    #[test]
    fn incomplete_source_coverage_is_not_mislabeled_as_package_truncation() {
        let fixture = fixture(2);
        let generation = &fixture.input.executions[0].generations[0];
        let mut events = vec![
            fixture.input.genesis.clone(),
            generation.lifecycle_command.clone(),
            generation.receipt.clone(),
            generation.metadata.clone(),
        ];
        events.extend(generation.transcript.iter().cloned());
        let request = ContextProjectionRequest {
            channel_id: fixture.input.channel_id,
            session_ref: fixture.input.session_ref.clone(),
            genesis_ref: fixture.input.genesis_ref.clone(),
            relay_self_pubkey: fixture.input.relay_self_pubkey.clone(),
            generated_at: fixture.input.generated_at,
            limits: ContextProjectionLimits::default(),
        };

        let package = group_and_project_session_context_events(
            &request,
            &events,
            ContextSourceCoverage {
                complete: false,
                total_history_items: None,
                notes: vec!["Transcript partition reached the relay row clamp".into()],
            },
        )
        .unwrap();

        assert!(!package.provenance.complete);
        assert!(!package.provenance.truncated);
        assert_eq!(package.provenance.total_history_items, None);
    }

    #[test]
    fn wrong_provider_signature_and_sequence_conflict_fail_closed() {
        {
            let mut fixture = fixture(2);
            let impostor = Keys::generate();
            let original = &fixture.input.executions[0].generations[0].transcript[0];
            fixture.input.executions[0].generations[0].transcript[0] =
                EventBuilder::new(original.kind, original.content.clone())
                    .tags(original.tags.clone())
                    .sign_with_keys(&impostor)
                    .unwrap();
            assert!(matches!(
                project_session_context(&fixture.input),
                Err(ContextProjectionError::InvalidFact(_))
            ));
        }

        {
            let mut fixture = fixture(2);
            let duplicate = fixture.input.executions[0].generations[0].transcript[0].clone();
            fixture.input.executions[0].generations[0]
                .transcript
                .push(duplicate);
            fixture.input.coverage.total_history_items = Some(3);
            assert!(matches!(
                project_session_context(&fixture.input),
                Err(ContextProjectionError::Conflict(_))
            ));
        }
    }

    #[test]
    fn truncation_is_explicit_and_retains_the_newest_verified_items() {
        let mut fixture = fixture(5);
        fixture.input.limits.max_history_items = 2;
        let package = project_session_context(&fixture.input).unwrap();
        assert!(package.provenance.complete);
        assert!(package.provenance.truncated);
        assert_eq!(package.provenance.included_history_items, 2);
        assert_eq!(package.provenance.omitted_history_items, 3);
        assert_eq!(package.provenance.total_history_items, Some(5));
        assert_eq!(package.history[0].event_seq, 4);
        assert_eq!(package.history[1].event_seq, 5);
    }

    #[test]
    fn a_complete_query_allows_provider_burned_sequence_gaps() {
        let mut fixture = fixture(2);
        fixture.input.executions[0].generations[0]
            .transcript
            .remove(0);
        fixture.input.coverage.total_history_items = Some(1);

        let package = project_session_context(&fixture.input).unwrap();
        assert!(package.provenance.complete);
        assert_eq!(package.history.len(), 1);
        assert_eq!(package.history[0].event_seq, 2);
    }

    #[test]
    fn no_native_cursor_or_signing_key_can_enter_the_package_shape() {
        let fixture = fixture(1);
        let package = project_session_context(&fixture.input).unwrap();
        let encoded = serde_json::to_string(&package).unwrap();
        assert!(!encoded.contains("privateKey"));
        assert!(!encoded.contains("resumeCursor"));
        assert!(!encoded.contains("acpSessionId"));
        assert!(!encoded.contains("workingDirectory"));
        assert_ne!(fixture.provider.public_key().to_hex(), "");
    }
}
