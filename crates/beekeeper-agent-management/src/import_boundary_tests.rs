use super::*;
use beekeeper_host_core::ownership::{self, Phase};
fn fixture() -> (tempfile::TempDir, Context, Transfer, nostr::Keys, Event) {
    let dir = tempfile::tempdir().expect("home");
    let context = Context {
        home: dir.path().to_owned(),
        instance: layout::Instance::Dev,
        native_home: None,
    };
    let owner = nostr::Keys::generate();
    let actor = "ab".repeat(32);
    let event =
        nostr::EventBuilder::new(nostr::Kind::Custom(30177), r#"{"name":"Preserved import"}"#)
            .tags([nostr::Tag::parse(["d", &actor]).expect("actor tag")])
            .sign_with_keys(&owner)
            .expect("original signed head");
    let expected = Transfer {
        operation_id: Uuid::new_v4().to_string(),
        actor_pubkey: actor,
        relay_url: "wss://isolated.example/".into(),
        legacy_nonce: "old-generation".into(),
        definition_event_id: event.id.to_hex(),
        target: None,
        phase: Phase::Prepared,
    };
    ownership::advance(dir.path(), context.instance, expected.clone()).expect("staged ownership");
    (dir, context, expected, owner, event)
}
#[test]
fn exact_original_evidence_survives_cas_retry_and_never_rewrites_scope() {
    let (_dir, context, mut expected, owner, event) = fixture();
    // The capture adapter carries original scope, not an ownership phase claim.
    expected.phase = Phase::Quiesced;
    supply_import_definition_evidence(&context, &expected, &owner.public_key(), &event)
        .expect("capture while actual Prepared");
    let path = evidence_path(
        &context,
        Uuid::parse_str(&expected.operation_id).expect("operation"),
    );
    let original = std::fs::read(&path).expect("evidence");
    ownership::advance(&context.home, context.instance, expected.clone()).expect("quiesced");
    let _pair = ownership::admission_lock(
        &context.home,
        context.instance,
        &expected.actor_pubkey,
        &expected.relay_url,
    )
    .expect("fence");
    ownership::revise_quiesced_import_definition(
        &context.home,
        context.instance,
        &expected,
        &"ef".repeat(32),
    )
    .expect("caller verified conversion");
    assert_eq!(
        read_import_definition_evidence(&context, &expected, &owner.public_key())
            .expect("original evidence after CAS")
            .expect("head")
            .id,
        event.id
    );
    supply_import_definition_evidence(&context, &expected, &owner.public_key(), &event)
        .expect("same bytes retry after CAS");
    assert_eq!(std::fs::read(&path).expect("same bytes"), original);
    let mut foreign = expected.clone();
    foreign.legacy_nonce = "other-generation".into();
    assert!(
        supply_import_definition_evidence(&context, &foreign, &owner.public_key(), &event).is_err()
    );
    assert!(read_import_definition_evidence(&context, &foreign, &owner.public_key()).is_err());
    assert!(read_import_definition_evidence(
        &context,
        &expected,
        &nostr::Keys::generate().public_key()
    )
    .is_err());
    assert_eq!(std::fs::read(&path).expect("preserved"), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path)
                .expect("permissions")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[test]
fn missing_provider_state_and_any_retained_client_operation_refuse_before_cas() {
    let (_dir, context, mut expected, owner, _event) = fixture();
    let scoped = beekeeper_host_core::provision::mint_provider_record(&owner, &expected.relay_url)
        .expect("provider custody");
    let base = context.home.join("provider");
    let config = HostConfig {
        version: 1,
        instance: context.instance,
        relay_url: expected.relay_url.clone(),
        provider_pubkey: scoped.provider_pubkey.clone(),
        session_provider_base_dir: base.clone(),
        provider_state_dir: base.join(&scoped.provider_pubkey),
        runtimes: Vec::new(),
        max_sessions: None,
        turn_idle_timeout_secs: None,
        turn_budget: None,
        provider_command: None,
        written_at: scoped.created_at.clone(),
    };
    beekeeper_host_core::provision::write_commissioning(&context.home, &config, &scoped)
        .expect("commissioned config");
    expected.phase = Phase::Quiesced;
    ownership::advance(&context.home, context.instance, expected.clone()).expect("quiesced");
    let _pair = ownership::admission_lock(
        &context.home,
        context.instance,
        &expected.actor_pubkey,
        &expected.relay_url,
    )
    .expect("fence");
    let operation = Uuid::parse_str(&expected.operation_id).expect("operation");
    assert!(require_unadmitted_import(
        &context,
        operation,
        &expected.actor_pubkey,
        &expected.relay_url
    )
    .is_err());
    assert!(
        !config.provider_state_dir.exists(),
        "readback never creates a missing provider state"
    );
    let mut store =
        beekeeper_session_provider::state::StateStore::open(&config.provider_state_dir, 3600)
            .expect("actual provider store fixture");
    store
        .record_watermark(Uuid::new_v4(), 1)
        .expect("authoritative protected snapshot");
    require_unadmitted_import(
        &context,
        operation,
        &expected.actor_pubkey,
        &expected.relay_url,
    )
    .expect("positive read-only protected state");
    let path = crate::management::local::operation_path(&context, operation)
        .expect("client journal fixture");
    atomic_write::atomic_write_json_restricted(&path, b"{}")
        .expect("retained even partial request");
    let guarded = require_unadmitted_import(
        &context,
        operation,
        &expected.actor_pubkey,
        &expected.relay_url,
    )
    .and_then(|_| {
        ownership::revise_quiesced_import_definition(
            &context.home,
            context.instance,
            &expected,
            &"ef".repeat(32),
        )
        .map_err(Error::Refused)
    });
    assert!(guarded.is_err());
    assert_eq!(
        ownership::for_pair(
            &context.home,
            context.instance,
            &expected.actor_pubkey,
            &expected.relay_url
        )
        .expect("ownership")
        .expect("transfer")
        .definition_event_id,
        expected.definition_event_id
    );
}

#[test]
#[ignore = "subprocess crash-cut fixture invoked by the parent test"]
fn import_evidence_crash_child() {
    let capsule = std::env::var("BEEKEEPER_TEST_IMPORT_EVIDENCE_CAPSULE").expect("capsule");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(capsule).expect("fixture")).expect("JSON");
    let context = Context {
        home: PathBuf::from(value["home"].as_str().expect("home")),
        instance: layout::Instance::Dev,
        native_home: None,
    };
    let expected: Transfer = serde_json::from_value(value["expected"].clone()).expect("scope");
    let owner = PublicKey::from_hex(value["owner"].as_str().expect("owner")).expect("public key");
    let event: Event = serde_json::from_value(value["event"].clone()).expect("signed event");
    supply_import_definition_evidence(&context, &expected, &owner, &event)
        .expect("must hit selected cut");
    panic!("crash cut was not reached");
}
#[test]
fn partial_temporary_and_published_lost_reply_recover_without_rewriting_evidence() {
    for cut in ["partial-temporary", "published-before-reply"] {
        let (_dir, context, expected, owner, event) = fixture();
        let capsule = context.home.join("public-capsule.json");
        atomic_write::atomic_write_json_restricted(&capsule, &serde_json::to_vec(&serde_json::json!({
            "home": context.home, "expected":expected, "owner": owner.public_key().to_hex(), "event":event
        })).expect("fixture JSON")).expect("capsule");
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--ignored",
                "--exact",
                "import_boundary::tests::import_evidence_crash_child",
                "--nocapture",
            ])
            .env_clear()
            .env("BEEKEEPER_TEST_IMPORT_EVIDENCE_CAPSULE", &capsule)
            .env("BEEKEEPER_TEST_IMPORT_EVIDENCE_CRASH", cut)
            .output()
            .expect("isolated child");
        assert_eq!(
            output.status.code(),
            Some(86),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let path = evidence_path(
            &context,
            Uuid::parse_str(&expected.operation_id).expect("op"),
        );
        let before = if cut == "partial-temporary" {
            assert!(
                !path.exists(),
                "incomplete bytes never occupy final evidence path"
            );
            let entries: Vec<_> = std::fs::read_dir(path.parent().expect("dir"))
                .expect("entries")
                .map(|entry| entry.expect("entry").path())
                .collect();
            assert!(entries
                .iter()
                .any(|path| path.extension().is_some_and(|v| v == "tmp")
                    && std::fs::metadata(path).expect("partial").len() == 3));
            None
        } else {
            Some(std::fs::read(&path).expect("complete published artifact"))
        };
        supply_import_definition_evidence(&context, &expected, &owner.public_key(), &event)
            .expect("same operation retry");
        assert_eq!(
            read_import_definition_evidence(&context, &expected, &owner.public_key())
                .expect("read")
                .expect("complete")
                .id,
            event.id
        );
        if let Some(before) = before {
            assert_eq!(std::fs::read(&path).expect("preserved"), before);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).expect("mode").permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
#[test]
fn concurrent_publication_deduplicates_identical_bytes_and_refuses_different_proof() {
    let dir = tempfile::tempdir().expect("dir");
    let owner = nostr::Keys::generate();
    let proofs: Vec<Vec<u8>> = (0..8)
        .map(|index| {
            let event = nostr::EventBuilder::new(
                nostr::Kind::Custom(30177),
                format!("signed original proof {index}"),
            )
            .tags([nostr::Tag::parse(["d", &"ab".repeat(32)]).expect("actor")])
            .sign_with_keys(&owner)
            .expect("signed proof");
            serde_json::to_vec(&event).expect("proof bytes")
        })
        .collect();
    for identical in [true, false] {
        let path = dir.path().join(format!("evidence-{identical}.json"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|index| {
                let path = path.clone();
                let barrier = barrier.clone();
                let bytes = proofs[if identical { 0 } else { index }].clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    (bytes.clone(), publication::publish(&path, &bytes))
                })
            })
            .collect();
        let outcomes: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("publisher"))
            .collect();
        let winner = std::fs::read(&path).expect("winner");
        assert_eq!(
            outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
            if identical { 8 } else { 1 }
        );
        for (bytes, result) in outcomes {
            if result.is_ok() {
                assert_eq!(bytes, winner);
            } else {
                assert!(matches!(result, Err(Error::Conflict(_))));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).expect("mode").permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
