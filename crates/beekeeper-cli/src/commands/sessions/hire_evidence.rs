//! CLI adapters for shared signed hire evidence.
use super::crew::{HiredSeat, SeatReceipt};
use crate::error::CliError;
pub(super) use beekeeper_agent_management::hire_evidence::HireEvidenceRequest;
use beekeeper_core::coding_session_command::CodingSessionTarget;
use nostr::Event;
use serde_json::Value;
pub(super) fn verified_genesis(
    events: &[Value],
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<Event, CliError> {
    beekeeper_agent_management::hire_evidence::verified_genesis(
        events,
        channel,
        session_ref,
        genesis,
    )
    .map_err(Into::into)
}
pub(super) fn create_receipt_binding(
    channel: &str,
    seat: &HiredSeat,
    receipt: &SeatReceipt,
) -> Result<Option<CodingSessionTarget>, CliError> {
    beekeeper_agent_management::hire_evidence::create_receipt_binding(channel, seat, receipt)
        .map_err(Into::into)
}
pub(super) fn verify_hire_evidence(
    events: &[Value],
    request: &HireEvidenceRequest<'_>,
    seat: &HiredSeat,
    receipt: &SeatReceipt,
) -> Result<(), CliError> {
    beekeeper_agent_management::hire_evidence::verify_hire_evidence(events, request, seat, receipt)
        .map_err(Into::into)
}
