//! `bee ci wait` — suspend on one exact, durable CI result.

use std::collections::BTreeMap;
use std::time::Duration;

use beekeeper_core::ci_result::{
    correlation_id, decode_ci_result, validate_identity, CiConclusion, CiResult, CiResultIdentity,
};
use beekeeper_core::kind::KIND_CI_RESULT;
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use nostr::PublicKey;
use serde::Serialize;
use serde_json::json;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::{CiCmd, CiContinuationCmd, CiPhaseArg};

mod continuation;

const SUBSCRIPTION_ID: &str = "bee-ci-wait";
const IDLE_RECONNECT_SECS: u64 = 900;
const MAX_BACKOFF_SECS: u64 = 30;

#[derive(Clone, Copy)]
struct WaitTiming {
    idle: Duration,
    first_backoff: Duration,
    max_backoff: Duration,
}

impl Default for WaitTiming {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(IDLE_RECONNECT_SECS),
            first_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(MAX_BACKOFF_SECS),
        }
    }
}

#[derive(Serialize)]
struct CiWaitOutput<'a> {
    event_id: &'a str,
    result: &'a CiResult,
}

fn encode_output(accepted: &AcceptedResult) -> Result<String, CliError> {
    serde_json::to_string(&CiWaitOutput {
        event_id: &accepted.event_id,
        result: &accepted.result,
    })
    .map_err(|error| CliError::Other(format!("cannot encode CI wait output: {error}")))
}

fn terminal_disposition(result: &CiResult) -> Result<(), CliError> {
    match &result.conclusion {
        CiConclusion::Success => Ok(()),
        CiConclusion::Failure => Err(CliError::Refused(format!(
            "CI {} check '{}' failed for commit {}",
            match &result.identity.phase {
                beekeeper_core::ci_result::CiPhase::Build => "build",
                beekeeper_core::ci_result::CiPhase::Deploy => "deploy",
            },
            result.identity.check,
            result.identity.commit
        ))),
        CiConclusion::Cancelled => Err(CliError::Refused(format!(
            "CI check '{}' was cancelled for commit {}",
            result.identity.check, result.identity.commit
        ))),
    }
}

#[derive(Debug)]
struct AcceptedResult {
    event_id: String,
    result: CiResult,
    canonical: Vec<u8>,
}

enum ConnectionOutcome {
    Result(Box<AcceptedResult>),
    Retry(String),
}

fn relay_self_from_nip11(raw: &str) -> Result<String, CliError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        CliError::Other(format!("relay info document is not valid JSON: {error}"))
    })?;
    let relay_self = value
        .get("self")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CliError::Other("relay info document missing 'self' field".into()))?;
    PublicKey::from_hex(relay_self)
        .map(|key| key.to_hex())
        .map_err(|error| CliError::Other(format!("relay info 'self' is invalid: {error}")))
}

fn wait_deadline(timeout: Option<Duration>) -> Result<Option<tokio::time::Instant>, CliError> {
    timeout
        .map(|duration| {
            tokio::time::Instant::now()
                .checked_add(duration)
                .ok_or_else(|| CliError::Usage("--timeout is too large".into()))
        })
        .transpose()
}

fn timeout_error(digest: &str) -> CliError {
    CliError::Unconfirmed(format!("timed out waiting for CI result {digest}"))
}

async fn trusted_relay_self(
    client: &BuzzClient,
    deadline: Option<tokio::time::Instant>,
    digest: &str,
) -> Result<String, CliError> {
    let request = client.get_public("/");
    let raw = if let Some(deadline) = deadline {
        tokio::time::timeout_at(deadline, request)
            .await
            .map_err(|_| timeout_error(digest))??
    } else {
        request.await?
    };
    relay_self_from_nip11(&raw)
}

fn verify_result_event(
    event: &nostr::Event,
    relay_self: &str,
    expected: &CiResultIdentity,
) -> Result<AcceptedResult, CliError> {
    event.verify().map_err(|error| {
        CliError::Other(format!(
            "CI result failed cryptographic verification: {error}"
        ))
    })?;
    if event.pubkey.to_hex() != relay_self {
        return Err(CliError::Other(format!(
            "CI result signer {} does not match relay self {relay_self}",
            event.pubkey.to_hex()
        )));
    }
    let result = decode_ci_result(event)
        .map_err(|error| CliError::Other(format!("invalid CI result: {error}")))?;
    let actual = serde_json::to_vec(&result.identity)
        .map_err(|error| CliError::Other(format!("cannot encode CI identity: {error}")))?;
    let wanted = serde_json::to_vec(expected)
        .map_err(|error| CliError::Other(format!("cannot encode expected CI identity: {error}")))?;
    if actual != wanted {
        return Err(CliError::Other(
            "relay returned a CI result for a different exact identity".into(),
        ));
    }
    let canonical = serde_json::to_vec(&result)
        .map_err(|error| CliError::Other(format!("cannot encode CI result: {error}")))?;
    Ok(AcceptedResult {
        event_id: event.id.to_hex(),
        result,
        canonical,
    })
}

fn fold_replay(
    results: BTreeMap<Vec<u8>, AcceptedResult>,
) -> Result<Option<AcceptedResult>, CliError> {
    if results.len() > 1 {
        return Err(CliError::Conflict(
            "relay stores conflicting CI results for the exact requested identity".into(),
        ));
    }
    Ok(results.into_values().next())
}

fn permanent_ws_error(error: WsClientError, stage: &str) -> Result<ConnectionOutcome, CliError> {
    match error {
        WsClientError::AuthFailed(message) => Err(CliError::Auth(message)),
        WsClientError::NoAuthChallenge => Err(CliError::Auth(
            "relay did not provide the required NIP-42 challenge".into(),
        )),
        WsClientError::ConnectionClosed | WsClientError::Timeout | WsClientError::WebSocket(_) => {
            Ok(ConnectionOutcome::Retry(format!("{stage}: {error}")))
        }
        other => Err(CliError::Other(format!("{stage}: {other}"))),
    }
}

async fn run_connection(
    client: &BuzzClient,
    relay_self: &str,
    identity: &CiResultIdentity,
    digest: &str,
    timing: WaitTiming,
) -> Result<ConnectionOutcome, CliError> {
    let mut conn = match NostrWsConnection::connect_authenticated(
        &client.ws_url(),
        client.keys(),
        client.auth_tag(),
    )
    .await
    {
        Ok(conn) => conn,
        Err(error) => return permanent_ws_error(error, "CI wait connection"),
    };
    let filter = json!({"kinds": [KIND_CI_RESULT], "#d": [digest]});
    if let Err(error) = conn
        .send_raw(&json!(["REQ", SUBSCRIPTION_ID, filter]))
        .await
    {
        return permanent_ws_error(error, "CI wait subscription");
    }

    let mut replay = BTreeMap::new();
    let mut replay_complete = false;
    loop {
        match conn.next_event(timing.idle).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == SUBSCRIPTION_ID => {
                let accepted = verify_result_event(&event, relay_self, identity)?;
                if replay_complete {
                    return Ok(ConnectionOutcome::Result(Box::new(accepted)));
                }
                replay.entry(accepted.canonical.clone()).or_insert(accepted);
            }
            Ok(RelayMessage::Eose { subscription_id }) if subscription_id == SUBSCRIPTION_ID => {
                replay_complete = true;
                if let Some(result) = fold_replay(std::mem::take(&mut replay))? {
                    return Ok(ConnectionOutcome::Result(Box::new(result)));
                }
            }
            Ok(RelayMessage::Closed {
                subscription_id,
                message,
            }) if subscription_id == SUBSCRIPTION_ID => {
                let lower = message.to_ascii_lowercase();
                if lower.contains("auth")
                    || lower.contains("forbidden")
                    || lower.contains("restricted")
                {
                    return Err(CliError::Auth(format!(
                        "CI wait subscription closed: {message}"
                    )));
                }
                return Err(CliError::Relay {
                    status: 400,
                    body: format!("CI wait subscription closed: {message}"),
                });
            }
            Ok(RelayMessage::Notice { message }) => {
                eprintln!("CI wait relay notice: {message}");
            }
            Ok(_) => {}
            Err(error) => return permanent_ws_error(error, "CI wait receive"),
        }
    }
}

async fn wait_for_result(
    client: &BuzzClient,
    relay_self: &str,
    identity: &CiResultIdentity,
    deadline: Option<tokio::time::Instant>,
    timing: WaitTiming,
) -> Result<AcceptedResult, CliError> {
    validate_identity(identity).map_err(CliError::Usage)?;
    let digest = correlation_id(identity).map_err(CliError::Usage)?;
    let mut backoff = timing.first_backoff;

    loop {
        let connection = run_connection(client, relay_self, identity, &digest, timing);
        let outcome = if let Some(deadline) = deadline {
            tokio::time::timeout_at(deadline, connection)
                .await
                .map_err(|_| timeout_error(&digest))??
        } else {
            connection.await?
        };
        match outcome {
            ConnectionOutcome::Result(result) => return Ok(*result),
            ConnectionOutcome::Retry(reason) => {
                eprintln!("{reason}; reconnecting to wait for CI result");
                if let Some(deadline) = deadline {
                    tokio::time::timeout_at(deadline, tokio::time::sleep(backoff))
                        .await
                        .map_err(|_| timeout_error(&digest))?;
                } else {
                    tokio::time::sleep(backoff).await;
                }
                backoff = (backoff * 2).min(timing.max_backoff);
            }
        }
    }
}

pub async fn dispatch(
    cmd: CiCmd,
    client: &BuzzClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    match cmd {
        CiCmd::Wait {
            project,
            repo,
            commit,
            check,
            run,
            attempt,
            workflow,
            phase,
            timeout,
        } => {
            cmd_wait(
                client, project, repo, commit, check, run, attempt, workflow, phase, timeout,
            )
            .await
        }
        CiCmd::Continue {
            channel,
            provider,
            target,
            driver,
            instance_id,
            session_id,
            generation,
            project,
            repository,
            commit,
            check,
            run,
            attempt,
            workflow,
            phase,
            continuation,
            expires_in,
            expires_at,
            ack_timeout,
        } => {
            continuation::cmd_continue(
                client,
                format,
                &channel,
                target.as_deref(),
                driver.as_deref(),
                instance_id.as_deref(),
                session_id.as_deref(),
                generation,
                project,
                repository,
                commit,
                check,
                run,
                attempt,
                workflow,
                phase,
                &continuation,
                expires_in,
                expires_at,
                ack_timeout,
                &provider,
            )
            .await
        }
        CiCmd::Continuation(CiContinuationCmd::Status {
            channel,
            command_id,
            target,
            provider,
        }) => {
            continuation::cmd_continuation_status(
                client,
                format,
                &channel,
                &command_id,
                &target,
                &provider,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_wait(
    client: &BuzzClient,
    project: String,
    repository: String,
    commit: String,
    check: String,
    run: String,
    attempt: u32,
    workflow: String,
    phase: CiPhaseArg,
    timeout_secs: Option<u64>,
) -> Result<(), CliError> {
    let identity = CiResultIdentity {
        project,
        repository,
        commit,
        check,
        run,
        attempt,
        workflow,
        phase: phase.into(),
    };
    validate_identity(&identity).map_err(CliError::Usage)?;
    let digest = correlation_id(&identity).map_err(CliError::Usage)?;
    let deadline = wait_deadline(timeout_secs.map(Duration::from_secs))?;
    let relay_self = trusted_relay_self(client, deadline, &digest).await?;
    let accepted = wait_for_result(
        client,
        &relay_self,
        &identity,
        deadline,
        WaitTiming::default(),
    )
    .await?;
    println!("{}", encode_output(&accepted)?);
    terminal_disposition(&accepted.result)
}

#[cfg(test)]
mod tests;
