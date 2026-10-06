# How t3code normalizes multiple agent providers — and what Beekeeper can take from it

Read-only study, 2026-08-20. t3code at `/Users/brian/Projects/t3code/t3code`, Beekeeper at
`/Users/brian/Projects/buzz`. Every claim is `file:line`. Time-boxed: depth on the
load-bearing seam, not exhaustive coverage.

---

## 1. The canonical model

**One schema-validated event union, declared in the shared contracts package:**
`packages/contracts/src/providerRuntime.ts` (1214 lines).

- `ProviderRuntimeEvent` = `ProviderRuntimeEventV2` — a 48-member tagged union
  (`providerRuntime.ts:1139-1193`). Every adapter emits *this*, nothing else.
- Common envelope `ProviderRuntimeEventBase` (`:250-265`): `eventId`, `provider`,
  `providerInstanceId`, `threadId`, `createdAt`, `turnId?`, `itemId?`, `requestId?`,
  `providerRefs?` (provider's own ids, `:45-50`), and **`raw?`** (`:263`) — the
  original provider frame, tagged with its source (`:21-31`:
  `claude.sdk.message`, `codex.eventmsg`, `acp.jsonrpc`, …).
- Closed vocabularies: `RuntimeSessionState` (`:52-60`), `RuntimeThreadState` (`:62-70`),
  `RuntimeTurnState` (`:72`), `RuntimeItemStatus` (`:78`), `RuntimeErrorClass` (`:95-102`),
  `CanonicalItemType` (`:121-133`), `CanonicalRequestType` (`:135-146`), `RuntimeTaskStatus` (`:596-606`).
- Payloads that matter: `ItemLifecyclePayload` (`:406-420`) — `{itemType, status?, title?,
  detail?, data?, agentId?, parentToolUseId?}` — and `ThreadTokenUsageSnapshot` (`:309-326`),
  whose `used*`/`last*` pairs hold cumulative and per-turn figures without clobbering.

**It is both a stream and derived state, in four layers**, which matters for the Beekeeper question:

1. `ProviderRuntimeEvent` — in-memory, one PubSub (`apps/server/src/provider/Layers/ProviderService.ts:233`, `:285-294`, `:1194-1195`).
2. `OrchestrationEvent` — **durable, append-only** SQLite (`apps/server/src/persistence/Services/OrchestrationEventStore.ts:22-40`), written by `apps/server/src/orchestration/Layers/ProviderRuntimeIngestion.ts`.
3. `OrchestrationThread` projection via a pure reducer (`packages/client-runtime/src/state/threadReducer.ts:65`).
4. A client fold into the Agents panel (`packages/client-runtime/src/state/subagentRuntime.ts`, `RuntimeSubagent` `:59-88`).

**Correction to the naive framing:** t3code is *not* freely renormalizable. Its durable
store holds the already-normalized `OrchestrationEvent`. The raw provider stream survives
only in rotating NDJSON logs — 14-day age cap, 512 MiB total
(`apps/server/src/provider/Layers/EventNdjsonLogger.ts:30-31`), which deliberately keep
`"native" | "canonical" | "orchestration"` as three separate views (`:49`). So t3code can
re-derive projections cheaply, but re-deriving *the normalization itself* is bounded by a
14-day raw window. Bear this in mind for §8 — the gap versus Beekeeper is smaller than it looks.

---

## 2. The adapter seam

**Formal.** `ProviderAdapterShape<TError>` — `apps/server/src/provider/Services/ProviderAdapter.ts:45-126`.
Thirteen members (`startSession`, `sendTurn`, `interruptTurn`, `respondToRequest`,
`stopSession`, `readThread`, `rollbackThread`, …) plus the one that carries everything:

```ts
/** Canonical runtime event stream emitted by this adapter. */
readonly streamEvents: Stream.Stream<ProviderRuntimeEvent>;   // ProviderAdapter.ts:122-125
```

Five adapters in `apps/server/src/provider/Layers/`: `ClaudeAdapter.ts` (4644),
`CodexAdapter.ts` (2001), `OpenCodeAdapter.ts` (1739), `GrokAdapter.ts` (1470),
`CursorAdapter.ts` (1188). Per-provider aliases are naming anchors only
(`Services/ClaudeAdapter.ts:19`). Registration is a static array (`builtInDrivers.ts:47-53`);
lookup is by instance id (`Services/ProviderAdapterRegistry.ts:53-55`).

**Cursor and Grok do not have hand-written mappers** — they share an ACP layer,
`apps/server/src/provider/acp/AcpCoreRuntimeEvents.ts` and `AcpRuntimeModel.ts`.
This is the part Beekeeper should read first (§7).

Fan-in enforces the seam at runtime: `correlateRuntimeEventWithInstance`
(`ProviderService.ts:194-212`) **throws** if an adapter emits an event whose `provider`
mismatches the instance that produced it.

---

## 3. Where normalization happens: split across four layers, mostly deliberate

| Concern | Where | Cite |
|---|---|---|
| provider frame → canonical event | **adapter** | `ClaudeAdapter.ts:2648-2670`, `CodexAdapter.ts:486-496` |
| lifecycle vocabulary mapping | **adapter** | "killed→cancelled and paused→idle are mapped at the adapter so the wire only carries the shared vocabulary" — `providerRuntime.ts:622-626` |
| human-readable title/detail | **adapter** | `titleForTool` `ClaudeAdapter.ts:1177-1196`; `summarizeToolRequest` `:1149-1175` |
| agent-vs-background classification | **ingest (server)** | `classifyTaskAgentKind` `providerRuntime.ts:527-538`, stamped once, "Clients trust this stamp outright" `:551-554` |
| activity-row identity / upsert keys | **ingest** | `ProviderRuntimeIngestion.ts:570-577`, `:595`, `:626` |
| cumulative-usage reconciliation | **client fold** | `subagentRuntime.ts:182-226` |
| model slug aliasing | **shared, called by client** | `normalizeModelSlug` `packages/shared/src/model.ts:235-249` |
| model chip text | **render** | `formatSubagentModelLabel` `subagentRuntime.ts:918-930` |

The split is **stated as policy**, not accidental — `threadReducer.ts:56-64`:
"This is a pure reducer operating on contract types. UI-specific mapping (e.g. resolving
attachment preview URLs, normalising model slugs, adding scoped fields like
`environmentId`) is the caller's responsibility."

**Accreted at the edges.** Four overlapping status vocabularies coexist: `RuntimeItemStatus`
(4, `providerRuntime.ts:78`), `RuntimeTurnState` (4, `:72`), `RuntimeTaskStatus` (8, `:596-606`),
and a hand-mirrored client copy `RuntimeSubagentStatus` (8, `subagentRuntime.ts:22-30`) that is
*not imported* from contracts. `subagentRuntime.ts:1-18` calls itself "deliberately
legacy-bridge code" awaiting an orchestration-v2 projection.

---

## 4. Five concrete divergences

**(a) Tool-call classification — Claude guesses from the name, ACP reads a discriminant.**
Claude's SDK gives a tool name and JSON input but no category, so t3code string-matches:
`classifyToolItemType` (`ClaudeAdapter.ts:697-739`) tests `includes("bash") ||
includes("command") || includes("shell") || includes("terminal")` → `command_execution`,
defaulting to `dynamic_tool_call`. Codex does the same over a de-camel-cased type string
(`CodexAdapter.ts:209-218`, `:220-239`). ACP providers do **not** guess — ACP carries
`toolCall.kind`, mapped by a closed switch (`AcpRuntimeModel.ts:292-306`).

**(b) Tool-call *payload* — genuinely not normalized (see §6).** Three adapters write
three different `data` blobs and nothing reads them.

**(c) Usage: cumulative-vs-cumulative, reconciled by field-wise max.**
Both providers send cumulative restatements, so the fold takes a field-wise maximum
(`mergeUsageMax`, `subagentRuntime.ts:182-226`; `pick` at `:202-203`). Summing would
double-count on duplicate delivery, late frames, and the terminal row that restates the
final total (`CodexAdapter.ts:670-671`: "Cumulative per child thread: always the `total`
breakdown, never `last` (which shrinks on follow-ups). Client folds max-merge.").
Field-wise, not whole-record, because "a terminal payload carrying only totalTokens must
not wipe a known breakdown" (`subagentRuntime.ts:189-190`) — at the cost of a chimera where
the breakdown no longer sums to the total (`subagentRuntime.test.ts:217-231`: `600+150 ≠ 1000`).
Ragged coverage is summed unlabeled: Claude workflow members give `totalTokens`+`toolUses`
(`ClaudeAdapter.ts:3044-3051`), Codex the breakdown but no `toolUses` (`CodexAdapter.ts:688-702`),
and the rollup adds them anyway (`subagentRuntime.ts:838`). **Stale contract comment:**
`providerRuntime.ts:474-475` claims Claude sends per-activation deltas and the merge is
"provider-specific" — both false; `mergeUsageMax` never receives a provider.

**(d) Model identity — one alias table, applied per provider.**
`MODEL_SLUG_ALIASES_BY_PROVIDER` (`packages/contracts/src/model.ts:168-215`) folds `opus`,
`opus-5`, `claude-opus-5.0`, `claude-opus-5-0` → `claude-opus-5`, and unifies *across*
providers: Cursor's `opus-4.6` → `claude-opus-4-6` (`:207-208`). Applied by
`normalizeModelSlug` (`packages/shared/src/model.ts:235-249`); `opus-5[1m]` itself is
produced at render by `formatSubagentModelLabel` (`subagentRuntime.ts:918-930`) — strip
`claude-`, strip a trailing `-YYYYMMDD`/`-latest`, append `· effort`.

**(e) Interruption vs failure.** Separate at turn level (`RuntimeTurnState`, `:72`) and task
level (`RuntimeTaskStatus`, `:596-606`); coarser at item level, where `RuntimeItemStatus`
(`:78`) folds approval-refusal into `declined`. Claude's `task_updated` patch
(`ClaudeAdapter.ts:3278-3280`: "main previously dropped this on the floor, losing all
transitions") is mapped to the shared vocabulary at the adapter.

---

## 5. Honesty properties

**Preserved.**
- `raw?` on every canonical event (`providerRuntime.ts:263`), source-tagged (`:21-31`).
  ACP events carry the whole JSON-RPC frame (`AcpCoreRuntimeEvents.ts:186-190`).
- **Dual carriage of usage**: `usage: Schema.Unknown` (the provider's raw claim) sits
  beside `typedUsage: RuntimeTaskUsage` (the normalized reading) on the same payload —
  `providerRuntime.ts:612-613` and `:642-643`, populated together at `ClaudeAdapter.ts:3267-3268`.
- Provider-native ids retained via `providerRefs` (`:45-50`); effort left an open string
  because "provider vocabularies differ" (`:565-566`); `unknown` is a first-class member of
  `CanonicalItemType`, `CanonicalRequestType`, and `RuntimeErrorClass`, so unmappable input
  is *labelled*, not force-fit.
- The offline usage page is scrupulous: `UsageCostSource = providerReported | modelPriced
  | unpriced` (`packages/contracts/src/usage.ts:45-54`), `UsageSourceStatus = ok | missing
  | partial | failed` (`:128`), `malformedRecords` (`:136-137`), and the caveat at `:76-78`:
  "`costUsd` is the raw API-equivalent cost of these tokens. It is not money spent:
  subscription plans bill separately."

**Lost.**
- `RuntimeTaskUsage` has no provider field (`:477-486`); `mergeUsageMax` takes no provider
  argument (`subagentRuntime.ts:192-195`). A merged count is unattributable, and may be a
  two-moment chimera with no marker.
- Silent narrowing with no counter: Codex's `.last` discarded (`CodexAdapter.ts:676-679`),
  Claude's `cache_creation_input_tokens` never read (`ClaudeAdapter.ts:970-974`), malformed
  usage dropped (`ClaudeAdapter.ts:967-969`, `CodexAdapter.ts:685-687`), and unknown Codex
  items dropped entirely rather than surfaced as `unknown` (`CodexAdapter.ts:474-476`).
- Cursor, Grok, and OpenCode report no usage at all — `UsageProviderKind =
  ["claude","codex"]` (`usage.ts:26`) — yet render identically to a measured zero via `?? 0`
  (`subagentRuntime.ts:838`). "Σ N tok" over a mixed fleet is a Claude+Codex subtotal
  presented as the whole.

---

## 6. What is NOT normalized

- **The tool payload `data` — the big one.** `ItemLifecyclePayload.data` is `Schema.Unknown`
  (`providerRuntime.ts:411`) and every adapter fills it differently: Claude `{toolName, input}`
  (`ClaudeAdapter.ts:2663-2666`), Codex the **raw provider payload verbatim**
  (`CodexAdapter.ts:494`), OpenCode `{tool, state}` (`OpenCodeAdapter.ts:931-934`), ACP
  `{toolCallId, kind?, command?, rawInput?, rawOutput?, content?, locations?}`
  (`AcpRuntimeModel.ts:334-353`). Ingest passes it through opaquely
  (`ProviderRuntimeIngestion.ts:808`, `:834`), and **`grep -rn "toolName" apps/web/src`
  returns nothing** — the UI never reads it.
- Consequently: **t3code's cross-provider uniformity is achieved on *presentation
  strings*, not on a structured tool model.** The canonical, universally-read fields are
  `itemType` + `title` + `detail`, where `detail` is a human string built at the adapter:
  `summarizeToolRequest` returns `` `${toolName}: ${command}` `` or `` `${toolName}:
  ${json}` `` (`ClaudeAdapter.ts:1149-1175`). That is sufficient to render one timeline.
  It is *not* sufficient to ask "has this exact thing already failed."
- Deliberate leaks to the UI: provider icons (`apps/mobile/src/components/ProviderIcon.tsx:15,26,42,53`)
  and a display-name label (`apps/mobile/src/lib/modelOptions.ts:36`,
  `PROVIDER_DISPLAY_NAMES` `model.ts:219-225`). These are the *only* `provider ===`
  branches in the whole client — a genuinely strong result.
- Accidental: three-plus status vocabularies (§3); `durationMs` riding a max-merge built for
  token counts (`subagentRuntime.ts:221-224`); the coordinator-skip guard applied
  unconditionally though its comment says it holds only "in some providers" (`:829-833`).

---

## 7. Mapping onto Beekeeper

Beekeeper's transcript surface, located: **there is no `coding_session_transcript.rs`.** Items
are untyped `serde_json::Value` in a `"kind"`-discriminated open union, produced by
`TranscriptTranslator` (`crates/beekeeper-session-provider/src/transcript.rs:60-66`,
`on_update` at `:98-146`) and wrapped in `TranscriptEnvelope`
(`crates/beekeeper-core/src/coding_session_payload.rs:457-473`) as the **content** of a signed
kind-44225 event (`crates/beekeeper-core/src/kind.rs:649`; built at
`crates/beekeeper-sdk/src/builders.rs:2743-2771`; signed at
`crates/beekeeper-session-provider/src/lib.rs:1848-1849`).

The SESSION_STATE §2 item 2 claim (`plans/SESSION_STATE.md:47-53`) is confirmed exactly:

- `tool_call_item` emits only `{kind, tool:{toolName, toolId, input}}` — `transcript.rs:192-206`.
- `tool_input` takes the first of `rawInput|input|arguments|args` that is an object, else
  `json!({})` — `transcript.rs:353-362`. That `{}` is the codex-acp case.
- `tool_name` falls back `toolName → title → kind → "unknown_tool"` — `transcript.rs:382-391`.
  **This is worse than the ledger states:** the fallback collapses ACP's `kind`
  *discriminant* into the *name* field, destroying the one structured signal ACP guarantees.
- No normalization on the write path — only size-bounding (`bounded_input` `:369-380`,
  `bound_text` `:432-440`, `fit_item` `:236-271`). The sole `toolName` sanitation is
  read-side and post-signing: `safe_brief_identifier` rejects non-`[A-Za-z0-9_.:-]` names and
  substitutes `redacted_tool` in the context brief
  (`crates/beekeeper-core/src/coding_session_context.rs:393-397`, `:500-506`).
- Host paths do reach signed content: nothing between `on_update` and `sign_with_keys`
  inspects `toolName`, and the content sanitizer elides only secret-ish *keys*
  (`coding_session_context.rs:821-841`) — not `toolName`, `title`, `input`, or paths.
- Per-adapter branching already exists in `buzz-acp` — but only for transport, env,
  capability, and token accounting (`acp.rs:585-591`, `config.rs:711-718`, `:759-763`,
  `acp.rs:2441-2475`, `usage.rs:325-334`). **`transcript.rs` has no idea which adapter
  produced the frame** — `TranscriptTranslator::new` takes only `include_thoughts`
  (`transcript.rs:70-78`).

### What Beekeeper would have to add

1. **A canonical tool vocabulary.** Beekeeper is ACP-native, so it gets this nearly free —
   ACP's `toolCall.kind` is already on the wire and Beekeeper already logs it
   (`acp.rs:2022-2031`). Port `canonicalItemTypeFromAcpToolKind`
   (`AcpRuntimeModel.ts:292-306`) and stop letting `kind` fall into the name slot.
2. **Command recovery.** t3code's `extractToolCallCommand`
   (`AcpRuntimeModel.ts:249-265`) tries `rawInput.command`, then
   `executable + args`, then **scrapes a backtick-quoted command out of the prose title**
   (`extractCommandFromTitle`, `:241-247`). That last fallback is written for exactly
   Beekeeper's codex-acp case.
3. **A structured attempt key** — which t3code does *not* have and cannot lend. Its
   `toolInputFingerprint` (`ClaudeAdapter.ts:1445-1447`) dedupes streaming input deltas
   within one call (`:2516-2528`), nothing more.
4. **A path scrubber before signing.** No t3code analogue — t3code never publishes.

### Where the analogy breaks

t3code's canonical layer is in-memory and its durable layer is a *local* SQLite projection
it owns. Beekeeper's transcript items are signed, relay-stored, append-only, publicly readable
(`kind.rs:645`), authored by the provider key. Three consequences:

- **A mis-normalization at ingest is permanent.** t3code's fuzzy `includes("command")`
  classifier (`ClaudeAdapter.ts:710-717`) is fine because a bad guess is one redeploy from
  fixed. The same heuristic in a signed Beekeeper event is wrong forever, for every reader.
- **A leaked host path cannot be un-signed.** This is not symmetric with the other
  concerns and must not be traded off against them.
- **But t3code is less renormalizable than assumed** (§1): raw frames live 14 days
  (`EventNdjsonLogger.ts:31`) and the durable store is already normalized. The real
  difference is *blast radius* (one local DB vs. every relay reader), not re-derivability.

### Where normalization belongs

**Both, split by reversibility — this is the actual answer.**

- **Redaction → adapter, before signing. Non-negotiable.** Host-path and secret scrubbing
  of `toolName`/`title`/`input` must happen in `transcript.rs` before
  `lib.rs:1848`. Cost: a scrub bug destroys information irrecoverably. Mitigate by
  scrubbing *narrowly* (workspace-root prefix → `$WORKSPACE`) rather than aggressively.
- **Interpretation → adapter, but additively, never destructively.** Copy t3code's dual
  carriage (`providerRuntime.ts:612-613`): keep `tool.toolName` and `tool.input`
  **verbatim as today**, and *add* a versioned sidecar — `tool.canonical: {v, kind,
  command?, argv?, confidence}` — so a future reader that distrusts `v:1` can fall back to
  the raw. Stop the `kind`-into-`toolName` collapse at `transcript.rs:382-391`; that one
  is pure loss with no upside. Cost: permanent bytes and a version to carry forever.
- **Display vocabulary and cross-adapter reconciliation → the fold, after signing.**
  Status labels, model chips, token max-merge. Cheap to change, and Beekeeper already does the
  analogous thing read-side (`coding_session_context.rs:393-397`). t3code's field-wise
  max-merge (`subagentRuntime.ts:182-226`) ports directly, and Beekeeper needs it: it already
  trusts Codex's `total_tokens` and distrusts Claude's (`usage.rs:325-334`).
- **The attempt key → a signed *tag*, not content.** "Has this exact thing already failed"
  should be a relay filter, not a transcript scan. A `cst-attempt` tag carrying
  `hash(canonical_kind ‖ canonicalized_input)` on the 44225 event makes it a `#cst-attempt`
  query — which is also what CLAUDE.md's "prefer events and tags over new endpoints" asks
  for. This has no t3code counterpart; it is the piece Beekeeper must design itself, and it is
  the piece the ledger item actually needs.

**One caution carried over:** t3code's uniformity rests on presentation strings that
nothing parses (§6). If Beekeeper normalizes only to a `detail`-style human string, it will
have bought t3code's *look* without buying the ledger item's *fix*.
