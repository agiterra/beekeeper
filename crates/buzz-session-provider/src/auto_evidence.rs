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
//! - **It binds only at echo time, with a bounded re-read.** When the relay
//!   serves this key no ref state at all for the repository, it reads again
//!   up to [`RefReadRetry::attempts`] times, [`RefReadRetry::interval`]
//!   apart, before the wake — unless the relay serves this key no kind:30621
//!   for the project either, which is the private-project gate rather than
//!   lag, and no re-read changes it. If the ref state has not caught up with
//!   the commit the verify ran at, nothing is bound and the wake says why;
//!   the lead binds with the CLI as before.
//! - **It reports the read, not a conclusion.** Control run 6
//!   (kettle-control-6, 2026-09-24) reported "the relay has observed no
//!   refs/heads/main" while three relay-signed kind:30618 for the repository
//!   existed: the relay withholds a private project's repository events from
//!   keys off its roster, and this host's key was not on it. The wake now
//!   carries the query it ran, the rows it got and when ([`RefStateRead`]).
//! - **At most once per result.** Custody of a result id is written to disk
//!   before any read, exactly as the wake ledger does it; a crash between the
//!   claim and the publish costs this one automatic binding, and the lead's
//!   CLI path is unchanged. A record already on the wire, whoever signed it,
//!   is never republished.

use std::path::PathBuf;
use std::time::Duration;

use buzz_core::host_step::{HostStepDisposition, HostStepExited};
use buzz_core::kind::{KIND_GIT_REPO_STATE, KIND_PROJECT, KIND_PROJECT_WORK_RECORD};
use buzz_core::project_plan::parse_plan;
use buzz_core::project_work::{
    validate_project_work_envelope, ProjectWorkEvent, ProjectWorkEvidenceBound, ProjectWorkPlanRef,
};
use buzz_core::project_work_autobind::{
    action_criteria, auto_bindings, current_declaration, green_head, newest_ref_observation,
    HostResultFacts, RefObservationMissing,
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
    /// The kind:30618 read this decision made, as run: filter, rows, time.
    /// Absent when the decision stopped before reading ref state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_read: Option<RefStateRead>,
}

/// The facts of one kind:30618 read: the filter as sent, what came back and
/// when. What `skipped` says about ref state is rendered from this, so the
/// wake states what was read rather than what it was taken to mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefStateRead {
    /// The filter's `kinds`.
    pub kinds: Vec<u32>,
    /// The filter's `#d`: the repository.
    pub d: String,
    /// The filter's single `authors` entry: the witnessed relay identity.
    pub author: String,
    /// Rows the relay answered on the last read.
    pub rows: usize,
    /// Of those, rows that decoded as events with a valid signature.
    pub verified: usize,
    /// When the last read was sent, RFC 3339 UTC.
    pub read_at: String,
    /// Reads made, the last included.
    pub attempts: u32,
    /// Seconds between reads, when there was more than one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    /// Whether the relay served this key the project's kind:30621 — read
    /// only after an empty ref-state read; `None` when not read or unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_served: Option<bool>,
}

impl RefStateRead {
    /// `0 rows for kinds=[30618] d=<repo> author=<12 hex>… at <time>`.
    pub fn sentence(&self) -> String {
        let kinds = self
            .kinds
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut text = format!(
            "{} rows for kinds=[{kinds}] d={} author={}… at {}",
            self.rows,
            self.d,
            self.author.get(..8).unwrap_or(&self.author),
            self.read_at
        );
        if self.verified != self.rows {
            text.push_str(&format!(" ({} with a valid signature)", self.verified));
        }
        if self.attempts > 1 {
            text.push_str(&format!(", the last of {} reads", self.attempts));
            if let Some(secs) = self.interval_secs {
                text.push_str(&format!(" {secs}s apart"));
            }
        }
        text
    }
}

/// How often an empty ref-state read is repeated before the wake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefReadRetry {
    /// Reads in total, the first included; at least one is always made.
    pub attempts: u32,
    /// The pause between reads.
    pub interval: Duration,
}

impl Default for RefReadRetry {
    fn default() -> Self {
        Self {
            attempts: 3,
            interval: Duration::from_secs(2),
        }
    }
}

/// One kind:30618 read as the relay answered it.
#[derive(Debug, Clone, Default)]
pub(crate) struct RefStateRows {
    /// Rows in the relay's answer, before any check.
    pub rows: usize,
    /// The signature-valid rows.
    pub states: Vec<ProjectWorkEvent>,
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
    async fn ref_states(&self, repository: &str) -> Result<RefStateRows, String>;
    /// Whether the relay serves this key the project's kind:30621 at
    /// `project_ref` (`30621:<owner>:<d>`).
    async fn project_served(&self, project_ref: &str) -> Result<bool, String>;
    /// Wait between two ref-state reads.
    async fn pause(&self, interval: Duration);
}

/// Everything one decision is made from, handed over by the caller.
///
/// Owned, not borrowed: this is exactly the shape a spawned, `'static` task
/// needs (ledger 257 — the ref-state re-read must not block the provider
/// loop), and a decision costs one clone of a keypair either way.
pub(crate) struct AutoEvidenceInput {
    pub keys: Keys,
    pub relay_self: String,
    pub envelope: ProjectWorkEnvelope,
    pub result: HostResultFacts,
    pub retry: RefReadRetry,
}

/// A decision: the summary for the wake and the signed records to queue.
#[derive(Debug, Clone)]
pub struct Prepared {
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
pub(crate) async fn prepare(reads: &impl AutoEvidenceReads, input: &AutoEvidenceInput) -> Prepared {
    let signer = input.keys.public_key().to_hex();
    let mut summary = AutoEvidenceSummary {
        host_result: input.result.result_event_id.clone(),
        ref_state: None,
        signed_by: signer.clone(),
        bound: Vec::new(),
        skipped: None,
        ref_read: None,
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
    let (read, observed) =
        match read_ref_state(reads, input, &plan.code_repository, &plan.delivery_ref).await {
            Ok(outcome) => outcome,
            Err(error) => return skip(summary, format!("ref state unreadable: {error}")),
        };
    let sentence = read.sentence();
    let project_served = read.project_served;
    summary.ref_read = Some(read);
    let observation = match observed {
        Ok(observation) => observation,
        Err(RefObservationMissing::NoState) => {
            let mut why = sentence;
            if project_served == Some(false) {
                why.push_str(&format!(
                    "; the relay also serves this key ({}…) no kind:{KIND_PROJECT} for {} (a \
                     private project withholds its repositories' ref state from keys off its \
                     roster)",
                    signer.get(..8).unwrap_or(&signer),
                    input.envelope.project_ref
                ));
            }
            why.push_str("; bind with the CLI");
            return skip(summary, why);
        }
        Err(RefObservationMissing::NoDeliveryRef { state_id }) => {
            return skip(
                summary,
                format!(
                    "{sentence}; the newest relay-signed ref state ({}) names no {}; bind with \
                     the CLI",
                    state_id.get(..12).unwrap_or(&state_id),
                    plan.delivery_ref
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
        match sign_evidence(&input.keys, &input.envelope, binding.body) {
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

/// Read the relay's ref state for `repository`, again while it names no
/// state at all, or while the newest state it does serve does not yet name
/// `delivery_ref` — the realistic lag shape, a push whose ref-state record
/// has not caught up with the commit the verify ran at.
///
/// Stops early once a relay-signed state naming `delivery_ref` is served, or
/// when the relay does not serve this key the project's kind:30621 either:
/// that is the private-project gate, and waiting does not open it. The gate
/// check only ever runs against a bare `NoState` — once any state exists the
/// project is plainly served, so there is nothing to check.
async fn read_ref_state(
    reads: &impl AutoEvidenceReads,
    input: &AutoEvidenceInput,
    repository: &str,
    delivery_ref: &str,
) -> Result<
    (
        RefStateRead,
        Result<buzz_core::project_work_autobind::RefObservation, RefObservationMissing>,
    ),
    String,
> {
    let attempts = input.retry.attempts.max(1);
    let mut project_served = None;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let read_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let answer = reads.ref_states(repository).await?;
        let observed =
            newest_ref_observation(&answer.states, &input.relay_self, repository, delivery_ref);
        let no_state = matches!(observed, Err(RefObservationMissing::NoState));
        let unresolved =
            no_state || matches!(observed, Err(RefObservationMissing::NoDeliveryRef { .. }));
        if no_state && attempt == 1 {
            project_served = reads.project_served(&input.envelope.project_ref).await.ok();
        }
        let done = !unresolved || attempt >= attempts || project_served == Some(false);
        if done {
            let read = RefStateRead {
                kinds: vec![KIND_GIT_REPO_STATE],
                d: repository.to_owned(),
                author: input.relay_self.to_ascii_lowercase(),
                rows: answer.rows,
                verified: answer.states.len(),
                read_at,
                attempts: attempt,
                interval_secs: (attempt > 1).then_some(input.retry.interval.as_secs()),
                project_served,
            };
            return Ok((read, observed));
        }
        reads.pause(input.retry.interval).await;
    }
}

/// The production reads: the relay for records, authority and ref state;
/// this host's agents clone (or the seat's sibling clone) for the plan.
struct RelayReads {
    rest: buzz_acp::relay::RestClient,
    relay_self: String,
    scope: crate::team_wake::WakeScope,
    clones: Vec<crate::agents_checkout::AgentsRepoRecord>,
}

impl AutoEvidenceReads for RelayReads {
    async fn provider_may_bind(&self, provider: &str) -> Result<bool, String> {
        let snapshot =
            crate::team_wake::fetch_verified_snapshot(&self.rest, &self.relay_self, &self.scope)
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
            &self.rest,
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

    async fn ref_states(&self, repository: &str) -> Result<RefStateRows, String> {
        use nostr::{Alphabet, Filter, Kind, PublicKey, SingleLetterTag};
        let author = PublicKey::from_hex(&self.relay_self).map_err(|error| error.to_string())?;
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
        Ok(RefStateRows {
            rows: rows.len(),
            states: rows
                .iter()
                .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
                .filter(|event| event.verify().is_ok())
                .map(|event| ProjectWorkEvent::from(&event))
                .collect(),
        })
    }

    async fn project_served(&self, project_ref: &str) -> Result<bool, String> {
        use nostr::{Alphabet, Filter, Kind, PublicKey, SingleLetterTag};
        let mut parts = project_ref.splitn(3, ':');
        let (Some(kind), Some(owner), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("{project_ref} is not a project coordinate"));
        };
        if kind != KIND_PROJECT.to_string() {
            return Err(format!("{project_ref} is not a project coordinate"));
        }
        let owner = PublicKey::from_hex(owner).map_err(|error| error.to_string())?;
        let filter = Filter::new()
            .kind(Kind::Custom(KIND_PROJECT as u16))
            .author(owner)
            .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [d]);
        let rows = self
            .rest
            .query(&[filter])
            .await
            .map_err(|error| error.to_string())?;
        Ok(rows.as_array().is_some_and(|rows| !rows.is_empty()))
    }

    async fn pause(&self, interval: Duration) {
        tokio::time::sleep(interval).await;
    }
}

/// What [`crate::Provider::auto_bind_host_result`] decided about one result.
pub(crate) enum AutoBindOutcome {
    /// Nothing to add: wake now, with no evidence. Every case settled
    /// without a relay read for ref state lands here — the common case, and
    /// the fast one, unchanged since before ledger 257.
    Ready,
    /// A decision was handed to a task spawned off the provider loop;
    /// nothing is bound or woken yet. The loop's own
    /// [`crate::session::SessionEvent::AutoEvidenceReady`] arm finishes the
    /// job — publishing what was signed and sending the wake, carrying the
    /// summary — once that task reports back.
    Pending,
}

impl crate::Provider {
    /// Bind what a relay-echoed host result proves, before its wake is sent.
    ///
    /// [`AutoBindOutcome::Ready`] when there is nothing to say: no
    /// witnessed relay, a replay of a result already answered, a run no seat
    /// on this host triggered, or a result that is not green. Otherwise the
    /// relay reads a decision needs — cheap, but potentially several seconds
    /// under a bounded ref-state re-read (ledger 257) — run off this loop, and
    /// [`AutoBindOutcome::Pending`] says so.
    ///
    /// Infallible, like the wake it precedes: nothing here may stop this
    /// provider from serving turns, and every refusal is a logged sentence.
    pub(crate) async fn auto_bind_host_result(&mut self, event: &Event) -> AutoBindOutcome {
        let Some(relay_self) = self.relay_self.clone() else {
            return AutoBindOutcome::Ready;
        };
        let Ok(exited) = crate::host_result_wake::verify_exited(event, &relay_self) else {
            return AutoBindOutcome::Ready;
        };
        let result_id = exited.result_event_id.to_ascii_lowercase();
        if self.host_result_wakes.already_waked(&result_id)
            || self.host_result_wakes.evidence_checked(&result_id)
        {
            return AutoBindOutcome::Ready;
        }
        let run_key =
            buzz_core::host_step::host_step_d_tag(&exited.result.run_id, &exited.result.step_id);
        let Some(trigger) = self.host_result_wakes.trigger(&run_key).cloned() else {
            return AutoBindOutcome::Ready;
        };
        let facts = host_result_facts(&exited, &trigger.workflow_name);
        // A red result is the seat's to read; there is nothing to bind and
        // the wake already carries the exit.
        if green_head(&facts).is_err() {
            return AutoBindOutcome::Ready;
        }
        let Some(record) = self
            .state
            .sessions()
            .filter(|candidate| {
                !candidate.closed
                    && candidate
                        .actor
                        .as_deref()
                        .is_some_and(|actor| actor.eq_ignore_ascii_case(&trigger.author))
            })
            .max_by_key(|candidate| (candidate.created_at_ms, candidate.generation))
        else {
            return AutoBindOutcome::Ready;
        };
        let (Some(session_ref), Some(genesis_ref), Some(project_ref)) = (
            record.session_ref.clone(),
            record.genesis_ref.clone(),
            record.project_ref.clone(),
        ) else {
            return AutoBindOutcome::Ready;
        };
        if project_ref != trigger.project {
            tracing::info!(
                target: "csp::auto_evidence",
                %result_id,
                "the run's project is not the triggering seat's project; nothing is bound"
            );
            return AutoBindOutcome::Ready;
        }
        let channel_ref = record.channel_id;
        let seat_clone = crate::gate_cwd::resolve(
            self.config.projects_file.as_deref(),
            &record.session_id,
            &record.cwd,
        )
        .present()
        .map(sibling_agents_clone);
        let Some(rest) = self.rest_client.clone() else {
            return AutoBindOutcome::Ready;
        };
        match self.host_result_wakes.claim_evidence(&result_id) {
            Ok(true) => {}
            Ok(false) => return AutoBindOutcome::Ready,
            Err(error) => {
                tracing::error!(
                    target: "csp::auto_evidence",
                    %result_id,
                    "could not take custody of this result; nothing is bound: {error}"
                );
                return AutoBindOutcome::Ready;
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
            rest,
            relay_self: relay_self.clone(),
            scope: crate::team_wake::WakeScope {
                channel_ref,
                session_ref: session_ref.clone(),
                genesis_ref: genesis_ref.clone(),
            },
            clones,
        };
        let input = AutoEvidenceInput {
            keys: self.config.keys.clone(),
            relay_self,
            envelope: ProjectWorkEnvelope {
                channel_ref: channel_ref.to_string(),
                session_ref,
                genesis_ref,
                project_ref,
            },
            result: facts,
            retry: RefReadRetry::default(),
        };
        // Off the loop from here: `prepare` can send several ref-state
        // reads, seconds apart under a re-read (ledger 257), and awaiting
        // that inline would delay every other session's turns and host
        // results for as long as it runs. Nothing is signed or queued until
        // `AutoEvidenceReady` reaches `handle_session_event`; custody of
        // `result_id` is already claimed above, so no other path can act on
        // this result while this task runs.
        let events_tx = self.session_events_tx.clone();
        let boxed_event = Box::new(event.clone());
        tokio::spawn(async move {
            let prepared = prepare(&reads, &input).await;
            let _ = events_tx
                .send(crate::session::SessionEvent::AutoEvidenceReady {
                    event: boxed_event,
                    prepared,
                })
                .await;
        });
        AutoBindOutcome::Pending
    }

    /// Finish an auto-evidence decision the loop deferred to a spawned task:
    /// queue what it signed, log the decision, and wake the seat.
    ///
    /// Called only from [`crate::session::SessionEvent::AutoEvidenceReady`],
    /// exactly where the inline tail of [`Self::auto_bind_host_result`] used
    /// to run before ledger 257 moved the ref-state read off the loop.
    pub(crate) fn finalize_auto_evidence(&mut self, event: &Event, prepared: Prepared) {
        let Prepared {
            mut summary,
            events,
        } = prepared;
        for signed in events {
            let event_id = signed.id.to_hex();
            if let Err(error) = self.outbox.enqueue(
                KIND_PROJECT_WORK_RECORD,
                &format!("auto-evidence:{event_id}"),
                crate::publish::Priority::High,
                signed,
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
            result_id = %summary.host_result,
            ref_state = ?summary.ref_state,
            bound = ?summary.bound,
            skipped = ?summary.skipped,
            "decided the evidence this host result proves"
        );
        self.on_host_step_exited(event, Some(summary));
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
