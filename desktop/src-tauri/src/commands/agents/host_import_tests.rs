use super::*;

fn intent(operation: Uuid, floor: u64) -> ImportStage {
    let persona: crate::managed_agents::AgentDefinition = serde_json::from_value(json!({
        "id":"journal-fixture", "display_name":"Journal fixture", "system_prompt":"fixture",
        "name_pool":[], "created_at":"2026-10-11T00:00:00Z", "updated_at":"2026-10-11T00:00:00Z"
    }))
    .unwrap();
    ImportStage {
        operation_id: operation,
        record: persona.into_agent_record(),
        relay_url: "wss://relay.example".into(),
        channel: Uuid::new_v4(),
        cwd: PathBuf::from("/private/tmp/journal-work"),
        activate: true,
        generation: None,
        quiesced: true,
        native_context: None,
        fresh: true,
        definition_revision: None,
        generation_inspected: true,
        replay_floor_unix: Some(floor),
        source_configuration: Some(PrivateConfiguration {
            record: json!({"effective_command":"adapter"}),
        }),
        backfill_since_unix: None,
    }
}

#[test]
fn lost_start_reply_uses_exact_successor_inputs_without_overwriting_import() {
    let temporary = tempfile::tempdir().unwrap();
    let pair = temporary.path().join("pair");
    let original = Uuid::new_v4();
    let successor = Uuid::new_v5(&original, b"explicit-host-start");
    save(&pair.join("stage.json"), &intent(original, 100)).unwrap();
    let protected_before = std::fs::read(pair.join("stage.json")).unwrap();
    save(&pair.join("active-stage.json"), &intent(successor, 200)).unwrap();
    // After ownership advanced but the client lost its reply, resolve the
    // successor's protected floor, not the original import or a new UUID.
    let path = enrollment_stage(&pair, &successor.to_string()).unwrap();
    let loaded: ImportStage = serde_json::from_slice(&private_bytes(&path).unwrap()).unwrap();
    assert_eq!(loaded.operation_id, successor);
    assert_eq!(loaded.replay_floor_unix, Some(200));
    assert_eq!(
        std::fs::read(pair.join("stage.json")).unwrap(),
        protected_before
    );
    // Before ownership advanced, retry still resolves its original inputs.
    assert_eq!(
        enrollment_stage(&pair, &original.to_string()).unwrap(),
        pair.join("stage.json")
    );
    assert!(enrollment_stage(&pair, &Uuid::new_v4().to_string()).is_err());
}

#[test]
fn second_successor_crash_preserves_predecessor_and_exact_pending_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let pair = temporary.path().join("pair");
    let a = Uuid::new_v4();
    let b = Uuid::new_v5(&a, b"explicit-host-start:None");
    let c = Uuid::new_v5(&b, b"explicit-host-start:Some(300)");
    let original = intent(a, 100);
    save(&pair.join("stage.json"), &original).unwrap();
    journal::save_enrollment_stage(&pair, &original).unwrap();
    journal::save_enrollment_stage(&pair, &intent(b, 200)).unwrap();
    let before_b = private_bytes(&enrollment_stage(&pair, &b.to_string()).unwrap()).unwrap();
    // B was admitted then stopped. C is durably reserved, but ownership still
    // names B when the client crashes before the shared request can advance it.
    let pending = intent(c, 300);
    journal::save_enrollment_stage(&pair, &pending).unwrap();
    assert_eq!(
        private_bytes(&enrollment_stage(&pair, &b.to_string()).unwrap()).unwrap(),
        before_b
    );
    let replay = journal::pending_successor(&pair, c).unwrap().unwrap();
    assert_eq!(replay.operation_id, c);
    assert_eq!(replay.replay_floor_unix, Some(300));
    journal::save_enrollment_stage(&pair, &replay).unwrap();
    // Lost reply after ownership advances resolves C independently of the
    // mutable active pointer. A and B remain exact original snapshots.
    assert!(enrollment_stage(&pair, &c.to_string())
        .unwrap()
        .ends_with(format!("{c}.json")));
    assert!(enrollment_stage(&pair, &a.to_string()).unwrap().exists());
    let mut changed = replay;
    changed.replay_floor_unix = Some(999);
    assert!(journal::save_enrollment_stage(&pair, &changed).is_err());
    assert_eq!(
        journal::pending_successor(&pair, c)
            .unwrap()
            .unwrap()
            .replay_floor_unix,
        Some(300)
    );
}
