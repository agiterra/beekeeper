//! NIP-PW: ingest admission for project work records (kind 44249).
//!
//! **Structure only**, exactly as for 44244, 44245, 44246 and 44247. The
//! schema, the six ordered two-field tags, the three closed record types,
//! every bound, and the parity between `d`/`a`/`pwk-genesis`/`pwk-type` and
//! the content they restate are all answerable from this one event, so the
//! relay answers them. Whether the signer held `may_lead` in that project and
//! session is **not**: that is the consuming fold's question against the
//! accepted NIP-CSAT chain
//! ([`buzz_core::project_work_fold::fold_work`]). A relay that adjudicated it
//! here would be asserting standing it cannot verify, and would be claiming a
//! gate stronger than the one its sibling kind has.
//!
//! Membership is still checked before any of this: 44249 is a coding-session
//! kind (`is_coding_session_kind`), so it goes through the strict
//! channel-membership gate first and a non-member is refused before its
//! content is ever parsed. The `a` tag is a **selector**, not a gate: 44249
//! is deliberately not in `is_project_a_scoped_kind`, and a const assert in
//! `buzz-core`'s `kind.rs` pins that.
//!
//! No `coding_session_content_cap` entry: that table bounds storage for the
//! four provider-authored kinds that get *no* envelope validator. 44249 has
//! one, and it already refuses content over
//! `MAX_PROJECT_WORK_CONTENT_BYTES`, so a second bound here could only drift
//! from the first.

use nostr::Event;

use buzz_core::project_work::{validate_project_work_envelope, ProjectWorkEvent};

/// Validate a signed kind:44249 event's exact envelope for ingest.
///
/// Returns the refusal as `"<stable code>: <why>"`, so a rejected publish
/// names a reason a CLI and a UI can both show — an unknown `pwk-type` or an
/// unknown schema version is `record-type` or `schema`, never a bare
/// "invalid".
///
/// # Errors
///
/// Returns the first refusal the contract's validator finds.
pub fn validate_project_work_record(event: &Event) -> Result<(), String> {
    validate_project_work_envelope(&ProjectWorkEvent::from(event))
        .map(|_| ())
        .map_err(|refusal| format!("{}: {}", refusal.code, refusal.message))
}

#[cfg(test)]
#[path = "project_work_tests.rs"]
mod tests;
