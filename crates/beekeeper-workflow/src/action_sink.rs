//! Action sink trait — interface for workflow side-effects.
//!
//! The relay implements [`ActionSink`] to provide direct DB access to the
//! executor, replacing the HTTP loopback pattern.

use std::future::Future;
use std::pin::Pin;

use beekeeper_core::{ci_result::CiResult, host_step::HostStepRequested, tenant::CommunityId};

/// What the relay publishes as a kind:46010 when a run suspends on approval.
///
/// The approval row already exists and the run is already `waiting_approval`
/// when this is sent, so a client that acts on the event finds a run in the
/// state the event describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// Lowercase hex of the stored token hash: the `d` tag, and what a
    /// kind:46030 grant names.
    pub approval_ref: String,
    /// The run waiting on this approval.
    pub run_id: uuid::Uuid,
    /// The workflow definition the run executes.
    pub workflow_id: uuid::Uuid,
    /// The definition's name, for the inbox row.
    pub workflow_name: String,
    /// The gated step's id.
    pub step_id: String,
    /// The gated step's zero-based index.
    pub step_index: usize,
    /// Who may approve, in `check_approver_spec` terms.
    pub approver_spec: String,
    /// Text shown to the approver.
    pub message: String,
    /// Unix seconds after which the approval expires.
    pub expires_at: u64,
    /// The workflow's channel, canonical UUID string.
    pub channel_id: String,
    /// Hex pubkey of the workflow owner, for the `p` attribution tag.
    pub owner_pubkey_hex: String,
    /// True when the engine inserted this gate itself before a `run_on_host`
    /// step (spec § 5.4), false for an authored `request_approval` step.
    pub synthetic: bool,
}

/// Errors from action sink operations.
#[derive(Debug, thiserror::Error)]
pub enum ActionSinkError {
    /// An input parameter is malformed (e.g. invalid UUID).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The target channel does not exist.
    #[error("channel not found: {0}")]
    ChannelNotFound(String),
    /// The target channel is archived.
    #[error("channel is archived: {0}")]
    ChannelArchived(String),
    /// Nostr event construction or signing failed.
    #[error("event construction failed: {0}")]
    EventBuild(String),
    /// A database operation failed.
    #[error("database error: {0}")]
    Database(String),
    /// Message content is empty or whitespace-only.
    #[error("empty message content")]
    EmptyContent,
    /// The same CI identity was already accepted with different canonical content.
    #[error("CI result conflict: {0}")]
    CiResultConflict(String),
    /// The workflow owner no longer has authority over the configured repository.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

impl From<ActionSinkError> for crate::WorkflowError {
    fn from(e: ActionSinkError) -> Self {
        match e {
            ActionSinkError::CiResultConflict(detail) => {
                crate::WorkflowError::CiResultConflict(detail)
            }
            ActionSinkError::Unauthorized(detail) => crate::WorkflowError::Unauthorized(detail),
            other => crate::WorkflowError::WebhookError(other.to_string()),
        }
    }
}

/// Interface for workflow actions that produce side effects.
///
/// Implemented by the relay to provide direct DB/event access to the executor.
/// This replaces the HTTP loopback where the executor POSTed to the relay's
/// REST API (which failed with 401 auth errors).
///
/// Returns `Pin<Box<dyn Future>>` for dyn-compatibility — required because
/// `WorkflowEngine` stores `Arc<dyn ActionSink>`.
pub trait ActionSink: Send + Sync {
    /// Post a message to a channel on behalf of a workflow owner.
    ///
    /// - `community_id`: the server-resolved community that owns the workflow
    ///   run driving this side effect. The relay-signed message is published
    ///   under *this* community, never the deployment/default tenant — the run
    ///   carries its owning community so a workflow in community B posts into B
    ///   even though the side effect has no inbound connection to bind.
    /// - `channel_id`: UUID string of the target channel
    /// - `text`: message body (must not be empty/whitespace-only)
    /// - `author_pubkey`: hex-encoded pubkey of the workflow owner (used for
    ///   the `p` attribution tag; the relay keypair signs the event)
    ///
    /// Returns the event ID hex string on success.
    fn send_message(
        &self,
        community_id: CommunityId,
        channel_id: &str,
        text: &str,
        author_pubkey: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, ActionSinkError>> + Send + '_>>;

    /// Persist a canonical relay-signed CI result for the run's community.
    ///
    /// Implementations must recheck the stored workflow owner's repository
    /// authority at this action boundary and atomically distinguish a new
    /// result, an exact retry, and conflicting content for the same identity.
    fn record_ci_result(
        &self,
        community_id: CommunityId,
        result: &CiResult,
    ) -> Pin<Box<dyn Future<Output = Result<String, ActionSinkError>> + Send + '_>>;

    /// Publish a relay-signed kind:46010 approval request for a run that the
    /// engine has already parked as `waiting_approval`.
    ///
    /// Returns the event id hex string on success.
    fn request_approval(
        &self,
        community_id: CommunityId,
        request: &ApprovalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<String, ActionSinkError>> + Send + '_>>;

    /// Publish a relay-signed kind:46013 host-step request for a run that the
    /// engine has already parked as `waiting_host`.
    ///
    /// Returns the event id hex string on success.
    fn request_host_step(
        &self,
        community_id: CommunityId,
        request: &HostStepRequested,
    ) -> Pin<Box<dyn Future<Output = Result<String, ActionSinkError>> + Send + '_>>;
}
