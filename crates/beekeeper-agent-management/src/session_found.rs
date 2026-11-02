//! Direct-owner genesis publication, with the same protected operation journal
//! as managed identity operations. No host custody or model execution is needed.
use crate::{management::local, Context, Error, RelayTransport};
use beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload;
use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// Immutable inputs to a fresh umbrella foundation; retry all inputs unchanged.
#[derive(Clone, Debug)]
pub struct FoundRequest {
    /// Caller-minted nonnil operation UUID, retained across uncertain outcomes.
    pub operation_id: Uuid,
    /// Conversation channel. Admission is enforced by the existing relay.
    pub channel: Uuid,
    /// Explicit canonical lowercase umbrella UUID; not an execution session ID.
    pub session_ref: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    request: Value,
    event: Event,
    observed: bool,
}

/// Found an umbrella with the direct signer, persisting before publication.
/// Success requires an exact signed event read back from the scoped relay;
/// an ACK alone never establishes success. Retry uses the original signed event.
pub async fn found(
    args: &FoundRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    let payload = CodingSessionGenesisPayload::new(args.session_ref.clone());
    payload.validate().map_err(Error::Usage)?;
    if args.operation_id.is_nil() || args.channel.is_nil() {
        return Err(Error::Usage(
            "operation and channel UUIDs must be nonnil".into(),
        ));
    }
    let owner = local::owner(client)?;
    let request = json!({"type":"session-found","operationId":args.operation_id,"owner":owner,"relay":client.relay_url(),
        "channel":args.channel,"sessionRef":args.session_ref});
    let _lock = local::management_lock(context)?;
    let path = local::operation_path(context, args.operation_id)?;
    let existing = local::read_operation_value(&path)?;
    let retry = existing.is_some();
    let mut operation = if let Some(value) = existing {
        if value.get("request") != Some(&request) {
            return Err(Error::Conflict(
                "operation UUID already has different intent, signer or relay; original retained"
                    .into(),
            ));
        }
        serde_json::from_value::<Operation>(value).map_err(local::fail)?
    } else {
        let builder = beekeeper_sdk::build_coding_session_genesis(args.channel, &payload)
            .map_err(local::fail)?;
        let event = client.sign_event_unchecked(builder)?;
        let operation = Operation {
            request,
            event,
            observed: false,
        };
        local::save(&path, &operation)?;
        operation
    };
    // Check the journal independently before using it as publication authority.
    let expected_tags = json!([
        ["h", args.channel.to_string()],
        ["csg-v", "csg1-1"],
        ["csg-session", args.session_ref]
    ]);
    if operation.event.verify().is_err()
        || operation.event.pubkey != client.keys().public_key()
        || operation.event.kind.as_u16() != beekeeper_core::kind::KIND_CODING_SESSION_GENESIS as u16
        || operation.event.content != serde_json::to_string(&payload).map_err(local::fail)?
        || serde_json::to_value(&operation.event.tags).map_err(local::fail)? != expected_tags
    {
        return Err(Error::Auth(
            "stored genesis signature, signer or canonical scope does not match request".into(),
        ));
    }
    let unconfirmed = |reason: String| {
        Error::Unconfirmed(format!(
        "session-found operation {} genesis {} unconfirmed: {reason}; retry the same operation and arguments; journal {}",
        args.operation_id, operation.event.id, path.display()))
    };
    let mut observed = false;
    if retry {
        observed = readback(client, &operation.event)
            .await
            .map_err(|e| unconfirmed(e.to_string()))?;
    }
    if !observed {
        let reply = client.submit_event(operation.event.clone()).await;
        // Even a rejection/duplicate can follow an accepted write whose ACK was
        // lost. Only this exact signed readback resolves that uncertainty.
        observed = readback(client, &operation.event)
            .await
            .map_err(|e| unconfirmed(e.to_string()))?;
        if !observed && crate::management::relay_refused_for_age(&reply, &operation.event) {
            return Err(crate::management::expired_error(&operation.event));
        }
        if !observed {
            if let Ok(value) = &reply {
                if value.get("accepted").and_then(Value::as_bool) == Some(false)
                    && !value
                        .get("message")
                        .and_then(Value::as_str)
                        .is_some_and(|m| m == "duplicate" || m.starts_with("duplicate:"))
                {
                    return Err(Error::Refused(format!(
                        "genesis {} refused: {}; original operation {} retained at {}",
                        operation.event.id,
                        value
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("relay refused"),
                        args.operation_id,
                        path.display()
                    )));
                }
            }
            return Err(unconfirmed(match reply {
                Ok(_) => "exact signed readback unavailable (ACK is insufficient)".into(),
                Err(error) => error.to_string(),
            }));
        }
    }
    operation.observed = true;
    local::save(&path, &operation)?;
    Ok(
        json!({"operation_id":args.operation_id,"session_ref":args.session_ref,
        "genesis":operation.event.id.to_hex(),"event_id":operation.event.id.to_hex(),
        "founder_pubkey":owner,"accepted":true,"evidence":"exact_signed_readback",
        "event":operation.event,"journal":path}),
    )
}

async fn readback(client: &impl RelayTransport, expected: &Event) -> Result<bool, Error> {
    let channel = expected
        .tags
        .iter()
        .find(|t| t.as_slice().first().is_some_and(|s| s == "h"))
        .and_then(|t| t.as_slice().get(1))
        .ok_or_else(|| Error::Auth("genesis channel missing".into()))?;
    let rows = client
        .query_all(
            json!({"kinds":[beekeeper_core::kind::KIND_CODING_SESSION_GENESIS],
        "ids":[expected.id.to_hex()],"authors":[expected.pubkey.to_hex()],"#h":[channel]}),
        )
        .await?;
    let observed = !rows.is_empty();
    for row in rows {
        let event: Event = serde_json::from_value(row).map_err(local::fail)?;
        if event.verify().is_err() || event != *expected {
            return Err(Error::Auth(
                "relay genesis readback is not the exact signed event".into(),
            ));
        }
    }
    Ok(observed)
}

#[cfg(test)]
#[path = "session_found_tests.rs"]
mod tests;
