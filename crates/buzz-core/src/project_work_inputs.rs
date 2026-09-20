//! NIP-PW: the one shared assembler that turns fetched events into the
//! coverage fold's input.
//!
//! [`fold_work`](crate::project_work_fold::fold_work) reads **no events but
//! the 44249 records**: everything else arrives as facts the caller
//! established and verified (`conformance/project-work/README.md` § (c) "The
//! input"). Establishing those facts is real work — decoding kind:44244
//! reports and verdicts, pairing a kind:46023 host result with the relay's
//! kind:46014 echo and the kind:46013 request that names the definition it
//! was supposed to run, keeping only relay-signed kind:30618 ref state — and
//! it is work every reader of a session's coverage must do identically.
//!
//! **So it is done once, here.** The CLI (`bee sessions work status`), the
//! desktop surface and the session provider all call
//! [`assemble_fold_inputs`]; a second assembler anywhere would be a second
//! place for "what counts as an approving disposition" to be decided, and the
//! two would eventually disagree about a criterion while both printed a
//! confident answer.
//!
//! Nothing here performs I/O, reads a clock or verifies a signature. The
//! caller fetched the events from a relay that verified their signatures at
//! ingest, and [`RawWorkInputs`] documents, field by field, where each input
//! comes from. What this module *does* decide is the **shape** questions the
//! README assigns to the caller: which events decode, which host result is
//! carried by which echo, which ref states are relay-signed, and which 44227
//! goal is current.
//!
//! An input this module cannot use is **left out, not guessed at**: the fold
//! then reports the criterion that needed it as `unknown` with
//! `evidence_unavailable` or `plan_unreadable`, which is the honest answer.
//! Only a defect that would make the whole projection meaningless — a raw
//! event set with no founder to judge authority against — is an
//! [`AssembleRefusal`].

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::coding_session_goal::{
    validate_coding_session_goal_content, validate_coding_session_goal_session_ref,
    CODING_SESSION_GOAL_TAG_VERSION,
};
use crate::kind::{
    KIND_CODING_SESSION_GOAL, KIND_CODING_SESSION_TEAM_TRANSACTION, KIND_GIT_REPO_STATE,
    KIND_HOST_STEP_RESULT, KIND_PROJECT_WORK_RECORD, KIND_WORKFLOW_HOST_STEP_EXITED,
    KIND_WORKFLOW_HOST_STEP_REQUESTED,
};
use crate::project_work::ProjectWorkEvent;
use crate::project_work_fold::{
    WorkActionDefinition, WorkActiveGrant, WorkActiveSeat, WorkAuthority, WorkCheckoutFact,
    WorkEvidenceFact, WorkFoldInputs,
};

/// The authority facts the 44244 fold judges standing with.
///
/// **Provenance.** `founder_pubkey` is the signer of the session's kind:44222
/// genesis. `active_seats` and `active_grants` are the *accepted* kind:44228
/// authority chain's projection — the one built from relay acceptance
/// receipts (`bee sessions`' `operations_authority`, the desktop's authority
/// reader). That projection needs relay-signed receipts and is therefore a
/// read, not a fold over the transitions alone, which is why it arrives here
/// already made rather than being recomputed from raw events: recomputing it
/// without the receipts would admit a transition the relay never accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAuthorityContext {
    /// The session genesis event, whose **signer** is the founder. Supplying
    /// it is how a caller proves the founder rather than asserting it.
    pub genesis_event: Option<ProjectWorkEvent>,
    /// The founder pubkey, when the caller holds it without the genesis event
    /// (the desktop's session record carries it). Ignored when
    /// `genesis_event` is present, so the two can never disagree silently.
    pub founder_pubkey: Option<String>,
    /// Actors holding an active seat, from the accepted 44228 projection.
    pub active_seats: Vec<WorkActiveSeat>,
    /// Actors holding an active grant, from the same projection.
    pub active_grants: Vec<WorkActiveGrant>,
}

/// Every event and blob a caller fetched, before anything is derived from it.
///
/// Each field says where its contents come from, because the honesty of the
/// projection rests on that provenance: a 30618 that is not relay-signed is
/// not an observation, and an action definition compiled from anything but
/// `actions.yml` at the declaration's plan commit lets evidence nominate its
/// own expected hash.
#[derive(Debug, Clone, Default)]
pub struct RawWorkInputs {
    /// The session's kind:44249 work records, from
    /// `{"kinds":[44249],"#h":[channel],"#d":[sessionRef]}`. Other kinds in
    /// this list are ignored by the fold, exactly as a mixed stream must be.
    pub work_events: Vec<ProjectWorkEvent>,
    /// The session's kind:44244 team transactions, from the same session
    /// query the 44244 fold uses. Reports, verdicts (dispositions) and
    /// assignments are read; every other subtype is ignored.
    pub team_events: Vec<ProjectWorkEvent>,
    /// Host-signed kind:46023 results for the project's action runs.
    pub host_results: Vec<ProjectWorkEvent>,
    /// The relay-signed kind:46014 echoes that carry those results. A result
    /// with no echo yields no fact: the relay's echo is what makes a host's
    /// self-report an accepted one.
    pub host_echoes: Vec<ProjectWorkEvent>,
    /// The relay-signed kind:46013 requests those runs answer, which name the
    /// action and the **definition hash the run was bound to** (ledger 193).
    /// Absent, the run's action name and hash cannot be established and no
    /// fact is derived.
    pub host_requests: Vec<ProjectWorkEvent>,
    /// Every known kind:30618 ref state for the plan's code repository.
    /// Non-relay-signed rows are dropped here, not shown to the fold.
    pub ref_states: Vec<ProjectWorkEvent>,
    /// The session's kind:44227 goal events. The newest is the current goal.
    pub goal_events: Vec<ProjectWorkEvent>,
    /// The authority facts, with their provenance documented above.
    pub authority: RawAuthorityContext,
    /// The relay's NIP-11 `self` key, lowercase 64-hex. `None` means the
    /// caller could not read it, and ref observations and host echoes then
    /// read `unknown` rather than being believed.
    pub relay_self_key: Option<String>,
    /// Plan blobs the caller read with `git show <commit>:<path>`, keyed by
    /// `(repository coordinate, commit, path)` — never from a working copy
    /// and never from a fetched tip.
    pub plan_blobs: BTreeMap<(String, String, String), String>,
    /// Action definitions the caller compiled with the publication compiler
    /// from `actions.yml` **at the declaration's plan commit**, keyed by
    /// `(repository coordinate, commit, action name)`.
    pub action_definitions: BTreeMap<(String, String, String), WorkActionDefinition>,
    /// The session this projection is about; derived from the records when
    /// absent.
    pub session_ref: Option<String>,
    /// The project this projection is about; derived from the records when
    /// absent.
    pub project_ref: Option<String>,
}

/// Why a raw input set could not be assembled at all.
///
/// Deliberately short. Almost every defect in a raw set is a *missing fact*,
/// which the fold reports as `unknown` against the criterion that needed it;
/// only the absence of an authority to judge signers against makes the whole
/// projection meaningless, because every record would then be excluded
/// `signer_not_may_lead` and the output would read as "nobody did anything".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssembleRefusalCode {
    /// Neither a genesis event nor a founder pubkey was supplied.
    FounderUnknown,
    /// The supplied genesis event is not a 64-hex-signed event.
    FounderMalformed,
}

impl AssembleRefusalCode {
    /// The stable string form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FounderUnknown => "founder_unknown",
            Self::FounderMalformed => "founder_malformed",
        }
    }
}

/// One refusal: the stable code and one sentence a person can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssembleRefusal {
    /// The stable code.
    pub code: AssembleRefusalCode,
    /// Why, in words, and what to supply instead.
    pub message: String,
}

impl std::fmt::Display for AssembleRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

/// The key the fold indexes a plan blob by.
fn blob_key(repository: &str, commit: &str, path: &str) -> String {
    format!("{repository}@{commit}:{path}")
}

/// Whether a string is lowercase hex of exactly `len` bytes.
fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Build the coverage fold's input from the events and blobs a caller fetched.
///
/// Pure: no I/O, no clock, no ordering assumption. Permuting any input list
/// produces the same [`WorkFoldInputs`], because every derived collection is
/// a `BTreeMap`/`BTreeSet` or is sorted by the event's own order key.
///
/// # What it derives
///
/// - `evidence` — one [`WorkEvidenceFact`] per usable kind:44244 report,
///   kind:44244 disposition verdict and echoed kind:46023 host result.
/// - `currentGoalRef` and `goalEvents` — the session's kind:44227 set, newest
///   by `(created_at, id)` being the current goal.
/// - `refStates` — the relay-signed kind:30618 rows only.
/// - `authority` — the founder from the genesis signer, with the accepted
///   seat and grant projection the caller supplied.
/// - `planBlobs` and `actionDefinitions` — re-keyed into the fold's flat key
///   space, so a caller cannot key a blob by a coordinate it never read.
///
/// # Errors
///
/// [`AssembleRefusal`] when no founder can be established — see
/// [`AssembleRefusalCode`]. Every other unusable input is left out, and the
/// fold reports the criterion that needed it as `unknown`.
pub fn assemble_fold_inputs(raw: RawWorkInputs) -> Result<WorkFoldInputs, AssembleRefusal> {
    let founder = resolve_founder(&raw.authority)?;
    let relay_self_key = raw
        .relay_self_key
        .as_deref()
        .map(str::to_ascii_lowercase)
        .filter(|key| is_hex(key, 64));

    let (current_goal_ref, goal_events) = derive_goals(&raw.goal_events);

    let mut evidence: BTreeMap<String, WorkEvidenceFact> = BTreeMap::new();
    for fact in derive_team_facts(&raw.team_events) {
        evidence.insert(fact.event_id().to_owned(), fact);
    }
    for fact in derive_action_facts(&raw.host_results, &raw.host_echoes, &raw.host_requests) {
        evidence.insert(fact.event_id().to_owned(), fact);
    }

    // An owner-signed claim about its own branch is not an observation. The
    // fold checks this too; dropping the rows here as well means a reader
    // that prints `refStates` never shows a row it would not have believed.
    let mut ref_states: Vec<ProjectWorkEvent> = raw
        .ref_states
        .into_iter()
        .filter(|state| {
            state.kind == KIND_GIT_REPO_STATE
                && relay_self_key
                    .as_deref()
                    .is_some_and(|key| state.pubkey == key)
        })
        .collect();
    ref_states.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
    ref_states.dedup_by(|a, b| a.id == b.id);

    let work_events: Vec<ProjectWorkEvent> = raw
        .work_events
        .into_iter()
        .filter(|event| event.kind == KIND_PROJECT_WORK_RECORD)
        .collect();

    Ok(WorkFoldInputs {
        events: work_events,
        relay_self_key,
        authority: WorkAuthority {
            founder_pubkey: founder,
            active_seats: raw.authority.active_seats,
            active_grants: raw.authority.active_grants,
        },
        current_goal_ref,
        goal_events,
        plan_blobs: raw
            .plan_blobs
            .into_iter()
            .map(|((repository, commit, path), blob)| (blob_key(&repository, &commit, &path), blob))
            .collect(),
        // The fold keys action definitions by name alone: a declaration's
        // criteria all resolve against one plan commit, so the caller's
        // (repository, commit, name) key collapses to the name it compiled.
        action_definitions: raw
            .action_definitions
            .into_iter()
            .map(|((_, _, name), definition)| (name, definition))
            .collect(),
        evidence,
        ref_states,
        session_ref: raw.session_ref,
        project_ref: raw.project_ref,
    })
}

/// The founder, from the genesis signer when one was supplied.
fn resolve_founder(authority: &RawAuthorityContext) -> Result<String, AssembleRefusal> {
    if let Some(genesis) = authority.genesis_event.as_ref() {
        let signer = genesis.pubkey.to_ascii_lowercase();
        if !is_hex(&signer, 64) {
            return Err(AssembleRefusal {
                code: AssembleRefusalCode::FounderMalformed,
                message: format!(
                    "the supplied session genesis is signed by {:?}, which is not a 64-hex pubkey",
                    genesis.pubkey
                ),
            });
        }
        return Ok(signer);
    }
    match authority.founder_pubkey.as_deref() {
        Some(founder) if is_hex(&founder.to_ascii_lowercase(), 64) => {
            Ok(founder.to_ascii_lowercase())
        }
        Some(founder) => Err(AssembleRefusal {
            code: AssembleRefusalCode::FounderMalformed,
            message: format!("founderPubkey {founder:?} is not a 64-hex pubkey"),
        }),
        None => Err(AssembleRefusal {
            code: AssembleRefusalCode::FounderUnknown,
            message: "no session genesis event and no founder pubkey were supplied, so no \
                      record's signer could be judged and every one of them would be excluded"
                .to_owned(),
        }),
    }
}

/// The session's goal set, and which of them is current.
///
/// The current goal is the newest by `(created_at, id)` — the same total
/// order every fold in this crate sorts by, so two readers with the same
/// events never disagree about which goal is in force.
fn derive_goals(events: &[ProjectWorkEvent]) -> (Option<String>, BTreeSet<String>) {
    let mut goals: Vec<&ProjectWorkEvent> = events
        .iter()
        .filter(|event| {
            event.kind == KIND_CODING_SESSION_GOAL
                && is_hex(&event.id, 64)
                && is_goal_envelope(event)
        })
        .collect();
    goals.sort_by_key(|event| event.order_key());
    let current = goals.last().map(|event| event.id.clone());
    let ids = goals.iter().map(|event| event.id.clone()).collect();
    (current, ids)
}

/// Whether a kind:44227 event carries the exact goal envelope.
///
/// The envelope validator in `coding_session_goal` takes a `nostr::Event`;
/// this reads the same three ordered two-field tags and the same content rule
/// off the plain event the assembler is handed, so a malformed row is dropped
/// rather than counted as a goal a declaration could be pinned to.
fn is_goal_envelope(event: &ProjectWorkEvent) -> bool {
    if validate_coding_session_goal_content(&event.content).is_err() {
        return false;
    }
    if event.tags.len() != 3 || event.tags.iter().any(|tag| tag.len() != 2) {
        return false;
    }
    event.tags[0][0] == "h"
        && event.tags[1][0] == "d"
        && validate_coding_session_goal_session_ref(&event.tags[1][1]).is_ok()
        && event.tags[2][0] == "csgl-v"
        && event.tags[2][1] == CODING_SESSION_GOAL_TAG_VERSION
}

/// The kind:44244 body shapes this assembler reads, and nothing else.
#[derive(Debug, Deserialize)]
struct TeamEnvelope {
    #[serde(rename = "type")]
    transaction_type: String,
    body: serde_json::Value,
}

/// Report and verdict facts from a session's kind:44244 records.
///
/// The record's **signer** is its event's `pubkey`: a body never restates who
/// wrote it, and the fold judges the verdict's signer against `may_lead`.
/// A report with no `headSha` yields no fact — a report that does not name a
/// revision cannot say a criterion was met at one — and a refutation verdict
/// yields none either, because only a disposition rules on a report.
fn derive_team_facts(events: &[ProjectWorkEvent]) -> Vec<WorkEvidenceFact> {
    let mut facts = Vec::new();
    for event in events {
        if event.kind != KIND_CODING_SESSION_TEAM_TRANSACTION || !is_hex(&event.id, 64) {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<TeamEnvelope>(&event.content) else {
            continue;
        };
        let signer = event.pubkey.to_ascii_lowercase();
        let body = &envelope.body;
        let text = |key: &str| body.get(key).and_then(serde_json::Value::as_str);
        match envelope.transaction_type.as_str() {
            "report" => {
                let (Some(assignment_ref), Some(head_sha)) =
                    (text("assignmentRef"), text("headSha"))
                else {
                    continue;
                };
                facts.push(WorkEvidenceFact::Report {
                    event_id: event.id.clone(),
                    signer,
                    assignment_ref: assignment_ref.to_owned(),
                    head_sha: head_sha.to_ascii_lowercase(),
                });
            }
            "verdict" => {
                let Some(subtype) = text("subtype") else {
                    continue;
                };
                let (Some(decision), Some(assignment_ref), Some(report_ref)) =
                    (text("decision"), text("assignmentRef"), text("reportRef"))
                else {
                    continue;
                };
                facts.push(WorkEvidenceFact::Verdict {
                    event_id: event.id.clone(),
                    signer,
                    subtype: subtype.to_owned(),
                    decision: decision.to_owned(),
                    assignment_ref: assignment_ref.to_owned(),
                    report_ref: report_ref.to_owned(),
                });
            }
            _ => {}
        }
    }
    facts
}

/// One host run's pairing: the result, the relay's echo of it, and the
/// request that bound it to a definition.
fn derive_action_facts(
    results: &[ProjectWorkEvent],
    echoes: &[ProjectWorkEvent],
    requests: &[ProjectWorkEvent],
) -> Vec<WorkEvidenceFact> {
    let mut facts = Vec::new();
    for result in results {
        if result.kind != KIND_HOST_STEP_RESULT || !is_hex(&result.id, 64) {
            continue;
        }
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(&result.content) else {
            continue;
        };
        let (Some(run_id), Some(step_id)) = (
            payload.get("runId").and_then(serde_json::Value::as_str),
            payload.get("stepId").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        // The relay's kind:46014 echo is what makes a host's self-report an
        // accepted result; without it, nothing here says this ran at all.
        let Some(echo) = echoes.iter().find(|echo| {
            echo.kind == KIND_WORKFLOW_HOST_STEP_EXITED
                && serde_json::from_str::<serde_json::Value>(&echo.content).is_ok_and(|body| {
                    body.get("resultEventId")
                        .and_then(serde_json::Value::as_str)
                        == Some(&result.id)
                })
        }) else {
            continue;
        };
        // The kind:46013 request names the action and the definition hash the
        // run was bound to. Reading either from the result would let the
        // runner nominate what its own run proves.
        let Some(request) = requests.iter().find_map(|request| {
            if request.kind != KIND_WORKFLOW_HOST_STEP_REQUESTED {
                return None;
            }
            let body = serde_json::from_str::<serde_json::Value>(&request.content).ok()?;
            let matches = body.get("runId").and_then(serde_json::Value::as_str) == Some(run_id)
                && body.get("stepId").and_then(serde_json::Value::as_str) == Some(step_id);
            matches.then_some(body)
        }) else {
            continue;
        };
        let (Some(action_name), Some(definition_hash)) = (
            request
                .get("workflowName")
                .and_then(serde_json::Value::as_str),
            request
                .get("definitionHash")
                .and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let checkout = payload.get("checkout");
        facts.push(WorkEvidenceFact::ActionResult {
            event_id: result.id.clone(),
            exited_event_id: echo.id.clone(),
            result_signer: result.pubkey.to_ascii_lowercase(),
            echo_signer: echo.pubkey.to_ascii_lowercase(),
            action_name: action_name.to_owned(),
            run_id: run_id.to_owned(),
            step_id: step_id.to_owned(),
            definition_hash: definition_hash.to_ascii_lowercase(),
            disposition: payload
                .get("disposition")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            exit_code: payload
                .get("exitCode")
                .and_then(serde_json::Value::as_i64)
                // An absent exit code is not a zero: a result that never ran
                // a command must never read as a passing one.
                .unwrap_or(i64::from(i32::MIN)),
            checkout: WorkCheckoutFact {
                mode: checkout
                    .and_then(|value| value.get("mode"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                sha: checkout
                    .and_then(|value| value.get("sha"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
                dirty_before: checkout
                    .and_then(|value| value.get("dirtyBefore"))
                    .and_then(serde_json::Value::as_bool)
                    // An unread pre-execution sample is not a clean tree.
                    .unwrap_or(true),
            },
            dirty: payload
                .get("dirty")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
        });
    }
    facts
}

#[cfg(test)]
#[path = "project_work_inputs_tests.rs"]
mod tests;
