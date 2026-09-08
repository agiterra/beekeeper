//! Unit tests for the durable CI-continuation store.

use super::*;
use buzz_core::ci_result::CiPhase;

fn identity(run: &str) -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "11".repeat(32)),
        commit: "ab".repeat(20),
        check: "main-validation".into(),
        run: run.into(),
        attempt: 1,
        workflow: "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
        phase: CiPhase::Build,
    }
}

fn record(command_id: &str, channel_id: Uuid, run: &str) -> CiContinuationRecord {
    let identity = identity(run);
    let correlation_id =
        buzz_core::ci_result::correlation_id(&identity).expect("valid identity digest");
    CiContinuationRecord {
        command_id: command_id.to_owned(),
        registration_event_id: "cc".repeat(32),
        payload_digest: format!("{command_id:0>64}"),
        channel_id,
        signer: "dd".repeat(32),
        target: CodingSessionTarget {
            driver: "claude".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 0,
        },
        identity,
        correlation_id,
        continuation: "report the failing test".into(),
        expires_at: 2_000,
        registered_at: 1_000,
        relay_answered: false,
        attempts: 0,
        next_check_at: 0,
        state: RecordState::Waiting,
    }
}

fn ready() -> ReadyResult {
    ReadyResult {
        result_event_id: "ee".repeat(32),
        result_signer: "ff".repeat(32),
        result_canonical_json: "{}".into(),
        observed_at: 1_500,
    }
}

#[test]
fn an_absent_store_file_opens_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = CiContinuationStore::open(dir.path()).expect("open");
    assert_eq!(store.records().count(), 0);
    assert!(!dir.path().join(STORE_FILE).exists());
}

#[test]
fn a_registration_survives_reopening_the_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    assert_eq!(
        store
            .insert(record("cic-a", channel_id, "1"))
            .expect("insert"),
        Admitted::Stored
    );

    let reopened = CiContinuationStore::open(dir.path()).expect("reopen");
    let stored = reopened.record("cic-a").expect("record survives");
    assert_eq!(stored.continuation, "report the failing test");
    assert_eq!(stored.state, RecordState::Waiting);
}

#[test]
fn an_unreadable_store_file_fails_open_rather_than_starting_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(STORE_FILE), b"{not json").expect("write");
    let error = CiContinuationStore::open(dir.path()).expect_err("must refuse");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_future_schema_version_fails_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(STORE_FILE),
        br#"{"version":99,"registrations":[]}"#,
    )
    .expect("write");
    let error = CiContinuationStore::open(dir.path()).expect_err("must refuse");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn the_same_command_with_the_same_bytes_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    assert_eq!(
        store.admission("cic-a", &format!("{:0>64}", "cic-a"), channel_id),
        Ok(Some(Admitted::AlreadyStored))
    );
    assert_eq!(
        store
            .insert(record("cic-a", channel_id, "1"))
            .expect("second"),
        Admitted::AlreadyStored
    );
    assert_eq!(store.records().count(), 1);
}

#[test]
fn the_same_command_with_different_bytes_is_a_conflict() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    assert_eq!(
        store.admission("cic-a", &"00".repeat(32), channel_id),
        Err(AdmitRefusal::CommandIdConflict)
    );
    let mut different = record("cic-a", channel_id, "1");
    different.payload_digest = "00".repeat(32);
    let error = store.insert(different).expect_err("conflicting insert");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_full_channel_partition_refuses_before_the_global_cap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    for index in 0..MAX_REGISTRATIONS_PER_CHANNEL {
        store
            .insert(record(
                &format!("cic-{index}"),
                channel_id,
                &index.to_string(),
            ))
            .expect("insert");
    }
    let refusal = store
        .admission("cic-overflow", &"01".repeat(32), channel_id)
        .expect_err("channel cap");
    assert!(matches!(refusal, AdmitRefusal::StoreFull { .. }));
    // Another channel still has room: the cap is per channel, not global.
    assert_eq!(
        store.admission("cic-elsewhere", &"01".repeat(32), Uuid::new_v4()),
        Ok(None)
    );
}

#[test]
fn a_terminal_record_frees_a_pending_slot_but_still_fences_its_command_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    store
        .mark_terminal(
            "cic-a",
            "CI_CONTINUATION_EXPIRED",
            3_000,
            Some(RelayObservation::Unanswered),
        )
        .expect("terminal");
    assert_eq!(store.pending_count(), 0);
    assert_eq!(
        store.admission("cic-a", &"02".repeat(32), channel_id),
        Err(AdmitRefusal::CommandIdConflict)
    );
    let stored = store.record("cic-a").expect("retained");
    assert_eq!(
        stored.state,
        RecordState::Terminal {
            code: "CI_CONTINUATION_EXPIRED".into(),
            at: 3_000,
            observation: Some(RelayObservation::Unanswered),
        }
    );
}

#[test]
fn marking_ready_moves_every_waiting_record_for_one_digest_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    store
        .insert(record("cic-b", channel_id, "1"))
        .expect("insert");
    store
        .insert(record("cic-c", channel_id, "2"))
        .expect("insert");
    let digest = store
        .record("cic-a")
        .expect("record")
        .correlation_id
        .clone();

    let mut moved = store.mark_ready(&digest, &ready()).expect("ready");
    moved.sort();
    assert_eq!(moved, vec!["cic-a".to_owned(), "cic-b".to_owned()]);
    // A duplicate result moves nothing a second time.
    assert!(store.mark_ready(&digest, &ready()).expect("dup").is_empty());
    assert_eq!(
        store.record("cic-c").expect("other digest").state,
        RecordState::Waiting
    );
}

#[test]
fn pending_identities_collapse_two_registrations_onto_one_digest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    store
        .insert(record("cic-b", channel_id, "1"))
        .expect("insert");
    assert_eq!(store.pending_identities().len(), 1);

    let digest = store
        .record("cic-a")
        .expect("record")
        .correlation_id
        .clone();
    store.mark_ready(&digest, &ready()).expect("ready");
    // Still watched: a second canonical result arriving before the turn
    // starts must be able to refuse the delivery.
    assert_eq!(store.pending_identities().len(), 1);

    store.remove("cic-a").expect("remove");
    store.remove("cic-b").expect("remove");
    assert!(store.pending_identities().is_empty());
}

#[test]
fn a_relay_answer_is_recorded_against_every_record_for_that_digest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    let digest = store
        .record("cic-a")
        .expect("record")
        .correlation_id
        .clone();
    store.mark_relay_answered(&[digest]).expect("answered");

    let reopened = CiContinuationStore::open(dir.path()).expect("reopen");
    assert!(
        reopened.record("cic-a").expect("record").relay_answered,
        "the answered window must survive a restart"
    );
}

#[test]
fn expiry_and_delivery_queues_only_name_eligible_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    store
        .insert(record("cic-b", channel_id, "2"))
        .expect("insert");
    let digest = store
        .record("cic-b")
        .expect("record")
        .correlation_id
        .clone();
    store.mark_ready(&digest, &ready()).expect("ready");

    assert_eq!(store.ready_for_delivery(1_600), vec!["cic-b".to_owned()]);
    assert!(store.expired(1_600).is_empty());
    let mut expired = store.expired(2_000);
    expired.sort();
    assert_eq!(expired, vec!["cic-a".to_owned(), "cic-b".to_owned()]);

    store.note_attempt("cic-b", 9_000).expect("attempt");
    assert!(store.ready_for_delivery(1_600).is_empty());
    assert_eq!(store.record("cic-b").expect("record").attempts, 1);
}

#[test]
fn removing_a_record_clears_it_from_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    store.remove("cic-a").expect("remove");
    store.remove("cic-a").expect("idempotent remove");

    let reopened = CiContinuationStore::open(dir.path()).expect("reopen");
    assert!(reopened.record("cic-a").is_none());
}

#[test]
fn terminal_retention_is_bounded_and_evicts_the_oldest_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    for index in 0..(MAX_RETAINED_TERMINALS + 8) {
        let command_id = format!("cic-{index}");
        store
            .insert(record(&command_id, channel_id, &index.to_string()))
            .expect("insert");
        store
            .mark_terminal(
                &command_id,
                "CI_CONTINUATION_EXPIRED",
                1_000 + index as u64,
                Some(RelayObservation::AnsweredEmpty),
            )
            .expect("terminal");
    }
    assert_eq!(store.records().count(), MAX_RETAINED_TERMINALS);
    assert!(
        store.record("cic-0").is_none(),
        "the oldest disposition is the one evicted"
    );
    assert!(store
        .record(&format!("cic-{}", MAX_RETAINED_TERMINALS + 7))
        .is_some());
}

#[test]
fn the_store_file_is_the_documented_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let mut store = CiContinuationStore::open(dir.path()).expect("open");
    store
        .insert(record("cic-a", channel_id, "1"))
        .expect("insert");
    let body = std::fs::read_to_string(dir.path().join(STORE_FILE)).expect("read");
    let value: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(value["version"], 1);
    let entry = &value["registrations"][0];
    for field in [
        "commandId",
        "registrationEventId",
        "payloadDigest",
        "channelId",
        "signer",
        "target",
        "identity",
        "correlationId",
        "continuation",
        "expiresAt",
        "registeredAt",
        "relayAnswered",
        "attempts",
        "nextCheckAt",
        "state",
    ] {
        assert!(
            entry.get(field).is_some(),
            "the durable record must carry {field}"
        );
    }
    assert_eq!(entry["state"]["type"], "waiting");
}
