# Live agent-progress surface, buzz-natively — design note

_2026-08-19. Design note, not an implementation handoff. Every claim below
carries the `file:line` the recon produced; nothing here was inferred from
memory. §3 folds in the full t3code Agents-sidebar capability inventory —
t3code paths are relative to `apps/web/src/components/` and
`packages/client-runtime/src/state/` unless shown otherwise._

> **Superseded on 2026-08-21 — read this first.**
>
> This note's freshness prescription (§5.3, and every mention below of
> `PULSE_ACTIVE_WINDOW_SECONDS = 1800`, a "fourth dot state: stale", or
> "44223 staleness") is **withdrawn**. It was written against a Project Pulse
> that decided liveness from how recently a session's metadata was signed.
> Pulse no longer does: metadata recency is *history* — a machine that dies
> mid-turn keeps its last signed fact saying `running` forever — and liveness
> now comes only from an unexpired kind-24223 lease.
>
> Agent Progress follows the same rule, through the same code. The complete
> session-coordination fold lives in
> `desktop/src/shared/coordination/sessionCoordinationFold.ts`; Project Pulse
> and Agent Progress are both adapters over it, so the app has exactly one
> answer to "is this alive?". The panel's row vocabulary is `Reachable` /
> `Unverified` / `Closed` (Pulse's `Provider-reachable` / `Open · liveness
> unverified` / `Closed`, mapped 1:1 in `sessionCoordinationFormat.ts`), and
> reported provider status is a **separate axis** beside it — never a
> substitute for it. `Working`/`Stale`/`Ended` are not coordination states.
>
> Everything else here — the t3code inventory (§3), the fixed-height row, the
> single activity line, the session-versus-execution distinction, and the
> deliberate omission of token totals — stands in the implementation.

## 1. What t3code's pipeline actually is

t3code's Agents panel is a **pure client-side derivation over one WebSocket RPC
stream — no polling, no file tailing.** The Claude Code SDK's `system` messages
are parsed in `apps/server/src/provider/Layers/ClaudeAdapter.ts`: coordinator
identity from `task_started` (`:3184-3240`), per-member rows from the SDK's
*undeclared* `workflow_progress` array on `task_progress`
(`parseWorkflowProgress:1072-1125`, emitted by
`emitWorkflowMemberProgress:2995-3059`), phases declared on the coordinator's
own row as `payload.phases` (`:3253-3270`), status patches from `task_updated`
(`:3273-3300`). `ProviderRuntimeIngestion.ts` then stamps `agentKind`
(`:322-331`), splits each progress tick into **two stable-id activities** so a
reasoning tick cannot blank the token count (`:568-641`), and appends
`thread.activity-appended` events (`decider.ts:1373`) fanned out over one
`orchestration.subscribeThread` WS stream (`ws.ts:1311`,
`packages/contracts/src/orchestration.ts:34`) into a reducer that **replaces by
id and re-sorts** (`threadReducer.ts:563-592`) — so the activity list is *latest
state, not history* (`ProviderRuntimeIngestion.ts:593-595`). The panel is one
`useMemo` over that array's identity (`ChatView.tsx:2225-2231`) feeding
`deriveAgentPanelModel`, phase state computed client-side
(`subagentRuntime.ts:782-811`), amplification bounded by a **material-transition
fingerprint filter** at the adapter (`ClaudeAdapter.ts:3016-3030`), the only
filesystem read being the on-demand `{}` script viewer
(`workflowScriptQuery.ts:25-27,73-113`).

**The one structural difference that drives this whole design:** t3code gets
"latest state" by *mutating* a row in an in-memory store keyed by a stable id.
Buzz's wire is an append-only log of signed events hitting Postgres and a Redis
`PUBLISH` with no debounce anywhere in the path
(`buzz-relay/src/handlers/ingest.rs:369-757`, `buzz-pubsub/src/lib.rs:279-299`).
Buzz therefore gets "latest state" by **fold**, and pays durably for every tick it
publishes. Cadence control is a correctness requirement here, not an optimisation.

## 2. The buzz-native equivalent

### 2a. What already carries what — reuse before inventing

| t3code concept | buzz carrier that exists today |
|---|---|
| per-member tool/activity line (`▸ Bash`, `▸ StructuredOutput`) | **44225 transcript** items `tool_call` / `tool_result` (`buzz-session-provider/src/transcript.rs:199,221`), already streaming per execution |
| per-member task list / progress tree | **44225 `plan` item** — entries + text, already live (`transcript.rs:139,318`) |
| terminal member result + tokens + cost | **44225 `result` item** (`coding_session_payload.rs:545-576`: `subtype`, `isError`, `durationMs`, `costUsd`, `input/output/totalTokens`, omitted-not-null) |
| member status dot (`working/idle/failed`) | **44223 metadata** `SessionStatus` (`coding_session_payload.rs:197-218`), republished event-driven on turn start/end (`buzz-session-provider/src/lib.rs:1739,1786,1885,1910`) |
| member elapsed / freshness | 44223 `verifiedAt` + the Pulse activity window (`pulseFold.ts:60-77`) |
| bounded/oversized payloads | **`elided` item** — byte count + SHA-256 rather than a silent drop (`transcript.rs:236-266`) |
| the "one durable thing, many executions" grouping | coding-session umbrella `sessionRef` (NIP-CSG/CSL), the grouping Pulse already joins on (Pulse plan §3 decision 16) |

So **per-agent lane detail needs no new kind**. The 44225 item union is
`kind`-discriminated and open (`coding_session_payload.rs:460-473`); it already
covers everything `AgentRow`'s three lines render.

### 2b. What is genuinely missing

1. **Lane/phase grouping across distinct agents.** `sessionRef` groups multiple
   *provider executions of one session*; nothing groups multiple *distinct agents*
   into one run with phases. `KIND_TEAM` 30176 / `KIND_MANAGED_AGENT` 30177
   (`kind.rs:196-327`) are roster kinds with no run concept.
2. **Workflow run progress never reaches the wire.** `46001`–`46012` are fully
   defined (`kind.rs:743-761`) and **nothing constructs or signs one** — run state
   lives in Postgres behind REST only. The vocabulary exists, dormant.
3. **No run-scoped usage rollup.** 44200 (NIP-AM) is per-turn, owner-scoped and
   **NIP-44 encrypted to the owner** (`kind.rs:589-756`), so it cannot back a
   shared `Σ N tok` footer at all.

### 2c. New wire surface — one durable kind, one overlay

**Durable: kind 44243 `KIND_AGENT_RUN_MEMBER`** (44240 is the Pulse entry,
`kind.rs:724`; 44241/44242 are reserved by Pulse plan §3 decisions 5–6). Signed by
the coordinator *or* the member itself. **Immutable per emission, folded to latest
— never rewritten**, the 44223 discipline (`docs/nips/NIP-CSL.md:271-272`).

```
kind:    44243
tags:    ["d", "<runId>:<laneIndex>"]      # stable slot, not per-attempt id
         ["a", "<projectCoordinate>"]      # same ACL join Pulse uses
         ["h", "<channelId>"]              # NIP-29 scoping (CLAUDE.md Key Patterns)
         ["run", "<runId>"]
         ["cs", "<sessionRef uuid>"]       # present iff this lane is a coding session
content: {
  "schema": "buzz-agent-run-member/v1",
  "runId", "laneIndex", "label", "role",
  "phaseIndex", "phaseTitle",              # null when undeclared — never inferred here
  "state": "pending|running|waiting|completed|failed|cancelled",
  "attempt": 1,
  "lastToolName": "Bash",                  # or null
  "observedAt": "<iso>",                   # freshness, same role as 44223 verifiedAt
  "claimed": { "totalTokens": …, "toolUses": …, "durationMs": … }  # see §5
}
```

Plus a **run header** on the same kind with `laneIndex: null` carrying
`workflowName` and the declared `phases: [{index,title}]` — t3code's choice of
attaching phases to the coordinator's own row rather than a separate one
(`ClaudeAdapter.ts:3253-3270`, whose comment records that a phases-only row
collided on the stable id and wiped usage). Copy it, for the same reason.

**Cadence: publish only on material transition.** Port
`ClaudeAdapter.ts:3016-3030`'s fingerprint filter
(`[state,label,model,lastToolName,error,tokens,toolCalls,phaseIndex,attempt]`)
into the buzz publisher. An unfiltered tick costs t3code an in-memory upsert; it
costs buzz a signed Postgres row and a Redis fan-out, so this filter is
load-bearing. Caps mirror t3code's (64 phases, 100 agents,
`ClaudeAdapter.ts:1042-1043`; roster cap `subagentRuntime.ts:109`) as named
`pub const`s in buzz-core, per Pulse plan §3 decision 13.

**Never-stored: kind 39012 `KIND_AGENT_RUN_DIGEST`** — the folded run view,
computed at query time and relay-signed, the 39005/39006 overlay pattern
(`buzz-relay/src/api/bridge.rs:398-627`, `sign_overlay`); 39011 is taken by Pulse
Slice 2 (Pulse plan §3 decision 4). For cheap cold start and agent/CLI
consumption, **not** the live path. Precedence when both exist is t3code's
`v2Projection` rule: one source wins outright, never merged
(`subagentRuntime.ts:1-18,726-736`). **Rejected:** a relay-signed *durable*
projection in the 39010 style (`side_effects.rs:2495-2554`) — it republishes on
every accepted op, which at member-tick cadence is a write amplifier with a
strictly-increasing-`created_at` same-second collision guard doing real work.

### 2d. What stays client-side fold

Everything derived: phase state, active/settled counts, lane grouping, staleness,
footer totals. This is the t3code split (`subagentRuntime.ts:782-811` computes
phase state client-side and never sends it) and it matches the Pulse architecture:
**one fold, two languages, bound to a conformance corpus**
(`conformance/project-pulse-fold/CONTRACT.md`, Pulse plan §3 decision 19;
`pulseFold.ts` module doc). New corpus: `conformance/agent-run-fold/`, same
constraints — zero cross-directory runtime imports, erasable TypeScript only, so
`node --test` type-stripping runs it.

## 3. Capability spec — the whole t3code sidebar, decided

Every capability the inventory found, marked **S1** (Slice 1), **LATER** (a named
later slice), or **NO** (deliberately not, with the reason). **REQ** marks an item
that is a design decision in disguise — the inventory's in-code comments record it
as a live-test finding, and dropping it re-breaks something t3code already paid for.

### 3a. Panel chrome & entry points

| Capability | Verdict | Reason |
|---|---|---|
| Per-surface tab strip, Agents tab with `Bot` icon + hover-X (`RightPanelTabs.tsx:625-681,512-513`) | LATER (S2) | S1 renders inside the existing project/session surface; buzz has no right-panel tab-strip abstraction to generalise yet |
| Middle-click closes tab (`:583-591`); tab context menu Close/others/right/all (`:522-578`); active-tab auto-scroll (`:593-596`); inline-dock vs overlay-sheet rendering (`ChatView.tsx:6076,6205`) | NO | Consequences of the tab strip and of responsive layout; nothing to port until there is one, and buzz's panel is docked |
| Empty-state launcher card: description "Follow subagents and workflows.", live-count badge (`:225-234,318-326`); "+" add-surface dropdown **disabled with a stated reason** ("Agents are only available from a thread.") (`:67,73,94,116,683-748`) | LATER (S2) | Needs the launcher grid; the live count is S1 (below). Keep disabled-with-reason — the honesty rule buzz already applies to controls that cannot act |
| Global letter shortcuts B/T/F/D/P/**A** while launcher visible, suppressed in inputs/menus/dialogs (`:246-274`); arrow-key card highlight + Enter (`:276-302`) | NO | Launcher-scoped keyboard model buzz doesn't have |
| Panel toggle badged with `liveAgentCount`, **badge suppressed when the Agents surface is already showing** (`PanelLayoutControls.tsx:62-99`, `ChatView.tsx:6057-6061`) | S1 | A count you are already looking at is noise. Buzz's count must be freshness-gated (§5.3) or the badge lies |
| Maximize/restore (`PanelLayoutControls.tsx:104-135`, `ChatView.tsx:6076-6081`); `rightPanel.toggle` / `toggleMaximized` keybindings (`keybindings.test.ts:90,686,694,728`); resizable persisted width (`RightPanelTabs.tsx:44-48,599-604`) | LATER | Panel ergonomics, orthogonal to the data model |
| In-chat "Kicked off N subagents" CTA row with its own live/settled summary (`MessagesTimeline.tsx:2130-2215` → `ChatView.tsx:3298-3301,6305`) | LATER (S2) | The right idea — a run announces itself where it started. Must share the coordinator-exclusion helper (`MessagesTimeline.tsx:2166-2171`), not re-implement it |

### 3b. Run list & workflow card

| Capability | Verdict | Reason |
|---|---|---|
| Empty state: icon + "No agents yet" + explanatory copy (`AgentsPanel.tsx:532-543`) | S1 | |
| Scroll list = every workflow run, then a "Direct spawns" section (`:549-566,557-566`) | S1 (direct spawns only) | S1 has no run concept on the wire; the flat lane list *is* the direct-spawn section |
| Collapsed run row: dot (**red if any member failed**, else run status), name, "N failed", agent count, `Σ tok`, elapsed **only when both `startedAt` and `completedAt` exist** (`:457-498`), and the expanded card header: dot, name, `{} script` chip, "settled/total settled", collapse chevron (`:399-430`) | LATER (S2) | Needs 44243. The two conditionals are honesty rules: a run containing a failure is not green, and a half-known duration is not shown |
| `{}` script chip + read-only viewer, `getWorkflowScript` RPC cached 5 min, truncation note, loading/failure states (`:261-274,286-294,396-398,406-418,265-310`; `orchestration.ts:15-21`) | NO (open, Q6) | A filesystem read behind the relay is a separate authorization decision, not a rendering one |
| Workflow open/closed is **presentation state**: live runs start expanded and **stay expanded when they settle** (`:193-201,500-521`) | LATER (S2), REQ | A panel that snaps shut at the moment of completion hides the outcome you were waiting for |
| Unphased / unknown-phase members render anyway (`:443-445`, `subagentRuntime.ts:813-818`) | S1, REQ | See §5.5 — a lane that vanishes because its phase didn't resolve is a lie by omission |
| A run with zero phases and zero members still renders as a single row (`:446-448`) | LATER (S2), REQ | The coordinator is never invisible |
| Fixed ordering, **no sort/filter/search control at all** — workflows by `firstSeenAt` then id, members by `agentIndex` (`subagentRuntime.ts:741,786,818,845-847`) | S1 | Stable spawn order is what makes "update in place" legible; a re-sortable list destroys it |

### 3c. Phase rail & phase sections

| Capability | Verdict | Reason |
|---|---|---|
| `PhaseRail`: one segment per phase in fixed order, chevron separators, ✓-prefixed titles when done, border tinted by state, one dot per member, `–` placeholder for an empty phase (`AgentsPanel.tsx:213-259`) | LATER (S2) | Nothing on the buzz wire declares phases before 44243 |
| `PhaseSection` header counts: "pending" / "N done" / "N active · N done" (`:334-370`); collapsed header shows inline member dots (`:363-369`); settled/historical runs render pre-expanded (`:441`) | LATER (S2) | Nothing to wait for on a settled run — show the record |
| Auto-open the instant a phase goes `running`; manual collapse afterwards sticks (`:317-332`) | LATER (S2), REQ | Attention follows the work, but never fights the user twice |
| Inferred phases (derived from members' `phaseIndex`, titled "Phase N" with **no marking**) (`subagentRuntime.ts:762-779`) | **EXCEED** (S2) | Buzz may do the same fold but must label the rail inferred — §5.4 |

### 3d. Lane row (t3code `AgentRow`) — the dense part

| Capability | Verdict | Reason |
|---|---|---|
| **Fixed 3-line grid at a fixed height (`h-[3.875rem]`)** so changing data never reflows the row (`AgentsPanel.tsx:141-190`, rule stated at `:6-9`) | S1, REQ | A roster where rows resize as text arrives is unreadable while it is doing the one thing you opened it to watch |
| Line 1: dot + truncated title + role chip **only when role ≠ title, case-insensitively** (`:158-168`) | S1 | Suppressing the tautological chip is what keeps line 1 scannable |
| Line 1 right: elapsed timer that **ticks by direct DOM write on a 1s `setInterval`, zero React re-renders**, freezing at `completedAt` when settled (`AgentElapsed`, `:87-114`); green `Check` appended **only** for `completed` (`:169-176`) | S1, REQ | Buzz's `useLiveCodingSessionDuration` re-renders per second — fine for one card, quadratic attention cost at 100 lanes. Adopt the DOM-write ticker at lane scale |
| Line 2 activity text, priority **differing by liveness**: live = `progress` → `▸ tool` → `result` → `error`; settled = `error` → `result` → `progress` → `▸ tool`; destructive color when failed; falls back to the status label (`:121-138,177-184`) | S1, REQ | Live rows lead with what is happening; settled rows lead with the outcome. **Errors lead only when the row failed** — so a red row explains itself, and a running row isn't dominated by a recovered error |
| Line 3 metadata `" · "`-joined: compact model label (`subagentRuntime.ts:918-930`), tokens or **`— tok` when unknown**, "N tools", "run N" when `activationCount > 1` (`:149-154,185-187`) | S1 (tokens gated by Q3) | `— tok` vs `0` is the unknown/confirmed-zero distinction buzz-acp already enforces (§5.2) |
| **All in-flight states collapse to one "Working"** — `pending`/`running`/`waiting` → one info-blue dot, one label (`STATUS_VISUALS`, `:39-50`; rationale at `:33-38`) | S1, REQ | Detail belongs in the activity sub-line. A queued or waiting agent is the fleet doing its job, not a user problem — three near-identical blue states are noise that reads as a fault |
| `idle` → **muted** dot, "Idle · resumable" — deliberately not sky-blue (`:43-45`) | S1, REQ | In-code live-test finding, verbatim: "sky idle dots read as stuck in-progress". A resting agent must look settled, not hung |
| `completed` → green "Completed"; `failed` → red "Failed"; `cancelled`/`interrupted` → muted **"Stopped"** (`:39-50`) | S1 | Only settled states differentiate |
| Row is **flat and non-interactive — no unfold, no click handler, no context menu** (`:140`) | S1 (body), **EXCEED** (affordances) | Keep the row body non-interactive; buzz adds explicit, labelled controls instead of hidden expansion — §3h |
| `sr-only` duplicate of the status label (`:188`) | S1 | The dot is `aria-hidden`; without this the status is invisible to a screen reader |
| Fourth dot state: **stale** (buzz-only) | S1, **EXCEED** | §5.3 |

### 3e. Footer

| Capability | Verdict | Reason |
|---|---|---|
| Sticky footer: "N working" (info color) · "N idle" · "N settled", **each hidden when zero**, right-aligned "Σ N tok" (`AgentsPanel.tsx:569-580`) | S1 (+ **EXCEED**: label the total *claimed*, §5.2) | Zero-suppression keeps the footer one glance wide |
| **Workflow coordinators with members are excluded from every count and from the token sum** (`subagentRuntime.ts:828-839`, repeated at `MessagesTimeline.tsx:2166-2171` and `AgentsPanel.tsx:466-471`) | LATER (S2), REQ | A container is not work: counting it reports one more agent working than exist and double-counts its members' tokens. Buzz ships this as **one shared helper**, not three copies |

### 3f. Interactions confirmed present

Collapsed run → expands (`AgentsPanel.tsx:478-495`) · header chevron → collapses
(`:422-429`) · phase header → toggles its members (`:336-373`) · `{}` chip →
script viewer, `X` closes (`:406-418,286-294`) · in-chat CTA → opens the surface
(`ChatView.tsx:3298-3301,6305`) · panel toggle/maximize buttons and keybindings ·
tab close/others/right/all · **A** from the launcher. Verdicts follow their owning
rows in §3a–3c. **That is the complete interaction surface** — the inventory found
no kill, retry, copy, transcript link, per-row menu, filter, sort, search, panel
settings, toast/notification, or drag-to-reorder in `AgentsPanel.tsx` or
`subagentRuntime.ts`.

### 3g. Data & fold rules

| Rule | Verdict | Reason |
|---|---|---|
| The roster is **not a feed** — a pure fold (`foldSubagentActivities`, `subagentRuntime.ts:459-675`) over server-synced thread activities (`packages/contracts/src/orchestration.ts:419`), so it survives restart and reconnect; recomputed by one `useMemo` on `[sessionLive, activities]` (`ChatView.tsx:2225-2231`) | S1 | Identical to buzz's model, and stronger: buzz folds signed relay-stored events |
| Live and historical are **the same fold** — "live" is just non-terminal status at derive time (`subagentRuntime.ts:97-105`, `AgentsPanel.tsx:193-201`) | S1 | A second data source for history is how the two disagree |
| **`sessionLive: false` force-settles every active agent to `interrupted`** (`subagentRuntime.ts:651-661`) | S1, REQ — **rederived** | See §3h; buzz must derive death from signed facts, not a socket flag |
| **Coordinator terminal → force-settle members** that never got their own terminal row (`completed` → member `completed`, else `interrupted`) (`subagentRuntime.ts:629-649`) | LATER (S2), REQ | In-code live-test finding: statuses drifted whenever member terminal rows were lost or never emitted. Nothing may read "working" after its run is over |
| **Usage merges field-wise max, never overwrite** (`subagentRuntime.ts:182-226`) | S1, REQ | Idempotent under duplicate/late/reordered frames — cumulative totals never shrink, and a terminal payload carrying only `totalTokens` cannot wipe a known breakdown. On an append-only wire with same-second bursts this is not optional |
| **Reactivation** detected from status transition (terminal/idle → running/pending): increments `activationCount` and **clears prior `result`/`error`/`completedAt`** (`subagentRuntime.ts:394-420`); duplicate terminal events are idempotent, first write wins | S1, REQ | A retried lane showing the previous run's output — especially its previous error — is the panel actively lying about the current attempt |
| Roster cap **100**, retention ranked **live first, then idle, then most-recently-updated settled** (`subagentRuntime.ts:109,663-672`) | S1, REQ + **EXCEED** | The ranking is the point: overflow drops history, never live work. Buzz additionally renders "showing 100 of N" — §5.6 |
| Recent-activity ring buffer: 6 entries, consecutive duplicates deduped, each summary bounded to 180 chars (`subagentRuntime.ts:42-45,107-139`) | S1 fold-side, LATER to render | Modelled but never rendered in t3code; buzz keeps it in the fold and exposes it through the transcript link instead (§3h) |
| Membership is decided **once, server-side**, by a persisted `agentKind: "agent"` stamp; unstamped rows are background by definition (`subagentRuntime.ts:111-121`) | S1 (buzz analogue) | On buzz the kind + tags decide. No client-side heuristic for "is this an agent" |
| Three `kind` values `subagent` / `workflow` / `workflow_agent`, classified from payload (`subagentRuntime.ts:258-269`); right-panel tab state persisted to `localStorage` (`rightPanelStore.ts:67-71,363-671`) — *which tabs are open*, never agent data | LATER | Neither is needed until there is a run concept and a tab strip |
| Format helpers: model label strips `claude-` prefix, 8-digit date and `-latest`, appends `· effort` (`:918-930`); tokens raw <1k, `Nk` <1M, `N.NM` ≥1M (`:932-941`) | S1 | Port shape-for-shape; pin in the conformance corpus |
| `workflowCardMembers` urgency ordering, present but **called from nowhere** (`subagentRuntime.ts:862-880`) | NO | Do not port code the source itself doesn't use |

### 3h. Where buzz should deliberately EXCEED t3code

The inventory's NOT-found list is mostly correct restraint. Four items are not —
buzz owns the machinery already, and shipping without them copies a limitation.

1. **A stop/cancel affordance.** The inventory is unambiguous: **no kill, stop, or
   cancel control exists anywhere** in `AgentsPanel.tsx` or `subagentRuntime.ts`;
   the panel's only outbound RPC is the read-only `getWorkflowScript`. Buzz has the
   opposite problem — machinery built, surface missing: signed **kind-44221
   lifecycle commands** already create and end sessions (`kind.rs:617`,
   `codingSessionPendingLifecycle.ts:2-4`) with 44224 receipts (`kind.rs:640`),
   44230 closure (`kind.rs:708`), and R27's closure/stop separation shipped
   (`docs/SESSION_STATE.md:22-23`). A fleet view that shows a runaway agent and
   cannot stop it is worse than no fleet view. Reuse the existing
   **optimistic-pending** overlay so the row reads "stopping…" rather than
   freezing, and expect this to surface R27's open tail — a provider stuck
   `disconnected` is currently unendable (`docs/SESSION_STATE.md:83`).
2. **Open the transcript.** t3code bounds a settled row to a 180-char
   `result`/`error` (`bounded()`, `subagentRuntime.ts:123-125`) with **no way to
   see the full output** — `runHandles.transcriptDir`/`sessionUrl` exist in the
   model, rendered nowhere. Buzz's full transcript is already on the wire and
   readable: 44225 items, `buzz sessions` over the stored record
   (`crates/buzz-cli/src/commands/sessions.rs:1-7`), the desktop transcript views
   (`CodingSessionTranscriptParts.tsx:149-254`), `buzz://` deep links
   (`desktop/src/app/AppShell.tsx:637`). Every settled lane links to its
   transcript; a bounded preview that cannot be opened is a dead end.
3. **A cross-project aggregate view.** t3code's surface is **thread-scoped only** —
   one singleton "agents" surface per thread (`rightPanelStore.ts:65,137-139`), and
   the screenshot's multiple run rows are runs *within one thread*; there is no
   all-threads roster. Pulse already answers the project-level question, so the run
   view composes into it rather than trapping the fleet in one channel.
4. **Session-death handling — carried over as REQUIRED.** t3code force-settles
   every active agent to `interrupted` when the session dies
   (`subagentRuntime.ts:651-661`), mirroring server-side liveness clearing on
   `session.exited` "so panel and sidebar can never disagree". This is the
   freshness-gate lesson in different clothes; buzz **must** have it but cannot
   copy it, having no client session flag to read — the observer is a relay
   subscriber, possibly watching a machine they never connected to. Derive it from
   **signed facts**: 44223 staleness against `PULSE_ACTIVE_WINDOW_SECONDS`
   (`pulseFold.ts:60-77`), provider liveness, 44230 closure. t3code's rule catches
   a disconnect while the flag is right; buzz's also catches what t3code cannot see
   — the agent dying while the socket stays up.

Deliberately not exceeded: no retry/resume control ("Idle · resumable" stays a
*label*; resuming is a lifecycle decision, not a roster one), no row
copy-to-clipboard, no drag-to-reorder, no multi-select or bulk actions, no
per-panel settings UI (caps stay named constants, §2c), no completion notification
or sound — that policy is product-wide, not this panel's to set.

## 4. Desktop rendering plan

Reuse, in order of how close the existing widget already is:

| t3code element | buzz component to generalise |
|---|---|
| `PhaseRail` chips + per-member dots (`AgentsPanel.tsx:213-259`) | **`CodingSessionInlinePlan`** (`CodingSessionTranscriptParts.tsx:149-254`) — already colored progress segments per task, an `N/M` counter, and an expandable status-icon list. Generalise from plan-entries to run-phases. |
| `AgentRow` line 1 (dot + title + role chip) | **`PulseSessionCard`** (`project-pulse/ui/PulseSessionCard.tsx`) — already dot + `pulseSessionStatusLabel` + stale "last seen" + goal + branch chip |
| `AgentRow` line 2 (`▸ Bash`) | **`CodingSessionActiveTool`** (`:40-110`) collapsed to one line |
| `AgentElapsed` right gutter (`:87-114`, DOM writes, zero React commits) | **`useLiveCodingSessionDuration`** in `CodingSessionWorking` (`:112-132`) — swap to the direct-DOM ticker once a run exceeds a handful of lanes (§3d) |
| settled row / terminal summary | **`CodingSessionTurnCompletion`** (`:374-429`) — state, duration, cost, color-coded |
| `CollapsedWorkflowSection` one-liner, list of concurrent runs | **`projectCodingSessionShelf.ts`** + `ProjectSidebarGroup.tsx` / `ProjectChildRowItem.tsx` |
| footer `N working / N idle / N settled` | `project-pulse/lib/pulseFormat.ts` |
| `{}` script viewer (`workflowScriptQuery.ts`) | **defer** (§3b, Q6) |

Status reconciliation goes through the existing priority ladder, not a new one:
`deriveCodingSessionWorkspaceStatus` (`codingSessionWorkspaceModel.ts:193-283`) —
**signed fact outranks inference, with an explicit freshness tie-break** — plus
the wire→label map `codingSessionWireWorkspaceStatus` (`:158-181`), so a lane
cannot read "Working" on the shelf and something else in the run panel.

## 5. Honesty rules this surface must obey

1. **Observed vs claimed, per signer.** t3code has one trusted local adapter, so a
   coordinator's claim about a member is effectively an observation. Buzz is
   multi-party: a 44243 signed by the coordinator is a *claim about someone else*.
   A lane's own signed 44223/44225 wins and the row reads observed; with only the
   coordinator's row, the row reads reported-by, coordinator identity visible —
   shipped operator-attribution, applied to lanes.
2. **Usage numbers are provider-reported claims, always.** t3code's `Σ tok` is
   `entry.tokens`/`entry.toolCalls` off the SDK array
   (`ClaudeAdapter.ts:3045-3052`) — nobody measured it. Same on buzz: hence the
   `claimed` envelope (§2c) and a footer that says so. Keep buzz-acp's `None`
   (unknown) vs `Some(0)` (confirmed zero) discipline and `delta_reliable`
   (`buzz-acp/src/usage.rs:1-120`); render `— tok` for unknown as t3code does
   (`AgentsPanel.tsx:149-154`), never `0`. **44200 cannot back this footer** — it
   is NIP-44 encrypted to the owner (`kind.rs:589-756`), so a non-owner sees
   per-lane claims only.
3. **A dead agent must not render as active.** t3code maps
   `pending`/`running`/`waiting` → one info dot reading "Working"
   (`AgentsPanel.tsx:39-50`) with **no freshness gate**; its only liveness backstop
   is the session-death sweep (`subagentRuntime.ts:651-661`), which fires on socket
   state, not on the agent going quiet. Copy the collapse rule (§3d), not the gap:
   buzz has `PULSE_ACTIVE_WINDOW_SECONDS = 1800` over `PULSE_ACTIVE_STATUSES` because
   *"the absence of a closure is not evidence of life"* (`pulseFold.ts:60-77`; Pulse
   plan §3 decision 14). The run fold gets that rule
   **and** the force-settle sweep, plus a **fourth dot state** — stale — reading
   `last seen 42m ago`, never `Working`.
4. **Never infer a phase silently.** t3code derives phases from members'
   `phaseIndex` and titles them `Phase N` with no marking
   (`subagentRuntime.ts:762-779`). Buzz may do the same fold but must label an
   inferred rail inferred; `phaseIndex: null` on the wire means unknown, and
   unknown is not false (Pulse plan §3 decision 12).
5. **Members with an unknown phase are shown, not dropped** — t3code gets this
   right (`unphasedMembers`, `subagentRuntime.ts:815-818`); a lane that vanishes
   because its `phaseIndex` didn't resolve is a lie by omission.
6. **Truncation is disclosed.** Reuse the `elided` item (`transcript.rs:236-266`)
   for any bounded field — including the 180-char summary bound
   (`subagentRuntime.ts:123-125`) — and the 64-phase/100-agent caps render "showing
   100 of N", never a silently short list (`subagentRuntime.ts:109,663-672`).
7. **A run digest injected into an agent's prompt is evidence, never instruction**
   — Pulse plan §3 decision 20, unchanged. Lane labels are third-party prose.
8. **Two decoders is a defect, not a nuance.** SESSION_STATE §2 item 18 (Rust
   strict `decode_coding_session_metadata` vs permissive
   `parseBuzzCodingSessionMetadata`) is inherited by any run fold that reads
   44223. Close it, or the run fold must not read 44223 directly.

## 6. Slices

**Slice 1 — the run view over events that already exist. No new kind.**
Fold the coding sessions already visible in a project into lanes; render live
per-lane activity from the 44225 items already streaming, freshness-gated. Zero
protocol change, zero new trust, and it answers "what are my agents doing right
now" today. No phases (nothing declares them yet) — a flat lane list. Carries
every **S1/REQ** row in §3d, §3e, §3g, plus stop and transcript links (§3h).
- `conformance/agent-run-fold/{CONTRACT.md,fixtures/fold-vectors.json}`
- `desktop/src/features/agent-runs/lib/runFold.ts` (+ `.test.mjs`), same
  zero-import / erasable constraints as `pulseFold.ts`
- `desktop/src/features/agent-runs/ui/{AgentRunPanel,AgentRunLaneRow}.tsx` wrapping
  `PulseSessionCard` / `CodingSessionActiveTool`, behind the preview flag

**Slice 2 — kind 44243 + material-transition publishing.** Declared phases and
lanes reach the wire; the phase rail, the workflow card, the coordinator
exclusion and the force-settle sweep become real (§3b, §3c, and their REQ rows).
- `crates/buzz-core/src/kind.rs` (+ a new `agent_run.rs` payload module, the
  `pulse.rs` shape), `crates/buzz-sdk/src/builders.rs`
- publisher + fingerprint filter in `crates/buzz-acp/` (or `sprig`)
- relay read gate alongside the 44240 gate (`kind.rs:1049`, `project_acl.rs`)
- `buzz run list|show` in `crates/buzz-cli/`, bound to the same corpus as the TS fold

**Slice 3 — kind 39012 overlay.** Query-time relay-signed run digest for cold
start and agent injection, `bridge.rs:398-627` pattern. Shape frozen in Slice 2 by
the CLI's `run show`; Slice 3 changes who computes it, never its shape (Pulse plan
§3 decision 17). **Slice 4 — wake 46001–46012:** `buzz-workflow` signs its dormant
run/step kinds so YAML workflow runs land in the same panel. Separate call, Q5.

## 7. Open product questions for Brian

1. **Is a "run" the coding-session umbrella, or a new grouping?** Reusing
   `sessionRef` gets ACL, rehydration and the Pulse join free but forces every
   lane to be a coding session; a new `runId` is general but is a second grouping
   concept in the product.
2. **Who may sign a lane row?** Coordinator-only is simple and matches t3code.
   Member-signed is more honest but requires every lane to hold a credential —
   the sidecar deliberately has none (`agent_fence.rs:37-44`, Pulse plan §3
   decision 8). Do we show coordinator-claimed lanes when the member never attests?
3. **Do non-owners see token/cost at all?** 44200 is owner-encrypted by design;
   per-lane `claimed` numbers in 44243 would be readable by every project member.
   Is that the intent, or should the footer show tool-call counts only?
4. **Durable retention for 44243.** Even filtered, a 100-lane run emits many
   permanently-stored signed rows. Cap per run? TTL? Or keep lane ticks ephemeral
   (overlay-only) and store just start/settle?
5. **One surface or two?** Agent runs (44243) and YAML workflow runs (46xxx) are
   different execution models — same panel, or is the workflow trace surface
   (VISION.md ⚡ Workflows) the right home for one?
6. **Does the `{}`-script equivalent ship?** t3code exposes the generated
   workflow script read-only over RPC (`orchestration.ts:15-21`); buzz's analogue
   is a filesystem read behind the relay — do we want it, under whose authority?
7. **Is inferred-phase rendering acceptable at all**, or should an undeclared
   run render flat until a coordinator declares phases?
8. **Does SESSION_STATE §2 item 18 (the two 44223 decoders) block Slice 1?**
   The run fold reads 44223 for lane status, so it inherits the divergence.
9. **Does the stop affordance (§3h.1) ship in Slice 1?** The biggest deliberate
   divergence from t3code, and the one with consequences — it sends a signed 44221
   that kills someone's work, and surfaces the open R27 tail
   (`docs/SESSION_STATE.md:83`) the moment a provider is stuck.
