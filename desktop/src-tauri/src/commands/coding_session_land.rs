//! The rule that decides whether a commit may land on a verdict-gated ref —
//! asked from the founder's own screen, and answered by the push path's code.
//!
//! Live run 3 (2026-09-02) ended with a verifier's FAIL on the wire and the
//! branch on `main` anyway. Lane L6 makes the relay's pre-receive hook read the
//! mission's ruling; this boundary makes the app read **the same rule from the
//! same function**, so the screen, `bee git check --ref` and the hook cannot
//! give three answers.
//!
//! # What this command does not do
//!
//! It does not push, and that is a ruling rather than an omission (§1l, and
//! LANE-L8's addendum 4). Three reasons, each disqualifying alone: the push is
//! irreversible and mutates a ref other people build on; the app holds no
//! working tree and cannot know which checkout or worktree the founder means,
//! and the main checkout is hot; and `git-credential-nostr` is configured in
//! the founder's shell, not in the Tauri process, so a push from here would
//! fail on NIP-98 or — worse — succeed under a different identity. The command
//! returns a verdict and a **string to copy**; nothing in this file, and
//! nothing in this lane, invokes git.
//!
//! # The rule is `buzz-core`'s, not this file's
//!
//! The predicate is
//! [`buzz_core_pkg::coding_session_verdict_admission::evaluate_verdict_admission`]
//! and the `require-verdict` flag is read through `buzz-core`'s own
//! `parse_protection_tags` + `EffectiveRules::for_ref`. This file held verbatim
//! copies of both while lane L6 was unlanded; the finalizer deleted them. There
//! is one rule, in one place, and the screen, `bee git check --ref` and the
//! pre-receive hook read it from there.

use buzz_core_pkg::coding_session_team_transaction::{
    validate_coding_session_team_transaction_envelope, CodingSessionTeamTransactionBody,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

use buzz_core_pkg::coding_session_verdict_admission::{
    evaluate_verdict_admission, VerdictAdmission, VerdictAdmissionCandidate, VerdictAdmissionQuery,
    VerdictAdmissionRecord, VerdictAdmissionRefusal, VerdictAdmissionRules,
};
use buzz_core_pkg::git_perms::{parse_protection_tags, EffectiveRules};
use buzz_core_pkg::repository_founders::RepositoryFounders;

/// Closed wire-schema identifier accepted by this boundary.
pub const CODING_SESSION_LAND_REQUEST_SCHEMA: &str = "buzz-coding-session-land-request/v1";
/// Closed wire-schema identifier this native adapter answers with.
pub const CODING_SESSION_LAND_ADAPTER_SCHEMA: &str = "buzz-coding-session-land-adapter/v1";

/// The mission the caller already folded, and the repository it would land on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionLandRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// Full ref name the push would update, e.g. `refs/heads/main`.
    pub ref_name: String,
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Immutable session-genesis event id.
    pub genesis_ref: String,
    /// Pubkey of the session-genesis signer.
    pub founder_pubkey: String,
    /// Pubkey of the kind:30617 announcement's author, or null when this
    /// surface holds no repository record at all.
    pub repo_owner_pubkey: Option<String>,
    /// The key that would run the push — the viewer's.
    pub pusher_pubkey: String,
    /// The repository's whole kind:30617 tag list, or null when unknown.
    ///
    /// Null is **not** "no rule": it is "no record reached this view", and the
    /// two produce different answers.
    pub protection_tags: Option<Vec<Vec<String>>>,
    /// Pubkeys the repository's project roster grants Owner, or null when
    /// this view could not read the roster.
    ///
    /// Finding 33: a repository's founders are its announcement's signer, its
    /// NIP-34 `maintainers`, **and** every Owner on the project roster its
    /// `["project", …]` back-reference names (commit `a56ad5d01`). The first
    /// two are on `protection_tags`; only this one needs a second read, and
    /// null is "not read", never "there are none" — the answer says which.
    pub project_owner_pubkeys: Option<Vec<String>>,
    /// Event ids the caller's fold marked canonical, in included order.
    pub included_event_ids: Vec<String>,
    /// Raw signed kind-44244 Nostr events. No projected outputs are accepted.
    pub events: Vec<serde_json::Value>,
}

/// What admitted a commit, for the confirm step's first sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandEvidence {
    /// Umbrella whose fold admitted it.
    pub session_ref: String,
    /// The approving disposition.
    pub disposition_event_id: String,
    /// Who signed that disposition.
    pub disposition_author_pubkey: String,
    /// The report it governs.
    pub report_event_id: String,
    /// That report's `headSha`, as published.
    pub head_sha: String,
}

/// The newest canonical verdict this mission holds, admitting or not.
///
/// Present even when the rule does not govern the repository: §1l's
/// no-rule sentence still says what the mission ruled, because "nothing gates
/// this push" and "nobody has ruled" are different facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandNewestVerdict {
    /// The verdict event id.
    pub event_id: String,
    /// Who signed it.
    pub author_pubkey: String,
    /// Its signed decision word, e.g. `approve` or `changes-requested`.
    pub decision: String,
    /// The report it governs.
    pub report_event_id: String,
    /// That report's `headSha`, when the report named one.
    pub head_sha: Option<String>,
}

/// The answer, for one mission and one ref.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandResponse {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose rule produced this, never "desktop".
    pub implementation: String,
    /// Whether a repository record reached this view at all.
    pub repository_known: bool,
    /// Whether `require-verdict` governs this ref on that repository.
    pub rule_governs: bool,
    /// Whether the rule admits the push. False whenever it does not govern.
    pub admitted: bool,
    /// What admitted it, or null.
    pub evidence: Option<CodingSessionLandEvidence>,
    /// §1j's refusal sentence, verbatim, or null.
    pub refusal_reason: Option<String>,
    /// The newest canonical verdict, or null when the mission holds none.
    pub newest_verdict: Option<CodingSessionLandNewestVerdict>,
    /// Every founder of the repository, lower-hex, signer first.
    pub founders: Vec<String>,
    /// The sentence naming who may rewrite the rules, who the founders are,
    /// and whether the project roster was readable from here.
    pub founders_note: String,
    /// Whether the viewer's own key is one of them.
    pub viewer_is_founder: bool,
    /// The announcement's signer — the one key that may rewrite the rules in
    /// v1 — or null when no announcement reached this view.
    ///
    /// Carried as its own field so the screen can render a *short* founder
    /// line (display names, 8-hex) without re-parsing the prose sentence.
    /// The rule stays `buzz-core`'s; only the rendering is the screen's.
    pub rules_signer: Option<String>,
    /// Whether the project roster was read. `false` is "not read", and the
    /// screen must say so rather than present a partial set as whole.
    pub roster_read: bool,
    /// The exact command a person runs, or null when nothing is admitted.
    ///
    /// It names the **commit**, never the branch: a branch name is not a
    /// commit, and two pushes to one branch are two commits of which one was
    /// ruled on.
    pub command: Option<String>,
}

/// What the founder line says when no repository record reached this view.
///
/// The empty set's own sentence would read as a claim about a repository
/// nobody here has seen — "founders are none" is a fact about the repository,
/// and this is a fact about the read.
pub const NO_REPOSITORY_FOUNDERS_NOTE: &str =
    "No repository record reached this view, so nothing here can name its founders.";

/// Whether `require-verdict` covers this ref.
///
/// Reads the **flag** through `buzz-core`'s own parser and rule union — never
/// through the parser's *unknown* list. Reading the unknown list was fix round
/// 1's shape and it fails open into silence: core parses the token, so it
/// leaves `unknown`, and this control would tell a founder the repository has
/// no rule when it has one (REVIEW-L8 F3).
///
/// A tag list core refuses outright yields no rules and therefore no flag,
/// which is the same answer the relay's gate gives it.
fn ref_requires_verdict(tags: &[Vec<String>], ref_name: &str) -> bool {
    let Ok(parsed) = parse_protection_tags(tags) else {
        return false;
    };
    EffectiveRules::for_ref(ref_name, &parsed.rules).require_verdict
}

/// Decode the caller's canonical events, in the fold's own included order.
///
/// Every event is verified here: a record read off an unverified event would
/// be a claim about who ruled, made by nobody. An event that fails to verify
/// or decode is skipped — the caller's fold already accepted the included
/// set, so this can only drop something the caller should not have sent.
fn canonical_records(
    request: &CodingSessionLandRequest,
) -> Result<Vec<VerdictAdmissionRecord>, String> {
    let mut events: Vec<Event> = Vec::with_capacity(request.events.len());
    for value in &request.events {
        let event: Event = serde_json::from_value(value.clone())
            .map_err(|error| format!("events[] is not a signed Nostr event: {error}"))?;
        event
            .verify()
            .map_err(|error| format!("event signature is invalid: {error}"))?;
        events.push(event);
    }
    Ok(request
        .included_event_ids
        .iter()
        .filter_map(|id| {
            let event = events.iter().find(|event| &event.id.to_hex() == id)?;
            let payload = validate_coding_session_team_transaction_envelope(event).ok()?;
            Some(VerdictAdmissionRecord {
                event_id: id.clone(),
                author_pubkey: event.pubkey.to_hex(),
                payload,
            })
        })
        .collect())
}

/// The newest canonical verdict of either subtype, with the report it governs.
fn newest_verdict(records: &[VerdictAdmissionRecord]) -> Option<CodingSessionLandNewestVerdict> {
    let head_sha_of = |report_ref: &str| -> Option<String> {
        records
            .iter()
            .find_map(|record| match &record.payload.body {
                CodingSessionTeamTransactionBody::Report(report)
                    if record.event_id == report_ref =>
                {
                    report.head_sha.clone()
                }
                _ => None,
            })
    };
    records.iter().rev().find_map(|record| {
        let CodingSessionTeamTransactionBody::Verdict(verdict) = &record.payload.body else {
            return None;
        };
        let (report_ref, decision) = match verdict {
            buzz_core_pkg::coding_session_team_transaction::CodingSessionTeamVerdict::Disposition {
                report_ref,
                decision,
                ..
            } => (report_ref.clone(), serde_json::to_value(decision).ok()?),
            buzz_core_pkg::coding_session_team_transaction::CodingSessionTeamVerdict::Refutation {
                report_ref,
                decision,
                ..
            } => (report_ref.clone(), serde_json::to_value(decision).ok()?),
        };
        Some(CodingSessionLandNewestVerdict {
            event_id: record.event_id.clone(),
            author_pubkey: record.author_pubkey.clone(),
            decision: decision.as_str()?.to_owned(),
            head_sha: head_sha_of(&report_ref),
            report_event_id: report_ref,
        })
    })
}

fn land_adapter(request: CodingSessionLandRequest) -> Result<CodingSessionLandResponse, String> {
    if request.schema != CODING_SESSION_LAND_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_LAND_REQUEST_SCHEMA}"
        ));
    }
    let records = canonical_records(&request)?;
    let newest = newest_verdict(&records);
    // REVIEW-L8 F13: "known" is "a repository record reached this rule", which
    // is exactly `protection_tags`. Requiring the owner too made
    // `RepositoryUnbound` unreachable — TypeScript tests `repositoryKnown`
    // first, so that refusal was swallowed by the unknown sentence. Tags
    // without an owner is now a *governed* repository bound to no channel,
    // which is the case §1j's fourth string was written for.
    let repository_known = request.protection_tags.is_some();
    let rule_governs = request
        .protection_tags
        .as_ref()
        .is_some_and(|tags| ref_requires_verdict(tags, &request.ref_name));
    // The founder set, composed by `buzz-core` from what this view read: the
    // signer and `maintainers` off the announcement's own tags, plus the
    // roster owners the caller resolved. A view with no announcement has no
    // founders to name, and says so rather than naming the viewer.
    let founders = match (&request.repo_owner_pubkey, &request.protection_tags) {
        (Some(signer), Some(tags)) => {
            let founders = RepositoryFounders::from_parts(signer, tags);
            match &request.project_owner_pubkeys {
                Some(owners) => founders.with_roster_owners(owners.clone()),
                None => founders,
            }
        }
        _ => RepositoryFounders::from_parts("", &[]),
    };
    // With no announcement there is no founder set to name, and the empty
    // set's own sentence would read as a claim about a repository this view
    // never saw. Say what actually happened instead.
    let founders_note = if repository_known && request.repo_owner_pubkey.is_some() {
        founders.rules_sentence()
    } else {
        NO_REPOSITORY_FOUNDERS_NOTE.to_owned()
    };
    let base = CodingSessionLandResponse {
        schema: CODING_SESSION_LAND_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        repository_known,
        rule_governs,
        admitted: false,
        evidence: None,
        refusal_reason: None,
        newest_verdict: newest,
        founders: founders.pubkeys().to_vec(),
        founders_note,
        viewer_is_founder: repository_known && founders.contains(&request.pusher_pubkey),
        rules_signer: if repository_known {
            request.repo_owner_pubkey.clone()
        } else {
            None
        },
        roster_read: founders.roster_owners_read().is_some(),
        command: None,
    };
    if !rule_governs {
        // Not a refusal: the rule simply does not govern this ref. The caller
        // renders §1l's own sentence for that case and still shows the verdict
        // it read, because "nothing gates this" and "nobody ruled" differ.
        return Ok(base);
    }
    let Some(_repo_owner) = request.repo_owner_pubkey.clone() else {
        return Ok(CodingSessionLandResponse {
            refusal_reason: Some(VerdictAdmissionRefusal::RepositoryUnbound.reason()),
            ..base
        });
    };
    let disposition_author = |event_id: &str| -> String {
        records
            .iter()
            .find(|record| record.event_id == event_id)
            .map(|record| record.author_pubkey.clone())
            .unwrap_or_default()
    };
    let candidate = VerdictAdmissionCandidate {
        session_ref: request.session_ref.clone(),
        genesis_ref: request.genesis_ref.clone(),
        founder_pubkey: request.founder_pubkey.clone(),
        canonical: records.clone(),
        active_seat_pubkeys: Vec::new(),
    };
    // Batch 3's ruling, and L6's own default: founder-signed ruling, landed by
    // the founder. If Brian relaxes either, L6 flips the field and this screen
    // follows through the shared predicate with no edit here.
    let rules = VerdictAdmissionRules::FOUNDER_ONLY;
    let query = VerdictAdmissionQuery {
        ref_name: &request.ref_name,
        new_oid: newest_head_sha(&records).unwrap_or_default(),
        pusher_pubkey: &request.pusher_pubkey,
        repo_founders: founders.pubkeys(),
    };
    match evaluate_verdict_admission(std::slice::from_ref(&candidate), &query, &rules) {
        VerdictAdmission::Admitted(evidence) => Ok(CodingSessionLandResponse {
            admitted: true,
            command: Some(format!(
                "git push origin {}:{}",
                evidence.head_sha, request.ref_name
            )),
            evidence: Some(CodingSessionLandEvidence {
                session_ref: evidence.session_ref,
                disposition_author_pubkey: disposition_author(&evidence.disposition_event_id),
                disposition_event_id: evidence.disposition_event_id,
                report_event_id: evidence.report_event_id,
                head_sha: evidence.head_sha,
            }),
            ..base
        }),
        VerdictAdmission::Refused(refusal) => Ok(CodingSessionLandResponse {
            refusal_reason: Some(refusal.reason()),
            ..base
        }),
    }
}

/// The commit this mission would land: the newest canonical report's `headSha`.
///
/// The predicate compares a *proposed* oid against every approving ruling. A
/// screen has no push in flight, so the oid under test is the newest thing the
/// mission actually produced — and when no report named one, there is nothing
/// to test and §1j's branch-only or no-verdict string is the honest answer.
fn newest_head_sha(records: &[VerdictAdmissionRecord]) -> Option<&str> {
    records
        .iter()
        .rev()
        .find_map(|record| match &record.payload.body {
            CodingSessionTeamTransactionBody::Report(report) => report.head_sha.as_deref(),
            _ => None,
        })
}

/// Ask the push path's own rule whether this mission's commit may land.
#[tauri::command]
pub async fn coding_session_land(
    request: CodingSessionLandRequest,
) -> Result<CodingSessionLandResponse, String> {
    tauri::async_runtime::spawn_blocking(move || land_adapter(request))
        .await
        .map_err(|error| format!("coding-session land task failed: {error}"))?
}

#[cfg(test)]
#[path = "coding_session_land_tests.rs"]
mod tests;
