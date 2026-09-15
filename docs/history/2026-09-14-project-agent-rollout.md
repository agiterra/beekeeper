# Project-agent hiring rollout for Beekeeper — prepared 2026-09-14

Branch `work/project-agent-hiring-opus`; contract
[`PROJECT_AGENT_HIRING_IMPL.md`](../PROJECT_AGENT_HIRING_IMPL.md); ledger 128
and 129. **Not installed.** This plan is to be followed after the corrections
for [Astra's review](2026-09-14-project-hiring-review.md) are themselves
reviewed.

## What changes at install

The first launch of the new build enforces project-scoped hiring immediately:

- A lead's hire in a project session seats only agents associated with that
  project, in their primary role.
- The lead picker for a new project session lists only that project's agents.

There is no grace period and no inferred membership. A grace period would be
the old fallback again: silently seating whichever same-role agent exists.
Executions already running are not interrupted, and resuming an existing
execution is unaffected.

Setup-created teams are associated automatically from their setup journals.
That covers Tank Loop: a read-only simulation on 2026-09-14 matched Loom,
Builder, Verifier, Runner, Designer and Project Setup. Every other agent stays
unassociated until an owner or collaborator associates it explicitly.

## Inventory (read-only, 2026-09-14, not membership)

Readable project heads on hive, from `bee events query --kinds 30621`:

| Project | Coordinate owner | Access | Brian's roster role |
| --- | --- | --- | --- |
| Beekeeper (`bee-keeper`) | `6cbdf445…` (Andy) | public | owner |
| General | `6cbdf445…` | public | not checked |
| Tank Loop (`tank-loop`) | `6cbdf445…` | public | owner |
| hallway | `3d3b7169…` (Brian) | private | owner |
| Test Pub | `11868153…` | public | not checked |

Seats this computer's agents held in Beekeeper-project executions, read from
provider-signed kind:44223 since 2026-08-01 (3291 events; 2520 name
`bee-keeper`):

| Agent | Pubkey | Primary role | Seat role | Sessions | Last seen (UTC) |
| --- | --- | --- | --- | --- | --- |
| Keystone | `ede63017` | lead | lead | 20 | 2026-09-15 |
| Keystone 2 | `9ef00c39` | lead | lead | 5 | 2026-09-15 |
| Bob | `1ddd35c6` | builder | builder | 9 | 2026-09-15 |
| Ira | `22f3b985` | verifier | verifier | 2 | 2026-09-15 |
| Gordan | `6759f39a` | runner | runner | 2 | 2026-09-15 |
| Parallax | `3b6f55c2` | verifier (codex) | verifier | 1 | 2026-08-31 |
| Banksy | `efefd4e5` | designer | designer | 2 | 2026-08-29 |
| Texas | `9796af42` | poker | poker | 1 | 2026-08-29 |
| Levain | `59285ede` | architect | architect | 1 | 2026-08-28 |

Four more seat pubkeys (three builders and a lead) are not managed on this
computer and cannot be associated here.

Two further facts from the same read:

- **Tank Loop:** Bob, Ira and Gordan also held seats there. Those were the
  borrowed hires that caused ledger 128.
- **hallway:** no agent seats; its sessions were unseated.

None of the nine agents is builtin, all have a primary role and none has an
association today. These are **candidates** for an explicit decision. An agent
belongs to one project, so associating Bob with Beekeeper keeps him out of
Tank Loop. Tank Loop has its own Builder.

## Rollout steps

1. **Review first.** Land the branch only after the correction commits are
   reviewed. Install with `scripts/app-from.sh <sha>` when no Beekeeper-project
   lead is about to hire: a hire answered after install and before step 3 is
   refused, not misrouted. Loom's running Tank Loop session keeps its seats.
2. **Confirm Tank Loop.** On Tank Loop's Agents tab, Loom, Builder, Runner,
   Verifier, Designer and Project Setup appear under Project agents. Bob,
   Gordan and Ira appear under Borrowed or Previously here, with their history
   attributed to them.
3. **Associate Beekeeper's agents explicitly.** On Beekeeper's Agents tab, the
   hiring-readiness notice names each role with past work but no associated
   agent, and who did it. For each role, choose an agent and use "Associate
   with Beekeeper". The candidates above are the likely choices, but nothing
   is preselected.
   - The native command verifies from the signed head and roster that the
     signer is the creator, an owner or a collaborator, and refuses otherwise.
   - Andy's agents (the pubkeys not on this computer) are associated from his
     computer.
4. **Check one hire.** Start or resume a Beekeeper lead and let it hire one
   role. Confirm the seated identity is the associated agent, then check the
   Agents tab row's staged role revision, runtime/model and host.
5. **Remedy for a refused hire.** A hire that arrives before step 3 answers
   `HIRE_NO_PROJECT_AGENT`. Its remedy points at the project's Agents tab,
   and the lead can hire again once the agent is associated.

## Owed after install

These live checks were not performed. They need the installed build:

- the Tank Loop backfill result;
- a Beekeeper association refused for a non-owner, and admitted for Brian;
- a second computer carrying, not withdrawing, a published association.
