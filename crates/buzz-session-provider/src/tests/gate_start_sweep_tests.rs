//! SV-41 S5: a gate start the previous provider left open is closed at boot.
//!
//! A provider that stops mid-gate takes the agent and its command with it.
//! The in-memory observer that would have closed the start is gone too, so
//! without the persisted store the start would read "gate running" for 30
//! minutes about a process that no longer exists.

use super::*;

use buzz_core::coding_session_observation::{
    CodingSessionObservationBody, CodingSessionObservationPhaseTiming,
};
use buzz_sdk::coding_session_observation::parse_coding_session_observation;

fn tool_call(tool_id: &str, command: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "tool_call",
        "tool": {
            "toolName": "Bash",
            "toolId": tool_id,
            "toolKind": "execute",
            "input": { "command": command },
        },
    })
}

fn feed(provider: &mut Provider, session_id: &str, items: Vec<serde_json::Value>) {
    provider
        .handle_session_event(session::SessionEvent::TranscriptItems {
            session_id: session_id.to_owned(),
            turn_id: "turn-1".to_owned(),
            items,
        })
        .expect("transcript items");
}

/// Every `gate:` phase row queued since the last flush.
async fn flushed_gate_phases(provider: &mut Provider) -> Vec<CodingSessionObservationPhaseTiming> {
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    sink.all()
        .into_iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_OBSERVATION)
        .filter_map(|event| {
            match parse_coding_session_observation(&event)
                .expect("44246")
                .body
            {
                CodingSessionObservationBody::Phase(phase) if phase.phase.starts_with("gate:") => {
                    Some(phase)
                }
                _ => None,
            }
        })
        .collect()
}

/// A provider with one governed session that has published one open start.
async fn provider_with_an_open_start(
    dir: &tempfile::TempDir,
) -> (Provider, String, CodingSessionObservationPhaseTiming) {
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let mut provider = provider(&state_dir, None);
    let record = governed_record(Uuid::new_v4(), &cwd, &"ab".repeat(32));
    let session_id = record.session_id.clone();
    provider.state.insert_session(record).expect("insert");
    feed(
        &mut provider,
        &session_id,
        vec![tool_call("t1", "cargo test -p buzz-core")],
    );
    provider.publish_due_gate_starts_at(now_ms() + 60_000);
    let starts = flushed_gate_phases(&mut provider).await;
    assert_eq!(starts.len(), 1, "one start on the wire");
    let start = starts[0].clone();
    assert_eq!(start.ended_at_ms, None);
    assert_eq!(
        provider.gate_starts_open.entries().len(),
        1,
        "the start is on disk before it is queued"
    );
    (provider, session_id, start)
}

#[tokio::test]
async fn a_start_the_previous_provider_left_open_is_closed_at_boot_with_no_span() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (dead, _session_id, start) = provider_with_an_open_start(&dir).await;
    // The provider dies mid-gate: no result, no exit, no turn end.
    drop(dead);

    let mut booted = provider(&dir.path().join("state"), None);
    booted.recover().await.expect("recover");
    let closes = flushed_gate_phases(&mut booted).await;
    assert_eq!(closes.len(), 1, "the boot closes the start it inherited");
    let close = &closes[0];
    assert_eq!(close.phase, start.phase);
    assert_eq!(close.started_at_ms, start.started_at_ms, "the pairing key");
    assert!(close
        .ended_at_ms
        .is_some_and(|ended| ended >= start.started_at_ms));
    assert_eq!(
        close.duration_ms, None,
        "the provider never saw the gate end and claims no span"
    );
    assert!(booted.gate_starts_open.entries().is_empty());
    assert!(
        gate_start_store::GateStartStore::open(&dir.path().join("state"))
            .expect("reopen")
            .entries()
            .is_empty(),
        "the store is cleared on disk, not only in memory"
    );

    // Running the sweep again enqueues nothing more.
    booted.close_gate_starts_left_open();
    assert!(flushed_gate_phases(&mut booted).await.is_empty());
}

#[tokio::test]
async fn the_boot_close_carries_the_boot_time_and_is_clamped_to_the_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (dead, _session_id, start) = provider_with_an_open_start(&dir).await;
    drop(dead);
    let mut booted = provider(&dir.path().join("state"), None);
    let started = i64::try_from(start.started_at_ms).expect("ms");

    // A boot clock behind the start (the clock moved backwards).
    booted.close_gate_starts_left_open_at(started - 5_000);
    let closes = flushed_gate_phases(&mut booted).await;
    assert_eq!(closes.len(), 1);
    assert_eq!(closes[0].ended_at_ms, Some(start.started_at_ms));
    assert_eq!(closes[0].duration_ms, None);
}

#[tokio::test]
async fn a_start_closed_by_its_result_leaves_nothing_for_the_boot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, session_id, _start) = provider_with_an_open_start(&dir).await;
    provider
        .handle_session_event(session::SessionEvent::Exited {
            session_id: session_id.clone(),
            reason: session::ExitReason::Requested,
        })
        .expect("exit");
    assert_eq!(flushed_gate_phases(&mut provider).await.len(), 1, "closed");
    assert!(
        provider.gate_starts_open.entries().is_empty(),
        "a queued close clears its start"
    );
    drop(provider);

    let mut booted = provider_at(&dir);
    booted.close_gate_starts_left_open();
    assert!(
        flushed_gate_phases(&mut booted).await.is_empty(),
        "a closed start is never closed twice"
    );
}

#[tokio::test]
async fn an_open_start_whose_session_record_is_gone_is_cleared_without_a_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("mkdir");
    {
        let mut store = gate_start_store::GateStartStore::open(&state_dir).expect("open");
        store
            .record(gate_start_store::OpenGateStart {
                session_id: "no-such-session".into(),
                gate: "cargo test".into(),
                started_at_ms: 1_000,
            })
            .expect("record");
    }
    let mut booted = provider(&state_dir, None);
    booted.close_gate_starts_left_open();
    assert!(flushed_gate_phases(&mut booted).await.is_empty());
    assert!(booted.gate_starts_open.entries().is_empty());
}

#[test]
fn a_corrupt_store_is_quarantined_and_starts_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("gate-starts-open.json"), b"{not json").expect("write");
    let store = gate_start_store::GateStartStore::open(dir.path()).expect("open");
    assert!(store.entries().is_empty());
    let quarantined = std::fs::read_dir(dir.path())
        .expect("read_dir")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("gate-starts-open.json.quarantined-")
        });
    assert!(quarantined, "the evidence is kept, not deleted");
}

#[test]
fn a_full_store_refuses_and_a_repeat_is_a_no_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = gate_start_store::GateStartStore::open(dir.path()).expect("open");
    let entry = |n: usize| gate_start_store::OpenGateStart {
        session_id: format!("s{n}"),
        gate: "cargo test".into(),
        started_at_ms: 1,
    };
    store.record(entry(0)).expect("record");
    store.record(entry(0)).expect("repeat");
    assert_eq!(store.entries().len(), 1);
    for n in 1..gate_start_store::MAX_OPEN_GATE_STARTS {
        store.record(entry(n)).expect("record");
    }
    assert!(store
        .record(entry(gate_start_store::MAX_OPEN_GATE_STARTS))
        .is_err());
}

fn provider_at(dir: &tempfile::TempDir) -> Provider {
    provider(&dir.path().join("state"), None)
}
