# Right-panel surface picker — t3code mechanism, Buzz inventory, Buzz design

_Read-only study, 2026-08-20. t3code paths relative to `/Users/brian/Projects/t3code/t3code`,
Buzz paths to `/Users/brian/Projects/buzz`. Every claim carries `file:line`. Things I could **not**
find evidence for are marked **NOT FOUND** rather than guessed._

> **Implementation decision, 2026-08-21.** Agent Progress remains a global
> `/agent-progress` route. Its question spans every coding session the viewer
> can read, while `CodingSessionSurfaceHost` is scoped to one session; placing
> the global list inside that host would make its scope depend on an unrelated
> active session. This decision does not choose between one or N open
> session-host tabs. The registry study below remains useful for session-local
> surfaces, but its C1 recommendation does not govern the global Agent Progress
> screen. Liveness also no longer uses the 30-minute metadata rule; see C3's
> lease-backed correction.

---

## Part A — how t3code actually does it

### A1. There is no surface registry

The headline structural finding: **t3code has no surface registry.** The six surfaces are hand-written
across eight sites kept in sync by hand.

| Concern | Where |
|---|---|
| Kind union (closest thing to a table) | `apps/web/src/rightPanelStore.ts:17-26` — `RIGHT_PANEL_KINDS`, 7 kinds |
| Per-kind surface shape | `rightPanelStore.ts:28-65` — discriminated union, one arm per kind |
| Card grid (label/description/icon/shortcut/badge/available) | `components/RightPanelTabs.tsx:170-235` — a **literal `actions` array inside the component** |
| `+` dropdown | `RightPanelTabs.tsx:683-748` — six hand-written `<SurfaceMenuItem>` blocks, in a *different order* from the grid |
| Availability + open callbacks | 6 `*Available` booleans + 6 `onAdd*` props, `RightPanelTabs.tsx:56-72` |
| Rendered body | `components/ChatView.tsx:6085-6193` — one ternary chain on `surface.kind` |
| Tab title / tab icon | `RightPanelTabs.tsx:394-441` / `:460-513` — two switches |

`find apps/web/src -iname "*surface*"` yields only browser/terminal/auth surfaces; no registry module
exists. **The UX is worth copying; the factoring is not** — a seventh surface means editing eight sites
plus two reason tables.

### A2. Singleton vs numbered instances

- Singletons `diff` / `files` / `agents`: id equals the kind (`rightPanelStore.ts:127-138`). A second
  Agents click does **not** add a tab — `upsertSurface` (`:206-214`) finds the existing id and only sets
  `activeSurfaceId`. Tested `rightPanelStore.test.ts:230`.
- Terminals are numbered **client-side**: `addTerminalSurface` (`ChatView.tsx:3349-3357`) calls
  `nextTerminalId(...)` (`packages/shared/src/terminalLabels.ts:33-40`), lowest unused `term-N` from
  `term-1`; the server never allocates (`:25-32`). "Terminal 1" is a *rendering* of the id by regex, not
  a counter (`getTerminalLabel`, `:4-11`). Surface id `terminal:${id}` (`rightPanelStore.ts:152-158`).
- Browser: one per preview tab, `browser:${tabId}`, plus a `browser:new` placeholder (`:139-143`).
  Pull request: keyed by reference (`:163-176`), so two PRs are two peer tabs.

### A3. The activity badge

- Five cards hardcode `badgeCount: 0` (`RightPanelTabs.tsx:183,193,203,213,223`); only Agents gets
  `badgeCount: props.liveAgentCount` (`:233`). Rendering is generic (`:311-327`), but **in practice the
  mechanism is special-cased to Agents.**
- Source: `liveAgentCount={agentPanelModel.liveCount}` (`ChatView.tsx:6650,6689`) ←
  `deriveAgentPanelModel` (`ChatView.tsx:2225-2231`); `liveCount = runningCount + waitingCount`
  (`packages/client-runtime/src/state/subagentRuntime.ts:854`), skipping workflow coordinators that have
  members so they are not double-counted (`:828-838`).
- The same count badges the **panel toggle** (`components/chat/PanelLayoutControls.tsx:62-99`) and is
  **suppressed while the Agents surface is visible** (`ChatView.tsx:6057-6061`: "the roster itself is on
  screen, so the toggle badge would be pointing at nothing"). Copy this instinct.
- **NOT FOUND: any freshness gate on `liveCount`.** A `running` agent whose provider died still counts;
  the only backstop is a socket-state session-death sweep (`subagentRuntime.ts:651-661`). This is the
  defect `docs/AGENT_PROGRESS_UI_DESIGN_NOTE.md:314-322` tells Buzz not to copy.
- Separate per-tab dot for unsaved file edits: `pendingSurfaceIds` (`RightPanelTabs.tsx:655-660`, fed
  `ChatView.tsx:1744-1748`). Boolean, not a count.

### A4. The disabled card ("Pull request")

- Availability is a plain boolean computed by the host:
  `pullRequestSurfaceAvailable = supportsPullRequests && activeThreadPr !== null && threadRepository !== null`
  (`ChatView.tsx:4143-4144`), `activeThreadPr` from `resolveDisplayedThreadPr` over git status + a
  change-request snapshot (`:4131-4136`). The other five: `ChatView.tsx:6643-6648`.
- **Two reason tables, deliberately.** `SURFACE_UNAVAILABLE_HINTS` (`RightPanelTabs.tsx:106-113`) is the
  short line that **replaces the card's description** — "No pull request on this branch yet." swaps in
  for "Open this branch's pull request." (`:217` vs `:222`, rendered `:377-395`).
  `SURFACE_DISABLED_REASONS` (`:84-91`) is the longer sentence shown as a **tooltip on the disabled `+`
  menu item**; `SurfaceMenuItem` keeps `pointer-events-auto` on the disabled item so the tooltip still
  fires (`:127-144`, esp. `:131`).
- A disabled card renders as a `<div>` at `opacity-40` — still visible, still showing its shortcut,
  never hidden (`:377-395`) — and is excluded from keyboard nav (`availableActions`, `:238`).

### A5. Tab lifecycle

All opens funnel through `upsertSurface` (`rightPanelStore.ts:206-214`): append, activate.
Close (`:507-524`) activates `surfaces[min(index, len-1)]`; closing the last sets `isOpen:false`.
Close-others / to-right / all at `:525-561`, also reachable from a **native context menu** on a tab
(`RightPanelTabs.tsx:522-575`) with per-item `disabled` predicates (`:544-563`) and "Copy path" for file
tabs (`:539-541`). Middle-click closes (`:577-590`, with `onMouseDown` swallowing button 1 to kill
autoscroll). **The close button is the icon** — it swaps to an X on tab hover (`:642-670`). Order is
insertion order; **NOT FOUND: any drag-reorder**. Overflow is a horizontal `ScrollArea`
(`hideScrollbars scrollFade`, `:617-624`) with the active tab auto-scrolled into view (`:593-596`).
Two reconcilers keep tabs honest against the outside world: `reconcileBrowserSurfaces` (`:562-590`)
drops tabs whose preview session vanished, `reconcileFileSurfaces` (`:591-611`) drops file tabs when the
workspace is gone; both re-pick an active tab.

### A6. Persistence

zustand `persist`, key `t3code:right-panel-state:v2` (`rightPanelStore.ts:66`), **schema version 11**
(`:70`), `createJSONStorage(resolveStorage(localStorage))` with a memory fallback (`:658-660`,
`lib/storage.ts:37-39`). Note the split: the key is a frozen namespace (`v2`), the *version* drives
migration; notes inline at `:67-69`. `migratePersistedRightPanelState` (`:236-361`) is exported and
unit-tested directly (`rightPanelStore.test.ts:24-214`); it drops removed kinds, repairs terminal split
state, and **refuses to reopen an empty panel** when migration removed everything (`:344-355`).
Scope is `byThreadKey`, key `${environmentId}:${threadId}`
(`packages/client-runtime/src/environment/scoped.ts:25-36`) — per thread per environment, not global;
`partialize` excludes the PR-list panel (`:661-667`) and empty threads are GC'd (`:222-234`). Persisted:
`isOpen`, `activeSurfaceId`, `surfaces[]` (`:78-82`). **Width persists separately** in localStorage
`t3code:preview-panel-width` (`components/preview/PreviewPanelShell.tsx:18`, default 540 `:26`, min 360
`:19`), written on drag-end only (`hooks/useResizableWidth.ts:29-36`), key overridable by the caller
(`routes/_chat.pull-requests.tsx:1548`). **Maximized is NOT persisted** (`ChatView.tsx:1358-1360`).

### A7. Keyboard

Letter shortcuts are **not** focus-scoped: a capture-phase `window` keydown listener runs while the
launcher is mounted (`RightPanelTabs.tsx:246-273`), guarded against modifiers, `defaultPrevented`, a
blocking-overlay list (`:93-104` — dialog/menu/select/popover/combobox/autocomplete), and
`input/textarea/select`; the nice detail is that an **empty** contenteditable does not count as typing,
so the resting composer does not steal the letters (`:260-264`). The app's own type-to-focus-composer
handler defers by reading `data-surface-launcher-keys` off the DOM (`ChatView.tsx:490-497`; attribute
set `RightPanelTabs.tsx:344`). Arrows + Enter navigate only while the container is focused (`:275-302`);
highlight starts at `-1` so nothing is highlighted until hover/arrow (`:169-170`); Enter is ignored when
a card button already has focus (`:293-295`); the container auto-focuses via a stable callback ref
(`:305-307`). Panel level: `rightPanel.toggle` = `mod+alt+b` (`packages/shared/src/keybindings.ts:24`,
handled `ChatView.tsx:4735-4740`); `rightPanel.toggleMaximized` exists as a command
(`packages/contracts/src/keybindings.ts:57-58`, handled `:4742-4747`) but has **no default binding**
(`apps/server/src/keybindings.test.ts:206`).

### A8. Layout

Docked vs overlay is a media query, not a preference:
`RIGHT_PANEL_INLINE_LAYOUT_MEDIA_QUERY = "(max-width: 980px)"` (`apps/web/src/rightPanelLayout.ts:1`,
used `ChatView.tsx:1381`); below it the same `<RightPanelTabs>` renders inside `<RightPanelSheet>`
(`ChatView.tsx:6655-6693`). Maximize collapses the chat column to `w-0` (`:6209-6211`), inline only
(`:1657-1659`). Width is clamped against a 360px sibling minimum and a fraction cap
(`PreviewPanelShell.tsx:34-45`); the resize handle exists only inline and non-maximized (`:95`).

---

## Part B — what Buzz has today

### B1. Buzz already has a surface host that calls itself a registry

`desktop/src/features/coding-sessions/ui/CodingSessionSurfaceHost.tsx:12-25` declares
`CodingSessionSurfaceDescriptor` and its own doc comment (`:12-18`) says it is
*"the extensibility seam for future surfaces (Browser, Terminal, Files)"*:

```ts
export type CodingSessionSurfaceDescriptor = {
  id: string; label: string; count?: number | null; content: React.ReactNode;
};                                    // CodingSessionSurfaceHost.tsx:19-25
```

Already built around it:
- `reconcileCodingSessionSurfaceTab` (`:41-54`) — pure: requested → last → first → closed.
- `useCodingSessionSurfaceHostState` (`:75-107`) — "closed or one tab", with render-phase
  reconciliation so a stale tab id never survives a frame (`:93-97`).
- `nextCodingSessionSurfaceTabIndex` (`:57-68`) — roving tabindex, Arrow/Home/End.
- `CodingSessionSurfaceHostLayout = "inline" | "sheet" | null` (`:32`) — Buzz already has t3code's
  docked-vs-sheet split, plus a third `null` "not measured yet" state t3code lacks.
- Its own in-panel tab strip (`role="tablist"`, `:243-288`) with count pill (`:281-285`) and close (`:289-299`).
- Persisted, resizable width: localStorage `buzz.desktop.coding-session-rail-width`
  (`useCodingSessionRailWidth.ts:12`), default 360 / min 288 / max 720 (`:5-8`), written `:133,:168`.
- Unit tests: `CodingSessionSurfaceHost.test.mjs`.

### B2. Only two surfaces exist, and Brian's screenshot row is three different things

Two **duplicated** `surfaces` arrays build the registry:
`CodingSessionWorkspace.tsx:445-465` (Agents only when `executions.length > 0`, `:447` — "never an
empty tab") and `CodingSessionUmbrellaWorkspace.tsx:141-157` (Agents unconditional).
Which branch renders is decided at `CodingSessionWorkspace.tsx:242` (`executions.length > 1`) —
**the screenshot is the single-session branch**, the only one that passes `onToggleTaskRail` (`:551-554`).

The rest of the row is hand-written `<Button>`s in one flat `<header>` (`CodingSessionHeader.tsx:126-418`):

| Button | Line | Source |
|---|---|---|
| Plan | `CodingSessionHeader.tsx:260-288` | hardcoded; count = `taskModel?.tasks.length ?? 0` (`CodingSessionWorkspace.tsx:573`) |
| Agents N / Observed changes N | `:289-322` (`surfaceTabs.map`) | **the registry**; counts = `umbrella.executions.length` (`CodingSessionWorkspace.tsx:452`) and `changedFiles.length` (`:438-441`) |
| People N | `:323-350` | hardcoded; count = `rosterQuery.data?.length ?? 0` (`CodingSessionWorkspace.tsx:236`), query `codingSessionRoster.ts:432-451`, `staleTime: 15_000` |
| Add provider / Close / Reopen / Export / Pop out | `:351-417` | all hardcoded, callback-conditional |

Adding a third surface today touches **five** places: both `surfaces` arrays, both icon ternaries
(`CodingSessionWorkspace.tsx:566-572`, `CodingSessionUmbrellaWorkspace.tsx:207-213`), and the **closed
icon union** `icon: "agents" | "changes"` (`CodingSessionHeader.tsx:27-33`, branched `:306`).
**The descriptor has no icon field.** That is the concrete debt.

**No persistence of the active tab.** `useCodingSessionSurfaceHostState` is `React.useState`
(`CodingSessionSurfaceHost.tsx:84-87`); umbrella hardcodes `initialTab: "agents"`
(`CodingSessionUmbrellaWorkspace.tsx:163-165`), single-session opens closed
(`CodingSessionWorkspace.tsx:470`). What *is* persisted: panel width (above) and the Plan rail's
open/closed flag in **sessionStorage** under `buzz.coding-session.task-rail:<channelId>:<generationId>`
(`CodingSessionTaskRail.tsx:23-33`, read/write `CodingSessionWorkspace.tsx:767-785`). Surface and Plan
rail are made mutually exclusive by hand (`CodingSessionWorkspace.tsx:551-558`).

**No badge / unread / activity mechanism anywhere on that row** — only plain numeric count pills and
a session-status dot whose comment explicitly says it must not read as unread
(`CodingSessionHeader.tsx:200-205`).

### B3. There is a *second*, app-level panel chassis — and candidate surfaces are routes, not panels

`shared/layout/AuxiliaryPanelShell.tsx` (165) + `AuxiliaryPanelHeader.tsx` (391) +
`AuxiliaryPanelBody.tsx` (46) + `auxiliaryPanelContext.ts` (52) is the app-level chassis, used by
`messages/ui/MessageThreadPanel.tsx:974-992`, `profile/ui/UserProfilePanelFrame.tsx:54-103`,
`channels/ui/AgentSessionThreadPanel.tsx:470-511`, `channels/ui/ChannelManagementAuxiliaryPanel.tsx:24`.
**`CodingSessionSurfaceHost` does not use it** — it hand-rolls its own `<aside>` (`:174-207`). Two
parallel chassis exist; a picker must not create a third. Those panels carry open/close state in **URL
search params**, not storage (`app/navigation/useAppNavigation.ts:89,213,226,232`;
`app/AppProfilePanelProvider.tsx:10-15`), with width in sessionStorage
(`shared/hooks/useThreadPanelWidth.ts:8`).

Candidate surfaces that exist today, all currently **routes**:
- **Agent progress** (new this branch) — `features/agent-progress/ui/AgentProgressPanel.tsx` (78) is
  already purely presentational (doc `:10-19`), all state from `foldAgentProgress`
  (`lib/agentProgressFold.ts`); route `app/routes/agent-progress.tsx:14-25`.
- **Project Pulse** — `features/project-pulse/ui/ProjectPulseView.tsx` (614); route
  `app/routes/projects.$projectId.pulse.tsx:12-32`, real `<FeatureGate feature="project-pulse">` at `:27`.
- **Built-in shell (NIP-ST)** — `features/builtin-shell/ui/*`; route `app/routes/shell.$sessionId.tsx:5-12`;
  kinds `KIND_SHELL_SESSION 30623`, `KIND_SHELL_WATCH 24310`, `KIND_SHELL_FRAME 24311`,
  `KIND_SHELL_INPUT 24312` (`crates/buzz-core/src/kind.rs:504-520,822`).
- **Terminal substrate** (separate) — `features/terminal/terminalPanelStore.ts:3-52`, a module-level
  `useSyncExternalStore` singleton (`"closed"|"docked"|"maximized"`), **not persisted**; dock height in
  localStorage `buzz-terminal-dock-height` (`TerminalSubstrate.tsx:143,523,584`).
- **Repo browser** — route `app/routes/projects.$projectId.code.$repoId.tsx`, picker state in URL params
  (`:15-29`). **NOT FOUND: any files, diff, or pull-request side surface.** `changes` is the only
  diff-shaped surface; there is no workspace file browser.

`docs/AGENT_PROGRESS_UI_DESIGN_NOTE.md:147` reaches the same conclusion from the other direction:
*"buzz has no right-panel tab-strip abstraction to generalise yet"* — true app-level; B1 shows one
exists session-level.

### B4. Conventions a picker must obey

- **There is no zustand.** Not in `desktop/package.json` or the root; zero imports in `desktop/src`.
  **A picker must not reach for `persist`.** House patterns: module-level singleton +
  `useSyncExternalStore` (`features/terminal/terminalPanelStore.ts:10-52`,
  `shared/features/useFeatureEnabled.ts:6-53`), TanStack Query for relay-derived data, URL params for
  open/closed, raw storage behind `shared/lib/safeStorage.ts:42,61`. Closest precedent for a persisted
  picker selection: `buzz.projects.filter` / `buzz.projects.sort`
  (`features/projects/lib/projectsViewHelpers.ts:60,65`).
- **Preview flags**: manifest at repo root `preview-features.json`; `defaultEnabled` is omitted by all 9
  current flags ⇒ off (`shared/features/manifest.ts:16`, `useFeatureEnabled.ts:105`). **A flag absent
  from the manifest fails open to `true`** (`useFeatureEnabled.ts:102`) — forgetting to declare it ships
  the feature. `FeatureGate` (`shared/features/FeatureGate.tsx:21-28`) actually gates;
  `usePreviewFeatureWarning` (`:140-159`) only toasts. `project-pulse` is the strongest pattern, gated in
  three places (route `:27`, sidebar `ProjectSidebarGroup.tsx:174`, home card
  `ProjectContainerScreen.tsx:411-427`).
- **Routes** are declared in `desktop/src/app/routes.ts` (36 ln, virtual-file-routes); `routeTree.gen.ts`
  is generated. Recipe: one line in `routes.ts`, a `createFileRoute` file, `React.lazy` + `Suspense` +
  `ViewLoadingFallback`. Gotcha `routes.ts:11-13`: flat multi-segment param paths are silently dropped.
- **Community reset**: `useCommunityInit.ts:56-98`, 22 entries. Registration shape (canonical example
  `resetProjectPulseState`): module singleton + exported `reset*` in the feature's `lib/`
  (`project-pulse/lib/projectPulseCache.ts:16,42-44`), re-export from the barrel (`index.ts:9`), import
  (`useCommunityInit.ts:40`), call with a justifying comment (`:94-97`). Effect-cleanup-managed
  singletons are exempt and say so (`:51-53`).
- **The 1000-line gate is a ratchet, not a cap**: `scripts/check-file-sizes-core.mjs:31-33` — a file
  already over 1000 may hold but not grow; one under may not cross. Roots at
  `desktop/scripts/check-file-sizes.mjs:10-53` (note `src/shared/layout`, `src/shared/hooks`,
  `src/shared/features` are **not** guarded). Pressure points: `CodingSessionWorkspace.tsx` **827**,
  `CodingSessionUmbrellaWorkspace.tsx` **788**, `CodingSessionHeader.tsx` 447,
  `CodingSessionSurfaceHost.tsx` 313. Elsewhere `UserProfilePanel.tsx` **998**,
  `MessageThreadPanel.tsx` **995**, `AppSidebar.tsx` **993**, `AppShell.tsx` **953** — which is exactly
  why the agent-progress sidebar entry was written self-navigating rather than prop-threaded
  (`sidebar/ui/AppSidebarPinnedHeader.tsx:95-100`).
- **rem-only text**: `desktop/scripts/check-px-text.mjs`, allowlist is 4 avatar glyphs (`:27-30`).
  t3code's badge uses `text-[9px]` (`RightPanelTabs.tsx:317`) — **that literal cannot be ported**; use
  `text-3xs`, as the host already does with `text-2xs` (`CodingSessionSurfaceHost.tsx:282`).

---

## Part C — the design for Buzz

### C1. Where it belongs: inside the coding-session workspace first

Not an app-level right panel. (1) Every real candidate is scoped to a **coding session or a project**,
not to the app — Buzz's left rail is already the app-level navigator. (2) t3code's panel is
thread-scoped (`scoped.ts:25-36`), the analogue of a Buzz coding session, not of the app shell. (3) Buzz
already has the host (B1) with reconciliation, sheet fallback, roving tabindex and persisted width, so
the picker is the missing 20%; and a third chassis beside `AuxiliaryPanelShell` and
`CodingSessionSurfaceHost` (B3) would be the wrong move. App-level comes later, and only if Pulse or
agent-progress prove they want to sit beside something other than a session.

### C2. Surface registry sketch

Extract the descriptor out of the UI file into **`desktop/src/features/coding-sessions/lib/codingSessionSurfaces.ts`**
(new, pure, zero React imports, `.test.mjs` beside it in the `pulseFold.ts` style), and widen it:

```ts
export type SessionSurfaceKind =
  | "agents" | "changes" | "plan" | "people" | "pulse" | "progress" | "shell";

export type SessionSurfaceDescriptor = {
  id: string;                       // instance id: "agents" | "shell:1"
  kind: SessionSurfaceKind;
  label: string;                    // "Agents", "Shell 1"
  description: string;              // picker card body when available
  icon: LucideIcon;                 // replaces the "agents"|"changes" union
  shortcut?: string;                // single letter, picker only
  multiInstance: boolean;           // shell yes; everything else no
  availability:
    | { state: "available" }
    | { state: "unavailable"; hint: string; reason: string }  // card line / menu tooltip
    | { state: "unreadable"; hint: string; reason: string };  // NEW — see C3
  badge: SurfaceBadge;
  content: React.ReactNode;
};
```

Real candidates, and what backs each today:

| kind | Backed by | Note |
|---|---|---|
| `agents` | `CodingSessionExecutionRail.tsx` (360) | already a surface |
| `changes` | `CodingSessionChangesRail.tsx` (139) | already a surface |
| `plan` | `CodingSessionTaskRail.tsx` (427) + `deriveCodingSessionTaskModel` | **migrate** — a surface wearing a toggle's clothes; retires the sessionStorage key |
| `people` | `CodingSessionPeoplePopover` + `useCodingSessionRoster` (`codingSessionRoster.ts:432-451`) | migrate; count already exists |
| `progress` | `agent-progress/ui/AgentProgressPanel.tsx` (78, presentational) | flag `agent-progress` |
| `pulse` | `project-pulse/ui/ProjectPulseView.tsx` (614) | project-scoped only; flag `project-pulse` |
| `shell` | `features/builtin-shell/`, kinds 30623 / 24310-24312 | **only multi-instance surface**; number `Shell N` mirroring `nextTerminalId` (t3code `terminalLabels.ts:33-40`) |
| `files`, `diff`, `pull request` | **NOT FOUND in Buzz** | do not put ghost cards in the picker |

Adding a surface then touches **one array**. `CodingSessionHeaderSurfaceTab.icon`
(`CodingSessionHeader.tsx:27-33`) collapses into `descriptor.icon` and both ternaries delete.

### C3. The activity-badge rule, as an honesty requirement

A badge asserts *"there is something here you are not looking at."* In this codebase a comfortable
guess is a bug (`CLAUDE.md`, "Honesty in the product"), so the badge type is not a number:

```ts
type SurfaceBadge =
  | { kind: "none" }
  | { kind: "count";   value: number; basis: string } // signed facts, freshness-gated
  | { kind: "partial"; value: number; basis: string } // a floor; the read was incomplete
  | { kind: "unknown"; reason: string };              // read failed — glyph, never a number
```

Per surface, the signed fact behind it:

- **`agents` / `progress`** — count = sessions in the shared coordination
  fold's `provider_reachable` state. That state requires a current,
  authority-bound kind-24223 lease; kind-44223 metadata is retained only as a
  historical report and its age never decides liveness. An open session with
  no valid lease is `open_unverified`, not absent and not working. Agent
  Progress's compact `Reachable` / `Unverified` / `Closed` words map 1:1 to
  Pulse's full register through
  `shared/coordination/sessionCoordinationFormat.ts`. This is the one place
  Buzz must not copy t3code, which has no independently verified liveness
  signal at all (A3).
- **`changes`** — count = `deriveCodingSessionChangedFiles` over the transcript
  (`CodingSessionUmbrellaWorkspace.tsx:113-125`), backed by signed 44225 items. Honest today.
- **`plan`** — count of *incomplete* tasks, not `tasks.length` (today's `taskCount`,
  `CodingSessionWorkspace.tsx:573`, is a total and would be a lying badge).
- **`people`** — membership is state, not activity. `{kind:"none"}` unless the roster changed since
  last view; and `useCodingSessionRoster` has `staleTime: 15_000` (`codingSessionRoster.ts:432-451`),
  so `isError`/`isPending` must map to `unknown`, never to `0`.
- **`pulse`** — count = entries newer than this surface's `lastSeenAt`. Pulse's digest already carries
  the exact envelope: `complete: sourceErrors.length === 0` plus a non-empty `errors[]` is *the only*
  representation of a partial read (`pulseFold.ts:141-151,558-565`). Map `complete:false` →
  `{kind:"partial"}`, never a clean number.
- **`shell`** — **NOT FOUND**: 24311 frames are ephemeral and never stored (`kind.rs:1531`), so
  "unread output" is not reconstructible from the log. Ship `{kind:"none"}` until a durable signal exists.

Three hard rules:

1. **Absence of a closure is never evidence of life** (`pulseFold.ts:297-311`; Pulse plan §3 decision 14).
2. **`complete:false` never renders as a clean count** — that is the "read error renders as an empty
   project" failure arriving by a side door (Pulse plan §5.4).
3. **Suppress a surface's badge while that surface is showing**, as t3code does for its panel toggle
   (`ChatView.tsx:6057-6061`). A count of things already on screen is noise.

### C4. Persistence

No zustand (B4). One module-level singleton store,
`desktop/src/features/coding-sessions/lib/codingSessionSurfaceStore.ts`, in the shape of
`features/terminal/terminalPanelStore.ts:10-52` (`useSyncExternalStore`) writing through
`shared/lib/safeStorage.ts:42,61`.

- Key `buzz.desktop.session-surfaces.v1` (dotted style of `buzz.desktop.coding-session-rail-width`,
  `useCodingSessionRailWidth.ts:12`), with a hand-written `migrate` on the `.v1` suffix. Borrow two
  rules from t3code's migration: drop unknown kinds, and **never reopen an empty panel** after a lossy
  migration (`rightPanelStore.ts:344-355`).
- Shape: `byKey`, key `${communityId}:${channelId}:${sessionRef ?? generationId}`. Persist
  `activeSurfaceId`, `openSurfaceIds[]`, and `lastSeenAt` per surface (the input to the badge).
  Content is rebuilt, never stored.
- localStorage vs sessionStorage: use **localStorage** — the Plan rail's sessionStorage choice
  (`CodingSessionTaskRail.tsx:23-24`) means a restart forgets which rail you had open, which is the
  behaviour we are trying to fix.
- Width stays where it is (`useCodingSessionRailWidth.ts:12`); do not fold it in.
- **`resetCodingSessionSurfaceState()` must be registered in `resetCommunityState`**
  (`useCommunityInit.ts:56-98`, beside `resetProjectPulseState()` at `:97`), following the four-step
  shape in B4. The persisted keys are community-scoped by construction, but the in-memory singleton is
  not and will otherwise leak across a switch.
- Do not persist maximize; t3code doesn't either (`ChatView.tsx:1358-1360`).

### C5. Slices, smallest-honest-first

**Slice 1 — the picker, no new event kind. Confirmed possible.** Every input already exists client-side
from kinds 44223 / 44225 / 44230 / 44240 that Pulse Slice 1 ships and `agentProgressFold.ts` folds.

1. `coding-sessions/lib/codingSessionSurfaces.ts` (new, ~150 ln, pure) — `SessionSurfaceDescriptor`,
   `SurfaceBadge`, `describeSessionSurfaces(input)`, `nextSurfaceInstanceId`. Plus `.test.mjs`.
2. `coding-sessions/ui/SessionSurfacePicker.tsx` (new, ~220 ln) — the empty-state card grid **and** the
   `+` dropdown, both rendered from the same array (t3code's two hand-kept lists is the bug not to
   port). Disabled cards keep their shortcut and swap description for `availability.hint`
   (`RightPanelTabs.tsx:377-395`). Badges use `text-3xs`, never `text-[9px]`.
3. `CodingSessionSurfaceHost.tsx` (313) — take the widened descriptor, render the picker when
   `activeTab === null`, add the `+` trigger beside the existing tab strip (`:243-288`). Leave
   `reconcileCodingSessionSurfaceTab` (`:41-54`) alone; it already does t3code's fallback correctly.
4. `CodingSessionHeader.tsx` (447) — delete the `icon` union (`:27-33`) and the inline branch (`:306`);
   read `descriptor.icon`.
5. `codingSessionSurfaceStore.ts` (new) + one import and one call in `useCommunityInit.ts` (near `:97`).
6. The two workspaces (827 / 788) lose their duplicated arrays and ternaries and **shrink**. The picker
   must not land inside them; the ratchet gives them ~173 and ~212 lines of headroom
   (`scripts/check-file-sizes-core.mjs:31-33`).

Gated by a new `session-surfaces` entry in `preview-features.json` — **it must be declared**, since an
undeclared flag fails open to `true` (`useFeatureEnabled.ts:102`) — and wrapped in `<FeatureGate>`
(`FeatureGate.tsx:21-28`), the real gate, not `usePreviewFeatureWarning`.

**Slice 2 — migrate Plan and People into the registry.** Delete their bespoke header buttons and the
`buzz.coding-session.task-rail:` sessionStorage preference. Add the keyboard model: letter shortcuts
using t3code's capture-phase + empty-contenteditable rule (`RightPanelTabs.tsx:246-273`), arrows/Enter,
and a panel toggle.

**Slice 3 — multi-instance `shell`**, tab context menu (close others / to right / all), middle-click
close, and reconcilers so a surface whose backing session died is dropped rather than left as a dead
tab (`rightPanelStore.ts:562-611`).

**Slice 4 — app-level panel**, only if C1's precondition is met; and if so, build it on
`AuxiliaryPanelShell` (B3), not a third chassis.

### C6. Open questions for Brian

1. **Session-scoped or project-scoped host?** Pulse is per project; agent-progress spans sessions.
   Two hosts, or one host whose registry varies by scope?
2. **Does Plan become a tab?** It gains registry uniformity but loses its always-visible toggle —
   plan visibility may be worth the exception.
3. **Should People be a surface at all**, or stay a popover? It is state, not activity.
4. **Is a `shell` badge worth durable state?** 24311 is ephemeral by design (`kind.rs:1531`); a real
   unread-output badge needs a wire change.
5. **One tab at a time, or many?** The host enforces exactly one today
   (`CodingSessionSurfaceHost.tsx:71-74`), which matches Brian's read of the intent; t3code stacks.
   Keeping one means the `+` menu *switches* rather than *adds* — a real divergence from the
   screenshots. Confirm which.
6. **Ship the `unknown` badge glyph in Slice 1**, or suppress the badge on a failed read? Suppression
   is quieter; the glyph is more honest and matches the five Pulse honesty states.
7. **New `session-surfaces` flag, or fold under `agent-progress`?**
8. **Do we take the chance to converge the two panel chassis** (`AuxiliaryPanelShell` vs the hand-rolled
   `<aside>` at `CodingSessionSurfaceHost.tsx:174-207`), or is that a separate cleanup?
