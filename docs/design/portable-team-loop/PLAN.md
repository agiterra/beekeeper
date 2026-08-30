# Portable Team Loop

Status: executing  
Program base: `0f37c1d4`  
Finish line: Andy completes the cold-host acceptance in §8 without Brian's
application state, terminal archaeology, or a separate session summary.

## 1. Outcome

Beekeeper must make a multi-participant mission portable across machines and
clients. An authorized teammate who pulls one exact SHA must be able to prepare
their host, start or join the same mission, understand assignments and evidence,
and determine what requires action from the signed session itself.

This program closes the gap between a working transport and a working team
product. The vision already requires another authorized teammate to open the
same session and understand it without a separate summary
([SESSION_VISION.md](../../SESSION_VISION.md#the-decisive-proof)). Today the
transport can deliver a turn, but the product cannot durably distinguish an
assignment, report, verdict, acknowledgement, or terminal mission state.

## 2. Product invariants

1. **Conversation remains first-class.** A one-agent session opens in the
   existing Conversation lens. The Singularity remodel is a Mission lens, not a
   replacement for the conversation experience.
2. **The lens never changes execution.** Conversation and Mission are
   projections of the same signed session. Switching lenses publishes nothing.
3. **Never force-switch an open session.** Mission may be suggested after a
   second seat or typed team transaction exists. The user's per-session choice
   persists.
4. **One chronology, multiple densities.** Mission supplies Brief, Live, and
   Trace without deleting evidence. Brief may not hide a blocker, failure,
   verdict, acknowledgement request, or required human action.
5. **Participants outrank providers.** The visible identity is the person or
   role. Provider, model, and routing remain inspectable facts.
6. **Host truth and wire truth stay separate.** Checkout, keychain, runtimes,
   local identities, and provider process state are host observations. Catalogs,
   creates, grants, reports, verdicts, and mission dispositions are signed wire
   facts.
7. **Unknown is not ready.** A missing probe produces an explicit unknown or
   blocker with one remedy. It never silently passes.
8. **Silence is not completion.** Idle, unreachable, stalled, blocked, waiting
   on a person, and complete remain distinct states.
9. **Prose is not structured evidence.** A message saying `3/3 passing` does
   not populate Tests. Only a valid signed report can do that.
10. **Password boundaries remain intact.** Development, tests, fixtures, and
    unsigned builds run continuously. Password-gated signing/relaunches are
    batched at milestone acceptance; the program does not weaken key custody.

The supplied design inputs are
`/Users/brian/Downloads/singularity/Singularity.png` and
`/Users/brian/Downloads/singularity/beekeeper-singularity-ui-remodel.md`.
They govern hierarchy and visual intent. Signed truth, the product vision, and
the shipped implementation govern behavior.

## 3. Baseline already proven

- Provider-neutral routing can independently choose Claude and Codex seats in
  one mission with no model override.
- Signed creates carry the router's complete decision, catalog revision, and
  diversity reason.
- Item 99 landed at `0f37c1d4fb4025ae6115cd647efb40b5a567a56d` and
  canonicalizes absent `profile` to `null` so desktop creates match Rust's
  provider-signed shape.
- Item 99 passed `just ci`: 6,924 desktop tests, 2,794 Tauri tests, and 1,716
  mobile tests, plus Rust, web, typecheck, lint, and production builds.
- Live create/44223 byte equality remains the last item-99 acceptance proof
  ([SESSION_STATE.md](../../SESSION_STATE.md)).
- Actor-owned bylines, honest observed-change states, and liveness demotion are
  already shipped. The remodel must reuse them rather than rebuilding them.

## 4. Architecture

The program has three planes and one finalizer.

### A. Team Readiness — before the session

One read-only report joins the host facts currently scattered across onboarding,
Agents, Project settings, provider settings, and launch-time refusals:

```text
build      app version, source SHA
project    project ref, checkout, HEAD, dirty state
roles      discovered/installed/missing, digest, source revision
identities role, pubkey, key accessible, attested, profile-sync state
provider   provisioned, running, provider pubkey
runtimes   adapter instance, installed/auth state, probed models
registry   path, schema/revision, parse state, host target coverage
hiring     enabled, allowed roles/providers, capacity
wire       relay reachable; catalog observed or awaiting first session
blockers   stable code, source, exact remedy
```

`team_readiness(projectRef)` is read-only. An explicit, idempotent
`prepare_project_for_team(projectRef, names)` performs local preparation and
returns a fresh report. It does not create a channel merely to manufacture a
catalog.

The primary UI action is **Prepare this project for teams**. The current folder
installer becomes an Advanced/manual fallback, not the main journey.

### B. Team transactions — inside the session

Add one append-only session event kind, `44244`, with schema
`buzz-coding-session-team-transaction/v1`. The closed operations are:

```text
assignment
report
verdict
acknowledgement
mission.completed
mission.blocked
```

The signature is the author. Common fields name the session, genesis,
operation, and optional superseded event. Exact tags carry channel, session,
schema version, genesis, and operation type.

The canonical semantic fold is:

```text
assigned -> reported -> ruled -> acknowledged -> settled
```

Provider delivery remains orthogonal. Existing `44220` commands and receipts
say whether a seat was queued or started; they never become semantic
acknowledgement.

CLI operations that must wake a seat publish the typed record first, then the
existing human-readable command with a shared command ID. If record publication
fails, nothing is sent. If delivery fails, the durable record remains visible as
**Authored, delivery unconfirmed**.

### C. Mission lens — the Singularity surface

The existing Conversation lens remains unchanged. Mission adds:

- participant chips derived from demoted per-seat status;
- a compact 44px live-activity bar instead of the occluding active-work card;
- Brief, Live, and Trace density modes;
- a narrative stream whose execution bundles remain reversible;
- one Inspector/Context surface for goal, accepted plan, seat plans, changes,
  files, structured tests, team, decisions, and diagnostic context;
- a mission card driven by typed transactions, not inactivity;
- explicit completed, blocked, waiting-on-person, stalled, and unknown states.

### D. Single finalizer

Parallel lanes do not edit the shared workspace composition. One finalizer
integrates their frozen contracts, owns responsive layout and lens persistence,
removes superseded paths, runs the complete gates, and lands the batch.

## 5. Execution stages

### Stage 0 — Close routing equality

1. Land item 99. **Done at `0f37c1d4`.**
2. Wait for the relay/desktop deployment boundary.
3. Relaunch from the landed SHA.
4. Run a fresh no-override builder/verifier hire.
5. Read the verifier create and provider-signed 44223 from the wire.
6. Require byte-identical routing JSON and record IDs, byte count, hash, signer,
   and SHA in the ledger.

### Stage 1 — Parallel foundations

#### R1. Readiness contract

Owns new Tauri readiness commands, tests, command registration, and build SHA
provenance. It may reuse existing probes but does not redesign the installer or
touch Mission UI.

Gate:

- cold host returns stable blockers without writes;
- locked keychain is `KEYCHAIN_UNAVAILABLE`, not a folder error;
- a second prepare preserves provider and identity pubkeys;
- missing, dirty, and wrong-project packs remain distinguishable;
- partial profile sync is prepared-with-warning;
- omitted source SHA is explicitly unknown.

#### T1. Transaction protocol

Owns the core schema, kind registry, protocol document, and conformance vectors.
The allocation scan preserves the existing 44231 checkpoint, 44232 native
snapshot, 44233/44234 transition/check, 44235-44239 headroom, and 44240-44243
reservations. `44244` is the lowest unused and unreserved kind in this fork and
vanilla.

Gate:

- exact key and tag sets;
- tag/content parity;
- content and collection bounds;
- cross-session, dangling-reference, and supersession rejection;
- backwards compatibility for clients that ignore the new kind.

#### U1. Fixture-driven Mission stream

Owns new participant-bar, live-activity-bar, and presentation-model files only.
It consumes existing trusted projection types and the supplied PNG/Markdown. It
does not edit shared workspace wiring or invent wire facts.

Gate:

- single-seat fixture remains Conversation by default;
- Mission participant status agrees with existing W1 liveness words;
- overflow roster remains usable;
- 44px activity bar distinguishes open-turn and terminal tool counts;
- light/dark, keyboard, reduced-motion, and zoom behavior are covered.

### Stage 2 — Complete the foundations

#### R2. Readiness UI and launch gate

Add the Agents readiness card, one Prepare action, exact remedies, and the Team
launch gate. A real session remains visible if its first signed catalog is late;
the UI says hiring is not ready and why.

#### T2. Relay, SDK, CLI, and provider context

Deploy structural validation before enabling writes. Add SDK builders and:

```text
bee sessions assign
bee sessions report
bee sessions verdict
bee sessions acknowledge
bee sessions complete
bee sessions block
bee sessions operation get|list
```

`complete` refuses when referenced assignments lack an acknowledged approving
verdict. Provider inbox context carries only operation ID/type; full records are
fetched from the signed session.

### Stage 3 — Parallel Mission UI

After the transaction decoder and readiness JSON contracts freeze:

- **Stream lane:** header hierarchy, participant chips, activity bar,
  Brief/Live/Trace, execution-bundle styling, scroll-follow.
- **Inspector lane:** accepted plan vs seat plan, structured Tests, Changes,
  Files, Team, Context, mission state, rejected-event disclosure, terminal usage.

Neither lane edits `CodingSessionUmbrellaWorkspace.tsx`.

### Stage 4 — Integration and landing

The finalizer alone:

1. adds the Conversation/Mission lens control and per-session persistence;
2. wires the Mission components into the shared workspace;
3. deletes the superseded active-work dock path;
4. resolves wide/narrow layouts and inspector drawer behavior;
5. updates the Singularity surface contract and living ledger;
6. runs focused real-state tests, `just ci`, relay integration tests, and one
   deliberate `just smoke` on the integrated UI;
7. captures distinct screenshot states and verifies unique hashes;
8. lands one coherent batch and performs one password-gated signed relaunch.

## 6. Worktree and ownership rules

Every build lane starts from a recorded `main` SHA in an isolated worktree.
Lanes do not commit. They report their diff and evidence; the finalizer rebases,
reviews, integrates, runs gates, and creates the signed commit.

No two concurrent lanes own the same file. In particular,
`CodingSessionUmbrellaWorkspace.tsx`, its integration test, and
`docs/SESSION_STATE.md` belong only to the finalizer.

The initial worktrees are:

```text
beekeeper.worktrees/team-readiness
beekeeper.worktrees/team-transactions
beekeeper.worktrees/singularity-stream
```

## 7. Automated acceptance

- Core schema rejection and compatibility vectors.
- Relay authorization, channel isolation, and session-reference tests.
- CLI record-before-command ordering and visible delivery failure.
- Report -> verdict -> acknowledgement -> terminal link integrity.
- Prose test claims never become structured test results.
- Existing one-agent `sessions send` behavior remains unchanged.
- Existing liveness three-voice tests remain green.
- Brief never hides attention or required action.
- Readiness cold-host and idempotency tests.
- Restart/recovery E2E between report and verdict.
- Distinct screenshot hashes for Conversation, Mission Live, Brief, Trace,
  completed, blocked, waiting, and stalled.
- `just ci` on every landing batch.
- `just test` for relay/core changes.
- `just smoke` once on the integrated desktop release candidate.

## 8. Andy cold-host acceptance

Acceptance runs only after the preparation path is in the product. Andy is the
proof, not the bootstrap mechanism.

1. Andy pulls one exact landed SHA with no copied app data, keys,
   `managed-agents.json`, or Brian-local state.
2. He opens the project and presses one Prepare action.
3. He resolves only blockers named by Team Readiness; no terminal or
   `SESSION_STATE.md` is required.
4. He launches a lead, builder, and verifier without model/provider overrides.
5. The signed session shows catalog revision, creates, byte-consistent routing,
   grants, assignment, report, linked verdict, acknowledgement, and terminal
   mission disposition.
6. A builder clones and pushes as its own identity.
7. The app/provider restarts between report and verdict and reconstructs the
   same fold.
8. A second authorized client opens the same session and identifies objective,
   ownership, changed files, structured tests, verdict, blockers, and required
   action without a separate summary.
9. One provider is aged out; header, participant chip, and inspector agree on
   **No provider answering**.
10. Conversation remains the unchanged default for a one-seat control session.

The program is complete only when all ten steps pass and their event IDs, SHAs,
signers, commands, and screenshots are recorded in the living ledger.

## 9. Landing cadence

Land coherent milestones, not micro-fixes:

1. routing canonicalization and live proof;
2. readiness contract plus preparation UI;
3. transaction protocol through CLI/provider context;
4. integrated Mission lens;
5. Andy cold-host acceptance findings, followed by one closure batch if needed.

Password-gated signing and app relaunch happen at the end of these milestones,
not after each implementation lane.
