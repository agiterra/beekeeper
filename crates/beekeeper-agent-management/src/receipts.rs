//! Existing lifecycle receipt verification and bounded waits, shared unchanged.
use crate::RelayTransport;
use beekeeper_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, ReceiptStatus,
};
use beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use beekeeper_sdk::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
};
use serde_json::{json, Value};
/// Verified create reconciliation outcome; absence is not acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateWaitOutcome {
    /// The exact provider signed one accepted target.
    Confirmed {
        /// Accepted lifecycle status.
        status: ReceiptStatus,
        /// Exact signed provider receipt ID.
        receipt_event_id: String,
        /// Accepted responsibility generation.
        target: beekeeper_core::coding_session_command::CodingSessionTarget,
        /// Initial-turn failure, when explicitly reported.
        error: Option<beekeeper_core::coding_session_payload::ReceiptError>,
    },
    /// The provider explicitly refused the operation.
    Refused {
        /// Exact signed refusal receipt ID.
        receipt_event_id: String,
        /// Named provider refusal.
        error: beekeeper_core::coding_session_payload::ReceiptError,
    },
    /// No answer before the bounded deadline.
    Unconfirmed {
        /// Whether relay queries failed during observation.
        query_failed: bool,
    },
    /// Signed answers contradict; never resolve by timestamp.
    Conflicted {
        /// Public contradiction description.
        detail: String,
    },
}

fn same_create_answer(left: &CreateWaitOutcome, right: &CreateWaitOutcome) -> bool {
    match (left, right) {
        (
            CreateWaitOutcome::Confirmed {
                status: left_status,
                target: left_target,
                error: left_error,
                ..
            },
            CreateWaitOutcome::Confirmed {
                status: right_status,
                target: right_target,
                error: right_error,
                ..
            },
        ) => {
            left_status == right_status && left_target == right_target && left_error == right_error
        }
        (
            CreateWaitOutcome::Refused {
                error: left_error, ..
            },
            CreateWaitOutcome::Refused {
                error: right_error, ..
            },
        ) => left_error == right_error,
        _ => false,
    }
}

/// Every lifecycle receipt in `events` that `provider_authority` signed for
/// exactly `command_id` in `channel_id`, with its event id.
///
/// The one place a 44224 answer to a CLI-published lifecycle command is
/// admitted: the signature verifies, the signer is the named provider, the
/// envelope carries exactly the four tags the SDK builder writes, and the
/// content decodes through `buzz-core`'s receipt decoder naming the same
/// command. Anything else — an unrelated command, a wrong signer, a malformed
/// event — is skipped, never guessed at.
pub fn verified_command_receipts(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    provider_authority: &str,
) -> Vec<(
    String,
    beekeeper_core::coding_session_payload::LifecycleReceipt,
)> {
    let expected_key = coding_session_lifecycle_receipt_semantic_key(command_id);
    let expected = [
        ["h", channel_id],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", command_id],
        ["csl-key", expected_key.as_str()],
    ];
    let mut admitted = Vec::new();
    for raw in events {
        let Ok(event) = serde_json::from_value::<nostr::Event>(raw.clone()) else {
            continue;
        };
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_LIFECYCLE_RECEIPT
            || !event
                .pubkey
                .to_hex()
                .eq_ignore_ascii_case(provider_authority)
        {
            continue;
        }
        if beekeeper_core::verify_event(&event).is_err() {
            continue;
        }
        if event.tags.len() != expected.len()
            || event.tags.iter().zip(expected).any(|(got, want)| {
                got.as_slice().len() != 2
                    || got.as_slice()[0] != want[0]
                    || got.as_slice()[1] != want[1]
            })
        {
            continue;
        }
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(&event.content) else {
            continue;
        };
        if receipt.command_id != command_id {
            continue;
        }
        admitted.push((event.id.to_hex(), receipt));
    }
    admitted
}

/// Select the provider's signed answer to one create. Unrelated commands,
/// wrong signers, malformed events, and turn receipts are ignored. Two
/// different answers are a contradiction and never get resolved by time.
pub fn classify_create_receipts(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    provider_authority: &str,
) -> Result<Option<CreateWaitOutcome>, String> {
    let mut answer: Option<CreateWaitOutcome> = None;
    for (receipt_event_id, receipt) in
        verified_command_receipts(events, channel_id, command_id, provider_authority)
    {
        let candidate = match receipt.status {
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn => {
                let Some(target) = receipt.session.clone() else {
                    continue;
                };
                CreateWaitOutcome::Confirmed {
                    status: receipt.status,
                    receipt_event_id,
                    target,
                    error: receipt.error.clone(),
                }
            }
            ReceiptStatus::Failed => {
                let Some(error) = receipt.error.clone() else {
                    continue;
                };
                CreateWaitOutcome::Refused {
                    receipt_event_id,
                    error,
                }
            }
            _ => continue,
        };
        if let Some(existing) = &answer {
            if !same_create_answer(existing, &candidate) {
                return Err(format!(
                    "provider published conflicting lifecycle receipts for create {command_id}"
                ));
            }
        } else {
            answer = Some(candidate);
        }
    }
    Ok(answer)
}

/// How a wait for one lifecycle command's receipt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptWait<T> {
    /// The classifier selected an answer.
    Answered(T),
    /// The provider signed two answers that disagree.
    Conflicted(String),
    /// Nothing answered before the deadline. `query_failed` says whether any
    /// receipt query failed or was cut off, so "no receipt" is never reported
    /// over a relay that could not be asked.
    Unconfirmed { query_failed: bool },
}

/// Poll the exact receipt query for `command_id` until `classify` selects an
/// answer or `timeout_secs` passes. Publishes nothing and never retries the
/// command itself.
pub async fn await_command_receipt<T>(
    client: &impl RelayTransport,
    channel_id: &str,
    command_id: &str,
    timeout_secs: u64,
    classify: impl Fn(&[Value]) -> Result<Option<T>, String>,
) -> ReceiptWait<T> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut query_failed = false;
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "#h": [channel_id],
        "#csl-command": [command_id],
    });
    loop {
        let query = tokio::time::timeout_at(deadline, client.query_all(filter.clone())).await;
        match query {
            Ok(Ok(events)) => match classify(&events) {
                Ok(Some(answer)) => return ReceiptWait::Answered(answer),
                Ok(None) => {}
                Err(detail) => return ReceiptWait::Conflicted(detail),
            },
            Ok(Err(_)) => query_failed = true,
            Err(_) => return ReceiptWait::Unconfirmed { query_failed: true },
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return ReceiptWait::Unconfirmed { query_failed };
        }
        let next_poll = std::cmp::min(deadline, now + std::time::Duration::from_millis(500));
        tokio::time::sleep_until(next_poll).await;
    }
}

/// Await one exact provider create outcome without republishing the command.
pub async fn await_create_receipt(
    client: &impl RelayTransport,
    channel_id: &str,
    command_id: &str,
    provider_authority: &str,
    timeout_secs: u64,
) -> CreateWaitOutcome {
    let wait = await_command_receipt(client, channel_id, command_id, timeout_secs, |events| {
        classify_create_receipts(events, channel_id, command_id, provider_authority)
    })
    .await;
    match wait {
        ReceiptWait::Answered(outcome) => outcome,
        ReceiptWait::Conflicted(detail) => CreateWaitOutcome::Conflicted { detail },
        ReceiptWait::Unconfirmed { query_failed } => {
            CreateWaitOutcome::Unconfirmed { query_failed }
        }
    }
}
