//! Signer-owned authority grant after a receipt-backed hire.

use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
use beekeeper_sdk::builders::build_coding_session_authority_transition;
use serde_json::Value;
use uuid::Uuid;

use super::operations::{fetch_projected_authority, ProjectedAuthority};
use super::operations_reads::fetch_founder_context;
use crate::client::BuzzClient;
use crate::error::CliError;

/// The accepted authority fact returned with a successful hire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SeatGrantResult {
    pub(super) event_id: String,
    pub(super) already_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SeatGrantDecision {
    Existing(String),
    Append,
}

fn should_retry_head_conflict(attempt: usize, error: &CliError) -> bool {
    attempt == 0 && super::is_chain_head_conflict(error)
}

fn decide_seat_grant(
    authority: &ProjectedAuthority,
    founder: &str,
    signer: &str,
    actor: &str,
    role: &str,
) -> Result<SeatGrantDecision, CliError> {
    if let Some(seat) = authority
        .seats
        .iter()
        .find(|seat| seat.actor_pubkey == actor)
    {
        if seat.role != role {
            return Err(CliError::Refused(format!(
                "actor {actor} already holds role {}; revoke that seat before granting {role}",
                seat.role
            )));
        }
        let event_id = authority.seat_grant_refs.get(actor).ok_or_else(|| {
            CliError::Other("active seat is missing accepted grant provenance".into())
        })?;
        return Ok(SeatGrantDecision::Existing(event_id.clone()));
    }
    if signer == actor {
        return Err(CliError::Refused(
            "the hired actor cannot nominate itself for a role seat".into(),
        ));
    }
    let is_founder = signer == founder;
    let is_operator = authority
        .grants
        .iter()
        .any(|grant| grant.actor_pubkey == signer && grant.may_steer);
    let is_lead = authority
        .seats
        .iter()
        .any(|seat| seat.actor_pubkey == signer && seat.role == "lead");
    if !(is_founder || is_operator || is_lead) {
        return Err(CliError::Refused(
            "the hire signer is not the founder, an active steering operator, or an active lead"
                .into(),
        ));
    }
    if is_lead && !is_founder && !is_operator && role == "lead" {
        return Err(CliError::Refused(
            "an active lead cannot grant lead authority; the founder or a steering operator must do it"
                .into(),
        ));
    }
    Ok(SeatGrantDecision::Append)
}

/// Ensure the exact actor-role pair has an accepted seat grant.
///
/// Reads are idempotent. A successful existing grant is returned without a
/// write; a different active role is never overwritten. A newly submitted
/// transition is re-read through the trusted relay-receipt projection before
/// it is reported accepted.
pub(super) async fn ensure_hired_seat_grant(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
    actor: &str,
    role: &str,
) -> Result<SeatGrantResult, CliError> {
    let context = fetch_founder_context(client, channel, session_ref, genesis).await?;
    let signer = client.keys().public_key().to_hex();
    let channel_uuid = Uuid::parse_str(channel)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;

    for attempt in 0..2 {
        let authority =
            fetch_projected_authority(client, channel, genesis, &context.founder_pubkey).await?;
        match decide_seat_grant(&authority, &context.founder_pubkey, &signer, actor, role)? {
            SeatGrantDecision::Existing(event_id) => {
                return Ok(SeatGrantResult {
                    event_id,
                    already_active: true,
                })
            }
            SeatGrantDecision::Append => {}
        }

        let seq = authority
            .head_seq
            .checked_add(1)
            .ok_or_else(|| CliError::Other("authority chain seq overflow".into()))?;
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_seat(
            genesis,
            authority.head_event_id,
            seq,
            actor,
            role,
        );
        let builder = build_coding_session_authority_transition(channel_uuid, &payload)
            .map_err(|error| CliError::Other(error.to_string()))?;
        let event = client.sign_event_unchecked(builder)?;
        let event_id = event.id.to_hex();
        let outcome = client.submit_event(event).await.and_then(|raw| {
            crate::commands::parse_write_response(&raw, "automatic seat authority already accepted")
        });
        let response = match outcome {
            Ok(response) => response,
            Err(error) if should_retry_head_conflict(attempt, &error) => {
                // Another accepted transition advanced the head after this
                // attempt read it. Rebuild against a fresh receipt-backed
                // projection exactly once; never replay the stale event.
                continue;
            }
            Err(error) => return Err(error),
        };
        let accepted = serde_json::from_str::<Value>(&response)
            .ok()
            .and_then(|value| value.get("accepted").and_then(Value::as_bool));
        if accepted != Some(true) {
            return Err(CliError::Refused(
                "relay response did not prove acceptance of the automatic role-seat grant".into(),
            ));
        }

        let projected = fetch_projected_authority(
            client,
            &channel_uuid.to_string(),
            genesis,
            &context.founder_pubkey,
        )
        .await?;
        let confirmed = projected
            .seats
            .iter()
            .any(|seat| seat.actor_pubkey == actor && seat.role == role)
            && projected.seat_grant_refs.get(actor) == Some(&event_id);
        if !confirmed {
            return Err(CliError::Unconfirmed(format!(
                "seat grant {event_id} was submitted but no trusted acceptance receipt confirms it; inspect the authority chain before retrying"
            )));
        }
        return Ok(SeatGrantResult {
            event_id,
            already_active: false,
        });
    }
    Err(CliError::Other(
        "automatic seat authority exhausted its single head-conflict retry".into(),
    ))
}

#[cfg(test)]
pub(super) fn decide_for_test(
    authority: &ProjectedAuthority,
    founder: &str,
    signer: &str,
    actor: &str,
    role: &str,
) -> Result<(Option<String>, bool), CliError> {
    decide_seat_grant(authority, founder, signer, actor, role).map(|decision| match decision {
        SeatGrantDecision::Existing(id) => (Some(id), false),
        SeatGrantDecision::Append => (None, true),
    })
}

#[cfg(test)]
pub(super) fn should_retry_for_test(attempt: usize, error: &CliError) -> bool {
    should_retry_head_conflict(attempt, error)
}
