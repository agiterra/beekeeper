# Sessions: the cleanest path

Companion to [SESSION_VISION.md](SESSION_VISION.md). Where the vision doc
deliberately avoids implementation, this document records the path from the
code as surveyed on 2026-08-12 to the experience the vision describes. It is
ordered so every step ships user-visible value alone, no step reshapes the
wire contract, and the single-provider case never inherits multi-agent
ceremony (product invariant 2).

## Where the code already is

The survey findings that shape the path:

- **The wire contract is provider-neutral today.** Kinds 44220–44225
  (NIP-CSC/CSL/CSPC/CST) carry opaque `driver`/`runtime`/`model`/
  `capabilities`; the relay validates envelopes and stores — it never parses
  provider content. Unknown transcript item kinds legally degrade to visible
  status rows in the desktop renderer.
- **The catalog schema supports 32 providers per event**
  (`crates/buzz-session-provider/src/catalog.rs`, `MAX_PROVIDERS = 32`); the
  builder hard-codes exactly one Claude entry (`catalog.rs:115-122`). The
  multi-provider seam is cut, not filled.
- **A provider registry already exists**: `KNOWN_ACP_RUNTIMES`
  (`desktop/src-tauri/src/managed_agents/discovery.rs:78`) lists goose,
  claude, codex, and buzz-agent with adapter commands, auth probes, and
  model-switching flags. The session provider consults it for exactly one
  lookup.
- **The adapter command is already per-session**: `CreateRequest.agent_command`
  (`crates/buzz-session-provider/src/session.rs:65`). One provider sidecar,
  one identity, one outbox can spawn a different ACP adapter per session.
- **Observation is a subscription, not a feature**: every 44225 carries an
  `h` tag and flows through normal per-channel fan-out. A second client
  observes with `{kinds:[44223,44224,44225], "#h":[channel], authors:[trusted]}`,
  narrowed per-session by `#cs-target`. Reads compose with project ACL;
  session *writes* take a strictly stronger gate (active membership).
- **Agent-to-agent messaging works today**: same-owner agents are
  cryptographically verified siblings (`is_owner_or_sibling`,
  `crates/buzz-acp/src/lib.rs:198`) and may prompt each other over `kind:9`
  with `p`-tag addressing, under the default `owner-only` policy. It is
  observable because it is ordinary channel traffic.
- **Claude coupling is small and enumerable**: the frontend bootstrap target
  (`newCodingSessionModel.ts:96-125`), auth-failure copy, four constants in
  `crates/buzz-session-provider/src/config.rs`, the single-entry catalog
  builder, `PROVIDER_AGENT_BINARY`/`resolve_claude_code_executable` in the
  Tauri supervisor, and the `Capabilities::claude_agent_acp()` constructor
  name.
- **The genuinely coupled parts are inference, not schema**: changed-file
  diffs are reverse-engineered client-side from Claude-shaped tool argument
  names (`codingSessionTranscriptModel.ts:306-376`); result cost/duration
  round-trip through a display string and a regex.
- **T3 Code (reference, inspected locally)** ships the same architecture —
  durable provider-neutral thread, detachable `provider_session_runtime`
  with resume cursor, normalized activity kinds, and per-turn workspace
  `checkpoint_diff_blobs` — but its schema allows only **one active provider
  runtime per thread**. Multiple concurrent executions in one session is the
  step past T3, and Buzz's shared relay is why it is reachable.

## Economic model (decided)

Everything runs on subscriptions. An execution runs on the machine and
provider login of whoever launched it; tokens follow the execution. A
teammate "continuing" a session means a **new execution on their own
subscription and machine** attached to the same durable session. There is no
shared-key or metering problem to solve.

## The path

### Step 0 — reconcile local work

`feature/coding-sessions` is two commits ahead of origin (provider-startup
reliability `30573991`, live Claude model discovery `fbf24a42`). The
integrated worktree carries an uncommitted partial copy of the first. The
model-discovery work must forward-port onto the projects-aware create flow;
the conflict surface is confined to `newCodingSessionModel.ts`,
`useNewCodingSessionCreate.ts`, and `NewCodingSessionScreen.tsx`. Nothing
below depends on this, but it removes drift before the seams are touched.

### Step 1 — second provider, same surface (Codex)

One provider process advertising several `providerInstanceRef`s — **not** N
supervised sidecars (which would multiply identities, trust entries, and
outboxes for no user benefit).

- `buzz-session-provider` config gains a runtime list; the catalog builder
  emits one `CatalogProvider` per configured runtime instead of one literal.
- `session.create` routes `providerInstanceRef` → adapter command; the field
  for this already exists per-request.
- The Tauri supervisor generalizes `PROVIDER_AGENT_BINARY` +
  `CLAUDE_CODE_EXECUTABLE` into per-runtime `{adapter_command,
  underlying_cli_env}` pairs sourced from `KNOWN_ACP_RUNTIMES`.
- The frontend bootstrap target becomes a list served by a Tauri command
  (host-local provider descriptors) instead of the `claude-primary` literal;
  `PROVIDER_AUTH_REQUIRED` remediation copy comes from the provider/receipt,
  not a hard-coded Claude string.
- `Capabilities::claude_agent_acp()` becomes per-runtime capability vectors.

Done when: the create screen offers Claude and Codex, both stream into the
identical transcript surface, and the picker was populated by catalog events
rather than code.

### Step 2 — harden the neutral transcript

Kill the two places where provider-specific shape leaks through the neutral
schema:

- Structured `costUsd`/`durationMs` fields consumed as fields (the display
  string is derived, never re-parsed).
- A modeled `file_change`/diff transcript item emitted by the provider,
  replacing client-side inference from tool argument names. `capabilities.diff`
  already exists to gate it honestly.

Without this, the second provider "works" but silently produces empty diff
cards — the worst kind of neutrality.

### Step 3 — observer mode (phase one of sharing)

The substrate is done; this is UI plus one derived fact.

- The **operator** is the signer of the 44221 create (and of subsequent
  44220 commands) — already on the wire, no schema change to *identify* one.
- Non-operators with read access get a read-only composer and an
  "operated by &lt;name&gt;" affordance; the current binary `isMember` check
  becomes operator/observer.
- Optionally strengthen the relay gate so only the operator (or an
  explicitly added co-operator) may publish 44220 against a target;
  phase one may enforce this client-side only.
- Cross-client, cross-machine observation needs no new plumbing: channel
  membership + project ACL + the existing subscription filter.

Observation is the load-bearing primitive: by 2028 even the operator is
mostly an observer of their own agents. One transcript renderer, two
permission levels.

### Step 4 — multiple agents in one session, comms visible

The contract's rule that transcript facts from different signers never merge
is correct — do not fight it. A user-facing session becomes an **umbrella
over N executions plus a conversation**:

- Add a `sessionRef` (umbrella reference) carried by 44221/44223 the same
  way `projectRef` is. New generation → same sessionRef. Second provider
  joining → new execution (new `cs-target`) under the same sessionRef.
- The session surface renders all member executions' transcript rails plus
  session-scoped `kind:9` conversation, time-ordered, signer-attributed.
- Agent-to-agent communication reuses the sibling mechanism in `buzz-acp`;
  the work is (a) wiring `SessionMetadata.agent_ref` (currently hardcoded
  null) so executions have attributable managed-agent identity, (b) tagging
  their conversation into the session scope, (c) rendering it in the primary
  narrative with progressive collapse.

Nothing here changes what a single-provider session looks like: one
execution under one umbrella renders exactly as today.

### Step 4.5 — addressing sugar and persistent session agents

Two follow-ons Brian has called for explicitly (2026-08-12):

- **@mention sugar (immediate):** typing `@codex` / `@claude` in the umbrella
  composer is a shortcut for the participant selector — pure UI mapping onto
  the same 44220 turn routing. No wire changes, no interaction with the
  managed-agent mention system.
- **Persistent session agents (named future step):** wire
  `SessionMetadata.agent_ref` (structurally present, currently always null)
  so an execution can be backed by a durable managed-agent identity
  (kind 30177) instead of an anonymous per-provision key. This is the
  convergence point between coding sessions and buzz agents: executions
  become mentionable first-class identities, can persist across sessions
  (memory, persona, reputation), and can address each other without the
  operator mediating. Prerequisites named in SESSION_STEP4_DESIGN.md:
  delegation grammar, signer plumbing (the sidecar must see the command
  author), loop budgets, cost governance. Do not start this before the
  umbrella surface has proven itself in daily use.

### Managed-agent ↔ session interop (Brian, 2026-08-12: "seamless")

Upstream built persistent managed agents (30177 identities, 30174 engram
memory, 30175 personas) and coding sessions on the same fabric but
deliberately uncoupled (`agent_ref` held null). The seam is the Step 4
conversation lane: it is kind:9 + mentions — the exact protocol managed
agents already speak. Two convergence directions, in order:

1. **Managed agents INTO sessions (near-term, Step 4.5-sized).** Example:
   a merge-captain agent. Invite it to the host channel; @mention it in
   the session lane with the session's `buzz://` deep link; it reads the
   signed transcripts via the CLI's read-only sessions surface, does repo
   work with its existing tools (git signed with its Nostr key), and
   reports in the lane as an attributable participant. Zero new wire
   concepts; needs only lane p-tag mentions + a session-context link
   convention. Crucially it never authors 44220s — it works on the repo
   and talks in the lane, so no new authority model is required.
2. **Session executions INTO managed agents (later = the persistent
   session agents step).** Wire `agent_ref` → 30177 so executions gain
   durable identity, engram memory, and mentionability. Only here do
   delegation grammar, loop budgets, and cost governance become
   prerequisites (already listed in SESSION_STEP4_DESIGN.md).

Do 1 before 2; it delivers the "buzz agent in charge of merges" experience
while the dangerous powers stay parked.

### Step 5 — continuity and control (later, ordered by demand)

- **Steering** (`threadSteer` is declared but false everywhere).
- **Approvals**: kinds 46010–46012/46030–46031 are reserved and
  `SessionStatus::WaitingForInput` exists; WF-08 is the stub to finish when
  session approvals matter.

**Possible future — portable session handoff.** Buzz remains the source of
truth for signed history, session identity, membership, and authority. At a
turn boundary, a local checkpoint provider could capture the working tree and
publish it to an already-authorized private Git remote. A teammate could then
choose **Continue on this machine**, restore that checkpoint, and start a new
execution from their own provider/subscription under the existing
`sessionRef`, primed from the durable transcript. Entire-compatible checkpoint
capture may be one optional backend, not the session authority or transcript
store. Buzz-hosted Git should only become a checkpoint backend after its
clone/push/fetch path is separately certified in the intended deployment; it
is not on the critical path for multi-provider sessions.

## Reference implementation: t3code (MIT)

T3 Code's source is public and MIT-licensed
(`github.com/pingdotgg/t3code`; local checkout at
`/Users/briansweet/agiterra/t3code`). MIT is compatible with this repo's
Apache-2.0 and with DCO sign-off (clause b). Rules of use:

- Borrow freely — read, port, or copy. For substantial copied/ported
  portions, retain the MIT notice (header comment
  `Portions derived from t3code, © 2026 T3 Tools Inc., MIT License` or a
  THIRD-PARTY-NOTICES entry). Idea-level borrowing needs nothing.
- The shipped desktop app bundle carries no license grant — only the source
  repo does. Borrow from the repo, not the asar.
- Value ranking (calibrated by reading the source): (1)
  `apps/server/src/provider/Drivers/` — tested per-provider quirk catalogs
  (Claude, Codex + CodexHomeLayout, Cursor, Grok, OpenCode; ~160–420 lines
  each) — the spec for Step 1's second adapter; (2) their test files as an
  edge-case checklist (provider unavailable mid-turn, recovery, resume
  cursors, pending input); (3) `apps/server/src/checkpointing/` — small
  (~700 lines non-test), core is git-diff-at-turn-boundary via
  `@pierre/diffs` (a library we can depend on directly) — re-derive in Rust
  for Step 5; (4) `apps/web/src/*-logic.ts` pure modules
  (composer-logic, modelSelection, proposedPlan, session-logic) — framework-
  free, tested, and shaped like our own `lib/*.ts` model files.
- **UI reference Brian explicitly likes (2026-08-12): T3's live Agents
  panel** — the right rail beside the conversation showing orchestration as
  it runs: workflow name + phase chips with done/active states, one row per
  agent (label, agent-type chip, model, reasoning effort, token count, tool
  count, duration, completion check, one-line result preview inline), a
  "Direct Spawns" section for one-off agents, and an "N agents working in
  the background" bar with a Stop affordance near the composer. This is the
  target pattern for how a Buzz umbrella session should surface execution
  activity: the transcript stays the narrative; the panel is the glanceable
  machine room. When we build the umbrella session's activity/observer rail
  (Step 4 follow-ons, Step 3 observer mode), model it on this — study
  T3's `apps/web/src` implementation for structure, rebuild in our design
  system.
- Skip wholesale UI/component porting: their React app is bound to TanStack
  Router, their RPC layer, and their design system; rebinding costs more
  than building against our own transcript model.
- Their backend is Effect-TS with a local-sqlite-projection architecture;
  ours is Rust with signed relay events. Borrow their answers, keep our
  substrate — the relay (shared observation, attribution, durability) is
  the differentiator, so backend reuse is transliteration with the source
  open, never structural adoption.

## What we deliberately are not doing

- No second supervised provider identity per runtime (one sidecar, N
  adapters).
- No merging of transcript facts across signers.
- No live-process migration; continuation is a new execution against durable
  state.
- No relay-side parsing of provider content; the relay stays a validating
  store.
- No multi-agent ceremony in the single-agent flow.

## Execution sandboxing (finding, 2026-08-12)

A Claude agent running inside a Buzz coding session was asked to audit its own
environment. It reported, and the code confirms, that the ACP adapter inherits
the sidecar's entire environment (`AcpClient::spawn` does not clear it):

1. **`BUZZ_PRIVATE_KEY` (the provider's nsec) is visible to the agent** —
   that key is the trust anchor for kinds 44222–44225, so an agent can sign
   transcript facts the desktop renders as authentic, and `BUZZ_AUTH_TAG`
   makes them carry the owner's NIP-OA delegation. *Fix landed: scrub the
   provider's credentials at the sidecar→adapter boundary. The managed-agent
   harness keeps its deliberate injection — a managed agent is supposed to act
   as itself; a coding-session execution is not a managed agent
   (`agent_ref` is null by design).*
2. **App infrastructure secrets leak in from the developer's `.env`**
   (`BUZZ_S3_*`, `TYPESENSE_API_KEY`, keyring service), letting an agent write
   the media store and search index directly, bypassing relay authorization.
   *Same fix.*
3. **The agent inherits the operator's personal MCP servers** — in this audit
   that included authenticated Gmail and Google Calendar, plus browser control
   with arbitrary script evaluation. Env scrubbing does NOT fix this: it comes
   from the provider CLI reading the operator's own user-level configuration.
   Fixing it means launching the adapter against a scoped config/profile
   directory. **Open.** This must be settled before sessions become steerable
   by other members (Step 3/4), because a shared session would otherwise give a
   teammate a path to the operator's personal integrations.
4. **Filesystem scope is the user's, not the session's** — the agent could read
   every other local session transcript. Inherent to shell-capable agents;
   noted so nobody assumes the working directory is a boundary.

The agent's own summary is the right way to hold this: *"the boundary here is
my own compliance, not the environment's."* Product invariant 9 (sensitive
execution state stays protected) is about the shared record; this finding is
about the execution sandbox, and the two must both hold before shared
observation becomes shared participation.
