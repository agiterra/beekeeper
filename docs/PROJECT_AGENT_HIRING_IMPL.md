# Project agents and role selection — implementation contract (2026-09-14)

Binding for the lanes correcting how a project's lead discovers, selects and
hires agents. Base: `work/project-agents-opus` `95a2f6f58` (the unlanded
project Agents tab). Worktree `review-project-agent-hiring-opus`, branch
`work/project-agent-hiring-opus`. Evidence and live results go into a dated
report under `docs/history/`, not here.

## Brian's model (binding)

- **Role**: a project-specific job — responsibilities, instructions, skills.
- **Agent**: a durable, named project participant with one primary role.
- **Assignment**: a bounded task given to an agent.
- **Execution**: runs an assignment on an available provider/model and host.
- The lead selects the agents each task needs; a roster is not a launch list.
- Ordinary Solo sessions need no project setup or dedicated agent.

## Confirmed root cause

Tank Loop setup minted Loom, Builder, Runner, Verifier, Designer and Project
Setup (`team_id f54498d1…`). Loom's hires seated Bob, Gordan and Ira
(`team_id 8bec5451…`, the Beekeeper crew, created 2026-08-28).

The founder's desktop answers `session.hire`. `chooseIdentity`
(`desktop/src/features/coding-sessions/lib/codingSessionHirePolicy.ts:644`)
filters **every managed agent on this computer** by `homeRole === role`,
excludes live seats, then sorts by staged pack, provider preference and
**name**. Both groups had packs and no pinned runtime, so the name decided:
"Bob" < "Builder", "Gordan" < "Runner", "Ira" < "Verifier". The candidate
list (`CodingSessionHireHost.tsx:145`, `useManagedAgentsQuery`) carries no
project fact. The umbrella's `projectRef` was used only for the catalog and the
registry (`useCodingSessionHire.ts:636`). Staging then resolved Tank Loop's
30624 pack by **seat role** (`actor_seats.rs plan_seat_pack`), so Bob received
Tank Loop's builder instructions.

Nothing records project association in a form hiring reads. The setup journal
records `installation.roles[{role, agentPubkey, packRef}]` per project, on this
computer only, read only by UI. The hire wire names a role, never an identity.
Hive's relay rejects any unknown hire key (`coding_session_lifecycle_command.rs`
exact key sets), so the lead cannot name an identity without a relay deploy.
No lead-facing command lists a project's agents. Setup's lead first turn lists
none. Team Start's first turn always says nobody else is on this computer,
because `useCodingSessionFoundedStart.ts` passes only the lead seat.

## Association: what records it

1. **Local, authoritative for hiring.** `ManagedAgentRecord.project_ref:
   Option<String>` (normalized `30621:<owner-hex>:<dtag>`), serde-default.
   An agent belongs to at most one project. Its primary role stays
   `home_role`. Set by:
   - project setup installation, for each installed role's agent;
   - an idempotent **backfill** from this owner's setup journals, where the
     record exists, `project_ref` is unset and `home_role == role`. It never
     overwrites a different project and logs the conflict. This migrates
     Tank Loop's existing agents. Bob and other unassociated agents stay
     unassociated;
   - `associate_managed_agent_with_project({pubkey, projectRef})`: explicit
     and permanent. It refuses builtin agents, setup actors, agents without a
     `home_role`, and agents already associated with another project. The same
     project is a no-op that returns the agent. It changes no relay ACL,
     channel membership or role.
     **Authority is enforced natively (review finding 1):** before persisting,
     the command reads the signed project head. That is the newest 30621 by
     the coordinate owner with an exact `d`, signature verified; if it can't
     be read, the command refuses. The signer is authorized if it is the
     creator. Otherwise it must be an owner or collaborator on the newest
     39010 signed by the relay's NIP-11 `self` key, or, only when no 39010
     exists, on the head's bootstrap `p` tags. Anything unreadable or
     unverified refuses (`project_association_authority.rs`). The head's
     `buzz-access` tag records `project_public`.
2. **Shared, for discovery.** The agent's owner-signed kind:30177 content gains
   `home_role` (when set) and `project_digest` =
   `buzz_core::project_agent_association::project_agent_digest(project_ref)`.
   The relay needs no change: 30177 has no content validation and old readers
   drop unknown keys. **Readers accept a claim only from the project's creator
   or a roster owner or collaborator.**
   - **Publication and privacy (review finding 4).** The digest is an equality
     key, not a secret. Coordinates are guessable, so anyone can hash
     candidates and correlate agents. A digest is therefore published only
     when a signed head read found the project public (`project_public ==
     Some(true)`). A private project's agents are never announced. An
     unverified project neither publishes nor withdraws, and a best-effort
     verifier reads heads after event sync and after installation. A private
     project's roster reaches its lead through the host-built first message
     instead.
   - **No withdrawal by a stale host (review finding 3).** A computer that
     sees this owner's association on the wire (inbound 30177, or the relay
     head checked just before publishing a digest-less 30177) keeps it as
     `carried_project_digest` and republishes it. The only intentional
     withdrawal is a local record that knows the project is private. A failed
     relay read withholds the digest-less publish rather than risk a
     withdrawal. A carried digest is not membership; hiring reads only
     `project_ref`.
   - **Withdrawal provenance (review 2026-09-15).** Visibility is re-read for
     every associated project on each verifier run, so a public→private change
     is noticed. A verified-private result publishes `project_withdrawn: true`
     instead of a digest. That signed, same-owner marker is sticky: an inbound
     marker, or a relay head carrying one, sets `project_publication_withdrawn`
     and drops any carried digest. The publish guard reads the relay head
     before any row whose digest isn't this computer's own verified-public
     digest, and never publishes a carried digest over a marker. Only this
     computer's own verified-public result clears the withdrawal. A digest-less
     record without a marker still never clears anything, which is the
     stale-host case.

### Why existing records were insufficient

- The setup journal is local and covers setup-minted agents only.
- `team_id` is a random saved-group id with no project in it.
- The 30621 head's `a` agent members can only be edited by the project's
  creator. Tank Loop's creator is Andy, not Brian.
- The 39010 roster grants access, and association must not grant access.
- A new kind or hire key needs a hive deploy.

Kind 30177 is already published for every agent on hive; checked 2026-09-14
with `bee events query --kinds 30177`.

## Hiring rules (authoritative path)

Let `P` be the umbrella's project (`codingSessionHireUmbrellaProjectRef`).

- **`P` set:** a candidate must have `projectRef == P` (see
  `agentMaySeatInProject`), `homeRole == role`, must not be a live seat in the
  umbrella, and must not be a setup actor. Matching role names, installed
  packs, past seats and role text are never evidence.
- **`P` null:** only agents with no project association may be seated, so a
  project's agent is never pulled into unrelated work.
- **Refusals:**
  - Project agents for the role all live: `HIRE_ROLE_BUSY` (unchanged).
  - No project agent for the role on this computer: new
    **`HIRE_NO_PROJECT_AGENT`**. The reason names the project and role, gives
    the *count* of other agents here with that role that are not this
    project's, and says borrowing is not supported. The remedy is to install
    project roles or associate an agent on the project's Agents tab.
  - There is never a fallback to another agent.
- **Borrowing** (seating another project's agent) is refused everywhere in
  this slice. A lead, a person or a picker cannot do it.
- **Primary role is fixed.** A seat's role equals the agent's `home_role`.
  Model or task choice never changes it, and a builder cannot be seated as a
  verifier, which preserves independent review.
- **Native defense in depth.** `stage_coding_session_actor_seat` and
  `preview_coding_session_seat_pack` accept `requireProjectRef` and
  `newSelection`. New selections pass `newSelection: true`: hire, team launch,
  seat picker, the pack preview and the setup lead launch. The host then
  refuses three cases:
  - a project session's seat for an agent of another project, or of no
    project (`SEAT_NOT_PROJECT_AGENT`);
  - a projectless session's seat for any project's agent;
  - a role other than the agent's `home_role` (`SEAT_ROLE_NOT_PRIMARY`).
  Resume and restage pass neither, so historical executions stay resumable and
  attributed.
- **The seat picker's unknown scope.** When no execution has reported a
  project, the join signs none, so only agents of no project are offered. A
  chosen agent with a primary role has its role box locked to that role.
- **Installed-pack fallback.** With no project pack source,
  `resolve_local_seat_pack` borrows another agent's installed pack only from
  an agent with the same association, never another project's.
- **Reinstall.** "Install team roles" and setup retries carry `project_ref`
  forward. A retry for a different project refuses; it never moves the agent.
- **Lead discovery:** `bee projects agents [SLUG --owner HEX | --project
  COORD]`, defaulting to `$BUZZ_PULSE_PROJECT` inside a seat. It reads the
  roster, queries 30177 by authorized authors, matches the digest, and prints
  `[{pubkey, name, role, owner, owner_role, verified}]`.
  - `verified` is present in JSON and compact (review finding 2). Unverified
    claims are never printed.
  - A private project prints `[]` with a note.
  - A project this identity cannot read is not-found, with no fallback rows.
  - Setup's lead first message lists the project's agents on the hosting
    computer, and Team Start's does too. Help text says hires are
  answered by the session founder's computer, which seats only agents it holds
  that belong to the project. The setup lead's first turn, Team Start's first
  turn, the hire help and the shipped lead `hire` skill all name the command.
  Tank Loop's published pack text is not changed.

## UI rules

- **Roles** answers "what jobs and instructions does this project define?"
  **Agents** answers "who belongs to this project, with what primary role,
  doing what?"
- The project Agents tab has three groups:
  - **Project agents:** associated, local plus published by authorized
    authors.
  - **Borrowed participants:** executions or assignments in open project
    sessions by non-associated identities, labelled "not a <project> agent".
  - **Previously here:** closed history, attribution untouched.
- **States** come from the newest execution, never from an open session alone:
  - *Working* — `starting`, `running`, `waiting_for_input`.
  - *Idle* — `idle`.
  - *Disconnected* — `disconnected`, `interrupted`.
  - *Available* — associated, local, no live execution.
  - *On another computer* — published association, no local record: shows the
    owner and that it cannot run here.
  - *Historical* — closed sessions only.
- Rename wherever an agent is encountered and a local record exists. It is
  pubkey-keyed and changes neither role nor association.
- "Associate with <project>" appears for local unassociated agents that have a
  primary role. For a viewer who cannot write the project it is shown
  disabled, with the reason next to it, rather than hidden. Native refuses
  regardless of the UI.
- Published claims carry `authority`. A roster read failure leaves
  non-creator claims under "Project authority not verified", and they are
  never counted as project agents.
- A private project shows no published agents and says why.
- A local agent carrying this project's digest reads "Associated from another
  computer" and is offered Associate.
- Hiring readiness: a notice names each role with work evidence in the project
  but no associated agent, and who did that work. It never preselects or
  infers.
- Execution details show the staged `role@sha`, runtime/model and host key.
  Pubkeys stay in details.
- The lead picker, bench and seat picker for a project session list only that
  project's agents, with a count sentence for the ones excluded. Solo is
  unaffected. A projectless session lists unassociated agents.
- Setup ends with the roster (names, roles, association) and one next action.
  It never reports complete or ready while an installed role's agent lacks the
  association the lead needs.
- Keep `/projects/$projectId/contributors` → `/agents`, rem text sizes, and
  layouts that survive a narrow window and larger text.

## Shared primitives (finalizer-owned; lanes import, never edit)

- `crates/buzz-core/src/project_agent_association.rs`
- `crates/buzz-core/testdata/project_agent_association/vectors.json`
- `desktop/src/shared/lib/projectAgentAssociation.ts` (+ test)
- `desktop/src/shared/api/tauriProjectAgents.ts`
- `ManagedAgent.projectRef` in `desktop/src/shared/api/types.ts` and
  `tauriManagedAgentRecord.ts`

## Lanes and exclusive ownership

| Lane | Owns |
| --- | --- |
| N native | `desktop/src-tauri/**` |
| K core+CLI | `crates/buzz-core/src/coding_session_lifecycle_command.rs` (refusal code list only), `crates/buzz-cli/**`, `personas/roles/lead/**`, `crates/buzz-persona/tests/**` |
| H hire host | `desktop/src/features/coding-sessions/lib/codingSessionHire*.ts` (+tests), `hooks/useCodingSessionHire.ts`, `ui/CodingSessionHireHost.tsx` (+test), `lib/codingSessionSeatedCreate.ts`, `lib/codingSessionActorSeatCustody.ts`, `lib/codingSessionSeatDeps.ts` |
| S session start | `desktop/src/features/coding-sessions/ui/founded/**`, `ui/NewCodingSession*.tsx`, `ui/AddCodingSessionProviderDialog.tsx`, `lib/codingSessionLeadCandidateGroups.ts`, `lib/useCodingSessionSeatDraft.ts`, `lib/codingSessionCrewLaunch.ts`, `ui/useCodingSessionCrewLaunch.ts` (+tests) |
| A agents | `desktop/src/features/project-agents/**`, `desktop/src/features/agents/**`, `desktop/src/app/routes/projects.$projectId.agents.tsx` |
| R roles | `desktop/src/features/roles/**` |
| E browser | `desktop/src/testing/e2eBridge*.ts`, `desktop/tests/e2e/**`, after A/R/S land |

Lanes never commit, stage, push, install or run the app. One finalizer
commits. Cross-lane needs go to the finalizer. Never touch
`/Users/brian/Projects/beekeeper/beekeeper` or `review-project-agents-opus`:
both are hot.

## Known limits (disclosed, not fixed in this slice)

- **Naming an agent in a hire.** It needs a hive relay redeploy, because the
  hire payload's key set is closed. The host picks among the project's agents
  by role, deterministically by staged pack, then provider preference, then
  pubkey; name is never a tie-breaker.
- **Borrowing** is refused everywhere, not implemented.
- ~~**A second computer holding the same agent.** If it has no `project_ref`,
  its next republish of kind:30177 drops the digest, and readers keep the
  newest event.~~ Superseded by the carried-digest guard (review finding 3).
  A local record there is still not reassociated from the wire: associating
  it on that computer is explicit.
- **The author owns the agent.** A published claim is checked against the
  author's project authority, not against the agent's owner attestation.
  Hiring is unaffected because it reads local records only.
- **Private-project discovery in the CLI** returns nothing. The lead's first
  message is the source for a private project's roster.
- **The withdrawal marker is visible.** Every agent of a verified-private
  project publishes `project_withdrawn: true`. It reveals no project identity
  or digest, only that the association is intentionally unpublished.
- **A withdrawal can outlive a return to public.** A computer that learned of a
  withdrawal from another computer, and holds no `project_ref` of its own,
  stays withdrawn. Its next publish can put the marker back over the owner's
  new public digest, until the associated computer republishes. This errs
  toward privacy over discovery.
- **Private-project roster freshness.** The lead's first message is a
  point-in-time roster, so agents associated after launch are absent after
  resume. This is a documented follow-up.
- **A carried digest can outlive intent.** No dissociate action exists, so an
  association published once is carried by every computer of that owner
  until a record there verifies the project private.

## Acceptance

- **A.** Two projects with distinct agents sharing role names: each lead
  discovers and hires its own Builder, Runner and Verifier.
- **B.** Renames do not change selection.
- **C.** Another project's matching-role agent cannot enter through the CLI,
  the host, a picker or native staging.
- **D.** A missing local agent yields `HIRE_NO_PROJECT_AGENT` with a remedy,
  never borrowing.
- **E.** Retry, reopen, reinstall and backfill create no duplicate agents and
  no reassociation.
- **F.** Bob's historical work stays attributed to Bob, labelled borrowed.
- **G.** Solo works before and after setup.
- **H.** The UI shows each state, including in a narrow window and at larger
  text sizes.
- Plus one real run: discovery → hire → staged role → assignment → report,
  with disposable identities and projects.
