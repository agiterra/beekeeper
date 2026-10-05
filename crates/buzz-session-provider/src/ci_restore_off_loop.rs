//! A CI continuation's generation reopen, run off the run loop (SV-76).
//!
//! A CI result can come due for an execution with no live process — after a
//! provider restart, or after the session's idle shutdown during a long CI
//! run. Delivery first reopens the generation in place
//! ([`Provider::restore_generation`]): a deletion re-check against the relay,
//! a read-only agents clone, the execution scope, and the adapter's own
//! startup, which may take up to [`crate::session::STARTUP_TIMEOUT`]. The
//! run-loop tick and the CI listener arm both awaited all of that inline, and
//! for that whole time nothing else ran — no session event was recorded and no
//! transcript row was published, for every session on the provider.
//!
//! In the run loop ([`Provider::restore_off_loop`]) the reopen now runs as one
//! task at a time: [`Provider::start_ci_restore`] prepares it on the loop
//! (every precondition, each a named obstacle, decided before anything slow)
//! and spawns the slow half; [`Provider::finish_ci_restore`] applies the
//! answer on the loop and delivers the turn. The turn itself is unchanged —
//! the same admission, the same operation-ledger fence, so a continuation is
//! still started at most once. Unit tests that drive delivery directly keep
//! the inline reopen.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::native_restore::{run_restore_job, RestoreRun};
use crate::off_loop::OffLoopSlot;
use crate::Provider;

/// The longest a reopen may take end to end: the adapter's own startup bound
/// plus a minute for the deletion read, the agents clone and the scope.
pub(crate) const CI_RESTORE_TIMEOUT: Duration =
    Duration::from_secs(crate::session::STARTUP_TIMEOUT.as_secs() + 60);

/// A finished reopen, for the registration that asked for it.
pub(crate) struct CiRestoreDone {
    command_id: String,
    run: RestoreRun,
}

pub(crate) type CiRestoreSlot = OffLoopSlot<CiRestoreDone>;

pub(crate) fn new_slot() -> CiRestoreSlot {
    OffLoopSlot::new(CI_RESTORE_TIMEOUT)
}

impl Provider {
    /// The run loop's queue of finished reopens.
    pub(crate) fn take_ci_restore_answers(&mut self) -> Option<mpsc::Receiver<CiRestoreDone>> {
        self.ci_restore.take_receiver()
    }

    /// Whether a reopen is in flight.
    #[cfg(test)]
    pub(crate) fn ci_restore_busy(&self) -> bool {
        self.ci_restore.busy()
    }

    /// Start reopening `session_id` for `command_id`, and return at once.
    ///
    /// One reopen at a time. While one runs, a second registration that needs
    /// one stays `ready` and is picked up by a later tick, so nothing is lost
    /// and no adapter is started twice for one generation. An obstacle found
    /// before anything slow is answered here, exactly as the inline path
    /// answers it.
    pub(crate) fn start_ci_restore(
        &mut self,
        command_id: &str,
        session_id: &str,
    ) -> anyhow::Result<()> {
        if self.ci_restore.busy() {
            return Ok(());
        }
        let job = match self.prepare_restore(session_id) {
            Ok(job) => job,
            Err(obstacle) => {
                // `AlreadyLive` cannot reach here — delivery only reopens a
                // session with no handle — and if it ever does, the next tick
                // finds the handle and delivers.
                self.settle_ci_restore(command_id, Err(obstacle))?;
                return Ok(());
            }
        };
        let meta = job.meta();
        let command_id = command_id.to_owned();
        let limit = self.ci_restore.timeout;
        self.ci_restore.spawn(async move {
            let run = match tokio::time::timeout(limit, run_restore_job(job)).await {
                Ok(run) => run,
                // Dropping the startup future ends any adapter it spawned.
                Err(_) => RestoreRun::timed_out(meta, limit),
            };
            CiRestoreDone { command_id, run }
        });
        Ok(())
    }

    /// Apply a finished reopen on the loop, then deliver the turn it was for
    /// if that is still owed.
    pub(crate) async fn finish_ci_restore(&mut self, done: CiRestoreDone) -> anyhow::Result<()> {
        self.ci_restore.settle();
        let CiRestoreDone { command_id, run } = done;
        let restored = self.finish_restore(run)?;
        if !self.settle_ci_restore(&command_id, restored)? {
            return Ok(());
        }
        // Re-checked, not remembered: the registration may have expired or
        // been answered while the adapter started.
        let Some((record, ready)) = self.ci_delivery_due(&command_id)? else {
            return Ok(());
        };
        if self.sessions.handle(&record.target.session_id).is_none() {
            return Ok(());
        }
        self.deliver_ci_turn(&command_id, record, ready).await
    }
}
