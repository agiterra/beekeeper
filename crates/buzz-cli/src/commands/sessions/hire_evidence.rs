//! Signed evidence fence between provider creation and role-seat authority.

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::coding_session_identity::ProviderInstanceAlias;
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
};
use buzz_core::coding_session_payload::{LifecycleReceipt, SessionMetadata};
use buzz_core::kind::{KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LIFECYCLE_COMMAND};
use buzz_sdk::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, coding_session_metadata_semantic_key,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION, CODING_SESSION_METADATA_TAG_VERSION,
};
use buzz_sdk::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA};
use nostr::Event;
use serde_json::Value;

use super::crew::{HiredSeat, SeatReceipt};
use crate::error::CliError;

fn signed_event(value: &Value, label: &str) -> Result<Event, CliError> {
    let event: Event = serde_json::from_value(value.clone())
        .map_err(|error| CliError::Other(format!("malformed signed {label}: {error}")))?;
    buzz_core::verify_event(&event)
        .map_err(|error| CliError::Other(format!("invalid {label} signature: {error}")))?;
    Ok(event)
}

fn exact_tags(event: &Event, expected: &[(&str, &str)]) -> bool {
    event.tags.len() == expected.len()
        && event.tags.iter().zip(expected).all(|(tag, (name, value))| {
            tag.as_slice().len() == 2 && tag.as_slice() == [*name, *value]
        })
}

/// Immutable request context the observed provider answer must satisfy.
pub(super) struct HireEvidenceRequest<'a> {
    pub(super) channel: &'a str,
    pub(super) session_ref: &'a str,
    pub(super) genesis: &'a str,
    pub(super) role: &'a str,
    pub(super) provider_instance: Option<&'a str>,
}

/// The exact signed genesis founding `session_ref` in `channel`, verified.
///
/// Split out of [`verify_hire_evidence`] so the founder pubkey can be
/// established *before* candidate creates are chosen rather than after one has
/// already been picked. `seat-repair` needs it as a filter — a create signed by
/// anyone but the founder is not a candidate at all — and there must be exactly
/// one implementation of "which key founds this umbrella".
pub(super) fn verified_genesis(
    events: &[Value],
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<Event, CliError> {
    let genesis_event = events
        .iter()
        .find_map(|value| {
            let event = signed_event(value, "coding-session genesis").ok()?;
            (u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_GENESIS
                && event.id.to_hex() == genesis)
                .then_some(event)
        })
        .ok_or_else(|| {
            CliError::Other(
                "the exact signed coding-session genesis was not present in the hire evidence"
                    .into(),
            )
        })?;
    let genesis_payload = decode_coding_session_genesis(&genesis_event.content)
        .map_err(|error| CliError::Other(format!("invalid coding-session genesis: {error}")))?;
    if genesis_payload.session_ref != session_ref
        || !exact_tags(
            &genesis_event,
            &[
                ("h", channel),
                ("csg-v", CODING_SESSION_GENESIS_TAG_VERSION),
                ("csg-session", session_ref),
            ],
        )
    {
        return Err(CliError::Other(
            "the exact coding-session genesis does not found this channel and session".into(),
        ));
    }
    Ok(genesis_event)
}

/// Whether one lifecycle receipt is cryptographically bound to one seated
/// create, returning the execution target it minted when it named one.
///
/// This is the receipt half of [`verify_hire_evidence`], extracted so that
/// evidence *selection* can apply the same bar as evidence *verification*.
/// Choosing a receipt by its self-asserted `created_at` and only then checking
/// the binding lets anyone who can publish a 44224 carrying the commandId deny
/// the repair; filtering by this predicate first means an unbound receipt is
/// invisible rather than fatal, whatever time it claims.
///
/// A failure-class receipt names no target, so the target parity check applies
/// only when one is present; the caller decides whether a missing target is an
/// error for its own status class.
pub(super) fn create_receipt_binding(
    channel: &str,
    seat: &HiredSeat,
    receipt: &SeatReceipt,
) -> Result<Option<CodingSessionTarget>, CliError> {
    let provider_receipt = signed_event(&receipt.raw, "provider create receipt")?;
    let receipt_payload: LifecycleReceipt = serde_json::from_str(&provider_receipt.content)
        .map_err(|error| CliError::Other(format!("invalid provider create receipt: {error}")))?;
    if u32::from(provider_receipt.kind.as_u16()) != KIND_CODING_SESSION_LIFECYCLE_RECEIPT
        || provider_receipt.id.to_hex() != receipt.event_id
        || provider_receipt.pubkey.to_hex() != seat.provider_authority_pubkey
        || receipt.signer != seat.provider_authority_pubkey
        || receipt_payload.command_id != seat.command_id
        || receipt_payload.status != receipt.status
        || !exact_tags(
            &provider_receipt,
            &[
                ("h", channel),
                ("cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION),
                ("csl-command", &seat.command_id),
                (
                    "csl-key",
                    &coding_session_lifecycle_receipt_semantic_key(&seat.command_id),
                ),
            ],
        )
    {
        return Err(CliError::Other(
            "provider receipt is not bound to the signed seated create".into(),
        ));
    }
    let Some(target) = receipt_payload.session else {
        return Ok(None);
    };
    // The receipt's `session.instanceId` is the provider's cryptographic
    // short id (the one every `cs-target` carries); the create's
    // `providerInstanceRef` is the human-facing alias the host chose
    // (`claude-primary`). They are different namespaces and never equal on
    // the wire: on 2026-09-01 the first live `seat-repair` against cleantest
    // refused the real `created` receipt (`3e53c993…`, instance
    // `1958c6c448e05eed`) as "unbound" because it compared them, exactly the
    // defect ledger item 102 removed from the Desktop hire host. The receipt
    // is already pinned to the exact command, channel, and provider signer
    // above; the alias is checked against the provider's own metadata in
    // `verify_hire_evidence`, where both sides speak the alias.
    if receipt.target_key.as_deref() != Some(coding_session_target_key(&target).as_str()) {
        return Err(CliError::Other(
            "provider receipt target does not match the observed seat's target key".into(),
        ));
    }
    Ok(Some(target))
}

/// Prove a successful hire created the exact signed provider execution before
/// the hiring signer may append authority for it.
pub(super) fn verify_hire_evidence(
    events: &[Value],
    request: &HireEvidenceRequest<'_>,
    seat: &HiredSeat,
    receipt: &SeatReceipt,
) -> Result<(), CliError> {
    let genesis_event = verified_genesis(
        events,
        request.channel,
        request.session_ref,
        request.genesis,
    )?;

    let create = signed_event(&seat.raw, "seated create")?;
    if u32::from(create.kind.as_u16()) != KIND_CODING_SESSION_LIFECYCLE_COMMAND
        || create.id.to_hex() != seat.event_id
        || create.pubkey.to_hex() != seat.create_signer
        || !exact_tags(
            &create,
            &[
                ("h", request.channel),
                ("csl-v", CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION),
                ("csl-command", &seat.command_id),
            ],
        )
    {
        return Err(CliError::Other(
            "seated create provenance does not match the requested channel".into(),
        ));
    }
    let payload = decode_coding_session_lifecycle_command(&create.content)
        .map_err(|error| CliError::Other(format!("invalid seated create: {error}")))?;
    let CodingSessionLifecycleAction::SessionCreate {
        session_ref: Some(created_session),
        genesis_ref: Some(created_genesis),
        provider_instance_ref,
        provider_authority_pubkey,
        actor: Some(actor),
        role: Some(role),
        ..
    } = payload.action
    else {
        return Err(CliError::Other(
            "seated create is missing actor, role, session, or genesis".into(),
        ));
    };
    if created_session != request.session_ref
        || created_genesis != request.genesis
        || actor != seat.actor
        || role != request.role
        || seat.role != request.role
        || create.pubkey != genesis_event.pubkey
        || provider_instance_ref != seat.provider_instance_ref
        || request
            .provider_instance
            .is_some_and(|requested| requested != provider_instance_ref.as_str())
        || provider_authority_pubkey != seat.provider_authority_pubkey
    {
        return Err(CliError::Other(
            "seated create actor, role, session, genesis, or provider does not match the hire"
                .into(),
        ));
    }

    // The receipt binding is the same predicate evidence SELECTION uses, so a
    // receipt that reaches here has already been shown to be the provider's.
    let target = create_receipt_binding(request.channel, seat, receipt)?.ok_or_else(|| {
        CliError::Other("successful provider receipt names no execution target".into())
    })?;
    let target = &target;

    let metadata_matches = events.iter().any(|value| {
        let Ok(event) = signed_event(value, "provider metadata") else {
            return false;
        };
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_METADATA
            || event.pubkey.to_hex() != provider_authority_pubkey
        {
            return false;
        }
        serde_json::from_str::<SessionMetadata>(&event.content).is_ok_and(|metadata| {
            exact_tags(
                &event,
                &[
                    ("h", request.channel),
                    ("csm-v", CODING_SESSION_METADATA_TAG_VERSION),
                    ("cs-target", &coding_session_target_key(target)),
                    ("csm-key", &coding_session_metadata_semantic_key(target)),
                ],
            ) && coding_session_target_key(&metadata.session) == coding_session_target_key(target)
                && metadata.agent_ref.as_deref() == Some(actor.as_str())
                && metadata.role.as_deref() == Some(role.as_str())
                && metadata.session_ref.as_deref() == Some(request.session_ref)
                && metadata
                    .provider
                    .as_ref()
                    .map(ProviderInstanceAlias::as_str)
                    == Some(provider_instance_ref.as_str())
            // `metadata.runtime` is the human runtime word (`claude`); the
            // target's `driver` is the ACP driver slug (`claude-agent-acp`).
            // Comparing them refused the real cleantest metadata on
            // 2026-09-01. The driver is already pinned twice above: by the
            // exact `cs-target` tag and by `metadata.session`'s target key.
        })
    });
    if !metadata_matches {
        return Err(CliError::Unconfirmed(
            "the provider has not signed metadata confirming the hired actor-role execution; no seat authority was granted"
                .into(),
        ));
    }
    Ok(())
}
