//! The group watch against real process groups whose only member has exited
//! and is not yet reaped. Darwin answers `EPERM` for such a group, which is
//! what C4's Codex rewind hit (ledger 387(b)); only `ESRCH` proves it gone.
//!
//! Each child is this test's own `sh -c 'exit 0'` in a group of its own, and
//! this test is its parent, so nothing else can reap it.

use super::kill_process_group_and_watch;
use std::os::unix::process::CommandExt;
use std::time::Duration;

/// A group whose one member has exited and waits for this test to reap it.
fn exited_unreaped_group() -> std::process::Child {
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .process_group(0)
        .spawn()
        .expect("spawn");
    // Long enough for the child to exit; it stays a zombie until waited.
    std::thread::sleep(Duration::from_millis(200));
    child
}

#[tokio::test]
async fn an_exited_group_reaped_inside_the_bound_is_proven_gone() {
    let mut child = exited_unreaped_group();
    let pid = child.id();
    let reaper = tokio::task::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(150));
        child.wait().expect("reap")
    });
    assert!(
        kill_process_group_and_watch(Some(pid), "group-watch-reaped", Duration::from_secs(5)).await,
        "an unsignalable group that becomes absent is gone"
    );
    reaper.await.expect("reaper");
}

#[tokio::test]
async fn a_group_still_present_at_the_bound_stays_unproven() {
    let mut child = exited_unreaped_group();
    let proven = kill_process_group_and_watch(
        Some(child.id()),
        "group-watch-unreaped",
        Duration::from_millis(300),
    )
    .await;
    child.wait().expect("reap");
    assert!(!proven, "a group never observed absent is not proven gone");
}
