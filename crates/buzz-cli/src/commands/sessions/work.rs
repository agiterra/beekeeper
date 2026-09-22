//! `bee sessions work` — adopt a committed plan as a session's contract, bind
//! assignments and evidence to its criteria, and read what remains.
//!
//! The normative contract is `conformance/project-work/README.md` § (d),
//! restated in `docs/nips/NIP-PW.md`. Three rules this file exists to keep:
//!
//! 1. **Adoption is atomic.** A declaration that references a plan, an action
//!    or a repository which did not resolve is a contract nobody can read, so
//!    everything is resolved *before* anything is signed. A failed compile
//!    signs nothing and exits 1.
//! 2. **The blob is read at the commit.** `git show <commit>:<path>`, never
//!    the working copy and never the fetched tip. With no commit, `validate`
//!    reads the working file and **says** it is uncommitted and cannot be
//!    adopted.
//! 3. **A refused or failed read is an error, never an empty result.**
//!    `status` names the read that failed. Incomplete coverage, by contrast,
//!    is exit 0: it is the fact the caller asked for.
//!
//! Coverage and the 44244 mission terminal are **two rows**, never merged
//! (README § (c) "coverageComplete, and the 44244 terminal").

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use buzz_core::coding_session_team_transaction::CodingSessionTeamFoldContext;
use buzz_core::kind::{
    KIND_CODING_SESSION_GOAL, KIND_CODING_SESSION_TEAM_TRANSACTION, KIND_GIT_REPO_ANNOUNCEMENT,
    KIND_GIT_REPO_STATE, KIND_HOST_STEP_RESULT, KIND_NIP29_GROUP_METADATA,
    KIND_PROJECT_PACK_SOURCE, KIND_PROJECT_WORK_RECORD, KIND_WORKFLOW_HOST_STEP_EXITED,
    KIND_WORKFLOW_HOST_STEP_REQUESTED,
};
use buzz_core::project_plan::{
    check_plan_adoptable, parse_plan, validate_plan_path, Plan, PlanProof,
};
use buzz_core::project_work::{
    validate_project_work_envelope, ProjectWorkAssignmentBound, ProjectWorkBody,
    ProjectWorkDeclared, ProjectWorkEvent, ProjectWorkEvidenceBound, ProjectWorkEvidenceKind,
    ProjectWorkEvidenceRef, ProjectWorkPlanRef,
};
use buzz_core::project_work_fold::{fold_work, WorkActionDefinition, WorkProjection};
use buzz_core::project_work_inputs::{assemble_fold_inputs, RawAuthorityContext, RawWorkInputs};
use buzz_sdk::project_work::{
    build_project_work_assignment_bound, build_project_work_declared,
    build_project_work_evidence_bound, project_work_payload, ProjectWorkEnvelope,
};
use clap::{Args, Subcommand};
use nostr::EventBuilder;
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};

/// How many events one `status` read will page through before it discloses
/// that it stopped. A bound that is never disclosed is the lie this avoids.
const READ_LIMIT: u32 = 5_000;

/// The relay operations `bee sessions work` performs, behind one seam.
///
/// Not an abstraction for its own sake: **adoption is atomic**, and the only
/// way to prove "a failing action compile signs nothing" is to count the
/// publishes a run made. A stub implementing this trait answers reads from a
/// fixture and records every publish, so that property is a test rather than
/// a claim.
pub(crate) trait WorkWire {
    /// Every event matching `filter`, paged, bounded by `limit`.
    async fn query_events(&self, filter: Value, limit: Option<u32>)
        -> Result<Vec<Value>, CliError>;
    /// Sign and submit one event, returning its id.
    async fn publish(&self, builder: EventBuilder, what: &str) -> Result<String, CliError>;
    /// The caller's own pubkey, lowercase hex.
    fn caller_pubkey(&self) -> String;
}

impl WorkWire for BuzzClient {
    async fn query_events(
        &self,
        filter: Value,
        limit: Option<u32>,
    ) -> Result<Vec<Value>, CliError> {
        match limit {
            Some(limit) => self.query_paginated(filter, limit).await,
            None => self.query_all(filter).await,
        }
    }

    async fn publish(&self, builder: EventBuilder, what: &str) -> Result<String, CliError> {
        let event = sign_work_record(self, builder)?;
        let event_id = event.id.to_hex();
        let raw = self.submit_event(event).await?;
        crate::commands::parse_write_response(&raw, &format!("{what} was already published"))?;
        Ok(event_id)
    }

    fn caller_pubkey(&self) -> String {
        self.keys().public_key().to_hex().to_ascii_lowercase()
    }
}

/// Sign one kind:44249 record the way the publish path signs it.
///
/// Named and separated from [`WorkWire::publish`] so the bytes the CLI puts on
/// the wire are reachable from a unit test without a relay: the defect this
/// seam exists for was invisible to every test that stubbed the wire, because
/// the stub never saw the signature step at all.
fn sign_work_record(client: &BuzzClient, builder: EventBuilder) -> Result<nostr::Event, CliError> {
    // `sign_event_unchecked`, never `sign_event`. The latter injects this
    // client's NIP-OA `auth` tag into every event it signs, and a managed
    // agent always has one (`BUZZ_AUTH_TAG`), so a seat's adopt carried a
    // seventh tag onto a record whose contract fixes six and hive refused it
    // with `tag-count` (ledger 237). Membership delegation still reaches the
    // relay: `submit_event` sends the same tag in the `x-auth-tag` header,
    // which is where `POST /events` reads it — the same rule the 44220/44221
    // publishers already keep (`crew_cmds.rs` module doc).
    let event = client.sign_event_unchecked(builder)?;
    // Then judge the bytes with the relay's own validator, before the round
    // trip. A writer that can only learn it is non-conforming from a 400 is
    // a writer whose conformance nobody tests; this says the same words the
    // relay would, here, naming the tag at fault.
    validate_project_work_envelope(&ProjectWorkEvent::from(&event)).map_err(|refusal| {
        CliError::Other(format!(
            "this is not a work record the relay will admit, so nothing was published: {refusal}"
        ))
    })?;
    Ok(event)
}

/// UUID v5 namespace for a session's derived work ids.
///
/// A fixed constant, never generated: the whole point is that two runs of the
/// same command on two machines derive the same id.
const WORK_ID_NAMESPACE: uuid::Uuid =
    uuid::Uuid::from_u128(0x7f2c_9d41_5a83_4e16_9b02_c7d5_e1a8_0f34);

/// The `workId` an initial adoption uses, derived from stable inputs alone.
///
/// **Retry safety is the whole reason** (ledger 213, finding 8). A random
/// id meant that a publish whose response was lost minted a *second* work
/// identity on the operator's next attempt, leaving two sets of obligations
/// for one plan. The id is a pure function of five caller-supplied facts —
/// the project, the session, the plan's repository coordinate, its path and
/// the plan id in its frontmatter.
///
/// **The commit is deliberately not an input.** The same plan at a new commit
/// is an *amendment of the same work*, which is exactly what `--supersedes`
/// is for; hashing the commit would make every revision new work.
///
/// No clock and no randomness take part: a "deterministic" id that hashed a
/// now-relative value broke this same promise once before, and the fix is
/// that only stable caller-supplied inputs are hashed.
#[must_use]
pub(super) fn derive_work_id(
    project_ref: &str,
    session_ref: &str,
    plan_repository: &str,
    plan_path: &str,
    plan_id: &str,
) -> String {
    let name = format!("{project_ref}\n{session_ref}\n{plan_repository}\n{plan_path}\n{plan_id}");
    uuid::Uuid::new_v5(&WORK_ID_NAMESPACE, name.as_bytes()).to_string()
}

/// `bee sessions work <verb>`: the work-record surface of NIP-PW.
#[derive(Subcommand)]
pub enum SessionWorkCmd {
    /// Validate a `beekeeper-plan/v1` file and resolve its action proofs
    #[command(
        after_help = "Examples:\n  bee sessions work validate --plan plans/kettle.md --agents-repo ../agents --commit HEAD\n  bee sessions work validate --plan /tmp/draft.md\n\nOffline except for git."
    )]
    Validate(WorkValidateArgs),
    /// Adopt a committed plan as this session's contract (one `work.declared`)
    Adopt(WorkAdoptArgs),
    /// Bind a 44244 assignment, or evidence, to a declaration's criteria
    #[command(subcommand)]
    Bind(WorkBindCmd),
    /// What remains, who owes it and what proves it
    Status(WorkStatusArgs),
}

/// Flags of `bee sessions work validate`.
#[derive(Args)]
pub struct WorkValidateArgs {
    /// Plan path. Relative to the agents repository root when `--commit` is given
    #[arg(long)]
    pub plan: String,
    /// Agents repository checkout to read the blob from
    #[arg(long = "agents-repo")]
    pub agents_repo: Option<String>,
    /// Commit to read the blob at. Omit to validate the uncommitted working file
    #[arg(long)]
    pub commit: Option<String>,
}

/// Flags of `bee sessions work adopt`.
#[derive(Args)]
pub struct WorkAdoptArgs {
    /// Plan path inside the agents repository, e.g. `plans/kettle.md`
    #[arg(long, required_unless_present = "example")]
    pub plan: Option<String>,
    /// Full agents commit the plan is adopted at
    #[arg(long, required_unless_present = "example")]
    pub commit: Option<String>,
    /// Agents repository checkout
    #[arg(long = "agents-repo", required_unless_present = "example")]
    pub agents_repo: Option<String>,
    /// Channel UUID the session was published into
    #[arg(long, required_unless_present = "example")]
    pub channel: Option<String>,
    /// Umbrella session UUID
    #[arg(
        long = "session-ref",
        alias = "session",
        required_unless_present = "example"
    )]
    pub session_ref: Option<String>,
    /// Work id. Omit to mint one; give it to amend, which requires `--supersedes`
    #[arg(long = "work-id")]
    pub work_id: Option<String>,
    /// A declaration this one supersedes. Repeatable; all heads on a resolution
    #[arg(long)]
    pub supersedes: Vec<String>,
    /// The 44244 decision event that authorized this adoption
    #[arg(long)]
    pub decision: Option<String>,
    /// The actor who owes the outcome. Defaults to the caller
    #[arg(long)]
    pub responsible: Option<String>,
    /// Print a complete example body and exit, offline
    #[arg(long, num_args = 0..=1, default_missing_value = "default")]
    pub example: Option<String>,
}

/// `bee sessions work bind <what>`.
#[derive(Subcommand)]
pub enum WorkBindCmd {
    /// Bind a 44244 assignment to criteria: who owes them
    Assignment(WorkBindAssignmentArgs),
    /// Bind evidence to criteria at one artifact commit: what proves them
    Evidence(WorkBindEvidenceArgs),
}

/// Flags every bind verb shares.
#[derive(Args, Clone)]
pub struct WorkBindEnvelopeArgs {
    /// Channel UUID the session was published into
    #[arg(long, required_unless_present = "example")]
    pub channel: Option<String>,
    /// Umbrella session UUID
    #[arg(
        long = "session-ref",
        alias = "session",
        required_unless_present = "example"
    )]
    pub session_ref: Option<String>,
    /// The `work.declared` event id this binds to
    #[arg(long, required_unless_present = "example")]
    pub declaration: Option<String>,
    /// Criterion ids, comma-separated. Repeatable
    #[arg(long, value_delimiter = ',', required_unless_present = "example")]
    pub criteria: Vec<String>,
    /// Agents repository checkout the declaration's plan is read from
    #[arg(long = "agents-repo")]
    pub agents_repo: Option<String>,
    /// Print a complete example body and exit, offline
    #[arg(long, num_args = 0..=1, default_missing_value = "default")]
    pub example: Option<String>,
}

/// Flags of `bee sessions work bind assignment`.
#[derive(Args)]
pub struct WorkBindAssignmentArgs {
    #[command(flatten)]
    pub envelope: WorkBindEnvelopeArgs,
    /// The kind:44244 assignment event id
    #[arg(long, required_unless_present = "example")]
    pub assignment: Option<String>,
    /// An earlier binding this supersedes
    #[arg(long)]
    pub replaces: Option<String>,
}

/// Flags of `bee sessions work bind evidence`.
#[derive(Args)]
pub struct WorkBindEvidenceArgs {
    #[command(flatten)]
    pub envelope: WorkBindEnvelopeArgs,
    /// The code commit the evidence is about, 40 or 64 hex
    #[arg(long, required_unless_present = "example")]
    pub artifact: Option<String>,
    /// `<kind>:<event id>` pairs, comma-separated. Repeatable
    #[arg(long, value_delimiter = ',', required_unless_present = "example")]
    pub evidence: Vec<String>,
    /// The kind:44244 `mission.completed` this coverage was computed for
    #[arg(long)]
    pub completion: Option<String>,
}

/// Flags of `bee sessions work status`.
#[derive(Args)]
pub struct WorkStatusArgs {
    /// Channel UUID the session was published into
    #[arg(long)]
    pub channel: String,
    /// Umbrella session UUID
    #[arg(long = "session-ref", alias = "session")]
    pub session_ref: Option<String>,
    /// Restrict the projection to one work id
    #[arg(long = "work-id", alias = "work")]
    pub work_id: Option<String>,
    /// Agents repository checkout the plan blobs are read from
    #[arg(long = "agents-repo", alias = "plans-from")]
    pub agents_repo: Option<String>,
}

// ── `--example`, dispatched before the key gate ─────────────────────────────

/// The `--example` request a `bee sessions work` verb carries, if any.
///
/// Answered in `lib.rs` ahead of the key gate, exactly as lane 182's bodies
/// are: a seat learning this wire needs neither an identity nor a relay.
#[must_use]
pub fn example_request(command: &SessionWorkCmd) -> Option<(&'static str, String)> {
    match command {
        SessionWorkCmd::Adopt(args) => args.example.clone().map(|label| ("adopt", label)),
        SessionWorkCmd::Bind(WorkBindCmd::Assignment(args)) => args
            .envelope
            .example
            .clone()
            .map(|label| ("bind assignment", label)),
        SessionWorkCmd::Bind(WorkBindCmd::Evidence(args)) => args
            .envelope
            .example
            .clone()
            .map(|label| ("bind evidence", label)),
        _ => None,
    }
}

/// A 64-hex placeholder that is obviously a placeholder, as lane 182's are.
const PLACEHOLDER_EVENT_ID: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// A second, for the records that name two events.
const PLACEHOLDER_EVENT_ID_2: &str =
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
/// A 40-hex placeholder git object id.
const PLACEHOLDER_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn example_envelope() -> ProjectWorkEnvelope {
    ProjectWorkEnvelope {
        channel_ref: "22222222-3333-4444-8555-666666666666".to_owned(),
        session_ref: "11111111-2222-4333-8444-555555555555".to_owned(),
        genesis_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        project_ref: format!("30621:{}:kettle", "1e".repeat(32)),
    }
}

/// The complete record one verb writes, serialized from the constructed
/// value — never a hand-typed string.
fn example_body(verb: &str) -> Option<ProjectWorkBody> {
    match verb {
        "adopt" => Some(ProjectWorkBody::Declared(ProjectWorkDeclared {
            work_id: "9d0f0f0f-1111-4222-8333-444444444444".to_owned(),
            goal_ref: PLACEHOLDER_EVENT_ID.to_owned(),
            decision_ref: None,
            responsible_actor: "1e".repeat(32),
            plan_ref: ProjectWorkPlanRef {
                repository: format!("30617:{}:kettle-beekeeper-agents", "1e".repeat(32)),
                commit: "ab".repeat(20),
                path: "plans/kettle.md".to_owned(),
            },
            supersedes: Vec::new(),
        })),
        "bind assignment" => Some(ProjectWorkBody::AssignmentBound(
            ProjectWorkAssignmentBound {
                declaration_ref: PLACEHOLDER_EVENT_ID.to_owned(),
                criterion_ids: vec!["cli-behaviour".to_owned()],
                assignment_ref: PLACEHOLDER_EVENT_ID_2.to_owned(),
                replaces_binding: None,
            },
        )),
        "bind evidence" => Some(ProjectWorkBody::EvidenceBound(ProjectWorkEvidenceBound {
            declaration_ref: PLACEHOLDER_EVENT_ID.to_owned(),
            criterion_ids: vec!["cli-behaviour".to_owned()],
            artifact_commit: PLACEHOLDER_SHA.to_owned(),
            evidence_refs: vec![ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Verdict,
                event_id: PLACEHOLDER_EVENT_ID_2.to_owned(),
            }],
            completion_ref: None,
        })),
        _ => None,
    }
}

/// Print one verb's example record and exit 0, offline.
///
/// # Errors
/// [`CliError::Usage`] when the verb has no example, and [`CliError::Other`]
/// when the constructed value does not render — a bug here, not a caller's.
pub fn print_example(verb: &str, requested: &str) -> Result<(), CliError> {
    if requested != "default" {
        return Err(CliError::Usage(format!(
            "`bee sessions work {verb}` offers one example, named \"default\""
        )));
    }
    let body = example_body(verb)
        .ok_or_else(|| CliError::Usage(format!("`bee sessions work {verb}` has no example")))?;
    let payload = project_work_payload(&example_envelope(), body);
    let rendered = serde_json::to_string_pretty(&payload)
        .map_err(|error| CliError::Other(format!("example did not render: {error}")))?;
    eprintln!(
        "kind:44249 {} example: the complete record `bee sessions work {verb}` signs, with \
         placeholder ids",
        payload.record_type.as_str()
    );
    println!("{rendered}");
    Ok(())
}

// ── git: the blob at the commit, never the working copy ────────────────────

/// Run one git command in `dir` and return its stdout, or a refusal naming
/// the exact command that failed.
fn git(dir: &str, args: &[&str]) -> Result<String, CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| {
            CliError::Usage(format!(
                "git -C {dir} {} could not run: {error}",
                args.join(" ")
            ))
        })?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "git -C {dir} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Resolve `--commit` to a full object id in `dir`.
fn resolve_commit(dir: &str, commit: &str) -> Result<String, CliError> {
    Ok(git(dir, &["rev-parse", &format!("{commit}^{{commit}}")])?
        .trim()
        .to_ascii_lowercase())
}

/// The bytes of `path` at `commit`, read with `git show`.
fn blob_at_commit(dir: &str, commit: &str, path: &str) -> Result<String, CliError> {
    git(dir, &["show", &format!("{commit}:{path}")])
}

/// Whether a commit is reachable from the relay's copy of the repository.
///
/// A plan must be **pushed before it is adopted**: a declaration pinned to a
/// commit nobody else can fetch is a contract nobody can read. Both checks
/// are named in the refusal, so the remedy is never a guess.
fn commit_is_published(dir: &str, commit: &str, relay_tips: &[String]) -> (bool, String) {
    let remote = git(dir, &["branch", "-r", "--contains", commit]).unwrap_or_default();
    let remote_branches: Vec<&str> = remote
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if !remote_branches.is_empty() {
        return (
            true,
            format!("reachable from {}", remote_branches.join(", ")),
        );
    }
    for tip in relay_tips {
        if tip == commit {
            return (true, format!("the relay's ref state names {tip}"));
        }
        if git(dir, &["merge-base", "--is-ancestor", commit, tip]).is_ok() {
            return (
                true,
                format!("an ancestor of the relay's ref state at {tip}"),
            );
        }
    }
    (
        false,
        format!(
            "no remote-tracking branch contains {commit} (git branch -r --contains) and the \
             relay's ref state for this repository names {} — push the plan before adopting it",
            if relay_tips.is_empty() {
                "nothing".to_owned()
            } else {
                relay_tips.join(", ")
            }
        ),
    )
}

// ── the plan, and the actions its criteria name ────────────────────────────

/// A plan resolved at a commit, with every `action` criterion compiled.
struct ResolvedPlan {
    plan: Plan,
    bytes: usize,
    /// Compiled definitions keyed by action name, as the fold takes them.
    actions: BTreeMap<String, WorkActionDefinition>,
    /// Why an `action` criterion did not resolve at this commit.
    ///
    /// **A missing action blocks adoption, not drafting** (README § (a)), so
    /// this is a list `validate` reports and `adopt` refuses on — not an
    /// error that hides the rest of the plan from someone still writing it.
    unresolved_actions: Vec<String>,
}

/// Compile the actions a plan's criteria name from `actions.yml` at the same
/// commit, with the publication compiler.
///
/// The hash comes from the compiler, never from an event: evidence that
/// nominated its own expected hash would prove nothing.
fn compile_plan_actions(
    plan: &Plan,
    project_ref: &str,
    actions_yml: Option<&str>,
) -> Result<BTreeMap<String, WorkActionDefinition>, CliError> {
    let mut compiled = BTreeMap::new();
    let needed: BTreeSet<(&str, &str)> = plan
        .criteria
        .iter()
        .filter_map(|criterion| match &criterion.proof {
            PlanProof::Action { name, step } => Some((name.as_str(), step.as_str())),
            _ => None,
        })
        .collect();
    if needed.is_empty() {
        return Ok(compiled);
    }
    let Some(text) = actions_yml else {
        return Err(CliError::Usage(format!(
            "unresolved-action: this plan's criteria name {} action(s) and actions.yml could not \
             be read at the same commit",
            needed.len()
        )));
    };
    let entries = buzz_workflow::parse_actions_yml(text, project_ref)
        .map_err(|error| CliError::Usage(format!("unresolved-action: actions.yml: {error}")))?;
    for (name, step) in needed {
        let entry = entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| {
                CliError::Usage(format!(
                "unresolved-action: actions.yml at this commit defines no action named {name:?}"
            ))
            })?;
        let steps: Vec<String> = entry.def.steps.iter().map(|item| item.id.clone()).collect();
        if !steps.iter().any(|known| known == step) {
            return Err(CliError::Usage(format!(
                "unresolved-action-step: action {name:?} defines no step {step:?} (it defines {})",
                steps.join(", ")
            )));
        }
        compiled.insert(
            name.to_owned(),
            WorkActionDefinition {
                definition_hash: entry.hash.clone(),
                steps,
            },
        );
    }
    Ok(compiled)
}

/// Read and parse a plan, and compile the actions it names.
fn resolve_plan(
    dir: Option<&str>,
    commit: Option<&str>,
    path: &str,
    project_ref: &str,
) -> Result<ResolvedPlan, CliError> {
    let (bytes, actions_yml) = match (dir, commit) {
        (Some(dir), Some(commit)) => (
            blob_at_commit(dir, commit, path)?,
            blob_at_commit(dir, commit, buzz_workflow::ACTIONS_YML).ok(),
        ),
        _ => {
            let file = dir.map_or_else(
                || std::path::PathBuf::from(path),
                |dir| std::path::Path::new(dir).join(path),
            );
            let text = std::fs::read_to_string(&file)
                .map_err(|error| CliError::Usage(format!("{}: {error}", file.display())))?;
            let sibling = dir
                .map(|dir| std::path::Path::new(dir).join(buzz_workflow::ACTIONS_YML))
                .and_then(|path| std::fs::read_to_string(path).ok());
            (text, sibling)
        }
    };
    let plan =
        parse_plan(bytes.as_bytes()).map_err(|refusal| CliError::Usage(refusal.to_string()))?;
    let (actions, unresolved_actions) =
        match compile_plan_actions(&plan, project_ref, actions_yml.as_deref()) {
            Ok(actions) => (actions, Vec::new()),
            Err(error) => (BTreeMap::new(), vec![error.to_string()]),
        };
    Ok(ResolvedPlan {
        plan,
        bytes: bytes.len(),
        actions,
        unresolved_actions,
    })
}

/// `bee sessions work validate`: is this a plan, and does everything it names
/// resolve at the commit it would be adopted at?
///
/// # Errors
/// [`CliError::Usage`] (exit 1) for an unreadable or refused plan.
pub fn cmd_validate(args: &WorkValidateArgs) -> Result<(), CliError> {
    validate_plan_path(&args.plan).ok();
    let dir = args.agents_repo.as_deref();
    let resolved_commit = match (&args.commit, dir) {
        (Some(commit), Some(dir)) => Some(resolve_commit(dir, commit)?),
        (Some(_), None) => {
            return Err(CliError::Usage(
                "--commit names a commit in a repository: pass --agents-repo <dir> too".into(),
            ))
        }
        (None, _) => None,
    };
    // The project coordinate only binds the compiled definition's `project`
    // field; validation is offline and has no session, so a syntactically
    // valid placeholder keeps the compiler's own rules in force.
    let project_ref = format!("30621:{}:validate", "0".repeat(64));
    let outcome = resolve_plan(dir, resolved_commit.as_deref(), &args.plan, &project_ref);
    match outcome {
        Ok(resolved) => {
            let adoptable = check_plan_adoptable(&resolved.plan, &args.plan)
                .map_err(|refusal| refusal.to_string())
                .and_then(|()| {
                    resolved
                        .unresolved_actions
                        .first()
                        .map_or(Ok(()), |error| Err(error.clone()))
                });
            let mut answer = json!({
                "valid": true,
                "planId": resolved.plan.id,
                "schema": resolved.plan.schema,
                "status": resolved.plan.status.as_str(),
                "codeRepository": resolved.plan.code_repository,
                "deliveryRef": resolved.plan.delivery_ref,
                "criteria": resolved.plan.criteria.iter().map(|criterion| json!({
                    "id": criterion.id,
                    "proof": criterion.proof,
                })).collect::<Vec<Value>>(),
                "retiredCriteria": resolved.plan.retired_criteria,
                "bytes": resolved.bytes,
                "commit": resolved_commit,
                "actionDefinitions": resolved.actions,
                "adoptable": adoptable.is_ok(),
                "adoptableReason": adoptable.err(),
                "unresolvedActions": resolved.unresolved_actions,
                "source": if resolved_commit.is_some() { "commit" } else { "working-copy" },
            });
            if resolved_commit.is_none() {
                answer["uncommitted"] = json!(true);
                answer["message"] = json!(
                    "this is the working file, not a committed blob: it cannot be adopted until \
                     it is committed and pushed, and `adopt` reads `git show <commit>:<path>`"
                );
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&answer)
                    .map_err(|error| CliError::Other(error.to_string()))?
            );
            Ok(())
        }
        Err(error) => {
            let message = error.to_string();
            let code = message
                .split(':')
                .next()
                .unwrap_or("invalid")
                .trim()
                .to_owned();
            println!(
                "{}",
                json!({"valid": false, "errors": [{"code": code, "message": message}]})
            );
            Err(CliError::Usage(message))
        }
    }
}

// ── the session this work belongs to ───────────────────────────────────────

/// Everything a write or a read needs about the session, fetched once.
pub(super) struct SessionContext {
    channel: String,
    session_ref: String,
    genesis_ref: String,
    project_ref: String,
    context: CodingSessionTeamFoldContext,
    relay_self: Option<String>,
}

impl SessionContext {
    fn envelope(&self) -> ProjectWorkEnvelope {
        ProjectWorkEnvelope {
            channel_ref: self.channel.clone(),
            session_ref: self.session_ref.clone(),
            genesis_ref: self.genesis_ref.clone(),
            project_ref: self.project_ref.clone(),
        }
    }
}

/// The project coordinate a channel belongs to, from its kind:39000 metadata.
async fn channel_project(wire: &impl WorkWire, channel: &str) -> Result<String, CliError> {
    let rows = wire
        .query_events(
            json!({"kinds": [KIND_NIP29_GROUP_METADATA], "#d": [channel]}),
            None,
        )
        .await?;
    rows.iter()
        .filter_map(|row| {
            row.get("tags")?.as_array()?.iter().find_map(|tag| {
                let parts = tag.as_array()?;
                (parts.first()?.as_str()? == "project")
                    .then(|| parts.get(1)?.as_str().map(str::to_owned))
                    .flatten()
            })
        })
        .find_map(|value| buzz_core::kind::normalize_project_coordinate(&value))
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "channel {channel} names no project in its kind:39000 metadata, and a work \
                 record is scoped to one project"
            ))
        })
}

/// Read the session's genesis, authority and project, and the relay's key.
pub(super) async fn session_context(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
) -> Result<SessionContext, CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    let genesis_events = super::fetch_channel_events(
        client,
        channel,
        &[buzz_core::kind::KIND_CODING_SESSION_GENESIS],
    )
    .await?;
    let genesis_ref = super::crew::resolve_umbrella_genesis(&genesis_events, session_ref)?;
    let authority = super::operations_reads::fetch_session_authority(
        client,
        channel,
        session_ref,
        &genesis_ref,
    )
    .await?;
    let project_ref = channel_project(client, channel).await?;
    let relay_self = super::operations_authority::fetch_trusted_relay_self(client)
        .await
        .ok();
    Ok(SessionContext {
        channel: channel.to_owned(),
        session_ref: session_ref.to_owned(),
        genesis_ref,
        project_ref,
        context: authority.context,
        relay_self,
    })
}

/// The project's agents-repository coordinate, `30617:<owner>:<id>`.
///
/// The signed kind:30624 pack source is the project's own statement of which
/// repository holds its roles and plans, so it answers first. Falling back to
/// the announcements, a name that resolves to two repositories is **refused
/// by name** rather than decided by a rule nobody wrote down.
async fn agents_repository(wire: &impl WorkWire, project_ref: &str) -> Result<String, CliError> {
    let rows = wire
        .query_events(
            json!({"kinds": [KIND_PROJECT_PACK_SOURCE], "#d": [project_ref]}),
            None,
        )
        .await?;
    let from_pack_source = rows.iter().find_map(|row| {
        let coordinate = json_tag(row, "repo")?;
        coordinate.starts_with("30617:").then_some(coordinate)
    });
    if let Some(coordinate) = from_pack_source {
        return Ok(coordinate);
    }
    let announcements = wire
        .query_events(
            json!({"kinds": [KIND_GIT_REPO_ANNOUNCEMENT]}),
            Some(READ_LIMIT),
        )
        .await?;
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for row in &announcements {
        let Some(id) = json_tag(row, "d") else {
            continue;
        };
        let owner = row
            .get("pubkey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let names_project = json_tag(row, "project")
            .and_then(|value| buzz_core::kind::normalize_project_coordinate(&value))
            .is_some_and(|value| value == project_ref);
        if names_project && id.ends_with(crate::commands::packs::AGENTS_REPO_SUFFIX) {
            candidates.insert(format!("30617:{owner}:{id}"));
        }
    }
    let mut found = candidates.into_iter();
    match (found.next(), found.next()) {
        (Some(one), None) => Ok(one),
        (Some(first), Some(second)) => Err(CliError::Usage(format!(
            "ambiguous-agents-repository: {project_ref} has more than one agents repository \
             announced — {first}, {second}{} — name the one to adopt from by publishing the \
             project's kind:30624 pack source",
            if found.next().is_some() {
                " and others"
            } else {
                ""
            }
        ))),
        _ => Err(CliError::NotFound(format!(
            "no agents repository is announced for {project_ref}: a plan lives in \
             `<slug>-beekeeper-agents` (spec § 4.11), and its kind:30617 announcement is what \
             the declaration pins"
        ))),
    }
}

/// The value of an event's first tag named `name`.
fn json_tag(row: &Value, name: &str) -> Option<String> {
    row.get("tags")?.as_array()?.iter().find_map(|tag| {
        let parts = tag.as_array()?;
        (parts.first()?.as_str()? == name)
            .then(|| parts.get(1)?.as_str().map(str::to_owned))
            .flatten()
    })
}

/// Every kind:44249 record of one session, as the fold's event type.
async fn fetch_work_records(
    wire: &impl WorkWire,
    channel: &str,
    session_ref: &str,
) -> Result<(Vec<ProjectWorkEvent>, bool), CliError> {
    let rows = wire
        .query_events(
            json!({"kinds": [KIND_PROJECT_WORK_RECORD], "#h": [channel], "#d": [session_ref]}),
            Some(READ_LIMIT),
        )
        .await
        .map_err(|error| read_failed("kind:44249 work records", error))?;
    let truncated = rows.len() as u32 >= READ_LIMIT;
    Ok((decode_rows(rows, "kind:44249 work records")?, truncated))
}

/// A read that failed is named, never swallowed into an empty result.
fn read_failed(what: &str, error: CliError) -> CliError {
    match error {
        CliError::Usage(message) => CliError::Other(format!("reading {what} failed: {message}")),
        other => CliError::Other(format!("reading {what} failed: {other}")),
    }
}

/// Decode relay rows into the plain event type both folds read.
fn decode_rows(rows: Vec<Value>, what: &str) -> Result<Vec<ProjectWorkEvent>, CliError> {
    rows.into_iter()
        .map(|row| {
            serde_json::from_value(row).map_err(|error| {
                CliError::Other(format!("relay returned a malformed {what}: {error}"))
            })
        })
        .collect()
}

// ── adopt ──────────────────────────────────────────────────────────────────

/// `bee sessions work adopt`: one `work.declared`, or nothing at all.
///
/// # Errors
/// [`CliError::Usage`] when anything the declaration would reference does not
/// resolve — and in that case **no event is signed**.
pub async fn cmd_adopt(client: &BuzzClient, args: &WorkAdoptArgs) -> Result<(), CliError> {
    if let Some(label) = &args.example {
        return print_example("adopt", label);
    }
    let (Some(channel), Some(session_ref)) = (args.channel.as_deref(), args.session_ref.as_deref())
    else {
        return Err(CliError::Usage(
            "adopt needs --channel and --session-ref".into(),
        ));
    };
    let session = session_context(client, channel, session_ref).await?;
    adopt_with(client, &session, args).await
}

/// The whole of adoption once the session is resolved, over the wire seam.
///
/// Everything is resolved before anything is signed, and the only `publish`
/// call in this function is the last statement but one — which is what makes
/// "a failed compile signs nothing" testable rather than asserted.
async fn adopt_with(
    wire: &impl WorkWire,
    session: &SessionContext,
    args: &WorkAdoptArgs,
) -> Result<(), CliError> {
    let (Some(plan_path), Some(commit), Some(dir)) = (
        args.plan.as_deref(),
        args.commit.as_deref(),
        args.agents_repo.as_deref(),
    ) else {
        return Err(CliError::Usage(
            "adopt needs --plan, --commit and --agents-repo".into(),
        ));
    };
    validate_plan_path(plan_path).map_err(|refusal| CliError::Usage(refusal.to_string()))?;
    for id in &args.supersedes {
        validate_lower_hex64("--supersedes", id)?;
    }
    if let Some(decision) = &args.decision {
        validate_lower_hex64("--decision", decision)?;
    }

    // ── resolve everything, before anything is signed ──────────────────────
    let repository_coordinate = agents_repository(wire, &session.project_ref).await?;
    let full_commit = resolve_commit(dir, commit)?;
    if full_commit.len() != 40 && full_commit.len() != 64 {
        return Err(CliError::Usage(format!(
            "--commit must resolve to a full object id; {commit} resolved to {full_commit:?}"
        )));
    }
    let relay_tips = repository_tips(wire, repository_id(&repository_coordinate)).await;
    let (published, how) = commit_is_published(dir, &full_commit, &relay_tips);
    if !published {
        return Err(CliError::Usage(format!("commit-not-published: {how}")));
    }
    let resolved = resolve_plan(
        Some(dir),
        Some(&full_commit),
        plan_path,
        &session.project_ref,
    )?;
    check_plan_adoptable(&resolved.plan, plan_path)
        .map_err(|refusal| CliError::Usage(refusal.to_string()))?;
    // Atomic: every action the plan names resolved at this commit, or
    // nothing is signed.
    if let Some(error) = resolved.unresolved_actions.first() {
        return Err(CliError::Usage(error.clone()));
    }
    let goal_ref = current_goal(wire, session).await?;
    let responsible = args
        .responsible
        .clone()
        .unwrap_or_else(|| wire.caller_pubkey())
        .to_ascii_lowercase();
    validate_lower_hex64("--responsible", &responsible)?;

    let body = ProjectWorkDeclared {
        // Derived, never minted: a lost response must not create a second
        // work identity. `--work-id` stays an explicit override.
        work_id: args.work_id.clone().unwrap_or_else(|| {
            derive_work_id(
                &session.project_ref,
                &session.session_ref,
                &repository_coordinate,
                plan_path,
                &resolved.plan.id,
            )
        }),
        goal_ref,
        decision_ref: args.decision.clone(),
        responsible_actor: responsible,
        plan_ref: ProjectWorkPlanRef {
            repository: repository_coordinate.clone(),
            commit: full_commit.clone(),
            path: plan_path.to_owned(),
        },
        supersedes: args.supersedes.clone(),
    };

    // Idempotence: the same arguments find the declaration they already
    // published and republish nothing.
    let (existing, _) = fetch_work_records(wire, &session.channel, &session.session_ref).await?;
    // The same plan already declared under this work, at another commit or
    // with another body, is an **amendment** — and an amendment names what it
    // supersedes. Checked after the read, because only the wire knows whether
    // this work already exists (ledger 213, finding 8).
    if body.supersedes.is_empty() {
        if let Some(prior) = prior_declaration(&existing, &body) {
            return Err(CliError::Usage(format!(
                "amendment-needs-supersedes: work {} was already declared at {prior} for \
                 {}. Re-adopting the same plan is an amendment of that work, not new work: \
                 re-run with --supersedes {prior} (and every other head, if it forked).",
                body.work_id, body.plan_ref.path
            )));
        }
    }
    if let Some(event_id) = existing_declaration(&existing, &body) {
        println!(
            "{}",
            json!({
                "event_id": event_id, "accepted": true, "republished": false,
                "work_id": body.work_id, "declaration_ref": event_id,
                "message": format!(
                    "this declaration is already on the wire at {event_id}; nothing was published"
                ),
            })
        );
        return Ok(());
    }

    let work_id = body.work_id.clone();
    let builder = build_project_work_declared(&session.envelope(), body)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event_id = wire.publish(builder, "a work declaration").await?;
    println!(
        "{}",
        json!({
            "event_id": event_id, "accepted": true, "republished": true,
            "work_id": work_id, "declaration_ref": event_id,
            "message": format!("adopted {} at {}", resolved.plan.id, &full_commit[..12]),
            "commit_published": how,
            "action_definitions": resolved.actions,
        })
    );
    Ok(())
}

/// The repository id half of a `30617:<owner>:<id>` coordinate.
fn repository_id(coordinate: &str) -> &str {
    coordinate.rsplit(':').next().unwrap_or(coordinate)
}

/// The commits the relay's newest ref state names for a repository.
async fn repository_tips(wire: &impl WorkWire, repository: &str) -> Vec<String> {
    let rows = wire
        .query_events(
            json!({"kinds": [KIND_GIT_REPO_STATE], "#d": [repository]}),
            None,
        )
        .await
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| row.get("tags")?.as_array().cloned())
        .flatten()
        .filter_map(|tag| {
            let parts = tag.as_array()?;
            let name = parts.first()?.as_str()?;
            let value = parts.get(1)?.as_str()?;
            (name.starts_with("refs/") && value.len() == 40).then(|| value.to_ascii_lowercase())
        })
        .collect()
}

/// Every declared body in a record set, with the event id that carried it.
fn declared_bodies(events: &[ProjectWorkEvent]) -> Vec<(String, ProjectWorkDeclared)> {
    events
        .iter()
        .filter_map(|event| {
            let payload: Value = serde_json::from_str(&event.content).ok()?;
            let body: ProjectWorkDeclared =
                serde_json::from_value(payload.get("body")?.clone()).ok()?;
            Some((event.id.clone(), body))
        })
        .collect()
}

/// The declaration this **exact body** was already published as, if any.
///
/// Exact means exact: goal, decision, responsible actor, plan reference and
/// supersedes list, not the three fields the first version compared while
/// calling itself a body match (ledger 213, finding 8).
fn existing_declaration(events: &[ProjectWorkEvent], body: &ProjectWorkDeclared) -> Option<String> {
    declared_bodies(events)
        .into_iter()
        .find(|(_, existing)| existing == body)
        .map(|(event_id, _)| event_id)
}

/// An earlier declaration of this same work that this one would amend.
///
/// Newest first by the order the records arrived, so the refusal names the
/// declaration an operator would actually supersede.
fn prior_declaration(events: &[ProjectWorkEvent], body: &ProjectWorkDeclared) -> Option<String> {
    declared_bodies(events)
        .into_iter()
        .filter(|(_, existing)| existing.work_id == body.work_id && existing != body)
        .map(|(event_id, _)| event_id)
        .next_back()
}

/// The session's current kind:44227 goal, which a declaration is pinned to.
async fn current_goal(wire: &impl WorkWire, session: &SessionContext) -> Result<String, CliError> {
    let rows = wire
        .query_events(
            json!({
                "kinds": [KIND_CODING_SESSION_GOAL],
                "#h": [session.channel],
                "#d": [session.session_ref],
            }),
            None,
        )
        .await
        .map_err(|error| read_failed("kind:44227 goals", error))?;
    let events = decode_rows(rows, "kind:44227 goal")?;
    let raw = RawWorkInputs {
        goal_events: events,
        authority: RawAuthorityContext {
            channel_ref: Some(session.channel.clone()),
            genesis_ref: Some(session.genesis_ref.clone()),
            genesis_event: None,
            founder_pubkey: Some(session.context.founder_pubkey.clone()),
            ..RawAuthorityContext::default()
        },
        ..RawWorkInputs::default()
    };
    assemble_fold_inputs(raw)
        .map_err(|refusal| CliError::Other(refusal.to_string()))?
        .current_goal_ref
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "session {} has no kind:44227 goal, and a declaration is adopted against the \
                 goal it answers",
                session.session_ref
            ))
        })
}

// ── bind ───────────────────────────────────────────────────────────────────

/// `bee sessions work bind assignment`: who owes these criteria.
///
/// # Errors
/// [`CliError::Usage`] when a criterion does not exist under the declaration's
/// plan, or the assignment is not readable on the relay.
pub async fn cmd_bind_assignment(
    client: &BuzzClient,
    args: &WorkBindAssignmentArgs,
) -> Result<(), CliError> {
    if let Some(label) = &args.envelope.example {
        return print_example("bind assignment", label);
    }
    let session = envelope_session(client, &args.envelope).await?;
    let plans = GitPlans(args.envelope.agents_repo.clone());
    bind_assignment_with(client, session, args, &plans).await
}

/// The assignment bind once the session is resolved, over the wire seam.
pub(super) async fn bind_assignment_with(
    wire: &impl WorkWire,
    session: SessionContext,
    args: &WorkBindAssignmentArgs,
    plans: &impl PlanSource,
) -> Result<(), CliError> {
    let bound = BoundDeclaration::resolve(wire, session, &args.envelope, None, plans).await?;
    let Some(assignment) = args.assignment.as_deref() else {
        return Err(CliError::Usage("bind assignment needs --assignment".into()));
    };
    validate_lower_hex64("--assignment", assignment)?;
    bound.require_event(wire, assignment, "assignment").await?;
    if let Some(replaces) = &args.replaces {
        validate_lower_hex64("--replaces", replaces)?;
    }
    let body = ProjectWorkAssignmentBound {
        declaration_ref: bound.declaration_ref.clone(),
        criterion_ids: args.envelope.criteria.clone(),
        assignment_ref: assignment.to_ascii_lowercase(),
        replaces_binding: args.replaces.clone(),
    };
    if let Some(event_id) =
        bound.existing(|existing: &ProjectWorkAssignmentBound| existing == &body)
    {
        return report_existing(&event_id, "assignment binding");
    }
    let builder = build_project_work_assignment_bound(&bound.session.envelope(), body)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event_id = wire.publish(builder, "an assignment binding").await?;
    println!(
        "{}",
        json!({"event_id": event_id, "accepted": true, "republished": true,
               "message": format!("bound assignment {} to {}", &assignment[..8],
                                  args.envelope.criteria.join(", "))})
    );
    Ok(())
}

/// `bee sessions work bind evidence`: what proves these criteria, at which
/// artifact commit.
///
/// # Errors
/// [`CliError::Usage`] for an unknown criterion, an evidence kind the
/// criterion's proof form cannot use, or an unreadable referenced event.
pub async fn cmd_bind_evidence(
    client: &BuzzClient,
    args: &WorkBindEvidenceArgs,
) -> Result<(), CliError> {
    if let Some(label) = &args.envelope.example {
        return print_example("bind evidence", label);
    }
    let session = envelope_session(client, &args.envelope).await?;
    let plans = GitPlans(args.envelope.agents_repo.clone());
    bind_evidence_with(client, session, args, &plans).await
}

/// The evidence bind once the session is resolved, over the wire seam.
pub(super) async fn bind_evidence_with(
    wire: &impl WorkWire,
    session: SessionContext,
    args: &WorkBindEvidenceArgs,
    plans: &impl PlanSource,
) -> Result<(), CliError> {
    let Some(artifact) = args.artifact.as_deref() else {
        return Err(CliError::Usage(
            "bind evidence needs --artifact <sha>".into(),
        ));
    };
    let artifact = artifact.to_ascii_lowercase();
    if !(artifact.len() == 40 || artifact.len() == 64)
        || !artifact.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CliError::Usage(format!(
            "--artifact is a full 40- or 64-hex commit; got {artifact:?}"
        )));
    }
    let mut refs = Vec::new();
    for item in &args.evidence {
        let (kind, event_id) = item.split_once(':').ok_or_else(|| {
            CliError::Usage(format!(
                "--evidence takes <kind>:<event id>; got {item:?} (kinds: report, verdict, \
                 action_result, ref_observation)"
            ))
        })?;
        let kind = ProjectWorkEvidenceKind::from_wire(kind).ok_or_else(|| {
            CliError::Usage(format!(
                "unknown evidence kind {kind:?}: one of report, verdict, action_result, \
                 ref_observation"
            ))
        })?;
        validate_lower_hex64("--evidence", event_id)?;
        refs.push(ProjectWorkEvidenceRef {
            kind,
            event_id: event_id.to_ascii_lowercase(),
        });
    }
    let bound =
        BoundDeclaration::resolve(wire, session, &args.envelope, Some(&refs), plans).await?;
    for reference in &refs {
        bound
            .require_event(wire, &reference.event_id, reference.kind.as_str())
            .await?;
    }
    if let Some(completion) = &args.completion {
        validate_lower_hex64("--completion", completion)?;
    }
    let body = ProjectWorkEvidenceBound {
        declaration_ref: bound.declaration_ref.clone(),
        criterion_ids: args.envelope.criteria.clone(),
        artifact_commit: artifact.clone(),
        evidence_refs: refs,
        completion_ref: args.completion.clone(),
    };
    if let Some(event_id) = bound.existing(|existing: &ProjectWorkEvidenceBound| existing == &body)
    {
        return report_existing(&event_id, "evidence binding");
    }
    let builder = build_project_work_evidence_bound(&bound.session.envelope(), body)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event_id = wire.publish(builder, "an evidence binding").await?;
    println!(
        "{}",
        json!({"event_id": event_id, "accepted": true, "republished": true,
               "message": format!("bound evidence at {} to {}", &artifact[..12],
                                  args.envelope.criteria.join(", "))})
    );
    Ok(())
}

/// A retry finds what it already published and republishes nothing.
fn report_existing(event_id: &str, what: &str) -> Result<(), CliError> {
    println!(
        "{}",
        json!({"event_id": event_id, "accepted": true, "republished": false,
               "message": format!("this {what} is already on the wire at {event_id}; nothing \
                                   was published")})
    );
    Ok(())
}

/// Resolve the session a bind's envelope names.
async fn envelope_session(
    client: &BuzzClient,
    envelope: &WorkBindEnvelopeArgs,
) -> Result<SessionContext, CliError> {
    let (Some(channel), Some(session_ref)) =
        (envelope.channel.as_deref(), envelope.session_ref.as_deref())
    else {
        return Err(CliError::Usage(
            "a bind needs --channel and --session-ref".into(),
        ));
    };
    session_context(client, channel, session_ref).await
}

/// One declaration, its plan, and the session it belongs to — everything a
/// bind checks itself against before it signs.
struct BoundDeclaration {
    session: SessionContext,
    declaration_ref: String,
    records: Vec<ProjectWorkEvent>,
}

impl BoundDeclaration {
    /// Resolve the declaration, read its plan at the pinned commit, and check
    /// every criterion id and evidence kind against it.
    async fn resolve(
        wire: &impl WorkWire,
        session: SessionContext,
        envelope: &WorkBindEnvelopeArgs,
        evidence: Option<&[ProjectWorkEvidenceRef]>,
        plans: &impl PlanSource,
    ) -> Result<Self, CliError> {
        let Some(declaration) = envelope.declaration.as_deref() else {
            return Err(CliError::Usage(
                "a bind needs --channel, --session-ref and --declaration".into(),
            ));
        };
        validate_lower_hex64("--declaration", declaration)?;
        if envelope.criteria.is_empty() {
            return Err(CliError::Usage(
                "a bind names at least one criterion: --criteria <id>[,<id>]".into(),
            ));
        }
        let (records, _) = fetch_work_records(wire, &session.channel, &session.session_ref).await?;
        let declared = records
            .iter()
            .find(|event| event.id == declaration)
            .and_then(|event| {
                let payload: Value = serde_json::from_str(&event.content).ok()?;
                serde_json::from_value::<ProjectWorkDeclared>(payload.get("body")?.clone()).ok()
            })
            .ok_or_else(|| {
                CliError::NotFound(format!(
                    "no work declaration {declaration} in session {}",
                    session.session_ref
                ))
            })?;
        // The plan is read for its criteria; when it cannot be read, the bind
        // says so rather than signing against a list it never saw.
        let plan = read_declaration_plan(&declared, plans)?;
        for id in &envelope.criteria {
            if !plan.criteria.iter().any(|criterion| &criterion.id == id) {
                let retired = plan.retired_criteria.contains(id);
                return Err(CliError::Usage(format!(
                    "unknown-criterion: {id:?} is {} in {} at {} — its active criteria are {}",
                    if retired {
                        "retired"
                    } else {
                        "not a criterion"
                    },
                    declared.plan_ref.path,
                    &declared.plan_ref.commit[..12],
                    plan.criteria
                        .iter()
                        .map(|criterion| criterion.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        if let Some(evidence) = evidence {
            check_evidence_kinds(&plan, &envelope.criteria, evidence)?;
        }
        Ok(Self {
            session,
            declaration_ref: declaration.to_ascii_lowercase(),
            records,
        })
    }

    /// An identical binding already on the wire, if this is a retry.
    fn existing<T>(&self, matches: impl Fn(&T) -> bool) -> Option<String>
    where
        T: serde::de::DeserializeOwned,
    {
        self.records.iter().find_map(|event| {
            let payload: Value = serde_json::from_str(&event.content).ok()?;
            let body: T = serde_json::from_value(payload.get("body")?.clone()).ok()?;
            matches(&body).then(|| event.id.clone())
        })
    }

    /// Refuse a binding that names an event the relay will not show us: a
    /// pointer nobody can follow is not evidence.
    async fn require_event(
        &self,
        wire: &impl WorkWire,
        event_id: &str,
        what: &str,
    ) -> Result<(), CliError> {
        let kinds = match what {
            "ref_observation" => vec![KIND_GIT_REPO_STATE],
            "action_result" => vec![KIND_HOST_STEP_RESULT],
            _ => vec![KIND_CODING_SESSION_TEAM_TRANSACTION],
        };
        let rows = wire
            .query_events(json!({"ids": [event_id], "kinds": kinds}), Some(1))
            .await
            .map_err(|error| read_failed(&format!("the {what} {event_id}"), error))?;
        if rows.is_empty() {
            return Err(CliError::NotFound(format!(
                "unreadable-evidence: the relay served no {what} with id {event_id}; a binding \
                 names events a reader can follow"
            )));
        }
        let _ = &self.session;
        Ok(())
    }
}

/// Read a declaration's plan at its pinned commit.
///
/// A bind checks its criterion ids against the plan the declaration named, so
/// a plan that cannot be read refuses the bind rather than letting it sign
/// against a list nobody saw.
fn read_declaration_plan(
    declared: &ProjectWorkDeclared,
    plans: &impl PlanSource,
) -> Result<Plan, CliError> {
    let bytes = plans.plan(&declared.plan_ref).map_err(|error| {
        CliError::Usage(format!(
            "plan_blob_unavailable: the declaration pins {}@{} and it could not be read \
             (--agents-repo <dir>): {error}",
            declared.plan_ref.path,
            &declared.plan_ref.commit[..12]
        ))
    })?;
    parse_plan(bytes.as_bytes()).map_err(|refusal| CliError::Usage(refusal.to_string()))
}

/// Every evidence kind must fit the proof form of every criterion it is bound
/// to. A verdict does not settle a `git-ref` criterion, and a ref observation
/// does not settle a review.
fn check_evidence_kinds(
    plan: &Plan,
    criteria: &[String],
    evidence: &[ProjectWorkEvidenceRef],
) -> Result<(), CliError> {
    for id in criteria {
        let Some(criterion) = plan.criteria.iter().find(|item| &item.id == id) else {
            continue;
        };
        let wanted = match criterion.proof {
            PlanProof::Review => ProjectWorkEvidenceKind::Verdict,
            PlanProof::Action { .. } => ProjectWorkEvidenceKind::ActionResult,
            PlanProof::GitRef => ProjectWorkEvidenceKind::RefObservation,
        };
        if !evidence.iter().any(|reference| reference.kind == wanted) {
            return Err(CliError::Usage(format!(
                "evidence-kind-mismatch: criterion {id:?} has a {} proof, which is answered by a \
                 {} — this binding carries none",
                proof_name(&criterion.proof),
                wanted.as_str()
            )));
        }
    }
    Ok(())
}

fn proof_name(proof: &PlanProof) -> &'static str {
    match proof {
        PlanProof::Review => "review",
        PlanProof::Action { .. } => "action",
        PlanProof::GitRef => "git-ref",
    }
}

// ── status ─────────────────────────────────────────────────────────────────

/// `bee sessions work status`: the coverage projection, and the 44244 mission
/// state on a separate row.
///
/// # Errors
/// [`CliError::Other`] naming the read that failed. Incomplete coverage is
/// exit 0: it is the fact the caller asked for.
pub async fn cmd_status(
    client: &BuzzClient,
    args: &WorkStatusArgs,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let session_ref = args.session_ref.clone().ok_or_else(|| {
        CliError::Usage("status needs --session-ref <uuid>: coverage is per session".into())
    })?;
    let session = session_context(client, &args.channel, &session_ref).await?;
    let (mut coverage, reads) =
        coverage_for_session(client, &session, args.agents_repo.as_deref()).await?;
    if let Some(work_id) = &args.work_id {
        coverage
            .declarations
            .retain(|declaration| &declaration.work_id == work_id);
        coverage
            .conflicts
            .retain(|conflict| &conflict.work_id == work_id);
    }
    let mission = mission_row(client, &session).await?;
    // The drift sentences travel in the JSON too, beside the coverage they
    // are about: `planDrift` is on every declaration row, and this is the
    // reader-facing rendering of the rows that moved (A10 § 2).
    let plan_drift: Vec<Value> = coverage
        .declarations
        .iter()
        .filter_map(|declaration| {
            plan_drift_line(declaration, &args.channel, &session_ref).map(|line| {
                json!({
                    "workId": declaration.work_id,
                    "declarationRef": declaration.declaration_ref,
                    "declaredCommit": declaration.plan_drift.declared_commit,
                    "currentCommit": declaration.plan_drift.current_commit,
                    "message": line,
                })
            })
        })
        .collect();
    let answer = json!({
        "coverage": coverage,
        "mission": mission,
        "reads": reads,
        "planDrift": plan_drift,
    });
    if matches!(format, crate::OutputFormat::Compact) {
        print_table(&coverage, &mission, &args.channel, &session_ref);
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&answer)
                .map_err(|error| CliError::Other(error.to_string()))?
        );
    }
    Ok(())
}

/// Fetch one session's raw inputs, assemble them and fold them.
///
/// The one path `status` and the completion gate both read coverage through,
/// so the command that *reports* coverage and the command that *checks* it
/// before a terminal can never disagree. Returns the projection and a
/// `reads` object disclosing truncation and every plan blob that could not be
/// read — an unread plan is `unknown` with a reason, never an empty answer.
///
/// # Errors
/// [`CliError::Other`] naming the read that failed; a refused or failed read
/// is never an empty result.
pub(super) async fn coverage_for_session(
    client: &BuzzClient,
    session: &SessionContext,
    agents_repo: Option<&str>,
) -> Result<(WorkProjection, Value), CliError> {
    coverage_with(client, session, &GitPlans(agents_repo.map(str::to_owned))).await
}

/// Where a declaration's plan bytes come from.
///
/// One seam, for one reason: the production reader is `git show <commit>:<path>`
/// in an agents checkout, and a test must be able to fold the frozen sequences
/// without one. Nothing here reads a working copy or a fetched tip.
pub(super) trait PlanSource {
    /// The plan blob at the declaration's pinned commit.
    fn plan(&self, plan_ref: &ProjectWorkPlanRef) -> Result<String, CliError>;
    /// `actions.yml` at that same commit, when it is readable.
    fn actions(&self, plan_ref: &ProjectWorkPlanRef) -> Option<String>;
    /// Definitions this source has already compiled for that commit.
    ///
    /// `None` — the production answer — means "compile them from
    /// [`PlanSource::actions`]". A source that holds compiled definitions
    /// (the frozen fixtures do) answers with them instead, so no second
    /// compiler is written to test the first.
    fn compiled(
        &self,
        _plan_ref: &ProjectWorkPlanRef,
    ) -> Option<BTreeMap<String, WorkActionDefinition>> {
        None
    }
}

/// The production plan source: a local agents checkout, or none at all.
struct GitPlans(Option<String>);

impl PlanSource for GitPlans {
    fn plan(&self, plan_ref: &ProjectWorkPlanRef) -> Result<String, CliError> {
        let Some(dir) = self.0.as_deref() else {
            return Err(CliError::Usage(
                "no --agents-repo was given, so this plan was not read at its commit".into(),
            ));
        };
        blob_at_commit(dir, &plan_ref.commit, &plan_ref.path)
    }

    fn actions(&self, plan_ref: &ProjectWorkPlanRef) -> Option<String> {
        let dir = self.0.as_deref()?;
        blob_at_commit(dir, &plan_ref.commit, buzz_workflow::ACTIONS_YML).ok()
    }
}

/// Fold one session's coverage from a wire and a plan source.
async fn coverage_with(
    client: &impl WorkWire,
    session: &SessionContext,
    plans: &impl PlanSource,
) -> Result<(WorkProjection, Value), CliError> {
    let session_ref = session.session_ref.clone();
    let args_channel = session.channel.clone();
    let (work_events, truncated) = fetch_work_records(client, &args_channel, &session_ref).await?;

    // Plan blobs, read at each declaration's pinned commit. Without a
    // checkout the declarations' criteria read `unknown` with
    // `plan_blob_unavailable` — never `open`, which would say nothing had
    // been done when the truth is that we cannot see the list.
    let mut plan_blobs = BTreeMap::new();
    let mut action_definitions = BTreeMap::new();
    let mut unresolved_plans: Vec<Value> = Vec::new();
    let mut code_repositories: BTreeSet<String> = BTreeSet::new();
    // The agents repository's own ref state, read for `planDrift` (A10): the
    // fold answers `unknown` unless somebody supplies it, and "we did not
    // look" must not read as "the tip is what you pinned".
    let mut agents_repositories: BTreeSet<String> = BTreeSet::new();
    for declared in declarations(&work_events) {
        if let Some(id) = declared.plan_ref.repository.rsplit(':').next() {
            agents_repositories.insert(id.to_owned());
        }
        let key = (
            declared.plan_ref.repository.clone(),
            declared.plan_ref.commit.clone(),
            declared.plan_ref.path.clone(),
        );
        if plan_blobs.contains_key(&key) {
            continue;
        }
        match plans.plan(&declared.plan_ref) {
            Ok(bytes) => {
                if let Ok(plan) = parse_plan(bytes.as_bytes()) {
                    code_repositories.insert(plan.code_repository.clone());
                    let actions_yml = plans.actions(&declared.plan_ref);
                    let precompiled = plans.compiled(&declared.plan_ref);
                    if let Some(compiled) = precompiled.map_or_else(
                        || {
                            compile_plan_actions(
                                &plan,
                                &session.project_ref,
                                actions_yml.as_deref(),
                            )
                            .ok()
                        },
                        Some,
                    ) {
                        for (name, definition) in compiled {
                            action_definitions.insert(
                                (
                                    declared.plan_ref.repository.clone(),
                                    declared.plan_ref.commit.clone(),
                                    name,
                                ),
                                definition,
                            );
                        }
                    }
                }
                plan_blobs.insert(key, bytes);
            }
            Err(error) => unresolved_plans.push(json!({
                "planRef": declared.plan_ref,
                "reason": "plan_blob_unavailable",
                "message": error.to_string(),
            })),
        }
    }

    // The signed 44244 records, because the assembler folds them with the
    // canonical team fold: coverage never admits what that contract excludes
    // (A5 decision 23). These are the same events `operations_reads` reads.
    let team_rows = client
        .query_events(
            json!({"kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION], "#h": [args_channel],
                   "#d": [session_ref], "#cstx-genesis": [session.genesis_ref]}),
            Some(READ_LIMIT),
        )
        .await
        .map_err(|error| read_failed("kind:44244 team transactions", error))?;
    let team_events: Vec<nostr::Event> = team_rows
        .into_iter()
        .map(|row| {
            serde_json::from_value(row).map_err(|error| {
                CliError::Other(format!(
                    "relay returned a malformed kind:44244 team transaction: {error}"
                ))
            })
        })
        .collect::<Result<_, CliError>>()?;
    let goal_rows = client
        .query_events(
            json!({"kinds": [KIND_CODING_SESSION_GOAL], "#h": [args_channel],
                      "#d": [session_ref]}),
            None,
        )
        .await
        .map_err(|error| read_failed("kind:44227 goals", error))?;
    let host_rows = client
        .query_events(
            json!({"kinds": [KIND_HOST_STEP_RESULT, KIND_WORKFLOW_HOST_STEP_EXITED,
                             KIND_WORKFLOW_HOST_STEP_REQUESTED], "#h": [args_channel]}),
            Some(READ_LIMIT),
        )
        .await
        .map_err(|error| read_failed("kind:46013/46014/46023 host-step events", error))?;
    let host_truncated = host_rows.len() as u32 >= READ_LIMIT;
    let host_events = decode_rows(host_rows, "host-step event")?;
    let mut ref_states = Vec::new();
    for repository in code_repositories.iter().chain(agents_repositories.iter()) {
        let rows = client
            .query_events(
                json!({"kinds": [KIND_GIT_REPO_STATE], "#d": [repository]}),
                None,
            )
            .await
            .map_err(|error| {
                read_failed(&format!("kind:30618 ref state for {repository}"), error)
            })?;
        ref_states.extend(decode_rows(rows, "kind:30618 ref state")?);
    }

    let split = |kind: u32| -> Vec<ProjectWorkEvent> {
        host_events
            .iter()
            .filter(|event| event.kind == kind)
            .cloned()
            .collect()
    };
    let raw = RawWorkInputs {
        work_events,
        team_events,
        host_results: split(KIND_HOST_STEP_RESULT),
        host_echoes: split(KIND_WORKFLOW_HOST_STEP_EXITED),
        host_requests: split(KIND_WORKFLOW_HOST_STEP_REQUESTED),
        ref_states,
        goal_events: decode_rows(goal_rows, "kind:44227 goal")?,
        authority: RawAuthorityContext {
            channel_ref: Some(session.channel.clone()),
            genesis_ref: Some(session.genesis_ref.clone()),
            genesis_event: None,
            founder_pubkey: Some(session.context.founder_pubkey.clone()),
            active_seats: session
                .context
                .active_seats
                .iter()
                .map(|seat| buzz_core::project_work_fold::WorkActiveSeat {
                    actor_pubkey: seat.actor_pubkey.clone(),
                    role: seat.role.clone(),
                })
                .collect(),
            active_grants: session
                .context
                .active_grants
                .iter()
                .map(|grant| buzz_core::project_work_fold::WorkActiveGrant {
                    actor_pubkey: grant.actor_pubkey.clone(),
                    grant_event_ref: grant.grant_event_ref.clone(),
                    may_steer: grant.may_steer,
                })
                .collect(),
        },
        relay_self_key: session.relay_self.clone(),
        plan_blobs,
        action_definitions,
        session_ref: Some(session_ref.clone()),
        project_ref: Some(session.project_ref.clone()),
    };
    let inputs =
        assemble_fold_inputs(raw).map_err(|refusal| CliError::Other(refusal.to_string()))?;
    let reads = json!({
        "truncated": truncated || host_truncated,
        "limit": READ_LIMIT,
        "unresolvedPlans": unresolved_plans,
    });
    Ok((fold_work(&inputs), reads))
}

/// Every declared body in a record set.
fn declarations(events: &[ProjectWorkEvent]) -> Vec<ProjectWorkDeclared> {
    events
        .iter()
        .filter_map(|event| {
            let payload: Value = serde_json::from_str(&event.content).ok()?;
            serde_json::from_value(payload.get("body")?.clone()).ok()
        })
        .collect()
}

/// The 44244 mission state, on its own row. Nothing merges the two.
async fn mission_row(client: &BuzzClient, session: &SessionContext) -> Result<Value, CliError> {
    let events = super::operations_reads::fetch_transactions(
        client,
        &session.channel,
        &session.session_ref,
        &session.genesis_ref,
    )
    .await
    .map_err(|error| read_failed("kind:44244 team transactions", error))?;
    let fold = buzz_core::coding_session_team_transaction::fold_coding_session_team_transactions(
        &events,
        &session.context,
    )
    .map_err(|error| CliError::Other(format!("the 44244 fold refused this session: {error}")))?;
    Ok(json!({
        "state": if fold.canonical_terminal.is_some() { "terminal" } else { "open" },
        "completionRef": fold.canonical_terminal.as_ref().map(|terminal| terminal.event_id.clone()),
        "pending": fold.pending_completion.as_ref().map(|pending| pending.event_id.clone()),
    }))
}

/// The one sentence a reader needs in order to decide whether to re-adopt,
/// and the command that does it.
///
/// **What this says and what it does not** (A10, contract § (c)): the agents
/// repository's `main` has moved since this declaration pinned its commit.
/// It does *not* say the plan file changed — relay ref state names a branch
/// tip, never a path — and the work is still judged against the plan at the
/// commit it pinned. Nothing here refuses anything.
pub(super) fn plan_drift_line(
    declaration: &buzz_core::project_work_fold::WorkDeclarationProjection,
    channel: &str,
    session_ref: &str,
) -> Option<String> {
    use buzz_core::project_work_fold::WorkPlanDriftState;
    let drift = &declaration.plan_drift;
    if drift.state != WorkPlanDriftState::Drifted {
        return None;
    }
    let current = drift.current_commit.as_deref()?;
    Some(format!(
        "plan moved: {}→{} — the agents repository's main has moved on since this plan commit; \
         this work is still judged against the plan at {}. Re-adopt it with: bee sessions work \
         adopt --plan {} --commit {current} --agents-repo <dir> --channel {channel} \
         --session-ref {session_ref} --work-id {} --supersedes {}",
        short_sha(&drift.declared_commit),
        short_sha(current),
        short_sha(&drift.declared_commit),
        declaration.plan_ref.path,
        declaration.work_id,
        declaration.declaration_ref
    ))
}

/// Twelve characters and an ellipsis, the contract's own abbreviation.
fn short_sha(commit: &str) -> String {
    format!("{}…", &commit[..commit.len().min(12)])
}

/// The compact table: one row per criterion, and the mission on its own row.
fn print_table(coverage: &WorkProjection, mission: &Value, channel: &str, session_ref: &str) {
    for declaration in &coverage.declarations {
        if let Some(line) = plan_drift_line(declaration, channel, session_ref) {
            println!("  {line}");
        }
        println!(
            "{} {} {} {}",
            &declaration.work_id[..8],
            format!("{:?}", declaration.state).to_lowercase(),
            declaration
                .candidate_artifact
                .as_deref()
                .map_or("-".to_owned(), |sha| sha[..sha.len().min(12)].to_owned()),
            if declaration.coverage_complete {
                "complete"
            } else {
                "incomplete"
            }
        );
        for criterion in &declaration.criteria {
            println!(
                "  {:<28} {:<8} {:<24} {}",
                criterion.criterion_id,
                criterion.status.as_str(),
                criterion
                    .reason_code
                    .map_or("-".to_owned(), |code| code.as_str().to_owned()),
                criterion.assignment_refs.join(",")
            );
        }
    }
    println!(
        "mission  : {} {}",
        mission["state"].as_str().unwrap_or("unknown"),
        mission["completionRef"].as_str().unwrap_or("-")
    );
}

/// Route one `bee sessions work` verb.
///
/// # Errors
/// Whatever the verb returns.
pub async fn dispatch(
    cmd: SessionWorkCmd,
    client: &BuzzClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    match cmd {
        SessionWorkCmd::Validate(args) => cmd_validate(&args),
        SessionWorkCmd::Adopt(args) => cmd_adopt(client, &args).await,
        SessionWorkCmd::Bind(WorkBindCmd::Assignment(args)) => {
            cmd_bind_assignment(client, &args).await
        }
        SessionWorkCmd::Bind(WorkBindCmd::Evidence(args)) => cmd_bind_evidence(client, &args).await,
        SessionWorkCmd::Status(args) => cmd_status(client, &args, format).await,
    }
}

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
