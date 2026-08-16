# Coding-session UI convergence handoff

**Audience:** Fable, Opus, or any agent taking final ownership of the coding-session UI cleanup.

**Written:** 2026-08-16

**Purpose:** Explain why Buzz currently has multiple coding-session rails, what was intentional, what is duplicated only in Git history, what remains incomplete, and the safest product and integration path to a clean final experience.

## Executive recommendation

Do **not** choose between the execution/agents rail and the changes rail. They are complementary data surfaces:

- **Agents / executions:** who is participating, which provider owns an execution, and its signed status/activity.
- **Observed changes:** which file changes were visible in transcript tool activity.

The correct product move is to put these surfaces behind **one collapsible, resizable right-side surface host**, modeled after T3 Code's tabbed side panel. Agents and Changes become sibling tabs in that host. Later Browser, Terminal, and Files can join the same host without changing the coding-session transcript contract.

The surface host should own:

- open/closed state;
- the active surface tab;
- width, resize behavior, and persistence;
- narrow-window sheet behavior;
- keyboard and ARIA tab behavior;
- a single header/tab strip and a single close control.

Individual surface components should own only their content. Do not let each rail independently recreate panel chrome, resizing, overlay behavior, or close controls.

## What happened

The transcript UI effort began on the historical worktree/branch:

- worktree: `/Users/brian/Projects/buzz-session-ui`
- branch: `feature/coding-sessions-ui`

That branch produced four relevant commits:

| Historical commit | Purpose |
| --- | --- |
| `30d7b5c4` | Coding-session changes rail |
| `5763460c` | Multi-agent execution rail |
| `a2e942a2` | Group coding sessions in project navigation |
| `fb2db9a1` | Collapsible project shelves |

The rail work was then moved onto the canonical feature branch with equivalent content:

| Canonical commit | Purpose |
| --- | --- |
| `50fd910a` | `feat(desktop): add coding session changes rail` |
| `931f3390` | `feat(desktop): add multi-agent session rail` |

The integrated assembly subsequently added the project-session shelf and integration glue. Current visual/integration truth is:

- branch/worktree: `integrated-build` at `46bb137b`
- tag: `build/2026-08-16.3`
- integration branch at the same commit: `integration/glue`

Important integrated commits include:

- `57029992` — group coding sessions in project navigation;
- `4305e7e6` — collapsible project shelves;
- `8fd26c1e` — hide transport channels and render one shelf row per umbrella;
- `6d8cf307` — project shelf reads with channel-membership authority;
- `d5db96ca` — ended sessions leave the active sidebar shelf;
- `46bb137b` — prefer active umbrella executions.

Therefore:

1. The two rails are not competing implementations.
2. The duplicated rail code across branches is historical duplication, not a product fork.
3. `feature/coding-sessions-ui` should not be merged into the current assembly.
4. Do not delete that worktree until its untracked design artifacts have been inspected and preserved deliberately.
5. Use `integrated-build` as the current end-to-end behavior reference, while respecting the repository's feature/glue integration ceremony for any new commits.

## Original product intent

The target was T3 Code's legibility, not decoration:

- Agent activity is a readable narrative, not a protocol log.
- Recent meaningful actions remain visible.
- Repetitive older tool calls collapse into counts.
- Plans and progress appear inline with work.
- Errors and approvals remain visible.
- Diffs are progressively disclosed rather than dumped into the transcript.
- A busy multi-tool turn remains scannable.
- A right-side work surface can show Agents, Diff, Browser, Terminal, or Files as tabs.
- The right surface is collapsible and resizable.

Buzz-specific intent added during the work:

- A session is an umbrella that can contain multiple provider executions.
- Claude and Codex executions should coexist in one consolidated session surface.
- The user should be able to view all activity or focus on one execution.
- Project navigation should show sessions separately from channels and other project children.
- Session shelf rows should be consolidated per umbrella, not duplicated per execution.
- Signed authority/founder/genesis/goal facts remain read-only inputs to the UI.
- The UI must not invent a subagent tree from tool-call text.

## The two current rails

### Execution / agents rail

Primary files:

- `desktop/src/features/coding-sessions/ui/CodingSessionExecutionRail.tsx`
- `desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx`
- `desktop/src/features/coding-sessions/lib/codingSessionUmbrellaModel.ts`

It answers: **Who is working in this session?**

Its data comes from the umbrella model and signed coding-session events. It can safely present executions, providers, status, generation, elapsed activity, and transcript-derived detail for a selected execution.

It must preserve R24: signed subagent/spawn relationships precede any claimed subagent hierarchy. Do not parse `Task`, `spawn_agent`, tool labels, or prose to fabricate parent/child agents. Until signed spawn vocabulary exists, show executions honestly and use a quiet empty state where child relationships are unavailable.

### Changes rail

Primary files:

- `desktop/src/features/coding-sessions/ui/CodingSessionChangesRail.tsx`
- `desktop/src/features/coding-sessions/lib/codingSessionTranscriptModel.ts`
- shared diff presentation through `FileEditDiffBlock`

It answers: **What changes were observed in this transcript?**

It derives changed files from normalized transcript tool activity and reuses the existing diff renderer. This is good progressive disclosure and good component reuse.

Its important limitation is epistemic: it is not necessarily a complete workspace diff. A shell command, external process, or provider action not surfaced as a recognized edit result can change files without appearing here. Until provider-signed working-tree facts are wired into this surface, prefer the label **Observed changes** over **Session changes** and avoid copy that claims completeness.

Provider-signed `observedCommit` and `dirty` facts can strengthen the header later (for example, “Dirty at last provider check” or “Clean at abc123”), but they do not automatically provide a complete per-file diff. Treat those facts and transcript-observed edits as distinct layers.

## Are the rails functionally competing?

No. They compete only for screen space and control ownership.

The current header exposes separate rail toggles and the workspace prevents conflicting simultaneous layouts. That was a reasonable intermediate integration seam, but it is not the desired final information architecture.

The cleanup should replace multiple peer toggles with one surface host:

```text
Coding session header                              [surface toggle]
--------------------------------------------------|-----------------
Transcript / composer                             | Agents | Changes | +
                                                  |-----------------
                                                  | active surface
                                                  |
```

Behavior:

- Opening Agents selects the Agents tab.
- Opening Observed changes selects the Changes tab.
- Switching tabs does not resize or close the host.
- Closing the host preserves the last active tab and preferred width.
- Reopening restores the last active tab, if still available.
- Only surfaces supported by current data are offered.
- On narrow layouts, the same host content appears in one sheet with one close button.

Do not build Browser/Terminal/Files in this cleanup unless explicitly assigned. Establish an extensible surface registry/host seam without speculative implementations.

## Project-sidebar intent

The requested project hierarchy was:

```text
General                                      +
  Sessions
    [avatar] Improve coding-session transcripts    Working 14m
             integrated-build · Claude + Codex
    [avatar] Session authority phase execution     5h
             integrated · Claude
    View all 8 sessions
  Channels
    # general
    # welcome
  Repositories / other project children…
```

The integrated assembly now has session/channel/other grouping, collapsible shelves, umbrella consolidation, and active-session filtering. The final visual pass should verify:

- section labels are real headings and controls, not decorative text;
- each nonempty section can collapse independently;
- empty sections do not add noise;
- sessions are one row per umbrella;
- provider labels consolidate (`Claude + Codex`) without duplicating rows;
- working/elapsed state remains right-aligned when titles truncate;
- founder/operator avatar resolution uses existing profile data, with a stable fallback;
- a session row routes to the consolidated umbrella surface;
- channels remain recognizably channels and do not mix with sessions;
- the recent-session limit and “View all N sessions” behavior are explicit and tested.

Do not add protocol fields merely to improve the shelf. Derive consolidated providers from umbrella executions and founder identity from existing founder/profile facts. If the required fact truly does not exist, document and escalate it rather than inventing it.

## Known robustness and accessibility cleanup

Audit these against the current integrated code; some may already have been fixed after the original rail slice:

1. **One close control:** narrow Sheet content must not render both the generic Sheet close and a rail-header close in the same location.
2. **Drag teardown:** resizing must restore `document.body` cursor and `user-select` on pointer-up, pointer-cancel, lost pointer capture, breakpoint transition, and unmount.
3. **Tabs:** use roving `tabIndex`; support Arrow Left/Right, Home, and End; provide visible keyboard focus.
4. **Stale selection:** if the selected execution/surface disappears, reconcile to a valid tab instead of showing content with no selected tab.
5. **Duplicate provider names:** disambiguate only when labels collide, using stable execution/generation context.
6. **First measurement:** avoid briefly mounting the desktop inline rail and replacing it with an animated sheet before the workspace width is known.
7. **Width safety:** clamp restored, persisted, and pointer-cancel widths to current container bounds; reject corrupt stored values strictly.
8. **Resize semantics:** expose `role="separator"`, vertical orientation, current/min/max values, and keyboard resizing.
9. **Landmarks:** avoid nesting an `aside` inside another `aside`.
10. **Instance IDs:** scope tab/panel IDs with `useId` so popouts or multiple workspaces cannot duplicate IDs.
11. **Density:** prefer a compact shared surface tab row over a second heavy bordered header.
12. **Redundant facts:** avoid repeating execution count or runtime in adjacent rows and detail facts.

## Recommended implementation sequence

### Slice 1 — establish the shared surface host

- Introduce a single workspace-owned surface state: closed or `{tab}`.
- Move width persistence and responsive inline/sheet choice to the host.
- Render Agents and Observed changes as tabs.
- Replace separate header rail toggles with a single surface control or compact direct tab affordances.
- Preserve current data models and content components.
- Add keyboard, resize, collapse, narrow-sheet, and stale-tab tests.

### Slice 2 — simplify each surface

- Strip duplicated panel chrome from execution and changes components.
- Flatten agent rows: status dot, agent/provider identity, current activity, elapsed time.
- Use Overview only if it adds meaningful consolidated information; do not force an extra tab merely to mirror a model shape.
- Rename Changes to Observed changes unless completeness is proven.
- Keep errors and unknown states visible but quiet.

### Slice 3 — project-shelf visual and interaction pass

- Verify labels, independent collapse, empty-section behavior, umbrella rows, avatars, provider aggregation, right-aligned status, truncation, and consolidated navigation.
- Capture a seeded project containing at least one working Claude+Codex umbrella and one settled single-provider session.

### Slice 4 — screenshots and final polish

- Capture host open on Agents.
- Capture the same host switched to Observed changes.
- Capture resized width.
- Capture collapsed host.
- Capture narrow sheet.
- Capture grouped/collapsed project shelves.
- Verify every screenshot hash is distinct.
- Review the pixels critically: hierarchy, instant scan path, noise, missing facts, and long-session density.

## Files likely in scope

Expected product files:

- `desktop/src/features/coding-sessions/ui/CodingSessionHeader.tsx`
- `desktop/src/features/coding-sessions/ui/CodingSessionWorkspace.tsx`
- `desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx`
- `desktop/src/features/coding-sessions/ui/CodingSessionExecutionRail.tsx`
- `desktop/src/features/coding-sessions/ui/CodingSessionChangesRail.tsx`
- `desktop/src/features/coding-sessions/ui/useCodingSessionRailWidth.ts`
- their direct tests
- project-sidebar presentation/model files only for the shelf-polish slice
- one uniquely named screenshot spec plus additive Playwright registration

Avoid changing:

- provider/Rust code;
- relay behavior;
- event schemas or kind contracts;
- founder/genesis/goal authority semantics;
- composer authority or gating, except for a narrowly required rendering seam;
- lifecycle truth to make a screenshot convenient.

## Branch and integration guidance

- Do not merge `feature/coding-sessions-ui` into the assembly. Its useful product commits already exist in canonical/integrated history.
- Do not casually fold `feature/session-authority` into `feature/coding-sessions`; that is a separate integration decision, not rail cleanup.
- Before editing, compare against `integrated-build`/`integration/glue` because that is where the shelf and authority seams currently coexist.
- Preserve additive screenshot registrations from both transcript and authority work when resolving Playwright conflicts.
- Report the exact changed-file list before handoff.
- Activate Hermit before Git/build commands.
- Commit with `git commit -s`.
- Follow `docs/INTEGRATION.md`; never push directly to `main`, `integrated`, or `integrated-build`.

## Required gates

For each reviewable slice:

```bash
cd /Users/brian/Projects/<the-correct-worktree>
. ./bin/activate-hermit
cd desktop
pnpm check:px-text
pnpm exec biome check <changed-files>
pnpm exec tsc --noEmit
pnpm test -- --run
```

Use the repository's actual focused unit-test command if the package script differs. For screenshots, always build with:

```bash
pnpm build:e2e
```

Never use the plain production build for E2E screenshots. Wait for Radix animations, capture focused states, and verify distinct hashes with:

```bash
shasum -a 256 test-results/**/*.png
```

## Definition of clean

The convergence is complete when:

- users perceive one right-side work surface, not multiple unrelated rails;
- Agents and Observed changes are one-click sibling tabs;
- the surface is collapsible, resizable, responsive, keyboard-operable, and persistent;
- multi-provider sessions remain consolidated under one umbrella;
- execution truth is sourced from signed records;
- transcript-derived changes are labeled honestly;
- no fake subagent hierarchy is inferred;
- project navigation clearly separates Sessions, Channels, and other children;
- no duplicate close controls, landmarks, IDs, or resize cleanup leaks remain;
- screenshots demonstrate the real working, switched, resized, collapsed, narrow, and project-shelf states;
- the final commit history does not reintroduce the obsolete UI branch or unrelated authority work.

## Final opinion

The strongest pieces already exist: normalized transcript rendering, progressive tool detail, plan rendering, umbrella executions, observed diffs, project session grouping, and signed authority boundaries. The remaining problem is orchestration and visual hierarchy, not a missing second implementation.

Converge the existing pieces under one small surface-host abstraction. Keep the transcript as the narrative, keep the right panel as optional working context, and make every claim match the authority of its source. That produces the T3-like workflow Brian wants while remaining recognizably Buzz and without destabilizing the protocol work that has already landed.
