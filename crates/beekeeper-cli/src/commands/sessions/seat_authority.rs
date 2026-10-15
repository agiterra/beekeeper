//! Signer-owned authority grant after a receipt-backed hire.

use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
use beekeeper_core::role_vocabulary::RoleVocabulary;
use beekeeper_sdk::builders::build_coding_session_authority_transition;
use serde_json::Value;
use uuid::Uuid;

use super::operations::ProjectedAuthority;
use super::operations_authority::fetch_projected_authority_with;
use super::operations_reads::fetch_founder_context;
use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The role vocabulary of the project this command runs inside.
///
/// Read from the nearest `team.yml` — the directory itself or the
/// `<dir>-agents` clone the host cuts beside a seat's worktree, walking up
/// from the current directory — so a `bee` run inside a seat judges with the
/// seat's own project's names. With no manifest near, the default
/// (`lead` / `verifier`, `runner`) applies; see
/// [`RoleVocabulary::foundation`].
///
/// # Errors
/// A manifest that is near but cannot be read or does not parse: that is a
/// broken project record, not a reason to guess the default.
pub(crate) fn cwd_role_vocabulary() -> Result<RoleVocabulary, CliError> {
    let Ok(cwd) = std::env::current_dir() else {
        return Ok(RoleVocabulary::foundation().clone());
    };
    match beekeeper_persona::team::role_vocabulary_near(&cwd) {
        Ok(Some((_, vocabulary))) => Ok(vocabulary),
        Ok(None) => Ok(RoleVocabulary::foundation().clone()),
        Err(error) => Err(CliError::Usage(format!(
            "the project's team manifest beside this directory does not parse, so its role \
             names cannot be read: {error}"
        ))),
    }
}

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
    vocabulary: &RoleVocabulary,
) -> Result<SeatGrantDecision, CliError> {
    let lead_role = vocabulary.lead();
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
        .any(|seat| seat.actor_pubkey == signer && seat.role == lead_role);
    if !(is_founder || is_operator || is_lead) {
        return Err(CliError::Refused(
            "the hire signer is not the founder, an active steering operator, or an active lead"
                .into(),
        ));
    }
    if is_lead && !is_founder && !is_operator && role == lead_role {
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
    client: &BeekeeperClient,
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
    // The hire pre-check predicts whether the relay will admit the seat, so
    // it judges with the vocabulary the relay enforces: the foundation words,
    // since the relay's chain insert and the db's hire check do not read
    // `team.yml` yet (NIP-CSTX § What the relay enforces today). With the
    // project's own words this said "allowed" for a `captain` lead and the
    // relay refused. `cwd_role_vocabulary()` stays for the provider-side
    // readers (briefs, wakes, verification input) the relay does not gate.
    let vocabulary = RoleVocabulary::foundation().clone();

    for attempt in 0..2 {
        let authority = fetch_projected_authority_with(
            client,
            channel,
            genesis,
            &context.founder_pubkey,
            &vocabulary,
        )
        .await?;
        match decide_seat_grant(
            &authority,
            &context.founder_pubkey,
            &signer,
            actor,
            role,
            &vocabulary,
        )? {
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

        let projected = fetch_projected_authority_with(
            client,
            &channel_uuid.to_string(),
            genesis,
            &context.founder_pubkey,
            &vocabulary,
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
    decide_for_test_with(
        authority,
        founder,
        signer,
        actor,
        role,
        RoleVocabulary::foundation(),
    )
}

#[cfg(test)]
pub(super) fn decide_for_test_with(
    authority: &ProjectedAuthority,
    founder: &str,
    signer: &str,
    actor: &str,
    role: &str,
    vocabulary: &RoleVocabulary,
) -> Result<(Option<String>, bool), CliError> {
    decide_seat_grant(authority, founder, signer, actor, role, vocabulary).map(|decision| {
        match decision {
            SeatGrantDecision::Existing(id) => (Some(id), false),
            SeatGrantDecision::Append => (None, true),
        }
    })
}

#[cfg(test)]
pub(super) fn should_retry_for_test(attempt: usize, error: &CliError) -> bool {
    should_retry_head_conflict(attempt, error)
}
