//! `bee sessions work bind ref` — bind a `git-ref` criterion to the relay's
//! own observation of the plan's delivery ref (ledger 255).
//!
//! The evidence kind already exists: a `ref_observation` is a **relay-signed
//! kind:30618** for the plan's `code_repository`, judged at evaluation time
//! (`conformance/project-work/README.md` § "`ref_observation`, and how
//! delivery is judged", ruling 13). What run 4's lead lacked was a way to
//! find one: `bind evidence` wants the 30618's event id, and nothing named
//! it. This verb looks it up, checks it before signing, and binds it.
//!
//! What it refuses, each before anything is signed:
//!
//! - `malformed-commit` — `--commit` is not 40 or 64 hex;
//! - `evidence-kind-mismatch` — a named criterion is not a `git-ref` proof;
//! - `ref-not-delivery-ref` — `--ref` is not the plan's `delivery_ref`: the
//!   plan names the ref a delivery is judged at, never the binder;
//! - `relay-self-unavailable` — the relay's NIP-11 `self` key could not be
//!   read, so no ref state can be told from an owner's claim;
//! - `no-ref-observation` — the relay has signed no ref state for the
//!   repository, or its newest one names no delivery ref;
//! - `ref-observation-mismatch` — the newest relay-signed ref state names the
//!   delivery ref at another commit than `--commit`;
//! - `observed-by-mismatch` — the `--observed-by` host result is not at the
//!   commit being bound.
//!
//! A binding of the same criteria at the same commit already on the wire is
//! reported and nothing is republished: bound evidence is immutable, and a
//! second record saying the same thing adds nothing. A binding at a *new*
//! commit, after the branch moved, is a new record; the first stays, and the
//! fold judges the newest (`sequences/ref-observation-rebound/`).

use beekeeper_core::kind::{KIND_GIT_REPO_STATE, KIND_HOST_STEP_RESULT};
use beekeeper_core::project_plan::{Plan, PlanProof};
use beekeeper_core::project_work::{
    ProjectWorkEvent, ProjectWorkEvidenceBound, ProjectWorkEvidenceKind, ProjectWorkEvidenceRef,
};
use beekeeper_core::project_work_autobind::{
    already_bound, newest_ref_observation, RefObservationMissing,
};
use beekeeper_sdk::project_work::build_project_work_evidence_bound;
use serde_json::{json, Value};

use super::{
    decode_rows, envelope_session, print_example, read_failed, report_existing, BoundDeclaration,
    GitPlans, PlanSource, SessionContext, WorkBindRefArgs, WorkWire,
};
use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::validate_lower_hex64;

/// `bee sessions work bind ref`.
///
/// # Errors
/// [`CliError::Usage`] for a refusal named in the module doc,
/// [`CliError::NotFound`] when the relay holds no usable observation, and a
/// read error naming the read that failed.
pub async fn cmd_bind_ref(client: &BuzzClient, args: &WorkBindRefArgs) -> Result<(), CliError> {
    if let Some(label) = &args.envelope.example {
        return print_example("bind ref", label);
    }
    let session = envelope_session(client, &args.envelope).await?;
    let plans = GitPlans(args.envelope.agents_repo.clone());
    bind_ref_with(client, session, args, &plans).await
}

/// The ref bind once the session is resolved, over the wire seam.
pub(super) async fn bind_ref_with(
    wire: &impl WorkWire,
    session: SessionContext,
    args: &WorkBindRefArgs,
    plans: &impl PlanSource,
) -> Result<(), CliError> {
    let expected = args
        .commit
        .as_deref()
        .map(|commit| well_formed_commit("--commit", commit))
        .transpose()?;
    if let Some(completion) = &args.completion {
        validate_lower_hex64("--completion", completion)?;
    }
    if let Some(observed_by) = &args.observed_by {
        validate_lower_hex64("--observed-by", observed_by)?;
    }
    // The kind check needs only the kind; the id is found below, after the
    // plan says which repository and ref to look at.
    let probe = [ProjectWorkEvidenceRef {
        kind: ProjectWorkEvidenceKind::RefObservation,
        event_id: String::new(),
    }];
    let relay_self = session.relay_self.clone();
    let bound =
        BoundDeclaration::resolve(wire, session, &args.envelope, Some(&probe), plans).await?;
    require_only_git_ref(&bound.plan, &args.envelope.criteria)?;
    let delivery_ref = bound.plan.delivery_ref.clone();
    if let Some(named) = &args.git_ref {
        if named != &delivery_ref {
            return Err(CliError::Usage(format!(
                "ref-not-delivery-ref: --ref {named} is not this plan's delivery_ref \
                 {delivery_ref}; the plan names the ref a delivery is judged at"
            )));
        }
    }
    let relay_self = relay_self.ok_or_else(|| {
        CliError::Other(
            "relay-self-unavailable: the relay's NIP-11 self key could not be read, so no ref \
             state can be told apart from an owner's claim about its own branch"
                .into(),
        )
    })?;
    let repository = bound.plan.code_repository.clone();
    let (observation, observed) =
        newest_observation(wire, &relay_self, &repository, &delivery_ref).await?;
    if let Some(expected) = &expected {
        if expected != &observed {
            return Err(CliError::Usage(format!(
                "ref-observation-mismatch: the newest relay-signed ref state for {repository} \
                 ({}) names {delivery_ref} at {observed}, not {expected}",
                head12(&observation)
            )));
        }
    }
    let corroboration = match &args.observed_by {
        Some(result_id) => Some(corroborate(wire, result_id, &observed).await?),
        None => None,
    };

    let criteria = &args.envelope.criteria;
    let body = ProjectWorkEvidenceBound {
        declaration_ref: bound.declaration_ref.clone(),
        criterion_ids: criteria.clone(),
        artifact_commit: observed.clone(),
        evidence_refs: vec![ProjectWorkEvidenceRef {
            kind: ProjectWorkEvidenceKind::RefObservation,
            event_id: observation.clone(),
        }],
        completion_ref: args.completion.clone(),
    };
    // The same predicate the host's automatic binding applies: a binding of
    // these criteria at this commit by ref observation, whoever signed it.
    if let Some(event_id) = already_bound(&bound.records, &body) {
        return report_existing(&event_id, "ref binding");
    }
    let builder = build_project_work_evidence_bound(&bound.session.envelope(), body)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event_id = wire.publish(builder, "a ref binding").await?;
    let corroborated = corroboration
        .map(|id| format!("; host result {} ran at the same commit", head12(&id)))
        .unwrap_or_default();
    println!(
        "{}",
        json!({"event_id": event_id, "accepted": true, "republished": true,
               "ref_observation": observation, "artifact_commit": observed,
               "message": format!(
                   "bound {} to {delivery_ref} at {} observed by the relay in {}{corroborated}",
                   criteria.join(", "), head12(&observed), head12(&observation))})
    );
    Ok(())
}

/// The first twelve characters of an id, for a message.
fn head12(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

/// A full 40- or 64-hex commit, lowercased.
fn well_formed_commit(flag: &str, commit: &str) -> Result<String, CliError> {
    let commit = commit.to_ascii_lowercase();
    if (commit.len() == 40 || commit.len() == 64)
        && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(commit)
    } else {
        Err(CliError::Usage(format!(
            "malformed-commit: {flag} is a full 40- or 64-hex commit; got {commit:?}"
        )))
    }
}

/// Every named criterion must be a `git-ref` proof: a ref observation proves
/// nothing about a review or an action.
fn require_only_git_ref(plan: &Plan, criteria: &[String]) -> Result<(), CliError> {
    for id in criteria {
        let proof = plan
            .criteria
            .iter()
            .find(|criterion| &criterion.id == id)
            .map(|criterion| &criterion.proof);
        if !matches!(proof, Some(PlanProof::GitRef)) {
            return Err(CliError::Usage(format!(
                "evidence-kind-mismatch: criterion {id:?} is not a git-ref proof; a ref \
                 observation answers only git-ref criteria (use `bind evidence` for the rest)"
            )));
        }
    }
    Ok(())
}

/// The newest relay-signed kind:30618 for `repository`, and the commit it
/// names at `delivery_ref`.
///
/// The filter asks the relay for its own rows, and the rows are checked
/// again here: an owner-signed 30618 is a claim, never an observation.
async fn newest_observation(
    wire: &impl WorkWire,
    relay_self: &str,
    repository: &str,
    delivery_ref: &str,
) -> Result<(String, String), CliError> {
    let rows = wire
        .query_events(
            json!({"kinds": [KIND_GIT_REPO_STATE], "#d": [repository], "authors": [relay_self]}),
            None,
        )
        .await
        .map_err(|error| read_failed(&format!("kind:30618 ref state for {repository}"), error))?;
    let states = decode_rows(rows, "kind:30618 ref state")?;
    // The one lookup the host's automatic binding uses too, so the two can
    // never disagree about which ref state is the observation.
    match newest_ref_observation(&states, relay_self, repository, delivery_ref) {
        Ok(observed) => Ok((observed.event_id, observed.commit)),
        Err(RefObservationMissing::NoState) => Err(CliError::NotFound(format!(
            "no-ref-observation: the relay has signed no ref state for {repository}; nothing \
             observed {delivery_ref} there"
        ))),
        Err(RefObservationMissing::NoDeliveryRef { state_id }) => Err(CliError::NotFound(format!(
            "no-ref-observation: the newest relay-signed ref state for {repository} ({}) names \
             no {delivery_ref}",
            head12(&state_id)
        ))),
    }
}

/// Check that a kind:46023 host result ran at `commit`; returns its id.
async fn corroborate(
    wire: &impl WorkWire,
    result_id: &str,
    commit: &str,
) -> Result<String, CliError> {
    let rows = wire
        .query_events(
            json!({"ids": [result_id], "kinds": [KIND_HOST_STEP_RESULT]}),
            Some(1),
        )
        .await
        .map_err(|error| read_failed(&format!("the host result {result_id}"), error))?;
    let events: Vec<ProjectWorkEvent> = decode_rows(rows, "kind:46023 host result")?;
    let Some(result) = events.first() else {
        return Err(CliError::NotFound(format!(
            "unreadable-evidence: the relay served no host result with id {result_id}"
        )));
    };
    let head_sha = serde_json::from_str::<Value>(&result.content)
        .ok()
        .and_then(|content| {
            content
                .get("headSha")?
                .as_str()
                .map(str::to_ascii_lowercase)
        });
    match head_sha {
        Some(head) if head == commit => Ok(result.id.clone()),
        head => Err(CliError::Usage(format!(
            "observed-by-mismatch: host result {result_id} ran at {}, not {commit}",
            head.as_deref().unwrap_or("no recorded headSha")
        ))),
    }
}
