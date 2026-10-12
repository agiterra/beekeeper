//! One headless management implementation shared by CLI and desktop adapters.
use nostr::{Event, EventBuilder, Keys};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// Explicit caller journal and host scope; adapters resolve environment outside this crate.
#[derive(Clone, Debug)]
pub struct Context {
    /// Restricted caller journal/service home.
    pub home: PathBuf,
    /// Explicit installed instance.
    pub instance: beekeeper_host_core::layout::Instance,
    /// Operating-system home excluded as an execution workspace.
    pub native_home: Option<PathBuf>,
}
/// Management refusals retain their semantic CLI/client distinction.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid request.
    #[error("{0}")]
    Usage(String),
    /// Unauthorized signer/scope.
    #[error("{0}")]
    Auth(String),
    /// Immutable operation or revision conflict.
    #[error("{0}")]
    Conflict(String),
    /// Missing requested identity.
    #[error("{0}")]
    NotFound(String),
    /// Executor or relay refused the operation.
    #[error("{0}")]
    Refused(String),
    /// Accepted operation lacks terminal confirmation; retry exact operation.
    #[error("{0}")]
    Unconfirmed(String),
    /// Relay returned a non-success HTTP status; adapters preserve its existing retry classification.
    #[error("relay error {status}: {body}")]
    Relay {
        /// Exact HTTP response status, not an inferred delivery outcome.
        status: u16,
        /// Existing relay response message.
        body: String,
    },
    /// Transport/storage unavailable.
    #[error("{0}")]
    Other(String),
}
/// Existing authenticated event transport. No subprocess, Tauri or owner-key environment.
#[allow(async_fn_in_trait)]
pub trait RelayTransport: Sized + Send + Sync {
    /// Scoped transport signing identity; never persisted as an owner secret.
    fn keys(&self) -> &Keys;
    /// Relay HTTP or WebSocket URL.
    fn relay_url(&self) -> &str;
    /// Owner of delegated transport attestation, if present.
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error>;
    /// Build another transport scoped to this actor with its existing attestation.
    fn scoped(&self, keys: Keys, auth_tag: String) -> Result<Self, Error>;
    /// Public NIP11 relay signing identity for relay-authored membership records.
    fn relay_identity(&self) -> impl std::future::Future<Output = Result<String, Error>> + Send {
        async { Err(Error::Refused("relay signing identity unavailable".into())) }
    }
    /// Query explicit kinds/authority/community filters, preserving signatures.
    fn query_all(
        &self,
        filter: Value,
    ) -> impl std::future::Future<Output = Result<Vec<Value>, Error>> + Send;
    /// Submit an exact signed event through existing generic event ingress.
    fn submit_event(
        &self,
        event: Event,
    ) -> impl std::future::Future<Output = Result<Value, Error>> + Send;
    /// Sign without injecting ambient delegated authentication into public content.
    fn sign_event_unchecked(&self, builder: EventBuilder) -> Result<Event, Error> {
        builder
            .sign_with_keys(self.keys())
            .map_err(|_| Error::Other("event signing failed".into()))
    }
}
/// Existing owner-attested scoped actor custody supplied by a custody adapter.
/// This type deliberately does not implement Debug or serialization.
pub struct IdentityInput {
    /// Scoped actor key, never owner signing key.
    pub keys: Keys,
    /// Existing verified owner attestation.
    pub auth_tag: String,
}
/// Executor-private preserved configuration. Never a public record projection.
#[derive(Clone, Serialize, Deserialize)]
pub struct PrivateConfiguration {
    /// Complete existing private record, with key material supplied separately.
    pub record: Value,
}
/// Verified executor-private configuration readback; never a public projection.
#[derive(Clone)]
pub struct PrivateConfigurationSnapshot {
    /// SHA256 of the exact protected snapshot bytes, for conditional local writes.
    pub snapshot_hash: String,
    /// Exact owner-signed managed definition bound by this snapshot.
    pub definition: nostr::Event,
    /// Independently verified enrollment definition; a differing ID identifies
    /// interrupted private materialization, not a canonical completed snapshot.
    pub enrollment_definition: nostr::Event,
    /// Complete preserved private settings, with identity custody kept separately.
    pub private: PrivateConfiguration,
}
impl std::fmt::Debug for PrivateConfigurationSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PrivateConfigurationSnapshot(<redacted>)")
    }
}
pub mod receipts;
mod support;
use support::*;
mod management;
pub use management::configuration::{private_configuration_snapshot, update_if_private_unchanged};
pub use management::{authorize_project, create, update, CreateRequest, UpdateRequest};

/// Validate an ordinary workspace against explicit custody roots, without requiring Git.
pub fn validate_workspace(
    cwd: &std::path::Path,
    home: &std::path::Path,
    state: &std::path::Path,
    native_home: Option<&std::path::Path>,
) -> Result<(), Error> {
    management::local::validate_checkout(cwd, home, state, native_home)
}

mod remote_start;
pub use remote_start::{observe_remote_start, restart_remote, start_remote, RemoteStartRequest};

mod lifecycle;
mod lifecycle_retry;
mod lifecycle_uncertain;
pub use lifecycle::{delete, restart, start, status, stop, DeleteRequest, LifecycleRequest};

#[cfg(test)]
mod test_transport;

impl std::fmt::Debug for PrivateConfiguration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PrivateConfiguration(<redacted>)")
    }
}
