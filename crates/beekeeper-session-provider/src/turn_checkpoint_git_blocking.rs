//! Blocking filesystem work a capture does, run off the async runtime and
//! bounded by the capture's own deadline (SV-52; see [`super`] § Off the
//! runtime).

use std::time::{Duration, Instant};

use super::{CaptureFailure, UnavailableCode};

/// When a capture is abandoned, for the blocking work a timeout cannot
/// cancel (see [`super`] § Off the runtime).
#[derive(Debug, Clone, Copy)]
pub(super) struct Deadline {
    /// `None` when the ceiling is too far out to represent: never passes.
    at: Option<Instant>,
    ceiling: Duration,
}

impl Deadline {
    pub(super) fn after(ceiling: Duration) -> Self {
        Self {
            at: Instant::now().checked_add(ceiling),
            ceiling,
        }
    }

    /// Whether the capture's ceiling has elapsed.
    pub(super) fn passed(self) -> bool {
        self.at.is_some_and(|at| Instant::now() >= at)
    }

    /// The failure a capture past its ceiling returns.
    pub(super) fn timed_out(self) -> CaptureFailure {
        CaptureFailure::new(
            UnavailableCode::TimedOut,
            format!(
                "Capturing the working tree took longer than {} ms, so it was abandoned.",
                self.ceiling.as_millis()
            ),
        )
    }
}

/// Run blocking filesystem work on the blocking pool rather than the async
/// runtime. The work bounds itself by its [`Deadline`].
pub(super) async fn off_runtime<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CaptureFailure> + Send + 'static,
) -> Result<T, CaptureFailure> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|_| {
        Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "The capture's check of the working tree's files stopped unexpectedly, so nothing \
             was captured.",
        ))
    })
}
