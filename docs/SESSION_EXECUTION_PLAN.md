# Sessions: execution plan (authority phase)

**Status:** execution authority for the phase. Written 2026-08-15.
**Governed by:** [SESSION_DESIGN_PHASE_PLAN.md](SESSION_DESIGN_PHASE_PLAN.md)
(v3 — design decisions D1–D6, gates G1–G7) under
[SESSION_VISION.md](SESSION_VISION.md) (product authority).
**Written for:** an orchestrating agent (Opus) directing implementing
subagents, reporting to Brian.

---

## 1. Execution philosophy — proof before polish

The failure mode this plan exists to prevent: forty hours of battle-hardened
implementation followed by the discovery that the architecture was wrong.
Every bite below is sized to **prove or disprove one architectural bet
cheaply**. The rules:

1. **Each bite has a "Proves" line and a "Stop-and-reassess if" line.** The
   bite exists to test the bet, not to ship a fortress. If the stop
   condition trips, halt the track and report to Brian — do not engineer
   around a disproven premise.
2. **Proof spikes may be throwaway.** P-track code is allowed to be ugly and
   discarded; its deliverable is the answer, not the code.
3. **Hardening is deferred by ledger, not by forgetting.** Each bite lists
   `Defer:` items (rate limits, exhaustive validation, edge UX). Log them in
   the bite's completion report; build them only after the architecture they
   protect has survived its proof. Exception: **fail-closed behavior and
   signature checks are never deferred** — a hole is not a hardening item.
4. **Every bite lands as a reviewable, PR-sized commit series** with tests
   and (for UI) a screenshot, plus a 5–15 line completion report against the
   Proves line.
5. **Small bites still ship whole.** A bite that passes its proof should
   leave the tree releasable (`just ci` green, no half-wired UI).

## 2. Working rules (non-negotiable)

- **Branches** (see SESSION_VISION.md "Andy's Git and integration workflow"):
  upstreamable session/protocol/provider/desktop-session work →
  `feature/coding-sessions` (worktree: `/Users/brian/Projects/buzz-coding-sessions`).
  Cross-feature coupling (project-access integration) and docs →
  `integration/glue` (worktree: `/Users/brian/Projects/buzz-integration-glue`).
  **Never commit to `integrated` or `main`. Never push** — Brian batches and
  runs the integration ceremony himself.
- Activate hermit before any git/build command in a worktree:
  `. ./bin/activate-hermit`. Commit with `git commit -s` (DCO), plus
  `--signoff` on any rebase/cherry-pick.
- `cargo fmt` for the Tauri crate fails in worktrees (CLAUDE.md gotcha #6):
  run `just desktop-tauri-fmt` from the main checkout if it trips.
- `just ci` before declaring any bite done. `just test` (needs
  Postgres+Redis via `just relay` infra) for bites touching
  `buzz-relay`/`buzz-db`/`buzz-auth`.
- In feature worktrees `just ci`'s desktop file-size check needs an
  explicit base — there is no local `main` and origin carries only
  `integrated`: run with
  `CHECK_FILE_SIZES_BASE=$(git merge-base upstream/main HEAD)`.
  Do not edit the upstream script to add fork fallbacks; glue stamps the
  CI base ref for assembled builds. A `just ci` claimed green without
  this leg having run is not green — say which legs ran.
- New event kinds follow the **full checklist in Appendix B.1** — a kind
  registered in one gate but not another is the known foot-gun.
- **Decisions reserved to Brian** (do not implement past them):
  the four [OPEN-BRIAN] items in DESIGN_PHASE_PLAN §7.1 (custody model,
  Contribute rung, goal UI promise, cite-back), contestability of takeover,
  co-owners by default. If a bite reaches one, take the design-plan default,
  note it in the report, and keep the door open.
- Before kickoff, **Brian runs `git fetch origin` interactively** (relay
  origin needs interactive Nostr auth) to confirm no unseen agent work.

## 3. Track structure

```
P-track (proofs, run first / alongside)   A-track (authority)        B-track (machinery)
P1 seed-quality spike (G1) ──────────────────────────────────────► B2 seeding translator
P2 reclone + reachability (G5, G7) ──────────────────────────────► B3 clone-at-coordinate
P3 provider git-read probe (G2) ─────────────────────────────────► B1 coordinate facts
                                          A1 genesis tracer bullet
                                          A2 goal event + pill
                                          A3 preflight unification
                                          A4 chain + acceptance receipt
                                          A5 provider chain enforcement
                                          A6 command binding + relay gate ◄─ release gate
                                          A7 lease + intersection
                                          A8 invite/grant UI (after A6)
                                          A9 takeover (after G3/G4)
B-track meets A-track at B4 fork (needs A1 lineage + B2 seeding).
```

A-track and B-track run in parallel (different owners/subagents). P-track
answers go to Brian and unblock the B-track bites.

---

## 4. P-track — proofs and gates

### P1 — Seed-quality spike (gate G1) — throwaway allowed
**Question:** can a fresh execution meaningfully continue from replayed
44225 history, and how does each adapter best accept it?
**Do:** query an existing session's transcript from the relay (kinds 44225
by `h` + `cs-target`, fold by `eventSeq`), hand-build a provenance-keeping
context package (author pubkey, kind, event id, role per item), feed it to
a fresh Claude adapter execution (then Codex) as (a) initial prompt,
(b) `session/load` where supported. Judge continuation quality on a real
session from Brian's history. Measure size vs the 32 KiB item-shrinking
reality (`fit_item`).
**Proves:** the highest-leverage bet in the phase — that durable-history
seeding produces real continuation, making fork and machine-death recovery
worth building.
**Stop-and-reassess if:** no packaging approach yields a continuation
better than "pasted summary" quality → B2/B3/B4 shrink to record-only
continuity and Brian decides the fork story.
**Needs:** Brian's machine (live relay + adapter logins). Report includes
the package format that worked best.

### P2 — Reclone + reachability probe (gates G5, G7)
**Question:** is the orphaned `agiterra/Hallway` workspace re-clonable from
the relay git service, and what is the cheapest "is commit X reachable in
the relay repo" check?
**Do:** attempt authenticated clone of the bound repo; then prototype a
reachability check (candidates: 30618 ref-state event lookup;
`info/refs` + fetch negotiation; see `crates/buzz-relay/src/api/git/`).
**Proves:** the named recovery case is real; D4a's "confirmed reachable"
fact has a cheap implementation.
**Stop-and-reassess if:** the repo isn't on the relay → G5 fails as a
*test case* (the machinery is still right); pick another repo and tell
Brian the Hallway sessions are record-only.
**Needs:** Brian's interactive auth for lightyear.

### P3 — Provider git-read probe (gate G2)
**Question:** can `buzz-session-provider` cheaply read `HEAD` + dirty state
from the session cwd it already holds?
**Do:** small spike in the provider: at generation start / turn end, shell
`git -C <cwd> rev-parse HEAD` + `git status --porcelain` (bounded, error-
tolerant). Confirm the env fence is irrelevant here (it guards keys, not
cwd access).
**Proves:** D4a facts are obtainable where they need to be signed.
**Stop-and-reassess if:** unexpected sandbox/timing constraints → record
coordinates from the desktop side instead (weaker authorship; flag it).

---

## 5. A-track — authority

### A1 — Genesis tracer bullet (D1 slices 1–3, thin)
**Goal:** the thinnest end-to-end slice of the new authority substrate: an
immutable **session genesis** event; 44221 create carrying `genesisRef`;
provider resolves the genesis and enforces **founder-only** commands;
honest failure receipts; minimal UI surfacing.
**Scope:**
- New kind `KIND_CODING_SESSION_GENESIS` (regular, channel-scoped,
  operator-signed; content: `sessionRef`, `v`; `h` tag). Full Appendix B.1
  checklist, including the ingest test sweep.
- **Genesis uniqueness slice (decided 2026-08-15):** ingest-time envelope
  validators are pure and cannot enforce uniqueness, so uniqueness of
  genesis per (channel, `sessionRef`) is enforced **transactionally in
  buzz-db** using the advisory-lock precedent — first insert wins, a
  duplicate is rejected with `OK false` (no acceptance receipt; receipts
  stay in A4). This slice is deliberate A4 groundwork: it proves the
  first-accepted-wins serialization architecture in its smallest form.
  Requires `just test` (Postgres + Redis).
- **Resolution rule (design invariant):** genesis is always resolved **by
  event id** through the receipt-joined create — nothing ever queries
  "the genesis for `sessionRef` X." Provider backfills the `genesisRef`
  its create names; desktop resolves founder via create→receipt→genesis;
  A4 chains root at a genesis id. Any duplicate that exists anyway
  (pre-uniqueness history, future multi-relay, a bug) therefore degrades
  to a flagged foreign attachment, never a bricked session. The provider
  fails closed on discovered ambiguity.
- 44221 `session.create` gains optional `genesisRef` (event id hex) via the
  **exact-fields two-form precedent** (Appendix B.3): historical 9-key form
  or new 10-key form, nothing between/beyond, with the smuggle-rejection
  test.
- Provider: on create with `genesisRef`, backfill the genesis by id via a
  new `query_event_by_id` on the existing REST bridge (Appendix B.4);
  cache founder pubkey in the `SessionRecord`. Enforce founder-only on
  turn/stop/resume for genesis-bearing sessions; legacy sessions =
  founder-projection interim rule. New error codes `GENESIS_NOT_FOUND`,
  `UNAUTHORIZED_OPERATOR` (Appendix B.3) with receipts published on
  rejection.
- Desktop: publish genesis before create in `useNewCodingSessionCreate`;
  thread `genesisRef`; render a minimal "founded by X" line from the
  genesis (no new panel).
**Proves:** new-kind pipeline end-to-end; provider backfill-by-id;
fail-closed authorization at the provider with acceptable UX; additive
44221 evolution doesn't break existing clients/tests.
**Stop-and-reassess if:** backfill-by-id is unreliable (bridge/auth/timing)
→ D1's bootstrap needs rework **before** the chain is built on it; or the
two-form decoder pattern breaks historical replay → schema strategy
rethink.
**Defer:** takeover, chain, multi-owner; genesis content beyond the
minimum; UI polish.

### A2 — Goal event + pill (must-have #1)
**Goal:** durable session goal, rendered pinned at the top of the session
surface; editable by founder (interim rule) with preflight gating.
**Scope:** new addressable goal kind (`d = sessionRef`, `h` tag, prose
content; Appendix B.1) **choosing a representation that keeps prior
revisions queryable** (verify relay retention of replaced addressable
versions; if replaced versions are purged, use regular events + fold to
latest — [OPEN-BRIAN] §7.1 only governs the UI promise, not history
retention). Desktop: goal display in session workspace + catalog list
rows; edit affordance founder-gated (preflight). Provider is not involved.
**Proves:** the addressable/goal lane and the first user-visible win —
Brian can glance at any session and know what it is doing.
**Stop-and-reassess if:** nothing architectural — this bite is
low-risk by design.
**Defer:** revision-history UI; owner-set-beyond-founder (arrives with A4).

### A3 — Preflight unification (fail-open holes)
**Goal:** close the client-side gaps found in verification: single-execution
composer has **no founder plumbing**; stop/resume/interrupt are **never
gated**; null-founder falls open.
**Scope:** route `CodingSessionComposer` (N=1 path) through the same
authority resolution as the umbrella composer; gate stop/resume/interrupt
affordances; flip null-founder fallback to restrictive **when the session
has a genesis** (legacy sessions keep permissive fallback + a visible
"ungoverned session" hint). Pure desktop; tests per existing composer
model tests.
**Proves:** the UX of fail-closed steering is acceptable before the chain
hardens it — cheap place to discover gating feels wrong.
**Stop-and-reassess if:** founder-only UX blocks legitimate solo flows →
report; the authority model may need a faster grant path in A8.

### A4 — Authority chain, one transition type (D1 slice 5, thin)
**Goal:** the append-only authority chain with **exactly one** transition
type — `grant-operator` — plus relay validation and a relay-signed
**acceptance receipt** establishing the canonical head.
**Scope:** new transition kind (content: `genesisRef`, `prevAccepted`
event id or null, `seq`, `type: grant-operator`, `granteePubkey`; `h`
tag). Relay: validate linkage (genesis exists, prev matches current head,
seq increments, signer == current owner) at ingest; on acceptance, emit a
relay-signed receipt via the `emit_system_message` pattern (Appendix
B.2) and reject non-linking transitions with a clear `OK false` reason.
Storage of "current head" may be derived (query last accepted receipt) —
no new table unless derivation proves too slow. Desktop: minimal "grant
operator" dev affordance (hidden/flagged is fine) + head display.
**Proves:** the riskiest novel architecture — relay-validated append-only
authority with relay-signed acceptance — on the smallest possible
surface, **before** revoke/transfer/takeover multiply the state machine.
**Stop-and-reassess if:** acceptance serialization is racy or awkward in
the ingest path (ordering vs pub/sub, multi-node), or derived-head
queries are pathologically slow → reconsider consumer-side selection or a
`session_authority` projection table **before** writing more transition
types. This is the bite most likely to change the design — treat every
surprise as signal, not an obstacle to code around.
**Defer:** revoke/transfer/takeover types; NIP-CSL/NIP-CSC spec text
finalization (draft amendments in the bite, finalize after A6).

### A5 — Provider enforcement from the chain
**Goal:** the provider authorizes turn/interrupt against the accepted
head (founder/owner + granted operators), not just the founder.
**Scope:** provider subscribes/backfills transition receipts for its
sessions; maintains per-session operator set; `decide_turn` checks signer
∈ set; stop/resume/end stay owner-only. Queue receipts: publish
accepted/queued/started ordering facts (ACP single-in-flight reality).
**Proves:** chain state is consumable at the point of execution with
acceptable freshness (a just-granted operator can act without provider
restart).
**Stop-and-reassess if:** chain-state propagation to the provider is too
slow/fragile → head caching strategy needs rework before A6 depends on it.

### A6 — Command binding + relay enforcement (collaboration release gate)
**Goal:** unauthorized commands stop entering the record. 44220 (and
44221 resume/stop) gain `sessionRef` + accepted-head event id via the
two-form precedent; relay validates signer against the head it minted the
acceptance for; unauthorized → `OK false`.
**Scope:** payload fields + envelope validator extension + relay check +
ingest test sweep (accept/reject matrix); provider tolerates both forms;
desktop sends the new form when a genesis exists. Copy gating patterns
from NIP-ST (fan-out branch / fail-closed lookups) where applicable.
**Proves:** the relay can enforce session authority cheaply from
event-carried bindings — no target→session index needed.
**Stop-and-reassess if:** head-reference staleness creates rejection
storms for legitimate operators (grant races) → widen acceptance window
(accept N recent heads) or revisit binding design.
**This bite gates A8.** Rate limits + freshness hardening: `Defer` here,
**required** before phase close (pattern: `ScopedRateLimiter`, NIP-ST).

### A7 — Host lease + intersection
**Goal:** the two-authority split becomes real: driving a *live execution*
requires session capability ∩ host lease; the lease is launcher-issued,
target-scoped, expirable — "may spend this host's quota" made explicit.
**Scope:** lease as a signed event from the launcher (target `cs-target`,
grantee, expiry; or "session-operators" mode tracking the head);
provider enforces intersection for turns into a live execution;
continuation/fork paths explicitly need no lease. Desktop: launcher
affordance ("allow this session's operators to drive this execution") +
quota-spend labeling.
**Proves:** the intersection model is implementable without confusing the
common owner==launcher case (it must stay invisible there).
**Stop-and-reassess if:** [OPEN-BRIAN] custody question is answered by
Brian before this bite — custody (control token) would *replace* parts of
this design; ask before building if the answer hasn't arrived.
**Convergence note (2026-08-15):** builtin-shell's Terminals settings
already ship per-session host consent toggles (INTERACT / AGENTS, "gated
by per-session consent" via the local session broker) plus
persist-across-restarts with honest dead-session rows — independently
the same host-grants-control and continuation shapes as this bite and
B-track. A7's lease UX must rhyme with those toggles (one consent
vocabulary across terminals and coding sessions, per the vision's
"single shared reality"), and the B-track continuation copy should match
the plain "reopen to respawn where it left off" register. Coordinate
with Andy before A7's UI lands.

### A8 — Invite/grant UI (after A6)
**Goal:** must-have #3's visible face: invite a channel member as
operator, see roles, see quota labeling. Reuse Andy's
`PersonaShareRecipients` picker (`allowDirectPubkeyEntry`) and
project-access dialog idioms.
**Blocked by:** A6 (release gate) and the **durable-record-vs-revocation
design note** (DESIGN_PHASE_PLAN §7.1 — write it as part of this bite:
what revocation means for already-replicated history; ingredients: NIP-09,
ACL re-reveal precedent).

### A9 — Takeover (after G3 + G4)
**Goal:** `takeover` transition type; admin-sourced; timeline-visible;
honest inheritance bounds (no machine, no memory, no dirty work).
**Pre-work in-bite:** G3 check (relay reads `relay_members` for admin
standing at validation; desktop displays from 13534/39001). G4 is **split
and scheduled** (ruling R12): a narrow chain/race review inside A4's
acceptance; the full adversarial review (forged genesis, replay, competing
takeovers, head-stripping) as its own scheduled bite after A6/A7 schemas
stabilize — **written artifact approved before the first A9 code commit**;
use a strong-model reviewer.
**Stop-and-reassess if:** G4 finds a hijack path → fix rules first;
takeover is the one place a design error is a security incident.

---

## 6. B-track — machinery (parallel owner)

### B1 — Coordinate facts (D4a; after P3)
44223/44224 carry: observed commit, dirty flag, `repoRef`,
relay-reachability confirmation + verification timestamp (five separate
facts; "recoverable" only when confirmed — P2's check). Two-form payload
evolution + tests. UI: coordinate + "one push from durable" hint on the
session surface.
**Proves:** honest coordinates flow without breaking transcript consumers.

### B2 — Seeding translator (D4b; after P1, using its winning format)
Productize the P1 package: translator in `buzz-session-provider`,
provenance-preserving, truncation policy, per-adapter delivery. Wire into
cross-machine continuation: a create carrying the umbrella's `sessionRef`
on a fresh machine offers "continue from history."
**Proves:** continuation-after-machine-death reaches "seeded" quality in
the real product path.
**Stop-and-reassess if:** P1's quality doesn't survive productization
(size limits, adapter drift) → ship record-only continuation, report.

### B3 — Clone-at-coordinate (D4c; after P2 + B1)
Attach flow on a machine without the workspace: when `repoRef` is bound
and coordinate confirmed reachable, offer clone + checkout into a chosen
directory, then proceed as normal create. Reuse git smart HTTP +
credential helper. Surface honesty: state what was lost (dirty flag at
last record).
**Proves:** the full machine-death recovery narrative end-to-end on the
Hallway test case (or P2's substitute).

### B4 — Fork, same machine (D5 slice 1; after A1 + B2)
Fork = new `sessionRef` + genesis carrying the **cut manifest**
(DESIGN_PHASE_PLAN D5: parent genesis id, authority head at cut,
per-execution high-water marks, session-lane cut, access statement) + new
execution seeded via B2 + goal event ("slice: …"). Same channel as parent
(access equivalence). No lease, no owner approval.
**Proves:** the decomposition workflow Brian described — master session,
fork per slice, full history carried.

### B5 — Fork, cross machine (after B3 + B4)
B4 + clone-at-coordinate on the forker's machine.

---

## 7. Reporting and cadence

- Per bite: a completion report (5–15 lines) — proof verdict, surprises,
  deferred-hardening ledger entries, screenshots for UI, commit list.
- Per stop-condition trip: halt the track, write what was learned, what
  you recommend, and wait for Brian on that track (the other track
  continues).
- Batch commits on the feature branch; **no pushes**; tell Brian when a
  coherent slice is ready for his integration ceremony.
- Questions that hit an [OPEN-BRIAN] item: take the design-plan default,
  flag it in the report, don't block — except A7 custody and A9, which
  ask first.

## 8. Suggested subagent economy

Orchestrator (Opus) holds the plan and reviews; implementing subagents per
bite; cheap-tier agents for mechanical sweeps (find-callsites, test
inventories); strong-model agents only for the G4 adversarial review and
for verifying claims the plan marks unproven. Verification agents check
implementers' work against the bite's Proves line before a bite is called
done.

---

## Appendix A — Opus kickoff prompt

See the prompt Brian was handed alongside this plan; canonical copy:

> You are the orchestrator for the Buzz sessions authority phase.
> Read, in order: `docs/SESSION_EXECUTION_PLAN.md` (execution authority —
> your contract), `docs/SESSION_DESIGN_PHASE_PLAN.md` v3 (design
> authority), `docs/SESSION_NEXT_PHASE_BRIEF.md` (context),
> `docs/SESSION_VISION.md` (product authority), all on the
> `integration/glue` worktree at
> `/Users/brian/Projects/buzz-integration-glue`. Also read `CLAUDE.md` at
> the repo root and follow its rules (hermit activation, `git commit -s`,
> `just ci`, worktree gotchas).
>
> Execute the execution plan's bites: P-track proofs first (P1 needs
> Brian's live relay/logins — coordinate with him; P3 is pure code),
> then A-track and B-track in parallel. Honor every bite's
> "Stop-and-reassess if" line literally: when one trips, halt that track
> and report — do not code around a disproven premise, and do not
> battle-harden anything whose architecture has not passed its proof
> (fail-closed behavior and signature checks are never deferred).
> Feature work goes on `feature/coding-sessions`
> (worktree `/Users/brian/Projects/buzz-coding-sessions`), coupling and
> docs on `integration/glue`. Never push; never commit to `integrated`
> or `main`. Decisions reserved to Brian are listed in Execution Plan §2
> — take defaults and flag, except A7 custody and A9 takeover, which ask
> first. Report per §7 after every bite. Before writing any code, confirm
> Brian has run the interactive `git fetch origin` check from
> DESIGN_PHASE_PLAN §7.2, and start with A1 + P3 (both pure-code) while
> waiting on anything that needs him.

## Appendix B — Implementation recipes (verified file:line, 2026-08-15)

### B.1 New event kind checklist
1. `crates/buzz-core/src/kind.rs`: constant + doc (pattern at 558–611);
   `ALL_KINDS` (1091–1096); compile-time shape assertions (1264–1301);
   opt into capability sets only if semantics apply (`AUTHOR_ONLY_KINDS`
   129, `P_GATED_KINDS` 159, etc.) — coding-session kinds use none.
2. `crates/buzz-relay/src/handlers/ingest.rs`: import (13–40); scope in
   `required_scope_for_kind` (358–368); `requires_h_channel_scope`
   (656–667); extend the family predicate `is_coding_session_kind`
   (675–685) or add one; content cap (696–704) or envelope validator
   (pattern 2133–2218, dispatch 3048–3070); strict membership gate call
   (2827); extend the in-file test sweep (6155+,
   `coding_session_predicate_covers_exactly_…` pattern).
3. `crates/buzz-sdk/src/builders.rs`: builder (section at 2304; pattern:
   validate payload → ordered payload-derived tags → `Kind::Custom`).
4. `desktop/src/shared/constants/kinds.ts`: constant (84–108) + the
   `CODING_SESSION_EVENT_KINDS` aggregate (112–119) so it stays out of
   channel timelines.
5. No migration needed (generic events table); no search change unless
   p-gating changes.

### B.2 Relay-signed acceptance receipt
Template: `emit_system_message`,
`crates/buzz-relay/src/handlers/side_effects.rs:789-821`
(`EventBuilder::new(...).sign_with_keys(&state.relay_keypair)` → insert →
pubsub publish). Wire-up: add triggering kind to `is_side_effect_kind`
(side_effects.rs:35-37), match arm in `handle_side_effects` (194–221);
generic dispatch fires post-insert at ingest.rs:3608-3620. Idempotent
reconciliation shape if needed: `reconcile_channel_events` (3246–3300).

### B.3 Payload evolution + error codes
Two-form exact-fields precedent (add optional field compatibly):
`crates/buzz-core/src/coding_session_lifecycle_command.rs:18-27`
(sessionRef amendment), `require_exact_fields_with_optional` (319),
canonical smuggle-rejection test
`rejects_action_shapes_between_and_beyond_the_two_forms` (625–640).
Error codes: `crates/buzz-core/src/coding_session_payload.rs:35-49`;
`ReceiptStatus` + constructors (54–182). Provider decision plumbing:
`decide_lifecycle` (commands.rs:170-322), `CreatePlan` (72–94), tests
from commands.rs:504 (e.g. `…fails_with_provider_unavailable` at 636).

### B.4 Provider backfill-by-id
No existing by-id fetch (verified). Build on
`HarnessRelay::query(&[nostr::Filter])`,
`crates/buzz-acp/src/relay.rs:417` (NIP-98 `POST /query` bridge);
filter-construction pattern `discover_channels` (687–745) with
`nostr::Filter::new().id(...)`. Live subscriptions (watermark, not by-id):
`crates/buzz-session-provider/src/lib.rs:366-383`.

### B.5 Desktop consumption wiring
Provider-fact lane: `TRUSTED_CODING_SESSION_INGRESS_KINDS`
(`desktop/src/features/coding-sessions/lib/useTrustedCodingSessionIngress.ts:29-33`,
filter at 85–99); classification branches + shape-sniffing cross-check in
`codingSessionTrustedIngress.ts:213-236`. Operator-authored publish
paths: `codingSessionCommand.ts:120`,
`codingSessionLifecycleCommand.ts:135,259`. Create↔receipt join:
`codingSessionCreateObservations.ts:32-33,67-68`. Catalog fold:
`useCodingSessionCatalog.ts:25-39`. New human-signed lanes (genesis,
transitions, goal) follow the create-observation pattern (own
subscription, join to receipts), not the trusted-ingress lane.

### B.6 Test surfaces
Relay ingest rules: in-file sweep in `ingest.rs` from 6155 (`---- Coding
sessions ----`) — extend it; no e2e coverage exists for these kinds (add
e2e only for live fan-out behavior unit tests can't reach). Provider:
inline tests in `commands.rs` (504+) and `lib.rs` (1190+). Desktop:
existing composer/model test patterns beside the files they test.

### B.7a Rulings log — 2026-08-15 (Opus Q1–Q14, Sol review, Fable adjudication)

Consolidated rulings on the questions raised during A1/P-track execution.
Sol's recommendations were adopted on all fourteen, with the addenda noted.
These override earlier text where they conflict.

- **R1 (branch):** Build against `feature/coding-sessions` alone; it must
  compile and test independently. Copy *behavioral patterns* from NIP-ST
  (A6) without importing feature-only modules. The A8 picker reuse is real
  cross-feature coupling: either extract a generic base component or put
  the adapter on `integration/glue`. `feature/coding-sessions` must never
  depend on `feature/project-access`. An integration ceremony is needed
  before any cross-feature testing (Brian schedules).
- **R2 (P1 owner):** Implementer builds the provenance package + procedure;
  Brian selects the real session, authorizes two bounded runs (one Claude,
  one Codex), and judges continuation quality. P1 blocks B2/B3/B4 only.
- **R3 (DB test gate):** A security property whose test never runs is not
  a proof. Add a **targeted** isolated-Postgres gate running the genesis
  concurrency proofs (`--run-ignored ignored-only`, externally serialized
  if needed). Do not make the whole buzz-db suite serial-safe this phase.
- **R4 (reachability):** Provider-side, via a narrow
  `git_object_reachable(repo, oid)` on the shared relay client
  (`HarnessRelay` already carries the NIP-98 `RestClient`). It must
  reproduce the git credential helper's repo-root NIP-98 signing, not the
  REST bridge's exact-path binding. Run it async; the provider signs the
  resulting fact. No desktop-side authorship weakening; no new relay
  endpoint.
- **R5 (event-loop stall):** Fix now, not at B1. The probe already runs
  per completed turn with a structural worst case near 4 s. Move probing
  to a task/result channel; this also enables R14.
- **R6 (genesis deletion):** **Deletion must not release `sessionRef`.**
  A genesis is a permanent identity anchor: reject NIP-09 deletion of
  genesis events, and count soft-deleted rows in the uniqueness probe as
  defense in depth. Sessions end via lifecycle facts, never by freeing
  identity. (Consistent with the A8 durable-record-vs-revocation note:
  identity facts are permanent; content redaction is a separate question.)
- **R7 (`csg-session` tag):** Keep it, restrict its normative meaning:
  relay/storage may use it for uniqueness enforcement and diagnostics;
  **consumers must never select authority by it** — authority resolves
  only through an explicit `genesisRef` event id. Fix the
  `coding_session_genesis.rs` module doc, which currently invites
  tag-based consumer queries in contradiction of the invariant.
- **R8 (legacy sessions — security-critical):** With first-insert-wins
  uniqueness, an ordinary genesis could *claim* a legacy `sessionRef`
  (first-claim hijack). Before genesis deploys: reject an ordinary
  genesis whenever accepted legacy create history already uses the
  `sessionRef`; add an **adoption** flow — a genesis referencing the
  accepted founding create/receipt, relay-verified so the signer matches
  the deterministically projected founder (receipt-joined earliest
  create); genuinely ambiguous founders require audited admin adoption
  (arrives with A9 machinery). Until adoption ships, legacy sessions stay
  readable and visibly ungoverned — and unclaimable. Addendum: the
  uniqueness probe must count adoption geneses identically (see R6).
- **R9 (44221 shapes):** Exactly three accepted forms — historical
  8-key, 9-key with `sessionRef`, 10-key with `sessionRef` + `genesisRef`.
  `genesisRef` without `sessionRef` is rejected; so is everything else.
- **R10 (git method binding):** Existing constrained behavior; document
  as security debt, do not redesign this phase. B1 mimics the credential
  helper's signing contract (see R4).
- **R11 (`hall` vs `hallway`):** Not demonstrated as a bug — provider
  state shows `repoRef: null` on the surviving records, and project vs
  repo `d` values are independent under NIP-MP. Needs the exact compared
  events before it's treated as drift; otherwise drop.
- **R12 (G4):** Split and scheduled — see A9 pre-work text.
- **R13 (multi-relay):** Confirmed intended, not accidental:
  `sessionRef` is an umbrella *label*; canonical identity is the genesis
  **event id** in its relay/community/channel context. The same signed
  genesis mirrored elsewhere is the same origin; an independently signed
  genesis with the same UUID is a different session. Reinforces R7.
- **R14 (public async API):** Undo. Once R5 lands, restore
  `handle_session_event` to synchronous and `pub(crate)`; no external
  callers exist, and the feature branch should stay upstream-clean.

**Immediate order:** R6 → R7 → R8 → R3 gate (and run the gated proofs) →
R5/R14 → run `just ci` to completion (it has not yet been reported green
over the buzz-db commits) → resume A1 bootstrap. Opus's flagged
unverified items (P2's timing/pkt-line figures, P3's timing table, the
concurrency mutation claim) are re-verified by a checking agent when B1
consumes them — not before.

### Rulings addendum — 2026-08-15 stop-point review (R15–R19)

Adjudication of the R8 deviation Opus flagged and Sol's five findings.
**The genesis E2E does not start until R15–R17 land**; the E2E is then
written against the final contract, including negative cases (wrong and
missing adoption references, both deletion mechanisms).

- **R15 (explicit adoption — the implicit deviation is rejected).**
  Adoption must be a durable statement **inside the signed genesis
  payload**: an `adopts` object carrying the founding create's event id
  and its receipt's event id (payload field — keeps the three-tag
  envelope; no fourth tag, no separate adoption event). The relay
  validates against the *referenced* events: they exist, the receipt
  joins the create, the create bears the `sessionRef`, and the genesis
  signer matches the create's signer (the projected founder). A signed
  event's meaning must never depend on what a particular relay's local
  database happens to contain (R13): without the reference, the same
  event is an adoption on one relay and a fresh genesis on another, and
  no auditor or replica can reconstruct its justification.
  **Standing rule, phase-wide: a subagent's file-set boundary is never a
  reason to alter a protocol contract — escalate instead.**
- **R16 (ambiguity parity — security-significant).** The relay's
  adoption validation must reproduce the desktop's rule exactly: for the
  founding `commandId`, inspect **every** create in the channel bearing
  that command id and reject on signer *or* `sessionRef` disagreement.
  The current shape (filter to candidate `sessionRef` first, then check
  signers) makes cross-`sessionRef` disagreement invisible to the relay
  while the desktop treats it as ambiguous. Fix + DB test.
- **R17 (probe fencing).** "Last arrival wins" is an observation, not a
  guarantee: a slow old probe can overwrite newer state. Per-session
  monotonic probe generation; results older than the latest applied
  generation are discarded (or the prior probe is cancelled); add a
  deterministic reversed-completion test.
- **R18 (gate placement and hygiene).** `just ci` stays infra-free by
  contract, so a green `just ci` must never be cited as proof of genesis
  uniqueness — completion reports cite the gate run itself. The gate
  must run wherever the infra path runs (`just test` and the repo CI
  pipeline), and its database name must be unique per run (fixed
  `buzz_genesis_gate` collides across simultaneous worktrees).
- **R19 (export completeness — ledger item).** `buzz sessions export`
  fetches only 44223/44224/44225: no operator commands, no genesis, no
  future transitions — not a complete audit bundle. Either expand it to
  the full command/authority record or rename it honestly as a
  provider-transcript export. Does not block A1; must be resolved before
  phase close (it is the "attributable corpus" story's tooling face).

**Order:** R15 → R16 (adoption path finalized) with R17 in parallel →
R18 → genesis E2E on the final contract. Sol's verification of R6/R7,
the gate proofs (9/9), and the provider suite (131 + startup) is
accepted as the current baseline.

### B.7 Patterns to copy from shipped work
Project ACL composition + fail-closed gate:
`crates/buzz-db/src/project_acl.rs`, `ProjectGate::admits` (see
upstream/feature/project-access). Observation gating template (fan-out
branch, `*_hidden_from` read predicate, coordinate-gate cache, rate
limiters, freshness): `crates/buzz-relay/src/handlers/shell_observe.rs` +
`docs/nips/NIP-ST.md` (upstream/feature/builtin-shell). Invite picker UI:
`PersonaShareRecipients` with `allowDirectPubkeyEntry`
(upstream/feature/project-access, `EditProjectContainerDialog.tsx`).
Note: these live on Andy's branches — when a bite needs them, coordinate
with Brian on whether the phase builds against `integrated` (which will
contain them after the next ceremony) or cherry-picks context.
