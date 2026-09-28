//! The host-assembled **work brief**: everything a seat opened for a canonical
//! assignment needs before its first tool call, written by the host and
//! prepended to that turn.
//!
//! Why the host and not the lead. On 2026-09-20 a lead hand-typed the channel,
//! session, genesis, assignment and base ids into each hire brief and
//! reproduced the plan text, because a worker's role holds no
//! agents-repository grant. The seats still spent 24–38% of their tool calls
//! on orientation — reading role and skill files, running `--help`,
//! rediscovering ids and report shapes. A seat's input cost per turn is its
//! context times the number of tool calls in the turn, so every orientation
//! call is paid at full context (plan amendment A4.1). The host already holds
//! every one of those facts as a *verified* fact; handing them over costs one
//! host-side render and no model turn at all.
//!
//! What it is not. It carries **no role prose**: the seat bundle already
//! delivers the role's own instructions and skills by absolute path
//! (ledger 132), and repeating them here would pay for the same bytes twice.
//! It carries no transcripts and no history: those stay lazy, reachable by
//! tool when a turn actually needs them.
//!
//! Every section is a *fact the host verified*, or an explicit sentence saying
//! it could not be read. A brief that guessed would be worse than no brief:
//! the whole value of the thing is that a seat may act on it without checking.
//!
//! The normative field list and the byte budget are
//! `conformance/project-work/README.md` § (e).

use std::fmt::Write as _;

use buzz_core::project_plan::PlanProof;

/// Total budget for one rendered brief, from README § (e).
///
/// A brief over budget is not truncated arbitrarily: whole sections are
/// dropped in [`DROP_ORDER`] and the brief **says** which, so a seat is never
/// left to wonder whether a missing section means "nothing there" or "no room".
pub const WORK_BRIEF_BUDGET_BYTES: usize = 8_192;

/// Per-criterion excerpt ceiling, from README § (e) row 1.
pub const MAX_CRITERION_EXCERPT_BYTES: usize = 512;

/// Ceiling on the lead's own prose, from README § (e) row 8.
pub const MAX_FREE_PROSE_BYTES: usize = 1_024;

/// The file the brief is also written to, inside the seat's own bundle.
pub const WORK_BRIEF_FILE_NAME: &str = "work-brief.md";

/// The literal every command line in the brief spells.
///
/// Never a bare `bee`: the `bee` on a host's `PATH` is routinely a stale build
/// (ledger §3a), and the harness exports `$BEE` pointing at the bundle's own
/// binary. A brief that printed `bee` would be teaching a seat to run the
/// wrong binary in its very first sentence.
pub const BEE: &str = "$BEE";

/// The brief's sections, in the order they are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkBriefSection {
    /// (a) What you owe.
    Owed,
    /// (b) Identifiers as ready-to-run commands.
    Commands,
    /// (c) The contract.
    Contract,
    /// (d) Decisions in force.
    Decisions,
    /// (e) What you may do.
    Permissions,
    /// (f) What you are.
    Runtime,
}

impl WorkBriefSection {
    /// The rendered heading.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Self::Owed => "## WHAT YOU OWE",
            Self::Commands => "## HOW TO ANSWER (ready to run)",
            Self::Contract => "## THE CONTRACT",
            Self::Decisions => "## DECISIONS IN FORCE",
            Self::Permissions => "## WHAT YOU MAY DO",
            Self::Runtime => "## WHAT YOU ARE",
        }
    }

    /// The short name used when the brief says a section was dropped.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Owed => "what you owe",
            Self::Commands => "how to answer",
            Self::Contract => "the contract",
            Self::Decisions => "decisions in force",
            Self::Permissions => "what you may do",
            Self::Runtime => "what you are",
        }
    }
}

/// The order sections are given up in when the render is over budget.
///
/// Lowest value first. (f) goes before (d) because a seat that does not know
/// which model it is can still do the work, and an answered decision it cannot
/// see is at worst a question it asks again — whereas without (a) it has no
/// task, without (b) it cannot answer, without (c) it does not know what
/// "done" means, and without (e) it may exceed its authority.
pub const DROP_ORDER: [WorkBriefSection; 4] = [
    WorkBriefSection::Runtime,
    WorkBriefSection::Decisions,
    WorkBriefSection::Contract,
    WorkBriefSection::Permissions,
];

/// What the host did about the commit this assignment names (ledger 202).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEstablishment {
    /// The host put this worktree on that commit, or found it already there.
    Established {
        /// The commit the tree holds.
        commit: String,
    },
    /// This role's turn is not a claim about a particular commit, so nothing
    /// was established and nothing needed to be.
    NotRequired,
    /// The host tried and could not, in git's own words.
    Failed {
        /// Git's own sentence, already bounded by the caller.
        words: String,
    },
    /// The host has no record either way. Said out loud rather than guessed.
    Unrecorded,
}

/// (a) The assignment itself, as the canonical 44244 record states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentFacts {
    /// Event id of the assignment.
    pub assignment_ref: String,
    /// The role it was issued to.
    pub role: String,
    /// The outcome this assignment owns.
    pub objective: String,
    /// The lead's own framing, verbatim, bounded at render time.
    pub brief: String,
    /// The branch this seat's worktree is on, as the host pinned it when it
    /// prepared the execution — the one branch the seat can commit to.
    pub branch: Option<String>,
    /// The branch the assignment's text named, when it named one. Shown only
    /// when it differs from `branch`, as the remote name to push to.
    pub assignment_branch: Option<String>,
    /// The exact commit the work is about, when the assignment named one.
    pub base_sha: Option<String>,
    /// Exclusive paths this assignment owns.
    pub file_ownership: Vec<String>,
    /// Ordered acceptance commands or observable checks.
    pub acceptance_steps: Vec<String>,
    /// Absolute path of the worktree the seat runs in.
    pub worktree: Option<String>,
    /// What the host did about `base_sha` before this turn.
    pub establishment: InputEstablishment,
}

/// (b) Every identifier this seat needs, already inside a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFacts {
    /// Channel UUID the session publishes into.
    pub channel: String,
    /// Umbrella session UUID.
    pub session_ref: String,
    /// Session genesis event id.
    pub genesis_ref: String,
    /// The assignment this turn answers.
    pub assignment_ref: String,
    /// The seat's role, which decides whether a verdict line is rendered.
    pub role: String,
    /// The report a verifier is ruling on, when the host could resolve one.
    pub report_ref: Option<String>,
    /// That report's own `assignmentRef` — the builder's assignment a
    /// verdict rules on, distinct from this seat's own `assignment_ref`.
    pub verdict_assignment_ref: Option<String>,
}

/// One criterion, excerpted from the plan blob read at the declaration's
/// pinned commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionExcerpt {
    /// The criterion's slug id.
    pub id: String,
    /// Its `accept` text, verbatim from the plan at `provenance`.
    pub accept: String,
    /// `<repository>@<commit-12>:<path>#<criterion id>`.
    pub provenance: String,
    /// The proof form, in words. `None` when the plan could not be read.
    pub proof: Option<PlanProof>,
}

/// (c) The adopted plan's criteria that this assignment is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractFacts {
    /// A head declaration binds these criteria to this assignment.
    Declared {
        /// The `work.declared` event id.
        declaration_ref: String,
        /// `<repository>@<commit-12>:<path>`.
        plan_provenance: String,
        /// The criteria, in plan order.
        criteria: Vec<CriterionExcerpt>,
    },
    /// The plan blob at the declaration's commit could not be read; the ids
    /// are still known because they come off the wire.
    PlanUnreadable {
        /// The `work.declared` event id.
        declaration_ref: String,
        /// Why, in one line.
        reason: String,
        /// The criterion ids the binding names.
        criterion_ids: Vec<String>,
    },
    /// No adopted plan names this session. Legacy sessions are ordinary.
    None,
    /// The work records could not be read at all.
    Unread {
        /// Why, in one line.
        reason: String,
    },
}

/// One answered decision that names this assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionSummary {
    /// The `decision.answer` event id.
    pub event_id: String,
    /// The question and the chosen option, in one line.
    pub summary: String,
}

/// (d) The goal, and the rulings that already bind this assignment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecisionFacts {
    /// The session's current kind:44227 goal event id.
    pub goal_ref: Option<String>,
    /// Its first line.
    pub goal_first_line: Option<String>,
    /// Answered decisions whose request named this assignment in `blocks`.
    pub decisions: Vec<DecisionSummary>,
}

/// (e) What this seat may actually do, as facts rather than encouragement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionFacts {
    /// `none`, `read` or `write` on the project's agents repository.
    pub agents_access: String,
    /// The clone's absolute path, when the seat has one.
    pub agents_path: Option<String>,
    /// Whether this seat may push its own branch.
    pub may_push: bool,
}

/// (f) The runtime, the model, and where that choice came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeFacts {
    /// The runtime token this execution actually runs under.
    pub runtime: String,
    /// The model, when one is known.
    pub model: Option<String>,
    /// Where the model came from, in words: the registry, a `team.yml` hint,
    /// or the identity's own pin (ledger 180).
    pub model_source: String,
    /// `composeRef.appVersion` of the role template composed for this seat.
    pub compose_app_version: Option<String>,
    /// `composeRef.digest`, which is what actually pins the text.
    pub compose_digest: Option<String>,
}

/// Everything one brief is rendered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkBriefInputs {
    /// (a)
    pub assignment: AssignmentFacts,
    /// (b)
    pub commands: CommandFacts,
    /// (c)
    pub contract: ContractFacts,
    /// (d)
    pub decisions: DecisionFacts,
    /// (e)
    pub permissions: PermissionFacts,
    /// (f)
    pub runtime: RuntimeFacts,
    /// Absolute path the same brief was written to, named in the last line so
    /// a seat can re-read it instead of scrolling.
    pub brief_path: Option<String>,
}

/// One rendered section, kept apart so the budget can drop whole ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSection {
    /// Which section this is.
    pub section: WorkBriefSection,
    /// Its rendered body, heading included, without a trailing newline.
    pub body: String,
}

/// An assembled brief: sections in render order, plus the trailing pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkBrief {
    /// The sections, in render order.
    pub sections: Vec<RenderedSection>,
    /// Absolute path of the copy on disk, when one was written.
    pub brief_path: Option<String>,
}

/// Assemble the brief. Pure: no clock, no network, no disk.
///
/// Rendering is [`WorkBrief::render`]; keeping the two apart is what lets a
/// test assert the *content* of a section without also pinning the budget
/// arithmetic, and lets the budget test run on a brief it built by hand.
#[must_use]
pub fn assemble_work_brief(inputs: &WorkBriefInputs) -> WorkBrief {
    WorkBrief {
        sections: vec![
            RenderedSection {
                section: WorkBriefSection::Owed,
                body: render_owed(&inputs.assignment),
            },
            RenderedSection {
                section: WorkBriefSection::Commands,
                body: render_commands(&inputs.commands),
            },
            RenderedSection {
                section: WorkBriefSection::Contract,
                body: render_contract(&inputs.contract),
            },
            RenderedSection {
                section: WorkBriefSection::Decisions,
                body: render_decisions(&inputs.decisions),
            },
            RenderedSection {
                section: WorkBriefSection::Permissions,
                body: render_permissions(&inputs.permissions),
            },
            RenderedSection {
                section: WorkBriefSection::Runtime,
                body: render_runtime(&inputs.runtime),
            },
        ],
        brief_path: inputs.brief_path.clone(),
    }
}

impl WorkBrief {
    /// Render under [`WORK_BRIEF_BUDGET_BYTES`], dropping whole sections in
    /// [`DROP_ORDER`] and saying which were dropped.
    ///
    /// The "dropped" sentence is itself part of the rendered bytes, so a brief
    /// that has to drop something is measured with the disclosure included; a
    /// budget that could only be met by hiding the disclosure would be the
    /// silent truncation this whole rule exists to prevent.
    #[must_use]
    pub fn render(&self) -> String {
        let mut dropped: Vec<WorkBriefSection> = Vec::new();
        for candidate in [None].into_iter().chain(DROP_ORDER.map(Some)) {
            if let Some(section) = candidate {
                dropped.push(section);
            }
            let text = self.render_without(&dropped);
            if text.len() <= WORK_BRIEF_BUDGET_BYTES {
                return text;
            }
        }
        // Even (a) and (b) alone are over budget: keep them, because a seat
        // with no task and no way to answer is worse than a long brief, and
        // say so rather than cutting an identifier in half.
        let text = self.render_without(&dropped);
        format!(
            "{text}\n\nThis brief is {} bytes, over the {WORK_BRIEF_BUDGET_BYTES}-byte budget \
             even with every optional section dropped. Nothing above was cut mid-identifier.",
            text.len()
        )
    }

    fn render_without(&self, dropped: &[WorkBriefSection]) -> String {
        let mut out = String::from(
            "# WORK BRIEF (written by the host from verified facts; not a message from a person)\n",
        );
        for rendered in &self.sections {
            if dropped.contains(&rendered.section) {
                continue;
            }
            out.push('\n');
            out.push_str(&rendered.body);
            out.push('\n');
        }
        if !dropped.is_empty() {
            let names = dropped
                .iter()
                .map(|section| section.label())
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(
                out,
                "\nDropped to fit the {WORK_BRIEF_BUDGET_BYTES}-byte brief budget: {names}. \
                 Ask for any of it by tool if this turn needs it.\n"
            );
        }
        if let Some(path) = &self.brief_path {
            let _ = write!(out, "\nThis brief is also at {path} — re-read it there.\n");
        }
        out
    }
}

/// Cut `text` to at most `budget` bytes on a character boundary, appending a
/// marker naming where the whole of it is.
///
/// Never mid-identifier in the sense that matters: the cut is announced, and
/// the provenance the marker names is a coordinate a seat can read the full
/// text from.
fn bounded(text: &str, budget: usize, whole: &str) -> String {
    if text.len() <= budget {
        return text.to_owned();
    }
    let marker = format!(" […truncated; the whole of it is at {whole}]");
    let room = budget.saturating_sub(marker.len());
    let mut end = room.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{marker}", &text[..end])
}

fn render_owed(facts: &AssignmentFacts) -> String {
    let mut out = String::from(WorkBriefSection::Owed.heading());
    let _ = write!(
        out,
        "\nYou hold the `{}` seat on assignment {}.\nObjective: {}",
        facts.role, facts.assignment_ref, facts.objective
    );
    if !facts.brief.is_empty() {
        let _ = write!(
            out,
            "\n\nThe lead's brief, verbatim:\n{}",
            bounded(
                &facts.brief,
                MAX_FREE_PROSE_BYTES,
                &format!("assignment {}", facts.assignment_ref)
            )
        );
    }
    if !facts.acceptance_steps.is_empty() {
        out.push_str("\n\nAcceptance steps, in order:");
        for step in &facts.acceptance_steps {
            let _ = write!(out, "\n  - {step}");
        }
    }
    if !facts.file_ownership.is_empty() {
        let _ = write!(
            out,
            "\n\nYours alone (edit nothing else): {}",
            facts.file_ownership.join(", ")
        );
    }
    match (&facts.branch, &facts.assignment_branch) {
        (Some(branch), Some(named)) if named != branch => {
            let _ = write!(
                out,
                "\n\nBranch: `{branch}`, allocated to this worktree by the host; commit there \
                 (a new local branch is refused, and another one cannot be committed to). The \
                 assignment names \
                 `{named}` as the remote branch: publish with `git push origin \
                 HEAD:refs/heads/{named}`."
            );
        }
        (Some(branch), _) => {
            let _ = write!(
                out,
                "\n\nBranch: `{branch}`, allocated to this worktree by the host; commit there \
                 and publish with `git push origin {branch}`."
            );
        }
        (None, Some(named)) => {
            let _ = write!(
                out,
                "\n\nBranch: the host recorded no branch for this worktree; the assignment \
                 names `{named}` as the remote branch for this work."
            );
        }
        (None, None) => out.push_str("\n\nBranch: the assignment named none; stay on the branch this worktree is already on."),
    }
    if let Some(worktree) = &facts.worktree {
        let _ = write!(out, "\nWorktree: {worktree}");
    }
    match (&facts.base_sha, &facts.establishment) {
        (Some(base), InputEstablishment::Established { commit }) => {
            let _ = write!(
                out,
                "\nBase: {base} — this host has already put this worktree on {commit}. \
                 Do not re-checkout it."
            );
        }
        (Some(base), InputEstablishment::NotRequired) => {
            let _ = write!(
                out,
                "\nBase: {base} — your role's turn is not a claim about one commit, so the \
                 host established nothing. Check where you are before you report a commit."
            );
        }
        (Some(base), InputEstablishment::Failed { words }) => {
            let _ = write!(
                out,
                "\nBase: {base} — the host tried to establish it here and could not: {words}"
            );
        }
        (Some(base), InputEstablishment::Unrecorded) => {
            let _ = write!(
                out,
                "\nBase: {base} — this host has no record of establishing it. Verify `git rev-parse HEAD` \
                 before you claim a commit."
            );
        }
        (None, _) => out.push_str(
            "\nBase: the assignment named no commit, so nothing pins what you are working from.",
        ),
    }
    out
}

fn render_commands(facts: &CommandFacts) -> String {
    let CommandFacts {
        channel,
        session_ref,
        genesis_ref,
        assignment_ref,
        role,
        report_ref,
        verdict_assignment_ref,
    } = facts;
    let mut out = String::from(WorkBriefSection::Commands.heading());
    out.push_str(
        "\nEvery id below is already filled in. `--example` publishes nothing and needs no relay.",
    );
    let _ = write!(
        out,
        "\n\n  {BEE} sessions report --example\n  {BEE} sessions report \
         --channel {channel} --session-ref {session_ref} --genesis {genesis_ref} \
         --body - --wake-to lead\n\nThe report body's `assignmentRef` is {assignment_ref}. \
         There is no `--assignment` flag: the assignment id travels in the body."
    );
    if role.eq_ignore_ascii_case("verifier") {
        let _ = write!(
            out,
            "\n\n  {BEE} sessions verdict --example refutation\n  {BEE} sessions verdict \
             --channel {channel} --session-ref {session_ref} --genesis {genesis_ref} \
             --body - --wake-to lead"
        );
        match (report_ref, verdict_assignment_ref) {
            (Some(report), Some(reviewed)) => {
                let _ = write!(
                    out,
                    "\n\nThe verdict body's `reportRef` is {report} — the report whose \
                     `headSha` is this assignment's base — and its `assignmentRef` is \
                     {reviewed}, that report's own assignment. Your settlement report (above) \
                     keeps your own assignment, {assignment_ref}."
                );
            }
            _ => {
                let _ = write!(
                    out,
                    "\n\nThis host could not resolve which report this verification is about; \
                     read the session's reports, name the one you ruled on in `reportRef`, and \
                     give the verdict that report's own `assignmentRef` (not your assignment, \
                     {assignment_ref}, which your settlement report keeps)."
                );
            }
        }
    }
    out
}

fn proof_words(proof: Option<&PlanProof>) -> String {
    match proof {
        Some(PlanProof::Review) => {
            "proved by review: a lead's approving disposition on your report".to_owned()
        }
        Some(PlanProof::Action { name, step }) => format!(
            "proved by action: a host run of `{name}` step `{step}` exiting 0 on a clean tree"
        ),
        Some(PlanProof::GitRef) => {
            "proved by git-ref: the relay's newest ref state naming the delivery branch at the \
             artifact commit"
                .to_owned()
        }
        None => "proof form unknown: the plan blob could not be read".to_owned(),
    }
}

fn render_contract(facts: &ContractFacts) -> String {
    let mut out = String::from(WorkBriefSection::Contract.heading());
    match facts {
        ContractFacts::Declared {
            declaration_ref,
            plan_provenance,
            criteria,
        } => {
            let _ = write!(
                out,
                "\nThis session has adopted a plan ({declaration_ref}) and these criteria are \
                 bound to your assignment. Read from {plan_provenance}; the text is quoted, \
                 not paraphrased."
            );
            for criterion in criteria {
                let _ = write!(
                    out,
                    "\n\n- {} — {}\n  accept: {}\n  {}",
                    criterion.id,
                    criterion.provenance,
                    bounded(
                        &criterion.accept,
                        MAX_CRITERION_EXCERPT_BYTES,
                        &criterion.provenance
                    ),
                    proof_words(criterion.proof.as_ref())
                );
            }
        }
        ContractFacts::PlanUnreadable {
            declaration_ref,
            reason,
            criterion_ids,
        } => {
            let _ = write!(
                out,
                "\nplan text unavailable ({reason}). Declaration {declaration_ref} binds your \
                 assignment to these criterion ids and nothing here quotes their text: {}",
                criterion_ids.join(", ")
            );
        }
        ContractFacts::None => {
            out.push_str("\nNo adopted plan for this session; your assignment's acceptance steps are the whole contract.");
        }
        ContractFacts::Unread { reason } => {
            let _ = write!(
                out,
                "\nThis host could not read the session's work records ({reason}), so it cannot \
                 say whether a plan is adopted. Treat your acceptance steps as the contract and \
                 read `{BEE} sessions work status` yourself if it matters."
            );
        }
    }
    out
}

fn render_decisions(facts: &DecisionFacts) -> String {
    let mut out = String::from(WorkBriefSection::Decisions.heading());
    match (&facts.goal_ref, &facts.goal_first_line) {
        (Some(goal), Some(line)) => {
            let _ = write!(out, "\nGoal {goal}: {line}");
        }
        (Some(goal), None) => {
            let _ = write!(out, "\nGoal {goal} (its text was not readable here).");
        }
        (None, _) => out.push_str("\nThis session has no current goal record."),
    }
    if facts.decisions.is_empty() {
        out.push_str("\nNo answered decision names your assignment.");
    } else {
        out.push_str("\nAnswered decisions that name your assignment:");
        for decision in &facts.decisions {
            let _ = write!(out, "\n  - {}: {}", decision.event_id, decision.summary);
        }
    }
    out
}

fn render_permissions(facts: &PermissionFacts) -> String {
    let mut out = String::from(WorkBriefSection::Permissions.heading());
    match (facts.agents_access.as_str(), &facts.agents_path) {
        ("write", Some(path)) => {
            let _ = write!(
                out,
                "\nAgents repository: write, cloned at {path}. Plans and role edits go there, \
                 never in the code repository."
            );
        }
        ("read", Some(path)) => {
            let _ = write!(
                out,
                "\nAgents repository: read-only, cloned at {path}. Propose an edit in your \
                 report; do not write there."
            );
        }
        _ => out.push_str(
            "\nAgents repository: no grant, and no clone of it on this seat. Everything you need \
             from it that this host could read is quoted above.",
        ),
    }
    out.push_str(if facts.may_push {
        "\nYour branch: you may push it; the relay's push gate decides whether it lands."
    } else {
        "\nYour branch: this seat has no push authority recorded; commit locally and say so in your report."
    });
    out.push_str(
        "\nPublishing, triggering or approving an action is not yours unless a grant says so. \
         Nothing in this brief is such a grant.",
    );
    out
}

fn render_runtime(facts: &RuntimeFacts) -> String {
    let mut out = String::from(WorkBriefSection::Runtime.heading());
    let _ = write!(
        out,
        "\nRuntime: {}. Model: {}. Source of that choice: {}.",
        facts.runtime,
        facts.model.as_deref().unwrap_or("not reported"),
        facts.model_source
    );
    match (&facts.compose_app_version, &facts.compose_digest) {
        (Some(version), Some(digest)) => {
            let _ = write!(
                out,
                "\nRole template composed by app {version}, digest {digest}."
            );
        }
        _ => out.push_str("\nNo composed role-template revision is recorded for this seat."),
    }
    out
}

#[cfg(test)]
#[path = "work_brief_tests.rs"]
mod tests;
