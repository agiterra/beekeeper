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
    validate_coding_session_team_transaction_envelope, CodingSessionTeamActiveSeat,
    CodingSessionTeamTransactionBody,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

use buzz_core_pkg::coding_session_observation::{
    CodingSessionObservationGateEntry, CodingSessionObservationGateOutcome,
    CodingSessionObservationGateRow, CodingSessionObservationSource,
};
use buzz_core_pkg::coding_session_verdict_admission::{
    evaluate_verdict_admission, GatePolicyResolution, VerdictAdmission, VerdictAdmissionCandidate,
    VerdictAdmissionCandidateSource, VerdictAdmissionEvidence, VerdictAdmissionGatePolicy,
    VerdictAdmissionQuery, VerdictAdmissionRecord, VerdictAdmissionRefusal,
};
use buzz_core_pkg::git_perms::EffectiveRules;
use buzz_core_pkg::repository_founders::RepositoryFounders;
use buzz_core_pkg::repository_protection::{
    decode_repository_protection, resolve_protection_layers, ProtectionLayer,
};

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
    /// Signed kind:30625 rule records for this repository, as this view read
    /// them, or null when it read none.
    ///
    /// Lane L26: any founder may set or remove a rule with a record of their
    /// own, so the announcement's tags are no longer the whole of the rules.
    /// **Read-optional**: absent or empty means "no record reached this view",
    /// and a repository whose rules were signed before that kind existed is
    /// governed by `protection_tags` exactly as it always was. A record whose
    /// author is not in the founder set is ignored here, as it is at the gate.
    #[serde(default)]
    pub rule_records: Option<Vec<serde_json::Value>>,
    /// Pubkeys the repository's project roster grants Owner, or null when
    /// this view could not read the roster.
    ///
    /// Finding 33: a repository's founders are its announcement's signer, its
    /// NIP-34 `maintainers`, **and** every Owner on the project roster its
    /// `["project", …]` back-reference names (commit `a56ad5d01`). The first
    /// two are on `protection_tags`; only this one needs a second read, and
    /// null is "not read", never "there are none" — the answer says which.
    pub project_owner_pubkeys: Option<Vec<String>>,
    /// The mission's active seats, with their roles.
    ///
    /// Arm (C) asks whether a **`verifier`** seat cleared the report, so a
    /// caller that sends no seats gets no arm-(C) answer — the response says
    /// `seatsRead: false` and the refusal is the honest "nobody verified it",
    /// never a claim that the mission has no verifier. Defaulted so a caller
    /// built before the 2026-09-03 ruling still decodes.
    #[serde(default)]
    pub active_seats: Vec<CodingSessionLandSeat>,
    /// The mission's folded kind 44246 gate rows, as this view already holds
    /// them.
    ///
    /// Arm (B) reads these, and since the 2026-09-03 follow-up ruling so does
    /// arm (C). A caller that sends none gets **no landing at all** on a
    /// governed ref unless the viewer is a founder — which is the honest
    /// answer, not a silent pass: the relay reads the rows whether this view
    /// did or not, and a screen that said "ready" over rows it never read
    /// would be predicting an admission the push will not get. Defaulted so a
    /// caller built before 2026-09-03 still decodes.
    #[serde(default)]
    pub observed_gates: Vec<CodingSessionLandObservedGate>,
    /// The gate half of the mission's newest founder-signed kind 44245 policy,
    /// or null when this view read none.
    ///
    /// Null resolves to [`GatePolicyResolution::Absent`] — *nobody set one* —
    /// which is what this field has always meant. A caller that can tell
    /// `absent` from `withdrawn` or `unreadable` says so through
    /// [`CodingSessionLandGatePolicy::resolution`] (finding 89).
    #[serde(default)]
    pub gate_policy: Option<CodingSessionLandGatePolicy>,
    /// The kind:30617 coordinates this mission may prove commits for, as this
    /// view read them, or null when it read none (finding 91).
    ///
    /// Null is **not** "bound to nothing": it is "this view did not read the
    /// binding", and the answer says which through
    /// [`CodingSessionLandResponse::bound_repositories_read`]. When it is
    /// null this adapter assumes the repository it was asked about, which is
    /// what it assumed before the field existed — the relay checks the real
    /// binding either way, so the only thing a null can cost is a prediction
    /// that is friendlier than the gate, and the response discloses it.
    #[serde(default)]
    pub bound_repositories: Option<Vec<String>>,
    /// Event ids the caller's fold marked canonical, in included order.
    pub included_event_ids: Vec<String>,
    /// Raw signed kind-44244 Nostr events. No projected outputs are accepted.
    pub events: Vec<serde_json::Value>,
}

/// One active seat of the mission, as the caller's authority fold read it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandSeat {
    /// Lowercase-hex pubkey holding the seat.
    pub actor_pubkey: String,
    /// The seat's role token, e.g. `builder`, `lead`, `verifier`.
    pub role: String,
}

/// One folded gate row, as the mission surface already holds it.
///
/// Deliberately the *folded* row and not a raw event: the desktop's fold has
/// already applied the provenance check that turns a seat's `observed` claim
/// into `declared` (REVIEW-L5 F2), and re-deriving it here would be a second
/// implementation of the rule that decides what counts as watched.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandObservedGate {
    /// Who signed the newest observation naming this gate.
    pub author_pubkey: String,
    /// `observed` or `declared`, as the fold settled it.
    pub source: String,
    /// The gate's name.
    pub gate: String,
    /// `passed`, `failed` or `not-run`.
    pub outcome: String,
    /// The commit the row names, or null.
    pub head_sha: Option<String>,
    /// Whether the tree was dirty, or null.
    pub dirty: Option<bool>,
}

/// The gate half of a mission policy, as this view read it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandGatePolicy {
    /// `gates.verifierRequired`, or null when the founder set no flag.
    pub verifier_required: Option<bool>,
    /// `gates.requiredGates`, or null when the policy names none.
    pub required_gates: Option<Vec<String>>,
    /// The kind 44245 record this was read from, when the caller knows it.
    #[serde(default)]
    pub event_id: Option<String>,
    /// How that record resolved: `present`, `withdrawn`, `absent` or
    /// `unreadable` (finding 89).
    ///
    /// Null means `present` — the shape every caller sent before the field
    /// existed, and the only one that carried a gate half at all. A token
    /// this build does not know resolves to **unreadable**, never to
    /// `present`: a word nobody recognises must not become a policy.
    #[serde(default)]
    pub resolution: Option<String>,
    /// The validator's own sentence, when the resolution is `unreadable`.
    #[serde(default)]
    pub unreadable_reason: Option<String>,
}

/// What admitted a commit, for the confirm step's first sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionLandEvidence {
    /// Which arm admitted: `founder`, `observed-gates` or `verifier-verdict`.
    ///
    /// A screen that says only "ready" over two very different facts — *"you
    /// are trusted"* and *"a machine checked it"* — is the kind of comfortable
    /// guess this project treats as a bug. The arm is always named.
    pub arm: String,
    /// Umbrella whose fold admitted it. Empty under arm (A), which reads none.
    pub session_ref: String,
    /// The approving disposition. Empty under arm (A).
    pub disposition_event_id: String,
    /// Who signed that disposition. Empty under arm (A).
    pub disposition_author_pubkey: String,
    /// The verifier's `not-refuted` refutation. Empty under arm (A).
    pub refutation_event_id: String,
    /// The verifier seat that signed it. Empty under arm (A).
    pub verifier_pubkey: String,
    /// The report both records govern. Empty under arm (A).
    pub report_event_id: String,
    /// That report's `headSha`, as published. Empty under arm (A). Under arm
    /// (B) it is the commit every required gate row named.
    pub head_sha: String,
    /// The gates that had to be green on this commit, in the order they were
    /// required. Empty only under arm (A), which reads no mission.
    ///
    /// Under arm (C) too since the 2026-09-03 follow-up ruling: a verifier's
    /// clearance is half of what that arm wants and these rows are the other
    /// half, so a confirm step that named only the verifier would be telling
    /// half the truth about what admitted the push.
    pub observed_gates: Vec<String>,
    /// How the mission's kind 44245 policy resolved under arms (B) and (C):
    /// `present`, `withdrawn` or `absent`. Empty under arm (A), which reads
    /// no policy at all.
    pub policy_resolution: String,
    /// The kind 44245 record the arm read, or null when none named the
    /// mission (`absent`) and under arm (A).
    pub policy_event_id: Option<String>,
    /// Why no policy was consulted, under arm (A) only: `founder_exception`.
    ///
    /// The audit's Q1: a founder's landing is the deliberate exception, and
    /// this is what stops it reading as though a verifier approved it.
    pub policy_not_evaluated: Option<String>,
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
    /// Whether the caller sent the mission's seat roster.
    ///
    /// `false` means arm (C) could not be evaluated at all from here — **not**
    /// that no verifier cleared the report. The two are different facts, and a
    /// screen that prints the second over the first is telling a comfortable
    /// lie about a gate.
    pub seats_read: bool,
    /// Whether the caller sent any folded kind 44246 gate row at all.
    ///
    /// Load-bearing since the 2026-09-03 follow-up ruling made those rows half
    /// of arm (C). `false` means the gate half **could not be evaluated from
    /// here** — not that the gates were never green. Without it the refusal
    /// *"gate `cargo fmt` has no observed green row on <sha>"* reads as a fact
    /// about the mission when it may only be a fact about this screen's reads,
    /// which is the same comfortable lie `seats_read` exists to prevent.
    pub gate_rows_read: bool,
    /// Whether the caller sent the mission's repository binding (finding 91).
    ///
    /// `false` means this answer **assumed** the mission is bound to the
    /// repository it was asked about. The relay checks the real binding, so a
    /// `false` here is the one place this adapter can be friendlier than the
    /// gate — and it says so rather than letting the screen imply otherwise.
    pub bound_repositories_read: bool,
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
fn ref_requires_verdict(
    tags: &[Vec<String>],
    rule_records: Option<&Vec<serde_json::Value>>,
    founders: &RepositoryFounders,
    ref_name: &str,
) -> bool {
    let Ok(announcement) = ProtectionLayer::from_announcement_tags(0, String::new(), tags) else {
        return false;
    };
    let mut layers = vec![announcement];
    for value in rule_records.map(Vec::as_slice).unwrap_or_default() {
        // Verified, not merely decoded: a rule read off an unverified event
        // would be a claim about who governs the repository, made by nobody.
        let Ok(event) = serde_json::from_value::<Event>(value.clone()) else {
            continue;
        };
        if event.verify().is_err() {
            continue;
        }
        let Ok(record) = decode_repository_protection(&event) else {
            continue;
        };
        if !founders.contains(record.author()) {
            continue;
        }
        // The announcement layer is stamped 0 above, so any record wins its
        // patterns. That is the safe direction here and only here: this
        // adapter predicts, and the relay — which holds both `created_at`s —
        // decides. Predicting "governed" for a ref the relay leaves ungoverned
        // costs a founder one unnecessary verdict; the reverse costs them a
        // refused push they were told would succeed.
        layers.push(ProtectionLayer::from_record(
            &record,
            record.created_at().max(1),
        ));
    }
    let resolved = resolve_protection_layers(&layers);
    EffectiveRules::for_ref(ref_name, resolved.rules()).require_verdict
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
    // The founder set, composed by `buzz-core` from what this view read: the
    // signer and `maintainers` off the announcement's own tags, plus the
    // roster owners the caller resolved. A view with no announcement has no
    // founders to name, and says so rather than naming the viewer.
    //
    // Resolved before the rule check, because since lane L26 the rules
    // themselves are filtered by it: only a founder's rule record governs.
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
    let rule_governs = request.protection_tags.as_ref().is_some_and(|tags| {
        ref_requires_verdict(
            tags,
            request.rule_records.as_ref(),
            &founders,
            &request.ref_name,
        )
    });
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
        seats_read: !request.active_seats.is_empty(),
        gate_rows_read: !request.observed_gates.is_empty(),
        bound_repositories_read: request.bound_repositories.is_some(),
        command: None,
    };
    if !rule_governs {
        // Not a refusal: the rule simply does not govern this ref. The caller
        // renders §1l's own sentence for that case and still shows the verdict
        // it read, because "nothing gates this" and "nobody ruled" differ.
        return Ok(base);
    }
    let Some(repo_owner) = request.repo_owner_pubkey.clone() else {
        return Ok(CodingSessionLandResponse {
            refusal_reason: Some(VerdictAdmissionRefusal::RepositoryUnbound.reason()),
            ..base
        });
    };
    // The coordinate the mission must be bound to (finding 91), built from
    // the announcement's own signer and `d` tag exactly as the relay builds
    // it. An announcement with no `d` addresses nothing and binds nothing.
    let repository = repository_coordinate(&repo_owner, request.protection_tags.as_deref());
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
        active_seats: request
            .active_seats
            .iter()
            .map(|seat| CodingSessionTeamActiveSeat {
                actor_pubkey: seat.actor_pubkey.clone(),
                role: seat.role.clone(),
            })
            .collect(),
        observed_gates: request
            .observed_gates
            .iter()
            .filter_map(observed_gate_entry)
            .collect(),
        gate_policy: gate_policy_resolution(request.gate_policy.as_ref()),
        // Finding 91: a mission proves commits only for the repositories it
        // is bound to. This view asks about one mission and one repository;
        // when the caller did not send the binding, the answer assumes the
        // repository it was asked about and `boundRepositoriesRead` says so.
        bound_repositories: match &request.bound_repositories {
            Some(bound) => bound.clone(),
            None => vec![repository.clone()],
        },
        // This surface reads one mission's policy record as the caller sends
        // it, not a page of them, so it excludes none and says `0` rather than
        // implying it counted (2026-09-05 refuter, S2).
        excluded_unauthorized_policies: 0,
    };
    // This screen is looking at one mission and asks about that one only. The
    // relay's push gate sweeps the pusher's seats, the project's session
    // channels or the bound channel (finding 56); saying so here would claim a
    // search this adapter never performs.
    let source = VerdictAdmissionCandidateSource::ThisMission {
        session_ref: request.session_ref.clone(),
    };
    let query = VerdictAdmissionQuery {
        ref_name: &request.ref_name,
        new_oid: newest_head_sha(&records).unwrap_or_default(),
        pusher_pubkey: &request.pusher_pubkey,
        repo_founders: founders.pubkeys(),
        candidate_source: &source,
        repository: &repository,
    };
    match evaluate_verdict_admission(std::slice::from_ref(&candidate), &query) {
        // Arm (A). The commit named in the command is the mission's newest
        // reported head, because that is the only commit this screen knows —
        // and the copy says the push lands because the viewer is a founder,
        // not because anything ruled on it.
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::FounderPush {
            policy_not_evaluated,
            ..
        }) => {
            let head_sha = newest_head_sha(&records).unwrap_or_default().to_owned();
            Ok(CodingSessionLandResponse {
                admitted: true,
                command: (!head_sha.is_empty())
                    .then(|| format!("git push origin {head_sha}:{}", request.ref_name)),
                evidence: Some(CodingSessionLandEvidence {
                    arm: "founder".to_owned(),
                    session_ref: String::new(),
                    disposition_event_id: String::new(),
                    disposition_author_pubkey: String::new(),
                    refutation_event_id: String::new(),
                    verifier_pubkey: String::new(),
                    report_event_id: String::new(),
                    head_sha,
                    observed_gates: Vec::new(),
                    policy_resolution: String::new(),
                    policy_event_id: None,
                    policy_not_evaluated: Some(policy_not_evaluated.as_str().to_owned()),
                }),
                ..base
            })
        }
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::VerifierVerdict {
            session_ref,
            disposition_event_id,
            refutation_event_id,
            report_event_id,
            head_sha,
            verifier_pubkey,
            gates,
            policy,
            ..
        }) => Ok(CodingSessionLandResponse {
            admitted: true,
            command: Some(format!("git push origin {head_sha}:{}", request.ref_name)),
            evidence: Some(CodingSessionLandEvidence {
                arm: "verifier-verdict".to_owned(),
                session_ref,
                disposition_author_pubkey: disposition_author(&disposition_event_id),
                disposition_event_id,
                refutation_event_id,
                verifier_pubkey,
                report_event_id,
                head_sha,
                observed_gates: gates,
                policy_resolution: policy.resolution.as_str().to_owned(),
                policy_event_id: policy.event_id.clone(),
                policy_not_evaluated: None,
            }),
            ..base
        }),
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::ObservedGates {
            session_ref,
            head_sha,
            gates,
            policy,
            ..
        }) => Ok(CodingSessionLandResponse {
            admitted: true,
            command: Some(format!("git push origin {head_sha}:{}", request.ref_name)),
            evidence: Some(CodingSessionLandEvidence {
                arm: "observed-gates".to_owned(),
                session_ref,
                disposition_event_id: String::new(),
                disposition_author_pubkey: String::new(),
                refutation_event_id: String::new(),
                verifier_pubkey: String::new(),
                report_event_id: String::new(),
                head_sha,
                observed_gates: gates,
                policy_resolution: policy.resolution.as_str().to_owned(),
                policy_event_id: policy.event_id.clone(),
                policy_not_evaluated: None,
            }),
            ..base
        }),
        VerdictAdmission::Refused(refusal) => Ok(CodingSessionLandResponse {
            refusal_reason: Some(refusal.reason()),
            ..base
        }),
    }
}

/// The `30617:<owner-hex>:<d>` coordinate the announcement addresses.
///
/// Built from the signer and the announcement's own `d` tag, exactly as
/// `buzz_relay::api::git::verdict_admission::repository_coordinate` builds it,
/// so this screen and the gate name one repository the same way. A view with
/// no tags, or an announcement with no `d`, addresses nothing and yields the
/// empty string — which no mission is bound to.
fn repository_coordinate(repo_owner: &str, protection_tags: Option<&[Vec<String>]>) -> String {
    let name = protection_tags
        .unwrap_or_default()
        .iter()
        .find_map(|tag| match tag.as_slice() {
            [key, value] if key == "d" => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_default();
    if name.is_empty() || repo_owner.is_empty() {
        return String::new();
    }
    format!("30617:{repo_owner}:{name}")
}

/// Turn the wire's policy half into the rule's resolution (finding 89).
///
/// `None` is `Absent` — nobody set a policy — which is what a missing field
/// has always meant here. A `resolution` token this build does not know
/// becomes `Unreadable`, so an unrecognised word closes both arms instead of
/// quietly becoming a policy the founder never set.
fn gate_policy_resolution(policy: Option<&CodingSessionLandGatePolicy>) -> GatePolicyResolution {
    let Some(policy) = policy else {
        return GatePolicyResolution::Absent;
    };
    let event_id = policy.event_id.clone().unwrap_or_default();
    match policy.resolution.as_deref() {
        None | Some("present") => GatePolicyResolution::Present {
            policy: VerdictAdmissionGatePolicy {
                verifier_required: policy.verifier_required,
                required_gates: policy.required_gates.clone(),
            },
            event_id,
        },
        Some("withdrawn") => GatePolicyResolution::Withdrawn { event_id },
        Some("absent") => GatePolicyResolution::Absent,
        Some("unreadable") => GatePolicyResolution::Unreadable {
            event_id,
            reason: policy
                .unreadable_reason
                .clone()
                .unwrap_or_else(|| "the caller read no reason".to_owned()),
        },
        Some(other) => GatePolicyResolution::Unreadable {
            event_id,
            reason: format!("this build does not know the policy resolution `{other}`"),
        },
    }
}

/// Turn one wire row into the fold entry the rule reads.
///
/// A row whose `source` or `outcome` is a word this build does not know is
/// **dropped**, never coerced: a token nobody recognises must not become
/// `observed` and `passed` by accident, which is the one direction that could
/// admit a push nothing measured.
fn observed_gate_entry(
    row: &CodingSessionLandObservedGate,
) -> Option<CodingSessionObservationGateEntry> {
    let source: CodingSessionObservationSource =
        serde_json::from_value(serde_json::Value::String(row.source.clone())).ok()?;
    let outcome: CodingSessionObservationGateOutcome =
        serde_json::from_value(serde_json::Value::String(row.outcome.clone())).ok()?;
    Some(CodingSessionObservationGateEntry {
        author_pubkey: row.author_pubkey.clone(),
        source,
        row: CodingSessionObservationGateRow {
            gate: row.gate.clone(),
            outcome,
            // The rule reads neither, and a command this boundary invented
            // would be a fabricated quotation.
            command: String::new(),
            summary: None,
            duration_ms: None,
            head_sha: row.head_sha.clone(),
            dirty: row.dirty,
        },
        event_ids: Vec::new(),
        dropped_event_ids: 0,
        assignment_ref: None,
    })
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
