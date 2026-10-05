//! SV-52: the capture's blocking filesystem work runs on the blocking pool
//! and stops at the capture's deadline. Every directory here is a throwaway
//! created by the test.

use super::*;

#[tokio::test]
async fn blocking_work_does_not_hold_the_async_runtime() {
    // A current-thread runtime: blocking work run on it would stall the
    // timer below until the work finished.
    let work = off_runtime(|| {
        std::thread::sleep(Duration::from_millis(400));
        Ok(())
    });
    tokio::pin!(work);
    tokio::select! {
        _ = &mut work => panic!("the blocking work finished before a 10 ms timer"),
        () = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
    work.await.expect("the work still finishes");
}

#[test]
fn the_untracked_scan_stops_at_the_deadline() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), "a\n").expect("write");
    let failure = omit::classify_untracked(b"a.txt\0", dir.path(), Deadline::after(Duration::ZERO))
        .expect_err("past the deadline");
    assert_eq!(failure.code, UnavailableCode::TimedOut);

    let found = omit::classify_untracked(
        b"a.txt\0",
        dir.path(),
        Deadline::after(Duration::from_secs(60)),
    )
    .expect("within the deadline");
    assert!(
        found.is_empty(),
        "a small readable file is captured, not omitted"
    );
}

#[test]
fn an_index_copy_finishing_after_the_capture_was_abandoned_removes_itself() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("index");
    std::fs::write(&real, b"DIRC").expect("write");
    let scratch = dir.path().join(format!("{SCRATCH_INDEX_PREFIX}test"));

    let abandoned = AtomicBool::new(true);
    assert_eq!(copy_index(&real, &scratch, &abandoned), None);
    assert!(!scratch.exists(), "the late copy removed what it wrote");

    let live = AtomicBool::new(false);
    assert!(copy_index(&real, &scratch, &live).is_some());
    assert!(scratch.exists());
}

#[test]
fn dropping_the_scratch_guard_marks_it_abandoned() {
    let dir = tempfile::tempdir().expect("tempdir");
    let guard = ScratchIndex::new(dir.path());
    let flag = guard.abandoned();
    assert!(!flag.load(Ordering::SeqCst));
    drop(guard);
    assert!(flag.load(Ordering::SeqCst));
}
