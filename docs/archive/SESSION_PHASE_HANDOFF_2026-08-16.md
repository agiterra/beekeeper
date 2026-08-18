# Sessions phase handoff — 2026-08-16

**For:** a fresh Fable session picking up the coding-sessions authority phase,
tasked with (a) resolving the branch/UX reconciliation mess and (b) finishing
the plan.
**From:** the Fable session that ran the 2026-08-15/16 work.
**Companion:** a Codex-authored report on the UX/transcript state accompanies
this document; read both. Where they disagree about *code facts*, verify
against source and trust neither.

---

## 1. Read these first, in order

| Document (all on `integration/glue`) | Role |
| --- | --- |
| `docs/SESSION_VISION.md` | **Product authority.** The 2028 horizon, ten product invariants. Unchanged. |
| `docs/SESSION_NEXT_PHASE_BRIEF.md` | Input brief. Note its epistemic markers — this project uses them deliberately. |
| `docs/SESSION_DESIGN_PHASE_PLAN.md` (v3) | **Design authority.** D1–D6, gates G1–G7. |
| `docs/SESSION_EXECUTION_PLAN.md` | **Execution contract.** Bites, proof/stop-condition rules, seams, and the rulings log **§B.7a (R1–R24)** — decisions already made, with reasons. |
| This document | Current state, the mess, and what's next. |

**Epistemic rule that governs this effort:** a completion report is not
evidence. Twice on 2026-08-15 a plan claim was wrong and source-checking
caught it (R15's implicit adoption, R20's projected legacy authority). Verify
with `file:line` before relying on anything, including this handoff.

---

## 2. Where the plan actually stands

### 2.1 Shipped, verified, and live

- **Genesis** (kind 44226) — sessions have a cryptographic founder. Relay
  enforces uniqueness transactionally (advisory lock), refuses rival claims,
  and requires **explicit adoption** (`adopts` naming the founding create +
  its receipt) for pre-genesis sessions. Resolve-by-event-id only (R7/R13).
- **Goal** (kind 44227) — append-only revisions, founder-gated editing.
- **Preflight unification** — the fail-open holes are closed (N=1 composer,
  stop/resume/interrupt), legacy sessions render "ungoverned — adopt to
  govern."
- **Authority chain** (kind 44228) — append-only transitions
  (`genesisRef`/`prevAccepted`/`seq`/`type`/`granteePubkey`), one type
  (`grant-operator`), relay-validated linkage under the same advisory-lock
  pattern, **relay-signed acceptance receipt** (kind 40099) as canonical
  head. Owner resolution is one seam function returning the genesis signer.
- **Coordinate facts** — five separate facts on kind 44223: `observedCommit`,
  `dirty`, `repoRef`, `relayReachable` (tri-state; `None` = *not checked*),
  `verifiedAt`. Reachability checked provider-side, repo-root NIP-98 signing,
  every ambiguous outcome degrades to not-checked.
- **Transcript UI** — substantially further along than the plan documents:
  17 item kinds, inline plans, paired tool calls, client-derived diffs, three
  rails. R24 honored in code *and* in copy.

**Deployed and proven on the shared relay** (`lightyear.agiterra.org`): a
real two-provider session (Claude + Codex under one `sessionRef`) with a
genesis and a goal, verified by querying 16 signed events back off the relay
— 1 genesis, 1 goal revision, 2 lifecycle commands, 2 turn commands, 8
generation metadata, 2 receipts. The relay refuses rival geneses live
(observed: `duplicate: coding-session already founded by event 199051b2…`).

### 2.2 The critical gap — read this before writing code

**The last two bites shipped their storage halves and dropped their consumer
halves.** The authority chain is complete at the write path and **consumed by
nothing**: the provider still authorizes on the founder pubkey alone
(`crates/buzz-session-provider/src/commands.rs` ~406-411), the desktop has
only the kind constant, the SDK builder has zero callers, no renderer knows
the 40099 receipt shape. Likewise B1's facts land on 44223 with **zero
desktop rendering** — the decided "one push from durable" surface doesn't
exist.

This happened because the implementing prompts scoped the UI halves out (to
keep bites small and avoid colliding with a parallel desktop session). The
consequence is the failure mode §1 of the execution plan exists to prevent:
substrate built, never exercised, with A5/A6's own stop-conditions (head
freshness; head-reference staleness → rejection storms) untested underneath
new work.

**Ordering corrective, binding on the next bite:** no new chain features
until one real grant flows **genesis → transition → acceptance → provider
decision → timeline row**, end to end, on the deployed relay.

### 2.3 Never ran: P1

P1 — the plan's self-declared highest-leverage bet (does replaying 44225
history into a fresh execution produce real continuation?) — has **not run**.
No spike, no report. It stalled structurally: R2 makes it need Brian's
machine, logins, and judgment, and the plan buried that human dependency in a
parallel track while pure-code tracks ran ahead. History seeding remains
absent; cross-machine continuation still starts empty.

Its answer reshapes the B-track: if seeding works, fork and machine-death
continuation are the phase's crown; if it yields pasted-summary quality, they
shrink to record-only — and the *value* of the whole invite ladder drops,
because an invited teammate can do little with a session whose machine died.

---

## 3. The mess: branches and worktrees

### 3.1 Established facts (verified, not inferred)

- **The "rival rails" are not rival.** `feature/coding-sessions` and
  `feature/coding-sessions-ui` carry the execution rail and changes rail with
  **byte-identical file content** (SHA-256 compared:
  `CodingSessionExecutionRail.tsx`, `useCodingSessionRailWidth.ts`,
  `CodingSessionChangesRail.tsx`). `git cherry` flags them as unique only
  because patch-ids differ from context lines on different bases. **No work
  needs discarding, no adjudication needed.**
- **The two rails are complementary features, not competitors.** Execution
  rail = *who is working* (tabbed, from the umbrella model / signed events;
  its empty state honors R24 in copy). Changes rail = *what changed* (from
  the client-side transcript model, reusing `FileEditDiffBlock`).
- **`feature/coding-sessions-ui` is based on the assembly, not the feature
  branch.** It contains the integrated build (`849d57eb`), glue commits, CI
  files, and merge commits. Under the fork's branch model it is
  **unmergeable** into an upstream-clean feature branch.
- **`feature/coding-sessions` is a superset** of all coding-session UI work,
  and additionally carries four newer fixes the UI branch lacks (guided
  runtime login, connect spec via channel menu, workdir placeholder hint,
  channel-membership read authority). Every file the UI branch has that
  `feature/coding-sessions` lacks belongs to *other features* (builtin-shell,
  project routes, observe routes) inherited from its assembly base.

**Recommendation:** retire `feature/coding-sessions-ui` and its worktree
(`/Users/brian/Projects/buzz-session-ui`); nothing is lost. Point transcript
UI work at `feature/coding-sessions`. **Not yet done** — awaiting Brian.

### 3.2 Current branch/worktree map

| Worktree | Branch | Tip | State |
| --- | --- | --- | --- |
| `/Users/brian/Projects/buzz` | `integrated-build` | `46bb137b` | The assembly; `= origin/integrated`. |
| `/Users/brian/Projects/buzz-coding-sessions` | `feature/coding-sessions` | `931f3390` | **Canonical feature branch.** Actively committed to by another session. |
| `/Users/brian/Projects/buzz-session-authority` | `feature/session-authority` | `d6e74776` | Two clean commits (A4 `95fb1474`, B1 `d6e74776`) on a `feature/coding-sessions` base. **Fold into `feature/coding-sessions` and retire** — A5, their consumer, must live there. |
| `/Users/brian/Projects/buzz-session-ui` | `feature/coding-sessions-ui` | `fb2db9a1` | Assembly-based, superseded. **Retire.** |
| `/Users/brian/Projects/buzz-integration-glue` | `integration/glue` | — | Docs and glue. |

### 3.3 Remote state

- `origin` (relay, `lightyear.agiterra.org`) has `integrated = 46bb137b`,
  build tags through `build/2026-08-16.3`.
- **GitHub (`upstream`, `agiterra/buzz`) is stale and divergent** —
  `integrated = f38dcb81`. `scripts/integrate.sh` pushes only to `origin`;
  GitHub was mirrored manually once. Mirror it when Andy needs current state.

### 3.4 Ceremony and deploy gotchas (hard-won, 2026-08-15)

- Run the ceremony as
  `LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse upstream/main) scripts/integrate.sh`
  — there is no local `origin/main`, and hooks otherwise dirty the tree
  mid-run. Park worktrees on detached HEAD first; restore after.
- The rerere cache was rebuilt on 2026-08-15 (it was lost in a machine
  migration); the eleven recurring cross-feature resolutions replay
  automatically again.
- **The relay auto-deploys** from a green `integrated` Woodpecker pipeline
  (`docs/INTEGRATION.md` § Deploying — `buzz-autodeploy.timer` on the agincus
  host, 5-minute poll, `pg_dump` backup, image flip, health-wait,
  auto-rollback). It has a **silent failure mode**: on 2026-08-15 the
  `integrated` webhook returned HTTP 500 and no pipeline was created, so the
  relay stayed stale with a green badge and nothing anywhere said so. A
  GitHub webhook redelivery fixed it. Recommend to Andy: have the deployer
  compare deployed SHA against the mirror tip so a dropped hook degrades to
  "late" rather than "never."

---

## 4. What to do next (ranked)

1. **Reconcile branches** (§3.1): fold `feature/session-authority` into
   `feature/coding-sessions`; retire it and `feature/coding-sessions-ui`
   plus their worktrees. Cheap now, compounding later.
2. **A5, expanded to include the dropped front halves.** Provider consumes
   the chain (founder ∪ granted operators; stop/resume stay owner-only),
   **plus** acceptance-receipt timeline rows, head display, and the
   coordinate-facts surface. Its Proves line is the **end-to-end grant flow
   on the deployed relay**, not a unit test. Honor its stop-condition
   literally: if chain-state propagation to the provider is slow or fragile,
   halt and report — that finding changes A6's design.
3. **P1, as a calendar item with Brian.** Pre-build the harness (query 44225
   by `h` + `cs-target`, fold by `eventSeq`, build a provenance-preserving
   package, feed to fresh Claude and Codex executions via initial prompt and
   `session/load`); Brian selects a real session, authorizes two bounded
   runs, and judges. One hour of his time; reshapes the whole B-track.
4. **A6 strictly after A5** — command binding (`sessionRef` + accepted-head
   id on 44220/44221) + relay-side rejection. Remains the **collaboration
   release gate**: no invite UI ships before it.
5. **Schedule the R24 subagent vocabulary.** It is currently homeless
   ("adjacent to the A/B tracks" is not a schedule), and the execution rail
   already ships an empty state promising it. Provider-side protocol work;
   natural companion to P1's harness since both live in the transcript
   publishing path.
6. **Defer on purpose:** more transition types (revoke/transfer/takeover)
   until A5/A6 survive contact; A9 takeover entirely; A7's lease build
   pending the custody decision (§5); B3 pending both P2's test case and the
   reachability fix (§6).

---

## 5. Decisions: settled, and open

**Settled (recorded in the rulings log R1–R24).** Read them before
re-litigating. Highlights: build against `feature/coding-sessions` alone
(R1); genesis deletion never releases a `sessionRef` (R6); `csg-session` is
enforcement/diagnostics only, never authority resolution (R7); adoption is an
explicit signed reference, never relay inference (R15); legacy authority is
per-execution and locally witnessed, never projected from relay history
(R20); **a provider derives authority only from facts it witnessed or events
it explicitly referenced (R21)**; a subagent's file-set boundary is never a
reason to alter a protocol contract (R15's standing rule).

**Open, on Brian's desk:**

- **Custody vs simultaneous co-input** (blocks A7's shape). Two independent
  analyses — this session's and a cold-read Fable assessment — converged on
  **custody**: ACP already serializes to one in-flight prompt, so
  simultaneity is an illusion the queue only papers over. The concrete
  proposal is *implicit-acquire custody*: taken by driving when nobody holds
  it, held while active, auto-released on idle, owner/admin can reclaim,
  handoffs visible in the transcript — with **contributing never gated** (any
  operator may post context without holding custody). This makes A7 simpler:
  **the lease defines who is eligible to drive; custody selects one of them
  at a time.** Awaiting Brian's confirmation.
- **Takeover contestability on return** (blocks A9). Recommendation: takeover
  is final; the returner is re-granted by a later transition.
- **R19 export identity**: `buzz sessions export` ships only 44223/44224/44225
  — no commands, no genesis, no transitions. Either expand it to a real audit
  bundle or rename it honestly as a provider-transcript export. Must resolve
  before phase close.

---

## 6. Known issues worth carrying

- **B3 reachability semantics.** `git_object_reachable` checks *advertised
  ref tips only*. Accurate at record time (the provider checks live `HEAD`),
  but B3 will re-check *old* coordinates at attach time, where a commit that
  has since become an ancestor of a moved tip would falsely read "not
  recoverable" — and the "one push from durable" hint would then be actively
  wrong. B3 needs an ancestry-aware check (fetch negotiation, or a
  30618-informed probe) before its copy makes recoverability claims; R4's
  "no new relay endpoint" ruling may need revisiting with evidence.
- **Changes rail is client-derived.** It renders inference over tool results,
  presented as "Session changes" — a claim about the workspace. A file
  changed outside a tracked tool call is invisible to it. B1's signed
  `observedCommit`/`dirty` facts are the honest fix; wiring them into this
  rail turns an inference into a fact.
- **Authority facts have no render path.** Invariant 5 ("the record is
  attributable") is currently unmet for authority itself: grants and
  acceptances would be invisible if they occurred. Folded into A5 above.
- **`api::mesh_demo::…round_trips_echo`** and two `buzz-pair-relay` timing
  tests are documented load/environment flakes, excluded in CI
  (`docs/INTEGRATION.md`). Do not chase them.
- **The Postgres gate is not part of `just ci`.** `just ci` is infra-free by
  contract; cite `just test-genesis` (currently 24 proofs: 15 genesis + 9
  authority-chain) as the uniqueness/authority evidence, never `just ci`.

---

## 7. Working rules (non-negotiable)

- Feature work on `feature/coding-sessions`; cross-feature coupling and docs
  on `integration/glue`. **Never** commit to `integrated`/`integrated-build`
  or `main`.
- **Never push.** Brian batches and runs the integration ceremony himself.
- Activate hermit (`. ./bin/activate-hermit`) before any git/build/test in a
  worktree. Commit with `git commit -s` (DCO).
- Per bite: `just ci` (state which legs ran — in feature worktrees it needs
  `CHECK_FILE_SIZES_BASE=$(git merge-base upstream/main HEAD)`), crate tests,
  `just test-genesis` when relay/db/authority code changes, and screenshots
  for any UI change (`pnpm build:e2e`, never a plain build; verify hash
  distinctness before presenting).
- Honor every bite's **stop-and-reassess** line literally. Halt and report
  rather than engineering around a disproven premise. Fail-closed behavior
  and signature checks are never deferred as "hardening."
- Decisions reserved to Brian (§5): take the design-plan default and flag it,
  except A7 custody and A9 takeover, which **ask first**.
