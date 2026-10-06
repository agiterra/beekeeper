//! The one policy fact the 44244 fold needs: `gates.verifierRequired`.
//!
//! Split out of [`super::operations`] rather than added to it, because that
//! file is already at the repository's 1,000-line ceiling and the rule is
//! "split, never bump".
//!
//! # Why the CLI computes it and the fold does not
//!
//! A 44245 policy record and a 44244 transaction are two different kinds with
//! two different authority rules, folded by two different functions. Letting
//! the 44244 fold read policy events would have given it a second authority
//! model to get wrong. Instead the caller runs
//! [`fold_coding_session_policies`] — the *same* fold `bee sessions policy
//! get` and the session provider run, through [`super::policy::fold_policies`]
//! — and hands the 44244 fold one boolean.
//!
//! # `false` means "this fold enforces nothing extra"
//!
//! It does not mean "no verifier is required". A caller that has not read the
//! policy set must not render this `false` as a fact about the session; that
//! is the policy's fact, and `bee sessions policy get` is where a reader gets
//! it with its author, its event id and everything it refused.

use beekeeper_core::coding_session_policy::CodingSessionPolicyGrant;
use beekeeper_core::coding_session_team_transaction::CodingSessionTeamFoldContext;
use beekeeper_core::kind::KIND_CODING_SESSION_POLICY;
use nostr::Event;
use serde_json::json;

use crate::client::BuzzClient;
use crate::error::CliError;

/// This session's fold context with `verifier_required` read from the wire.
///
/// One extra relay read per 44244 fold: the umbrella's published 44245 set,
/// which is small (one record per revision) and scoped by `h`, `d` and
/// `csp-genesis` exactly as `bee sessions policy get` scopes it.
pub(super) async fn fetch_context_with_verifier_gate(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<CodingSessionTeamFoldContext, CliError> {
    let authority =
        super::operations_reads::fetch_session_authority(client, channel, session_ref, genesis)
            .await?;
    let records = fetch_policy_records(client, channel, session_ref, genesis).await?;
    let verifier_required = policy_requires_a_verifier(
        &records,
        session_ref,
        genesis,
        &authority.context.founder_pubkey,
        &authority.policy_grants,
    );
    let mut context = authority.context;
    context.verifier_required = verifier_required;
    Ok(context)
}

/// Every kind-44245 record filed under this umbrella, unjudged.
///
/// Standing, decodability and addressing are all
/// [`super::policy::fold_policies`]'s to judge, so a malformed row is dropped
/// here only when it is not an `Event` at all — exactly as `bee sessions
/// policy get` reads them.
async fn fetch_policy_records(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<Vec<Event>, CliError> {
    let values = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_POLICY],
            "#h": [channel],
            "#d": [session_ref],
            "#csp-genesis": [genesis],
        }))
        .await?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .collect())
}

/// Whether the newest accepted policy sets `gates.verifierRequired: true`.
///
/// Absent policy, absent `gates`, absent flag and `false` all answer `false`,
/// which is what makes "the fold behaves exactly as today" true for every
/// session that has not asked for a verifier.
pub(super) fn policy_requires_a_verifier(
    records: &[Event],
    session_ref: &str,
    genesis: &str,
    founder: &str,
    grants: &[CodingSessionPolicyGrant],
) -> bool {
    super::policy::fold_policies(records, session_ref, genesis, founder, grants)
        .selected
        .and_then(|selected| selected.record.gates)
        .and_then(|gates| gates.verifier_required)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "operations_verifier_gate_tests.rs"]
mod tests;
