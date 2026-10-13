use super::*;
use crate::managed_agents::retention::inbound_event_outcome;
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};
use std::sync::{mpsc, Arc};

fn retained(keys: &Keys, time: u64) -> RetainedEvent {
    let actor = "a".repeat(64);
    let event = EventBuilder::new(Kind::Custom(30177), "configured")
        .tags([Tag::parse(["d", &actor]).expect("actor tag")])
        .custom_created_at(Timestamp::from(time))
        .sign_with_keys(keys)
        .expect("signed fixture");
    RetainedEvent {
        kind: 30177,
        pubkey: keys.public_key().to_hex(),
        d_tag: actor,
        content: event.content.clone(),
        created_at: time as i64,
        raw_event: event.as_json(),
        pending_sync: false,
    }
}

#[test]
fn accepted_ack_waits_for_inbound_preflight_save_and_retain_without_poisoning() {
    let dir = tempfile::tempdir().expect("isolated retention");
    let db = dir.path().join("retention.sqlite");
    let lock = Arc::new(Mutex::new(()));
    let event = retained(&Keys::generate(), 100);
    let conn = open_retention_db(&db).expect("retention");
    let inlet = lock.lock().expect("inlet store guard");
    assert_eq!(
        inbound_event_outcome(&conn, &event).expect("preflight"),
        InboundOutcome::Applied
    );
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let ack_lock = lock.clone();
    let ack_db = db.clone();
    let ack_event = event.clone();
    let ack = std::thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        let outcome = retain_with_store_lock(&ack_lock, &ack_db, &ack_event);
        done_tx.send(outcome).expect("completion");
    });
    ready_rx.recv().expect("ack worker ready");
    assert!(matches!(
        done_rx.recv_timeout(std::time::Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    // This is the inlet's real final retain after its source/public cache save.
    // The acknowledgment cannot turn this Applied decision into Skipped.
    assert_eq!(
        retain_inbound_event(&conn, &event).expect("inlet retain"),
        InboundOutcome::Applied
    );
    drop(inlet);
    done_rx
        .recv()
        .expect("ack completes")
        .expect("exact echo accepted");
    ack.join().expect("ack worker");
    assert!(lock.lock().is_ok(), "store mutex never poisoned");
    let revision = serde_json::from_str::<Event>(&event.raw_event)
        .expect("event")
        .id
        .to_hex();
    require_exact_head(&db, &event.pubkey, &event.d_tag, &revision).expect("exact signed revision");
}

#[test]
fn acknowledgment_refuses_newer_or_pending_heads_and_preserves_their_bytes() {
    let dir = tempfile::tempdir().expect("isolated retention");
    let db = dir.path().join("retention.sqlite");
    let keys = Keys::generate();
    let old = retained(&keys, 100);
    let newer = retained(&keys, 101);
    let lock = Mutex::new(());
    retain_with_store_lock(&lock, &db, &newer).expect("newer head");
    assert!(retain_with_store_lock(&lock, &db, &old).is_err());
    let conn = open_retention_db(&db).expect("retention");
    assert_eq!(
        get_retained_event(&conn, 30177, &old.pubkey, &old.d_tag)
            .expect("read")
            .expect("head")
            .raw_event,
        newer.raw_event
    );
    let old_revision = serde_json::from_str::<Event>(&old.raw_event)
        .expect("old")
        .id
        .to_hex();
    assert!(require_exact_head(&db, &old.pubkey, &old.d_tag, &old_revision).is_err());
    conn.execute("UPDATE persona_events SET pending_sync=1", [])
        .expect("pending publication");
    let revision = serde_json::from_str::<Event>(&newer.raw_event)
        .expect("newer")
        .id
        .to_hex();
    assert!(require_exact_head(&db, &newer.pubkey, &newer.d_tag, &revision).is_err());
}
