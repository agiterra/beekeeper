//! Canonical report extraction and fail-closed turn ordering.

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamFoldContext,
    CodingSessionTeamTransactionBody,
};
use nostr::Event;

/// One canonical included report and the causal assignment it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludedReport {
    pub event_id: String,
    pub assignment_ref: String,
    pub author_pubkey: String,
    pub created_at: u64,
}

/// Validate the complete transaction graph and return included reports.
pub fn included_reports(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
) -> Result<Vec<IncludedReport>, String> {
    let fold = fold_coding_session_team_transactions(events, context)?;
    let included: std::collections::HashSet<&str> =
        fold.included_event_ids.iter().map(String::as_str).collect();
    let mut reports = Vec::new();
    for event in events {
        let id = event.id.to_hex();
        if !included.contains(id.as_str()) {
            continue;
        }
        let payload = buzz_core::coding_session_team_transaction::validate_coding_session_team_transaction_envelope(event)?;
        if let CodingSessionTeamTransactionBody::Report(report) = payload.body {
            reports.push(IncludedReport {
                event_id: id,
                assignment_ref: report.assignment_ref,
                author_pubkey: event.pubkey.to_hex(),
                created_at: event.created_at.as_secs(),
            });
        }
    }
    Ok(reports)
}

/// Suppress a missing-report diagnostic only when signed-second report time
/// is wholly inside the provider's millisecond turn window.
///
/// A Nostr timestamp denotes only its complete one-second interval. If either
/// boundary intersects that interval, ordering is unknowable and this fails
/// closed: the report wake and explicit diagnostic are safer than silently
/// losing the terminal fact.
pub fn report_suppresses_terminal(
    reports: &[IncludedReport],
    assignment_ref: &str,
    actor: &str,
    prompt_at_ms: Option<i64>,
    terminal_at_ms: i64,
) -> bool {
    let Some(prompt_at_ms) = prompt_at_ms else {
        return false;
    };
    reports.iter().any(|report| {
        let earliest_created_at_ms = i64::try_from(report.created_at)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1_000));
        let latest_created_at_ms =
            earliest_created_at_ms.and_then(|earliest| earliest.checked_add(999));
        report.assignment_ref == assignment_ref
            && report.author_pubkey == actor
            && earliest_created_at_ms.is_some_and(|created| created >= prompt_at_ms)
            && latest_created_at_ms.is_some_and(|created| created <= terminal_at_ms)
    })
}
