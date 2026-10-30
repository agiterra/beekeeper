use super::*;

fn reservation() -> (Keys, Operation) {
    let owner = Keys::generate();
    let actor = provision::mint_agent_identity(&owner).expect("scoped actor");
    let public = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "{}")
        .tags([Tag::parse(["d", &actor.pubkey]).expect("actor coordinate")])
        .sign_with_keys(&owner)
        .expect("owner definition");
    let auth: Vec<String> = serde_json::from_str(&actor.auth_tag).expect("attestation tag");
    let profile = nostr::EventBuilder::new(nostr::Kind::Custom(0), "{}")
        .tags([Tag::parse(auth).expect("attestation")])
        .sign_with_keys(&actor.keys)
        .expect("scoped profile");
    let operation = Operation {
        hire: None,
        hire_decision: None,
        request: json!({"type":"create","owner":owner.public_key().to_hex(),"operation":Uuid::new_v4()}),
        actor_pubkey: actor.pubkey,
        events: vec![public, profile],
        target: None,
        receipt_event_id: None,
        predecessor: None,
        custody_handoff: Some(ReservedCustody {
            key: actor.private_key_nsec,
            auth_tag: actor.auth_tag,
        }),
    };
    (owner, operation)
}

#[test]
fn reserved_operation_without_key_recovers_same_identity_then_clears_handoff_permanently() {
    let root = tempfile::tempdir().expect("private state");
    let path = root.path().join("operation.json");
    let (owner, operation) = reservation();
    let actor = operation.actor_pubkey.clone();
    let events = operation.events.clone();
    let request = operation.request.clone();
    save(&path, &operation).expect("identity and scoped handoff reserved atomically");
    let bytes = std::fs::read(&path).expect("reservation");
    assert!(!String::from_utf8(bytes)
        .expect("JSON")
        .contains(&owner.secret_key().to_secret_hex()));
    drop(operation); // The actual after-reservation/before-custody process cut.
    assert!(!root.path().join("actors").join(&actor).join("key").exists());
    let mut retry = read_operation(&path, &request)
        .expect("restart")
        .expect("same reservation");
    let (key, auth) =
        finish_reserved_custody(root.path(), &path, &mut retry, &owner.public_key().to_hex())
            .expect("same scoped mint");
    assert_eq!(Keys::parse(&key).expect("key").public_key().to_hex(), actor);
    assert_eq!(
        beekeeper_sdk::nip_oa::verify_auth_tag(
            &auth,
            &nostr::PublicKey::from_hex(&actor).expect("actor")
        )
        .expect("owner"),
        owner.public_key()
    );
    assert_eq!(retry.events, events);
    assert_eq!(retry.request, request);
    let saved = read_operation(&path, &request)
        .expect("settled")
        .expect("journal");
    assert!(saved.custody_handoff.is_none());
    assert!(
        !serde_json::from_slice::<Value>(&std::fs::read(&path).expect("settled bytes"))
            .expect("JSON")
            .as_object()
            .expect("object")
            .contains_key("custodyHandoff")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)
                .expect("restricted journal")
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    std::fs::remove_dir_all(root.path().join("actors").join(&actor))
        .expect("simulate later confirmed custody deletion");
    let mut replay = read_operation(&path, &request)
        .expect("completed journal")
        .expect("same op");
    assert!(finish_reserved_custody(
        root.path(),
        &path,
        &mut replay,
        &owner.public_key().to_hex()
    )
    .is_err());
    assert!(
        !root.path().join("actors").join(&actor).exists(),
        "cleared handoff cannot resurrect deleted custody"
    );
}

#[test]
fn absent_tampered_foreign_or_completed_handoff_refuses_without_effects_or_journal_changes() {
    for mode in ["absent", "key", "owner", "request", "completed"] {
        let root = tempfile::tempdir().expect("custody");
        let path = root.path().join("operation.json");
        let (owner, mut operation) = reservation();
        match mode {
            "absent" => operation.custody_handoff = None,
            "key" => {
                operation.custody_handoff.as_mut().expect("handoff").key =
                    Keys::generate().secret_key().to_secret_hex()
            }
            "owner" => {
                let foreign = Keys::generate();
                let handoff = operation.custody_handoff.as_mut().expect("handoff");
                let actor = Keys::parse(&handoff.key).expect("actor");
                handoff.auth_tag =
                    beekeeper_sdk::nip_oa::compute_auth_tag(&foreign, &actor.public_key(), "")
                        .expect("valid foreign owner attestation");
                assert_ne!(actor.public_key(), foreign.public_key());
            }
            "request" => operation.request["owner"] = json!(Keys::generate().public_key().to_hex()),
            "completed" => operation.receipt_event_id = Some("ab".repeat(32)),
            _ => unreachable!(),
        }
        save(&path, &operation).expect("reserved journal");
        let original = std::fs::read(&path).expect("bytes");
        assert!(
            finish_reserved_custody(
                root.path(),
                &path,
                &mut operation,
                &owner.public_key().to_hex()
            )
            .is_err(),
            "{mode}"
        );
        assert!(
            !root.path().join("actors").exists(),
            "{mode}: no materialization"
        );
        assert_eq!(std::fs::read(path).expect("unchanged journal"), original);
    }
}

#[test]
fn partial_custody_retry_keeps_exact_key_and_refuses_conflicting_existing_key() {
    let root = tempfile::tempdir().expect("custody");
    let path = root.path().join("operation.json");
    let (owner, mut operation) = reservation();
    save(&path, &operation).expect("reservation");
    let dir = root.path().join("actors").join(&operation.actor_pubkey);
    atomic_write::create_dir_all_restricted(&dir).expect("actor dir");
    let handoff = operation.custody_handoff.as_ref().expect("handoff");
    atomic_write::atomic_write_json_restricted(&dir.join("key"), handoff.key.as_bytes())
        .expect("cut after key before attestation");
    finish_reserved_custody(
        root.path(),
        &path,
        &mut operation,
        &owner.public_key().to_hex(),
    )
    .expect("finish exact partial custody");
    assert!(operation.custody_handoff.is_none());
    let (owner, mut operation) = reservation();
    save(&path, &operation).expect("another reservation");
    let dir = root.path().join("actors").join(&operation.actor_pubkey);
    atomic_write::create_dir_all_restricted(&dir).expect("dir");
    let conflicting = Keys::generate().secret_key().to_secret_hex();
    atomic_write::atomic_write_json_restricted(&dir.join("key"), conflicting.as_bytes())
        .expect("conflicting custody");
    assert!(finish_reserved_custody(
        root.path(),
        &path,
        &mut operation,
        &owner.public_key().to_hex()
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("key")).expect("original retained"),
        conflicting
    );
}
