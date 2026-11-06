//! The execution witness's Windows creation-time identity: how it is stored,
//! that old records still read, and that the witness decides on the pair.

use super::ExecutionProcessWitness;
use beekeeper_core::coding_session_command::CodingSessionTarget;

const T: u64 = 134_000_000_000_000_000;

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "python".into(),
        instance_id: "4a1129008ad5fd92".into(),
        session_id: "bd551f6d-76eb-484a-b90d-1b34f92ea927".into(),
        generation: 1,
    }
}

fn witness(group_id: Option<u32>, creation_filetime: Option<u64>) -> ExecutionProcessWitness {
    ExecutionProcessWitness {
        target: target(),
        attempted_target: None,
        group_id,
        creation_filetime,
        quiesced: false,
    }
}

#[test]
fn the_creation_time_survives_a_serialization_round_trip() {
    let written = witness(Some(4242), Some(T));
    let json = serde_json::to_string(&written).expect("serialize");
    assert!(
        json.contains(&format!("\"creationFiletime\":{T}")),
        "{json}"
    );
    let read: ExecutionProcessWitness = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(read, written);
    assert_eq!(read.creation_filetime, Some(T));
    // Exact: a FILETIME is far above 2^53 and must not pass through a float.
    assert_eq!(
        serde_json::from_str::<ExecutionProcessWitness>(
            &json.replace(&T.to_string(), "134000000000000001")
        )
        .expect("deserialize")
        .creation_filetime,
        Some(134_000_000_000_000_001)
    );
}

#[test]
fn a_record_written_before_the_identity_existed_still_reads_and_has_none() {
    let old = r#"{"target":{"driver":"python","instanceId":"4a1129008ad5fd92","sessionId":"bd551f6d-76eb-484a-b90d-1b34f92ea927","generation":1},"groupId":4242,"quiesced":false}"#;
    let read: ExecutionProcessWitness = serde_json::from_str(old).expect("legacy witness");
    assert_eq!(read.group_id, Some(4242));
    assert_eq!(read.creation_filetime, None);
    assert!(!read.quiesced);
}

#[test]
fn a_witness_without_the_identity_writes_exactly_what_it_wrote_before() {
    // Unix never has one: its state files stay byte-for-byte what they were.
    let json = serde_json::to_string(&witness(Some(4242), None)).expect("serialize");
    assert!(!json.contains("creationFiletime"), "{json}");
}

#[test]
fn a_quiesced_witness_is_absent_whatever_the_process_table_says() {
    let mut quiesced = witness(None, None);
    quiesced.quiesced = true;
    assert!(quiesced.proves_absent(&target()));
}

#[test]
fn a_witness_for_a_different_target_proves_nothing() {
    let mut other = target();
    other.generation = 2;
    assert!(!witness(Some(4242), Some(T)).proves_absent(&other));
}

#[test]
fn no_pid_recorded_is_unproved() {
    assert!(!witness(None, Some(T)).proves_absent(&target()));
}

#[cfg(windows)]
mod windows {
    use super::*;
    use win_custody::{Process, ProcessAccess};

    fn own_creation() -> u64 {
        Process::open(std::process::id(), ProcessAccess::QueryLimited)
            .expect("open self")
            .creation_time()
            .expect("own creation time")
    }

    #[test]
    fn a_live_recorded_child_is_present_not_absent() {
        let present = witness(Some(std::process::id()), Some(own_creation()));
        assert!(!present.proves_absent(&target()));
    }

    #[test]
    fn a_reused_pid_is_absent_for_the_recorded_generation() {
        let reused = witness(Some(std::process::id()), Some(own_creation() + 10_000_000));
        assert!(reused.proves_absent(&target()));
    }

    #[test]
    fn a_legacy_windows_witness_is_unproved_even_for_a_live_or_missing_pid() {
        for pid in [std::process::id(), 0x7fff_fff0] {
            assert!(
                !witness(Some(pid), None).proves_absent(&target()),
                "pid {pid}"
            );
            assert!(
                !witness(Some(pid), Some(0)).proves_absent(&target()),
                "pid {pid}, zero"
            );
        }
    }

    #[test]
    fn a_recorded_child_that_has_exited_and_been_released_is_absent() {
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "exit", "0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn");
        let pid = child.id();
        let recorded = Process::open(pid, ProcessAccess::QueryLimited)
            .and_then(|process| process.creation_time())
            .expect("creation time while the child is held");
        let gone = witness(Some(pid), Some(recorded));
        child.wait().expect("wait");
        drop(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !gone.proves_absent(&target()) {
            assert!(
                std::time::Instant::now() < deadline,
                "the exited child never read as absent"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
