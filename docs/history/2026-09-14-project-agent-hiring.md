# Project agents and role selection — 2026-09-14

Branch `work/project-agent-hiring-opus` in worktree
`review-project-agent-hiring-opus`, rebased onto `main` `2cce920db`, including the
previously unlanded Agents-tab commit (`95a2f6f58`, now `b2ab6c73b`). The contract is
[`PROJECT_AGENT_HIRING_IMPL.md`](../PROJECT_AGENT_HIRING_IMPL.md); ledger item
128 carries the finding.

## Root cause (confirmed)

Tank Loop's lead hired Bob, Gordan and Ira because the founder's desktop
answered every `session.hire` with `chooseIdentity` in
`codingSessionHirePolicy.ts`. That function took every managed agent on the
computer with a matching home role, then sorted by pack, provider preference
and **name**. The installed records (read 2026-09-14 with secrets stripped)
show both groups had packs and no pinned runtime:

- Tank Loop's Builder, Runner and Verifier have `team_id f54498d1…`.
- Bob, Gordan and Ira have `team_id 8bec5451…`, created 2026-08-28.

So "Bob" < "Builder", "Gordan" < "Runner", "Ira" < "Verifier" decided each
hire. Nothing the host read recorded project membership. Staging then resolved
Tank Loop's kind:30624 pack by seat role, so the borrowed identities received
Tank Loop's instructions.

The lead had no discovery tool:

- Setup's lead first turn listed no agents.
- Team Start's first turn always said nobody else was on the computer.
- A hire cannot name an identity without a relay deploy: the hire key set is
  closed, and hive rejects unknown keys.

## What changed

- **Association.** `ManagedAgentRecord.project_ref` is recorded three ways:
  - by setup installation;
  - by an idempotent backfill from setup journals, which migrates Tank Loop;
  - by an explicit "Associate with <project>" action.

  Reinstall carries it forward and never moves an agent between projects. The
  agent's kind:30177 publishes `home_role` and `project_digest` (~~a digest,
  because 30177 is readable by every relay member~~ — the digest gives no
  confidentiality and is now published only for public projects; see
  [review](2026-09-14-project-hiring-review.md) finding 4 and ledger 129). Readers accept only the
  project's creator, owners and collaborators. The relay is unchanged: hive
  already carries 30177 for every agent, checked with `bee events query`.
- **Hiring.**
  - A project session seats only that project's agents, each in its primary
    role. A projectless session seats only agents of no project.
  - A missing agent is refused `HIRE_NO_PROJECT_AGENT`; there is no fallback.
    Name is no longer a tie-breaker.
  - Native staging re-checks new selections (project and primary role) and
    never borrows another project's installed pack. Resume is unchanged.
- **Discovery.**
  - `bee projects agents` defaults to the seat's project. ~~For a private
    project the seat cannot read, it lists the attesting owner's published
    agents, labelled unverified.~~ Removed: compact output dropped the label
    (review finding 2); the lead's first message now carries a private
    project's roster (ledger 129).
  - Setup's lead turn, Team Start's turn, the hire help and the shipped lead
    `hire` skill name the command.
- **UI.**
  - The project Agents tab groups project agents (Working, Idle, Disconnected,
    Available, On another computer), borrowed participants and history.
    Rename, Associate, and per-execution role revision, runtime/model and host
    are all on the tab.
  - The lead, bench and seat pickers use the same association. The seat role
    is locked to the primary role.
  - Setup ends with a roster and one next action, and is never complete while
    the lead cannot hire an installed agent.

## Commits

- `b2ab6c73b` Add the project Agents tab (previously unlanded as `95a2f6f58`).
- `d742c199f` Hire and select only a project's own agents.
- `639d68a0c` Let a seat discover a private project's agents from its attesting
  owner.
- A docs commit records this report, ledger item 128 and the map.

## Checks

Full gates on the tree first committed as `cae7eea02`, before the rebase onto `main` `2cce920db` (two commits, no conflicts; the pre-push floor re-ran on the rebased tree) (logs:
`../review-project-agent-hiring-e2e/gates2/`, untracked):

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check`, desktop fmt | pass |
| `cargo clippy --workspace --all-targets -D warnings` | pass |
| desktop Tauri clippy `--all-targets -D warnings` | pass |
| `cargo test -p buzz-core` | 1108 passed |
| `cargo test -p buzz-cli` | 1197 passed |
| `cargo test -p buzz-persona` | 194 passed |
| desktop Tauri tests | 3312 passed |
| file-size check, `pnpm check`, `pnpm typecheck` | pass |
| `pnpm test` | 9508 passed |

For `639d68a0c` (first `748afbf4d`): `cargo clippy -p buzz-cli --all-targets` passed and
`cargo test -p buzz-cli project_agents` gave 14 passed.

Browser run (fresh `pnpm build:e2e`, mock bridge, 27/27 passed):
`project-agent-hiring.spec.ts` (new, 8), `project-agents-tab`,
`project-roles-walkthrough`, `project-team-setup`, `role-packs-project`.

Adversarial review against the named constraints found two majors and six
minors before landing; all were fixed and tested:

- Reinstall dropped `project_ref`.
- A builder could be seated as verifier through the join dialog's free-text
  role.
- Unknown seat scope, reader parity and whitespace parity (minors).
- Unhireable agents in the lead roster, and a futile retry remedy (minors).
- ~~A second device erasing the digest (minor), which is disclosed rather than
  fixed.~~ Reclassified major by Astra's review and fixed (ledger 129).

## Live end-to-end run (real desktop host, provider and Claude seats)

The run used a disposable local relay on :3100 (database `buzz_pah_e2e`),
fresh keys, and two private projects `pah-p1` and `pah-p2` founded by a
disposable founder. This build of the desktop ran as instance
`io.agiterra.beekeeper.app.dev.pah-e2e` under an isolated HOME, with
file-based keys and no keychain. Seats ran `claude-primary`/`haiku`. Agents:

- **P1:** Lumen (lead), Zephyr (builder), Yarrow (verifier), and no runner.
- **P2:** Lark, Aardvark, Abacus (runner), Acorn.
- **Neither:** Aaron (builder), associated with no project.

The names are chosen so the old name sort picks wrong every time. Runbook and
raw evidence are in `../review-project-agent-hiring-e2e/` (`RUNBOOK.md`,
`evidence-live/`, `evidence/20260914T211620/`), untracked.

1. Founder discovery lists exactly P1: Zephyr, Lumen, Yarrow; and P2:
   Aardvark, Lark, Abacus, Acorn. Aaron appears in neither.
2. Founder hires of `lead` seated Lumen in P1 and Lark in P2, each with the
   role-seat grant accepted.
3. Lead-side discovery with the lead's own key and `$BUZZ_PULSE_PROJECT`
   first failed with `project "pah-p1" not found`: a private project hides its
   roster from the seat. The fix is `639d68a0c`; rerun, it lists P1's three
   agents as `attesting-owner`, and P2's four.
4. Lead hire `builder` seated Zephyr (`b17b52cb…`) in P1 and Aardvark in P2.
5. Bypass: a P1 lead hire of `runner` was refused `HIRE_NO_PROJECT_AGENT`
   ("1 other agent here has that role but does not belong to this project, and
   borrowing is not supported"). No runner seat exists in P1.
6. Bypass: a second P1 `builder` hire while Zephyr is seated was refused
   `HIRE_ROLE_BUSY` and named Zephyr's execution. Neither Aardvark nor Aaron
   was seated.
7. The assignment to Zephyr produced a kind:44244 report signed by Zephyr with
   summary `p1-builder`, P1's own pack marker. `E2E_PROOF.md` in the seat's
   worktree holds the same marker.
8. `70-verify.sh` passed 15 of 16. The failing check was "44223 packRef
   present", which is correct behaviour here: the rig's packs are local copies
   no repository vouches for, so native publishes `packRef: null` rather than
   inventing one. The staged revision is shown only for project-sourced packs.

## Not done or not proven

- **Tank Loop and Loom were not touched.** There were no installed-app
  checks, and neither Brian's live dev app nor the installed bundle was
  rebuilt. After install, confirm:
  - backfill associates Loom, Builder, Runner, Verifier, Designer and Project
    Setup;
  - Bob, Gordan and Ira show as borrowed or previous;
  - Loom's next builder hire seats Tank Loop's Builder.
- **Not exercised live:** rename (B) and reinstall (E) are covered by tests
  only, and the projectless Solo path by browser specs only (G). The run's
  founder executions were unseated project sessions.
- **Borrowing** is refused, not implemented. **Naming an agent in a hire**
  needs a relay change.
- **Screenshots** are in `../review-project-agent-hiring-e2e/screenshots/`
  (11, hash-distinct), untracked, from the mock-bridge browser run.

## Compatibility

- **Existing installs** (Tank Loop) are associated by backfill on first
  workspace apply, as long as the journal role matches the agent's home role.
  The rule was simulated read-only (Python, secrets skipped) against Tank
  Loop's real `publication.json` installation and `managed-agents.json`: it
  would associate Loom, Builder, Verifier, Runner, Designer and Project Setup,
  and none of Bob, Gordan or Ira. The native code has not run on that data.
- **Agents with no association** (Bob, Gordan, Ira, Keystone and the rest)
  stay unassociated. They remain hireable in projectless sessions, and in a
  project only after an explicit Associate.
- **Beekeeper's own project sessions** whose umbrella names a project will be
  refused `HIRE_NO_PROJECT_AGENT` until agents are associated with that
  project.
- **One-time republish:** every agent with a home role republishes its
  kind:30177 once, because `home_role` is now in the content.
- **Solo sessions** are unchanged.

## Review corrections (later on 2026-09-14)

Astra's [review](2026-09-14-project-hiring-review.md) of `42929e209` found four
gaps (ledger 129). All four are closed on the branch in `2379c7cf5`, with focused regression
evidence for each. No broad marathon was run. Logs are in
`../review-project-agent-hiring-e2e/gates3/`, untracked.

| Finding | Correction | Regression evidence |
| --- | --- | --- |
| 1. Association authority only in React | `project_association_authority.rs`. The native command reads the signed head (newest 30621 by the coordinate owner, exact `d`, verified) and the newest relay-signed 39010, or the head's bootstrap `p` tags when none exists. It admits the creator, owners and collaborators, and fails closed when state is unreadable or unverified. Network reads run outside the store lock, with an identity re-check under it. | 16 `project_association_authority` tests: creator, owner, collaborator; viewer and non-member refused; missing head; wrong-author and wrong-`d` heads; a 39010 not signed by the relay ignored; a 39010 overriding head `p` tags; newest 39010; tampered signatures; unknown relay signer; duplicate viewer row; private tag |
| 2. Fallback rows lost "unverified" in compact | Attesting-owner fallback removed. Rows carry `verified` in JSON and compact, and unverified claims are never printed. The desktop marks non-creator claims "Project authority not verified" after a roster read failure and never counts them. | `every_format_carries_verified_and_compact_drops_only_the_owner_role`, `without_a_roster_no_claim_is_accepted_not_even_the_attesting_owners`, `the_unreadable_project_message_says_why_and_where_to_look`; desktop published-agents authority tests |
| 3. A stale host could withdraw the association | `project_association_carry.rs`. An inbound same-owner digest is carried. Before a digest-less 30177 publishes, the relay head is read: a digest there is carried and republished; a failed read withholds the publish. Only a record that knows its project is private withdraws. | Projection, inbound and flush decision tables; four flush tests against a stub relay (carried instead of withdrawn, unreadable head withholds, known-private withdraws, no digest publishes); inbound `carry_tests.rs`; reconcile-once-then-no-op |
| 4. The digest gave no private-project secrecy | Docs corrected. A digest is published only for heads verified public, with a verifier after event sync and installation. Private projects publish nothing. The CLI prints `[]` or not-found. The Agents tab says so. Setup's and Team Start's lead messages carry the local roster. | `agent_events` tests (a private project publishes no digest, coordinate, owner or slug, even with a carried digest); `a_head_is_private_only_when_it_says_so`; `lead_first_message_lists_this_projects_agents_on_the_hosting_computer`; desktop private-project view test |

Focused gates on the corrected tree:

| Gate | Result |
| --- | --- |
| Tauri fmt, clippy `--all-targets` | pass |
| buzz-cli, buzz-core, buzz-persona fmt and clippy | pass |
| `cargo test --lib` for the desktop Tauri crate | 3341 passed |
| `cargo test -p buzz-cli project_agents` | 16 passed |
| `pack_rules` | pass |
| `pnpm typecheck`, `pnpm check` | pass |
| Focused desktop tests: agents, project-agents, roles, lead first turn, hire, association | 2178 passed |
| File-size and current-state checks | pass |
| Fresh `pnpm build:e2e`, five affected Playwright specs | 27 passed |

The Beekeeper rollout inventory and steps are in
[the rollout plan](2026-09-14-project-agent-rollout.md). Not live-checked: the
native authority command against hive, and carry-forward between two real
computers. Both need the installed build.
