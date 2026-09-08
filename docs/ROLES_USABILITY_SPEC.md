# Roles page usability — Fable slice (2026-09-08)

Worktree `/Users/brian/Projects/beekeeper/review-roles-usability-fable`, branch
`work/roles-usability-fable`, base `e517f3d5f`. Presentation only. Nothing in provenance
query/hook/authority, session stores, signature helper, native, relay, build scripts or the
ledger changes. The performance repairs in `5f192ea1c` + `347e9e9af` (signature memo,
batching, store revision guards, 25-row report pagination) are preserved byte-for-byte
where they live; the paginated list keeps its `data-testid`s.

## The problem, in Brian's words
He could not understand "Packs"; the test instructions named a Refresh control that does
not exist; duplicated Runner entries hid the version details; the page scrolled badly
(fixed). Goal: a person with no protocol knowledge opens the tab and can answer, per role,
"what is this role for, which agents can take it, which version of its instructions is
available on this computer, and what version are running agents actually reporting?"

## Page structure (top to bottom)

1. **Tab** label "Roles" (`ProjectPageTabs.tsx`: change only the label; id `packs`,
   path `/projects/$projectId/packs`, `data-testid="project-tab-packs"` unchanged).
2. **Header** `h2` "Roles" + button "Install roles" (unchanged) + a new button
   "Check again" (`data-testid="roles-recheck"`, calls the existing `refetchPacks` and
   `refetchAgents`; while any of packs/revisions/provenance/reports is fetching it is
   disabled and reads "Checking…"). Under it one plain sentence
   (`data-testid="roles-subtitle"`): "Each role is a set of instructions an agent follows
   in this project. Here you can see what each role is for, which agents can take it, and
   which version of its instructions is available on this computer and reported by
   running agents." Then the source line (`data-testid="packs-source-sentence"`), reworded
   by `packsSourceSentence`: "Instructions come from the project's repository at
   `<sha8>` · checked <age> ago" / "Instructions come from this app's built-in defaults
   (v<version>)" / "Instructions come from this computer only (not shared with the
   project)" / "No role instructions are available for <project>." Mixed origins: "Roles
   come from more than one place; see Technical details." Full sha stays in `title`.
   Errors keep `roles-error` with `rolesErrorSentence`.
3. **Role cards** (grid, unchanged ids `role-card-<slug>`). Each card, in this order:
   - name (`text-sm font-medium`), slug muted; **no origin badge, no bare version in the
     header** (they move into the version line).
   - purpose: `description` (`text-sm`), `summary` clamped to 3 lines (`text-sm
     text-muted-foreground`).
   - **Available here** line (`data-testid="role-available-<slug>"`,
     `data-availability="available|unavailable"`): "Available here: v1.3.0 (9f2e1d0c)
     from the project's repository" | "… from this app's built-in defaults" | "… from
     this computer only" | "Not available here — <refusal sentence>". Uses the
     card's own `packRef`/`version`/`origin`/`refusal`; never invents a version.
   - **Agents**: chips as today (`role-agents-<slug>`), empty text "No agent has this
     role yet."
   - **Reported by agents** (`data-testid="role-reports-<slug>"`), built from the new
     view model (§ contract): empty → "No agent has reported running this role yet."
     (`data-reports="none"`). Otherwise one summary sentence + one line per distinct
     reported version (max 5 lines, newest first; then "and N more versions in Report
     history"): e.g. "3 reports · 2 on the same version as this computer · 1 on an
     earlier version" then lines "9f2e1d0c · same version as here · 2 reports · latest
     just now · sender confirmed" / "dd11ee22 · earlier, 1 behind · 1 report · 2m ago ·
     sender unconfirmed". Vocabulary: relation words from `revisionRelationText` but in
     lower-case short form via a new `revisionRelationShortText` ("same version as
     here", "earlier, N behind", "newer than here, N ahead", "different history",
     "version unknown here", "different source", "built-in defaults, other app version",
     "version not reported"); provenance short form via `provenanceShortText`:
     `commissioned` → "sender confirmed", `proof-unavailable` → "sender unconfirmed",
     `disputed` → "disputed"; with the long label in `title`. Never say instructions ran.
   - **Sessions** (renamed from "Seats", `role-seats-<slug>` ids kept): rows as today;
     empty "No open sessions."
   - **Skills** collapsed: `<details data-testid="role-skills-<slug>"><summary>Skills
     (n)</summary>…` (ids `role-skill` kept). Empty: "No skills listed." inside.
   - refusal sentence stays at the bottom (`role-refusal-<slug>`) only when
     `refusal` and the Available line did not already carry it (it did → omit).
4. **Agents by project** section unchanged except title "Sessions by project" and empty
   "No agents in open sessions."
5. **Technical details** (`<details data-testid="roles-technical-details">` with
   `<summary>` "Technical details" — collapsed by default; keyboard-operable natively).
   Inside, three named groups, each its own `<details>`:
   - "Where instructions come from" (`data-testid="roles-diagnostics-source"`): the
     existing Available-here block (`role-pack-resolved`, rows `role-pack-resolved-row`,
     `role-pack-checkout-status`, `role-pack-resolved-error`) verbatim in structure —
     coordinates, checkout commit, git answered time, stale/errors.
   - "Report history (N)" (`data-testid="roles-diagnostics-history"`): the existing
     paginated reported list, unchanged rows/ids/pagination (`role-pack-reported`,
     `role-pack-reported-row` with `data-provenance`/`data-relation`, Previous/Next,
     `role-pack-reports-empty/partial/unavailable`). Open by default *only* when the
     outer details is open? No — collapsed; the count in the summary tells the reader
     there is something.
   - "How Beekeeper checks reports" (`data-testid="roles-diagnostics-checks"`): the
     `ROLE_PACK_SNAPSHOTS_SUBTITLE` sentence, `role-pack-provenance-notes`, the shelf
     notice (`roles-shelf-notice`), and the reports partial/unavailable sentences.
   Above the three groups, a one-line **uncertainty summary** (`data-testid=
   "roles-uncertainty-summary"`), always visible (outside the details): built by the
   view model: "" when nothing is uncertain; otherwise a short sentence such as "2 of 7
   reports could not be confirmed · report history may be incomplete · version check is
   from an earlier read" (join the applicable phrases with " · "). It is a summary, not
   the list of warnings.

## Type contract (lane V exports exactly these; lane C codes against them)

```ts
// desktop/src/features/roles/lib/roleVersionSummary.ts  (pure; no React)
import type { RolePackSnapshots, ReportedRolePackSnapshot } from "./rolePackSnapshots";
export type RoleReportedVersion = {
  sha: string | null;            // coordinate.sha or null when the report carried none
  relation: ReportedRolePackSnapshot["relation"];
  behind: number | null; ahead: number | null;
  count: number;                 // reports with this (sha, relation)
  latestAgeSeconds: number | null;
  provenance: { commissioned: number; unavailable: number; disputed: number };
  latest: ReportedRolePackSnapshot;  // newest report for this version
};
export type RoleReportSummary = {
  role: string;
  total: number;
  sameAsHere: number; earlier: number; newer: number; other: number; unknown: number;
  versions: RoleReportedVersion[];  // newest first, ALL of them (caller shows 5)
};
export function summarizeRoleReports(snapshots: RolePackSnapshots): ReadonlyMap<string, RoleReportSummary>;
export function roleReportSentence(summary: RoleReportSummary): string;      // "3 reports · 2 on the same version as this computer · 1 on an earlier version"
export type RolesUncertainty = { phrases: string[] };
export function summarizeRolesUncertainty(input: {
  snapshots: RolePackSnapshots;
  resolvedError: string | null; resolvedIsStale: boolean;
  revisionsError: string | null; provenanceError: string | null;
  reports: { isLoading: boolean; error: string | null; authorityError: string | null };
}): RolesUncertainty;
export function rolesUncertaintySentence(u: RolesUncertainty): string;         // phrases joined by " · ", "" when none
```
Reports are grouped by the role the report itself names: `coordinate.role` when a full
coordinate exists, else the new `ReportedRolePackSnapshot.role` (the 44223's own `role`
field, added as one additive field in `rolePackSnapshots.ts` from `session.role`).
Three cases are kept distinct and never inferred from a label, agent name or identity:
(a) role named + version coordinate → a version line; (b) role named, no coordinate →
counted on that role's card as "version not reported" (`sha: null`, relation
`incomplete`); (c) no role at all → not on any card; counted in `summarizeRoleReports`
under the key `""` and surfaced only in Report history and the uncertainty sentence
("N reports name no role").

## Copy rules
No "seat", "pack", "rung", "coordinate", "genesis", "commissioned", "provider" in primary
copy (cards, header, source line). Those words may appear inside Technical details.
Text sizes: `text-sm` for card body, `text-xs` for meta, `text-2xs` only for tiny
labels; no arbitrary literals. Every state (loading, empty, error, stale) has a
sentence. `<details>`/`<summary>` for every disclosure (keyboard-native); buttons for
paging.

## Lanes (strict ownership; lanes do not commit)
| Lane | Model | Owns |
| --- | --- | --- |
| V (view model + E2E) | sonnet | `desktop/src/features/roles/lib/roleVersionSummary.ts` (+`.test.mjs`), one additive `role: string \| null` field in `desktop/src/features/roles/lib/rolePackSnapshots.ts` (+ its test), `desktop/tests/e2e/role-packs-project.spec.ts` (port 4176 via `BUZZ_E2E_PORT`), screenshots under `desktop/test-results/roles-usability/` |
| C (components + copy) | opus | `desktop/src/features/roles/ui/**` (all components, `rolesCopy.ts`, their tests), `desktop/src/features/projects-container/ui/ProjectPageTabs.tsx` |
| Finalizer | Fable | review, gates (typecheck, check, roles + projects-container tests, file sizes, E2E on 4176, 1000-report fixture), commit with signoff, checkpoint |

Acceptance: tab reads "Roles"; a card shows purpose, agents, "Available here" and
"Reported by agents" in one place with distinct available / reported / unknown wording;
Technical details holds coordinates, provenance, history and limitations; the
uncertainty summary is one line; 1000-report fixture still renders bounded with
Previous/Next; keyboard opens every disclosure; empty/error states have sentences;
"Check again" refetches and shows real state. Mock E2E only — no live WebView claim.

## Finalizer notes and validation record (2026-09-08)

Review changes after the lanes: `useProjectPacksView` gained an additive
`isRefreshing` flag (every read the page shows, including a warm re-read after
"Check again") so the button reports actual state; the uncertainty line gained a
sessions phrase ("some sessions could not be read" / "sessions unavailable") so the
shelf notice is not only inside the collapsed details. Lane C's judgment calls kept:
the project column stays on card session rows (a session in another project is a
fact); `role-pack-reports-*` notices stay beside the history list; availability maps
`checkout`/`installed` to "from this computer only"; a shipped role shows its app
version, not a commit.

Logs: `/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/finalU-*.log`
(lane logs `laneV-*`, `laneC-*`).

| Check | Result | Log |
| --- | --- | --- |
| desktop typecheck | exit 0 | `finalU-typecheck.log` |
| desktop `pnpm check` + file sizes | exit 0 | `finalU-desktop-check.log` |
| roles + projects-container node tests | 477 passed, 0 failed | `finalU-focused-tests.log` |
| full desktop unit suite | 8455 passed, 0 failed, 0 skipped | `finalU-desktop-tests.log` |
| Roles E2E on port 4176, rebuilt bundle, `--repeat-each 2` | 6 passed; three screenshots with distinct hashes under `desktop/test-results/roles-usability/` | `finalU-e2e.log` |
| 1,000-report bound | view model bounded by distinct versions (unit); history stays paginated (E2E) | `laneV-lib-tests-final.log` |

Not claimed: live WebView acceptance (mock bridge only), Rust/mobile checks (pure
TypeScript change). Performance repairs from `5f192ea1c`/`347e9e9af` are untouched
(no edit to the ingress/store files; pagination and ids preserved and asserted).
