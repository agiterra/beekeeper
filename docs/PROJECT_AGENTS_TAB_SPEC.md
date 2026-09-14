# Project Agents tab — spec (2026-09-14)

Branch `work/project-agents-opus`, worktree `review-project-agents-opus`, base
`main` `149c4cc3a`. Direction: Brian, 2026-09-14, with Astra's adjustments.

## Problem

Brian could not tell why Bob was working in Tank Loop. The project page has
Roles (instructions) and Contributors (seat history) but nothing that answers
**who is working here and why**. Two defects made that worse:

- The Roles page and Contributors read the *umbrella* shelf
  (`ProjectCodingSessionShelfEntry`, one row per `sessionRef`). A worker seated
  inside a lead's umbrella — Bob, Gordan, Ira inside Loom's session — is an
  execution of that umbrella, not a shelf row, so it never counts as an agent.
  Builder reads "No agents yet" beside "1 report" from Bob.
- An agent installed for the project but never seated appears nowhere on the
  project page.

## Evidence the tab reads (no new record)

Astra challenged "a new signed roster record". Reconciled 2026-09-14; the
existing records answer every question the tab asks:

| Question | Record | Scope |
| --- | --- | --- |
| Installed for this project, as which role, at which pack revision | setup publication journal, `project_team_list_installed_roles` (`projectInstalledRoles.ts`) | this computer, this owner, this relay |
| Seated here, as which role, runtime, model, status, staged pack | kind 44223 per execution: `agentRef`, `role`, `provider`, `model`, `status`, `statusAt`, `packRef`, `projectRef`, `sessionRef` (`useGlobalCodingSessionCatalog` entries, un-umbrella'd) | any computer, readable project channels, ingress history limit |
| Assigned by whom, to do what, with what outcome | kind 44244 assignments via the declared-work projection (`usePulseDeclaredWork`): `assignerPubkey`, `assigneeActor`, `assigneeRole`, `objective`, `brief`, `acceptanceSteps`, reports, dispositions, status | any computer, newest sessions eight per page |
| Which session, its name, whether closed, how to open it | umbrella shelf entry by `sessionRef` | as the sidebar |

What they cannot answer, disclosed on the page rather than guessed:

- Installations made on another computer (journal is local).
- Who granted a seat: the 44228 grant signer is dropped by the accepted-chain
  projection, and `session.hire.requestedBy` is an unverified claim. The tab
  says "assigned by" only from a signed assignment, never "hired by".
- Which physical machine: only the provider authority pubkey exists.
- Assignments in sessions beyond the pages read.

A roster record is not needed for this slice; revisit only if a future
question is not answerable from the table above.

## Membership rule

An identity is listed under a project if and only if at least one holds:

1. a setup journal on this computer installed it for this project address;
2. an execution whose signed `projectRef` resolves to this project names it as
   `agentRef`;
3. a canonically included assignment in one of this project's sessions names it
   as `assigneeActor`.

A matching `homeRole`, channel membership, or a name alone never lists an agent.

## Sections

- **Working here** — has an execution or assignment in a session that is not
  closed. Idle, stopped and finished executions stay here while their session
  is open: idle is not absent.
- **Installed, waiting for a first assignment** — rule 1 only.
- **Previously here** — every session it appeared in is closed. This is the
  Contributors history, kept.

## Row

One row per identity; sessions expand beneath it.

- Name (managed name, then relay name, then the compact pubkey form), avatar,
  "On this computer" when a managed record exists.
- One relationship sentence: `Builder in Loom Session · assigned by Loom`, built
  only from the facts above. Without an assignment: `Builder in <session>`.
  Installed only: `Installed as Builder`.
- Installation line when present: role and installed pack revision (short sha).
- Per session (expandable): session name, role, provider · model, status and
  age, staged instructions `role @ sha` with "differs from installed" when the
  shas disagree, "Open session".
- Per assignment (expandable): objective, assigner, status (`unresolved`,
  `reported`, `settled`), newest disposition decision, brief and acceptance
  steps in a details block.
- Actions: Open session; Rename when a managed record exists on this computer
  (the native rule — `update_managed_agent` requires the local record); a link
  to the Roles tab for the role's instructions. Change role and seat repair are
  out of scope.

## Routes and tabs

- Tabs: Overview · Pulse · Agents · Roles.
- `/projects/$projectId/agents` is new. `/projects/$projectId/contributors`
  redirects there, so existing links survive. The Contributors screen, hook and
  copy are removed; their history is the Previously section.
- Roles page: "Sessions by project" moves out (the Agents tab carries it). Role
  cards additionally count executions seated inside another agent's umbrella.

## Acceptance

Tank Loop's Agents tab explains Loom, Bob, Gordan and Ira while idle, with each
one's Tank Loop role, staged instructions and assignments inspectable, and
lists installed agents awaiting their first assignment. Model and rendering
tests cover: seat inside another umbrella; installed-only; home-role-only is
excluded; closed-only goes to Previously; assignment attribution; pack
revision mismatch; unread pages disclosed.
