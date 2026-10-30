use super::*;
use beekeeper_core::coding_session_payload::ReceiptStatus;
use nostr::Keys;

#[test]
fn stop_evidence_is_exact_and_independent_of_metadata_order() {
    let owner = Keys::generate();
    let provider = Keys::generate();
    let stranger = Keys::generate();
    let channel = Uuid::new_v4();
    let target = CodingSessionTarget {
        driver: "acme-acp".into(),
        instance_id: "fixture".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: Uuid::new_v4().to_string(),
        action: Action::SessionStop {
            session: target.clone(),
            provider_authority_pubkey: provider.public_key().to_hex(),
        },
    };
    let command = |keys: &Keys| {
        beekeeper_sdk::build_coding_session_lifecycle_command(channel, &payload)
            .unwrap()
            .sign_with_keys(keys)
            .unwrap()
    };
    let mut body = LifecycleReceipt::failed(&payload.command_id, "TEST", "fixture");
    body.status = ReceiptStatus::Stopped;
    body.error = None;
    body.session = Some(target.clone());
    let receipt = |keys: &Keys, body: &LifecycleReceipt| {
        beekeeper_sdk::build_coding_session_lifecycle_receipt(
            channel,
            &payload.command_id,
            &serde_json::to_string(body).unwrap(),
        )
        .unwrap()
        .sign_with_keys(keys)
        .unwrap()
    };
    let check = |rows: &[Value], target: &CodingSessionTarget| {
        stopped(
            rows,
            &channel.to_string(),
            &owner.public_key().to_hex(),
            &provider.public_key().to_hex(),
            target,
        )
    };
    let c = serde_json::to_value(command(&owner)).unwrap();
    let r = serde_json::to_value(receipt(&provider, &body)).unwrap();
    assert!(check(&[c.clone(), r.clone()], &target));
    assert!(check(&[r.clone(), c.clone()], &target));
    assert!(!check(std::slice::from_ref(&r), &target));
    assert!(!check(
        &[serde_json::to_value(command(&stranger)).unwrap(), r.clone()],
        &target
    ));
    assert!(!check(
        &[
            c.clone(),
            serde_json::to_value(receipt(&stranger, &body)).unwrap()
        ],
        &target
    ));
    let mut next = target.clone();
    next.generation += 1;
    assert!(!check(&[c.clone(), r], &next));
    body.status = ReceiptStatus::Failed;
    body.error = Some(beekeeper_core::coding_session_payload::ReceiptError {
        code: "SESSION_QUIESCENCE_UNCONFIRMED".into(),
        message: "not stopped".into(),
    });
    assert!(!check(
        &[c, serde_json::to_value(receipt(&provider, &body)).unwrap()],
        &target
    ));
}
