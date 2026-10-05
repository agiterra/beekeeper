//! The tick's relay-identity retry, run off the run loop (SV-76).
//!
//! Startup witnesses the relay's NIP-11 `self` inline, once, before the loop
//! starts ([`Provider::witness_relay_identity`]). When that fails the tick
//! retries until an identity is known — and the retry used to await the
//! NIP-11 fetch inside the tick arm, so a relay that accepted the connection
//! and answered slowly held transcript publication for as long as it took,
//! every tick. Now the tick starts the fetch ([`Provider::start_relay_identity_witness`])
//! and the run loop applies its answer in its own arm
//! ([`Provider::finish_relay_identity_witness`]).

use std::time::Duration;

use tokio::sync::mpsc;

use crate::off_loop::OffLoopSlot;
use crate::Provider;

/// The longest one off-loop NIP-11 identity read may take.
pub(crate) const RELAY_IDENTITY_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// A finished identity read: the verified `self`, none published, or why not.
pub(crate) type RelayIdentityAnswer = Result<Option<String>, String>;

pub(crate) type RelayIdentitySlot = OffLoopSlot<RelayIdentityAnswer>;

pub(crate) fn new_slot() -> RelayIdentitySlot {
    OffLoopSlot::new(RELAY_IDENTITY_FETCH_TIMEOUT)
}

impl Provider {
    /// The run loop's queue of finished identity reads.
    pub(crate) fn take_relay_identity_answers(
        &mut self,
    ) -> Option<mpsc::Receiver<RelayIdentityAnswer>> {
        self.relay_identity_fetch.take_receiver()
    }

    /// Start the identity read when one is still needed, and return at once.
    ///
    /// Nothing to do when an identity is known (it is the trust root and is
    /// never re-read), when there is no relay reader, or when a read is
    /// already out.
    pub(crate) fn start_relay_identity_witness(&mut self) {
        if self.relay_self.is_some() || self.relay_identity_fetch.busy() {
            return;
        }
        let Some(rest) = self.rest_client.clone() else {
            return;
        };
        let timeout = self.relay_identity_fetch.timeout;
        self.relay_identity_fetch.spawn(async move {
            match tokio::time::timeout(timeout, rest.fetch_relay_self_verified()).await {
                Ok(Ok(answer)) => Ok(answer),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err(format!(
                    "the relay identity read did not finish within {}s",
                    timeout.as_secs_f64()
                )),
            }
        });
    }

    /// Apply one identity read on the loop.
    ///
    /// Returns whether an identity is known afterwards. An identity already
    /// witnessed — by startup, or by an earlier read — is never replaced.
    pub(crate) fn finish_relay_identity_witness(&mut self, answer: RelayIdentityAnswer) -> bool {
        self.relay_identity_fetch.settle();
        if self.relay_self.is_some() {
            return true;
        }
        match answer {
            Ok(Some(relay_self)) => {
                tracing::info!(target: "csp::authority", %relay_self, "witnessed relay identity");
                self.set_relay_self(relay_self);
                true
            }
            Ok(None) => false,
            Err(error) => {
                tracing::debug!(
                    target: "csp::authority",
                    "the relay identity is still not readable: {error}"
                );
                false
            }
        }
    }

    /// Whether an identity read is in flight.
    #[cfg(test)]
    pub(crate) fn relay_identity_fetch_busy(&self) -> bool {
        self.relay_identity_fetch.busy()
    }
}
