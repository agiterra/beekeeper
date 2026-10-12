//! Renew only a verified retryable stop delivery, keeping the logical operation.
use super::lifecycle::Responsibility;
use crate::{Error, RelayTransport};
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use nostr::Event;
use serde_json::json;
use uuid::Uuid;

pub(crate) async fn command_id(
    operation: Uuid,
    row: &Responsibility,
    events: &[Event],
    client: &impl RelayTransport,
    renew: bool,
) -> Result<String, Error> {
    let attempts: Vec<_> = events
        .iter()
        .filter_map(|event| {
            let payload = decode_coding_session_lifecycle_command(&event.content).ok()?;
            match payload.action {
                CodingSessionLifecycleAction::SessionStop {
                    session,
                    provider_authority_pubkey,
                } if session == row.target
                    && provider_authority_pubkey == row.provider_authority
                    && event
                        .tags
                        .iter()
                        .any(|tag| tag.as_slice() == ["h", &row.channel_id.to_string()]) =>
                {
                    Some(payload.command_id)
                }
                _ => None,
            }
        })
        .collect();
    let target = beekeeper_core::coding_session_command::coding_session_target_key(&row.target);
    let Some(latest) = attempts.last() else {
        return Ok(Uuid::new_v5(&operation, target.as_bytes()).to_string());
    };
    if renew {
        let receipts = client
            .query_all(
                json!({"kinds":[44224],"authors":[row.provider_authority],"#h":[row.channel_id]}),
            )
            .await?;
        if crate::receipts::verified_command_receipts(
            &receipts,
            &row.channel_id.to_string(),
            latest,
            &row.provider_authority,
        )
        .iter()
        .any(|(_, receipt)| {
            receipt.status == beekeeper_core::coding_session_payload::ReceiptStatus::Failed
                && receipt
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "SESSION_QUIESCENCE_UNCONFIRMED")
        }) {
            return Ok(Uuid::new_v5(
                &operation,
                format!("{target}/quiescence-retry/{}", attempts.len()).as_bytes(),
            )
            .to_string());
        }
    }
    Ok(latest.clone())
}
