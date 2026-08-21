# t3code Agents-sidebar capability inventory

_Read-only inventory, 2026-08-19. Companion to
`AGENT_PROGRESS_UI_DESIGN_NOTE.md` §3, which folds these findings into a Buzz
design. t3code paths are relative to `/Users/brian/Projects/t3code/t3code`.
Every line carries the `file:line` the recon produced._

---

### Header / panel chrome (`RightPanelTabs.tsx`)
- Tab strip shows one tab per open surface (Browser/Terminal/Files/Diff/Pull-request/**Agents**), each with icon + title + close-on-hover X — `apps/web/src/components/RightPanelTabs.tsx:625-681`.
- Agents tab icon is `Bot` — `RightPanelTabs.tsx:512-513`.
- Middle-click (aux click) on a tab closes it — `RightPanelTabs.tsx:583-591`.
- Right-click on a tab opens a native context menu: Close, Close others, Close to the right, Close all (Copy path only for file tabs) — `RightPanelTabs.tsx:522-578`.
- Active tab auto-scrolls into view in the (horizontally scrollable, fade-edged) tab list — `RightPanelTabs.tsx:593-596`.
- "+" button (`Plus` icon) opens a dropdown menu to add any surface kind, incl. "Agents" with `Bot` icon; disabled with tooltip reason when unavailable (`"Agents are only available from a thread."`) — `RightPanelTabs.tsx:67,73,94,116,683-748`.
- `onAddAgents`/`addAgentsSurface` opens the "agents" surface for the active thread via `useRightPanelStore.getState().open(activeThreadRef, "agents")` — `ChatView.tsx:3298-3301`.
- Empty-state launcher (when panel has no surfaces yet): card grid with all surface kinds; Agents card has keyboard shortcut **A**, description "Follow subagents and workflows.", and a live-count badge (`liveAgentCount`) — `RightPanelTabs.tsx:225-234,318-326`.
  - Letter shortcuts (B/T/F/D/P/A) fire globally while the launcher is visible (not just focused), suppressed inside inputs/menus/dialogs — `RightPanelTabs.tsx:246-274`.
  - Arrow keys move a highlight across cards, Enter activates the highlighted card — `RightPanelTabs.tsx:276-302`.
- Layout/expand icons (`PanelLayoutControls`, `apps/web/src/components/chat/PanelLayoutControls.tsx`):
  - `PanelRightIcon` toggle for right panel open/close, badged with live agent count when the panel isn't already showing the Agents surface — `PanelLayoutControls.tsx:62-99`, suppression rule at `ChatView.tsx:6057-6061`.
  - `PanelBottomIcon` toggle for the terminal drawer (unrelated to Agents but co-located) — `PanelLayoutControls.tsx:38-61`.
  - `Maximize2Icon`/`Minimize2Icon` toggle (`RightPanelMaximizeControl`) to maximize/restore the right panel, shown only when the panel is open and not in sheet mode — `PanelLayoutControls.tsx:104-135`, `ChatView.tsx:6076-6081`.
- Keybinding `rightPanel.toggle` (default seen in tests as Alt/Opt+B) and `rightPanel.toggleMaximized` toggle panel visibility/maximized state — `apps/web/src/keybindings.test.ts:90,686,694,728` (defaults defined in `apps/web/src/keybindings.ts`).
- Panel width is resizable and persisted per surface via `widthStorageKey`/`defaultWidth` props into `PreviewPanelShell` — `RightPanelTabs.tsx:44-48,599-604`.
- Panel can render "inline" (docked) or as an overlay "sheet", chosen by `shouldUseRightPanelSheet` in `ChatView.tsx` (not shown above but referenced at `ChatView.tsx:6076,6205`).

### Run list / Workflow card (top-level `AgentsPanel`)
- Empty state when thread has no agents: bot icon + "No agents yet" + explanatory copy — `AgentsPanel.tsx:532-543`.
- Scrollable list combining, in order: every workflow run (`model.workflows`), then a "Direct spawns" section for non-workflow agent spawns — `AgentsPanel.tsx:549-566`.
- Each workflow run renders as a `WorkflowSection`, which is either collapsed (`CollapsedWorkflowSection`) or expanded (`ExpandedWorkflowSection`) — `AgentsPanel.tsx:501-521`.
- **Collapsed run row** (matches "pulse-slice1-build" style rows in the screenshot): status dot (red if any member failed, else workflow status), run/workflow name, `"N failed"` in destructive color when failures exist, agent count, `Σ token` total, elapsed duration (only if both `startedAt`/`completedAt` present), chevron-right; entire row is a button that expands it — `AgentsPanel.tsx:457-498`.
- **Expanded workflow card** (matches "REHYDRATION-HARDENING-BUILD" card): header row with status dot, workflow name, optional `{} script` chip toggle, `"settled/total settled"` counter, collapse (`ChevronDown`) button — `AgentsPanel.tsx:399-430`.
- `{} script` chip only renders when the workflow carries a `scriptPath` run handle and an environment/thread are known; toggles a read-only script viewer fetched via the `getWorkflowScript` RPC (never a raw filesystem read) — `AgentsPanel.tsx:261-274,396-398,406-418`, RPC def at `packages/client-runtime/src/state/orchestration.ts:15-21` (cached 5 min, "scripts are immutable per run").
  - Script viewer: filename-only header (`Braces` icon), close (`X`) button, scrollable `<pre>` body, truncation note when the fetched script was cut off, and Loading/Failure states — `AgentsPanel.tsx:265-310`.
- Phase rail (`PhaseRail`): one segment per phase in fixed order, chevron separators, phase title (✓-prefixed when done), border tinted by state (running/done/pending), and one `StatusDot` per phase member (or a `–` placeholder if the phase has no members yet) — `AgentsPanel.tsx:213-259`.
- Phase sections (`PhaseSection`) below the rail: collapsible per phase.
  - Header shows expand chevron, ✓ for done phases, phase title, and a count string: `"pending"` / `"{settledCount} done"` / `"{activeCount} active · {settledCount} done"` — `AgentsPanel.tsx:334-370`.
  - When collapsed, shows a compact row of member status dots inline in the header — `AgentsPanel.tsx:363-369`.
  - Auto-opens the moment a phase transitions into `running`; manual collapse/expand otherwise sticks — `AgentsPanel.tsx:317-332`.
  - Default-open forced true when the whole workflow is not live (i.e., historical/settled runs open pre-expanded) — `AgentsPanel.tsx:441`.
  - When open, renders every member as an `AgentRow` — `AgentsPanel.tsx:371`.
- Unphased members (no resolvable phase, or an unknown phase index) render directly under the workflow, never dropped — `AgentsPanel.tsx:443-445`, model logic at `subagentRuntime.ts:813-818`.
- A workflow with zero phases and zero unphased members still renders itself as a single `AgentRow` (fallback so the coordinator is never invisible) — `AgentsPanel.tsx:446-448`.
- Workflow open/closed state is pure presentation state (`useState`), independent of live/settled status: a live run starts expanded (`workflowIsLive`) and, per the file's design comment, **stays expanded when it settles** rather than snapping shut — `AgentsPanel.tsx:193-201,500-521`.
- "Direct spawns" section header (non-workflow subagents spawned directly in the thread) — `AgentsPanel.tsx:557-566`.

### Agent row (`AgentRow`)
- Fixed 3-line grid layout (identity / activity / metadata) with a fixed height (`h-[3.875rem]`) so changing data never reflows row height — `AgentsPanel.tsx:141-190` (design rule stated in file header comment, `AgentsPanel.tsx:6-9`).
- Line 1: status dot, truncated title, optional role chip (only shown if role differs from title, case-insensitively) — `AgentsPanel.tsx:158-168`.
- Line 1 right: live elapsed timer (DOM-write ticking every 1s via `setInterval`, zero React re-renders per tick — `AgentElapsed`, `AgentsPanel.tsx:87-114`) that freezes at `completedAt` once settled; a green `Check` icon appended only when status is `completed` — `AgentsPanel.tsx:169-176`.
- Line 2: activity text — for live agents: `progress` → `▸ {lastToolName}` → `result` → `error`, in that priority; for settled agents: `error` → `result` → `progress` → `▸ {lastToolName}` (errors lead only when failed, so a red row explains itself at a glance); falls back to the status label; rendered in destructive color when `status === "failed"` — `AgentsPanel.tsx:121-138,177-184`.
- Line 3: metadata string joined by `" · "`: compact model label (`formatSubagentModelLabel`, e.g. `sonnet-5 · high`, vendor prefix/date-suffix stripped — `subagentRuntime.ts:918-930`), token count (`formatSubagentTokenCount`, e.g. `9.7M tok`, or `"— tok"` if no usage yet), tool-call count (`"{n} tools"`), and `"run {n}"` when `activationCount > 1` (reactivation/retry counter) — `AgentsPanel.tsx:149-154,185-187`.
- Status → visual mapping (`STATUS_VISUALS`, `AgentsPanel.tsx:39-50`):
  - `pending`/`running`/`waiting` → info-blue dot, label "Working" (all in-flight states collapse to one steady "Working" presentation by design — file header comment, lines 33-38).
  - `idle` → muted dot, label **"Idle · resumable"** (deliberately reads as settled/muted, not sky-blue, per a live-test finding cited in-code: "sky idle dots read as stuck in-progress" — `AgentsPanel.tsx:43-45`).
  - `completed` → success-green dot, "Completed".
  - `failed` → destructive-red dot, "Failed".
  - `cancelled`/`interrupted` → muted dot, "Stopped".
- Explicitly documented as **non-interactive**: "Flat, non-interactive agent status line. No unfold." — `AgentsPanel.tsx:140`. **No click handler, no expand affordance, no context menu, no kill/stop/retry/resume button anywhere in `AgentRow`.**
- Screen-reader-only duplicate of the status label (`sr-only`) for accessibility — `AgentsPanel.tsx:188`.

### Footer
- Sticky footer bar below the scroll area: `"{running+waiting} working"` (info color, hidden if zero) · `"{idleCount} idle"` (hidden if zero) · `"{settledCount} settled"` (hidden if zero), right-aligned `"Σ {totalTokens} tok"` — `AgentsPanel.tsx:569-580`. Matches the screenshot's `"6 working · 94 settled · Σ 9.7M tok"`.

### Interactions — confirmed present
- Click a collapsed run row → expands it (`CollapsedWorkflowSection` button, `onExpand`) — `AgentsPanel.tsx:478-495`.
- Click the `ChevronDown` on an expanded workflow header → collapses it (`onCollapse`) — `AgentsPanel.tsx:422-429`.
- Click a phase-group header → toggles that phase's member list open/closed — `AgentsPanel.tsx:336-373`.
- Click the `{} script` chip → toggles the inline read-only script viewer; click its `X` → closes it — `AgentsPanel.tsx:406-418,286-294`.
- Click an in-chat "Kicked off N subagents" / "Ran N subagents" CTA row (`AgentSpawnCtaRow`, one per spawn batch, in `MessagesTimeline.tsx:2130-2215`) → calls `onOpenAgents` → opens/activates the Agents surface (`addAgentsSurface`, `ChatView.tsx:3298-3301,6305`). This CTA shows its own live/settled summary (lead text, phase-in-progress or working count, failed count or "✓ completed", token sum, "Open Agents ▸"/"View ▸") but is a separate component from the sidebar.
- Toggle the right panel open/closed via the `PanelRightIcon` button or `rightPanel.toggle` keybinding.
- Toggle panel maximize/restore via `RightPanelMaximizeControl` or `rightPanel.toggleMaximized` keybinding.
- Tab-strip level: close/close-others/close-to-right/close-all via right-click context menu on the Agents tab; close via the tab's hover-X or middle-click.
- Keyboard shortcut **A** opens the Agents surface from the empty-state launcher (only when the panel has no surfaces yet).

### Interactions — explicitly NOT found (do not assume)
- **No kill/stop/cancel affordance** for a running agent or workflow anywhere in `AgentsPanel.tsx` or `subagentRuntime.ts`.
- **No retry/resume button** — `activationCount`/"run N" is a read-only counter, not a control; "Idle · resumable" is a status *label* only, no resume action is wired in this panel.
- **No copy-to-clipboard** affordance on any row (title, IDs, tokens, etc.).
- **No "open transcript" / "jump to file" / "view full output" link** — settled rows show only a bounded (180-char) `result`/`error` string (`bounded()`, `subagentRuntime.ts:123-125`); there is no way to see the full transcript from this panel. (`runHandles.transcriptDir`/`sessionUrl` exist in the data model but are not rendered or linked anywhere in `AgentsPanel.tsx`.)
- **No per-agent-row context menu.**
- **No filtering, sorting, or search control** in the panel UI — ordering is fixed: workflows sorted by `firstSeenAt` then `id`; phase members sorted by `agentIndex`; direct agents sorted by `firstSeenAt` then `id` (`subagentRuntime.ts:741,786,818,845-847`). No user-facing sort/filter/search affordance exists.
- **No settings/preferences UI** specific to the Agents panel (no configurable roster limit, no toggle for auto-expand behavior, etc.) — the `ROSTER_LIMIT = 100` and retention/ranking rule are hardcoded constants (`subagentRuntime.ts:109,663-672`).
- **No toast/desktop notification** tied to agent/workflow completion or failure found anywhere in `ChatView.tsx` or the agents data path (searched for `toast(...)` near agent/subagent/workflow context — no matches).
- **No drag-to-reorder** of runs or agents.

### Data & persistence
- The roster is **not a separate live feed** — it's a pure fold (`foldSubagentActivities`, `subagentRuntime.ts:459-675`) over `activeThread.activities`, a `ReadonlyArray<OrchestrationThreadActivity>` that is part of the server-synced `OrchestrationThread` object (`packages/contracts/src/orchestration.ts:419`, field `activities`). Because thread activities are part of the shared/synced read model, the roster **does survive app restart / reconnect** — it rebuilds from server-persisted `task.started` / `task.progress` / `task.updated` / `task.completed` / `tool.progress` activity rows, not from any client-only cache.
- Recomputed via `useMemo` keyed on `[agentSessionLive, threadActivities]` in `ChatView.tsx:2225-2231` — i.e., it's a derived view, recomputed whenever the activity list identity changes (new activities arrive over the live connection) or connection liveness (`agentSessionLive = phase !== "disconnected"`) changes.
- `sessionLive: false` (session disconnected) forces every still-"active" agent to `interrupted` client-side, mirroring server-side liveness-registry clearing on `session.exited`, so a dead session can't leave the panel showing agents "Working" forever — `subagentRuntime.ts:651-661`.
- Historical vs. live distinction is *not* a separate data source: the same fold handles both; "live" is simply `!isTerminalSubagentStatus`/`workflowIsLive` computed from status at render/derive time (`subagentRuntime.ts:97-105`, `AgentsPanel.tsx:193-201`).
- `v2Projection` hook exists in `deriveAgentPanelModel` for a future orchestration-v2 subagent projection to fully replace this legacy fold (source precedence: v2 wins outright, never merged with the v1 fold) — currently always `null`/unused in `ChatView.tsx` (`subagentRuntime.ts:1-18,726-736`).
- Roster cap: **100 agents** (`ROSTER_LIMIT`); when exceeded, retained agents are ranked live-first, then idle, then most-recently-updated settled, and the rest are dropped from the derived roster (not from the server) — `subagentRuntime.ts:109,663-672`.
- Recent-activity ring buffer per agent, capped at 6 entries, deduped on consecutive identical summaries, each summary bounded to 180 chars (`SubagentActivityEntry`, `recentActivity` field) — `subagentRuntime.ts:42-45,107-139`. **Not rendered anywhere in `AgentsPanel.tsx`** — the field exists in the model but only `progress`/`lastToolName`/`result`/`error` (single latest values) are shown, not the history list.
- Which activity rows even count as "agent" rows (vs. ordinary work-log rows) is decided **once, server-side**, via a persisted `agentKind: "agent"` stamp at ingestion (`isBackgroundTaskActivity`, `subagentRuntime.ts:111-121`); legacy/pre-stamp servers fall back to treating everything as background (i.e., such threads show no Agents surface content for old runs).
- Right-panel open/closed state, active tab, and the ordered surface list (including whether an "agents" tab exists for a thread) persist to `localStorage` under key `t3code:right-panel-state:v2` (zustand `persist` middleware, schema version 11 with a migration function) — `rightPanelStore.ts:67-71,363-671`. This persists *which tabs are open*, not agent data itself.
- Workflow member/phase/coordinator status reconciliation: if a workflow coordinator reaches a terminal state, any member that never got its own terminal row is force-settled (`completed`→member `completed`, else member `interrupted`) so nothing reads as stuck forever — `subagentRuntime.ts:629-649`.
- Usage merge strategy is provider-aware: field-wise max-merge across repeated frames (handles both Codex-style cumulative frames and Claude-style per-task cumulative usage) so totals never shrink or double count — `subagentRuntime.ts:182-226`.

### Misc / model details not visually obvious
- Three `RuntimeSubagent["kind"]` values: `"subagent"`, `"workflow"`, `"workflow_agent"` — classified from payload (`taskType === "local_workflow"` → workflow; `parentAgentId` present or id contains `:wf:` → workflow_agent) — `subagentRuntime.ts:258-269`.
- A "reactivation" (retry) is detected purely from status transitions (terminal/idle → running/pending) and increments `activationCount`, clearing prior `result`/`error`/`completedAt` so a retried agent's card doesn't show stale prior-run output — `subagentRuntime.ts:394-420`.
- Workflow coordinators with members are excluded from the footer's running/settled/token counts (to avoid double-counting containers vs. their members) — `subagentRuntime.ts:828-839`, same rule repeated for the in-chat CTA row's token sum (`MessagesTimeline.tsx:2166-2171`) and the collapsed-row token sum (`AgentsPanel.tsx:466-471`).
- `workflowCardMembers` helper (urgency ordering: failed → running → waiting → most-recently-updated, capped with an overflow count) exists in `subagentRuntime.ts:862-880` but **is not called anywhere in `AgentsPanel.tsx`** — it appears to be for a different, more compact card surface (possibly the in-chat CTA or another embed) rather than the sidebar itself. Worth confirming its actual call site before assuming the sidebar caps members per phase (evidence here says the sidebar phase sections render the full member list, uncapped).
- Format helpers: `formatSubagentModelLabel` strips `claude-` prefix, trailing 8-digit dates, and `-latest` suffix, then appends `· {effort}` if present (`subagentRuntime.ts:918-930`); `formatSubagentTokenCount` renders `<1000` raw, `<1M` as `{n}k` (100+ rounds to integer, else 1 decimal), `≥1M` as `{n.n}M` (`subagentRuntime.ts:932-941`).

### Not found — flagged explicitly so nothing is assumed
- No evidence of multi-select, bulk actions, export, or "copy as markdown" for runs/agents.
- No evidence of a dedicated "Agents" top-level app section/route outside the per-thread right-panel surface — it is thread-scoped only (`singletonSurface`, one "agents" surface per thread in `rightPanelStore.ts:65,137-139`), not a cross-thread aggregate view. The screenshot's multiple run rows are multiple **workflow runs within one thread's activity history**, not multiple threads.
- No evidence of desktop notifications, sound, or badge-on-app-icon tied to agent state (only the in-app `liveAgentCount` badges on the panel toggle and empty-state launcher card were found).
- No evidence this panel calls any RPC to cancel/kill a task — the only outbound RPC found from this surface is `getWorkflowScript` (read-only).
- Did not check `apps/mobile/src/widgets/AgentActivity.tsx` in depth (a different, mobile-only widget) — flagging it exists in case a parallel mobile surface needs separate inventory; it was out of scope for this desktop/web screenshot."