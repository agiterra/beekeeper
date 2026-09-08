# Work coordination visibility — step 3

Product authority: `VISION_COLLABORATION.md`, especially “Observe parallel work
before the push”, and `COLLABORATIVE_WORKSPACE_PLAN.md` step 3. Current findings
and implementation status belong in `SESSION_STATE.md`. This is an execution
specification, not a claim that the described screen is implemented.

## Outcome

Before two participants finish competing implementations, either can open the
project's existing Pulse view and understand the declared work, who is responsible,
the named scope, and where to inspect or discuss it. A human and an agent are
participants under the same existing authority rules.

The first increment is an inspectable projection of existing work records. It
does not require another registry, a model call, or an approval before work starts.
It also does not claim to detect semantically duplicated features from filenames.

## Existing records to extend

- Project Pulse kind 44240: author, plan/note/blocker/handoff/milestone, prose,
  `codeAreas`, branch, session reference, and same-author supersession. Reuse the
  existing project-scoped fold; its `active` flag means “not superseded”, not
  “this person is coding now”. Its `pu-session` reference is author-controlled;
  resolve the session independently before attaching a declaration to it. The
  reference grants no session, project, repository or execution authority.
- Team assignments: `assigneeActor`, role, objective, brief, `branch`, `baseSha`,
  `fileOwnership`, `acceptanceSteps`. Reuse the canonical team fold's accepted
  assignment IDs and join only to their verified source payloads in that scope.
- Team reports and dispositions: reported head/files/tests, departures, residuals,
  and settlement. A report is evidence of a report, not automatic completion or
  independent verification of its tests.
- Decision requests and answers: existing `blocks` references and the named
  decision holder. Do not infer a general dependency graph from prose or treat
  every unanswered question as a project-wide stop.
- Session coordination: exact channel/session/genesis and execution target,
  reported goal/name, durable lifecycle, and separately observed reachability.
- Existing checkpoint/shared-path readers remain a separate evidence source.
  Validate their producer, authority and repository binding before describing
  their rows as actual file overlap or their identifiers as Git commits.

Primary implementation references: `crates/buzz-core/src/pulse.rs`,
`pulse_fold.rs`, `coding_session_team_transaction.rs`,
`coding_session_team_transaction_fold.rs`, and
`desktop/src/features/project-pulse/`. Existing CLI surfaces are `bee pulse`
and the coding-session team operations; retain those entry points.

The current overlap path cannot be treated as an implemented producer/consumer
contract: `CodingSessionObservationCheckpoint` has no `files` or commit field, and strict
observation decoding rejects extra fields. `pulse_mission.rs` nevertheless scrapes
raw `body.files` before the authority projection and uses the event ID as `sha`.
`PulseOverlapSide` also has no repository identity. Do not activate this dormant
path by adding one checkpoint field; replace its assumptions with accepted,
repository-qualified evidence when implementing a later comparison increment.

## Smallest useful increment: declared work in Pulse

Add one section to the existing project Pulse view. For each visible declaration,
show the objective or plan, responsible participant, declared paths, branch/base
when reported, record age, and a route to the source session or event. Keep the
full brief, tests and identifiers behind disclosure.
Pulse already renders plans with `PulseEntryRow`. Reuse or regroup those rows;
do not add a second plan list alongside the same declarations. The new value is
the joined assignment view across visible sessions, with useful source fields
and accurate unresolved/settled state.

The existing mission response exposes owed assignment IDs rather than complete
assignment payloads. The inspector's trusted transaction projection also omits
assignee actor, branch and base. Add a bounded projection seam from the existing
verified mission gather/native fold; merely drawing another card from the current
mission wire cannot supply those facts. Preserve source event identity and the
canonical inclusion/settlement decision through that seam.

Use explicit provenance labels: “Plan posted”, “Assigned”, “Report submitted”,
or a disposition the canonical fold actually establishes. A plan without a
session stays visible as its author's declaration. An assigned actor is the
responsible participant; the assigning author's identity remains inspectable.
Show missing branch, scope or assignment evidence as missing. Never promote
lease reachability into proof that a participant is editing a particular file.

The section includes authorized visible work across participants and machines,
not just agents owned by this computer. Deduplication keys include community,
project, channel, session/genesis and source event as applicable. A local agent
catalog is not the source of truth for whether another participant exists.

Closing an execution does not silently settle its assignments. Supersession and
settlement come from their existing folds. Preserve historical declarations and
allow a reader to distinguish them from current unresolved assignments.
The current canonical settlement requires an approving disposition and the
assignee's acknowledgement; this view reports that existing fact without adding
a new approval requirement or changing admission behavior.

## Scope comparisons

Keep three facts distinct: declared scope, reported changed files, and an
independently observed change. The first increment must not blend them into a
single red “conflict” badge.

Automatic overlap comparison requires an unambiguous shared repository identity.
A project can contain multiple repositories. Matching `src/main.rs`, a branch
name, or a project name alone does not prove both declarations concern the same
file. Missing or conflicting repository binding disables that comparison and
discloses why; it does not suppress the underlying declarations.
Lifecycle creates have optional `repoRef`/`projectRef`, but an umbrella can contain
multiple executions. An assignment targets an actor/role in that umbrella, not
one exact execution, so one create's repository cannot automatically bind every
assignment. An author-controlled Pulse session pointer cannot supply this proof.

This first increment displays declarations without automatic scope comparison.
Existing Pulse paths reject a trailing slash, while assignment `fileOwnership`
validates bounded strings without a shared exact-file/directory/glob grammar.
Do not reinterpret either field silently. A later contract must first establish
repository binding and explicit scope semantics; then directory comparisons can
use path segments rather than string prefixes, preserving case. Do not resolve
local symlinks or invent glob semantics. An ambiguous path is uncomparable, not
an inferred match.

An overlap is advisory: name both sources and what matched, then let an authorized
participant inspect or discuss it. Reading a source link does not schedule an
agent. Notifications, conversation, and execution grants are different actions.
Do not require a human to interpret the row; agents can read the same records
and use their existing authorized communication and decision operations.

Different files can implement the same feature. Semantic duplicate detection is
explicitly outside this deterministic comparison. A later agent judgment should
cite the work records and product intent, record one decision with a terminal
disposition, and let unrelated work continue.

## Reads and performance

Reuse current Pulse/session query caches and canonical verification adapters.
Do not query once per rendered card or repeat signature/fold work on hover,
scroll, disclosure, or unrelated channel events. Bound history reads and result
rendering; retain cancellation and community-switch isolation.

Failed, incomplete, or capped reads carry source-specific limitations. “No work”
is allowed only for a successful empty read within its stated scope. A project
channel scan cannot claim to discover sessions outside that channel set.
“Check again” refreshes existing reads; it never starts a model or another agent.
The existing mission adapter starts from open sessions and selects at most eight.
Reusing it unchanged is insufficient for unresolved assignments in ended sessions.
Extend its selection with bounded pagination over visible sessions, including
ended ones, and disclose the scan limit. Do not replace the cap with unbounded
per-session requests or advertise completeness before the relevant pages resolve.

## Execution lanes and file ownership

Before code changes, record exact file claims in the shared orchestration mailbox.
Fable retains all CI recovery/provider and host custody files. This slice must
not edit them or reuse its Cargo target, browser port or scratch infrastructure.

1. Root finalizes the reuse/authority audit and the projection schema. If existing
   records cannot support trustworthy scope comparison, deliver the declared-work
   view first and name the missing binding instead of inventing a second registry.
2. Projection lane owns a small new pure projection module, its regression tests,
   and only the necessary existing query adapter integration files.
3. View lane owns the new Pulse section, its interaction tests, and the existing
   Pulse view insertion. No duplicate state machine or authored authority inference.
4. Independent review checks provenance, multi-repository isolation, incomplete
   reads, old declarations, accessibility, and whether any new gate was introduced.
   Root integrates and commits; build lanes do not commit.

## Acceptance

- Two visible participants in different sessions appear from shared records;
  one may be a human plan author and the other an assigned agent.
- The view is useful before a commit or checkpoint exists.
- Superseded plans, unsettled reports, ended executions, and explicit settlement
  produce different truthful states. Missing assignment payloads do not become
  fabricated empty work.
- Another project or community with the same names/paths never contaminates the
  view. Two repositories in one project do not create an inferred file conflict.
- Existing grants determine visibility; an unreadable participant's records do not
  leak through counts, error detail, or overlap labels.
- Source navigation opens the correct session/thread and sends no message or turn.
- Partial/offline/capped reads retain recovered rows and a clear limitation.
- Repeated renders, scrolling and disclosure do not trigger network reads/folds.
- Exercise mock-bridge UI with two owners, narrow width, zoom, and stale evidence;
  then use the two-machine runbook for real relay/native acceptance. Browser mocks
  do not establish cross-machine delivery or native responsiveness.

This increment prepares the broader coordination and absent-participant handover
work. It does not itself introduce a cross-machine execution claim, automatic
takeover, or a new task scheduler.
