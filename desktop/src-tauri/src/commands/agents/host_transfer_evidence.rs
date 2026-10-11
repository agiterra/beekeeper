//! Verify every responsibility independently before changing actor ownership.
use beekeeper_core_pkg::coding_session_command::CodingSessionTarget;
use beekeeper_core_pkg::coding_session_payload::{decode_coding_session_metadata, SessionStatus};
pub(super) fn require_target_stopped(
    actor: &str,
    target: &CodingSessionTarget,
    provider: &str,
    events: Vec<nostr::Event>,
) -> Result<(), String> {
    let mut evidence = Vec::new();
    let mut terminal_receipt = false;
    for event in events {
        if event.pubkey.to_hex() != provider || event.verify().is_err() {
            continue;
        }
        if event.kind.as_u16() == 44224 {
            if beekeeper_core_pkg::coding_session_payload::decode_coding_session_lifecycle_receipt(
                &event.content,
            )
            .is_ok_and(|r| {
                r.session.as_ref() == Some(target)
                    && r.status
                        == beekeeper_core_pkg::coding_session_payload::ReceiptStatus::Stopped
            }) {
                terminal_receipt = true;
            }
            continue;
        }
        if event.kind.as_u16() != 44223 {
            continue;
        }
        let Ok(metadata) = decode_coding_session_metadata(&event.content) else {
            continue;
        };
        if metadata.agent_ref.as_deref() == Some(actor) && metadata.session == *target {
            evidence.push((event.created_at, event.id, metadata.status));
        }
    }
    if terminal_receipt {
        return Ok(());
    }
    let Some(latest) = evidence.iter().map(|v| v.0).max() else {
        return Err(format!(
            "responsibility {} has no verified terminal evidence",
            target.session_id
        ));
    };
    if evidence
        .iter()
        .filter(|v| v.0 == latest)
        .any(|v| v.2 != SessionStatus::Stopped)
    {
        return Err(format!("responsibility {} is not proved stopped; stop every responsibility before restoring legacy custody",target.session_id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core_pkg::coding_session_payload::LIFECYCLE_RECEIPT_SCHEMA;
    fn target(id: &str, generation: u64) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "test".into(),
            instance_id: "provider".into(),
            session_id: id.into(),
            generation,
        }
    }
    fn stopped(keys: &nostr::Keys, target: &CodingSessionTarget) -> nostr::Event {
        let content = serde_json::json!({"schema":LIFECYCLE_RECEIPT_SCHEMA,"commandId":"stop-1","status":"stopped","session":target,"error":null}).to_string();
        beekeeper_sdk_pkg::builders::build_coding_session_lifecycle_receipt(
            uuid::Uuid::new_v4(),
            "stop-1",
            &content,
        )
        .unwrap()
        .sign_with_keys(keys)
        .unwrap()
    }
    #[test]
    fn primary_stop_does_not_certify_sibling_or_replacement_generation() {
        let provider = nostr::Keys::generate();
        let actor = "ab".repeat(32);
        let primary = target("primary", 1);
        let sibling = target("sibling", 1);
        let receipt = stopped(&provider, &primary);
        let authority = provider.public_key().to_hex();
        assert!(
            require_target_stopped(&actor, &primary, &authority, vec![receipt.clone()]).is_ok()
        );
        assert!(
            require_target_stopped(&actor, &sibling, &authority, vec![receipt.clone()]).is_err()
        );
        assert!(
            require_target_stopped(&actor, &target("primary", 2), &authority, vec![receipt])
                .is_err()
        );
        let other = nostr::Keys::generate();
        assert!(require_target_stopped(
            &actor,
            &sibling,
            &authority,
            vec![stopped(&other, &sibling)]
        )
        .is_err());
        assert!(require_target_stopped(
            &actor,
            &sibling,
            &authority,
            vec![stopped(&provider, &sibling)]
        )
        .is_ok());
    }
}
