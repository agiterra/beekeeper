use std::sync::atomic::Ordering;

use super::AppState;

pub(crate) const KEYCHAIN_UNAVAILABLE: &str = "KEYCHAIN_UNAVAILABLE";

impl AppState {
    /// Return a non-secret witness for the already-loaded owner identity.
    ///
    /// This never clones or returns key material and never reads the keychain.
    /// It proves only that boot loaded a signable identity. Recovery flags and
    /// a poisoned in-memory key lock fail closed.
    pub(crate) fn signing_public_key_for_readiness(&self) -> Result<String, String> {
        if self.identity_lost.load(Ordering::Acquire) || self.keyring_locked.load(Ordering::Acquire)
        {
            return Err(KEYCHAIN_UNAVAILABLE.to_string());
        }
        self.keys
            .lock()
            .map_err(|_| KEYCHAIN_UNAVAILABLE.to_string())
            .map(|keys| keys.public_key().to_hex())
    }
}
