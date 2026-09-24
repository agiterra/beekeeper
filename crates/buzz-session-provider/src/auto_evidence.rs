//! The host binds the evidence a green delivered verify proves, by mechanism.
//!
//! # The defect this exists for
//!
//! Ledger 257(d), control run 5 (kettle-control-5, 2026-09-24): the host
//! verify came back green at the delivered commit at 01:27:20Z, and the
//! terminal record followed at 01:30:30Z. The 190 s between were a lead turn
//! doing clerical work — read the kind:46023, `bind evidence` for the action
//! criterion, `bind ref` for the git-ref criterion — with four CLI usage
//! errors on the way. Nothing in those binds needs judgement: each is a pure
//! function of facts the relay has already signed
//! ([`buzz_core::project_work_autobind`]).
//!
//! So when the relay echoes a host result (kind:46014) for a run a seat on
//! this host triggered, this host — before it wakes that seat — binds what
//! the result proves, and the wake says what it bound.
//!
//! # Whose key signs, and why that record counts
//!
//! **This provider's own key**, never the seat's. The CLI path signs with the
//! seat's key because the seat is the one acting there. Here the seat did not
//! act, so a record under its key would claim an authorship that did not
//! happen. The work fold admits any signer that `may_lead` the session —
//! founder, active lead seat, or holder of an active `grant-operator`
//! (`buzz_core::project_work_fold`, `signer_not_may_lead`) — and the desktop
//! grants this provider exactly that when it founds a team session (control
//! run 5: kind:44228 seq 1, `grant-operator` to the provider key, before the
//! lead's own grant at seq 2). Before signing, this module checks that
//! standing in the session's verified authority chain and binds nothing
//! without it: a record the fold would exclude is noise, not evidence.
//!
//! The records are the same records `bee sessions work bind evidence` and
//! `bind ref` publish: same builder (`buzz_sdk::project_work`), same
//! validator, no new tag and no new field. What makes them attributable is
//! the signer, and what names their inputs is the evidence pointer each one
//! carries — the kind:46023 id for the action binding, the relay's kind:30618
//! id for the ref binding. The wake additionally names both inputs and every
//! record this host bound.
//!
//! # What it deliberately does not do
//!
//! - **It does not trigger the verify itself.** A kind:46020 is admitted for
//!   the workflow's owner, a project Owner or Collaborator, or the holder of a
//!   live `grant-project-actions` delegation who is the session's active lead
//!   (`buzz-relay` `command_executor.rs` trigger admission,
//!   `project_action_grant.rs`). This provider is none of those, and the
//!   host-result wake routes to the run's trigger author, so a run it
//!   started would wake nobody. Nothing here widens either rule.
//! - **It binds only at echo time.** If the relay's ref state has not yet
//!   caught up with the commit the verify ran at, nothing is bound and the
//!   wake says why; the lead binds with the CLI as before.
//! - **At most once per result.** Custody of a result id is written to disk
//!   before any read, exactly as the wake ledger does it; a crash between the
//!   claim and the publish costs this one automatic binding, and the lead's
//!   CLI path is unchanged. A record already on the wire, whoever signed it,
//!   is never republished.

use std::path::PathBuf;

use buzz_core::host_step::{HostStepDisposition, HostStepExited};
use buzz_core::kind::{KIND_GIT_REPO_STATE, KIND_PROJECT_WORK_RECORD};
use buzz_core::project_plan::parse_plan;
use buzz_core::project_work::{
    validate_project_work_envelope, ProjectWorkEvent, ProjectWorkEvidenceBound, ProjectWorkPlanRef,
};
use buzz_core::project_work_autobind::{
    action_criteria, auto_bindings, current_declaration, green_head, newest_ref_observation,
    HostResultFacts,
};
use buzz_sdk::project_work::{build_project_work_evidence_bound, ProjectWorkEnvelope};
use nostr::{Event, Keys};
use serde::{Deserialize, Serialize};

/// What the host bound for one result, carried in the wake as `autoEvidence`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoEvidenceSummary {
    /// The kind:46023 this was decided from.
    pub host_result: String,
    /// The relay's kind:30618 the delivered commit was read from, when read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_state: Option<String>,
    /// The key that signed every record in `bound`.
    pub signed_by: String,
    /// Records queued by this host, or found already on the wire.
    #[serde(default)]
    pub bound: Vec<AutoEvidenceBinding>,
    /// Why nothing (more) was bound, when that is the answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

/// One `work.evidence_bound` the host queued or found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoEvidenceBinding {
    /// The record's event id.
    pub event_id: String,
    /// The criteria it binds.
    pub criteria: Vec<String>,
    /// `<kind>:<event id>`, exactly as `bind evidence --evidence` takes it.
    pub evidence: String,
    /// `false` when the same binding was already on the wire and nothing
    /// was published.
    pub published: bool,
}

/// The reads one decision needs, behind a seam so the decision is testable
/// without a relay or a clone.
pub(crate) trait AutoEvidenceReads {
    /// Whether `provider` may lead the session: founder or an active
    /// operator grant in its verified authority chain.
    async fn provider_may_bind(&self, provider: &str) -> Result<bool, String>;
    /// Every kind:44249 record of this session.
    async fn work_records(&self) -> Result<Vec<ProjectWorkEvent>, String>;
    /// The plan blob at the declaration's own pinned commit.
    async fn plan_blob(&self, plan_ref: &ProjectWorkPlanRef) -> Result<String, String>;
    /// Relay-signed kind:30618 ref states for `repository`.
    async fn ref_states(&self, repository: &str) -> Result<Vec<ProjectWorkEvent>, String>;
}

/// Everything one decision is made from, handed over by the caller.
pub(crate) struct AutoEvidenceInput<'a> {
    pub keys: &'a Keys,
    pub relay_self: &'a str,
    pub envelope: ProjectWorkEnvelope,
    pub result: HostResultFacts,
}

/// A decision: the summary for the wake and the signed records to queue.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub summary: AutoEvidenceSummary,
    pub events: Vec<Event>,
}

/// The facts [`buzz_core::project_work_autobind`] reads, from one echo and
/// the action name its kind:46013 carried.
pub(crate) fn host_result_facts(exited: &HostStepExited, action_name: &str) -> HostResultFacts {
    let result = &exited.result;
    HostResultFacts {
        result_event_id: exited.result_event_id.to_ascii_lowercase(),
        action_name: action_name.to_owned(),
        step_id: result.step_id.clone(),
        exited: result.disposition == HostStepDisposition::Exited,
        exit_code: result.exit_code,
        dirty: result.dirty,
        head_sha: result.head_sha.clone(),
    }
}

/// Sign one binding under `keys`, through the same builder and the same
/// envelope validator the CLI uses.
pub(crate) fn sign_evidence(
    keys: &Keys,
    envelope: &ProjectWorkEnvelope,
    body: ProjectWorkEvidenceBound,
) -> Result<Event, String> {
    let builder = build_project_work_evidence_bound(envelope, body)
        .map_err(|error| format!("the binding could not be built: {error}"))?;
    let event = builder
        .sign_with_keys(keys)
        .map_err(|error| format!("the binding could not be signed: {error}"))?;
    validate_project_work_envelope(&ProjectWorkEvent::from(&event)).map_err(|refusal| {
        format!("the binding is not a record the relay would admit: {refusal}")
    })?;
    Ok(event)
}

fn evidence_words(body: &ProjectWorkEvidenceBound) -> String {
    body.evidence_refs
        .iter()
        .map(|reference| format!("{}:{}", reference.kind.as_str(), reference.event_id))
        .collect::<Vec<_>>()
        .join(",")
}

/// Decide, and sign, what one clean green result proves.
///
/// Never fails: every refusal is the summary's `skipped` sentence and an
/// empty `events`. Reads are ordered cheapest-refusal first.
pub(crate) async fn prepare(
    reads: &impl AutoEvidenceReads,
    input: &AutoEvidenceInput<'_>,
) -> Prepared {
    let signer = input.keys.public_key().to_hex();
    let mut summary = AutoEvidenceSummary {
        host_result: input.result.result_event_id.clone(),
        ref_state: None,
        signed_by: signer.clone(),
        bound: Vec::new(),
        skipped: None,
    };
    let skip = |mut summary: AutoEvidenceSummary, why: String| {
        summary.skipped = Some(why);
        Prepared {
            summary,
            events: Vec::new(),
        }
    };
    if let Err(not_green) = green_head(&input.result) {
        return skip(summary, not_green.to_string());
    }
    match reads.provider_may_bind(&signer).await {
        Ok(true) => {}
        Ok(false) => {
            return skip(
                summary,
                "this host's key is neither the session's founder nor an operator grantee, so \
                 the work fold would exclude a record it signed; bind with the CLI"
                    .into(),
            )
        }
        Err(error) => {
            return skip(
                summary,
                format!("the session's authority chain could not be read: {error}"),
            )
        }
    }
    let records = match reads.work_records().await {
        Ok(records) => records,
        Err(error) => return skip(summary, format!("work records unreadable: {error}")),
    };
    let (declaration_ref, declared) = match current_declaration(&records) {
        Ok(current) => current,
        Err(why) => return skip(summary, why),
    };
    let plan = match reads.plan_blob(&declared.plan_ref).await {
        Ok(text) => match parse_plan(text.as_bytes()) {
            Ok(plan) => plan,
            Err(refusal) => {
                return skip(summary, format!("the adopted plan is invalid: {refusal}"))
            }
        },
        Err(error) => {
            return skip(
                summary,
                format!(
                    "plan {}@{} unavailable on this host: {error}",
                    declared.plan_ref.path,
                    declared.plan_ref.commit.get(..12).unwrap_or_default()
                ),
            )
        }
    };
    // Before the ref read: a plan with no criterion this action proves binds
    // nothing at all, its git-ref criterion included.
    if action_criteria(&plan, &input.result.action_name, &input.result.step_id).is_empty() {
        return skip(
            summary,
            format!(
                "no active plan criterion is proved by action {:?} step {:?}",
                input.result.action_name, input.result.step_id
            ),
        );
    }
    let states = match reads.ref_states(&plan.code_repository).await {
        Ok(states) => states,
        Err(error) => return skip(summary, format!("ref state unreadable: {error}")),
    };
    let observation = match newest_ref_observation(
        &states,
        input.relay_self,
        &plan.code_repository,
        &plan.delivery_ref,
    ) {
        Ok(observation) => observation,
        Err(missing) => {
            return skip(
                summary,
                format!(
                    "the relay has observed no {} for {}: {missing:?}",
                    plan.delivery_ref, plan.code_repository
                ),
            )
        }
    };
    summary.ref_state = Some(observation.event_id.clone());
    let bindings = match auto_bindings(
        &plan,
        &declaration_ref,
        &records,
        &input.result,
        &observation,
    ) {
        Ok(bindings) => bindings,
        Err(why) => return skip(summary, why.to_string()),
    };
    let mut events = Vec::new();
    let mut failures = Vec::new();
    for binding in bindings {
        let criteria = binding.body.criterion_ids.clone();
        let evidence = evidence_words(&binding.body);
        if let Some(existing) = binding.existing {
            summary.bound.push(AutoEvidenceBinding {
                event_id: existing,
                criteria,
                evidence,
                published: false,
            });
            continue;
        }
        match sign_evidence(input.keys, &input.envelope, binding.body) {
            Ok(event) => {
                summary.bound.push(AutoEvidenceBinding {
                    event_id: event.id.to_hex(),
                    criteria,
                    evidence,
                    published: true,
                });
                events.push(event);
            }
            Err(error) => failures.push(format!("{}: {error}", criteria.join(","))),
        }
    }
    if !failures.is_empty() {
        summary.skipped = Some(failures.join("; "));
    }
    Prepared { summary, events }
}

/// The production reads: the relay for records, authority and ref state;
/// this host's agents clone (or the seat's sibling clone) for the plan.
struct RelayReads<'a> {
    rest: &'a buzz_acp::relay::RestClient,
    relay_self: &'a str,
    scope: crate::team_wake::WakeScope,
    clones: Vec<crate::agents_checkout::AgentsRepoRecord>,
}

impl AutoEvidenceReads for RelayReads<'_> {
    async fn provider_may_bind(&self, provider: &str) -> Result<bool, String> {
        let snapshot =
            crate::team_wake::fetch_verified_snapshot(self.rest, self.relay_self, &self.scope)
                .await
                .map_err(|error| error.to_string())?;
        Ok(crate::team_wake::provider_may_wake(
            provider,
            &snapshot.founder_pubkey,
            &snapshot.authority,
        ))
    }

    async fn work_records(&self) -> Result<Vec<ProjectWorkEvent>, String> {
        let events = crate::context_projector::query_complete_kind_partition(
            self.rest,
            self.scope.channel_ref,
            KIND_PROJECT_WORK_RECORD,
        )
        .await
        .map_err(|error| error.to_string())?;
        Ok(events
            .iter()
            .map(ProjectWorkEvent::from)
            .filter(|record| record.tag_value("d") == Some(self.scope.session_ref.as_str()))
            .collect())
    }

    async fn plan_blob(&self, plan_ref: &ProjectWorkPlanRef) -> Result<String, String> {
        let mut reasons = Vec::new();
        for clone in &self.clones {
            match crate::agents_plan_blob::read_blob_at_commit(
                clone,
                &plan_ref.commit,
                &plan_ref.path,
            )
            .await
            {
                Ok(text) => return Ok(text),
                Err(error) => reasons.push(format!("{}: {error}", clone.path.display())),
            }
        }
        if reasons.is_empty() {
            return Err("this host has no clone of the project's agents repository".into());
        }
        Err(reasons.join("; "))
    }

    async fn ref_states(&self, repository: &str) -> Result<Vec<ProjectWorkEvent>, String> {
        use nostr::{Alphabet, Filter, Kind, PublicKey, SingleLetterTag};
        let author = PublicKey::from_hex(self.relay_self).map_err(|error| error.to_string())?;
        let filter = Filter::new()
            .kind(Kind::Custom(KIND_GIT_REPO_STATE as u16))
            .author(author)
            .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [repository]);
        let rows = self
            .rest
            .query(&[filter])
            .await
            .map_err(|error| error.to_string())?;
        let rows = rows
            .as_array()
            .ok_or_else(|| "the relay did not answer a JSON array".to_owned())?;
        // Signature-checked here: the author filter is the relay's promise,
        // the signature is the proof.
        Ok(rows
            .iter()
            .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
            .filter(|event| event.verify().is_ok())
            .map(|event| ProjectWorkEvent::from(&event))
            .collect())
    }
}

impl crate::Provider {
    /// Bind what a relay-echoed host result proves, before its wake is sent.
    ///
    /// `None` when there is nothing to say: no witnessed relay, a replay of a
    /// result already answered, a run no seat on this host triggered, or a
    /// result that is not green. Otherwise the summary the wake carries.
    ///
    /// Infallible, like the wake it precedes: nothing here may stop this
    /// provider from serving turns, and every refusal is a logged sentence.
    pub(crate) async fn auto_bind_host_result(
        &mut self,
        event: &Event,
    ) -> Option<AutoEvidenceSummary> {
        let relay_self = self.relay_self.clone()?;
        let exited = crate::host_result_wake::verify_exited(event, &relay_self).ok()?;
        let result_id = exited.result_event_id.to_ascii_lowercase();
        if self.host_result_wakes.already_waked(&result_id)
            || self.host_result_wakes.evidence_checked(&result_id)
        {
            return None;
        }
        let run_key =
            buzz_core::host_step::host_step_d_tag(&exited.result.run_id, &exited.result.step_id);
        let trigger = self.host_result_wakes.trigger(&run_key)?.clone();
        let facts = host_result_facts(&exited, &trigger.workflow_name);
        // A red result is the seat's to read; there is nothing to bind and
        // the wake already carries the exit.
        green_head(&facts).ok()?;
        let record = self
            .state
            .sessions()
            .filter(|candidate| {
                !candidate.closed
                    && candidate
                        .actor
                        .as_deref()
                        .is_some_and(|actor| actor.eq_ignore_ascii_case(&trigger.author))
            })
            .max_by_key(|candidate| (candidate.created_at_ms, candidate.generation))?;
        let (Some(session_ref), Some(genesis_ref), Some(project_ref)) = (
            record.session_ref.clone(),
            record.genesis_ref.clone(),
            record.project_ref.clone(),
        ) else {
            return None;
        };
        if project_ref != trigger.project {
            tracing::info!(
                target: "csp::auto_evidence",
                %result_id,
                "the run's project is not the triggering seat's project; nothing is bound"
            );
            return None;
        }
        let channel_ref = record.channel_id;
        let seat_clone = crate::gate_cwd::resolve(
            self.config.projects_file.as_deref(),
            &record.session_id,
            &record.cwd,
        )
        .present()
        .map(sibling_agents_clone);
        let rest = self.rest_client.clone()?;
        match self.host_result_wakes.claim_evidence(&result_id) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(error) => {
                tracing::error!(
                    target: "csp::auto_evidence",
                    %result_id,
                    "could not take custody of this result; nothing is bound: {error}"
                );
                return None;
            }
        }
        let mut clones: Vec<crate::agents_checkout::AgentsRepoRecord> =
            crate::commands::ProjectsFile::load(self.config.projects_file.as_deref())
                .agents_repos
                .get(&project_ref)
                .cloned()
                .into_iter()
                .collect();
        clones.extend(seat_clone);
        let reads = RelayReads {
            rest: &rest,
            relay_self: &relay_self,
            scope: crate::team_wake::WakeScope {
                channel_ref,
                session_ref: session_ref.clone(),
                genesis_ref: genesis_ref.clone(),
            },
            clones,
        };
        let keys = self.config.keys.clone();
        let input = AutoEvidenceInput {
            keys: &keys,
            relay_self: &relay_self,
            envelope: ProjectWorkEnvelope {
                channel_ref: channel_ref.to_string(),
                session_ref,
                genesis_ref,
                project_ref,
            },
            result: facts,
        };
        let Prepared {
            mut summary,
            events,
        } = prepare(&reads, &input).await;
        for event in events {
            let event_id = event.id.to_hex();
            if let Err(error) = self.outbox.enqueue(
                KIND_PROJECT_WORK_RECORD,
                &format!("auto-evidence:{event_id}"),
                crate::publish::Priority::High,
                event,
            ) {
                summary.bound.retain(|binding| binding.event_id != event_id);
                let note = format!("{event_id} was signed but could not be queued: {error}");
                summary.skipped = Some(match summary.skipped.take() {
                    Some(earlier) => format!("{earlier}; {note}"),
                    None => note,
                });
            }
        }
        tracing::info!(
            target: "csp::auto_evidence",
            %result_id,
            ref_state = ?summary.ref_state,
            bound = ?summary.bound,
            skipped = ?summary.skipped,
            "decided the evidence this host result proves"
        );
        Some(summary)
    }
}

/// `<worktree>-agents`, the clone a team seat has beside its worktree.
fn sibling_agents_clone(worktree: &std::path::Path) -> crate::agents_checkout::AgentsRepoRecord {
    let mut name = worktree.as_os_str().to_owned();
    name.push("-agents");
    crate::agents_checkout::AgentsRepoRecord {
        path: PathBuf::from(name),
        ref_name: "refs/heads/main".into(),
        url: None,
    }
}

#[cfg(test)]
#[path = "auto_evidence_tests.rs"]
mod tests;
