//! One slow piece of tick work, run off the provider's run loop (SV-76).
//!
//! The run loop is a single biased `select!`. Anything a tick arm awaits holds
//! every other arm — session events are not recorded and no transcript row
//! (44223/44225) is published — for as long as it takes. SV-72 moved the
//! team-wake relay read off the loop for exactly that reason
//! ([`crate::team_wake_fetch`]); this is the same shape, generalised, for the
//! other tick work that waits on a relay or an adapter: witnessing the relay
//! identity, re-reading unverified authority chains, and reopening a
//! generation a CI continuation came due for.
//!
//! The contract every user keeps:
//!
//! - **One in flight.** [`OffLoopSlot::busy`] gates every start, so the work
//!   is never stacked on a slow relay.
//! - **Bounded.** The caller wraps the spawned future in its own timeout; a
//!   timeout is an ordinary transient failure, answered like any other.
//! - **Applied on the loop.** The answer comes back through the receiver
//!   [`OffLoopSlot::take_receiver`] hands out, in its own `select!` arm, and
//!   only there does it touch provider state.
//! - **An older answer never frees a running slot.** [`OffLoopSlot::settle`]
//!   clears the handle only when its task has finished, so a tick that started
//!   the next task before this answer was taken off the queue keeps it.

use std::future::Future;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// The single slot one kind of off-loop work occupies.
pub(crate) struct OffLoopSlot<T> {
    tx: mpsc::Sender<T>,
    rx: Option<mpsc::Receiver<T>>,
    task: Option<JoinHandle<()>>,
    /// The bound the caller applies to the spawned work. Tests shorten it.
    pub(crate) timeout: Duration,
}

impl<T: Send + 'static> OffLoopSlot<T> {
    pub(crate) fn new(timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            tx,
            rx: Some(rx),
            task: None,
            timeout,
        }
    }

    /// Whether work is still running. A task that ended without reporting
    /// (only a panic can) frees the slot rather than wedging it.
    pub(crate) fn busy(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    /// The queue answers arrive on, for the run loop to hold. `None` after the
    /// first call.
    pub(crate) fn take_receiver(&mut self) -> Option<mpsc::Receiver<T>> {
        self.rx.take()
    }

    /// Run `work` on its own task; its output is delivered to the receiver.
    pub(crate) fn spawn<F>(&mut self, work: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        let tx = self.tx.clone();
        self.task = Some(tokio::spawn(async move {
            let answer = work.await;
            let _ = tx.send(answer).await;
        }));
    }

    /// Called when an answer is applied: forget the handle only if its task
    /// is done. See the module docs.
    pub(crate) fn settle(&mut self) {
        if !self.busy() {
            self.task = None;
        }
    }
}

impl<T> Drop for OffLoopSlot<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Wait for the next answer, or forever when the receiver is gone.
pub(crate) async fn next<T>(rx: &mut Option<mpsc::Receiver<T>>) -> T {
    match rx.as_mut() {
        Some(rx) => match rx.recv().await {
            Some(answer) => answer,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}
