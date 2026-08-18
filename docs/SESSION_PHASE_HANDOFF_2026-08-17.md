# Sessions phase handoff — 2026-08-17 (to Sol)

**For:** Codex Sol, taking over the coding-sessions authority phase.
**From:** the Fable session that ran 2026-08-16/17 (ceremony, A5 live proof,
UI surface host, session-stability fixes).
**Supersedes:** `SESSION_PHASE_HANDOFF_2026-08-16.md` (still accurate for
history; this document carries the current state).

---

## 0. The standard this project runs on

**A completion report is not evidence.** Three times in three days a plan
claim was wrong and source-checking caught it (R15 implicit adoption, R20
projected legacy authority, and last night my own "the limit rejected your
End click" — the code showed the guard is create-only and the End was never
sent). Cite `file:line`. Verify before relying, including on this document.

Corollary earned last night: **when a subagent's report contradicts your
diagnosis, the report is often right.** The stability agent overturned three
of my six premises with source evidence, and the fixes were better for it.

---

## 1. Read in this order

| Document (on `integration/glue`) | Role |
| --- | --- |
| `docs/SESSION_VISION.md` | **Product authority.** 2028 horizon, ten invariants. |
| `docs/SESSION_NEXT_PHASE_BRIEF.md` | Input brief; note the epistemic markers. |
| `docs/SESSION_DESIGN_PHASE_PLAN.md` (v3) | **Design authority.** D1–D6, gates G1–G7. |
| `docs/SESSION_EXECUTION_PLAN.md` | **Execution contract.** Bites, stop-conditions, and the rulings log §B.7a — now **R1–R28**. Read the rulings; they are decisions with reasons. |
| `docs/CODING_SESSION_UI_CONVERGENCE_HANDOFF.md` | Codex-authored UI convergence spec (Slice 1 shipped; Slices 2–4 open). |
| This document | Current state, last night's forensics, next moves. |
| `docs/P1_JUDGE_SCRIPT.md` | Brian's judging protocol for the P1 gate. |

Also read the repo root `AGENTS.md` — **the workflow changed on 2026-08-16**
(§7 below).

---

## 2. State of the plan

### 2.1 Shipped and deployed (live on `lightyear.agiterra.org`)

Ceremony `build/2026-08-16.4` pushed `integrated = 98809318` to both remotes
(relay origin *and* the GitHub mirror). The relay auto-deployed and is
serving it.

- **Genesis** (44226), **goal** (44227), preflight unification, explicit
  adoption — all as described in the prior handoff.
- **Authority chain** (44228) + relay-signed acceptance receipt (kind 40099).
- **A5 — the provider consumes the chain.** Turn authorization = founder ∪
  granted operators; stop/resume stay owner-only. Trust anchor is the
  relay-signed 40099 verified against the relay identity witnessed from
  NIP-11 `self` at connect; the accepted 44228 is then resolved by explicit
  `acceptedEventId` and re-verified (R21-clean).
- **Coordinate facts** on 44223 (observedCommit / dirty / relayReachable /
  verifiedAt), with a real relay reachability probe.
- **UI surface host** — Agents + "Observed changes" as sibling tabs in one
  collapsible/resizable/keyboard-operable host.

### 2.2 The A5 proof — completed live, in production

Every leg passed against the deployed relay on 2026-08-17:

| Leg | Evidence |
| --- | --- |
| Forged genesis refused | `invalid: genesisRef does not name a coding-session genesis this relay has stored` |
| Founder grant accepted | transition `c9cdc7a9…`, `OK true`, seq 1, on genesis `cc7a54d3…` |
| Relay-signed acceptance | 40099 receipt `ed7e754f…` signed by `2ba5c5e7…` = the relay's NIP-11 `self` |
| Rival transition refused | `prevAccepted does not match the chain's current head (expected c9cdc7a9…)` |
| **Provider applied it live** | `csp::authority: applied accepted grant-operator transition … seq=1 grantee=49f706d6…` at 01:32:56, mid-session, **no restart** |
| **Survives restart** | same line again at 01:49:07 after a full process death, following `witnessed relay identity relay_self=2ba5c5e7…` |

**Not yet done:** the grantee-turn leg (send a 44220 signed by the scratch
operator key and watch the turn run, then a stop and watch it refuse).
Requires the provider instance that holds that session (`69146ae0…`, no
longer the running one). Optional; the authority path is proven.

Tooling: `/tmp/grant-proof` (scratch cargo project, path-deps into the repo)
with `genesis` / `publish` / `receipts` subcommands. Scratch grantee key at
`/tmp/p1-scratch-grantee.key` (0600). **Note:** that grant is append-only
until a `revoke` transition type exists — it is a live operator grant on a
real session, not a test artifact that can be swept.

### 2.3 Never ran: P1 (unchanged, and now the biggest open question)

The seed-quality gate has still not run. **The harness is now built and
waiting**: `scripts/p1-seed-spike/` (untracked in the assembly worktree,
committed on the spike branch) with `translate.py`, `acp_probe.py`,
fixtures, and `RUNBOOK.md`; Brian's judging protocol is
`docs/P1_JUDGE_SCRIPT.md` (three arms: cold / pasted-summary comparator /
seeded; five scored questions; explicit pass and stop-condition thresholds).

Two findings from building it, both load-bearing:

- **The in-band seed budget is 12 KiB, not 32** — `MAX_TURN_TEXT_BYTES`,
  `crates/buzz-core/src/coding_session_command.rs:16`.
- **`session/load` cannot carry a relay package** (`crates/buzz-acp/src/
  acp.rs:876-902`): it replays the adapter's *local* store via the
  machine-bound cursor. **Initial-prompt is the only relay-durable seeding
  channel on every adapter**, which means the entire cross-machine
  continuation story rides on prompt-path quality — exactly what P1 judges.

Needs one hour of Brian's time. It gates B2/B3/B4 only.

---

## 3. Last night's forensics — read before touching session code

Brian's live session degraded badly; the root causes are fixed but the
lessons are structural.

### 3.1 The metadata blackout (the phase's cautionary tale)

The coordinate-facts bite shipped the provider half and never taught the
desktop parser. `parseBuzzCodingSessionMetadata`'s exact-key validation —
correct and deliberate security behavior — then **rejected every
fact-bearing 44223 event** the moment the new provider binary started
publishing. One additive protocol change produced a total blackout of the
metadata lane, wearing three costumes: create dialogs hung on "waiting for
its signed metadata", the project shelf missed new sessions, and statuses
degraded to "Status unknown". Fixed in `cb251c9b`.

**The rule this earns:** a payload-evolution bite is not done when the
producer ships. Every strict consumer of that payload — Rust *and*
TypeScript — must be taught the new shape in the same bite, and the
two-form/N-form discipline (Appendix B.3) applies to the desktop decoders
too. This is the same class of failure as A4/B1 shipping storage halves
with no consumer.

### 3.2 Multi-instance provider corruption

Three orphaned `buzz-session-provider` processes shared one state directory:
one `session.create` was consumed by four consumers (four ACP sessions from
one click — Brian's "three Codex sessions"), and interleaved writes
physically tore the JSONL ledgers. Fixed by an exclusive lock on the state
dir + kill-stale-then-start in the supervisor + single-`write()` appends.

### 3.3 Fixes now on `wip/session-stability` (8 commits, unpushed)

`cd0f751e` single provider instance · `a92fd728` stale-Reconnect echoes no
longer multiply reconnect cycles · `f4d36bd7` MSRV 1.89 (std file locking) ·
`d5fad362` desktop backfills stored session events once the live edge exists
· `6f288183` retried create reuses its published channel · `64bb9652` join
fails visibly when the umbrella's genesis is unresolved · `09159885` session
limit counts only running actors · `cb251c9b` desktop accepts coordinate-fact
metadata.

Gates run: `cargo test -p buzz-session-provider -p buzz-acp` (all pass),
tauri lib tests 2531 pass, desktop `tsc --noEmit` clean, full desktop suite
**5412/5412**, biome clean on changed files. Not run: full `just ci`
(mobile/web legs untouched), `just test` (no relay/db changes).

**Gate: Brian's confirmation that the app behaves better.** He was using it
at the end of the night; durability was confirmed by him ("i believe we have
durability. Ack"). If he says ship it, split per §7 and run the ceremony.

---

## 4. Open bugs, verified, not yet fixed

1. **No way to rename a session** → R26.
2. **No working End affordance**; and a stop for a session the provider has
   no record of is **silently ignored** (`crates/buzz-session-provider/src/
   commands.rs:225-231`, `Ignored::UnknownTarget`) → R27. An orphaned
   session is currently unendable by anyone, forever, with no feedback.
3. **No archive/hide** → R28.
4. **`SESSION_LIMIT` is unactionable**: "provider is already running its
   maximum of 4 session(s)" (`commands.rs:306-313`) names no slot-holder.
   The cap is create-only and is *correct*; the failure it exposes is that
   users cannot see what occupies their four slots. **Do not raise the cap**
   — Brian's judgment, and it is right: raising it multiplies invisible
   state. Build the running-sessions view instead.
5. **A crashing adapter can never be classified as auth-required.**
   `mentions_auth` only inspects `AgentError { message }`
   (`crates/buzz-session-provider/src/session.rs:571-573`), so an adapter
   that exits *because it is not logged in* surfaces
   `PROVIDER_UNAVAILABLE` / "Agent process exited unexpectedly" — and the
   guided-login Connect affordance never appears. **Andy hit exactly this
   on his machine.** The most likely first-run failure routes around its own
   remedy.
6. **Adapter stderr is inherited, not captured** (`crates/buzz-acp/src/
   acp.rs:639-640`), so the one diagnostic that names the cause never
   reaches the log a user can send. Capture a bounded tail into the provider
   log and echo a snippet in the failure receipt.
7. **Execution labels collide**: every execution row is labelled with the
   provider pubkey, so multi-execution sessions render identical targets
   (Brian could not tell three Codex sessions apart). Needs per-execution
   identity in the label. Belongs to UI Slice 2.

---

## 5. Decisions in force (beyond R1–R24)

- **R25 — custody *and* co-input both ship**, owner-selectable per session;
  the lease/capability intersection decides *who may drive*, the mode
  decides *how drivers interleave*; contributing is never gated. Custody is
  the flagged-reversible default for multi-operator sessions; a solo session
  has no mode ceremony.
- **R26 — name and goal both exist** (separate operator-signed lanes).
- **R27 — close a session ≠ stop an execution** (two controls; close needs
  no provider).
- **R28 — archive is personal, encrypted-to-self, never destructive.**

Still reserved to Brian: takeover contestability (A9), R19 export identity.

---

## 6. Recommended order from here

1. **Split + ceremony for `wip/session-stability`** once Brian confirms
   (§3.3). It is strictly better than what is deployed, and Andy benefits.
2. **Session-management bite** (R26/R27/R28 + the running-sessions view with
   slot accounting + an actionable `SESSION_LIMIT`). This is what Brian is
   actively blocked by, and it is mostly desktop + one small kind.
   Prototype-first on a `wip/*` branch cut from the fresh assembly.
3. **Adapter diagnosability** (§4.5, §4.6) — small, and it is what stands
   between Andy and a working session on his machine.
4. **A5b — the dropped front halves**: acceptance-receipt timeline rows,
   authority-head display, and the coordinate-facts surface ("one push from
   durable"). Made urgent by §3.1: the facts have been published since the
   bite landed and still have no visible consumer.
5. **P1** whenever Brian has the hour. Reshapes the whole B-track.
6. **A6** — command binding + relay enforcement. Remains the collaboration
   release gate: no invite UI (A8) before it. Watch its stop-condition —
   A5's ledger already flags the grant-then-immediate-turn race as a
   rejection-storm candidate.
7. **UI convergence Slices 2–4** per the Codex handoff.
8. **Deferred on purpose:** revoke/transfer/takeover transition types until
   A6 survives contact; A7's lease build (R25 shapes it); B3 pending P2 and
   the reachability-semantics fix (advertised-tips-only; see prior handoff
   §6); R24 subagent vocabulary before any Agents-tab work.

---

## 7. Working rules — **the workflow changed**

`a1cbd52a docs(integration): prototype-first development workflow` (Andy,
2026-08-16), in `AGENTS.md` + `docs/INTEGRATION.md`:

> Cut `wip/<topic>` from `integrated-build`, commit freely there (feature
> code and cross-feature wiring together), **let Brian manually test each
> feature addition**, and only when he confirms, split the patch into the
> owning `feature/*` branches + glue, reassemble, and verify
> `git diff wip/<topic> integrated-build` is empty.

Standing caveats:

- A `wip/*` branch cut from the assembly only contains what has been
  ceremonied. Work that depends on unshipped feature-branch code must still
  be built on the feature branch (that is why A5 was not prototype-first).
- **Recommend adding to the ceremony gate:** after a split, build/test each
  touched `feature/*` branch *in isolation*. The equivalence check proves
  the tree matches; it cannot prove the work was routed to the right branch,
  and nobody ever runs a feature branch alone under this workflow.
- Never commit to `integrated`/`integrated-build`/`main`. Never push without
  Brian's word. `git commit -s`. Activate hermit before any git/build/test.
- `just ci` in feature worktrees needs
  `CHECK_FILE_SIZES_BASE=$(git merge-base upstream/main HEAD)`.
- Honor every bite's stop-and-reassess line literally; fail-closed behavior
  and signature checks are never deferred as hardening.

---

## 8. Environment facts (hard-won; do not rediscover)

- **Ceremony:** `LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse
  upstream/main) scripts/integrate.sh`. Park other worktrees on detached
  HEAD first. If it stops mid-rebuild, `scripts/integrate.sh` is
  *missing from the tree* (it is glue-owned and the half-built assembly
  predates glue) — rerun it from another worktree's copy.
- **Pushes need `LEFTHOOK=0`** too, or pre-push hooks abort every ref.
- **Two remotes, both required:** `origin` = the relay (lightyear),
  `upstream` = GitHub. They drift; Andy pushes to GitHub, `integrate.sh`
  pushes only to origin. Push both.
- **Known gate flakes** (documented in `docs/INTEGRATION.md`, all reproduced
  last night): `mesh_demo::…round_trips_echo` (loopback QUIC) and the two
  `buzz-pair-relay` timing tests — 51/51 in isolation. Do not chase.
- **Two new ceremony conflicts are now in rerere** (Cargo.lock crate-block
  ordering; `side_effects.rs` project-ACL vs authority-transition dispatch)
  — thirteen recorded resolutions.
- **CLI against the deployed relay without env setup:**
  `scripts/p1-seed-spike/p1` (`check` / `export` / `buzz …`) reads Brian's
  identity from the macOS keychain at runtime (`buzz-desktop`, falling back
  to `buzz-desktop-dev`), never prints it, and defaults to
  `https://lightyear.agiterra.org`. Verified working.
- **Provider state lives per app-instance**:
  `~/Library/Application Support/xyz.block.buzz.app.dev.<slug>/session-provider/<pubkey>/`
  with `state.json`, `commands.jsonl`, `outbox.jsonl`, and `logs/`. The slug
  comes from the branch (`scripts/instance-env.sh`), so **switching the
  assembly worktree's branch gives the app a different provider identity and
  a different session set** — expected, but it surprises everyone once.
- **Outbox format:** `{"op":"enqueue","entry":{…}}` / `{"op":"ack","id":…}`.
  Pending = enqueued ids with no ack. (Naive parsing reports zero events.)
- **Relay identity:** `2ba5c5e72c56deaf6ab43283de414fad1a8800d48f6d1f353a6409771d13dda7`.

---

## 9. What Brian is waiting on

1. **His verdict on `wip/session-stability`** → then split + ceremony.
2. **His P1 hour** → unblocks the entire B-track.
3. Andy's adapter output (`claude-agent-acp` run directly; `which claude`)
   → diagnoses his machine, and tests whether he can *observe* Brian's
   session while his own provider is down (the collaboration property).

The mission, unchanged: **make a session a durable, shared, multiplayer work
surface.** Observation is done. Granting is proven end-to-end and one UI
(plus A6's safety gate) from shipping. Continuation is one judged hour from
being decidable. What stands between here and a product Brian enjoys using
is mostly the unglamorous session-management surface in §4 — build that
first.
