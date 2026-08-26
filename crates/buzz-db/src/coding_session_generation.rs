//! Immutable lifecycle proof resolution for exact coding-session generations.

use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
};
use buzz_core::coding_session_payload::{decode_coding_session_lifecycle_receipt, ReceiptStatus};
use buzz_core::kind::{
    event_kind_u32, KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
};
use buzz_core::CommunityId;
use nostr::{Event, PublicKey};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

const RECEIPT_TAG_VERSION: &str = "cslr1-1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MintAction {
    Create,
    Resume,
}

/// Immutable event IDs proving that a provider may lease one exact generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationAuthorityProof {
    /// Accepted lifecycle command event ID.
    pub command_event_id: nostr::EventId,
    /// Successful lifecycle receipt event ID.
    pub receipt_event_id: nostr::EventId,
    /// Provider authority bound by both proof events.
    pub provider_authority: PublicKey,
}

/// Fail-closed generation-authority resolution outcome.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GenerationAuthorityError {
    /// No strictly valid create/resume command matched the complete tuple.
    #[error("generation authority command not found")]
    MissingCommand,
    /// More than one strictly valid command matched, so authority is ambiguous.
    #[error("generation authority command is ambiguous")]
    AmbiguousCommand,
    /// The lease signer does not match the unique command's provider authority.
    #[error("generation authority does not match lease signer")]
    AuthorityMismatch,
    /// No strictly valid successful receipt matched the complete tuple.
    #[error("generation authority receipt not found")]
    MissingReceipt,
    /// More than one strictly valid receipt matched, so authority is ambiguous.
    #[error("generation authority receipt is ambiguous")]
    AmbiguousReceipt,
    /// More than one distinct command/receipt proof minted the exact generation.
    #[error("exact generation has competing lifecycle authority proofs")]
    AmbiguousGeneration,
}

/// Resolve lease authority from the writer database, including soft-deleted rows.
///
/// Soft deletion is deliberately not a revocation mechanism for immutable
/// lifecycle proof. Candidates are selected by community and channel, then
/// every exact-target command/receipt proof is revalidated in memory so a
/// competing command ID cannot mint the same generation unnoticed.
pub async fn resolve_generation_authority(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    lease_signer: &PublicKey,
) -> crate::Result<GenerationAuthorityProof> {
    let rows = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
         FROM events WHERE community_id = $1 AND channel_id = $2 \
         AND kind IN ($3, $4) \
         ORDER BY created_at ASC, id ASC",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(KIND_CODING_SESSION_LIFECYCLE_COMMAND as i32)
    .bind(KIND_CODING_SESSION_LIFECYCLE_RECEIPT as i32)
    .fetch_all(pool)
    .await?;

    let mut commands = Vec::new();
    let mut receipts = Vec::new();
    for row in rows {
        if let Some(stored) = super::event::row_to_stored_event(row)? {
            match event_kind_u32(&stored.event) {
                KIND_CODING_SESSION_LIFECYCLE_COMMAND => commands.push(stored.event),
                KIND_CODING_SESSION_LIFECYCLE_RECEIPT => receipts.push(stored.event),
                _ => {}
            }
        }
    }
    select_unique_generation_authority(
        commands,
        receipts,
        channel_id,
        command_id,
        target,
        lease_signer,
    )
    .map_err(|error| crate::DbError::AccessDenied(error.to_string()))
}

fn select_unique_generation_authority(
    mut command_events: Vec<Event>,
    mut receipt_events: Vec<Event>,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    lease_signer: &PublicKey,
) -> Result<GenerationAuthorityProof, GenerationAuthorityError> {
    command_events.sort_by_key(|event| event.id);
    command_events.dedup_by_key(|event| event.id);
    receipt_events.sort_by_key(|event| event.id);
    receipt_events.dedup_by_key(|event| event.id);

    let commands: Vec<_> = command_events
        .iter()
        .filter_map(|event| valid_command(event, channel_id, command_id, target))
        .collect();
    let command = match commands.as_slice() {
        [] => return Err(GenerationAuthorityError::MissingCommand),
        [command] => command,
        _ => return Err(GenerationAuthorityError::AmbiguousCommand),
    };
    let receipts: Vec<_> = receipt_events
        .iter()
        .filter_map(|event| {
            valid_receipt(event, channel_id, command_id, target, &command.2, command.1)
        })
        .collect();
    let receipt = match receipts.as_slice() {
        [] => return Err(GenerationAuthorityError::MissingReceipt),
        [receipt] => receipt,
        _ => return Err(GenerationAuthorityError::AmbiguousReceipt),
    };

    let competing_command_ids: std::collections::BTreeSet<String> = command_events
        .iter()
        .filter_map(|event| {
            decode_coding_session_lifecycle_command(&event.content)
                .ok()
                .map(|payload| payload.command_id)
        })
        .filter(|candidate| candidate != command_id)
        .collect();
    for competing_id in competing_command_ids {
        let competing_commands: Vec<_> = command_events
            .iter()
            .filter_map(|event| valid_command(event, channel_id, &competing_id, target))
            .collect();
        let has_competing_proof = competing_commands.iter().any(|competing_command| {
            receipt_events.iter().any(|event| {
                valid_receipt(
                    event,
                    channel_id,
                    &competing_id,
                    target,
                    &competing_command.2,
                    competing_command.1,
                )
                .is_some()
            })
        });
        if has_competing_proof {
            return Err(GenerationAuthorityError::AmbiguousGeneration);
        }
    }

    if command.2 != *lease_signer {
        return Err(GenerationAuthorityError::AuthorityMismatch);
    }
    Ok(GenerationAuthorityProof {
        command_event_id: command.0.id,
        receipt_event_id: receipt.id,
        provider_authority: command.2,
    })
}

fn valid_command(
    event: &Event,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
) -> Option<(Event, MintAction, PublicKey)> {
    if event_kind_u32(event) != KIND_CODING_SESSION_LIFECYCLE_COMMAND
        || !exact_tags(
            event,
            &[
                ("h", channel_id.to_string()),
                (
                    "csl-v",
                    CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION.to_owned(),
                ),
                ("csl-command", command_id.to_owned()),
            ],
        )
    {
        return None;
    }
    let payload = decode_coding_session_lifecycle_command(&event.content).ok()?;
    if payload.command_id != command_id {
        return None;
    }
    let (authority, action) = match payload.action {
        CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        } if target.generation == 1 => (provider_authority_pubkey, MintAction::Create),
        CodingSessionLifecycleAction::SessionResume {
            session,
            provider_authority_pubkey,
        } if same_execution_next_generation(&session, target) => {
            (provider_authority_pubkey, MintAction::Resume)
        }
        _ => return None,
    };
    let authority = PublicKey::from_hex(&authority).ok()?;
    Some((event.clone(), action, authority))
}

fn valid_receipt(
    event: &Event,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    provider_authority: &PublicKey,
    action: MintAction,
) -> Option<Event> {
    let semantic_key = structured_key("coding-session-lifecycle-receipt/v1", &[command_id]);
    if event_kind_u32(event) != KIND_CODING_SESSION_LIFECYCLE_RECEIPT
        || event.pubkey != *provider_authority
        || !exact_tags(
            event,
            &[
                ("h", channel_id.to_string()),
                ("cslr-v", RECEIPT_TAG_VERSION.to_owned()),
                ("csl-command", command_id.to_owned()),
                ("csl-key", semantic_key),
            ],
        )
    {
        return None;
    }
    let receipt = decode_coding_session_lifecycle_receipt(&event.content).ok()?;
    if receipt.command_id != command_id || receipt.session.as_ref() != Some(target) {
        return None;
    }
    let status_matches = match action {
        MintAction::Create => matches!(
            receipt.status,
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
        ),
        MintAction::Resume => matches!(
            receipt.status,
            ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext
        ),
    };
    if !status_matches {
        return None;
    }
    Some(event.clone())
}

fn same_execution_next_generation(
    previous: &CodingSessionTarget,
    next: &CodingSessionTarget,
) -> bool {
    previous.driver == next.driver
        && previous.instance_id == next.instance_id
        && previous.session_id == next.session_id
        && previous.generation.checked_add(1) == Some(next.generation)
}

fn exact_tags(event: &Event, expected: &[(&str, String)]) -> bool {
    event.tags.len() == expected.len()
        && event.tags.iter().zip(expected).all(|(tag, (key, value))| {
            let parts = tag.as_slice();
            parts.len() == 2 && parts[0] == *key && parts[1] == *value
        })
}

fn structured_key(domain: &str, fields: &[&str]) -> String {
    let mut key = format!("{domain}|");
    for field in fields {
        key.push_str(&field.len().to_string());
        key.push(':');
        key.push_str(field);
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::coding_session_payload::{LifecycleReceipt, LIFECYCLE_RECEIPT_SCHEMA};
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

    fn tag(parts: &[&str]) -> Tag {
        Tag::parse(parts.iter().copied()).unwrap()
    }

    fn target(generation: u64) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation,
        }
    }

    fn command(keys: &Keys, provider: &PublicKey, channel: Uuid, id: &str) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": "instance-1",
                "providerAuthorityPubkey": provider.to_hex(),
                "model": null,
                "title": null,
                "initialTurn": null
            }
        });
        EventBuilder::new(Kind::Custom(44_221), content.to_string())
            .tags([
                tag(&["h", &channel.to_string()]),
                tag(&["csl-v", "csl1-1"]),
                tag(&["csl-command", id]),
            ])
            .sign_with_keys(keys)
            .unwrap()
    }

    fn resume_command(
        keys: &Keys,
        provider: &PublicKey,
        channel: Uuid,
        id: &str,
        previous: &CodingSessionTarget,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": id,
            "action": {
                "type": "session.resume",
                "session": previous,
                "providerAuthorityPubkey": provider.to_hex()
            }
        });
        EventBuilder::new(Kind::Custom(44_221), content.to_string())
            .tags([
                tag(&["h", &channel.to_string()]),
                tag(&["csl-v", "csl1-1"]),
                tag(&["csl-command", id]),
            ])
            .sign_with_keys(keys)
            .unwrap()
    }

    fn stop_command(
        keys: &Keys,
        provider: &PublicKey,
        channel: Uuid,
        id: &str,
        current: &CodingSessionTarget,
    ) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": id,
            "action": {
                "type": "session.stop",
                "session": current,
                "providerAuthorityPubkey": provider.to_hex()
            }
        });
        EventBuilder::new(Kind::Custom(44_221), content.to_string())
            .tags([
                tag(&["h", &channel.to_string()]),
                tag(&["csl-v", "csl1-1"]),
                tag(&["csl-command", id]),
            ])
            .sign_with_keys(keys)
            .unwrap()
    }

    fn receipt(keys: &Keys, channel: Uuid, id: &str, target: &CodingSessionTarget) -> Event {
        let payload = LifecycleReceipt {
            schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
            command_id: id.into(),
            status: ReceiptStatus::Created,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        };
        EventBuilder::new(
            Kind::Custom(44_224),
            serde_json::to_string(&payload).unwrap(),
        )
        .tags([
            tag(&["h", &channel.to_string()]),
            tag(&["cslr-v", "cslr1-1"]),
            tag(&["csl-command", id]),
            tag(&[
                "csl-key",
                &structured_key("coding-session-lifecycle-receipt/v1", &[id]),
            ]),
        ])
        .sign_with_keys(keys)
        .unwrap()
    }

    fn receipt_with_status(
        keys: &Keys,
        channel: Uuid,
        id: &str,
        target: &CodingSessionTarget,
        status: ReceiptStatus,
    ) -> Event {
        let payload = LifecycleReceipt {
            schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
            command_id: id.into(),
            status,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        };
        EventBuilder::new(
            Kind::Custom(44_224),
            serde_json::to_string(&payload).unwrap(),
        )
        .tags([
            tag(&["h", &channel.to_string()]),
            tag(&["cslr-v", "cslr1-1"]),
            tag(&["csl-command", id]),
            tag(&[
                "csl-key",
                &structured_key("coding-session-lifecycle-receipt/v1", &[id]),
            ]),
        ])
        .sign_with_keys(keys)
        .unwrap()
    }

    #[test]
    fn exact_create_chain_dedupes_replay_and_distinct_commands_fail_closed() {
        let operator = Keys::generate();
        let provider = Keys::generate();
        let channel = Uuid::new_v4();
        let command = command(&operator, &provider.public_key(), channel, "create-1");
        let receipt = receipt(&provider, channel, "create-1", &target(1));
        let proof = select_unique_generation_authority(
            vec![command.clone()],
            vec![receipt.clone()],
            channel,
            "create-1",
            &target(1),
            &provider.public_key(),
        )
        .unwrap();
        assert_eq!(proof.command_event_id, command.id);
        assert_eq!(proof.receipt_event_id, receipt.id);

        assert!(select_unique_generation_authority(
            vec![command.clone(), command.clone()],
            vec![receipt.clone()],
            channel,
            "create-1",
            &target(1),
            &provider.public_key(),
        )
        .is_ok());

        let distinct_command = EventBuilder::new(command.kind, command.content.clone())
            .tags(command.tags.clone())
            .custom_created_at(Timestamp::from_secs(command.created_at.as_secs() + 1))
            .sign_with_keys(&operator)
            .unwrap();
        assert!(matches!(
            select_unique_generation_authority(
                vec![command, distinct_command],
                vec![receipt],
                channel,
                "create-1",
                &target(1),
                &provider.public_key(),
            ),
            Err(GenerationAuthorityError::AmbiguousCommand)
        ));
    }

    #[test]
    fn commands_with_different_provider_authorities_are_ambiguous_for_both_signers() {
        let operator = Keys::generate();
        let first_provider = Keys::generate();
        let second_provider = Keys::generate();
        let channel = Uuid::new_v4();
        let first_command = command(&operator, &first_provider.public_key(), channel, "create-1");
        let second_command = command(
            &operator,
            &second_provider.public_key(),
            channel,
            "create-1",
        );
        let first_receipt = receipt(&first_provider, channel, "create-1", &target(1));
        let second_receipt = receipt(&second_provider, channel, "create-1", &target(1));

        for lease_signer in [first_provider.public_key(), second_provider.public_key()] {
            assert!(matches!(
                select_unique_generation_authority(
                    vec![first_command.clone(), second_command.clone()],
                    vec![first_receipt.clone(), second_receipt.clone()],
                    channel,
                    "create-1",
                    &target(1),
                    &lease_signer,
                ),
                Err(GenerationAuthorityError::AmbiguousCommand)
            ));
        }
    }

    #[test]
    fn competing_command_ids_minting_the_same_target_are_ambiguous_in_any_row_order() {
        let operator = Keys::generate();
        let first_provider = Keys::generate();
        let second_provider = Keys::generate();
        let channel = Uuid::new_v4();
        let first_command = command(
            &operator,
            &first_provider.public_key(),
            channel,
            "create-first",
        );
        let second_command = command(
            &operator,
            &second_provider.public_key(),
            channel,
            "create-second",
        );
        let first_receipt = receipt(&first_provider, channel, "create-first", &target(1));
        let second_receipt = receipt(&second_provider, channel, "create-second", &target(1));

        for (commands, receipts) in [
            (
                vec![first_command.clone(), second_command.clone()],
                vec![first_receipt.clone(), second_receipt.clone()],
            ),
            (
                vec![second_command.clone(), first_command.clone()],
                vec![second_receipt.clone(), first_receipt.clone()],
            ),
        ] {
            for (command_id, lease_signer) in [
                ("create-first", first_provider.public_key()),
                ("create-second", second_provider.public_key()),
            ] {
                assert!(matches!(
                    select_unique_generation_authority(
                        commands.clone(),
                        receipts.clone(),
                        channel,
                        command_id,
                        &target(1),
                        &lease_signer,
                    ),
                    Err(GenerationAuthorityError::AmbiguousGeneration)
                ));
            }
        }
    }

    #[test]
    fn ambiguous_competing_command_id_cannot_hide_a_successful_proof_pair() {
        let operator = Keys::generate();
        let requested_provider = Keys::generate();
        let competing_provider = Keys::generate();
        let channel = Uuid::new_v4();
        let requested_command = command(
            &operator,
            &requested_provider.public_key(),
            channel,
            "requested",
        );
        let first_competing_command = command(
            &operator,
            &competing_provider.public_key(),
            channel,
            "competing",
        );
        let second_competing_command = EventBuilder::new(
            first_competing_command.kind,
            first_competing_command.content.clone(),
        )
        .tags(first_competing_command.tags.clone())
        .custom_created_at(Timestamp::from_secs(
            first_competing_command.created_at.as_secs() + 1,
        ))
        .sign_with_keys(&operator)
        .unwrap();

        assert!(matches!(
            select_unique_generation_authority(
                vec![
                    requested_command,
                    first_competing_command,
                    second_competing_command,
                ],
                vec![
                    receipt(&requested_provider, channel, "requested", &target(1)),
                    receipt(&competing_provider, channel, "competing", &target(1)),
                ],
                channel,
                "requested",
                &target(1),
                &requested_provider.public_key(),
            ),
            Err(GenerationAuthorityError::AmbiguousGeneration)
        ));
    }

    #[test]
    fn wrong_lease_signer_and_failed_receipt_are_not_authority() {
        let operator = Keys::generate();
        let provider = Keys::generate();
        let attacker = Keys::generate();
        let channel = Uuid::new_v4();
        let command = command(&operator, &provider.public_key(), channel, "create-1");
        let receipt = receipt(&provider, channel, "create-1", &target(1));
        assert!(matches!(
            select_unique_generation_authority(
                vec![command],
                vec![receipt],
                channel,
                "create-1",
                &target(1),
                &attacker.public_key(),
            ),
            Err(GenerationAuthorityError::AuthorityMismatch)
        ));
    }

    #[test]
    fn create_command_rejects_resume_status_even_when_target_matches() {
        let operator = Keys::generate();
        let provider = Keys::generate();
        let channel = Uuid::new_v4();
        let command = command(&operator, &provider.public_key(), channel, "create-1");
        let wrong = receipt_with_status(
            &provider,
            channel,
            "create-1",
            &target(1),
            ReceiptStatus::Resumed,
        );
        assert!(matches!(
            select_unique_generation_authority(
                vec![command],
                vec![wrong],
                channel,
                "create-1",
                &target(1),
                &provider.public_key(),
            ),
            Err(GenerationAuthorityError::MissingReceipt)
        ));
    }

    #[test]
    fn resume_command_rejects_create_status_even_when_target_matches() {
        let operator = Keys::generate();
        let provider = Keys::generate();
        let channel = Uuid::new_v4();
        let command = resume_command(
            &operator,
            &provider.public_key(),
            channel,
            "resume-1",
            &target(1),
        );
        let wrong = receipt_with_status(
            &provider,
            channel,
            "resume-1",
            &target(2),
            ReceiptStatus::Created,
        );
        assert!(matches!(
            select_unique_generation_authority(
                vec![command],
                vec![wrong],
                channel,
                "resume-1",
                &target(2),
                &provider.public_key(),
            ),
            Err(GenerationAuthorityError::MissingReceipt)
        ));
    }

    #[test]
    fn receipt_replay_dedupes_while_distinct_receipts_and_failures_fail_closed() {
        let operator = Keys::generate();
        let provider = Keys::generate();
        let channel = Uuid::new_v4();
        let command = command(&operator, &provider.public_key(), channel, "create-1");
        let successful = receipt(&provider, channel, "create-1", &target(1));
        assert!(select_unique_generation_authority(
            vec![command.clone()],
            vec![successful.clone(), successful.clone()],
            channel,
            "create-1",
            &target(1),
            &provider.public_key(),
        )
        .is_ok());
        let distinct_success = EventBuilder::new(successful.kind, successful.content.clone())
            .tags(successful.tags.clone())
            .custom_created_at(Timestamp::from_secs(successful.created_at.as_secs() + 1))
            .sign_with_keys(&provider)
            .unwrap();
        assert!(matches!(
            select_unique_generation_authority(
                vec![command.clone()],
                vec![successful, distinct_success],
                channel,
                "create-1",
                &target(1),
                &provider.public_key(),
            ),
            Err(GenerationAuthorityError::AmbiguousReceipt)
        ));

        let failed_payload = LifecycleReceipt::failed("create-1", "PROVIDER_UNAVAILABLE", "no");
        let failed = EventBuilder::new(
            Kind::Custom(44_224),
            serde_json::to_string(&failed_payload).unwrap(),
        )
        .tags([
            tag(&["h", &channel.to_string()]),
            tag(&["cslr-v", "cslr1-1"]),
            tag(&["csl-command", "create-1"]),
            tag(&[
                "csl-key",
                &structured_key("coding-session-lifecycle-receipt/v1", &["create-1"]),
            ]),
        ])
        .sign_with_keys(&provider)
        .unwrap();
        assert!(matches!(
            select_unique_generation_authority(
                vec![command],
                vec![failed],
                channel,
                "create-1",
                &target(1),
                &provider.public_key(),
            ),
            Err(GenerationAuthorityError::MissingReceipt)
        ));

        let stop = stop_command(
            &operator,
            &provider.public_key(),
            channel,
            "stop-1",
            &target(1),
        );
        assert!(matches!(
            select_unique_generation_authority(
                vec![stop],
                Vec::new(),
                channel,
                "stop-1",
                &target(1),
                &provider.public_key(),
            ),
            Err(GenerationAuthorityError::MissingCommand)
        ));
    }
}
