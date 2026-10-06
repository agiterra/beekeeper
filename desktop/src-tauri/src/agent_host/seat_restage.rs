//! Re-staging agent seats for a provider this app did not start.
//!
//! When the app owned the provider, its supervisor handed the pid straight to
//! the re-stage path: spawn the child, wait up to 30 seconds for
//! `seat-requests.json` to name that pid, then re-stage. The app no longer
//! spawns anything, so the handshake inverts — and the inverted version is the
//! better design even without a daemon. The file **already** names the current
//! live child, so there is nothing to wait for: the app reads the pid the host
//! reports, compares it to the pid in the file, and acts when they agree.
//!
//! # Why this stays in the app
//!
//! `restage_actor_seats_for_provider` takes an `AppHandle` and an `AppState`,
//! holds the managed-agent store lock, loads managed-agent records and reads
//! the relay. None of that can move to a headless host, and managed agents are
//! out of this landing's scope anyway.
//!
//! # The degradation, disclosed
//!
//! With no Beekeeper running, seats are not re-staged until one next runs. That
//! is acceptable — seats belong to managed agents, which are still the app's —
//! but it must be *visible*, so the host's `status` carries
//! `SEAT_RESTAGE_REQUIRES_DESKTOP` whenever rows are waiting. A degradation
//! nobody is told about is how a person concludes the product is broken.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use tauri::{AppHandle, Manager};

use crate::app_state::AppState;
use crate::managed_agents::actor_seats_restage::{
    read_seat_requests, restage_actor_seats_for_provider, seat_requests_path,
};

/// The child pid this app has already re-staged for.
///
/// Process-lifetime, not persisted: a restart of the app should re-stage
/// again, because it has no way to know whether the previous run finished. A
/// second re-stage of the same generation is idempotent; a missed one leaves a
/// seat unable to act.
static LAST_RESTAGED_PID: AtomicU32 = AtomicU32::new(0);

/// Re-stage seats if the live child has rows waiting and we have not served
/// this child yet.
///
/// Best-effort and fire-and-forget: it is called from the status poll, which
/// must answer quickly and must not fail because a re-stage did. A re-stage
/// that does not run this time is not lost — the file still names the row, and
/// the next poll tries again.
pub(crate) fn restage_if_needed(app: &AppHandle, state_dir: &Path, live_pid: Option<u32>) {
    let Some(pid) = live_pid else {
        // No live child: nothing to re-stage *for*. Clear the marker so the
        // next child gets served even if it happens to reuse a pid.
        LAST_RESTAGED_PID.store(0, Ordering::Release);
        return;
    };
    if LAST_RESTAGED_PID.load(Ordering::Acquire) == pid {
        return;
    }
    // The file must already name this exact child. A row from a previous
    // generation is not this child's work, and acting on it would re-stage
    // seats against a provider that no longer holds them.
    let names_this_child = read_seat_requests(&seat_requests_path(state_dir))
        .is_ok_and(|file| file.provider_pid == pid && !file.requests.is_empty());
    if !names_this_child {
        return;
    }
    // Claim the pid before the work, not after: the status poll can run
    // concurrently, and two re-stages racing over one generation would each
    // load the managed-agent store and contend on its lock.
    if LAST_RESTAGED_PID.swap(pid, Ordering::AcqRel) == pid {
        return;
    }

    let app = app.clone();
    let state_dir = state_dir.to_path_buf();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let relay_url = crate::relay::relay_ws_url_with_override(&state);
        // Never cancelled: the app is not shutting the provider down any more,
        // so there is no stop flag for this to observe.
        let never = AtomicBool::new(false);
        match restage_actor_seats_for_provider(&app, &state, &state_dir, &relay_url, pid, &never)
            .await
        {
            Ok(report) => {
                if report.requested > 0 {
                    eprintln!(
                        "beekeeper-desktop: agent-host: re-staged {} of {} agent seats for provider \
                         pid {pid}",
                        report.staged, report.requested
                    );
                }
            }
            Err(error) => {
                eprintln!(
                    "beekeeper-desktop: agent-host: agent seat re-stage failed for provider pid \
                     {pid}: {error}"
                );
                // Release the claim so a later poll retries. A permanent
                // failure then logs once per poll, which is noisy and true;
                // a claim held over a failure would be quiet and wrong.
                let _ =
                    LAST_RESTAGED_PID.compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marker is what stops the status poll re-staging the same generation
    /// every few seconds. These assertions are about the decision, not the
    /// re-stage itself, which needs an `AppHandle`.
    #[test]
    fn a_child_is_claimed_once_and_a_gone_child_clears_the_claim() {
        LAST_RESTAGED_PID.store(0, Ordering::Release);
        assert_eq!(LAST_RESTAGED_PID.swap(42, Ordering::AcqRel), 0);
        // A second claim on the same pid is refused, which is what keeps the
        // poll from re-staging on every tick.
        assert_eq!(LAST_RESTAGED_PID.swap(42, Ordering::AcqRel), 42);
        LAST_RESTAGED_PID.store(0, Ordering::Release);
        assert_eq!(LAST_RESTAGED_PID.load(Ordering::Acquire), 0);
    }

    /// A row from a previous generation must not be acted on: the pid in the
    /// file is the whole handshake now that nothing waits for a snapshot.
    #[test]
    fn only_a_file_naming_this_exact_child_counts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = seat_requests_path(dir.path());
        let names = |pid: u32| {
            read_seat_requests(&path)
                .is_ok_and(|file| file.provider_pid == pid && !file.requests.is_empty())
        };
        assert!(!names(42), "no file at all");

        let row = r#"{"commandId":"c-1","actor":"aa","role":"lead","sessionId":"s-1"}"#;
        std::fs::write(
            &path,
            format!(r#"{{"version":1,"providerPid":7,"requests":[{row}]}}"#),
        )
        .expect("write");
        assert!(
            !names(42),
            "a previous generation's row is not this child's"
        );
        assert!(names(7));

        std::fs::write(&path, r#"{"version":1,"providerPid":42,"requests":[]}"#).expect("write");
        assert!(!names(42), "an empty list is nothing to re-stage");
    }
}
