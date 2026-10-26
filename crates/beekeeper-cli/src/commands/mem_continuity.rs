//! Private continuity snapshots over the existing NIP-AE address and HTTP bridge.
//! Preflight bases are observations, never atomic compare-and-swap.
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use beekeeper_core::{continuity as c, engram, kind::KIND_AGENT_ENGRAM};
use clap::Subcommand;
use nostr::{Event, PublicKey};
use serde::Serialize;
use serde_json::json;

use crate::{client::BeekeeperClient, error::CliError};

/// Structured private memory operations. Ordinary `mem` commands remain unchanged.
#[derive(Subcommand)]
pub enum ContinuityCmd {
    /// Read current claims and freshly check their exact evidence under current access.
    Get {
        #[arg(long)]
        channel: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long = "claim")]
        claims: Vec<String>,
        /// Must match the cryptographically verified NIP-OA owner.
        #[arg(long, conflicts_with = "agent")]
        owner: Option<String>,
        /// Owner-side read only; this CLI key is the owner.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Publish once and confirm the exact ID/value at read time; never CAS.
    Put {
        #[arg(long)]
        channel: String,
        #[arg(long)]
        session: Option<String>,
        /// Complete decoded companion JSON; '-' reads bounded stdin.
        #[arg(long)]
        file: String,
        /// Exact observed head ID, or 'absent' for an observed empty address.
        #[arg(long)]
        base_event: String,
        /// Must match the cryptographically verified NIP-OA owner.
        #[arg(long)]
        owner: Option<String>,
    },
}

fn input(error: c::Error) -> CliError {
    CliError::Usage(error.to_string())
}

fn now() -> Result<u64, CliError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| CliError::Other("clock precedes Unix epoch".into()))
}

struct Identity {
    actor: PublicKey,
    owner: PublicKey,
    peer: PublicKey,
}

fn identity(
    client: &BeekeeperClient,
    owner: Option<&str>,
    agent: Option<&str>,
) -> Result<Identity, CliError> {
    let public_key = |s: &str| {
        PublicKey::from_hex(s).map_err(|_| CliError::Usage("expected a 64-hex public key".into()))
    };
    if let Some(agent) = agent {
        if owner.is_some() {
            return Err(CliError::Usage("--owner conflicts with --agent".into()));
        }
        let actor = public_key(agent)?;
        if actor == client.keys().public_key() {
            return Err(CliError::Usage(
                "--agent must differ from the CLI identity".into(),
            ));
        }
        return Ok(Identity {
            actor,
            owner: client.keys().public_key(),
            peer: actor,
        });
    }
    let supplied = owner.map(public_key).transpose()?;
    let verified = client.verified_auth_tag_owner_hex()?.ok_or_else(|| {
        CliError::Auth(
            "continuity requires verified NIP-OA custody; --owner alone is not authority".into(),
        )
    })?;
    let verified = public_key(&verified)?;
    if supplied.is_some_and(|key| key != verified) {
        return Err(CliError::Auth(
            "--owner differs from verified NIP-OA custody".into(),
        ));
    }
    Ok(Identity {
        actor: client.keys().public_key(),
        owner: verified,
        peer: verified,
    })
}

struct Observation {
    result: c::HeadRead,
    read_at: u64,
    count: Option<usize>,
    coverage: &'static str,
}

async fn read_head(
    client: &BeekeeperClient,
    identity: &Identity,
    scope: &c::Scope,
) -> Result<Observation, CliError> {
    let key = engram::conversation_key(client.keys().secret_key(), &identity.peer);
    let filter = json!({"kinds":[KIND_AGENT_ENGRAM], "authors":[identity.actor.to_hex()],
        "#p":[identity.owner.to_hex()], "#d":[engram::d_tag(&key, &scope.slug().map_err(input)?)],
        "limit": c::HEAD_QUERY_LIMIT});
    let response = client.query(&filter).await;
    let read_at = now()?;
    let raw = match response {
        Ok(raw) => raw,
        Err(CliError::Auth(_))
        | Err(CliError::Relay {
            status: 401 | 403, ..
        }) => return Err(CliError::Auth("continuity head access refused".into())),
        Err(_) => {
            return Ok(Observation {
                result: c::HeadRead::Unknown {
                    reason: c::UnknownReason::QueryFailed,
                },
                read_at,
                count: None,
                coverage: "failed",
            })
        }
    };
    let parsed = serde_json::from_str::<Vec<serde_json::Value>>(&raw);
    let (events, count) = match parsed {
        Ok(values) => {
            let count = values.len();
            (
                values
                    .into_iter()
                    .map(serde_json::from_value::<Event>)
                    .collect::<Result<Vec<_>, _>>(),
                count,
            )
        }
        Err(_) => {
            return Ok(Observation {
                result: c::HeadRead::Unknown {
                    reason: c::UnknownReason::InvalidCandidate,
                },
                read_at,
                count: None,
                coverage: "invalid",
            })
        }
    };
    let coverage = if count >= c::HEAD_QUERY_LIMIT {
        "incomplete"
    } else {
        "complete"
    };
    let result = match events {
        Ok(events) => c::select_head(
            scope,
            &c::ReadIdentity {
                actor: &identity.actor,
                owner: &identity.owner,
                reader_secret: client.keys().secret_key(),
                peer: &identity.peer,
            },
            &events,
            c::QueryCoverage::Complete,
        ),
        Err(_) => c::HeadRead::Unknown {
            reason: c::UnknownReason::InvalidCandidate,
        },
    };
    Ok(Observation {
        result,
        read_at,
        count: Some(count),
        coverage,
    })
}

async fn evidence(
    client: &BeekeeperClient,
    reference: &c::Evidence,
) -> Result<Event, c::UnknownReason> {
    let raw = client
        .query(
            &json!({"ids":[reference.event_id], "kinds":[reference.kind],
        "#h":[reference.channel], "limit":2}),
        )
        .await
        .map_err(|_| c::UnknownReason::EvidenceUnavailable)?;
    let mut events: Vec<Event> =
        serde_json::from_str(&raw).map_err(|_| c::UnknownReason::EvidenceInvalid)?;
    if events.len() != 1 {
        return Err(c::UnknownReason::EvidenceUnavailable);
    }
    let event = events.pop().ok_or(c::UnknownReason::EvidenceUnavailable)?;
    c::verify_evidence(reference, &event)?;
    Ok(event)
}

async fn get(
    client: &BeekeeperClient,
    scope: &c::Scope,
    identity: &Identity,
    requested: &[String],
) -> Result<(), CliError> {
    if requested.len() > c::MAX_CLAIMS {
        return Err(input(c::Error::TooLarge));
    }
    for id in requested {
        c::validate_claim_id(id).map_err(input)?;
    }
    let observation = read_head(client, identity, scope).await?;
    let mut output = json!({"scope":scope, "relay":scope.community, "read_at":observation.read_at,
        "coverage":observation.coverage, "result_count":observation.count,
        "head_event_id":null, "value_hash":null, "claims":[]});
    match observation.result {
        c::HeadRead::Present { head } => {
            // Only this read's results are retained. Repeated get/turn calls must query again.
            let mut fetched = Vec::new();
            for claim in &head.record.claims {
                if claim.state != c::ClaimState::Asserted
                    || head.conflicts.contains(&claim.id)
                    || (!requested.is_empty() && !requested.contains(&claim.id))
                {
                    continue;
                }
                for reference in &claim.evidence {
                    fetched.push((reference.clone(), evidence(client, reference).await));
                }
            }
            let claims = c::evaluate_claims(&head, requested, |reference| {
                fetched
                    .iter()
                    .find(|(r, _)| r == reference)
                    .map(|(_, result)| result.clone())
                    .unwrap_or(Err(c::UnknownReason::EvidenceUnavailable))
            })
            .map_err(input)?;
            output["status"] = json!("present");
            output["head_event_id"] = json!(head.event_id);
            output["value_hash"] = json!(head.value_hash);
            output["claims"] = json!(claims);
        }
        c::HeadRead::AbsentAtRead => output["status"] = json!("absent_at_read"),
        c::HeadRead::WithdrawnRecord { event_id } => {
            output["status"] = json!("withdrawn_record");
            output["head_event_id"] = json!(event_id);
        }
        c::HeadRead::Unknown { reason } => {
            output["status"] = json!("unknown");
            output["reason"] = json!(reason);
            println!("{output}");
            return Err(CliError::Unverifiable(
                "continuity head unknown; no older fallback".into(),
            ));
        }
    }
    output["evidence_read_completed_at"] = json!(now()?);
    println!("{output}");
    Ok(())
}

fn read_input(file: &str) -> Result<String, CliError> {
    let reader: Box<dyn Read> = if file == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(
            std::fs::File::open(file)
                .map_err(|_| CliError::Usage("cannot open continuity input file".into()))?,
        )
    };
    let mut value = String::new();
    reader
        .take((c::MAX_BODY_BYTES + 1) as u64)
        .read_to_string(&mut value)
        .map_err(|_| CliError::Usage("continuity input must be readable UTF-8".into()))?;
    if value.len() > c::MAX_BODY_BYTES {
        return Err(input(c::Error::TooLarge));
    }
    Ok(value)
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum WriteOutcome<'a> {
    ConfirmedAtRead {
        event_id: &'a str,
        value_hash: String,
        read_at: u64,
    },
    SubmittedUnknown {
        event_id: &'a str,
    },
    Conflict {
        event_id: Option<&'a str>,
        observed_head: Option<&'a str>,
    },
}

fn emit(outcome: WriteOutcome<'_>) -> Result<(), CliError> {
    println!(
        "{}",
        serde_json::to_string(&outcome)
            .map_err(|_| CliError::Other("cannot encode continuity outcome".into()))?
    );
    Ok(())
}
fn uncertain(id: &str) -> Result<(), CliError> {
    emit(WriteOutcome::SubmittedUnknown { event_id: id })?;
    Err(CliError::DeliveryUnknown(format!(
        "submitted_unknown event_id={id}; resolve by reading, do not blindly republish"
    )))
}
fn conflict(attempted: Option<&str>, observed: Option<&str>) -> Result<(), CliError> {
    emit(WriteOutcome::Conflict {
        event_id: attempted,
        observed_head: observed,
    })?;
    Err(CliError::Conflict(
        "continuity base/head changed; reread and explicitly correct".into(),
    ))
}

async fn put(
    client: &BeekeeperClient,
    scope: &c::Scope,
    identity: &Identity,
    file: &str,
    base: &str,
) -> Result<(), CliError> {
    let expected = if base == "absent" {
        None
    } else {
        if base.len() != 64
            || !base
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(CliError::Usage(
                "--base-event requires lowercase 64-hex or absent".into(),
            ));
        }
        Some(base)
    };
    let next = c::Record::decode(&read_input(file)?, scope).map_err(input)?;
    if next.base_event_id.as_deref() != expected {
        return Err(input(c::Error::BaseMismatch));
    }
    let before = read_head(client, identity, scope).await?;
    let previous = match &before.result {
        c::HeadRead::AbsentAtRead if expected.is_none() => None,
        c::HeadRead::Present { head } if Some(head.event_id.as_str()) == expected => {
            Some(head.as_ref())
        }
        c::HeadRead::Present { head } => return conflict(None, Some(&head.event_id)),
        c::HeadRead::WithdrawnRecord { event_id } => return conflict(None, Some(event_id)),
        c::HeadRead::AbsentAtRead => return conflict(None, None),
        c::HeadRead::Unknown { .. } => {
            return Err(CliError::Unverifiable(
                "continuity preflight unknown; nothing submitted".into(),
            ))
        }
    };
    c::validate_transition(
        &next,
        previous.map(|head| (head.event_id.as_str(), &head.record)),
    )
    .map_err(input)?;
    // A visible fork requires an explicit correction for every differing claim.
    if let Some(head) = previous {
        for id in &head.conflicts {
            let resolves = next
                .claims
                .iter()
                .find(|claim| &claim.id == id)
                .and_then(|claim| claim.correction.as_ref())
                .is_some_and(|correction| correction.prior_head == head.event_id);
            if !resolves {
                return conflict(None, Some(&head.event_id));
            }
        }
    }
    let created_at =
        c::write_created_at(now()?, previous.map(|head| head.created_at)).map_err(input)?;
    let body = next.to_body().map_err(input)?;
    let engram::Body::Memory {
        value: Some(value), ..
    } = &body
    else {
        return Err(input(c::Error::Schema));
    };
    let event = engram::build_event(client.keys(), &identity.owner, &body, created_at)
        .map_err(|_| CliError::Other("cannot build continuity event".into()))?;
    let id = event.id.to_hex();
    // submit_event retries stored-event sends, including lost acknowledgements.
    // Reuse the existing one-shot authenticated POST instead: one signed event,
    // one HTTP submission, same /events bridge and auth-tag handling.
    let wire = serde_json::to_value(&event)
        .map_err(|_| CliError::Other("cannot encode continuity event".into()))?;
    let ack = client.post_bootstrap("/events", &wire).await;
    let raw = match ack {
        Ok(raw) => raw,
        Err(CliError::Auth(_))
        | Err(CliError::Relay {
            status: 401 | 403, ..
        }) => {
            return Err(CliError::Auth(format!(
                "continuity submission refused event_id={id}"
            )))
        }
        Err(_) => return uncertain(&id),
    };
    let ack: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return uncertain(&id),
    };
    if ack.get("accepted").and_then(|v| v.as_bool()) != Some(true) {
        return uncertain(&id);
    }
    let after = match read_head(client, identity, scope).await {
        Ok(v) => v,
        Err(_) => return uncertain(&id),
    };
    match after.result {
        c::HeadRead::Present { head }
            if head.event_id == id && head.value == *value && head.conflicts.is_empty() =>
        {
            emit(WriteOutcome::ConfirmedAtRead {
                event_id: &id,
                value_hash: head.value_hash,
                read_at: after.read_at,
            })
        }
        c::HeadRead::Present { head } if head.event_id != id || !head.conflicts.is_empty() => {
            conflict(Some(&id), Some(&head.event_id))
        }
        c::HeadRead::WithdrawnRecord { event_id } => conflict(Some(&id), Some(&event_id)),
        _ => uncertain(&id),
    }
}

/// Dispatch parsed continuity arguments using only the configured community.
pub async fn dispatch(cmd: ContinuityCmd, client: &BeekeeperClient) -> Result<(), CliError> {
    match cmd {
        ContinuityCmd::Get {
            channel,
            session,
            claims,
            owner,
            agent,
        } => {
            let scope =
                c::Scope::new(client.relay_url(), &channel, session.as_deref()).map_err(input)?;
            let identity = identity(client, owner.as_deref(), agent.as_deref())?;
            get(client, &scope, &identity, &claims).await
        }
        ContinuityCmd::Put {
            channel,
            session,
            file,
            base_event,
            owner,
        } => {
            let scope =
                c::Scope::new(client.relay_url(), &channel, session.as_deref()).map_err(input)?;
            let identity = identity(client, owner.as_deref(), None)?;
            put(client, &scope, &identity, &file, &base_event).await
        }
    }
}
