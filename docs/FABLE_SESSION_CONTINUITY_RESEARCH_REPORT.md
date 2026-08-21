# Durable coding-session continuity — research report

**Answers:** `docs/FABLE_SESSION_CONTINUITY_RESEARCH_BRIEF.md`
**Date:** 2026-08-17 · **Repo state:** branch `wip/session-management-build-2026-08-17`,
HEAD `6ff77128`, with uncommitted WIP in `crates/buzz-session-provider/src/{context_projector,lib,session}.rs`
**Method:** four parallel read-only investigations (Buzz provider/protocol, Buzz
desktop, T3 Code, comparative products), followed by direct re-verification of
every load-bearing claim at the cited line. Citations marked **[V]** were
re-read directly by the report author; all others come from the read-only
investigation passes and cite exact `file:line`. **[C]** = committed at HEAD,
**[W]** = working-tree-only WIP. No product code was modified.

---

## 1. Executive finding

**The observed failure is not a missing mechanism. It is a missing truth.**

Buzz already possesses the strongest durable-history substrate of any system
surveyed — a relay-stored, signature-verified, channel-shared transcript that
survives process death, machine death, and provider death, which no comparable
product has. It also already contains a working (uncommitted) rehydration path
that projects that history into a verified private package and serves it to a
new execution over a read-only MCP server. The observed user-facing failure —
"a new execution attached to a visible session behaved like a stranger" —
happened in the gap between three things the product currently conflates:

1. **Durable UI history** — relay events the desktop renders. Portable,
   shared, verified, always present.
2. **Provider-native model context** — the adapter's own conversation state,
   reachable only through a machine-local opaque cursor. Never portable.
3. **Reconstructed orientation context** — the rehydration package. Built
   best-effort, delivered only through MCP tools the model must choose to
   call, and — decisively — **never disclosed to anyone when it fails**.

The continuity outcome of attaching a new execution is decided provider-side
by a chain of seven conditions, every one of which fails silently to `Fresh`
(`crates/buzz-session-provider/src/lib.rs:885-978`); a `Fresh` create
publishes **no status event at all** (`lib.rs:807-815` **[V]**); the one wire
signal that says "context was lost" (`resumed_without_context`) is decoded by
the desktop and then discarded
(`desktop/src/features/coding-sessions/lib/codingSessionTrustedIngress.ts:614-660`
**[V]**); and the attach dialog promises the new execution "joins this same
session" with no statement about context
(`desktop/src/features/coding-sessions/ui/AddCodingSessionProviderDialog.tsx:80-84`).
So when the user asked the new execution to identify its continuity mode, the
system had never told the user, the model, or itself what that mode was.

Every comparable product that reliably reopens weeks-old sessions does it one
of two ways: **replay a self-contained local transcript into a stateless API**
(Claude Code, Pi, Codex's cold path), or **point a provider-native cursor at
the provider's own machine-local store** (T3 Code, Codex's hot path). Buzz
already stores the better artifact — the portable one — and already has both
delivery transports built (a bounded prompt/systemPrompt path and an
on-demand MCP path). What is missing, in order of importance:

1. **Continuity as a first-class disclosed fact** — published for every mode
   including `Fresh`, rendered by the UI, and promised honestly at attach
   time. This is small, mostly wiring, and would have converted the observed
   failure from a betrayal into an accurate label.
2. **A deterministic orientation floor** — a small, always-delivered seed
   (session identity, goal, mode, provenance counts) via the already-built
   systemPrompt transport, so orientation does not depend on the model
   electing to call a tool.
3. **Evidence** — the P1/continuity-matrix experiment, designed and ready,
   has never run. Whether reconstructed context is *good enough to be worth
   promising* is currently an untested bet.

A secondary but serious independent finding: the transcript pipeline
publishes tool-call inputs and tool results verbatim (bounded only by size)
into channel-readable relay events, including absolute host paths — in
tension with the crate's own stated invariant that signed content never
carries filesystem paths (`transcript.rs:344-372` vs `state.rs:38-41`). See
§5.3.

---

## 2. Current Buzz behavior — end-to-end map

### 2.1 The boundary table

Every persistence boundary the brief asked to prove, with writer / location /
readers / lifetime / portability / integrity:

| Boundary | Written by | Lives in | Readable by | Survives proc restart | Survives machine move | Survives provider change | Integrity |
|---|---|---|---|---|---|---|---|
| **Durable session identity** — genesis 44226 | Human founder (desktop signs: `desktop/src/features/coding-sessions/lib/codingSessionGenesis.ts:40-45`) | Relay Postgres, channel-scoped `h` tag | Any channel member | ✅ | ✅ | ✅ | Schnorr sig + relay uniqueness probe under advisory lock (`crates/buzz-db/src/event.rs:1533,1567`; insert `crates/buzz-db/src/lib.rs:2320-2327`); undeletable (`crates/buzz-relay/src/handlers/side_effects.rs:259-264`) |
| **Goal 44227 / Name 44229 / Closure 44230** | Human, desktop-signed (`codingSessionGoal.ts:47`, `codingSessionName.ts:77`, `codingSessionClosure.ts:90`) | Relay Postgres | Channel members | ✅ | ✅ | ✅ | Sig; append-only latest-wins revisions (`crates/buzz-core/src/kind.rs:632-633,660-661`) |
| **Signed transcript** — 44223/44224/44225 | Provider process (its Nostr key) | Relay Postgres (+ transient local outbox) | Channel members | ✅ | ✅ | ✅ | Per-event sig; consumer-side signer trust (`crates/buzz-session-provider/src/agent_fence.rs:4-6`); elisions carry byte count + SHA-256 (`transcript.rs:27-29`) |
| **Execution identity** | Provider | Wire target `{driver, instance_id, session_id, generation}` (`crates/buzz-core/src/coding_session_command.rs:23-32`) | Everyone (in every event) | ✅ | n/a (label) | n/a | Deterministic key encoding (`:92-106`); no separate "execution id" exists — execution = `session_id` + `generation` |
| **Provider-native conversation id** — `resume_cursor` | Provider (captured from adapter) | `state.json` (`crates/buzz-session-provider/src/state.rs:105-108` **[V]** — "Never published or passed through the adapter environment") | Provider process only | ✅ | ❌ meaningless elsewhere | ❌ adapter-specific | None — opaque; validity discovered only by trying |
| **In-memory agent state** — the actual model conversation | ACP adapter subprocess | Adapter process memory + adapter's own store (e.g. `~/.claude/projects/**.jsonl`) | Nobody in Buzz | ❌ (proc) / ✅ (adapter store, same machine) | ❌ | ❌ | None visible to Buzz |
| **Machine-local provider state** | Provider | `BUZZ_CSP_STATE_DIR`: `state.json`, `commands.jsonl`, `outbox.jsonl`, `context-packages/*.json` (0700/0600, `context_store.rs:97-122`), `projects.json` | Local user | ✅ | ❌ | partially (state format is Buzz-owned) | Atomic writes (`state.rs:6-8`); seq-before-publish invariant (`state.rs:14-20`); outbox dropped on key rotation (`publish.rs:12-17`) |
| **Membership & session authority** | Owner signs 44228; **relay** signs 40099 acceptance | Relay Postgres | Channel members | ✅ | ✅ | ✅ | Two-witness rule (`authority.rs:1-24`): relay-signed receipt verified against NIP-11 `self` witnessed at connect + explicit `acceptedEventId` resolution; relay enforces head/seq/owner atomically with storage (`crates/buzz-db/src/event.rs:2480-2574`); fail-closed on unknown transition types (`authority.rs:88-91`) |
| **User-facing continuity labels** | Provider (status items in 44225); desktop (dialog copy) | Relay / desktop code | Channel members / user | ✅ | ✅ | ✅ | **Broken in practice** — see §2.3 |

### 2.2 The attach flow, end to end — where the failure lives

Reconstructed with evidence at each hop, for the exact scenario in the brief
(existing durable session + prior history; user attaches a new provider
execution):

1. **UI promise.** The dialog says "Add a provider to this session … The new
   execution joins this same session and this same channel. It keeps its own
   signed transcript — nothing is merged"
   (`AddCodingSessionProviderDialog.tsx:80-84`), with first-message
   placeholder "What should this provider pick up?" (`:292-302`). The
   transcript above it is rendered from verified relay events
   (`useCodingSessionCatalog.ts:25-39`, `useTrustedCodingSessionIngress.ts:34-38`),
   which makes "the new agent can see this" the natural inference. No copy
   anywhere in `desktop/src` says otherwise (categorical negative, verified
   by grep across the feature).
2. **The signed command carries no history.** The 44221 create payload is
   `{projectRef, repoRef, sessionRef, genesisRef, providerInstanceRef,
   providerAuthorityPubkey, model, title, initialTurn}` — no transcript, no
   context reference (`codingSessionLifecycleCommand.ts:34-45,110-134`).
   `genesisRef` is forwarded only when the umbrella has one
   (`addCodingSessionProviderModel.ts:119-121`), and is null whenever founder
   resolution is ambiguous (`codingSessionUmbrellaModel.ts:340-382`).
3. **Provider decides continuity silently.** `prepare_rehydration_context`
   (`lib.rs:885-978` **[V]** for the guard structure) requires, in order: a
   configured context-MCP sidecar command; an absolute path; a `sessionRef`;
   a `genesisRef`; a reachable relay surface; a successful verified
   projection; a successful 0600 package write. **Each failure logs and
   returns `None` → `SessionContinuity::Fresh`.** The function's own doc
   states this as intent ("best-effort … leaves the execution Fresh",
   `lib.rs:885-892`).
4. **Even success is pull-only (plus a first-turn preamble in WIP).** A
   `Rehydrated` execution gets a private read-only MCP with three tools —
   `session_overview`, `session_history` (page ≤20), `search_session`
   (≤50 hits) (`crates/buzz-dev-mcp/src/lib.rs:154-187`), MCP
   `initialize.instructions` that mandate calling `session_overview` first
   (`:190-199`), and — WIP only — a one-shot first-turn preamble prepended to
   the first user message (`session.rs:51`, injected via `Option::take()` at
   `session.rs:808-814` **[W]**; the durable transcript keeps the original
   text). Nothing pushes history into the model's context.
5. **Disclosure of the outcome:**
   - `Rehydrated` create → one `session_rehydrated` status item
     (`lib.rs:807-815` **[V]**).
   - **`Fresh` create → nothing.** No status item, no receipt error, no
     metadata field (`lib.rs:807-815` **[V]** — the guard is
     `continuity == Rehydrated` only; no `session_fresh` slug exists
     anywhere in the repo).
   - On the *reattach* path the vocabulary is honest at the wire
     (`session_resumed` / `session_loaded` / `session_rehydrated` /
     `session_restarted_without_context`, receipts `resumed` /
     `resumed_without_context` — `lib.rs:1075-1099`), **but** the desktop
     maps `resumed_without_context` to the same lifecycle state as a normal
     `created` (`codingSessionTrustedIngress.ts:614-660` **[V]** — only
     `failed` and `created_with_failed_initial_turn` are special-cased), so
     the user sees "Opening the exact signed session…"
     (`newCodingSessionModel.ts:504`). When a status item *is* rendered, it
     appears as the raw snake_case token under a generic "Status" title
     inside a default-collapsed "Session details" disclosure
     (`codingSessionTranscriptItems.ts:480-489`,
     `codingSessionTranscriptModel.ts:70-80`,
     `CodingSessionTranscript.tsx:66-68,219-228`).
6. **Timeline shows no seam.** A newly attached execution's first block
   simply appears in the interleaved umbrella scroll with a grey provenance
   chip; the "execution attached" moment has no lifecycle row (the generation
   row is emitted only for `index > 0` within an existing execution,
   `codingSessionUmbrellaTimeline.ts:188-207`). Execution labels collide —
   same-provider executions render identical names and the pubkey
   "disambiguator" appends the same provider key to both
   (`codingSessionUmbrellaModel.ts:141-173`,
   `CodingSessionExecutionRail.tsx:280-292`).

**Inference (labeled as such):** the reported failure has two candidate
causes that the system cannot currently distinguish from outside the provider
process — (a) the attach landed `Fresh` (e.g. no `genesisRef` on a
legacy/ambiguous umbrella, or any of the seven silent bail-outs), so the
agent genuinely had nothing; or (b) it landed `Rehydrated` (committed state,
so no first-turn preamble existed yet) and the model answered from its empty
window without calling `session_overview`. Both produce exactly the reported
symptom. That indistinguishability is itself the core defect.

### 2.3 Corrections to standing assumptions

The brief demanded that wrong assumptions be called out directly. Three
standing claims in the project's own documents are wrong or overstated:

1. **"`session/load` cannot carry a relay package" (RUNBOOK, 2026-08-17
   handoff §2.3) — false as stated.** All three ACP session-opening methods
   accept an `mcpServers` list (`crates/buzz-acp/src/acp.rs:811-817`
   (new), `:853-857` (resume), `:881-885` (load)). The restriction is a
   **provider policy choice**: both reattach call sites hard-code
   `Vec::new()` (`session.rs:524,535` **[V]**), and the reattach path
   hard-codes `rehydration_mcp: None` (`lib.rs:1037`). What *is* true: the
   load/resume **replay content** comes from the adapter's machine-local
   store via the opaque cursor — that part is native and non-portable. But a
   relay-durable context channel on the reattach path is one deliberate
   decision away, not a protocol impossibility. Consequence: after a
   provider restart whose native resume fails, the execution lands
   `RestartedWithoutContext` with no rehydration fallback even though a
   verified package could have been built.
2. **"Initial prompt is the only relay-durable seeding channel on every
   adapter" — overstated twice.** Besides the MCP channel above, a
   systemPrompt transport is fully implemented for `session/new`
   (`SystemPromptTransport::{Field, ClaudeMeta}`, `acp.rs:825-831` **[V]**,
   with `ClaudeMeta` explicitly designed to preserve claude-agent-acp's
   native preset) — and passed `None` at both provider call sites
   (`session.rs:512,550` **[V]**). Notably, the WIP's own shipping doc
   mandates systemPrompt as the primary bootstrap transport and first-turn
   text as fallback-only (`docs/P1_CONTINUITY_MATRIX.md:186-200` **[W]**);
   the WIP code implements only the fallback. The code contradicts its own
   governing document.
3. **"History items are deeply redacted" — overclaimed.**
   `coding_session_context.rs:135` documents package item content as
   "deeply redacted"; the projector assigns `content: envelope.item`
   verbatim (`context_projector.rs:1465`). Bounding happens upstream at
   publish time; no redaction happens at projection time.

Confirmed as stated: the 12 KiB turn cap (`MAX_TURN_TEXT_BYTES`,
`coding_session_command.rs:16`, byte-exact, test-pinned `:182-207`); the
cursor's privacy (`state.rs:105-108`, no path to `payload.rs`/`publish.rs`,
redacted from `Debug`, never logged on adapter error `session.rs:526-528`).

### 2.4 What "persistent agent" actually decomposes into (brief Q1)

- **Identity** — genesis event id (canonical; tags are diagnostics only,
  `kind.rs:621-623`), plus goal/name/closure revisions. Fully durable,
  portable, human-owned.
- **Configuration** — provider catalog (44222), model/runtime in 44223
  metadata, machine-local `projects.json` cwd map. Split durable/local.
- **Memory** — *none exists as a distinct concept.* The signed transcript is
  the only durable memory; there is no summarization, no compaction, no
  durable checkpoint anywhere in Buzz (contrast: every comparator compacts,
  §3).
- **Live native session state** — adapter process + machine-local cursor.
  Dies with the machine; correctly never published.
- **Reconstructed context** — the WIP projection/package/MCP path,
  create-only, best-effort, undisclosed on failure.

### 2.5 What is and is not in the transcript (brief Q4/Q5 groundwork)

Captured in 44225 (`transcript.rs`): assistant text (coalesced),
**reasoning** (`include_thoughts` defaults **true**, `config.rs:89,159`),
tool calls with verbatim inputs (8 KiB cap then digest+preview,
`transcript.rs:40,355-372`), terminal tool results (8 KiB cap), plans
(Claude-capability only, `coding_session_payload.rs:275,286`), user prompts,
per-turn results, context-window telemetry. Not captured: file diffs (no
item kind exists), intermediate tool progress (`transcript.rs:313-317`),
unrecognized update kinds (silently dropped, `transcript.rs:135`), and any
repository state beyond the B1 coordinate facts on 44223
(`observedCommit`/`dirty`/`relayReachable`/`verifiedAt`) — which the desktop
parses, validates, and then drops without a single consumer
(`codingSessionIngressPayloads.ts:315-364` vs
`useCodingSessionCatalog.ts:305-330`).

### 2.6 Scale facts (brief's empirical datum extended)

Observed, not designed-for:

- `ReplayTest2`: 8 structured items = 13,646 B (brief datum) → ~1.7 KiB/item.
- Spike dry run 2026-08-17: 42,620 B raw transcript events → 12,618 B
  package items → 8,390 B rendered prompt (~2,094 tokens) for 23 kept items
  (`docs/P1_CONTINUITY_MATRIX.md` §2).
- Growth drivers: items scale with turns × tool calls (2 items per tool
  call), plus reasoning (on by default), plus per-turn metadata republication
  (44223 published at generation start **and every turn end**,
  `coding_session_payload.rs:303-307`). Caps: 8 KiB/tool item, 32 KiB
  envelope, package 4,096 items / 8 MiB
  (`coding_session_context.rs:21-41`).
- **Hard ceiling with a failure cliff:** the projector queries by
  **channel + kind**, not by session (`context_projector.rs:308-313`), with
  a 1,000-row clamp per partition (`:66`). A transcript partition at the
  clamp degrades to `complete: false` and proceeds (`:279-282`); **any other
  kind partition at the clamp aborts projection entirely**
  (`:256-259`) → silently `Fresh`. Because 44223 is republished every turn
  end, a busy channel — or one long session of ~1,000 turns — will cross
  this. Multi-session channels share the budget. This is the first scaling
  wall, and it fails silently into the exact observed symptom.
- Implication for the 12 KiB in-band seed: it holds ~7–8 average items
  verbatim. Beyond a trivial session, any bounded seed is necessarily
  *selective*, which is why the translator's drop policy and honest
  truncation reporting (`translate.py:338-398,607-623`) are load-bearing,
  and why weeks-scale sessions eventually need durable summarization
  (deferred; §7).

### 2.7 Additional invariants the system already imposes (brief §"identify any additional")

- Genesis is undeletable and its `sessionRef` is never released, even via
  soft-delete (`side_effects.rs:244-264`); closures likewise undeletable
  (`:265-270`).
- Seq-before-publish: gaps possible, duplicates impossible
  (`state.rs:14-20`); duplicate transcript seqs are relay-refused by design
  (`kind.rs:609`).
- R21 two-witness authority: never derive authority from projected,
  backfilled, counted, or cached relay history (`authority.rs:19-21`;
  execution plan B.7a).
- Steering vs lifecycle split: turns = founder ∪ granted operators; stop /
  resume / end = owner only (`commands.rs:427-449`); `grant-operator` never
  moves ownership.
- Key rotation invalidates the outbox by design (`publish.rs:12-17`).
- Legacy (no-genesis) sessions can never *gain* operators and genesis-bearing
  records can never fall open when the founder record is absent
  (`commands.rs:428-448`).
- Continuity honesty *at the vocabulary level*: `Rehydrated` is defined as
  "reconstructed context, never Native" (`session.rs:143-159`); there is
  **no `Native` variant in code** — "Native" exists only as a word the model
  is instructed not to claim.

---

## 3. Comparative findings

Full grids in the investigation record; the load-bearing conclusions:

### 3.1 Claude Code (verified by local inspection + primary docs)

- Persists one JSONL per session under `~/.claude/projects/<encoded-cwd>/`,
  self-contained: full API messages (text, thinking, tool_use/result, usage,
  model), envelope metadata (`uuid`/`parentUuid` chains, `cwd`, `gitBranch`,
  CLI version), subagent transcripts and oversized tool results in sidecar
  dirs. UI metadata (`ai-title`, `last-prompt`) lives alongside but separate
  from context-bearing lines.
- **Resume = stateless full replay** of the JSONL into the next API call
  (docs: "reprocesses and re-caches the full history"). No provider-side
  conversation handle. Fork = copy the file under a new id. Model change
  survives resume; provider is Anthropic-only.
- Oversized history: clears old tool outputs first, then summarizes
  (`/compact`, auto-compaction, "resume from summary" after cache expiry).
- Deliberately not restored: `bypassPermissions`/`plan` modes, session-scoped
  permission grants across process forks. Security posture is part of the
  resume contract.
- Cloud sessions: durable server-side conversation + disposable VM, replayed
  into fresh compute on reopen; `--teleport` copies history down (a fork in
  effect, no sync-back).

### 3.2 T3 Code (the brief's Q2, answered from source)

- **Two separate stores.** (a) A complete, self-contained, event-sourced
  SQLite transcript (`~/.t3/userdata/state.sqlite` — threads, full message
  text, all tool/approval/compaction activities, per-turn checkpoints) that
  reproduces the entire *reading* experience on any machine. (b) One
  `provider_session_runtime` row per thread holding a tiny opaque
  `resumeCursor` — which is nothing more than the provider CLI's own session
  id (`{resume: <claude-session-uuid>}`, `{threadId}` for Codex,
  `{sessionId}` for Cursor/Grok/OpenCode)
  (`apps/server/src/persistence/Migrations/004_ProviderSessionRuntime.ts:7-18`).
- **Reopening a weeks-old thread does zero provider work** — it is a SQLite
  read. The provider is re-attached only on the next send:
  `ProviderService.startSession` transparently rehydrates cursor + cwd +
  model from the persisted binding
  (`apps/server/src/provider/Layers/ProviderService.ts:620-659`) and calls
  the provider's native resume (Claude SDK `resume:`, Codex `thread/resume`,
  ACP `session/load`). **T3 never assembles message arrays for a model**; the
  model's memory is rebuilt by the provider CLI from *its own* on-disk
  transcript on *that* machine (`~/.claude/projects/**`,
  `~/.codex/sessions/**`).
- Therefore: display state portable, model state machine-bound. Machine move
  is answered architecturally — move the *client*, not the session; the
  server+machine is the environment.
- Provider switch mid-thread is **blocked** ("bound to driver X"); model
  switch allowed only where the adapter supports in-session switching, else
  the cursor is dropped. Graceful degradation exists (Codex falls back to
  `thread/start` on "no rollout found").
- No compaction/summarization in T3 — it *observes* provider compaction as
  activity rows. No "resumed" framing is injected to model or user; a
  reopened thread just renders as "disconnected" until the next send.

### 3.3 Pi (pi.dev — primary source, local clone)

- One JSONL per session; **sessions are trees** (every entry has
  `id`+`parentId`); in-file branching (`/tree`), forking (`/fork`/`/clone`)
  with `parentSession` lineage.
- **Provider-portable by design**: provider-agnostic message history,
  `model_change` entries recording mid-session cross-provider switches, one
  unified LLM API. The only surveyed system designed for continuation on a
  different provider/model.
- Resume = deterministic replay of the active branch; compactions are
  **self-contained checkpoints** (`retainedTail` materialized, cumulative
  read/modified-file tracking), so resume never needs pre-compaction
  entries. Abandoned branches get optional LLM branch-summaries.

### 3.4 Codex CLI

- Rollout JSONL per session + index; **hybrid continuity**: hot path uses
  provider-native `previous_response_id` over a cached connection; cold
  resume replays the rollout locally. Restores cwd/model/sandbox policy from
  recorded turn context; warns on model drift. Sandbox policy is *restored*
  (opposite security choice from Claude Code).

### 3.5 aider (the clean counterexample)

- Always writes a durable markdown transcript; **restores it into model
  context only on opt-in** (`--restore-chat-history`, default false).
  Cleanest proof that transcript-on-disk and model context are independent
  axes — durable history with zero continuity is a coherent (and default)
  product state.

### 3.6 What the comparison proves

1. **Durable-artifact + stateless replay is the industry's continuity
   backbone.** Provider-native state is only ever a same-machine
   optimization or hot-path accelerator; nobody ports it, everybody has a
   fallback for its absence, and the two failure recoveries observed (Codex
   "no rollout found" → fresh; OpenCode 404 → fresh) both *detect and
   handle* the miss rather than silently pretending.
2. **Every product that supports long sessions compacts.** Buzz has no
   summarization anywhere. At weeks scale this is not optional; at current
   scale it is deferrable (§7).
3. **Reopen ≠ resume ≠ fork ≠ fresh are explicit, user-visible states
   everywhere else** — pickers, warnings on model drift, fork lineage,
   compaction markers. Buzz's equivalents exist at the wire (§2.2.5) but are
   not surfaced.
4. **What is *not* restored is a security decision** (permission modes,
   grants, credentials). Buzz's analogue — never restoring another
   machine's cwd, cursor, or keys — is already correct; it should be stated
   as a product rule, not just an implementation accident.
5. **Buzz's substrate is differentiated.** No comparator has a shared,
   signed, access-controlled, multi-writer transcript. The cost of that
   uniqueness: Buzz cannot simply "replay the local JSONL" — its
   reconstruction must be *verified* projection (which the WIP correctly
   implements) and its context budget is not its own (adapters own the
   window). The comparators validate the architecture Buzz chose; none of
   them had to solve verification, authority, or multi-execution.

---

## 4. Continuity taxonomy

Precise levels that do not overclaim. Each level names what the *model*
actually has — not what the UI shows. (Levels are per-execution; the durable
record is level-independent.)

| Level | Name | Model context source | Exists in Buzz today | Honest label |
|---|---|---|---|---|
| **L0** | **Record** | None — the durable session identity + transcript exist for humans/consumers | ✅ always (verified ingress) | "Session history is stored and visible; no agent holds it" |
| **L1** | **Fresh** | Nothing beyond workspace + initial turn | ✅ `SessionContinuity::Fresh` — but **publishes no marker** | "Fresh — no prior session context" |
| **L2** | **Seeded** | A bounded, one-shot, provenance-marked injection (≤12 KiB in-band; larger via systemPrompt/direct) | Harness only (`scripts/p1-seed-spike/translate.py`); not productized | "Limited context — rebuilt from a bounded history seed" |
| **L3** | **Rehydrated** | On-demand verified history via private read-only MCP (+ orientation bootstrap) | ✅ WIP (create path only; best-effort; silent failure) | "Rehydrated — verified history available on demand" |
| **L4** | **Native re-attach** | Adapter's own machine-local store via opaque cursor (`Resumed` = no replay; `Loaded` = replay) | ✅ (same machine + same adapter only) | "Native — original provider history, this machine only" |
| **L5** | **Native transfer** (cross-machine/provider native continuation) | — | ❌ nowhere, in any surveyed product, as a supported feature | Never promised. Copying provider stores is an experimental safety-bounded technique only (P1 matrix §4.2) |

Rules the taxonomy imposes:

- Levels are **facts, not aspirations**: the level is determined by what was
  actually delivered, after failure handling — never by what was attempted.
- L2–L3 must never be presented as L4 ("Rehydrated, never Native" — already
  the code's own vocabulary, `session.rs:149`).
- Downgrades must be visible: L4-attempt → L1 is `RestartedWithoutContext`
  and must say why; L3-attempt → L1 currently silent — that is the bug.
- L0 is always true for a verified session and is the product's floor
  promise; everything above it is per-execution and disclosed.
- Concurrent executions on one session may sit at different levels
  simultaneously; the level is an execution property, displayed per
  execution (which requires fixing the execution-label collision,
  §2.2.6).

---

## 5. Requirements and threat model

### 5.1 Functional requirements (derived from the vision invariants, rulings, and the failure)

1. **Truthful mode, everywhere, always** — every execution start publishes
   its continuity level as a signed fact (including `Fresh`), the UI renders
   it in human language at the execution and timeline seam, and the attach
   dialog states what the new execution will and will not have *before* the
   user commits. (Vision invariant 8; the observed failure.)
2. **Failure leaves a usable, labeled fresh execution** — already true
   functionally (fallback chain, `session.rs:506-563`), not yet true
   disclosively.
3. **Deterministic orientation floor** — a returning execution can answer
   "what session is this, what is its goal, what is my continuity mode,
   how complete is my history access" without electing to call a tool.
4. **Provenance discipline in reconstructed context** — replayed history is
   evidence, not instructions (already enforced in MCP instructions and
   translator framing, `buzz-dev-mcp/src/lib.rs:190-199`,
   `translate.py:403-418`); attribution-≠-verification (prior work is
   prior).
5. **Honest completeness** — `complete` and `truncated` are independent
   facts (already modeled, `coding_session_context.rs:78-95`) and must
   propagate to the model *and* the user.
6. **Multi-execution correctness** — level is per-execution; rehydration
   packages must scope to the session (currently channel-wide,
   §2.6) and remain correct when several executions write concurrently
   (eventSeq folding already handles interleave).
7. **Non-founder continuation** — a granted operator may steer (turns) but
   not stop/resume (owner-only) — already enforced
   (`commands.rs:427-449`). Rehydration for a non-founder execution must
   verify the same authority chain it grants context under (the projector
   already verifies the chain when a relay identity exists,
   `context_projector.rs:1046-1050` **[W]**). Revocation does not exist yet
   (grants are append-only); until it does, the honest statement is "grants
   are permanent" — not a silent assumption.

### 5.2 Threat model

| Threat | Vector | Current state |
|---|---|---|
| **Secret/path exfiltration via shared history** | Tool inputs/results published verbatim to channel-readable 44225 (8 KiB bound only, no key filtering / path scrubbing / secret detection) — `transcript.rs:344-372,190-217` | **Open.** Contradicts `state.rs:38-41`'s own invariant. The rehydration package inherits it wholesale (`context_projector.rs:1465`). Highest-volume leak surface in the system. Cursor/cwd/keys themselves are correctly protected (§2.1). |
| **Prompt injection via replayed history** | Stale instructions in transcript/tool output executed as fresh commands | Mitigated by framing (MCP instructions, translator untrusted-history header, P1 red-flag scoring); not enforceable — must stay a judged criterion. |
| **Fabricated continuity** | An execution claiming Native/complete context it lacks | Mitigated by vocabulary + bootstrap ("Rehydrated, never Native"); undermined whenever the mode itself is unpublished (Fresh) — a model can't disclose a mode nobody told it. |
| **Authority spoofing** | Fake grants, tag-query projection, unaccepted transitions | Closed (two-witness rule, relay-side atomic enforcement, fail-closed unknown types — §2.1). |
| **Identity hijack** | Claiming a legacy `sessionRef` | Closed (adoption flow, undeletable genesis, R8/R15/R16). |
| **Cross-machine state theft** | Publishing/transporting the native cursor | Closed by design (never published, never in env, never logged). Keep it that way — L5 stays unpromised. |
| **Stale/partial rehydration presented as complete** | 1,000-row clamp; package bounds | Modeled honestly inside the package; **silent at the system level** when the clamp aborts projection (→ silent Fresh). |
| **Revoked member continuing** | Using an old grant after membership change | **Unhandled** — no revoke transition type exists (deferred in the plan); channel-level read ACL is the only gate on history access. Must be named in any sharing promise. |

### 5.3 Product rulings this report cannot make (feed §9)

Tool-input/result scrubbing policy; whether reattach gains a rehydration
fallback (needs the `Vec::new()` policy reversed deliberately); revocation
semantics.

---

## 6. Design space

Three materially different approaches, derived from the evidence rather than
from the in-flight implementation. Each is evaluated against the brief's
dimensions; all three assume the disclosure wiring of §7.1 (which is not a
design choice — it is owed under vision invariant 8 regardless).

### Design A — Record-plus-honesty (continuity as disclosure only)

Ship *no* context machinery. Every execution is Fresh or Native
(same-machine); the product promise is L0/L1/L4: durable record, visible
history, honest labels, and a first-class "catch up by reading the session"
human workflow (open transcript, copy/handoff affordance — the umbrella
"Send to ⟨execution⟩" quote mechanism generalized).

- **Orientation quality:** poorest — the human is the integration layer
  again (exactly the 2025 posture the vision rejects).
- **Fidelity/provenance:** perfect (nothing is transformed).
- **Cost/latency/complexity:** near zero; nothing to verify at scale.
- **Failure modes:** none new. Honest by construction.
- **Verdict:** this is the *floor*, and precisely what the execution plan
  calls "record-only continuity" — the designated fallback if P1's stop
  condition trips. It is not a goal state; it is the state to fall back to
  with dignity.

### Design B — Deterministic push seed (bounded injection at session open)

Productize the translator (execution plan B2): at execution start, the
provider projects the verified package, renders a bounded, provenance-marked
seed, and **pushes** it via the already-built systemPrompt transport
(`acp.rs:825-831`) — falling back to first-turn text only where the adapter
lacks the transport (exactly the P1 matrix's mandated order). The model is
oriented before its first token; no tool-call election involved.

- **Orientation:** good for small/medium sessions; degrades with size — the
  seed is necessarily selective (12 KiB ≈ 7–8 verbatim items; even the
  96 KiB direct budget truncates real sessions).
- **Fidelity:** every seeded item is verified; the drop policy decides what
  the model *never sees* — buried-detail recovery (P1 Q3) is the known weak
  axis, and truncation must be disclosed in the seed header (already done,
  `translate.py:607-623`).
- **Cost:** one-time tokens per execution start (~2k tokens at spike scale);
  no per-question latency. Cheap to reason about, fully inspectable
  (the seed is an artifact).
- **Failure modes:** projection failure → Fresh (must be disclosed);
  over-budget sessions → summarize-or-drop decisions baked at start time,
  wrong for questions asked later.
- **Verdict:** the right *floor above A* — but insufficient alone at
  weeks scale.

### Design C — Verified retrieval (pull model; the WIP's shape)

The context MCP as the primary context carrier: full verified package on
private disk, model pulls overview/pages/search on demand. Scales to the
8 MiB / 4,096-item package bound; per-question retrieval instead of
front-loaded selection.

- **Orientation:** potentially the best for large sessions and buried
  detail — *if the model actually calls the tools*. That compliance is
  today's biggest unverified assumption (§9); the observed failure is
  consistent with a model that didn't.
- **Fidelity:** strongest — items retain full structure, ids, signers,
  seqs; provenance travels with every response.
- **Cost:** per-question latency + retrieval tokens; a package projection
  per execution start (relay queries — currently the clamp-limited,
  channel-scoped fetch, which must be fixed for this design to scale).
- **Failure modes:** silent-Fresh (current); model ignores tools; retrieval
  precision unknown; package staleness during concurrent executions (the
  package is a start-time snapshot — live turns from a sibling execution
  are invisible to it).
- **Verdict:** the right *ceiling mechanism* — but as the *only* channel it
  makes orientation contingent on model behavior, which is what just
  failed.

### Design D — considered and rejected: native-state transport

Sync/copy provider-native stores (cursors, `~/.claude/projects`,
rollouts) across machines to offer L5. Rejected: every surveyed product
treats native state as machine-bound; the stores are opaque,
version-unstable ("internal … changes between versions" — Claude Code's own
docs), credential-adjacent, and unverifiable; and Buzz's own P1 matrix
already classifies copied-store resume as "an experimental safety boundary,
not a supported cross-machine transfer mechanism." The vision explicitly
does not require it ("not … a promise to migrate arbitrary live processes").

### The real decision

A, B, C are not alternatives at the same altitude: A is the fallback, B is a
deterministic floor, C is an on-demand ceiling. The genuine design questions
are (1) whether B, C, or **B+C** clears the P1 bar over a pasted human
summary, and (2) whether B's determinism is worth its truncation. The
continuity matrix (`docs/P1_CONTINUITY_MATRIX.md`) is already the
discriminating experiment for exactly this — it has simply never run.

---

## 7. Recommendation

**The smallest justified design: disclosure now, a deterministic bootstrap
header next, and B+C productization only after the experiment says so.**

### 7.1 Ship regardless of any experiment (small, owed, mostly wiring)

These convert the observed failure into an honest, diagnosable state and are
justified by the evidence alone — no P1 outcome changes them:

1. **Publish `Fresh` (and every mode) as a status fact.** Add a
   `session_fresh` (or `session_started_without_context`) status item on the
   create path — one guard change at `lib.rs:807-815` — carrying a bounded,
   non-sensitive reason (which of the seven bail-outs). Absence of a signal
   must stop being the signal.
2. **Surface `resumed_without_context` in the desktop.** Give it its own
   lifecycle state in `resolveLifecycle`
   (`codingSessionTrustedIngress.ts:614-660`) and human copy; stop mapping
   it to "Opening the exact signed session…".
3. **Un-bury the continuity status items.** Exempt
   `session_rehydrated`/`session_restarted_without_context`/`session_resumed`/
   `session_loaded` from the diagnostics collapse
   (`codingSessionTranscriptModel.ts:70-80`) and render human language, not
   raw slugs.
4. **One honest sentence in the attach dialog** stating what the joining
   execution will have ("This provider starts fresh with verified access to
   the session history" / "starts fresh — this session's history could not
   be prepared"), and an "execution attached" seam row in the umbrella
   timeline (the `index > 0` guard at
   `codingSessionUmbrellaTimeline.ts:197` is where it belongs).
5. **Follow the WIP doc's own transport order**: deliver the Rehydrated
   bootstrap via `SystemPromptTransport` at both `session_new_full` call
   sites (`session.rs:512,550`), first-turn preamble as fallback only, and
   record which transport was used (the P1 matrix requires it). This also
   removes the 12 KiB-contract bypass and the crash-loses-preamble
   fragility (`session.rs:686,808` **[W]**).
6. **Distinguish `Loaded` from `Resumed` at the receipt layer**
   (`lib.rs:1077-1084`) so receipt-only consumers can tell replay from
   no-replay.

### 7.2 The deterministic orientation header (Design B, minimal form)

Regardless of retrieval, every non-Fresh execution should *start* knowing:
session name, goal (latest 44227), continuity level, generation boundaries,
history-item counts with `complete`/`truncated`, and the instruction that
history is evidence. That is a few hundred bytes, deterministic, and rides
the systemPrompt transport from 7.1.5. It removes the "model must elect to
call a tool to learn it has a past" failure mode while keeping depth
on-demand.

### 7.3 Gate the rest on evidence

Run the continuity matrix (§8). If Seed and/or Rehydrated clears G1
(beats the pasted-summary comparator), productize the winning arm(s): B2's
translator in the provider for the seed, and/or hardening of the MCP path.
If neither clears, fall back to Design A with the 7.1 honesty intact — the
plan's own stop condition already prescribes this.

### 7.4 Fix the two structural hazards uncovered (independent of continuity)

- **Session-scoped projection queries.** Replace channel+kind partition
  fetch with session-scoped filters (or pagination) so one busy channel or
  one long session cannot silently abort rehydration
  (`context_projector.rs:251-313`).
- **Decide the tool-input/result publication policy** (product ruling —
  §9.1). At minimum, correct the "deeply redacted" doc claim
  (`coding_session_context.rs:135`) so nobody builds on it.

### 7.5 Explicitly deferred (with reasons)

- **Durable summarization / compaction facts** (relay-signed checkpoint
  summaries à la Pi's self-contained compactions). Needed at weeks scale
  (§2.6, §3.6.2); premature before the matrix shows which arm wins and
  where its truncation actually bites.
- **Rehydration on the reattach path** (reversing the `Vec::new()` policy,
  `session.rs:524,535`, `lib.rs:1037`). Real gap (#4/#5 in the provider
  findings) but touches native-resume semantics; decide after the matrix's
  Native-vs-Rehydrated data exists.
- **Cross-provider seed parity tuning** — G1 is per-adapter by ruling; do
  not generalize.
- **Revocation / takeover transitions** — already deferred by the plan
  until A6 survives contact; continuity design must simply not *assume*
  revocation exists.
- **L5 native transfer** — permanently out of scope absent new evidence
  (Design D).
- **Live cross-execution context** (sibling turns visible to a running
  execution's package) — real multi-execution question; needs R24's
  subagent/activity vocabulary and a staleness design first.

---

## 8. Proof plan

The centerpiece already exists and is endorsed: **run
`docs/P1_CONTINUITY_MATRIX.md` as written** — five arms (Cold / Human
handoff / 12 KiB Seed / Rehydrated MCP / Native ceiling), pre-registered
answer key, frozen worktrees, provider-state isolation with no-write
canaries, blind scoring on the five G1 dimensions, hard red flags
(fabrication, stale-instruction execution, provenance violations), token and
retrieval-precision accounting, per-adapter verdicts. It requires roughly
one hour of Brian's judging time and gates B2/B3/B4. Its verdict grid
(G1 PASS / STOP / Rehydration-viable / Seed-sufficient / Native-only)
maps one-to-one onto §7.3's decision.

Add four focused experiments the matrix does not cover:

- **E1 — Disclosure conformance (fixture-level, no judgment needed).** For
  each of the seven rehydration bail-outs plus resume-rejection, assert: a
  status fact is published, the receipt status is correct, and the desktop
  renders a distinct human-readable state. Acceptance: zero silent-Fresh
  paths remain; `Fresh` vs `Rehydrated-but-unqueried` are distinguishable
  from the relay record alone. (Directly tests the observed failure.)
- **E2 — Bootstrap transport compliance.** Same session, three deliveries
  of the identical bootstrap (systemPrompt / first-turn preamble /
  MCP-instructions-only), per adapter; measure whether the model (a) states
  its continuity mode unprompted, (b) calls `session_overview` before
  answering, (c) never claims Native. Acceptance: the mandated transport
  order (P1 matrix §"Required transport order") is either confirmed or
  revised with data. (Tests the biggest unverified assumption: tool-call
  compliance.)
- **E3 — Scale probe.** Synthesize sessions at 10/100/500/1,000 transcript
  events and a two-session busy channel; measure projection wall time,
  package bytes, clamp behavior, and the exact event count at which
  projection degrades (`complete:false`) or aborts. Acceptance: the failure
  cliff of §2.6 is characterized and the session-scoped-query fix (§7.4)
  verified to remove it.
- **E4 — Non-founder continuation leg.** The A5 proof's unfinished leg plus
  continuity: a granted operator on a second machine attaches an execution,
  receives Rehydrated context, steers a turn; a stop attempt by the grantee
  is refused with the owner-only error. Acceptance: authority and
  continuity compose; the refusal is user-visible.

Fixtures largely exist: `scripts/p1-seed-spike/` (deterministic fixture with
a booby-trapped tool result for the provenance red flag), `acp_probe.py` for
capability evidence, the `p1` CLI wrapper for relay access.

---

## 9. Open questions

**Product rulings required (not derivable from code or evidence):**

1. **Tool-input/result publication policy.** Verbatim-bounded (status quo,
   leaky), key/path-scrubbed, digest-only-with-local-fetch, or
   operator-configurable? This decides what shared history *is* and
   constrains every continuity design downstream (§5.2). Currently the
   system's stated invariant and its behavior disagree.
2. **What the attach dialog promises.** Minimum honest sentence (§7.1.4) vs
   a full continuity preview ("this provider will start Rehydrated /
   Fresh") — the latter requires attempting projection before create, a
   real ordering change.
3. **Reattach rehydration** (reverse `Vec::new()`?) — after the matrix.
4. **Revocation semantics** for granted operators (currently append-only,
   forever).
5. **Naming**: does the product ever show the word "Native"? No such
   variant exists in code; `Resumed`/`Loaded` are the de-facto native modes
   and the receipt layer currently conflates them.

**Design note — agent-to-agent addressing (Brian, 2026-08-17):** staged as
instance-to-instance first, agent-identity later. The wire primitive is
already the execution target (44220 at driver/instance/session/generation —
what `@claude`/`@codex` resolve to); executions are otherwise deaf to each
other. Near-term path: grant a provider pubkey operator standing (the 44228
machinery proven live tonight) so one execution can sign turns at another's
target — gated on (a) operator attribution in `user_prompt` items so
agent-authored turns render as the agent, never "You"; (b) loop/budget
protection (agent↔agent turn ping-pong burns subscriptions); (c) per-session
owner opt-in (R25-adjacent mode decision). Full agent-identity addressing
waits on R24's signed vocabulary and A6 command binding, per plan.

**Facts still unverified:**

6. **Model tool-call compliance** — no evidence yet that any adapter's
   model reliably calls `session_overview` before answering (E2). The
   observed failure may be exactly this.
7. **Rehydrated quality vs Seed vs a human summary** — the entire G1
   question; unmeasured.
8. **Which of the two candidate causes produced the observed failure** —
   the record needed to distinguish them was never published
   (§2.2, inference). E1 makes this class of question answerable
   after the fact; the original incident is unrecoverable.
9. **`resumeSessionAt`-style pinning** — whether adapters honor a
   caller-supplied cwd on load/resume well enough for the matrix's Native
   arm on a disposable worktree (matrix §5 already hedges this;
   T3's Claude adapter deliberately resumes at transcript tail, not a
   pinned message — a caution for any pinning ambition).
10. **Real growth curve** — §2.6's per-item averages come from two small
    sessions; E3 supplies the curve. Whether 44223's per-turn
    republication is the first kind to cross the 1,000-row clamp in
    practice is measurable there.

---

## Addendum A — Relay-durable context checkpoints (Pi-derived design)

*Added 2026-08-17 after direct source inspection of
`/Users/brian/Projects/pi/packages/coding-agent/src/core/compaction/compaction.ts`
and `docs/session-format.md`, in answer to: "can't we just do what Pi is
doing and send it to the relay as a 'sum' object with a ref to a signed
session event?"*

**Short answer: yes — Pi's compaction entry is almost exactly
relay-event-shaped, and publishing it as a signed kind fixes three of this
report's findings at once.** This is the concrete form of the "durable
summarization" item deferred in §7.5, pulled forward with a design.

### A.1 What Pi actually stores (verified from source)

A compaction is an ordinary append-only entry in the session log
(`session-manager.ts:69-80`):

```jsonc
{ "type": "compaction", "id": "…", "parentId": "…",
  "summary": "…structured markdown…",
  "firstKeptEntryId": "c3d4e5f6",      // pointer form, or:
  "retainedTail": [ …messages… ],       // newer self-contained form
  "tokensBefore": 50000,
  "details": { "readFiles": [...], "modifiedFiles": [...] },
  "usage": { … } }
```

Resume-time consumption (`buildContextEntries`,
`session-manager.ts:418-454`): walk leaf→root, take the **latest**
compaction, emit it as one `compactionSummary` message, then only the kept
entries from `firstKeptEntryId` (or the embedded `retainedTail`) forward.
Everything older is omitted from context but **never deleted from the log**.
Key mechanics worth copying:

- **Cut at turn boundaries, never at tool results**
  (`findCutPoint`, `compaction.ts:403-461`); split-turn handling produces a
  merged history+turn-prefix summary (`compaction.ts:861-900`).
- **Iterative update**: each compaction feeds the *previous summary* into
  the summarization prompt with preserve/add/update rules
  (`UPDATE_SUMMARIZATION_PROMPT`, `compaction.ts:500-537`), so knowledge
  accumulates rather than eroding.
- **Cumulative file tracking**: `readFiles`/`modifiedFiles` merged across
  compaction generations (`extractFileOperations`, `compaction.ts:42-70`).
- **The summary template** (`compaction.ts:467-498`): Goal / Constraints &
  Preferences / Progress (Done · In Progress · Blocked) / Key Decisions /
  Next Steps / Critical Context, with "preserve exact file paths, function
  names, and error messages." **This template is nearly an answer key for
  `docs/P1_JUDGE_SCRIPT.md`'s five questions** (orientation, done/open,
  next step, buried detail, attribution) — independent convergence worth
  noticing.
- Summarization calls disable cache writes and use fresh routing ids
  (`completeSummarization`, `compaction.ts:563-583`).

### A.2 The Buzz mapping — a checkpoint kind

New provider-signed kind (next free: **44231**,
`KIND_CODING_SESSION_CONTEXT_CHECKPOINT`), following the 44223/44225
envelope discipline (`h` tag, target tags, 32 KiB bound, agent-fence trust):

```jsonc
{ "schema": "coding-session-checkpoint/v1",
  "session": { driver, instanceId, sessionId, generation },
  "sessionRef": "…", "genesisRef": "…",
  "coverage": { "fromSeq": 1, "throughSeq": 214,
                "eventCount": 214, "eventIdsDigest": "sha256:…" },
  "prevCheckpointRef": "<event id of prior 44231 or null>",
  "summary": "…Pi-style structured markdown…",
  "readFiles": ["repo-relative/only.rs"], "modifiedFiles": [...],
  "summarizer": { "runtime": "claude-code", "model": "…" },
  "tokensBefore": 50000 }
```

Buzz should prefer Pi's **pointer form over `retainedTail`**: Pi embeds the
tail because walking its local file is the only alternative; in Buzz the
tail already exists as signed 44225 events, and duplicating their content
inside the checkpoint would bloat it and create a second,
signature-ambiguous copy of history. The Buzz-native equivalent of
`firstKeptEntryId` is `coverage.throughSeq` — "everything ≤ N is
summarized; fetch events > N as the live tail." The `eventIdsDigest` lets
any consumer audit exactly which signed events the summary claims to cover.

Consumer folding: latest checkpoint per target by
(`coverage.throughSeq`, `created_at`, event id) — append-only revisions,
same discipline as goal/name (`kind.rs:632-633`).

### A.3 What it fixes

1. **The 1,000-row cliff (§2.6).** The projector's transcript fetch becomes
   "latest 44231 + events with `eventSeq > throughSeq`" — bounded forever,
   regardless of session age. (The non-transcript partitions still need the
   session-scoped-query fix of §7.4.)
2. **The 12 KiB seed.** The seed becomes deterministic: checkpoint summary
   + rendered tail, instead of a lossy drop-policy over raw history. The
   translator's hardest job (deciding what the model never sees)
   is replaced by an iteratively-maintained summary that was written *by
   the execution that had full context at the time* — structurally better
   than any after-the-fact selection.
3. **Weeks-scale sessions (§3.6.2).** This is the missing compaction
   answer, and unlike every comparator's, it is **shared and portable**: a
   new execution on another machine, another provider, or another
   authorized member's client gets the checkpoint from the relay like any
   other signed fact.
4. **The intelligible record on provider death** (vision §Continuity): a
   checkpoint published at stop/close is exactly "a provider becoming
   unavailable must leave an intelligible record."

### A.4 Where Buzz must deviate from Pi

- **A summary is derived content, not fact.** Pi trusts its own file; Buzz
  consumers must not. The checkpoint carries provenance
  (`summarizer`, `coverage`, `prevCheckpointRef`, digest) and is folded
  under the existing rule that provider facts establish provenance, not
  truth. It **accelerates** access to history; it never replaces it — the
  signed 44225 record remains the attributable corpus (R19, invariant 5),
  and rehydration/MCP search still runs over real events.
- **Injection surface.** The summary is distilled from untrusted tool
  output; it must ride the same evidence-not-instructions framing as
  replayed history (already in the MCP instructions and translator header).
- **Secrets.** The summarization prompt must require repo-relative paths
  and forbid credentials/env values — done right, checkpoints *reduce* the
  §5.2 leak surface relative to verbatim tool results (which remain an
  independent open ruling).
- **Multi-execution.** Start per-execution-generation (matching the
  transcript's target scoping). An umbrella-level checkpoint spanning
  executions raises authority and attribution questions (who signs a
  summary of someone else's execution?) — defer, same as live
  cross-execution context (§7.5).
- **Who runs the summarization inference — resolved by the subscription
  constraint; see A.6.**

### A.6 Self-checkpointing: the summarizer is the live session itself

*Resolved 2026-08-17 under the operating constraint that all provider
access is **subscription-authenticated through the adapter CLIs** — there
is no API key for a side-channel summarization call. That eliminates Pi's
own-API shape and any relay-side service outright. The only inference the
system possesses is the live ACP session — and it turns out to be the
ideal summarizer, not a compromise.*

**Why the live session is the right summarizer, not just the only one:**

1. **The input is free.** The live agent already holds the entire
   conversation in its native context. Pi must serialize its log into a
   summarization request and pay full input tokens every time
   (`compaction.ts:653-663`); a checkpoint turn sent to the live session
   costs mostly cache-read input plus a template-sized output.
2. **The provenance is the strongest available under R21.** The checkpoint
   is authored by the execution that locally witnessed the history it
   summarizes — self-attested by the same signing provider that published
   the underlying 44225 events. No other candidate summarizer (ephemeral
   session, relay service) has first-hand context.
3. **It is the same pattern Claude Code itself uses** — `/compact` runs
   inside the session (§3.1) — so adapters are already exercised this way.

**Mechanism — a provider-initiated checkpoint turn:**

- **Delivery.** A new low-priority `SessionCommand::Checkpoint` variant on
  the existing actor mailbox (`session.rs:176`, queue drain at
  `:707-720` **[V]**). It never preempts operator work: it is dispatched
  only when the actor is idle (queue empty), and an operator turn arriving
  while a checkpoint turn is in flight interrupts and discards it —
  checkpoints are best-effort accelerators, losing one costs nothing.
- **Triggers.** (a) Actor-idle after a completed turn when thresholds
  cross — items/bytes covered since the last checkpoint, or context
  fraction from the session's own `context_window_updated` telemetry
  (`transcript.rs:50-56`); (b) once in the idle-shutdown arm
  (`session.rs:720-729` **[V]**) before the actor exits — the **closing
  checkpoint**, published at exactly the moment the execution's native
  context is about to become unreachable. This is the vision's "a provider
  becoming unavailable must leave an intelligible record," implemented.
- **Prompt.** The A.1 template + the previous checkpoint's summary
  (Pi's iterative preserve/add/update rules, `compaction.ts:500-537`) +
  the A.4 rules (repo-relative paths, no credentials, per-decision
  attribution). Trigger *below* the adapter's own auto-compact threshold,
  so the checkpoint captures early detail before native compaction can
  erase it.
- **Harvest.** Parse the structured summary from the turn's assistant
  text; validate the template; publish 44231 with
  `coverage = (lastCheckpoint.throughSeq, currentSeq]` and
  `prevCheckpointRef`. On parse failure, retry once, else skip — never
  block the session on its own bookkeeping.
- **Transcript visibility.** Publish the checkpoint exchange as a visible,
  attributable row (its own item kind or the status lane, `turn: None` as
  used at `lib.rs:807-815`) rather than hiding it: the checkpoint is
  itself part of the record, and operators should see their subscription
  being spent.
- **Cost envelope.** One bounded turn per threshold crossing, on the same
  subscription that runs the session; input ≈ cache-read, output ≈
  template size. No new execution slot (it rides the existing actor), no
  new process, no new credentials.

**The dead-execution bootstrap.** An execution that dies before its first
checkpoint leaves only raw history — and the next execution to hold
context continues the chain: a Rehydrated execution, after orienting from
checkpoint+tail (or the raw package), emits a catch-up checkpoint as its
own first idle act. Checkpoint authorship thus follows context custody:
whoever currently holds the session's context maintains its summary. If no
execution ever returns, the record-only floor (§Design A) is unchanged.

### A.5 Revised path forward

Unchanged: the §7.1 disclosure wiring ships first — checkpoints make
continuity *better*, disclosure makes it *honest*, and the observed failure
was a failure of honesty. Then:

1. Adopt Pi's summary template (adapted: add an explicit **Attribution**
   line per key decision — P1 Q4 — and repo-relative path rules) as the
   checkpoint format.
2. Run the P1 matrix with the Seed arm defined as **checkpoint + tail**
   (generate the checkpoint via the A.6 self-checkpoint turn on the chosen
   real session). The Human-handoff comparator is literally a hand-written
   checkpoint, so G1 collapses to the cleanest possible question: *does
   the machine-written checkpoint match the human-written one?*
3. If G1 passes: 44231 lands as the B2 productization (new kind per the
   §B.1 checklist; projector consumes it per A.3.1; seed and MCP overview
   render it first).
4. If G1 fails on summary quality: iterate the template once (the matrix
   allows one policy revision), then fall back per the plan's stop
   condition.

---

## Addendum B — Agentless session sync (the Entire.io pattern)

*Added 2026-08-17 in answer to: "why can't there just be a session sync with
no agent involvement — the session gets sent, almost as if it was local
storage? How is entire.io doing it?" Researched from Entire's public docs
and its open-source CLI (`github.com/entireio/cli`, Go).*

### B.1 What Entire actually does (verified from primary sources)

Entire is agent-session observability made git-native, founded by GitHub's
ex-CEO. The mechanics:

- **Capture:** the CLI installs *agent lifecycle hooks* (session start,
  prompt submitted, turn end) plus git hooks. On commit, it captures the
  agent's session — "transcripts, prompts, files touched, token usage, tool
  calls" — as a **checkpoint** stored on a shadow branch
  `entire/checkpoints/v1`, never touching the working branch. A 12-char
  checkpoint ID rides the commit message as a trailer, linking code to
  session. Supported agents: Claude Code, Codex, Copilot CLI, Cursor,
  Factory Droid, Gemini CLI, OpenCode, **and Pi**.
- **Sync:** the shadow branch pushes/pulls with the repo. The session
  record travels wherever the repo goes — pure data transport, zero
  inference, zero agent involvement.
- **Resume — the decisive part** (from `cmd/entire/cli/resume.go`):
  `entire session resume` "checks out its branch, **restores its checkpoint
  session log**, and asks whether to start the agent … Restores the session
  log if it doesn't exist locally (an existing local log wins by default)."
  I.e., it writes the agent's **native session file** (e.g. the Claude Code
  JSONL under `~/.claude/projects/<slug>/`, per
  `cmd/entire/cli/agent/claudecode/` and `paths/paths.go`) back onto the
  machine from the git checkpoint, then launches or prints the agent's own
  resume command (`ResumeCommandSpecFor`, `agent/resume_command.go`). The
  agent replays its restored file exactly as if it had never left.
- They also ship a `redact/` package (PII redaction over transcripts/paths)
  — because their checkpoint branch is *shared*, sanitization is
  load-bearing for them.

**Why this works at all:** for these CLIs, "native context" is not
provider-side magic — it *is* a local file that gets statelessly replayed
on resume (§3.1). Syncing the file therefore **is** syncing the model's
memory. Entire's insight is that no summarization, no protocol, and no
agent cooperation are needed for continuity — only faithful transport of
the native store plus invocation of the agent's own replay.

### B.2 What this corrects in the report

Design D (§6) rejected "native-state transport" wholesale. That was too
broad. What stays rejected is **live-process migration** and **shared,
plaintext** native-store publication. What Entire proves shippable — and
what Buzz should adopt — is **encrypted native-store snapshot transport**:
same-adapter, cross-machine, full-fidelity native resume with no inference
anywhere in the loop. The taxonomy gains a level between L4 and the
unpromised L5:

> **L4t — Native, transported store:** the adapter's own session file,
> snapshotted from the origin machine and restored on the target machine,
> replayed by the adapter's native resume. Full fidelity; same adapter
> only; disclosed as "Native — restored from a synced snapshot of the
> original session."

### B.3 The Buzz mapping — snapshot blob + pointer event

Buzz has a better transport for this than a git shadow branch: it already
runs a media store and a signed-event fabric.

1. **Snapshot.** At the same hooks the A.6 checkpoint uses (turn end
   debounced, execution stop, idle shutdown), the provider copies the
   adapter's native session file. The provider already holds the two keys
   needed to locate it: the ACP session id (`resume_cursor`,
   `state.rs:105-108`) and the cwd (`state.rs:38-41`); the Claude path is
   `~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl` — the same
   derivation T3's `UsageService.ts:186-225` and Entire's importer both
   perform. Codex rollouts analogously.
2. **Encrypt + upload.** Encrypt to the founder's key (the R28 pattern:
   personal, encrypted-to-self, never destructive) — the native log
   contains *everything verbatim*, including host paths and any secrets
   that crossed the session, so it must never be relay-readable plaintext.
   Upload as a Blossom blob (`buzz-media`).
3. **Pointer event.** A small signed kind (next free after 44231: 44232,
   `KIND_CODING_SESSION_NATIVE_SNAPSHOT`): target, adapter name + exact
   version, adapter-native session id, blob hash + size, snapshot
   timestamp, covered-through `eventSeq` (linking it to the shared
   transcript's clock). Latest-wins per target.
4. **Restore.** A provider on another machine (same operator key, or the
   founder from another install) resolving a create/resume for that
   session: fetch pointer → fetch + decrypt blob → write into the *local*
   adapter store under the **locally re-encoded cwd slug** (this is how the
   absolute-path keying is defused: the restoring side re-derives the slug
   from its own checkout path) → run the existing resume→load→new fallback
   chain (`session.rs:506-563`), which now succeeds natively. Continuity =
   `Resumed`/`Loaded`, disclosed as L4t. Every failure falls through the
   already-built chain into Rehydrated/Fresh — with A.6/§7.1 disclosure.

No inference. No agent involvement in the sync itself. The subscription is
not consumed. This is the direct answer to "almost as if it was local
storage": it *is* local storage, escorted.

### B.4 Honest constraints (why this is a lane, not the whole answer)

- **Same adapter only.** A Claude log restores into Claude; it does
  nothing for Codex, another provider, or an authorized teammate on a
  different stack. Cross-provider and shared continuation still ride the
  checkpoint + verified-history path (A.2/A.6).
- **Format fragility.** Claude Code's docs state the transcript format "is
  internal … and changes between versions." Mitigations: record the exact
  adapter version in the pointer; treat restore as best-effort; never let
  a failed restore block the fallback chain. Entire carries the same risk
  and ships anyway (its per-agent `AGENT.md` compatibility matrices are
  the maintenance cost, visible in their repo).
- **Privacy is structural, not optional.** Entire needed a redaction
  package because its artifact is shared. Buzz's encrypted-to-founder blob
  avoids redaction for transport — but that also means the snapshot is
  *not* the shared record and must never become it. Sharing snapshots with
  granted operators is a product ruling (it hands them the founder's
  machine-verbatim history), deferred.
- **Size.** Native logs grow large (a 5.6 MB session file was observed
  locally, §3.1); debounce + compress; Blossom is built for blobs.
- **Not verifiable.** The snapshot is opaque third-party data; Buzz signs
  the *pointer* (provenance of transport), not the content. It can never
  substitute for the signed transcript as the attributable record.

### B.5 The complete layered picture

The three lanes are complementary, not competing — each answers a
different clause of the brief's return cases:

| Lane | Inference needed | Serves | Fidelity |
|---|---|---|---|
| Signed transcript + disclosure (§7.1) | none | everyone, always — the shared record | exact, verified |
| **Native snapshot sync (B.3)** | **none** | same operator + same adapter, any machine | full native |
| Checkpoint 44231 (A.2/A.6) | one turn on the subscription | cross-provider, cross-model, teammates, weeks-scale seeds | distilled, provenance-marked |

The answer to "why can't sync be agentless" is therefore: **it can, and
for two of the three lanes it already is.** Only *distillation* requires
inference — and with the snapshot lane in place, the checkpoint earns its
keep precisely where distillation is genuinely irreplaceable: crossing
providers, crossing people, and keeping the orientation seed small.

Sources: [entireio/cli](https://github.com/entireio/cli) (README, `resume.go`, `resume_continue.go`, `checkpoint_resume.go`, `agent/`, `redact/` via `gh search code`), [docs.entire.io — Resume Sessions](https://docs.entire.io/guides/sessions/resume-sessions.md), [docs.entire.io index](https://docs.entire.io/llms.txt), [Entire blog — Agent Hooks](https://entire.io/blog/agent-hooks-the-integration-layer-between-entire-cli-and-your-agent), [OSTechNix coverage](https://ostechnix.com/entire-cli-git-observability-ai-agents/).

---

## Addendum C — MCP spec audit (2025-11-25 and 2026-07-28) against this design

*Added 2026-08-17 after Brian asked whether recent MCP revisions add
functionality this design should be using. Both revisions post-date the
research model's training; everything below is from the primary changelogs:
[2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/changelog),
[2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/changelog),
[extensions overview](https://modelcontextprotocol.io/docs/extensions/overview).*

### C.1 Findings that change or confirm decisions in this report

1. **Sampling is deprecated (2026-07-28) — the alternate checkpoint
   summarizer is gone.** MCP sampling was the one protocol-native way the
   context sidecar could have requested subscription-billed inference from
   the adapter's model (it even gained `tools`/`toolChoice` in 2025-11-25
   before being deprecated with "integrate directly with LLM provider APIs
   instead"). A.6's in-session checkpoint turn is therefore the right design
   by elimination as well as by merit. **Decision: A.6 stands; do not build
   on sampling.**
2. **The `initialize` handshake no longer exists in 2026-07-28** (stateless
   core; per-request `_meta`; `server/discover`). The sidecar's original
   bootstrap channel — `initialize.instructions` — is thus deprecated by
   architecture, not just insufficient by observation (§2.2.4, the Codex
   tool-ignoring failure). **Decision: the ACP systemPrompt transport ruling
   (P1 matrix "Required transport order") is structurally vindicated; treat
   `initialize.instructions` as legacy-only.**
3. **Server-minted handles are now official doctrine** (SEP-2567: cross-call
   state travels as explicit handles in ordinary tool arguments). The
   sidecar's stable-event-reference pagination already conforms. No change.

### C.2 Adoptable when the adapters speak 2026-07-28 (not before)

- `ttlMs`/`cacheScope` on reads + deterministic tool ordering (explicitly
  motivated by LLM prompt-cache hit rates): the per-execution package is
  immutable → `cacheScope: "private"`, long TTL. Free win at upgrade time.
- `server/discover` as the stdio back-compat probe when claude-agent-acp /
  codex-acp move revisions.
- **Act-on item:** when the context surface is formalized, register it as a
  named extension with a reversed-domain vendor prefix (e.g.
  `org.agiterra.buzz/session-context`) advertised through the extensions
  capability field, rather than three loose tools.

### C.3 Watch items (no action now)

- **Tasks extension** (`io.modelcontextprotocol/tasks`: durable handles,
  polling, mid-flight input) — the shape to reach for if `search_session`
  or projection-on-demand ever becomes slow at weeks scale.
- **URL-mode elicitation** (2025-11-25) — a plausible future transport for
  consent/approval ceremonies (bounce to a `beekeeper://` approval surface);
  unrelated to continuity today.
- **`experimental-ext-interceptors`** — name suggests middleware hooks;
  if it matures, interception is a candidate home for transcript
  redaction (§9.1's open ruling). Name-only sighting; verify before use.
- **Resources + `subscriptions/listen`** — not new, but the principled fix
  for package staleness (§6 Design C failure mode: sibling executions'
  turns invisible after snapshot): expose the transcript as subscribable
  resources once adapters support the redesigned stream. Queued behind the
  same evidence gate as rehydrate-on-reattach.

---

*Investigation record: four read-only research passes (Buzz
provider/protocol; Buzz desktop; T3 Code at
`/Users/brian/Projects/t3code/t3code`; comparative products via local
inspection of `~/.claude/projects`, the local Pi clone, and primary docs for
Claude Code / Codex / aider), 2026-08-17. Direct re-verification by the
report author covered: `session.rs:506-563` (fallback chain, `Vec::new()`
call sites, `None` systemPrompt), `lib.rs:800-820` (Fresh-create silence),
`acp.rs:805-835` (SystemPromptTransport), 
`codingSessionTrustedIngress.ts:596-660` (`resumed_without_context`
fall-through), `MAX_TURN_TEXT_BYTES`, and the governing docs
(`SESSION_VISION.md`, `SESSION_EXECUTION_PLAN.md` §B.7a rulings,
`P1_JUDGE_SCRIPT.md`, `P1_CONTINUITY_MATRIX.md`, `p1-seed-spike/RUNBOOK.md`).*
