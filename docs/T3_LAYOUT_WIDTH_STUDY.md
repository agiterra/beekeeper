# t3code transcript width strategy vs. Buzz coding-session workspace

Read-only study. t3code paths relative to `/Users/brian/Projects/t3code/t3code`, Buzz to `/Users/brian/Projects/buzz`. Nothing modified.

## Headline finding (uncomfortable but true)

**t3code pins its transcript to the exact same 48rem measure Buzz does.** `apps/web/src/components/chat/MessagesTimeline.tsx:554` — `mx-auto w-full min-w-0 max-w-3xl overflow-x-clip`. Composer `apps/web/src/components/ChatView.tsx:6413` — `mx-auto w-full max-w-3xl`. No clamp, no vw, no container query, no JS measurement, no wide mode, no breakpoint above `sm:`. On a 3456px display t3code has the same gutters.

The delta is not the column width. It is (a) what happens to content that does not fit inside 48rem, and (b) that 48rem is rem and rides the font-size control. Buzz's real bug is (a).

## Part A — t3code mechanisms

### A1. Container width rule

| Surface | file:line | class |
|---|---|---|
| Timeline row root (per virtualized row) | `chat/MessagesTimeline.tsx:554` | `mx-auto w-full min-w-0 max-w-3xl overflow-x-clip` |
| Timeline "load earlier" header | `chat/MessagesTimeline.tsx:177` | `mx-auto w-full max-w-3xl pb-2` |
| Scroll viewport (owns padding) | `chat/MessagesTimeline.tsx:594` | `scrollbar-gutter-both h-full min-h-0 overflow-x-hidden overscroll-y-contain px-3 [overflow-anchor:none] sm:px-5` |
| Composer shell | `ChatView.tsx:6413` | `chat-composer-glass-shell relative mx-auto w-full max-w-3xl` |
| Composer outer padding | `ChatView.tsx:6375` | `ps-[calc(env(safe-area-inset-left)+0.75rem)] … sm:ps-[…+1.25rem]` (= `px-3`/`sm:px-5`, mirrors the list) |
| Composer banner stack | `chat/ComposerBannerStack.tsx:101` | `mx-auto mb-2 max-w-3xl` |
| Composer context strip | `BranchToolbar.tsx:472` | `mx-auto w-[calc(100%-2.75rem)] max-w-[calc(48rem-2.75rem)]` |
| Thread error banner | `chat/ThreadErrorBanner.tsx:45` | `mx-auto w-fit max-w-[min(48rem,calc(100%-2rem))]` |

(All under `apps/web/src/components/`.) Value is `max-w-3xl` = **48rem**, in rem, so it scales with root font size (A6). Two sites hardcode the literal `48rem` to stay in step.

**Panel states do not change the rule.** The chat column is a flex sibling of the panel: `ChatView.tsx:6207-6212` — `flex min-h-0 min-w-0 flex-col overflow-x-hidden` plus `rightPanelMaximized ? "w-0 flex-none" : "flex-1"`. Docked panel: `DiffPanelShell.tsx:33` — `w-[42vw] min-w-[360px] max-w-[560px] shrink-0 border-l border-border`. So the transcript is `min(48rem, remaining flex space)`: it narrows when the panel docks and `max-w-3xl` stops binding. **This is t3code's actual answer to a 3440px display — fill the right gutter with the diff/file panel, not with wider prose.**

Below 980px (`apps/web/src/rightPanelLayout.ts:1`, `RIGHT_PANEL_INLINE_LAYOUT_MEDIA_QUERY = "(max-width: 980px)"`) the panel becomes an overlay sheet at `w-[min(42vw,28rem)] min-w-80 max-w-[28rem]` (`rightPanelLayout.ts:2-3`), so the transcript keeps full width when narrow.

### A2. Prose vs wide content — there is no breakout

Searched `components/chat/`, `ChatView.tsx`, `ChatMarkdown.tsx` for negative margins, `100vw`, full-bleed, named grid columns: only cosmetic `-mx-1` group sections (`chat/MessagesTimeline.tsx:1363,2198`) and `-mx-0.5` icon nudges (`chat/ComposerControl.tsx:9,49`). **No code block, tool output, diff, or table escapes the 48rem measure.** Everything is constrained to the column and made to behave inside it. Enforcement is a three-link containment chain, and this is the load-bearing part:

1. row root `min-w-0` — `chat/MessagesTimeline.tsx:554`
2. markdown root `w-full min-w-0` — `ChatMarkdown.tsx:1792`: `"chat-markdown w-full min-w-0 text-sm leading-relaxed text-foreground/80 [overflow-wrap:anywhere] [word-break:break-word]"`
3. `apps/web/src/index.css:1762` — `.chat-markdown pre { max-width: 100%; overflow-x: auto; … }`

`min-w-0` is what lets `overflow-x:auto` engage instead of the child blowing the parent out and being clipped by an ancestor.

### A3. Horizontal overflow inside code blocks

Default is **horizontal scroll owned by the `<pre>`** — `index.css:1762-1771`:
```css
.chat-markdown pre { max-width: 100%; overflow-x: auto; border: 1px solid var(--border);
  border-radius: 0.75rem; background: var(--muted); padding: 0.8rem 0.9rem;
  scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--border) 78%, transparent) transparent; }
```
plus a 7px styled thumb (`index.css:1779-1786`); inside fenced chrome the inner `pre` is flattened (`index.css:1802-1807`).

**Per-block wrap toggle.** Every code block carries a `WrapText` button: `ChatMarkdown.tsx:674-676` sets `data-wrap={wrapped ? "true" : "false"}`, button at `ChatMarkdown.tsx:688-704`; CSS `index.css:1809-1812`:
```css
.chat-markdown .chat-markdown-codeblock[data-wrap="true"] pre { white-space: pre-wrap; overflow-wrap: anywhere; }
```
Initial state from a global setting — `ChatMarkdown.tsx:392-394` `readInitialWordWrapSetting() => getClientSettings().wordWrap`, switch at `settings/SettingsPanels.tsx:1301-1316` ("Wrap long lines in code blocks, tables, diffs, and file previews by default"). Same flag drives the diff panel (`DiffPanel.tsx:107,961,976`) and PR code tab (`pullRequest/PullRequestCodeTab.tsx:220,744`).

**Tool call/output never scrolls horizontally** — it wraps, capped vertically. `chat/MessagesTimeline.tsx:2066-2067`: `toolCallExpandedBodyClassName = "max-h-64 cursor-text overflow-auto whitespace-pre-wrap break-words font-mono text-secondary-label text-[length:var(--font-size-code,0.6875rem)] leading-relaxed select-text"`.

**Prose never clips** — `[overflow-wrap:anywhere] [word-break:break-word]` on the markdown root (`ChatMarkdown.tsx:1792`); user bubbles `whitespace-pre-wrap wrap-break-word` (`chat/MessagesTimeline.tsx:1776,1815,1857`).

**Tables** get bespoke handling: `index.css:1823-1830` sets `width:100%; min-width:max-content; overflow-wrap:normal; word-break:normal` — explicitly undoing the root `anywhere` so columns cannot collapse to one character — inside a `ScrollArea` with masked fades (`ChatMarkdown.tsx:471-477`; primitive `components/ui/scroll-area.tsx:43-52`, `scrollFade` applies `mask-t-from-… mask-r-from-… [--fade-size:1.5rem]`). Cells truncate at `max-width:24rem` collapsed (`index.css:1851-1857`) and wrap when expanded (`index.css:1859-1862`); the toggle measures column widths and pins `min-width` in px before expanding (`ChatMarkdown.tsx:405-426`).

### A4. Responsive behaviour

Only two breakpoints touch this layout: `sm:` (640px) steps list padding `px-3`→`px-5` (`chat/MessagesTimeline.tsx:594`) with the composer mirroring it (`ChatView.tsx:6375`); 980px switches the right panel from docked to sheet (`rightPanelLayout.ts:1`; 760px shrinks the sheet, `:3`). Above that, nothing — **the column stops at 48rem and never grows again.** `git log -L 554,554:…/MessagesTimeline.tsx` shows the line has only ever been `max-w-3xl` (introduced `96c9306db`; `449e1aaa4` changed `overflow-x-hidden`→`overflow-x-clip` for sticky headers). **No comment and no commit explains the 48rem number**; grep of `docs/`, `AGENTS.md`, `CLAUDE.md` for a measure/reading-width rule found nothing.

### A5. Alignment

**No shared layout primitive for chat.** Alignment is repetition: `max-w-3xl` at 5 sites, the literal `48rem` at 2 more, and `px-3 / sm:px-5` mirrored between the scroll viewport (`chat/MessagesTimeline.tsx:594`) and the composer wrapper (`ChatView.tsx:6375`). The header is deliberately full-bleed at `h-[var(--workspace-topbar-height)]` with its own `px-3 sm:px-5` (`ChatView.tsx:6215-6227`); t3code has no goal/status bar spanning the column. The pattern does exist elsewhere in the repo — `settings/settingsLayout.tsx:253` `mx-auto flex w-full max-w-4xl flex-col gap-12` — chat just never adopted it.

### A6. Reading-comfort features

- **Interface font size scales every rem dimension, including the measure.** `apps/web/src/appearanceFonts.ts:118` — `root.style.fontSize = \`${clampInterfaceFontSize(preferences.sizeInterface)}px\``, clamped 12–20px, default 16 (`packages/contracts/src/settings.ts:79-85`). At 20px the transcript is 960px; at 12px, 576px. The doc comment at `appearanceFonts.ts:94-97` states the intent: *"the interface size drives the root font size (and with it every rem-based dimension)"*.
- **Code font size is deliberately absolute px** — `--font-size-code` / `--diffs-font-size` (`appearanceFonts.ts:120-122`) "so they do not scale twice"; consumed at `index.css:1539-1542`, scoped to `.chat-markdown`.
- Global word-wrap switch, per-block wrap toggle, per-table expand toggle (A3).
- **No** user-adjustable column width, **no** wide mode, **no** density control, **no** `ch`-unit measure anywhere. Reported honestly: they do not exist.

### A7. Other discipline worth copying

`overflow-x-clip` (not `hidden`) on the row root so sticky headers inside rows still work (`449e1aaa4`). The scroll viewport is `overflow-x-hidden` (`chat/MessagesTimeline.tsx:594`) — the page can never scroll sideways; only individual `pre`/table elements can. One `ScrollArea` primitive with `scrollFade`/`hideScrollbars`/`chainVerticalScroll` (`components/ui/scroll-area.tsx:26-56`) is reused for every inner scroller, so overflow always has a visible affordance. Every long-content widget has a visible control for its overflow behaviour; nothing silently truncates.

## Part B — delta for Buzz

**Already right:** `desktop/src/shared/ui/markdown/CodeBlock.tsx:110` has `max-h-[400px] overflow-x-auto overflow-y-auto`; `CodingSessionTranscriptParts.tsx:98,103` have `max-h-48 overflow-auto whitespace-pre-wrap`. The mechanics exist; the containment chain around them is missing.

### B1. Fix the clip — this is the whole bug

`desktop/src/shared/ui/markdown.tsx:1876-1880` renders the markdown root with `max-w-none wrap-anywhere text-sm …` and **no `w-full min-w-0`**, unlike t3code's `ChatMarkdown.tsx:1792`. Without `min-w-0`, a `pre` wider than the column pushes the markdown root past its parent and the nearest `overflow-hidden` ancestor — `CodingSessionUmbrellaWorkspace.tsx:241` and `CodingSessionWorkspace.tsx:602`, both `flex min-w-0 flex-1 flex-col overflow-hidden` — clips it instead of the `pre` scrolling. Changes:

1. `shared/ui/markdown.tsx:1879` — add `w-full min-w-0` to the class list (keep `max-w-none`; harmless once `min-w-0` is present).
2. `coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx:244` and `CodingSessionWorkspace.tsx:609` — add `min-w-0` to the `mx-auto … max-w-3xl px-5 …` wrapper.
3. `coding-sessions/ui/CodingSessionTranscript.tsx:512` — user bubble `max-w-[80%] rounded-2xl …` needs `min-w-0` (t3code's equivalent is `chat/MessagesTimeline.tsx:980`, with the markdown root supplying `min-w-0`).
4. `shared/ui/markdown/CodeBlock.tsx:110` — add `max-w-full`, mirroring t3code `index.css:1762` `max-width: 100%`.
5. `CodingSessionTranscriptParts.tsx:348` — changed-file container is `overflow-hidden`; diff lines inside already wrap (`features/agents/ui/FileContentBlock.tsx:96` `block min-w-full whitespace-pre-wrap wrap-break-word px-3`), but add `min-w-0` so the border box tracks the column.

Also style the code-block scrollbar (t3code `index.css:1770-1786`). Buzz's `overflow-x-auto` gives no visible affordance today, which reads as "clipped" even when it isn't.

### B2. Adopt the wrap toggle

Port `MarkdownCodeBlock` chrome from `ChatMarkdown.tsx:669-720` into `shared/ui/markdown/CodeBlock.tsx` (the copy button already lives at `CodeBlock.tsx:113-129`; add a `WrapText` sibling). `useState` + `data-wrap` on the wrapper, and two CSS lines in `shared/styles/globals/markdown.css` beside the existing `.code-block-lines` rules (`markdown.css:298-316`): `[data-code-block][data-wrap="true"] pre { white-space: pre-wrap; overflow-wrap: anywhere; }`. Note `CODE_BLOCK_CLASS` (`CodeBlock.tsx:31`) is `whitespace-pre`, so the rule must win on specificity or the class must become conditional in TSX. Buzz has no settings surface equivalent to t3code's `wordWrap`; ship the per-block toggle only, default off (scroll).

### B3. Column width — the unit question, answered

**Keep `max-w-3xl` (rem). Do not switch to px, vw, or a vw-based clamp().** Buzz scales root font size for Cmd +/− (`desktop/src/app/useWebviewZoomShortcuts.ts:85`, `BASE_FONT_SIZE_PX = 16` at :10) — mechanically identical to t3code `appearanceFonts.ts:118`. Therefore:

- **rem is correct.** The measure is a typographic quantity that should hold a roughly constant character count; when the user zooms in, glyphs widen and the column must widen with them. `max-w-3xl` does that for free — and it means Brian already has a lever on his 3456px display: Cmd + widens the column proportionally.
- **px would freeze the measure against zoom** (at 150% zoom, 768px holds ~2/3 the characters) — the same class of bug as the `text-[15px]` regression in PR #891.
- **vw would jump with window size and ignore zoom entirely** — worst of both. t3code uses vw only for the *panel* (`DiffPanelShell.tsx:33` `w-[42vw]`), a chrome dimension, not a measure. That distinction is the rule worth copying.

`max-w-3xl` is a stock token, so `pnpm check:px-text` (text-size literals only) is not implicated. If a wider measure is ever wanted, add a named token under `theme.extend.maxWidth` in `desktop/tailwind.config.js` rather than an arbitrary `max-w-[56rem]`.

### B4. Fill the gutter instead of widening the column

t3code's answer to a wide display is the docked panel. Buzz has the same primitive: `CodingSessionSurfaceHost` with `layout={isNarrow ? "sheet" : "inline"}` (`CodingSessionUmbrellaWorkspace.tsx:274-283`, mirrored in `CodingSessionWorkspace.tsx`), sized by `useCodingSessionRailWidth.ts`. Compare its bounds to t3code's `w-[42vw] min-w-[360px] max-w-[560px]`; auto-opening Changes/Agents above ~1800px would consume the gutter the way t3code does. Product decision — flag it, do not unilaterally change default open state.

### B5. Shared layout primitive — yes, Buzz should exceed t3code here

t3code repeats the literal 7 times and it has already drifted (`max-w-3xl` vs `max-w-[calc(48rem-2.75rem)]` vs `max-w-[min(48rem,…)]`). Buzz has the same disease across 5 files. Add `desktop/src/features/coding-sessions/ui/CodingSessionColumn.tsx`:

```tsx
export function CodingSessionColumn({ className, children }: { className?: string; children: React.ReactNode }) {
  return <div className={cn("mx-auto w-full min-w-0 max-w-3xl", className)}>{children}</div>;
}
```

Consumers: `CodingSessionUmbrellaWorkspace.tsx:244` and `:257`, `CodingSessionWorkspace.tsx:609` and `:641`, `CodingSessionGoalPill.tsx:74`. Padding (`px-5 sm:px-8`) stays at the call site — t3code keeps padding on the scroller, not the measure box. While consolidating, fix the real misalignment: the transcript uses `px-5 sm:px-8` (`CodingSessionUmbrellaWorkspace.tsx:244`) while the composer overlay uses `px-4` (`:253`), so transcript text and composer edge are 4–12px out of register.

### B6. NewCodingSessionScreen and PendingCodingSessionScreen

- **`NewCodingSessionScreen.tsx:399`** (`max-w-2xl`, a form): **leave it, do not route through the primitive.** t3code's closest analogue is deliberately *narrower* than the transcript — `NoActiveThreadState.tsx:34` `w-full max-w-lg px-8 py-12`. Forms want a short measure; they are not the tunnel complaint.
- **`PendingCodingSessionScreen.tsx:102`** (`max-w-3xl`): **route through `CodingSessionColumn`** — same conceptual surface as the transcript, should align pixel-for-pixel across the transition. t3code's draft state does exactly this: the hero composer is the same `max-w-3xl` shell (`ChatView.tsx:6413`); only the headline goes wider, at `max-w-5xl` (`chat/DraftHeroHeadline.tsx:170`).

### B7. Porting risks

- **Framework: none.** Both are Tailwind v4 (`apps/web/package.json:68`, `desktop/package.json:105`), both mix utilities with a global stylesheet (`apps/web/src/index.css` ↔ `desktop/src/shared/styles/globals/markdown.css`). No CSS modules on either side.
- **Dependency Buzz lacks:** t3code's `ScrollArea` is Base-UI (`components/ui/scroll-area.tsx`); its `scrollFade` uses Tailwind v4 `mask-*-from-[…]`, which Buzz can use directly without the dependency. The table-expand column-measuring JS (`ChatMarkdown.tsx:405-426`) is optional — skip in slice 1.
- **Architecture:** t3code virtualizes with LegendList so `max-w-3xl` sits on *every row* (`chat/MessagesTimeline.tsx:554`). Buzz uses a plain scroll container with one wrapper (`CodingSessionUmbrellaWorkspace.tsx:244`). Buzz's shape is simpler and fine — do not adopt per-row wrappers.
- **Buzz has surfaces t3code doesn't** — `CodingSessionGoalPill`, `CodingSessionFounderLine`, `CodingSessionExecutionRail`. No t3code precedent for aligning a goal bar to the column; `CodingSessionGoalPill.tsx:74` is already `max-w-3xl` and just needs to join the primitive.
- **`text-[length:var(--font-size-code,…)]`** (t3code `chat/MessagesTimeline.tsx:2067`) would trip `pnpm check:px-text`. Use `text-xs`/`text-2xs` instead — Buzz's tool-output `pre` (`CodingSessionTranscriptParts.tsx:98,103`) already does.

### B8. Recommended slice order

1. Containment chain + scrollbar styling (B1). Zero visual change when content fits; fixes clipping when it doesn't.
2. `CodingSessionColumn` primitive + 5 call sites, plus the transcript/composer padding reconciliation (B5).
3. Per-code-block wrap toggle (B2).
4. Report to Brian that the tunnels are by design in t3code, that Cmd + already widens Buzz's column, and that the gutter is meant for the Changes/Agents rail (B4) — then let him decide whether he still wants a wider measure.
