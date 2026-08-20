# Sessions — living state

**The one document to read first.** Everything else is either a binding
authority (§4), a protocol for a specific experiment, or history. This file
is updated at every ceremony and whenever live use produces a finding; if it
disagrees with an older document about *current state*, this one wins.

_Last updated: 2026-08-20, correcting §1 (the deployed build is
`build/2026-08-19.4`, not `build/2026-08-18.7`) after probing lightyear from
outside._

---

## 1. What is live

| | |
| --- | --- |
| Deployed | `build/2026-08-19.4` (`246dfa1b`) on lightyear — auto-deployed by `buzz-autodeploy.timer` after CI #91 went green 2026-08-20T03:55Z; relay verified from outside 2026-08-20 by a member-key probe: a kind-44240 write was rejected `restricted: unknown project coordinate`, which is the new build's discriminator (the old build says `unknown event kind`) |
| Assembly | Andy rebuilt on top of our work; CI gate is now ~7 min (was ~40) and two of three documented flakes have real fixes |
| Unshipped locally | the `just dev` nokeyring fix, this ledger's newest entries, and three verified-missing session-stability fixes (§3) |
| Built, awaiting acceptance | **Project Pulse Slice 1** — five signed commits on `wip/project-pulse` (`aced60f2`…`d235189a`, 2026-08-19): kind 44240 end to end (core contract, relay ACL on every read surface, `buzz pulse` CLI, ACP digest injection, Desktop screen behind the `project-pulse` preview flag). Gated green (live e2e 11/11, desktop 5832/5832, conformance 42/42, clippy/fmt clean). Blocked on Brian's §5.8 manual acceptance (`docs/PULSE_SLICE1_ACCEPTANCE_RUNBOOK.md`); split ceremony pre-computed in `docs/PULSE_SLICE1_SPLIT_MAP.md`. Plan: `docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md`. Next build queued: `docs/REHYDRATION_HARDENING_IMPLEMENTATION_PLAN_2026-08-19.md` (verified; zero file overlap with Pulse). **Shipped 2026-08-19 night as `build/2026-08-19.3`** — split onto `feature/project-pulse` + `integration/glue` and pushed to both remotes. That build shipped **without** the UX-fix pass, which was still uncommitted in `/Users/brian/Projects/buzz-uxfix` when the window closed. **The UX pass then shipped the same night as `build/2026-08-19.4`** — all 15 critique findings plus the error-card fix, folded as per-file diffs onto `feature/project-pulse` (`ebf5085c`, `877723fc`) and `integration/glue` (`db8614cc`) per the split map's EXECUTED banner, changed-line multisets verified identical (2,689 pulse-owned + 20 glue-owned lines) and `git diff wip/pulse-ux-fixes integrated-build` clean of every product hunk. Gate cited: desktop 5845/5845, fold conformance 42/42, `tsc --noEmit`, px-text guard; the 62 e2e-smoke failures were reproduced at `1ac2ac51` in a throwaway worktree and are therefore inherited, not caused by this delta — CI re-gates on push. §5.8 manual acceptance is **still owed**, and those 62 inherited smoke failures are still unexplained (§3) |

Shipped in this arc: durable names (R26), closure/stop separation (R27),
verified rehydration + systemPrompt-first bootstrap, continuity disclosure
end to end, reconnect that follows its generation, visible turn refusals,
foreign-member authority resolution, project-claim inheritance on join,
operator attribution.

Proven live, not merely tested: the full authority ladder (observe → refuse →
grant → grantee turn → stop refused, 2026-08-18 local); cross-provider
rehydration on the deployed relay; operator attribution rendering correctly
across two machines simultaneously.

## 2. Open — ordered, each with its evidence

1. **A duplicated create silently kills rehydration.** Observed end to end in
   Session_Test_8_18_8PM: one click produced two Codex executions 8 ms apart
   (`ef518a28`, `7c4ed948`, one command id). The projector then refused —
   "context fact conflict: command … has more than one" — so both executions
   started `Fresh` with no context tooling, and the joined agent correctly
   told the operator it could not see the prior work. The refusal is right;
   the duplicate is the bug. Its cause (one provider instance per state
   directory) shipped in this build — **re-test before assuming it is
   closed.** The disclosure remains useless: the operator sees
   `session_fresh` while the reason lives only in the provider log. Surface
   the bail-out reason.

2. **"What was attempted" is unstructured for some adapters.** A turn records
   the downstream richly — full tool output, `exit_code`, `isError`, duration,
   tokens — but the upstream is adapter-dependent: `claude-agent-acp`
   populates `tool.input` verbatim; `codex-acp` leaves it `{}` and puts the
   whole command in `tool.toolName` as prose. Nothing supports "has this exact
   thing already failed", which is why failed attempts repeat. `toolName` also
   carries host paths into signed events.

3. **Rehydration is create-only.** ~~A reattached execution gets no context
   MCP.~~ **Fixed in code 2026-08-18 by `b9de9a6d`** — verified 2026-08-19:
   `session.rs:646/:667` now pass `mcp_servers.clone()` into
   resume/load, and `lib.rs:1081-1098` builds a real `rehydration_mcp` on
   `resume_session`. The 2026-08-18 live observations (resumed Codex lost
   its package; Claude gen 2 "did not see that tool available") predate the
   fix. Still owed: a live re-test of resume-with-context, and confirmation
   the fix is in the deployed build (it postdates `build/2026-08-18.7`).
4. **The package is a start-time snapshot.** Provenance says
   `complete: true` meaning "complete when projected", which reads as
   "current". A joined execution never learns what a sibling did afterward
   (observed: Codex reported 16/16 complete while Claude had advanced to 6
   turns). Fix: time-bound the provenance, then add refresh.
5. **`session_history` page cap is 20** (`session_context.rs:23-24`). Reading
   101 items took six calls plus a hard `limit must be between 1 and 20`
   error. Untenable as sessions grow.
6. **Provenance has an unexplained delta.** `sourceEventCount` counts the
   proof graph (`1 + authority_links×2 + names + goals + per generation
   (3 + transcript)`, `context_projector.rs:945-955`), not content, so it
   exceeds `totalHistoryItems`. Nothing is missing, but the package never
   says so and a careful agent had to flag it as unexplained.
7. **An execution whose provider identity is gone is a dead row, silently.**
   The provider's `SessionRecord` and opaque resume cursor live in one
   app-instance state directory keyed by provider pubkey. Lose that identity
   — reinstall, new machine, keychain loss, or in dev a branch-derived slug
   change (`scripts/instance-env.sh`) — and no provider claims those
   executions: Reconnect goes unanswered (`Ignored::UnknownTarget` publishes
   no receipt, by design) and Stop is silently ignored, so the row is stuck
   `disconnected` forever and the session is unendable (R27's tail).
   Observed 2026-08-18 by switching the assembly worktree's branch.
   The durable session is unharmed — transcript, goal, roster and history
   all survive, and Add provider still rehydrates a new execution from
   them — but the UI never says so. It has what it needs to: compare the
   create's `providerAuthorityPubkey` with the local provider's pubkey and
   say "this execution belongs to a provider this computer no longer has;
   start a new one and its history comes with it." Wants a founder-signed
   way to retire an orphaned execution, too.

8. **Desktop grant-awareness (A8-shaped).** Granted operators still see a
   gated composer; every new session needs its members added by CLI, because
   each create stamps a fresh transport channel. The observation half needs
   no new protocol and is not blocked by A6.
9. **Stale clients are invisible.** An old client shows "ungoverned — adopt
   to govern", falls open, and silently swallows refusals. There is no
   version signal in the UI.
10. **P1 has never run.** Harness (`scripts/p1-seed-spike/`), judging protocol
   (`P1_JUDGE_SCRIPT.md`), and matrix (`P1_CONTINUITY_MATRIX.md`) are ready.
   Gates the whole seed/checkpoint track.
11. Smaller: stale-Reconnect echo port (`a92fd728` absent from this line);
   `rejectedAuthorCount` surfaced nowhere; `Loaded` vs `Resumed` conflated at
   the receipt layer.
### Found 2026-08-19 during the Project Pulse Slice 1 build

18. **Rust and Desktop disagree on malformed-44223 admissibility** (found by
   the Pulse fold-parity review). The CLI fold decodes with strict
   `decode_coding_session_metadata` (exact key set); Desktop uses the
   permissive `parseBuzzCodingSessionMetadata`, so a 44223 carrying an
   unknown key is dropped by one and kept by the other. Documented in
   `pulseFold.ts`'s module doc with an instruction not to bank a conformance
   vector on it. Needs a product call: pick one decoder as the contract.
19. **The ACP-injected Pulse digest carries entries only, no session facts**
   (deliberate Slice 1 deviation, disclosed in the injection via
   `SESSIONS_OMITTED_LINE`, `pulse_fetch.rs:66`). An overlap visible only in
   session facts will not reach the agent until the session fold is built
   into `buzz-acp`. Not lying, but not done.

### Found 2026-08-20 in the first live use of Project Pulse

Items 21–23 are fixed in `build/2026-08-20.2`. Item 20 is **open** and needs
Brian's product call.

20. **A live session ages out of Active work.** `publish_metadata` fires only
   on transitions (`crates/buzz-session-provider/src/lib.rs:905` create,
   `:1170` idle, `:1194` stopped, `:440` disconnected); there is no
   heartbeat, so an idle-but-alive session emits nothing and after
   `PULSE_ACTIVE_WINDOW_SECONDS` (1800,
   `desktop/src/features/project-pulse/lib/pulseFold.ts:67`) it falls to Last
   seen. Observed live 2026-08-20: PulseTestV2 was alive while Pulse showed
   "Idle · last observed 30m ago" and Active work was empty. Widening the
   window re-admits the ghosts the gate was built to stop; the fix is a
   provider heartbeat, so silence honestly means absence. Needs a product
   call on cadence vs permanent event volume. *Re-verified at the
   2026-08-20.2 ceremony, and the mechanism is stronger than "only on
   transitions":* `publish_metadata` itself (`lib.rs:1786`) returns early
   when the serialized content equals the last publication (`:1794-1799`), so
   even the periodic git-probe republish (`:2068`) emits nothing when nothing
   changed. The silence is structural, not incidental.
21. **Entries were buried below the session groups**, so the screen led with
   what Pulse *observed* rather than what people *claimed*. Fixed: entries +
   superseded now render above the session groups, with a counts line and an
   entries empty-state carrying the same three-way honesty split the sessions
   empty-state already had.
22. **Repeated executions of one session rendered as repeated full cards.**
   Fixed **at presentation level only**: `groupPulseSessionExecutions`
   (`desktop/src/features/project-pulse/lib/pulseFormat.ts`) keys on
   `session.sessionRef ?? session.targetKey` and collapses them into one card
   with a `> N executions` disclosure. The digest shape, `pulseFold.ts`
   identity and the 42 conformance vectors are untouched and re-verified
   green — what the fold calls one session did not change, only how many
   cards that draws.
23. **Card noise, including a redundant Closed/Ended pair.** Fixed: the
   commit-confirmation qualifier moved inline into the chip row (~19
   dedicated lines gone), the "observed Nm ago" chip renders only on active
   sessions (non-active headers carry "… · last observed Nh ago"), and the
   "Closed" chip is suppressed only when the status label already reads
   "Ended". The honest tri-state survives; only the duplication went.

The fix for 21 introduced, and the verify pass caught, one honesty defect
before it shipped: the new counts line asserted an *absence* over a read that
admits it lost data — a partial read printed a bare "no entries no sessions",
a claim about the project made from a read that returned no claims.
`ProjectPulseView.tsx` now derives `countsAreLowerBounds` and states floors;
at zero the floor reads "no entries in what this read returned", a fact about
the read rather than about the project.

### Recovered 2026-08-18 from superseded handoffs (verified still true)

These were tracked in documents that had been superseded and had fallen off
every live list. Each was re-verified against current source before landing
here.

12. **The agent inherits the operator's personal MCP servers.** The env fence
   scrubs the `BUZZ_*` namespace plus a few enumerated third-party secrets
   (`agent_fence.rs:52-60`) and says nothing about MCP configuration, so an
   adapter can reach an operator's personal Gmail/Calendar/browser servers.
   Env scrubbing cannot fix it. **This gates the collaboration release**
   (A6/A8): sharing a session must not share the founder's personal
   integrations. Originally SESSION_HANDOFF_SOL §7.3.
13. **Permission requests are auto-approved in the transport.**
    `session/request_permission` is answered `allow_once` inside the ACP read
    loop (`acp.rs:1445`, dispatch `:1520`). No operator ever sees a prompt.
    This is the root blocker for any approval story, and approvals appear
    nowhere in D1–D6. Prior art to reuse when it is picked up:
    a neutral runtime-mode vocabulary (`approval-required` /
    `auto-accept-edits` / `auto` / `full-access`) mapped per provider
    (SESSION_HANDOFF_SOL §6), and the architectural recommendation that
    session permission requests get their own signed kinds rather than
    reusing workflow kind 46010, while reusing Buzz's existing
    pending-decision / grant-deny / `needs_action` / push mechanics
    (SESSION_NATIVE_SUBSTRATE §1).
14. **A crashing adapter can never be classified auth-required.**
    `mentions_auth` inspects only `AgentError` message text
    (`session.rs:774`), so an adapter that exits *because it is not logged
    in* surfaces as generic `PROVIDER_UNAVAILABLE` and the guided-login
    affordance never appears. Andy hit exactly this. Compounded by **adapter
    stderr being inherited, not captured** (`acp.rs:649`), so the one
    diagnostic naming the cause never reaches a log a user can send.
15. **Execution labels collide.** Every execution row is labelled by the
    provider pubkey, so same-provider executions render identically. Assigned
    to UI Slice 2 but absent from the UI convergence handoff — it fell
    between two documents.
16. **Handoff carries two accepted v1 risks** (SESSION_STEP4_DESIGN): the
    quoted block is prefilled verbatim into another execution's
    operator-signed prompt (cross-agent prompt injection), and the
    "Handoff from X" chip is recognized from prose, not verified (label
    spoofing). Must be revisited before any autonomous/unmediated handoff —
    a named prerequisite of the persistent-agents step in D6.
17. **Managed-agent ↔ session convergence, concrete order** (SESSION_PATH):
    managed agents *into* sessions first via lane mention (no new wire
    concepts), then session executions *into* managed agents via `agentRef`
    (needs delegation grammar). D6 references the topic only abstractly.

## 2a. Direction settled 2026-08-18

Three independent answers to "what should a new execution get on its first
turn" — two blind design reviews and Sol's implementation — converged on: push
a small brief the agent would not know to ask for, pull the rest, never inject
raw transcripts. Sol's shipped answer goes further and emits only what the
record already proves (turn outcomes, tool attempt outcomes, cited event ids)
rather than a model-written summary, because a generated summary manufactures
a new unverified claim. That is the shipped direction. Distillation of older
material is still worth building, but it must cite those evidence ids rather
than replace them, and it is not started.

## 3. Next — one track at a time, in this order

1. **Re-test the duplicate create** (§2 item 1) on this build. The
   single-provider-instance lock shipped in `build/2026-08-18.12`; nobody has
   yet created a session on it and confirmed one click makes one execution.
   Until that is done, item 1 is suspected-fixed, not fixed.
2. **Rehydration hardening** — §2 items 4–6 as a single bite (item 3's code
   half shipped in `b9de9a6d`; re-verify live rather than striking it).
   Every one came from live use, and together they make the multi-execution
   story honest instead of subtly misleading.
3. **Verify Andy's People/roles flow** with a second identity
   (viewer → operator → revoke). It plausibly closes §2 items 7–8; do not
   strike them on commit messages alone.
4. **P1**, whenever an hour exists. It gates the entire seed/checkpoint
   track; everything downstream in the research report §7 is speculation
   until it runs.


The four rehydration defects (§2 items 3–6) are one subsystem and one
coherent bite. Then 7–8 to make multi-member testing self-service. P1 whenever an hour
exists. Longer horizon lives in the research report §7: relay-durable
checkpoints (kind 44231) and encrypted native-snapshot sync (44232).

## 3a. Environment facts that cost real time (do not rediscover)

- **Provider identity is per app-instance, and the instance slug comes from
  the git branch** (`scripts/instance-env.sh`). Switching the worktree's
  branch gives the app a different app-data dir, a different provider
  identity, and a different session set. Sessions created under one branch
  cannot be resumed or stopped from another (§2 item 7).
- **Provider private keys live in the OS keychain**; the provider record
  (`<app-data>/session-provider/coding-session-provider.json`) holds only
  `providerPubkey`. So `BUZZ_DESKTOP_NOKEYRING=1` on an instance whose
  identities were created in keyring mode fails with "no key in JSON or
  keyring" — the mode switch needs a migration that does not exist yet.
- **`just dev` and `just desktop-standalone` differ**: only the latter
  honored `BUZZ_DESKTOP_NOKEYRING` until 2026-08-18. `just dev` also starts
  local Postgres/Redis/relay; `desktop-standalone` starts none of it.
- **The per-instance identity trap is broader than a branch switch — *how*
  you launch is the other half of it.** `just dev` uses the default OS keyring
  service `buzz-desktop-dev`, while `just desktop-standalone` scopes the
  service to `buzz-desktop-dev.<branch-slug>` (`Justfile:609`) and unsets
  `BUZZ_SHARE_IDENTITY` (`Justfile:603`); `just dev` sets neither, so it falls
  back to the plain default. So launching the *same* checkout the other way
  presents an empty keyring, and the provider refuses to start a session
  ("has no private key available"). The branch slug (bullet above) is only
  half the rule. Observed 2026-08-20.
- **Non-interactive shells do not source `~/.zshrc`**, so anything an agent
  launches misses profile exports (this is how the mode mismatch happened).
- Ceremony: `LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse upstream/main)
  scripts/integrate.sh`. Park other worktrees on detached HEAD first. Push
  **both** remotes (`origin` = relay, `upstream` = GitHub).
- **`git-credential-nostr` intermittently 401s on the second push request.**
  The first request of a push authenticates, the next one comes back HTTP 401
  and the push aborts; an immediate plain retry succeeds with no other change.
  Suspected NIP-98 replay protection or clock-window handling on the relay's
  git endpoint. Observed twice on 2026-08-19, during the `build/2026-08-19.3`
  and `build/2026-08-19.4` pushes. Retry once before investigating anything.
- **Cross-feature merge conflicts are already solved on the assembly.** When
  a rebuild re-raises them, take `upstream/integrated`'s version of the
  conflicted file rather than reconstructing the union by hand.
- **A bad conflict resolution is persistent.** rerere records whatever you
  commit — including a broken union — and replays it into every later
  rebuild; the same unclosed `impl` came back three times on 2026-08-18. When
  a recurring cross-feature conflict reappears, take `upstream/integrated`'s
  version of the file. To purge a cached bad resolution:
  `grep -rl "<symbol from the hunk>" .git/rr-cache/*/preimage` and delete that
  directory.
- **Cargo.lock drift is invisible to tests and fatal to the deploy.** `cargo
  test` updates the lock in place, so a drifted lock passes every check and
  then fails the release image build. Run `cargo metadata --locked`; the
  ceremony gate and CI now do.
- **Work can be lost between ceremonies.** Two ledger commits made on a wip
  branch did not survive the next rebuild. After any ceremony, check that your
  own commits are present by content, not by assuming the rebase carried them.
- CI status without a login: `https://ci.agiterra.org/api/badges/1/cc.xml`
  (build number + status) — Brian has a Woodpecker login for the rest.
- Relay capability probes live in `/tmp/grant-proof` (`kindprobe`,
  `typeprobe`, `turn`, `stopcmd`): non-mutating checks for whether a
  deployed relay knows a kind or transition type.
- **Which build a relay is running can be read off a rejection message.**
  Run a member-key `buzz pulse update` against a deliberately bogus project
  coordinate: a relay carrying the Pulse code rejects it `restricted: unknown
  project coordinate`, while an older build rejects it `unknown event kind`.
  The write never lands, so nothing is stored — this is the cheapest way to
  confirm a deploy from outside, without a cluster login.
- **A raw NUL byte in a TypeScript source makes git classify the file as
  binary**, which hides it from every diff and every reviewer: `git diff`
  prints "Binary files differ", so a whole-file rewrite reviews as nothing at
  all. Hit **twice**, both times a NUL used as a composite-key delimiter —
  `desktop/src/features/project-pulse/ui/ProjectPulseView.tsx` during the
  Pulse UX pass, and
  `desktop/src/features/agent-progress/lib/agentProgressSources.ts` on
  `wip/agent-sidebar` (fixed by `9cf627a0a`). Use `"\u0000"` in source, or a
  printable delimiter. Worth a lint rule.

## 4. Authorities — unchanged, read when the question is "why"

| Document | Role |
| --- | --- |
| `SESSION_VISION.md` | Product authority: the ten invariants |
| `SESSION_DESIGN_PHASE_PLAN.md` | Design authority: D1–D6, gates G1–G7 |
| `SESSION_EXECUTION_PLAN.md` | Execution contract + rulings R1–R28 |
| `FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md` | Continuity investigation, taxonomy, design space, addenda A–C |
| `P1_JUDGE_SCRIPT.md`, `P1_CONTINUITY_MATRIX.md` | The G1 experiment |
| `CODING_SESSION_UI_CONVERGENCE_HANDOFF.md` | UI convergence slices 2–4 (open) |
| `SESSION_PHASE_HANDOFF_2026-08-18.md` | Ship record + Andy's runbook |

`coding-session-analysis.md` is **not** history — it is the evergreen how-to
for querying stored session data (CLI + SQL). Updated 2026-08-19: its kind
table now covers 44220–44230 plus 44240 (Pulse), with per-kind fold rules and
query examples.

History lives in `docs/archive/` (eight documents, moved 2026-08-18). Their
load-bearing content is extracted into §2 items 9–14 above; the rest is
provenance. Brian's working agreements moved from
`archive/SESSION_HANDOFF_SOL.md` §10 into `AGENTS.md`.

## 5. The rule that keeps this from becoming the nineteenth document

A finding from live use lands **here**, in §2, the day it is found — not in a
new document. New documents are for authorities and experiment protocols
only. When this file's §2 empties into shipped work, it shrinks; it is not an
append-only log.

**Note the irony deliberately:** this file is a hand-maintained ledger of
goals, decisions, open threads, and next steps — exactly the structure the
checkpoint design (report Addendum A) proposes storing as signed session
facts. Buzz should eventually hold this state itself, and this workflow is
its first customer.
