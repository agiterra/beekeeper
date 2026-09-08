# Roles page — visual design pass (Fable, 2026-09-08)

Worktree `/Users/brian/Projects/beekeeper/review-roles-design-fable`, branch
`work/roles-design-fable`, base `5234cfe6d`. Presentation only; preserves `572375bc9`
(usability), `e71cb7ba4` (warm recheck busy), the escaped-NUL repair, and the
performance repairs (signature memo, batching, store revision guards, 25-row history
pagination). No hook/store/provenance/native edits. Root is fixing the project-scope
bug in `useProjectPacksView`/`rolesViewModel` seat filtering — **do not compensate in
UI**; keep the project column on session rows.

## Goal
"Understand my project's agent team at a glance." Before: a documentation page of tall
cards full of paragraphs and repeated empty-state sentences. After: a quiet summary
strip, compact cards with a clear hierarchy (name → version → people → activity),
restrained color used only for real states, and every long text behind a deliberate
disclosure. Actual data only; nothing invented.

## Honest scope (Astra's verified boundary)
The agents shown are this computer's managed-agent records joined by home role — not
the project's team roster (root is building the project-scoped shared identity join).
So: one visible scope label `roles-scope` reading **"On this computer"** beside the
Agents count, with `title`/tooltip "Agents set up on this computer that carry a project
role. The project's shared team list is not available here yet." No Team/Mine switch,
no project filter in UI, no ownership inferred from names. The label is the reserved
place a scope control will occupy once root supplies real data.

## Layout (top → bottom), test ids in brackets

1. **Header** — title "Roles" (`text-xl font-semibold`) with actions right: "Check
   again" (`variant="ghost" size="sm"`, `roles-recheck`, busy → "Checking…" + disabled)
   and "Install roles" (`variant="outline" size="sm"`). One-line description
   (`roles-subtitle`, `text-sm text-muted-foreground`): "What each role is for, who can
   take it, and which version of its instructions is available and in use." The source
   line (`packs-source-sentence`, `text-xs text-muted-foreground`) stays one line under
   it. The old two-sentence explanation moves into Technical details → "How Beekeeper
   checks reports".
2. **Summary strip** (`roles-summary`; a row of 4 stat tiles, `grid grid-cols-2
   md:grid-cols-4 gap-2`; each tile `rounded-lg border border-border/70 bg-card px-3
   py-2`, number `text-xl font-semibold tabular-nums text-foreground`, label `text-xs
   text-muted-foreground`; **no color**; text tokens only):
   - `roles-summary-roles`: N · "roles"
   - `roles-summary-agents`: M · "agents" + the `roles-scope` label "On this computer"
   - `roles-summary-sessions`: K · "open sessions" (distinct seat keys whose status is
     not completed/stopped/failed)
   - `roles-summary-reports`: R · "reports" with a second muted line only when
     non-zero: "U unconfirmed" and/or "D disputed" (text, no color; disputed gets the
     `AlertTriangle` icon from lucide at `size-3` in `text-destructive` **with** the
     word).
   Counts come from a new pure `rolesPageSummary(view, snapshots)` in
   `desktop/src/features/roles/lib/rolesPageSummary.ts`.
3. **Role cards** (`role-card-<slug>`; grid `grid gap-3 sm:grid-cols-2 xl:grid-cols-3`;
   card `rounded-lg border border-border bg-card p-3 flex flex-col gap-2`):
   - **Title row**: `UserAvatar`-style role glyph is *not* invented — use a neutral
     `lucide` `Shapes`/`Sparkles`? No: use no icon; instead a 6px status dot at the left
     that is emerald when the role has ≥1 open session with status `running`, muted
     otherwise (`role-activity-<slug>` with `data-activity="running|idle|none"` and
     `aria-label`). Then name (`text-sm font-semibold`), slug (`text-xs
     text-muted-foreground font-mono`), and right-aligned the version chip
     (`role-available-<slug>`, `data-availability`): `InlineChip`-style muted mono
     "v1.3.0 · 9f2e1d0c" with `title` = the full sentence ("Available here: … from the
     project's repository"); unavailable → amber text "Not available here" with the
     reason in `title` and the sentence in a `<details>` below.
   - **Description**: one line, `line-clamp-2 text-sm text-muted-foreground`, full text
     in `title`. The persona `summary` paragraph is removed from the card face; it lives
     inside "About this role" (`role-about-<slug>`, a `<details>` at the card foot,
     together with Skills).
   - **Agents row** (`role-agents-<slug>`): a row of avatars (`UserAvatar` size "sm"
     with `avatarUrl` + name initials) with the name beside each (`text-sm`), a 6px
     status dot per agent from `ManagedAgent.status` (`running` emerald,
     `deployed` sky, `stopped`/`not_deployed` muted) **and** the status word in
     `title`. Pack-state (missing/refused/blocked) stays as the dashed outline + title
     exactly as today (`data-agent-pack` kept). Empty → one muted line "No agents yet."
   - **Activity row** (`role-reports-<slug>`, `data-reports`): one line: "3 reports ·
     2 same version · 1 earlier" (`text-xs text-muted-foreground`) followed by a
     "Versions (n)" `<details>` (`role-versions-<slug>`) holding the ≤5 version lines
     exactly as the usability slice renders them (`role-report-version`). Empty → "No
     reports yet." When ≥1 report is `disputed`, prefix the line with the
     `AlertTriangle` icon + "disputed" in `text-destructive`.
   - **Sessions** (`role-seats-<slug>`): only rendered when ≥1 seat; each row a
     compact button (`SeatRowButton columns="role-card"`) with a leading 6px status
     dot (running emerald / waiting_for_input amber / others muted) + the status word;
     **keep the project column** (root's scope fix pending). No "No open sessions."
     line — the card's activity dot and the summary strip already say so.
   - **Foot**: `<details>` "About this role · Skills (n)" (`role-about-<slug>`) with the
     summary paragraph and the skill list (`role-skill` ids kept; `role-skills-<slug>`
     id stays on the inner list container so existing assertions hold).
   - When agents, reports and seats are all empty: one muted line under the
     description "Nothing is using this role yet." (`role-quiet-<slug>`) instead of
     three empty sentences.
4. **Sessions by project** (`AgentsByProject`): render only project blocks that have
   ≥1 seat plus "Unplaced" when non-empty, as a compact list (`text-sm`, status dot,
   agent · role · status (age)); when none anywhere, a single muted line "No open
   sessions." under the "Sessions by project" heading (`project-agents-empty` id kept
   on that line).
5. **Uncertainty summary** (`roles-uncertainty-summary`): `flex items-center gap-2
   text-xs text-muted-foreground` with lucide `Info` `size-3.5`; unchanged sentences.
6. **Technical details** (`roles-technical-details`): unchanged structure/ids; visual
   only: `rounded-lg border border-border/70`, summary `text-sm font-medium px-3 py-2
   cursor-pointer`, inner groups indented `pl-3 border-l border-border/60`.

## Color and type rules
Text: `text-sm` body, `text-xs` meta, `text-2xs` only inside Technical details and
badges; rem tokens only; numbers `tabular-nums`. Color only for real states and always
with a word or `title`: emerald (running), sky (deployed), amber (waiting / unconfirmed /
not available), destructive (disputed). Dots are `size-1.5 rounded-full`. Dark theme:
use the same semantic tokens (`bg-card`, `border-border`, `text-muted-foreground`);
the emerald/sky/amber/destructive classes already carry `dark:` variants in this
codebase — reuse the exact classes the Badge variants use.

## Responsiveness / zoom / keyboard
Grid collapses to one column under `sm`; the summary strip to two columns; long
names `truncate` with `title`; every `<details>` is keyboard-native; buttons keep
focus rings. Verified by screenshots at 1280×900 light, 1280×900 dark (`.dark` on
`<html>`), 640×900 light, and 250% zoom (`document.documentElement.style.fontSize =
"40px"`, the app's own text-scale mechanism) — no clipped text, no overflow.

## Type contract (lane D exports; lane E asserts ids only)
```ts
// desktop/src/features/roles/lib/rolesViewModel.ts — additive on RoleAgentChip
avatarUrl: string | null; runtime: string | null; model: string | null;
// desktop/src/features/roles/lib/rolesPageSummary.ts (new, pure)
export type RolesPageSummary = { roles: number; agents: number; openSessions: number;
  reports: number; unconfirmed: number; disputed: number };
export function rolesPageSummary(view: RolesView, snapshots: RolePackSnapshots): RolesPageSummary;
```

## Lanes (strict; lanes never commit)
| Lane | Model | Owns |
| --- | --- | --- |
| D (design build) | opus | `desktop/src/features/roles/ui/**` (all components, copy, tests), `desktop/src/features/roles/lib/rolesViewModel.ts` (+test, chip fields only), `desktop/src/features/roles/lib/rolesPageSummary.ts` (+test) |
| E (E2E + screenshots) | sonnet | `desktop/tests/e2e/role-packs-project.spec.ts`, screenshot matrix under `desktop/test-results/roles-design/` (before set already in `roles-design-before/`) |
| Finalizer | Fable | screenshot inspection, gates, commit with signoff, checkpoint |

Acceptance: the four screenshots read as one calm system; a card is ≤ ~9 lines tall
with no reports; cards with data show version, people and activity without opening
anything; every color has a word; nothing claims team completeness; every previous
test id still resolves; 1000-report history still paginated; keyboard opens all
disclosures; gates green; E2E on 4176.

## Finalizer notes and validation record (2026-09-08)

Fixes after the lanes: card header wraps (`flex-wrap`, name+slug grouped, name
`flex-1`) so a 250%-zoom title never collapses to zero width (lane E's finding); summary
tile labels wrap instead of truncating; avatar initials render immediately
(`fallbackDelayMs={0}`); "Sessions by project" counts read "N sessions" (rows are
sessions, with or without an agent); the dark capture uses the app's real theme path
(stored `buzz-dark` + dark color scheme + native theme-changed event on a fresh page)
instead of flipping a class, so `bg-card` surfaces are actually dark.

Root's contract applied: `isManagedHere === false` → "shared" badge, no status dot,
`data-agent-pack="unknown"`, honest tooltips; strip reads "N on this computer · M
shared"; `ownerPubkey` typed optional, unrendered; `roleAgentPackState` call site shaped
for root's one-line wiring. "Available here" describes only this computer.

Logs `/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/finalD*-*.log`
(lanes `laneD-*`, `laneE2-*`).

| Check | Result | Log |
| --- | --- | --- |
| typecheck | exit 0 | `finalD6-typecheck.log` |
| `pnpm check` + file sizes | exit 0 | `finalD-desktop-check.log` |
| roles + projects-container tests | 513 passed, 0 failed | `finalD6-focused-tests.log` |
| full desktop suite | 8491 passed, 0 failed, 0 skipped | `finalD-desktop-tests.log` |
| Roles E2E on 4176, rebuilt bundle, `--repeat-each 2` | 8 passed; six distinct screenshots (light, dark, 640px, 250% zoom, card, details) with no horizontal overflow at 640px or 250% | `finalD6-e2e.log` |
| NUL-byte scan of changed files | clean | — |

Known evidence artefact: the dark capture is a second page in the same browser context,
so the mock channel holds the seeded reports twice (8 sessions instead of 4); product
code is unaffected. Not claimed: live WebView acceptance; Rust/mobile checks.
