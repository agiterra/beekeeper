//! Existing correlated Failed receipts retain uncertain startup responsibilities.
use super::lifecycle::{provider_authorized, Responsibility};
use crate::{Error, RelayTransport};
use beekeeper_core::{
    coding_session_lifecycle_command::{
        decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    },
    coding_session_payload::{ReceiptStatus, SessionStatus},
};
use nostr::Event;
use serde_json::json;
use uuid::Uuid;

pub(crate) async fn inventory(
    client: &impl RelayTransport,
    actor: &str,
    channel: Uuid,
) -> Result<Vec<Responsibility>, Error> {
    let profiles = client
        .query_all(json!({"kinds":[0],"authors":[actor],"limit":1}))
        .await?;
    let owner = profiles
        .into_iter()
        .filter_map(|raw| serde_json::from_value::<Event>(raw).ok())
        .find_map(|event| {
            if event.kind.as_u16() != 0 || event.pubkey.to_hex() != actor || event.verify().is_err()
            {
                return None;
            }
            let auth = event
                .tags
                .iter()
                .find(|tag| tag.as_slice().first().is_some_and(|v| v == "auth"))?;
            let encoded = serde_json::to_string(auth.as_slice()).ok()?;
            beekeeper_sdk::nip_oa::verify_auth_tag(&encoded, &event.pubkey)
                .ok()
                .map(|owner| owner.to_hex())
        })
        .ok_or_else(|| Error::Unconfirmed("actor owner profile unavailable".into()))?;
    let commands = client
        .query_all(json!({"kinds":[44221],"authors":[owner],"#h":[channel]}))
        .await?;
    let receipts = client
        .query_all(json!({"kinds":[44224],"#h":[channel]}))
        .await?;
    let mut held = Vec::new();
    for raw in commands {
        let Ok(event) = serde_json::from_value::<Event>(raw) else {
            continue;
        };
        if event.kind.as_u16() != 44221
            || event.pubkey.to_hex() != owner
            || event.verify().is_err()
            || !event
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["h", &channel.to_string()])
        {
            continue;
        }
        let Ok(payload) = decode_coding_session_lifecycle_command(&event.content) else {
            continue;
        };
        let CodingSessionLifecycleAction::SessionCreate {
            actor: Some(selected),
            provider_authority_pubkey,
            ..
        } = payload.action
        else {
            continue;
        };
        if selected != actor {
            continue;
        }
        let channel_text = channel.to_string();
        let expected = [["h",channel_text.as_str()], ["csl-v",beekeeper_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION], ["csl-command",payload.command_id.as_str()]];
        if event.tags.len() != expected.len()
            || event
                .tags
                .iter()
                .zip(expected)
                .any(|(got, want)| got.as_slice() != want)
        {
            continue;
        }
        let outcomes = crate::receipts::verified_command_receipts(
            &receipts,
            &channel.to_string(),
            &payload.command_id,
            &provider_authority_pubkey,
        );
        if outcomes.is_empty() {
            return Err(Error::Unconfirmed(
                "accepted actor create has no verified outcome; zero work cannot be certified"
                    .into(),
            ));
        }
        for (id, receipt) in outcomes {
            let uncertain = receipt.status==ReceiptStatus::Failed && receipt.error.as_ref().is_some_and(|error|error.code==beekeeper_core::coding_session_payload::CREATE_START_OUTCOME_UNCONFIRMED);
            let accepted = matches!(
                receipt.status,
                ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
            );
            if !uncertain && !accepted {
                continue;
            }
            let Some(target) = receipt.session else {
                if uncertain {
                    return Err(Error::Unconfirmed(
                        "startup uncertainty has no reserved target; zero work cannot be certified"
                            .into(),
                    ));
                }
                continue;
            };
            provider_authorized(client, actor, &provider_authority_pubkey).await?;
            held.push(Responsibility {
                channel_id: channel,
                target,
                provider_authority: provider_authority_pubkey.clone(),
                metadata_event_id: id,
                status: SessionStatus::Disconnected,
            });
        }
    }
    Ok(held)
}
