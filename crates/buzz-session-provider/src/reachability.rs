//! Bounded, best-effort relay reachability check for one observed commit.
//!
//! Complements [`crate::git_probe`]: where that module answers "what does the
//! local worktree say," this one answers "does the relay's git storage
//! already have it" — the other half of D4a's honesty rule that a locally
//! observed `HEAD` proves nothing about recoverability on its own.
//!
//! # Honesty contract
//!
//! [`check`] returns [`None`] for every failure mode — no repository
//! coordinate, no observed commit, a network error, an auth failure, a
//! malformed relay response, a timeout — and `None` means exactly one thing
//! everywhere it is read: *not checked*. It is never coerced into "confirmed
//! not reachable." Only [`RestClient::git_object_reachable`] returning
//! `Ok(false)` — the relay's ref advertisement was read successfully and the
//! commit was not among the refs — produces that claim, via [`Some`] with
//! `reachable: false`.

use std::time::Duration;

use buzz_acp::relay::RestClient;

use crate::state::now_secs;

/// Ceiling on one reachability check, from the moment the request is issued.
///
/// Set comfortably above the shared HTTP client's own per-request timeout
/// (10s, configured in [`buzz_acp::relay::HarnessRelay::connect`]) so this
/// bound is a backstop rather than the primary one — a connection-stage stall
/// the client's own timeout does not obviously cover (DNS, TLS) still has to
/// resolve in bounded time.
const REACHABILITY_TIMEOUT: Duration = Duration::from_secs(15);

/// One relay-confirmed answer about a specific observed commit.
///
/// Constructed only on a definitive answer — see the module's honesty
/// contract — so `reachable` and `verified_at` always travel together. There
/// is no "checked, but the outcome or the time is unknown" state: a
/// [`ReachabilityFact`] exists exactly when a check ran to completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachabilityFact {
    /// Whether the relay's advertised refs included the observed commit.
    pub reachable: bool,
    /// Unix seconds when this confirmation was obtained.
    pub verified_at: i64,
}

/// Ask the relay whether `oid` is present in `repo_ref`'s git storage.
///
/// `None` on any of: a network or auth failure, a malformed `repo_ref` or
/// `oid` the relay client could not turn into a request, a response the
/// relay client could not parse, or the check exceeding
/// [`REACHABILITY_TIMEOUT`]. Every one of those means "not checked" — the
/// caller must never read `None` as "confirmed absent."
pub(crate) async fn check(
    rest_client: &RestClient,
    repo_ref: &str,
    oid: &str,
) -> Option<ReachabilityFact> {
    let outcome = tokio::time::timeout(
        REACHABILITY_TIMEOUT,
        rest_client.git_object_reachable(repo_ref, oid),
    )
    .await;
    let reachable = match outcome {
        Ok(Ok(reachable)) => reachable,
        Ok(Err(error)) => {
            tracing::debug!(
                target: "csp::git",
                %repo_ref,
                %oid,
                %error,
                "reachability check could not be completed"
            );
            return None;
        }
        Err(_) => {
            tracing::warn!(
                target: "csp::git",
                %repo_ref,
                %oid,
                "reachability check timed out after {REACHABILITY_TIMEOUT:?}"
            );
            return None;
        }
    };
    Some(ReachabilityFact {
        reachable,
        verified_at: i64::try_from(now_secs()).unwrap_or(i64::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A [`RestClient`] pointed at a closed local port: every call fails
    /// fast with a connection error, exercising the `Ok(Err(_))` arm without
    /// a real server.
    fn unreachable_rest_client() -> RestClient {
        RestClient {
            http: reqwest::Client::new(),
            base_url: "http://127.0.0.1:1".to_owned(),
            keys: nostr::Keys::generate(),
            auth_tag_json: None,
        }
    }

    #[tokio::test]
    async fn a_network_failure_reports_not_checked() {
        let client = unreachable_rest_client();
        let repo_ref = format!("30617:{}:my-repo", "a".repeat(64));
        let result = check(&client, &repo_ref, &"b".repeat(40)).await;
        assert!(
            result.is_none(),
            "a connection failure must read as not checked, not as confirmed absent"
        );
    }

    #[tokio::test]
    async fn a_malformed_coordinate_reports_not_checked() {
        let client = unreachable_rest_client();
        let result = check(&client, "not-a-coordinate", &"b".repeat(40)).await;
        assert!(result.is_none());
    }
}
