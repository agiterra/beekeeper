//! One pre-publish Start inventory reads each identical filter once (ledger 403).
//!
//! An actor with history repeats the same reads per row — its owner profile,
//! each provider's profile, each provider/channel receipt set — and under the
//! relay's per-identity budget a Start sat in rate-limit back-off with no
//! outcome. Reads only: writes and scoping are refused, and the cache lives for
//! one inventory, never across a publish or a wait.

use std::collections::HashMap;
use std::sync::Mutex;

use nostr::Keys;
use serde_json::Value;

use crate::{Error, RelayTransport};

pub(crate) struct ReadCache<'a, T> {
    inner: &'a T,
    reads: Mutex<HashMap<String, Vec<Value>>>,
}

impl<'a, T: RelayTransport> ReadCache<'a, T> {
    pub(crate) fn new(inner: &'a T) -> Self {
        Self {
            inner,
            reads: Mutex::new(HashMap::new()),
        }
    }
}

impl<T: RelayTransport> RelayTransport for ReadCache<'_, T> {
    fn keys(&self) -> &Keys {
        self.inner.keys()
    }
    fn relay_url(&self) -> &str {
        self.inner.relay_url()
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        self.inner.verified_auth_tag_owner_hex()
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Refused(
            "an inventory read cache is not a signing scope".into(),
        ))
    }
    fn relay_identity(&self) -> impl std::future::Future<Output = Result<String, Error>> + Send {
        self.inner.relay_identity()
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        let key = filter.to_string();
        let held = self
            .reads
            .lock()
            .map_err(|_| Error::Other("inventory read cache unavailable".into()))?
            .get(&key)
            .cloned();
        if let Some(rows) = held {
            return Ok(rows);
        }
        let rows = self.inner.query_all(filter).await?;
        self.reads
            .lock()
            .map_err(|_| Error::Other("inventory read cache unavailable".into()))?
            .insert(key, rows.clone());
        Ok(rows)
    }
    async fn submit_event(&self, _: nostr::Event) -> Result<Value, Error> {
        Err(Error::Refused(
            "an inventory read cache never writes".into(),
        ))
    }
}

#[cfg(test)]
#[path = "read_cache_tests.rs"]
mod tests;
