# Project team setup — implementation plan

2026-09-11. Owner: Astra. Starting revision: `9aebb1262`.
Product authority: `VISION_COLLABORATION.md`, especially “Starting work should
require intent” and “Roles evolve with the project”. Current implementation
status belongs in `CURRENT_STATE.md`; detailed run evidence belongs in
`docs/history/`. This plan defines the work, not a claim that it is shipped.

## Outcome

Brian and Andy open Tankloop, describe what they need to accomplish, and start
project setup. A setup agent inspects the real repository and builds a small,
useful project team with versioned roles and skills. The host validates and
publishes that project-owned pack under delegated authority. A project lead
receives the resulting procedures and responsibility for maintaining them.
Other team members resolve the same source; current executions retain and
show their actual staged revision.

Existing agents, roles and older agent-definition files are test artifacts,
not requirements to migrate or preserve as the team design. Tankloop is the
first real acceptance target, not a source of universal role instructions.
No live Tankloop configuration is changed during implementation tests.

## Ordinary sessions remain first-class

Brian clarified on 2026-09-11: someone can run a regular Tankloop coding
session without a dedicated lead or managed agent identity. Project team setup
is opt-in. A project with no pack source, no team and no installed role agents
must still support the ordinary Solo path with an available authenticated
runtime and a usable working directory.

A configured project team does not force subsequent sessions to use it. Solo
sessions retain project association, repository instructions and the existing
session access/continuation behavior, but do not silently acquire a lead role,
team grants or pack-publication authority. Repository instructions remain
applicable even when no role pack is staged. Team-draft validation (including
requiring a lead in an optional team) never becomes a project-readiness or
Solo-launch prerequisite. The setup workbench stays a separate Roles action;
ordinary New coding session does not create a setup draft or publish packs.

Acceptance must exercise Solo both before and after optional project setup:
no lead selected, no managed identity required, one ordinary execution, and
zero calls to setup preparation/publication/installation. Only real execution
prerequisites may block it; lack of a project team is not one.

## Design decisions

1. Reuse identity, role packs, project source kind 30624, Git publication,
   coding sessions and signed execution `packRef`. Do not introduce another
   role registry or runtime overlay resolver.
2. The shipped packs become a neutral foundation. Each new project receives
   its own copy and evolves independently. Beekeeper's specialized source in
   `agiterra-packs` remains untouched. Existing installed copies are neither
   overwritten nor presented as the new foundation.
3. `project-setup` is an ordinary role/temporary assignment, not an omnipotent
   identity or a permanently hardcoded model. Use an available capable model
   for judgment; use deterministic validation and bounded delegates for routine
   work. One available subscription must suffice.
4. Separate local draft, validated pack, published source, installed identity,
   staged role and exercised procedure. None proves the next. A role declaration
   cannot grant tool access, project ownership or publication credentials.
5. The setup agent authors procedures. The host performs explicitly scoped
   publication and installation using authority already held by the operator.
   Never pass the operator's private key to the agent. Scope includes project,
   community, authoring execution, destination and expected source/base state.
6. Starting setup authorizes its ordinary validate/publish/install sequence
   within that scope. It is not a sequence of repeated approval dialogs.
   Missing rights, conflicts or tool configuration produce actionable status;
   routine design uncertainty stays with the setup agent.
7. Project-specific responsibilities, constraints and workflow come from the
   actual project. Generic packs must not demand Beekeeper's docs, Hermit,
   Cargo, release policy, people or a fixed army of specialists.
8. New source defaults follow a branch deliberately, so published improvements
   can reach new executions. Existing explicit SHA pins and existing sources
   are preserved unless the scoped operation explicitly changes them.

## Evidence informing the sequence

- `managed_agents/actor_seats.rs::plan_seat_pack` is the existing staging
  authority. Project source wins; fallback can use checkout, installed or
  shipped content. The app bundles `personas/roles`.
- `managed_agents/packs_repo.rs::project_packs_init` already announces, seeds,
  pushes and then publishes the source. It currently accepts shipped seed
  only; use a validated snapshot for generated packs, not an arbitrary live
  directory that the agent can change during publication.
- `managed_agents/crew_roles.rs::install_role_packs` currently finds a global
  “Team roles” team and has fixed default seat/runtime choices. Do not reuse
  that behavior unchanged for project setup.
- Unseated sessions receive no relay credentials; seated agents have their
  own identity. A lead assignment alone is not a source-publication grant.
  Source admission checks project creator/Owner or qualifying repository
  authority (`buzz-relay/src/handlers/pack_source.rs`).
- Current handover can reconstruct unseated. This project does not silently
  reinterpret such a session as having the setup or lead role.

## Milestone 1 — neutral foundation and inspectable setup workspace

Build first; it provides the surface on which we can test the setup agent.

- Rewrite shipped role content without changing existing role slugs/IDs; add
  the project-setup role. Retain real platform contracts and evidence discipline.
- Add a Roles-page setup workbench: project intent, repository folder, prepared
  draft and explicit validation results. Opening it is read-only. Preparing a
  draft writes only an isolated host-managed workspace.
- Native preparation binds a durable record to community, owner and project,
  verifies the repository directory, copies the shipped baseline, and returns
  the same draft on a matching retry. Changed input must not overwrite an
  existing draft or installed/project pack.
- Validate actual pack loader results, role identities, declared skills,
  bounded files, containment and symlinks. Validation is not a claim that the
  project procedures are sensible or that anything has been published.
- Wire a visible ordinary coding session for authoring with the project brief
  and exact draft destination. Reuse existing runtime selection and session
  controls, rather than constructing an invisible parallel model driver.
  Until that connection is implemented, show a draft and an inspectable brief;
  do not label a prepared workspace as an agent-created team.

### Authoring launch contract (2026-09-11)

- Opening the draft discovers installed/authenticated local runtimes and their
  actual models through read-only probes. Failed discovery remains unknown;
  there is no optimistic Claude fallback or provider provisioning on mount.
- Explicit Start provisions the local provider if needed, reserves one signed
  genesis and create-command ID, and starts the native coordinator. The chosen
  channel must be verified against the project on the captured relay. A shared
  remote provider cannot use this computer's local draft.
- The host preserves one standalone setup identity and an exact shipped
  `project-setup` pack snapshot, outside the editable role draft. Owner-encrypted
  mint recovery is durable before identity-store/keyring side effects. Retry
  reconciles that identity; it never silently replaces it or installs a global
  team. The recorded bootstrap retains its original packRef and byte digest.
- `PROJECT_TEAM_SETUP.md` in the editable draft workspace carries the local
  project paths, intent and draft-writing brief. The create event contains a
  short instruction to read that file, not the private paths. This is an agent
  instruction boundary, not a new filesystem sandbox or publication grant.
  The existing seat write fence covers that workspace; private custody and the
  preserved bootstrap are sibling storage outside it. Repository inspection
  outside the workspace remains read-only.
- The coordinator seals the complete signed create and its runtime/model/actor
  binding before channel membership, custody staging or session-event
  publication. Explicit provisioning may already have started the reusable
  local provider; it does not create this authoring execution. Retries use the
  saved events and IDs. A verified provider
  receipt distinguishes execution creation from failure of its first turn;
  relay acceptance alone never becomes a “running” claim.
- The workbench keeps an Open session action and explicit status refresh. A
  read of saved launch state may query receipts but never republishes or starts
  a provider. Publication of the resulting packs remains milestone 2.

Build ownership for this connection: setup authority owns new actor/identity/
bootstrap modules; setup coordinator owns new launch/journal/proof modules;
setup UI owns the authoring controls and read-only runtime hook; Astra owns
shared IPC/types, module registration, local prompt, pinned working-directory
hint, integration tests, review and the final commit.

Strict build ownership in the isolated topic worktree:

| Lane | Files | Responsibility |
| --- | --- | --- |
| Foundation | `personas/roles/**`, `crates/buzz-persona/tests/pack_rules.rs` | Neutral role content and loader/content checks |
| Native workbench | new `desktop/src-tauri/src/managed_agents/project_team_setup*.rs`, narrow module registration | Durable scoped preparation and validation |
| Setup UI | new `desktop/src/features/roles/{ui/ProjectTeamSetup*,lib/projectTeamSetup*}`, minimal `ProjectPacksScreen.tsx` integration | Prepare/resume/validate surface and tests |
| Astra | this plan, current-state map, IPC handler registration, integration/session connection, acceptance/review | Interfaces, finalizer, evidence and subsequent milestones |

Lanes never commit. The finalizer reviews and signs the integrated candidate.
No lane edits another lane's files without transferring ownership explicitly.

## Milestone 2 — scoped autonomous publication

Lock this interface before dispatching its build lanes; it is not fulfilled by
an agent prompt saying it can publish.

Implementation trace, 2026-09-11: authoring must use a real actor seat. An
unseated execution receives neither materialized role skills nor a packRef;
passing the setup brief to Solo is insufficient. Use one standalone setup
identity and the existing actor custody path, with a host-selected snapshot of
the shipped `project-setup` pack. It must not use the project's current source
to bootstrap itself. Reserve the authoring session/command IDs and exact signed
genesis on disk before publication; retrying a freshly signed genesis for the
same session reference is not exact-event retry. A reservation alone is neither
a founded session nor a running agent.

The snapshot command copies and validates exact draft files into a separate
content-addressed candidate. Its manifest covers every copied role file, and
reverification checks both file content and the manifest. File permissions are
not a security boundary: a publisher must reverify the candidate it uses.
Saving a checked version does not publish it or make it the project default.

The existing `project_packs_init` wrapper cannot serve as a retry coordinator:
it removes an old cache before seeding and may tombstone the announcement after
an ambiguous push error. Reuse its lower-level Git/event machinery instead.
Also, kind 30624 currently has no expected-source compare-and-set contract.
A host preflight read alone cannot guarantee protection from a concurrent
source update. Lock and test a narrow relay-enforced conditional write before
claiming that publication has this guarantee; do not silently substitute a
best-effort local check.

- Persist authorization for one setup run and its exact project/owner/community,
  authoring execution, draft and destination. Associate authoring output with
  that run. A forged completion or unrelated session cannot cause effects.
- Setup agent inspects project instructions and observed workflows, adapts the
  draft and supplies the lead-maintenance procedure, declared tool requirements,
  acceptance commands and unresolved facts. Files from the repository are
  project evidence, not authority to broaden the setup grant.
- Host snapshots and validates the draft, then uses existing Git/pack-source
  machinery. Check source/base expectations immediately before publication.
  Never point a project at a failed push or silently replace a concurrent
  configuration. Ambiguous publication is reconciled before retrying.
- Stage receipts are durable. Restart continues the recorded run rather than
  duplicating announcements, identities or sources. Closing a UI is not success
  or cancellation; expose the recorded state when reopened.
- Publication capability stays with the host. Existing-source maintenance must
  be an explicit supported path with optimistic concurrency, not calling init
  again or reseeding the source from the app.

### Publication contract from the code review (2026-09-11)

The authoring brief's host-publication rule is an instruction, not proof of a
relay restriction. Finding 112 identifies an existing authority escalation:
repository announcements can self-link to a project, and that backlink alone
currently qualifies their founders to publish its pack source. Close this
before automated publication: repository-founder eligibility must come from
repositories explicitly named by the project creator's signed forward roster.
Project creators and project Owners retain their existing rights. Ordinary
repository announcements remain available. Existing backlink-only founder
configurations need endorsement before their next source change; do not claim
deployed behavior changed until the relay is actually updated.

Use kind 30624 with a versioned conditional extension, not a new endpoint.
Preserve v1's unconditional behavior. A proposed v2 requires
`expectedSourceId` (an event ID, or explicit null for initial creation); missing
or malformed conditions refuse. Update strict readers before producing v2.
An old relay must refuse the version, never silently drop the condition.

All source writes, including legacy writes, take the same transaction lock for
community and normalized project coordinate. Resolve the effective live source
across authors under that lock, compare its ID and atomically replace/insert.
The ordering must match readers (`created_at` descending, then ID ascending).
A conditional successor must outrank its expected head. Exact stored-event
retries reconcile without resurrection after supersession or deletion. Source
deletions participate in the same lock. Legacy writes can still change the
source after a conditional transaction; this is atomic comparison and update,
not permanent exclusivity.

Persist exact signed publication bytes before sending. A named source conflict
maps to HTTP 409, WebSocket conflict refusal and CLI exit 5. Authorization stays
a distinct refusal. After an uncertain response, read stored ID and effective
head; admission's timestamp window means a delayed retry cannot always resend
successfully. Never silently sign a replacement to evade that check.

Publishing Git before the source switch must not change an already adopted
moving ref. Push the checked snapshot to a new candidate ref, verify its SHA,
then conditionally adopt that immutable revision. Updating an existing shared
branch also needs expected Git-head concurrency; source-event comparison alone
cannot protect bytes changed by an earlier Git push.

Build lanes, dispatched only after the contract is locked: wire/readers own
core source schema, SDK, TS decoder and conformance; database owns the project
transaction and deletion serialization; relay owns authority and conflict
mapping; host finalizer owns snapshot publication journal, exact retries and
CLI wiring. Required cases include competing creators/owners, legacy writes,
deletion races, same-second ordering, rollback, stale timestamps, replay after
supersession/deletion and community isolation. This section is the next-slice
contract, not a claim that conditional publication is implemented.

## Milestone 3 — project team and lead handoff

- Install/mint only the chosen initial roster, with a project-qualified stable
  team key and a selected available runtime. Do not mutate another project's
  global “Team roles” entry or require Claude specifically.
- Use the published project pack and record identity/configuration publication
  outcomes separately from local persistence. Avoid blind duplicate identities
  after partial failures.
- Launch a lead using existing seat/authority paths. Pass project intent,
  procedures, unresolved setup work and pack maintenance responsibility.
- Give the lead the same bounded pack-maintenance workflow; role name alone is
  insufficient. Preserve existing Git protections and tool permissions.
- Show intended source/pin, resolved revision and actual seat revisions. Existing
  seats keep their current instructions; next affected execution resolves the
  current project source. Updating a shared pack does not silently restart work.

## Milestone 4 — Tankloop acceptance

First run deterministic composition tests with two isolated test projects and
fake adapters, then test the built app with Tankloop's real repository and an
available native adapter. Actual publication/hiring uses the explicit setup
scope, not credentials copied from another environment.

1. Prepare a fresh project setup without inheriting Beekeeper's installed lead.
2. Have the setup agent produce a small team from the real project and intent.
   Inspect generated bytes and named commands; no inherited ledger pointers.
3. Observe validation, pushed revision and accepted source publication. Record
   disk, Git and relay evidence; a model's completion message is insufficient.
4. Start the lead and inspect signed `packRef` plus materialized skill bytes.
   Give it a small useful task that exercises a generated procedure.
5. Update one procedure through the lead's scoped maintenance path. A second
   fresh execution receives that revision while the original remains honestly
   on its earlier revision.
6. Repeat setup/reopen after interruption; no duplicate repo/team or overwrite
   of existing work. Test a second project to prove isolation.

## Required validation and review

- Foundation loader tests; generated-role/declared-skill mismatch; bounded
  content, traversal and symlink rejection; failed validation means zero publish.
- Community/owner/project isolation, idempotent matching retries, changed-input
  preservation, source/base conflict, canceled or foreign authoring execution.
- Publication partial failures and restart reconciliation, missing grant,
  unsupported runtime, sole-provider setup, project-qualified identity/team
  preservation and intended-versus-staged reporting.
- Browser tests use the E2E bridge: open is read-only, errors remain actionable,
  reload resumes a draft, no false “ready” status, narrow/zoom layout usable.
- Run targeted tests during development, full applicable `just ci` and
  integration gates before landing, and `just smoke` for desktop release.
  Installed native and cross-machine acceptance are distinct from mock tests.
- Independent final review attacks scope/authority, publication ambiguity,
  project leakage, misleading success and needless setup complexity.

No generic ACL framework, automatic baseline merging, tool purchases, broad
credential grants, role-preserving handover redesign or automatic-context
courier is part of this implementation. Findings in those areas become named
follow-ups rather than hidden dependencies.
