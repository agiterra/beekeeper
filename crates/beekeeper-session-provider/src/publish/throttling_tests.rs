use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::sync::Mutex;

fn event(keys: &Keys, content: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(44223), content)
        .sign_with_keys(keys)
        .expect("sign")
}

#[derive(Default)]
struct Sink(Mutex<Vec<String>>);
impl EventSink for Sink {
    async fn publish(&self, event: nostr::Event) -> Result<(), String> {
        self.0.lock().expect("lock").push(event.content);
        Ok(())
    }
}

struct Gate {
    deadline: Instant,
    sent: bool,
    calls: Mutex<usize>,
}
impl EventSink for Gate {
    async fn publish(&self, _: nostr::Event) -> Result<(), String> {
        panic!("typed path required")
    }
    async fn publish_detailed(&self, _: nostr::Event) -> Result<(), AcknowledgedPublishError> {
        *self.calls.lock().expect("lock") += 1;
        Err(AcknowledgedPublishError::RateLimited {
            deadline: self.deadline,
            sent: self.sent,
        })
    }
}

#[tokio::test(start_paused = true)]
async fn gate_pauses_whole_queue_without_local_attempts_or_lingering_backoff() {
    for sent in [false, true] {
        let dir = tempfile::tempdir().expect("temp");
        let keys = Keys::generate();
        let mut outbox = Outbox::open(dir.path(), &keys.public_key().to_hex()).expect("open");
        outbox
            .enqueue(44225, "a", Priority::Normal, event(&keys, "a"))
            .expect("queue");
        let gate = Gate {
            deadline: Instant::now() + Duration::from_secs(44),
            sent,
            calls: Mutex::new(0),
        };
        outbox.flush(&gate).await.expect("flush");
        assert_eq!(outbox.pending[0].attempts, u32::from(sent));
        assert_eq!(outbox.pending[0].next_attempt_at, None);
        outbox
            .enqueue(44224, "receipt", Priority::Live, event(&keys, "receipt"))
            .expect("queue");
        assert_eq!(outbox.next_retry_delay(), Some(Duration::from_secs(44)));
        outbox.flush(&gate).await.expect("flush");
        tokio::time::advance(Duration::from_secs(43)).await;
        outbox.flush(&gate).await.expect("flush");
        assert_eq!(
            *gate.calls.lock().expect("lock"),
            1,
            "whole queue must wait"
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        let sink = Sink::default();
        assert_eq!(outbox.flush(&sink).await.expect("flush"), 2);
        assert_eq!(*sink.0.lock().expect("lock"), ["receipt", "a"]);
    }
}

#[tokio::test(start_paused = true)]
async fn restart_background_is_below_quota_and_live_receipt_bypasses_it() {
    let dir = tempfile::tempdir().expect("temp");
    let keys = Keys::generate();
    let signer = keys.public_key().to_hex();
    let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
    for i in 0..95 {
        outbox
            .enqueue(
                44223,
                &format!("old-{i}"),
                Priority::High,
                event(&keys, r#"{"status":"disconnected"}"#),
            )
            .expect("queue");
    }
    drop(outbox);
    let mut outbox = Outbox::open(dir.path(), &signer).expect("restart");
    outbox.set_background_interval(Duration::from_secs(3));
    let sink = Sink::default();
    assert_eq!(outbox.flush(&sink).await.expect("flush"), 1);
    outbox
        .enqueue(44224, "receipt", Priority::Live, event(&keys, "receipt"))
        .expect("queue");
    outbox
        .enqueue(
            44225,
            "transcript",
            Priority::Live,
            event(&keys, "transcript"),
        )
        .expect("queue");
    assert_eq!(outbox.next_retry_delay(), Some(Duration::ZERO));
    assert_eq!(outbox.flush_one(&sink).await.expect("flush"), 1);
    assert_eq!(sink.0.lock().expect("lock")[1], "receipt");
    outbox.flush_one(&sink).await.expect("flush");
    for _ in 1..60 {
        tokio::time::advance(Duration::from_secs(1)).await;
        outbox.flush(&sink).await.expect("flush");
    }
    assert_eq!(
        sink.0.lock().expect("lock").len(),
        22,
        "20 background and two live sends in first minute"
    );
}

#[tokio::test]
async fn accepted_status_survives_restart_compaction_but_discard_does_not_claim_acceptance() {
    let dir = tempfile::tempdir().expect("temp");
    let keys = Keys::generate();
    let signer = keys.public_key().to_hex();
    let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
    outbox
        .enqueue_latest(
            44223,
            "session:channel:g1",
            Priority::High,
            event(&keys, "idle"),
        )
        .expect("queue");
    outbox.flush(&Sink::default()).await.expect("flush");
    outbox
        .enqueue_latest(
            44223,
            "discarded",
            Priority::High,
            event(&keys, "never accepted"),
        )
        .expect("queue");
    outbox.discard(|_| true).expect("discard");
    // Force actual compaction, then load the compacted accepted fact.
    let file = OpenOptions::new()
        .append(true)
        .open(&outbox.path)
        .expect("ledger");
    file.set_len(COMPACT_THRESHOLD_BYTES + 1).expect("grow");
    outbox.compact_if_idle().expect("compact");
    drop(outbox);
    let mut outbox = Outbox::open(dir.path(), &signer).expect("restart");
    assert_eq!(
        outbox.accepted_content(44223, "session:channel:g1"),
        Some("idle")
    );
    assert_eq!(outbox.accepted_content(44223, "discarded"), None);
    assert_eq!(outbox.accepted_content(44223, "session:channel:g2"), None);
    outbox
        .enqueue_latest(
            44223,
            "session:channel:g1",
            Priority::High,
            event(&keys, "running"),
        )
        .expect("queue");
    assert_eq!(
        outbox.pending_content(44223, "session:channel:g1"),
        Some("running")
    );
    assert_eq!(
        outbox.accepted_content(44223, "session:channel:g1"),
        Some("idle")
    );
    let rotated =
        Outbox::open(dir.path(), &Keys::generate().public_key().to_hex()).expect("rotation");
    assert_eq!(rotated.accepted_content(44223, "session:channel:g1"), None);
}

#[test]
fn exact_membership_refusal_is_final_but_other_restrictions_are_not() {
    assert_eq!(
        final_rejection_reason(
            "Unexpected message: relay rejected durable event: restricted: not a channel member"
        ),
        Some("restricted: not a channel member")
    );
    assert_eq!(
        final_rejection_reason("restricted: must authenticate"),
        None
    );
}

#[tokio::test]
async fn legacy_catalog_versions_collapse_per_channel_on_restart() {
    let dir = tempfile::tempdir().expect("temp");
    let keys = Keys::generate();
    let signer = keys.public_key().to_hex();
    let channel = uuid::Uuid::new_v4();
    let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
    for revision in [33, 34, 35] {
        let event = EventBuilder::new(Kind::Custom(44222), revision.to_string())
            .tags([Tag::parse(["h", &channel.to_string()]).expect("tag")])
            .sign_with_keys(&keys)
            .expect("sign");
        outbox
            .enqueue(44222, &format!("v{revision}"), Priority::Normal, event)
            .expect("queue");
    }
    drop(outbox);
    let mut outbox = Outbox::open(dir.path(), &signer).expect("restart");
    assert_eq!(outbox.pending_len(), 1);
    assert_eq!(
        outbox.pending_content(44222, &format!("catalog:{channel}")),
        Some("35")
    );
    let sink = Sink::default();
    outbox.flush(&sink).await.expect("flush");
    assert_eq!(*sink.0.lock().expect("lock"), ["35"]);
    drop(outbox);
    let restarted = Outbox::open(dir.path(), &signer).expect("second restart");
    assert_eq!(
        restarted.pending_len(),
        0,
        "old versions must never resurrect after newest ACK"
    );
}
