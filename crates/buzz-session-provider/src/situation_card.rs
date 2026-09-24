//! The **situation card**: the facts a team seat needs before its first tool
//! call, rendered by the provider and placed with its first turn — the goal
//! for the lead, the hire's first words for every other seat.
//!
//! Why. Control run 5 (2026-09-24) measured the lead seat spending 21 of its
//! 50 tool calls on orientation — the channel id, the session ref and genesis,
//! the plan path and commit, the criteria, the code checkout, the roster and
//! command shapes — while this provider held every one of those facts when it
//! started the lead's first turn. The worker's equivalent is the work brief
//! ([`crate::work_brief`], ledger 209), which covers an assignment turn; the
//! card covers every seat's first turn, lead or hired, whether or not an
//! assignment rides on it (Brian, 2026-09-24: orientation is given to every
//! seat, discovered by none).
//!
//! Rules the card keeps:
//!
//! - **Facts only, never a guess.** Every field either carries a value this
//!   host read at that moment or reads `unknown (<reason>)` with a one-word
//!   reason.
//! - **No instructions.** The role pack carries the instructions; the card is
//!   data the pack's command shapes are filled from.
//! - **Once per execution.** A copy is written into the seat's own bundle
//!   ([`SITUATION_CARD_FILE_NAME`]) and its presence is what says the card was
//!   already delivered, so a restart does not deliver it twice.
//! - **Never a gate.** Every read is best-effort; nothing here refuses, defers
//!   or delays a turn.
//!
//! Split in two like the work brief: [`render_situation_card`] is pure and
//! pinned by a golden test; the reads live in the `impl Provider` block below.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use buzz_core::coding_session_command::CodingSessionDelivery;
use buzz_core::project_plan::{
    check_plan_adoptable, parse_plan, validate_plan_path, Plan, PlanProof,
};
use tokio::process::Command;
use uuid::Uuid;

use crate::session::TurnFraming;

/// The file the card is also written to, inside the seat's own bundle.
pub const SITUATION_CARD_FILE_NAME: &str = "situation-card.md";

/// The fence's info string, so a reader can find the card in a transcript.
pub const SITUATION_CARD_FENCE: &str = "situation-card";

/// Most plans listed; the rest are counted.
pub const MAX_CARD_PLANS: usize = 3;

/// Most criteria listed per plan; the rest are counted.
pub const MAX_CARD_CRITERIA: usize = 8;

/// Most action definitions listed; the rest are counted.
pub const MAX_CARD_ACTIONS: usize = 6;

/// Most roster rows listed; the rest are counted.
pub const MAX_CARD_ROSTER: usize = 10;

/// Ceiling on one git read. A card is a saving, never a delay worth more
/// than the orientation it replaces.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);

/// A fact, or the one-word reason it could not be read.
pub type Fact = Result<String, String>;

/// One committed plan under `plans/`, judged by `work validate`'s rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardPlan {
    /// Path inside the agents repository, e.g. `plans/kettle.md`.
    pub path: String,
    /// `Ok` when adoptable; otherwise the refusal's stable code.
    pub adoptable: Result<(), String>,
    /// `(criterion id, proof words)` in presentation order.
    pub criteria: Vec<(String, String)>,
}

/// One entry of the agents repository's `actions.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardAction {
    /// Action name.
    pub name: String,
    /// Definition hash, as a kind:46013 carries it.
    pub hash: String,
    /// Step ids, in order.
    pub steps: Vec<String>,
}

/// One `team.yml` agent, with what this host knows of its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardRosterEntry {
    /// Role slug.
    pub role: String,
    /// Agent name as `team.yml` records it.
    pub name: String,
    /// `persistent` or `ephemeral`.
    pub lifetime: String,
    /// The role's advisory runtime hint, when `team.yml` gives one.
    pub runtime: Option<String>,
    /// The role's advisory model hint, when `team.yml` gives one.
    pub model: Option<String>,
    /// The seat's pubkey, when a seat of this role is recorded here.
    pub pubkey: Fact,
}

/// What a hired (non-lead) seat is additionally told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiredSeatFacts {
    /// The `session.hire` event the create answering it named.
    pub hire: Fact,
    /// The kind:44244 assignment the first turn's pointer names.
    pub assignment: Fact,
    /// `(criterion id, proof words)` the assignment is bound to.
    pub criteria: Result<Vec<(String, String)>, String>,
    /// The plan's `delivery_ref` for that binding.
    pub delivery_ref: Fact,
    /// The lead execution's `cs-target` key, what `--to` takes.
    pub lead_target: Fact,
    /// The lead seat's pubkey.
    pub lead_pubkey: Fact,
    /// The report command with every id filled in.
    pub report_command: Fact,
    /// The send command addressed to the lead.
    pub send_command: Fact,
}

/// Everything one card is rendered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationFacts {
    /// Channel UUID.
    pub channel: String,
    /// Umbrella session ref.
    pub session_ref: Fact,
    /// Session genesis event id.
    pub genesis: Fact,
    /// Project slug and coordinate.
    pub project: Fact,
    /// This seat's role slug.
    pub seat_role: String,
    /// This seat's pubkey.
    pub seat_pubkey: Fact,
    /// This seat's model, as recorded.
    pub seat_model: Fact,
    /// This seat's runtime.
    pub seat_runtime: String,
    /// The project's code checkout on this host.
    pub code_checkout: Fact,
    /// This seat's worktree.
    pub worktree: Fact,
    /// Branch and commit the worktree's `HEAD` names.
    pub worktree_head: Fact,
    /// The agents-repository checkout this seat reads.
    pub agents_repo: Fact,
    /// The commit that checkout's `HEAD` names.
    pub agents_commit: Fact,
    /// Committed plans at that commit.
    pub plans: Result<Vec<CardPlan>, String>,
    /// `actions.yml` at that commit.
    pub actions: Result<Vec<CardAction>, String>,
    /// `team.yml` agents at that commit.
    pub roster: Result<Vec<CardRosterEntry>, String>,
    /// Where the copy of this card is written.
    pub card_path: Fact,
    /// Present for every seat that is not the team's lead.
    pub hired: Option<HiredSeatFacts>,
}

fn fact(value: &Fact) -> String {
    match value {
        Ok(value) => value.clone(),
        Err(reason) => format!("unknown ({reason})"),
    }
}

/// The words a criterion's proof is named by — the same forms
/// `work validate` prints.
pub fn proof_words(proof: &PlanProof, plan: &Plan) -> String {
    match proof {
        PlanProof::Review => "review".to_owned(),
        PlanProof::Action { name, step } => format!("action {name} step {step}"),
        PlanProof::GitRef => format!("git-ref {}", plan.delivery_ref),
    }
}

/// Render the card: one fenced block, one fact per line.
pub fn render_situation_card(facts: &SituationFacts) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "```{SITUATION_CARD_FENCE}");
    let _ = writeln!(out, "channel: {}", facts.channel);
    let _ = writeln!(out, "session-ref: {}", fact(&facts.session_ref));
    let _ = writeln!(out, "genesis: {}", fact(&facts.genesis));
    let _ = writeln!(out, "project: {}", fact(&facts.project));
    let _ = writeln!(
        out,
        "seat: role {} pubkey {} model {} runtime {}",
        facts.seat_role,
        fact(&facts.seat_pubkey),
        fact(&facts.seat_model),
        facts.seat_runtime
    );
    let _ = writeln!(out, "code-checkout: {}", fact(&facts.code_checkout));
    let _ = writeln!(out, "worktree: {}", fact(&facts.worktree));
    let _ = writeln!(out, "worktree-head: {}", fact(&facts.worktree_head));
    let _ = writeln!(out, "agents-repo: {}", fact(&facts.agents_repo));
    let _ = writeln!(out, "agents-commit: {}", fact(&facts.agents_commit));
    if let Some(hired) = &facts.hired {
        render_hired(&mut out, hired);
    }
    match &facts.plans {
        Err(reason) => {
            let _ = writeln!(out, "plan: unknown ({reason})");
        }
        Ok(plans) if plans.is_empty() => {
            let _ = writeln!(out, "plan: none");
        }
        Ok(plans) => {
            for plan in plans.iter().take(MAX_CARD_PLANS) {
                let adoptable = match &plan.adoptable {
                    Ok(()) => "yes".to_owned(),
                    Err(code) => format!("no ({code})"),
                };
                let _ = writeln!(out, "plan: {} adoptable {adoptable}", plan.path);
                for (id, proof) in plan.criteria.iter().take(MAX_CARD_CRITERIA) {
                    let _ = writeln!(out, "  criterion {id}: {proof}");
                }
                if plan.criteria.len() > MAX_CARD_CRITERIA {
                    let _ = writeln!(
                        out,
                        "  criteria not listed: {}",
                        plan.criteria.len() - MAX_CARD_CRITERIA
                    );
                }
            }
            if plans.len() > MAX_CARD_PLANS {
                let _ = writeln!(out, "plans not listed: {}", plans.len() - MAX_CARD_PLANS);
            }
        }
    }
    match &facts.actions {
        Err(reason) => {
            let _ = writeln!(out, "action: unknown ({reason})");
        }
        Ok(actions) if actions.is_empty() => {
            let _ = writeln!(out, "action: none");
        }
        Ok(actions) => {
            for action in actions.iter().take(MAX_CARD_ACTIONS) {
                let _ = writeln!(
                    out,
                    "action: {} hash {} steps {}",
                    action.name,
                    action.hash,
                    action.steps.join(",")
                );
            }
            if actions.len() > MAX_CARD_ACTIONS {
                let _ = writeln!(
                    out,
                    "actions not listed: {}",
                    actions.len() - MAX_CARD_ACTIONS
                );
            }
        }
    }
    match &facts.roster {
        Err(reason) => {
            let _ = writeln!(out, "roster: unknown ({reason})");
        }
        Ok(roster) if roster.is_empty() => {
            let _ = writeln!(out, "roster: none");
        }
        Ok(roster) => {
            for entry in roster.iter().take(MAX_CARD_ROSTER) {
                let _ = writeln!(
                    out,
                    "roster: {} {:?} {} runtime {} model {} pubkey {}",
                    entry.role,
                    entry.name,
                    entry.lifetime,
                    entry.runtime.as_deref().unwrap_or("unset"),
                    entry.model.as_deref().unwrap_or("unset"),
                    fact(&entry.pubkey)
                );
            }
            if roster.len() > MAX_CARD_ROSTER {
                let _ = writeln!(
                    out,
                    "roster rows not listed: {}",
                    roster.len() - MAX_CARD_ROSTER
                );
            }
        }
    }
    let _ = writeln!(out, "card-copy: {}", fact(&facts.card_path));
    out.push_str("```");
    out
}

fn render_hired(out: &mut String, hired: &HiredSeatFacts) {
    let _ = writeln!(out, "hire: {}", fact(&hired.hire));
    let _ = writeln!(out, "assignment: {}", fact(&hired.assignment));
    match &hired.criteria {
        Err(reason) => {
            let _ = writeln!(out, "assigned-criteria: unknown ({reason})");
        }
        Ok(criteria) if criteria.is_empty() => {
            let _ = writeln!(out, "assigned-criteria: none");
        }
        Ok(criteria) => {
            for (id, proof) in criteria.iter().take(MAX_CARD_CRITERIA) {
                let _ = writeln!(out, "assigned-criterion {id}: {proof}");
            }
            if criteria.len() > MAX_CARD_CRITERIA {
                let _ = writeln!(
                    out,
                    "assigned-criteria not listed: {}",
                    criteria.len() - MAX_CARD_CRITERIA
                );
            }
        }
    }
    let _ = writeln!(out, "delivery-ref: {}", fact(&hired.delivery_ref));
    let _ = writeln!(out, "lead-target: {}", fact(&hired.lead_target));
    let _ = writeln!(out, "lead-pubkey: {}", fact(&hired.lead_pubkey));
    let _ = writeln!(out, "report-command: {}", fact(&hired.report_command));
    let _ = writeln!(out, "send-command: {}", fact(&hired.send_command));
}

/// The report command a hired seat runs, every id filled in — the same
/// shape the work brief prints.
pub fn report_command(channel: &str, session_ref: &Fact, genesis: &Fact) -> Fact {
    let session_ref = session_ref.as_ref().map_err(Clone::clone)?;
    let genesis = genesis.as_ref().map_err(Clone::clone)?;
    Ok(format!(
        "{} sessions report --channel {channel} --session-ref {session_ref} --genesis {genesis} \
         --body - --wake-to lead",
        crate::work_brief::BEE
    ))
}

/// The send command addressed to the lead's execution.
pub fn send_command(channel: &str, lead_target: &Fact) -> Fact {
    let target = lead_target.as_ref().map_err(Clone::clone)?;
    Ok(format!(
        "{} sessions send --channel {channel} --to {target}",
        crate::work_brief::BEE
    ))
}

/// Attach a card to a turn's frame.
///
/// A founder-sent turn has no frame; the card then rides on a frame that
/// renders no `[Context]` block ([`TurnFraming::unaddressed`]), so the
/// founder's words reach the lead exactly as they did, below the card.
pub(crate) fn attach_situation_card(
    framing: Option<TurnFraming>,
    card: Option<String>,
    channel_id: Uuid,
    sender_pubkey: &str,
    delivery: CodingSessionDelivery,
) -> Option<TurnFraming> {
    let Some(card) = card else {
        return framing;
    };
    let mut framing = framing.unwrap_or_else(|| TurnFraming {
        channel_id,
        sender_pubkey: sender_pubkey.to_owned(),
        sender_role: None,
        reply_target: None,
        delivery,
        work_brief: None,
        situation_card: None,
        unaddressed: true,
    });
    framing.situation_card = Some(card);
    Some(framing)
}

/// `30621:<owner>:<slug>` → `<slug> (<coordinate>)`.
fn project_words(project_ref: Option<&str>) -> Fact {
    let project = project_ref.ok_or_else(|| "no-project".to_owned())?;
    let slug = project
        .rsplit(':')
        .next()
        .filter(|slug| !slug.is_empty())
        .ok_or_else(|| "malformed-coordinate".to_owned())?;
    Ok(format!("{slug} ({project})"))
}

/// One git invocation in `dir`, hermetic and never prompting.
async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "git-timeout".to_owned())?
        .map_err(|_| "git-unavailable".to_owned())?;
    if !output.status.success() {
        return Err("git-refused".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Every committed file under `plans/` at `commit` that parses as a
/// `beekeeper-plan/v1`, judged by the rules `work validate` applies:
/// `validate_plan_path`, `parse_plan`, `check_plan_adoptable`, then every
/// `action` proof must name an action and step `actions.yml` defines.
pub(crate) fn judge_plans(
    files: &[(String, String)],
    actions: &Result<Vec<CardAction>, String>,
) -> Vec<CardPlan> {
    let mut plans = Vec::new();
    for (path, text) in files {
        if validate_plan_path(path).is_err() {
            continue;
        }
        // A file that is not a plan (the map, a report) is not listed: only
        // a parse is evidence that it was meant as one.
        let Ok(plan) = parse_plan(text.as_bytes()) else {
            continue;
        };
        let adoptable = check_plan_adoptable(&plan, path)
            .map_err(|refusal| refusal.code.as_str().to_owned())
            .and_then(|()| unresolved_action(&plan, actions).map_or(Ok(()), Err));
        let criteria = plan
            .criteria
            .iter()
            .map(|criterion| (criterion.id.clone(), proof_words(&criterion.proof, &plan)))
            .collect();
        plans.push(CardPlan {
            path: path.clone(),
            adoptable,
            criteria,
        });
    }
    plans
}

/// The first `action` proof `actions.yml` does not resolve, as the code
/// `work validate` would refuse it with.
fn unresolved_action(plan: &Plan, actions: &Result<Vec<CardAction>, String>) -> Option<String> {
    for criterion in &plan.criteria {
        let PlanProof::Action { name, step } = &criterion.proof else {
            continue;
        };
        let Ok(actions) = actions else {
            return Some("unresolved-action".to_owned());
        };
        match actions.iter().find(|action| &action.name == name) {
            None => return Some("unresolved-action".to_owned()),
            Some(action) if !action.steps.iter().any(|known| known == step) => {
                return Some("unresolved-action-step".to_owned())
            }
            Some(_) => {}
        }
    }
    None
}

/// Everything this host can read about one agents-repository commit.
struct AgentsReads {
    plans: Result<Vec<CardPlan>, String>,
    actions: Result<Vec<CardAction>, String>,
    team: Result<buzz_persona::team::TeamManifest, String>,
}

async fn read_agents_commit(dir: &Path, commit: &str, project_ref: Option<&str>) -> AgentsReads {
    let actions = match project_ref {
        None => Err("no-project".to_owned()),
        Some(project) => match git(dir, &["show", &format!("{commit}:actions.yml")]).await {
            Err(_) => Ok(Vec::new()),
            Ok(text) => buzz_workflow::parse_actions_yml(&text, project)
                .map(|entries| {
                    entries
                        .into_iter()
                        .map(|entry| CardAction {
                            steps: entry.def.steps.iter().map(|step| step.id.clone()).collect(),
                            name: entry.name,
                            hash: entry.hash,
                        })
                        .collect()
                })
                .map_err(|_| "actions-invalid".to_owned()),
        },
    };
    let plans = match git(dir, &["ls-tree", "--name-only", commit, "plans/"]).await {
        Err(reason) => Err(reason),
        Ok(listing) => {
            let mut files = Vec::new();
            for path in listing
                .lines()
                .map(str::trim)
                .filter(|path| path.ends_with(".md"))
            {
                if let Ok(text) = git(dir, &["show", &format!("{commit}:{path}")]).await {
                    files.push((path.to_owned(), text));
                }
            }
            Ok(judge_plans(&files, &actions))
        }
    };
    let team = match git(dir, &["show", &format!("{commit}:team.yml")]).await {
        Err(_) => Err("no-team-yml".to_owned()),
        Ok(text) => buzz_persona::team::parse_team_yml(&text, Path::new("team.yml"))
            .map_err(|_| "team-yml-invalid".to_owned()),
    };
    AgentsReads {
        plans,
        actions,
        team,
    }
}

impl crate::Provider {
    /// The situation card for a seat's first turn, or `None`.
    ///
    /// `None` for an unseated or Solo execution (no role or no umbrella) and
    /// for an execution whose bundle already holds a card — which is what
    /// makes it the first turn's alone. `hire_ref` is the `session.hire` the
    /// create answered, when the turn is a create's initial turn.
    pub(crate) async fn seat_situation_card(
        &self,
        session_id: &str,
        text: &str,
        hire_ref: Option<&str>,
    ) -> Option<String> {
        let record = self.state.session(session_id)?;
        let role = record.role.clone()?;
        record.session_ref.as_ref()?;
        let card_path = crate::session::seat_bundle_dir(&self.config.state_dir, session_id)
            .join(SITUATION_CARD_FILE_NAME);
        if card_path.exists() {
            return None;
        }
        let worktree = crate::gate_cwd::resolve(
            self.config.projects_file.as_deref(),
            session_id,
            &record.cwd,
        );
        let worktree_path = worktree.present().map(Path::to_path_buf);
        let projects = crate::commands::ProjectsFile::load(self.config.projects_file.as_deref());
        let project_ref = record.project_ref.clone();
        let host_agents = project_ref
            .as_deref()
            .and_then(|project| projects.agents_repos.get(project).cloned());
        // The seat's own clone beside its worktree is what the lead passes as
        // `--agents-repo`; the host's clone is the fallback it can also read.
        let agents_dir: Option<PathBuf> = worktree_path
            .as_deref()
            .and_then(buzz_core::model_registry_source::seat_agents_clone_path)
            .filter(|path| path.join(".git").exists())
            .or_else(|| host_agents.map(|record| record.path))
            .filter(|path| path.exists());
        let agents_commit: Fact = match &agents_dir {
            None => Err("no-checkout".to_owned()),
            Some(dir) => git(dir, &["rev-parse", "HEAD"])
                .await
                .map(|sha| sha.trim().to_owned()),
        };
        let reads = match (&agents_dir, &agents_commit) {
            (Some(dir), Ok(commit)) => {
                Some(read_agents_commit(dir, commit, project_ref.as_deref()).await)
            }
            _ => None,
        };
        let lead_role = match reads.as_ref().map(|reads| &reads.team) {
            Some(Ok(team)) => team.lead.clone().unwrap_or_else(|| "lead".to_owned()),
            _ => "lead".to_owned(),
        };
        let hired = if role == lead_role {
            None
        } else {
            Some(
                self.hired_seat_facts(record, &lead_role, text, hire_ref, agents_dir.as_deref())
                    .await,
            )
        };
        let worktree_head = match &worktree_path {
            None => Err("no-worktree".to_owned()),
            Some(dir) => {
                let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
                let sha = git(dir, &["rev-parse", "HEAD"]).await;
                match (branch, sha) {
                    (Ok(branch), Ok(sha)) => Ok(format!("{} {}", branch.trim(), sha.trim())),
                    (_, Err(reason)) | (Err(reason), _) => Err(reason),
                }
            }
        };
        let umbrella = record.session_ref.clone();
        let roster = match reads.as_ref().map(|reads| &reads.team) {
            None => Err("no-checkout".to_owned()),
            Some(Err(reason)) => Err(reason.clone()),
            Some(Ok(team)) => Ok(team
                .agents
                .iter()
                .map(|agent| {
                    let hints = team.role(&agent.role);
                    let seated = self
                        .state
                        .sessions()
                        .filter(|candidate| {
                            !candidate.closed
                                && candidate.session_ref == umbrella
                                && candidate.role.as_deref() == Some(agent.role.as_str())
                        })
                        .find_map(|candidate| candidate.actor.clone());
                    CardRosterEntry {
                        role: agent.role.clone(),
                        name: agent.name.clone(),
                        lifetime: match agent.lifetime {
                            buzz_persona::team::AgentLifetime::Persistent => "persistent",
                            buzz_persona::team::AgentLifetime::Ephemeral => "ephemeral",
                        }
                        .to_owned(),
                        runtime: hints.runtime.clone(),
                        model: hints.model.clone(),
                        pubkey: seated.ok_or_else(|| "not-seated".to_owned()),
                    }
                })
                .collect()),
        };
        let (plans, actions) = match reads {
            None => (Err("no-checkout".to_owned()), Err("no-checkout".to_owned())),
            Some(reads) => (reads.plans, reads.actions),
        };
        let facts = SituationFacts {
            channel: record.channel_id.to_string(),
            session_ref: record
                .session_ref
                .clone()
                .ok_or_else(|| "unrecorded".to_owned()),
            genesis: record
                .genesis_ref
                .clone()
                .ok_or_else(|| "unrecorded".to_owned()),
            project: project_words(project_ref.as_deref()),
            seat_role: role,
            seat_pubkey: record.actor.clone().ok_or_else(|| "unseated".to_owned()),
            seat_model: record.model.clone().ok_or_else(|| "unreported".to_owned()),
            seat_runtime: record.runtime.clone(),
            code_checkout: project_ref
                .as_deref()
                .and_then(|project| projects.projects.get(project))
                .map(|path| path.display().to_string())
                .ok_or_else(|| "unrecorded".to_owned()),
            worktree: worktree_path
                .as_ref()
                .map(|path| path.display().to_string())
                .ok_or_else(|| "missing".to_owned()),
            worktree_head,
            agents_repo: agents_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .ok_or_else(|| "no-checkout".to_owned()),
            agents_commit,
            plans,
            actions,
            roster,
            card_path: Ok(card_path.display().to_string()),
            hired,
        };
        let card = render_situation_card(&facts);
        if write_card_copy(&card_path, &card) {
            return Some(card);
        }
        // No copy on disk: the card must not name one, and nothing marks
        // this execution as served, so a later turn may try again.
        let facts = SituationFacts {
            card_path: Err("write-failed".to_owned()),
            ..facts
        };
        Some(render_situation_card(&facts))
    }

    /// What a hired seat is told beyond the shared facts.
    async fn hired_seat_facts(
        &self,
        record: &crate::state::SessionRecord,
        lead_role: &str,
        text: &str,
        hire_ref: Option<&str>,
        agents_dir: Option<&Path>,
    ) -> HiredSeatFacts {
        let channel = record.channel_id.to_string();
        let session_ref: Fact = record
            .session_ref
            .clone()
            .ok_or_else(|| "unrecorded".to_owned());
        let genesis: Fact = record
            .genesis_ref
            .clone()
            .ok_or_else(|| "unrecorded".to_owned());
        let lead = self
            .state
            .sessions()
            .filter(|candidate| {
                !candidate.closed
                    && candidate.session_ref == record.session_ref
                    && candidate.role.as_deref() == Some(lead_role)
            })
            .max_by_key(|candidate| (candidate.created_at_ms, candidate.generation));
        let lead_target: Fact = lead
            .map(|lead| {
                buzz_core::coding_session_command::coding_session_target_key(&self.target_for(lead))
            })
            .ok_or_else(|| "lead-not-local".to_owned());
        let lead_pubkey: Fact = lead
            .and_then(|lead| lead.actor.clone())
            .ok_or_else(|| "lead-not-local".to_owned());
        let assignment: Fact = crate::verification_input::assignment_pointer(text)
            .ok_or_else(|| "no-pointer".to_owned());
        let (criteria, delivery_ref) = match &assignment {
            Err(reason) => (Err(reason.clone()), Err(reason.clone())),
            Ok(assignment) => match (&self.rest_client, session_ref.as_ref()) {
                (None, _) => (Err("no-relay".to_owned()), Err("no-relay".to_owned())),
                (_, Err(reason)) => (Err(reason.clone()), Err(reason.clone())),
                (Some(rest), Ok(umbrella)) => {
                    assigned_criteria(rest, record.channel_id, umbrella, assignment, agents_dir)
                        .await
                }
            },
        };
        HiredSeatFacts {
            hire: hire_ref
                .map(str::to_owned)
                .ok_or_else(|| "not-a-create".to_owned()),
            assignment,
            criteria,
            delivery_ref,
            report_command: report_command(&channel, &session_ref, &genesis),
            send_command: send_command(&channel, &lead_target),
            lead_target,
            lead_pubkey,
        }
    }
}

/// The criteria a `work.assignment_bound` gives `assignment`, with the proof
/// each carries in the plan blob at its declaration's pinned commit, and that
/// plan's delivery ref.
async fn assigned_criteria(
    rest: &buzz_acp::relay::RestClient,
    channel: Uuid,
    session_ref: &str,
    assignment: &str,
    agents_dir: Option<&Path>,
) -> (Result<Vec<(String, String)>, String>, Fact) {
    use buzz_core::project_work::{decode_project_work_content, ProjectWorkBody};
    let events = match crate::context_projector::query_complete_kind_partition(
        rest,
        channel,
        buzz_core::kind::KIND_PROJECT_WORK_RECORD,
    )
    .await
    {
        Ok(events) => events,
        Err(_) => {
            return (
                Err("relay-unread".to_owned()),
                Err("relay-unread".to_owned()),
            )
        }
    };
    let payloads: Vec<(String, ProjectWorkBody)> = events
        .iter()
        .filter_map(|event| {
            let payload = decode_project_work_content(&event.content).ok()?;
            (payload.session_ref == session_ref).then(|| (event.id.to_hex(), payload.body))
        })
        .collect();
    // The newest binding wins only by the relay's own order; a binding that
    // was replaced names what it replaced, so the unreplaced one is current.
    let replaced: Vec<String> = payloads
        .iter()
        .filter_map(|(_, body)| match body {
            ProjectWorkBody::AssignmentBound(bound) => bound.replaces_binding.clone(),
            _ => None,
        })
        .collect();
    let binding = payloads.iter().find_map(|(id, body)| match body {
        ProjectWorkBody::AssignmentBound(bound)
            if bound.assignment_ref.eq_ignore_ascii_case(assignment)
                && !replaced.iter().any(|gone| gone.eq_ignore_ascii_case(id)) =>
        {
            Some(bound.clone())
        }
        _ => None,
    });
    let Some(binding) = binding else {
        return (Ok(Vec::new()), Err("unbound".to_owned()));
    };
    let plan_ref = payloads.iter().find_map(|(id, body)| match body {
        ProjectWorkBody::Declared(declared)
            if id.eq_ignore_ascii_case(&binding.declaration_ref) =>
        {
            Some(declared.plan_ref.clone())
        }
        _ => None,
    });
    let plan = match (plan_ref, agents_dir) {
        (Some(plan_ref), Some(dir)) => git(
            dir,
            &["show", &format!("{}:{}", plan_ref.commit, plan_ref.path)],
        )
        .await
        .ok()
        .and_then(|text| parse_plan(text.as_bytes()).ok()),
        _ => None,
    };
    let Some(plan) = plan else {
        let criteria = binding
            .criterion_ids
            .iter()
            .map(|id| (id.clone(), "unknown (plan-unread)".to_owned()))
            .collect();
        return (Ok(criteria), Err("plan-unread".to_owned()));
    };
    let criteria = binding
        .criterion_ids
        .iter()
        .map(|id| {
            let proof = plan
                .criteria
                .iter()
                .find(|criterion| &criterion.id == id)
                .map_or_else(
                    || "unknown (not-in-plan)".to_owned(),
                    |criterion| proof_words(&criterion.proof, &plan),
                );
            (id.clone(), proof)
        })
        .collect();
    (Ok(criteria), Ok(plan.delivery_ref.clone()))
}

/// Put the same bytes in the seat's bundle: a copy the seat can re-read, and
/// the marker that this execution already received its card.
fn write_card_copy(path: &Path, text: &str) -> bool {
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::debug!(target: "csp::situation_card", %error, "the seat bundle directory could not be created for the situation card");
            return false;
        }
    }
    match std::fs::write(path, text) {
        Ok(()) => true,
        Err(error) => {
            tracing::debug!(target: "csp::situation_card", %error, "the situation card could not be written into the seat bundle");
            false
        }
    }
}

#[cfg(test)]
#[path = "situation_card_tests.rs"]
mod tests;
