//! Adapt native transport outcomes to the legacy pool acknowledgement.

use super::{SteerAck, SteerError};
use crate::steer::{NotDeliveredReason, SteerResolution, UnknownReason};

impl SteerAck {
    /// Map what the transport established onto the legacy ack the pool's
    /// main loop keys on (`docs/NATIVE_STEERING_IMPL.md` §3.1 item 8).
    ///
    /// The read loop decodes every steer answer into one
    /// [`SteerResolution`] and this is the only place it becomes a
    /// `SteerAck`, so the two surfaces cannot drift. Legacy semantics are
    /// preserved variant for variant: a write failure stays
    /// [`SteerError::Transport`] (release + cancel+merge fallback), because
    /// that is what the main loop has always done with it.
    pub(crate) fn from_resolution(resolution: SteerResolution, session_id: &str) -> Self {
        match resolution {
            SteerResolution::Injected { .. } | SteerResolution::StartedNewTurn { .. } => {
                Self::Success {
                    session_id: session_id.to_owned(),
                }
            }
            SteerResolution::NotDelivered { reason } => match reason {
                NotDeliveredReason::Unsupported => Self::Err(SteerError::ExpectedRunIdMissing),
                NotDeliveredReason::MethodNotFound { message } => {
                    Self::Err(SteerError::AgentError {
                        code: -32601,
                        message,
                    })
                }
                NotDeliveredReason::Rejected { code, message } => {
                    Self::Err(SteerError::AgentError { code, message })
                }
                // The legacy harness never sends an idle guard, so this is
                // an outcome it does not recognize: released and re-sent
                // through cancel+merge, exactly like any other unrecognized
                // success.
                NotDeliveredReason::PromptRequired => Self::Err(SteerError::OutcomeRejected {
                    outcome: "promptRequired".to_owned(),
                }),
                NotDeliveredReason::PromptEndedBeforeWrite
                | NotDeliveredReason::DispatchPrevented { .. } => Self::PromptCompletedNeutral,
            },
            SteerResolution::Unknown { reason, .. } => match reason {
                UnknownReason::UnrecognizedAck { outcome }
                | UnknownReason::AdapterReportedFailure { outcome } => {
                    Self::Err(SteerError::OutcomeRejected { outcome })
                }
                UnknownReason::WriteFailed { message } => Self::Err(SteerError::Transport(message)),
                UnknownReason::PromptEndedBeforeAck
                | UnknownReason::AckTimeout
                | UnknownReason::RuntimeExited => Self::PromptCompletedNeutral,
            },
        }
    }
}
