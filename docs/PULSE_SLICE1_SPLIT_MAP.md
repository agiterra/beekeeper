# Project Pulse — Slice 1 feature-branch split map

> **EXECUTED 2026-08-19 → tag `build/2026-08-19.2`** (`integrated-build` =
> `8f32ec49`, not pushed). **Step 3's recipe is superseded:** `git checkout
> wip -- <path>` takes the whole ASSEMBLY version of a shared file, dragging
> other features' content onto the new branch (measured: kind.rs +313 lines
> instead of the Pulse-only +121). The safe method, used in execution: apply
> the Pulse **diff** per file (`git diff 7fc415b0 1ac2ac51 -- <path> | git
> apply`), hand-placing hunks whose context is absent on the base branch, and
> verify each file's changed-line multiset against the wip diff. Use that for
> any future delta (e.g. the UX-fix commit).

Computed 2026-08-19 against `wip/project-pulse`, then re-verified after the
Slice 1 commit series landed. The branch head is now **`d235189a`** (=
`integrated` `50dee7a7` + the ledger commit `7fc415b0` + five signed Slice 1
commits: `aced60f2` core, `0fb1c1fd` relay, `c35ef054` cli, `1c01b362` acp,
`d235189a` desktop). An earlier hash series (`07921567`…`1ea7796a`) was
observed mid-flight and superseded: the committer replayed the series to
correct three factual errors in its commit messages, verifying the resulting
tree byte-identical. File-to-branch assignments below are unaffected (same
tree).

- `git diff 7fc415b0..HEAD --name-only` → **56 paths**, +12,459 / −53.
- `git status --short` → **3** still-uncommitted paths: the two new `docs/`
  files and `trash me.md`.
- **59 paths** in total.

At first computation the Slice 1 tree was entirely uncommitted (59 paths in
`git status --short`, 53 of them tracked, +12,253 / −53, plus 3 untracked Pulse
sources under `desktop/src/features/project-pulse/lib/`). The five
`conformance/transcript-export/` files that were dirty at that point have since
been reverted — see §1c.

Branch heads at computation time:
`main cc8a8b0d` · `feature/project-containers d4b39d3b` ·
`feature/project-access d3a86d28` · `feature/builtin-shell bee7a76f` ·
`feature/coding-sessions b9de9a6d` · `integration/glue 50dee7a7` ·
`integration/glue-base 32476d2b`.

---

## 0. The decision, up front

**Create a new `feature/project-pulse`, stacked on `feature/builtin-shell`,
inserted into `FEATURES` between `feature/builtin-shell` and
`feature/coding-sessions`.** Do not fold Pulse into an existing feature.

Why a new branch and not a fold:

- **It is not project-containers/access/builtin-shell work.** It adds a new
  protocol (kind 44240, `crates/buzz-core/src/pulse.rs`, 946 lines), a new
  relay gate, a new SDK builder, a new CLI verb, a new ACP injection path and
  a new desktop feature directory. None of those are in any existing feature's
  charter.
- **It is not coding-sessions work either**, even though it reads
  coding-session facts. Its authorization substrate is the *project* ACL
  (`buzz_db::project_acl::ProjectGate`, `HiddenRepos.project_coordinates`),
  which lives on `feature/project-access` — a branch `feature/coding-sessions`
  is deliberately independent of.
- **It cannot be glue.** `integration/glue` is defined as "cross-feature
  adaptation patches" that are *rebased onto the assembly every build*
  (`docs/INTEGRATION.md:12`, `scripts/integrate.sh:112-146`). Putting a
  ~12k-line product feature into a force-pushed rebase series is how you lose
  it (see trap 3).

Why stacked on `feature/builtin-shell` and not on `main`:

`feature/project-pulse` needs symbols that only exist above
`feature/project-access`, verified by `git show <branch>:<path>`:

| Symbol needed by Pulse | First branch that has it |
|---|---|
| `KIND_PROJECT` (30621) | `main` |
| `buzz_db::project_acl::{ProjectGate, ProjectRole}`, `KIND_PROJECT_MEMBERS`, `is_git_project_gated_kind`, `normalize_project_coordinate`, `HiddenRepos.project_coordinates` | `feature/project-access` |
| `crates/buzz-relay/src/handlers/req.rs::{event_visible_to_reader (project arm), filter_can_match_git_gated_kinds, filter_can_match_project_kind}` | `feature/project-access` |
| `KIND_SHELL_SESSION` arm inside `filter_can_match_git_gated_kinds` (a line Pulse's diff edits) | `feature/builtin-shell` |
| `desktop/src/features/projects-container/**` (imported by `pulseChannelSet.ts`, `ProjectPulseCard.tsx`, `ProjectPulseScreen.tsx`, `pulseEntry.test.mjs`, `projectPulseTouchpoints.test.mjs`) | `feature/project-containers` |

Stacking on the *top* of that stack (`feature/builtin-shell`) is required, not
just convenient: the Pulse hunk in `req.rs::filter_can_match_git_gated_kinds`
literally rewrites the line containing `KIND_SHELL_SESSION`, which exists only
on `feature/builtin-shell` (`git show feature/project-access:crates/buzz-relay/src/handlers/req.rs | grep -c KIND_SHELL_SESSION` → 0;
same on `feature/builtin-shell` → 1).

Precedent that a stacked branch may reach *down* into a base branch's files:
`git show feature/builtin-shell:desktop/src/features/projects-container/ui/ProjectContainerScreen.tsx`
line 18 imports `@/features/builtin-shell/ui/ProjectTerminalsCard`. Pulse's
`ProjectContainerScreen.tsx` hunk is structurally identical.

### Position in `scripts/integrate.sh` `FEATURES`

```sh
FEATURES=(
  "fix/git-sign-oa-pubkey-validation"
  "fix/relay-multichannel-subscriptions"
  "feature/project-containers"
  "feature/project-access:feature/project-containers"
  "feature/builtin-shell:feature/project-access"
  "feature/project-pulse:feature/builtin-shell"   # ← NEW, here
  "feature/coding-sessions"
)
```

`docs/INTEGRATION.md:78-79` requires the entry to be added *on
`integration/glue`* ("Add the branch to `FEATURES` in `scripts/integrate.sh`
(on `integration/glue`), in merge order (a stacked branch after its base)").
Placing it before `feature/coding-sessions` keeps the containers stack
contiguous, so the one cross-line union merge (Pulse's relay/desktop files vs.
coding-sessions' relay/desktop files) happens at a single predictable point
that `rerere` can bank, instead of two.

Also update the stack list in `docs/INTEGRATION.md:19-24` in the same glue
commit.

### The one honest tension, stated explicitly

`feature/project-pulse` will **not compile standalone**, because three files
reference `feature/coding-sessions` symbols:

| File:line | Cross-line reference |
|---|---|
| `crates/buzz-core/src/pulse.rs:26` | `use crate::coding_session_goal::validate_coding_session_goal_session_ref;` |
| `desktop/src/features/project-pulse/lib/pulseFormat.ts:10-11` | `codingSessionWireWorkspaceStatus`, `type CodingSessionStatus` |
| `desktop/src/features/project-pulse/lib/pulseQueries.ts:20` | `parseBuzzCodingSessionMetadata` |

(plus `crates/buzz-cli/src/commands/pulse.rs`, which reads
`KIND_CODING_SESSION_{METADATA,GOAL,NAME,CLOSURE,LIFECYCLE_RECEIPT}` for
`buzz pulse sessions` / `buzz pulse digest`.)

This violates `docs/INTEGRATION.md:76` ("Keep it upstream-clean: … no references
to other features").
Three responses were considered:

1. **Move every coding-sessions-touching file to glue.** Rejected. The
   dependency is transitive through `crate::pulse` / `pulseFold.ts`, so it
   cascades: `pulse.rs` → `kind.rs` → `builders.rs` → all four relay handlers
   → `pulse_fetch.rs` → `e2e_pulse.rs`, i.e. the entire Rust half (~5,000
   lines) ends up in a force-pushed rebase series. Strictly worse.
2. **Fix the couplings before splitting.** The `pulse.rs:26` one is genuinely
   trivial — `validate_coding_session_goal_session_ref`
   (`crates/buzz-core/src/coding_session_goal.rs:18-25`) is a 7-line
   canonical-lowercase-UUID check whose error string Pulse already discards
   and rewrites (`pulse.rs:326-327`). Inlining it makes `buzz-core` Pulse-clean
   and removes the whole Rust cascade. **Recommended, as a commit on
   `wip/project-pulse` before the split** (see §2, step 0b) — the equivalence
   gate in `docs/INTEGRATION.md:61-64` compares `wip` against
   `integrated-build`, so changing `wip` first keeps them equal. Do **not**
   make this edit during the split.
3. **Accept the remaining three (`pulseFormat.ts`, `pulseQueries.ts`,
   `commands/pulse.rs`) as recorded impurities on the branch.** Recommended.
   They are the product thesis itself — "explicit member claims *beside*
   observed coding-session state" (`preview-features.json` description) — so
   they will exist however the branches are drawn. Record them in the
   `feature/project-pulse` commit message and in `docs/INTEGRATION.md` beside
   the existing note that coding-sessions' "project-shelf coupling lives in
   glue".

Consequence to accept knowingly: `feature/project-pulse` is not independently
upstreamable to `block/buzz` today. Neither is anything above
`feature/project-containers`, which is also unlanded.

---

## 1. File → branch → rationale

Legend for **Present on** (from `git ls-tree -r --name-only <branch> -- <path>`):
`main` / `cont` = project-containers / `acc` = project-access /
`shl` = builtin-shell / `cs` = coding-sessions / `glue` / `—` = nowhere.

The `#` column is the **original 1–64 enumeration** of the first computation and
is kept stable so earlier discussion still resolves. It no longer counts the
current 59 paths: #59–63 (§1c) are resolved and #64 (§1d) is the scratch file.

### 1a. → `feature/project-pulse` (new branch)

| # | File | Present on | Rationale |
|---|---|---|---|
| 1 | `crates/buzz-core/src/pulse.rs` | — | New: the 44240 contract (`PulseEntry`, `validate_pulse_entry_envelope`, `pulse_entry_project_coordinate`). Introduced by this feature. Apply fix (2) above first. |
| 2 | `crates/buzz-core/src/kind.rs` | all | Shared upstream file; the diff is only `KIND_PULSE_ENTRY`, `pulse_entry_hidden_from`, the `ALL_KINDS` entry, four const-asserts and three tests → serves Pulse. Uses `normalize_project_coordinate` (project-access) → legal on this stack. |
| 3 | `crates/buzz-core/src/lib.rs` | all | One line: `pub mod pulse;`. Serves Pulse. |
| 4 | `crates/buzz-sdk/src/builders.rs` | all | `pulse_entry` / `pulse_entry_envelope` builders + tests; only new kind referenced is `KIND_PULSE_ENTRY`. |
| 5 | `crates/buzz-db/src/project_acl.rs` | acc, shl, glue | File introduced by `feature/project-access`; the added `project_exists_by_coordinate` exists solely because a Pulse `a` tag is a required singleton (see its doc comment). "Changes to shared upstream files go to the feature they serve." *Defensible alternative:* `feature/project-access`, since the query is generic — but nothing else calls it. |
| 6 | `crates/buzz-db/src/lib.rs` | all | Thin `Db::project_exists_by_coordinate` wrapper over #5. |
| 7 | `crates/buzz-db/src/event.rs` | all | `EventQuery::a_tags` pushdown + the `KIND_PULSE_ENTRY` hidden-coordinate SQL exclusion inside the `git_gate` block. Both exist only for the Pulse read (`{"kinds":[44240],"#a":[…]}`). |
| 8 | `crates/buzz-relay/src/handlers/ingest.rs` | all (+glue) | `required_scope_for_kind` arm, `pulse_write_admitted`, envelope validation, 8 new tests. Depends on `project_acl::ProjectGate` (project-access). **Glue also edits this file** (`657507f6`, `62698e07`, `95843271`, `67697b89`) — expect a rebase touch, not a conflict (different regions). |
| 9 | `crates/buzz-relay/src/handlers/event.rs` | all (+glue) | Fan-out Pulse gate; depends on `state.project_coordinate_gate_cached` (project-access). Same glue-overlap note as #8 — one glue commit (`62698e07`) edits this file. |
| 10 | `crates/buzz-relay/src/handlers/count.rs` | all | `needs_pulse_gate_filtering` forces the COUNT fallback so a private project's entry total cannot leak its existence. |
| 11 | `crates/buzz-relay/src/handlers/req.rs` | all | `filter_can_match_pulse_kind`, `#a` pushdown, `pulse_entry_hidden_from` in `event_visible_to_reader`. **Carries a rider** — see §1e. |
| 12 | `crates/buzz-relay/src/api/bridge.rs` | all | `pulse_coordinates_by_filter` + Pulse request-shape validation. **Carries a rider** — see §1e. |
| 13 | `crates/buzz-acp/src/lib.rs` | all | One line: `mod pulse_fetch;`. |
| 14 | `crates/buzz-acp/src/pool.rs` | all | `pulse_cache`, `pulse_project`, `BUZZ_PULSE_PROJECT` on MCP env. Uses only `KIND_PROJECT` (main) + `KIND_PULSE_ENTRY`. No coding-sessions symbol. |
| 15 | `crates/buzz-acp/src/pulse_fetch.rs` | — | New; `KIND_PROJECT` + `KIND_PULSE_ENTRY` only (the "session" hits are ACP sessions, not coding sessions). |
| 16 | `crates/buzz-acp/src/base_prompt.md` | all | Adds the `buzz pulse` row and the "Project Pulse" prompt section. Prose only; no code dependency. |
| 17 | `crates/buzz-cli/src/lib.rs` | all (+glue) | `Cmd::Pulse`, `PulseCmd`, `PulseKindArg`. Glue touched this file once (`a1623062`), additive at a different location. |
| 18 | `crates/buzz-cli/src/commands/mod.rs` | all | One line: `pub mod pulse;`. |
| 19 | `crates/buzz-cli/src/commands/pulse.rs` | — | New (1,938 lines). **Recorded impurity:** reads five `KIND_CODING_SESSION_*` constants for `pulse sessions` / `pulse digest`. Splitting the file into a Pulse-only half and a glue half is a refactor, and refactoring during the split breaks the `git diff wip/<topic> integrated-build`-empty gate. |
| 20 | `crates/buzz-cli/src/links.rs` | all | **Verify before assigning.** The diff adds only `#[allow(dead_code)]` to `is_linkable_dtag` and `project_link` — unrelated to Pulse, and both functions predate it (`project_link` exists on `main`). If the warning fires only with Pulse's new code present, this belongs here; otherwise it is a standalone dead-code fix and belongs on the branch that introduced the caller-less function (`main`-owned → its own small commit). |
| 21 | `crates/buzz-test-client/tests/e2e_pulse.rs` | — | New (1,204 lines). Zero coding-session references (`grep -c` → 0). |
| 22 | `desktop/src/shared/constants/kinds.ts` | all | `KIND_PULSE_ENTRY = 44240`, mirroring #2. |
| 23–40 | `desktop/src/features/project-pulse/**` (18 files) | — | New feature directory; introduced by this feature. Files importing `@/features/projects-container/**` (`pulseChannelSet.ts`, `pulseEntry.test.mjs`, `projectPulseTouchpoints.test.mjs`, `ProjectPulseCard.tsx`, `ProjectPulseScreen.tsx`) are legal on this stack. **Recorded impurities:** `pulseFormat.ts:10-11`, `pulseQueries.ts:20`. Largest file 587 lines (`pulseFold.ts`) — well under the 1000-line ratchet. |
| 41 | `desktop/src/app/routes/projects.$projectId.pulse.tsx` | — | New route component; imports only `ProjectPulseScreen` + `ViewLoadingFallback`. Pulse-owned code. *(The two files that **register** it are glue — see §1b.)* |
| 42 | `desktop/src/features/communities/useCommunityInit.ts` | all | One import + one `resetProjectPulseState()` call. References Pulse only; the file is `main`-owned and the change serves exactly one feature. Also satisfies the CLAUDE.md rule that every community-scoped singleton is reset here. |
| 43 | `desktop/src/testing/e2eBridge.ts` | all | Mock bridge learns 44240 and the `30621:` `a`-tag prefix. Serves Pulse only. Not under a file-size ratchet root (`desktop/scripts/check-file-sizes.mjs` covers `src/app`, `src/features`, `src/shared/{api,context,lib,ui,styles}`, `src-tauri/**` — not `src/testing`), so its 13,686→13,700 growth is safe. |
| 44 | `desktop/playwright.config.ts` | all | Registers `**/projectPulse.spec.ts` in the smoke project. |
| 45 | `desktop/tests/e2e/projectPulse.spec.ts` | — | New spec for #23–41. |
| 46 | `preview-features.json` | all | The `project-pulse` preview-flag entry that `FeatureGate feature="project-pulse"` reads. |
| 47–49 | `conformance/project-pulse-fold/{CONTRACT.md, fixtures/fold-vectors.json, implementation.test.mjs}` | — | New corpus; binds `desktop/src/features/project-pulse/lib/{pulseFold,pulseEntry}.ts`, both on this branch, so the corpus is self-consistent here. Note the runner (`just conformance-check`, `Justfile:133-134`, a `conformance/**/*.test.mjs` glob) is `feature/coding-sessions`-owned — the corpus therefore runs in the **assembly**, not on this branch in isolation. No `Justfile` change needed. |

### 1b. → `integration/glue`

| # | File | Present on | Rationale |
|---|---|---|---|
| 50 | `desktop/src/features/projects-container/lib/projectChildren.ts` | cont, acc, shl, glue | Cross-feature wiring: adds the `pulse` row type **and renumbers `PROJECT_CHILD_TYPE_RANK`** (channel 1→2 … remote-shell 7→8) directly on top of glue's `coding-session: 0` shelf work. Nineteen existing glue commits own these exact files (`8291e3f7`, `fa2ec517`, `27251f7b`, `9adba0d6`, `f9fb5d9a`, …). Landing the Pulse hunk on the feature branch guarantees a rank-table conflict on every glue rebase, which `rerere` will then bank (trap 1). Landing it *in* glue puts it beside the hunks it collides with. |
| 51 | `desktop/src/features/projects-container/ui/ProjectChildRowItem.tsx` | cont, acc, shl, glue | Same: renders the `pulse` case. |
| 52 | `desktop/src/features/projects-container/ui/ProjectContainerScreen.tsx` | cont, acc, shl, glue | Same: mounts `ProjectPulseCard` inside `<FeatureGate feature="project-pulse">`. |
| 53 | `desktop/src/features/projects-container/ui/ProjectSidebarGroup.tsx` | cont, acc, shl, glue | Same: `pulseEnabled` gate + `includePulse` + `onOpenPulse` plumbing. |
| 54 | `desktop/src/features/projects-container/ui/ProjectSidebarSections.tsx` | cont, acc, shl, glue | Same: passes `onOpenPulse` navigation. |
| 55 | `desktop/src/app/routes.ts` | all (+glue) | Route registration nests under the containers-owned `/projects/$projectId` route; glue already owns the sibling hunk for `projects.$projectId.sessions.new.tsx` (`97527526 glue: project-scoped session creation`). Same file, same reason, same series. |
| 56 | `desktop/src/app/routeTree.gen.ts` | all (+glue) | Generated companion of #55; same glue commit owns it. Regenerate rather than hand-resolve on conflict (trap 5). |
| 57 | `docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md` | — | Fork-local docs are glue-owned: **37** of the 113 paths in `git diff --name-only integration/glue-base..integration/glue` are under `docs/`, and both new files are absent from `main`. (`main` is not docs-free — it carries 54 `docs/` paths as the upstream mirror, against 99 on glue — but neither `docs/INTEGRATION.md` nor `docs/SESSION_STATE.md` is among them.) The push-destination tripwire (`.lefthook/pre-push/push-destination.sh`) exists precisely because this `docs/` line is fork-local. |
| 58 | `docs/DUAL_STREAM_THESIS_RESEARCH_2026-08-19.md` | — | Same. **Note:** the document's own header says it belongs on `wip/dual-stream-thesis`, and `git ls-tree -r wip/dual-stream-thesis -- docs/` shows it is **not** there. It exists only in this working tree. See trap 3. |
| — | `scripts/integrate.sh` | glue | Not in `git status` — the `FEATURES` entry from §0 is a *new* edit made during the ceremony, on glue, per `docs/INTEGRATION.md:78-79`. Update `docs/INTEGRATION.md:19-24` (the stack list) in the same commit. |
| — | `docs/SESSION_STATE.md` | glue | Already committed on `wip/project-pulse` as `7fc415b0`. Cherry-pick onto glue with `--signoff`; do not assume the rebuild carries it (trap 3). |

### 1c. → `feature/coding-sessions` (or revert) — not Pulse work

> **Resolved — no action needed.** These five files have already been reverted:
> `git diff 7fc415b0..HEAD --name-only | grep -c transcript-export` → **0**, and
> they do not appear in `git status --short`. Outcome (a) below was taken. Kept
> here for the record.

| # | File | Present on | Rationale |
|---|---|---|---|
| 59–63 | `conformance/transcript-export/{fixtures.test.mjs, implementation.test.mjs, fixtures/bundle-vectors.json, fixtures/naming-vectors.json, fixtures/release-manifest-vectors.json}` | cs, glue | **Pure formatter churn, zero semantic change** — every hunk is Biome line-wrapping (`git diff -w --ignore-blank-lines` still shows them only because the reflow moves tokens across lines; inspect any hunk and it is `assert.equal(a, b, c)` → 4-line form). Introduced by `feature/coding-sessions` (the `conformance/` tree exists nowhere below it). Two clean outcomes: **(a)** `git checkout HEAD -- conformance/transcript-export/` and drop it, or **(b)** if the repo's root `biome.json` genuinely now formats `conformance/` (verify with `pnpm biome check conformance/` or `just fix-all` on a clean tree), land it as its own commit on `feature/coding-sessions` — `style: biome-format the transcript-export conformance corpus`. Either way it must **not** ride inside a Pulse commit. |

### 1d. → drop

| # | File | Rationale |
|---|---|---|
| 64 | `trash me.md` | 60-byte scratch file ("Secret message: the buzz flies at midnight"). Delete before the ceremony — `integrate.sh:63` aborts on any dirty working tree. |

### 1e. Riders: two hunks that are *not* Pulse and should be lifted out

These are inside Pulse-assigned files but fix pre-existing fail-open bugs that
affect repo, shell and membership gating with no 44240 involved. Both are
independently valuable and independently upstreamable; both are cheap to lift
into their own commit on the owning branch **before** the Pulse commit.

| Where | Rider | Owning branch | Why |
|---|---|---|---|
| `crates/buzz-relay/src/handlers/req.rs`, `filter_can_match_git_gated_kinds` | `ks.is_empty() ||` — an explicit `{"kinds":[]}` deserializes to `Some(∅)`, `.any()` over which is `false`, so the predicate read it as "matches no gated kind" and left every consumer running against an empty `HiddenRepos` — failing **open** on private-project repo, shell and membership events. | `feature/project-access` introduced the function; `feature/builtin-shell` added its `KIND_SHELL_SESSION` arm → land on `feature/builtin-shell` so the fix covers both, with a test asserting only the git predicate. The combined test (`empty_kind_set_arms_the_gated_kind_predicates`, which also asserts `filter_can_match_pulse_kind`) stays on `feature/project-pulse`. |
| `crates/buzz-relay/src/api/bridge.rs`, `handle_channel_window_filter` + the feed branch | (a) empty-`kinds` guard — the window SQL omits `AND e.kind IN (…)` for an empty slice, turning `"kinds":[]` into a wildcard over every top-level row in the channel; (b) replacing the feed's hand-rolled `reader_authorized_for_event` + `repo_event_hidden_from` pair with the single `event_visible_to_reader` call, and adding that call to the window rows / aux hop / thread-summary paths. Closes leaks for repo, shell announce and membership events too. | `feature/builtin-shell` (it routes through `event_visible_to_reader`, which `feature/builtin-shell` is the top owner of: 5 refs vs. 4 on `project-access`, 3 on `main`). |

Practical caveat, stated plainly: these hunks **interleave** with the Pulse
hunks in the same functions. If splitting them out proves conflict-prone under
time pressure, ship the whole file on `feature/project-pulse` and record the
debt in the commit message — do not silently drop the fixes, and do not spend
the ceremony window on a surgical `git add -p` that risks the equivalence gate.

---

## 2. Recommended branch plan (ordered)

**Step 0a — MANDATORY, before anything else: verify every project-pulse file is
committed on `wip/project-pulse`.** Step 3 populates the feature branch with
`git checkout wip/project-pulse -- <paths>`, which reads the **branch tree**.
Anything still uncommitted in the working tree therefore never reaches
`feature/project-pulse` — silently, with a zero exit status.

```sh
git status --porcelain            # must show no project-pulse paths
```

Three files were **untracked** at check time:

```
desktop/src/features/project-pulse/lib/pulseChannelSet.ts
desktop/src/features/project-pulse/lib/pulseChannelSet.test.mjs
desktop/src/features/project-pulse/lib/pulseQueries.test.mjs
```

Confirm they made it into the commit series, and if not, commit them with `-s`
first:

```sh
git ls-files --error-unmatch \
  desktop/src/features/project-pulse/lib/pulseChannelSet.ts \
  desktop/src/features/project-pulse/lib/pulseChannelSet.test.mjs \
  desktop/src/features/project-pulse/lib/pulseQueries.test.mjs
```

This is not a hypothetical: `desktop/src/features/project-pulse/ui/ProjectPulseScreen.tsx:16`
does `import { projectPulseChannelIds } from "../lib/pulseChannelSet"`, so
shipping the branch without `pulseChannelSet.ts` breaks the desktop build.

**Status at re-verification:** all three are now tracked — the commit series
picked them up, and `git ls-files desktop/src/features/project-pulse` returns
the full 18 files of §1a #23–40. The same hazard still applies to step 4's two
`docs/` files, which remain untracked; `git add` them before the glue checkout.

**Step 0b — before anything, on `wip/project-pulse` (optional but recommended).**
Land one commit inlining `crates/buzz-core/src/pulse.rs:26`'s
`validate_coding_session_goal_session_ref` call (7 lines, error already
remapped at `pulse.rs:326-327`), and delete `trash me.md`. Re-run the gate.
This is the *only* code change permitted around the split; everything after
this point is `git checkout`-and-commit, because `docs/INTEGRATION.md:61-64`
requires `git diff wip/project-pulse integrated-build` to end empty.

```sh
. ./bin/activate-hermit
cd /Users/brian/Projects/buzz
rm "trash me.md"
# (edit crates/buzz-core/src/pulse.rs)
git commit -s -am "refactor(pulse): inline the session-ref UUID check"
```

**Step 1 — resolve the non-Pulse churn** (§1c, §1d). Either revert
`conformance/transcript-export/` or commit it separately on
`feature/coding-sessions`.

**Step 2 — create the branch.**

```sh
git checkout -b feature/project-pulse feature/builtin-shell
```

**Step 3 — populate it** from `wip/project-pulse`, paths from §1a:

```sh
git checkout wip/project-pulse -- \
  crates/buzz-core/src/pulse.rs \
  crates/buzz-core/src/kind.rs \
  crates/buzz-core/src/lib.rs \
  crates/buzz-sdk/src/builders.rs \
  crates/buzz-db/src/project_acl.rs \
  crates/buzz-db/src/lib.rs \
  crates/buzz-db/src/event.rs \
  crates/buzz-relay/src/handlers/ingest.rs \
  crates/buzz-relay/src/handlers/event.rs \
  crates/buzz-relay/src/handlers/count.rs \
  crates/buzz-relay/src/handlers/req.rs \
  crates/buzz-relay/src/api/bridge.rs \
  crates/buzz-acp/src/lib.rs \
  crates/buzz-acp/src/pool.rs \
  crates/buzz-acp/src/pulse_fetch.rs \
  crates/buzz-acp/src/base_prompt.md \
  crates/buzz-cli/src/lib.rs \
  crates/buzz-cli/src/commands/mod.rs \
  crates/buzz-cli/src/commands/pulse.rs \
  crates/buzz-cli/src/links.rs \
  crates/buzz-test-client/tests/e2e_pulse.rs \
  desktop/src/shared/constants/kinds.ts \
  desktop/src/features/project-pulse \
  'desktop/src/app/routes/projects.$projectId.pulse.tsx' \
  desktop/src/features/communities/useCommunityInit.ts \
  desktop/src/testing/e2eBridge.ts \
  desktop/playwright.config.ts \
  desktop/tests/e2e/projectPulse.spec.ts \
  preview-features.json \
  conformance/project-pulse-fold
git commit -s -m "feat(pulse): Project Pulse — kind 44240 entries, relay gate, CLI, desktop view"
```

Optionally split into 2–4 commits (protocol+relay / SDK+CLI / ACP / desktop).
Record the §0 impurities in the commit message.

If lifting the §1e riders, do them **first** on `feature/builtin-shell`, then
rebase `feature/project-pulse` onto the updated base before step 4.

**Step 4 — glue.** Per `docs/INTEGRATION.md:57-60`, glue commits are added
*after* the ceremony rebuild, on the rebased `integration/glue`:

```sh
git checkout integration/glue          # after integrate.sh has rebased it
git checkout wip/project-pulse -- \
  desktop/src/features/projects-container \
  desktop/src/app/routes.ts \
  desktop/src/app/routeTree.gen.ts \
  docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md \
  docs/DUAL_STREAM_THESIS_RESEARCH_2026-08-19.md
git commit -s -m "glue: Project Pulse as a project child — sidebar row, home card, route"
git cherry-pick --signoff 7fc415b0    # the SESSION_STATE ledger commit
# + the FEATURES / INTEGRATION.md edit from §0:
git commit -s -m "chore(integration): add feature/project-pulse to the assembly (stacked on builtin-shell)"
```

**Step 5 — reassemble, verify equivalence, gate, push** (§3).

**Step 6 —** `git diff wip/project-pulse integrated-build` must be empty, or
every remaining hunk explained. Then delete `wip/project-pulse`
(`docs/INTEGRATION.md:61-65`).

---

## 3. The exact ceremony command sequence

From `docs/SESSION_STATE.md:205-207` (§3a) plus `docs/INTEGRATION.md:129-137,
150-157`.

```sh
# ── 0. environment ───────────────────────────────────────────────────────────
cd /Users/brian/Projects/buzz
. ./bin/activate-hermit          # SESSION_STATE §3a: non-interactive shells do
                                 # not source ~/.zshrc; never rewrite hook
                                 # commands to compensate for an unset PATH

# ── 1. park every other worktree on detached HEAD ────────────────────────────
# integrate.sh:100-109 refuses (or reroutes) a rebase whose branch is held by
# another worktree, and aborts outright if that worktree is dirty.
git worktree list                # currently 9; feature/coding-sessions is held
                                 # by /Users/brian/Projects/buzz-coding-sessions
git -C /Users/brian/Projects/buzz-coding-sessions status --porcelain   # must be empty
git -C /Users/brian/Projects/buzz-coding-sessions checkout --detach
# repeat for any worktree still on a named branch:
#   .claude/worktrees/agent-a18d9db646ed69582  (ui-surface-host)
#   .claude/worktrees/agent-a196f2486f278cb9d
#   .claude/worktrees/agent-ab1f7ac58e4a485dc

# ── 2. the local feature stack + glue-base must exist as local branches ──────
# (docs/INTEGRATION.md:155-157). Verified present today; re-check after a fetch:
git rev-parse --verify feature/project-containers feature/project-access \
  feature/builtin-shell feature/coding-sessions integration/glue \
  integration/glue-base
git rev-parse --verify feature/project-pulse     # created in §2 step 2

# ── 3. working tree clean (integrate.sh:63) ──────────────────────────────────
git status --porcelain           # must be empty

# ── 4. the two CI-only steps the ceremony gate does NOT run ──────────────────
# docs/INTEGRATION.md:129-137 — .woodpecker/gate.yml also runs these, so a
# green ceremony can still land red. Slice 1 ADDS a conformance corpus, so the
# first of these is not optional this time.
just conformance-check
just export-viewer-manifest-test

# ── 5. the ceremony ──────────────────────────────────────────────────────────
LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse upstream/main) \
  scripts/integrate.sh --no-push          # dry run first
# then, once the assembly is right and the glue commits from §2 step 4 are on:
LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse upstream/main) \
  scripts/integrate.sh --skip-gate        # if the gate was just run by hand

# ── 6. equivalence gate (docs/INTEGRATION.md:61-64) ──────────────────────────
git diff wip/project-pulse integrated-build      # must be empty

# ── 7. push BOTH remotes (SESSION_STATE §3a: origin = relay, upstream = GitHub)
# integrate.sh:181-187 pushes `origin` only. Mirror every ref to `upstream`:
for b in fix/git-sign-oa-pubkey-validation fix/relay-multichannel-subscriptions \
         feature/project-containers feature/project-access feature/builtin-shell \
         feature/project-pulse feature/coding-sessions; do
  git push --force-with-lease upstream "$b"
done
git push --force-with-lease upstream integration/glue integration/glue-base \
  integrated-build:integrated
git push upstream "$(git describe --tags --abbrev=0)"
# Both remotes pass .lefthook/pre-push/push-destination.sh:
#   origin  = https://lightyear.agiterra.org/git/…/buzz
#   upstream= https://github.com/agiterra/buzz.git
# (…and LEFTHOOK=0 disables that tripwire anyway — see trap 6.)
```

Why the env vars, precisely (`docs/INTEGRATION.md:150-157`): there is **no
`origin/main`** in this clone (origin is the relay mirror), so
`resolveBaseRef` in `scripts/check-file-sizes-core.mjs:44-60` would fall back
to a `git merge-base origin/main HEAD` that cannot resolve;
`CHECK_FILE_SIZES_BASE` supplies the base explicitly. `LEFTHOOK=0` stops the
hook-driven `cargo fmt --all` from dirtying the tree mid-run — which would
trip `integrate.sh:63` on the next pass.

---

## 4. Known traps

1. **A bad conflict resolution is persistent** — `docs/SESSION_STATE.md:212-217`.
   `rerere` records whatever you commit, including a broken union, and replays
   it into every later rebuild; "the same unclosed `impl` came back three times
   on 2026-08-18". When a recurring cross-feature conflict reappears, take
   `upstream/integrated`'s version of the file rather than reconstructing the
   union by hand (`:210`). To purge a cached bad resolution:
   ```sh
   grep -rl "<symbol from the hunk>" .git/rr-cache/*/preimage
   rm -rf .git/rr-cache/<hash>
   ```
   **Highest-risk file this ceremony:**
   `desktop/src/features/projects-container/lib/projectChildren.ts` — Pulse
   renumbers `PROJECT_CHILD_TYPE_RANK` (`channel: 1→2` … `remote-shell: 7→8`)
   in the same table glue's shelf commits own. That is why §1b assigns it to
   glue rather than to the feature branch. Runner-up:
   `crates/buzz-relay/src/handlers/{ingest,event}.rs`, which glue also edits
   (`657507f6`, `62698e07`, `95843271`, `67697b89`; `62698e07` is the only one
   that touches `event.rs`). Note that the frequently-cited `b953082e` and
   `a1623062` are **not** among them — `b953082e` touches only `Cargo.lock`, and
   `a1623062` touches `crates/buzz-cli/src/lib.rs`, `crates/buzz-db/src/migration.rs`,
   `crates/buzz-relay/src/handlers/side_effects.rs`, `crates/buzz-relay/src/state.rs`
   and `desktop/src-tauri/src/lib.rs`.

2. **Cargo.lock drift is invisible to tests and fatal to the deploy** —
   `docs/SESSION_STATE.md:218-221`. `cargo test` updates the lock in place, so
   a drifted lock passes every check and then fails the release image build
   (it did, 2026-08-18: `buzz-shell-host` and its closure were missing and the
   relay could not ship — `scripts/integrate.sh:163-167`). The gate now runs
   `cargo metadata --locked --format-version 1` first. **Good news for Slice 1:**
   `git status --porcelain -- Cargo.lock` is empty — Pulse adds no new
   dependency. Re-check after the split, because a per-branch `cargo check`
   can still rewrite it.

3. **Work can be lost between ceremonies** — `docs/SESSION_STATE.md:222-224`:
   "Two ledger commits made on a wip branch did not survive the next rebuild.
   After any ceremony, check that your own commits are present by content, not
   by assuming the rebase carried them." Two live instances right now:
   - `7fc415b0` (the `docs/SESSION_STATE.md` edit) exists **only** on
     `wip/project-pulse`. It must be cherry-picked onto `integration/glue`
     (§2 step 4) or it dies with the wip branch.
   - `docs/DUAL_STREAM_THESIS_RESEARCH_2026-08-19.md` claims to live on
     `wip/dual-stream-thesis`; `git ls-tree -r wip/dual-stream-thesis -- docs/`
     shows it is **not there**. It exists only in this working tree,
     uncommitted, on a branch that §2 step 6 deletes.

   Verify after the ceremony by content, not by ancestry:
   ```sh
   git show integrated:docs/SESSION_STATE.md | grep -c "b9de9a6d"
   git ls-tree -r integrated -- docs/ | grep -E "DUAL_STREAM|PROJECT_PULSE_TRUTH"
   git ls-tree -r integrated -- crates/buzz-core/src/pulse.rs conformance/project-pulse-fold
   ```

4. **The ceremony gate is narrower than CI** — `docs/INTEGRATION.md:129-137`.
   `integrate.sh` runs `cargo test --workspace`, `just desktop-check`,
   `just desktop-test`, `pnpm typecheck`. `.woodpecker/gate.yml` *additionally*
   runs `just conformance-check` and `just export-viewer-manifest-test`, and
   migrates a **fresh** database. Slice 1 adds `conformance/project-pulse-fold/`,
   which the local gate will not run — hence §3 step 4. It adds no migration,
   so the fresh-DB ordering risk is low, but `cargo run -p buzz-admin -- migrate`
   against a clean database is still the only thing that exercises migration
   ordering.

5. **`routeTree.gen.ts` is generated.** On conflict, regenerate rather than
   hand-merge; a hand-merged generated file is exactly the shape of bad
   resolution `rerere` will bank forever (trap 1).

6. **`LEFTHOOK=0` disables the pre-push hooks entirely.** That is deliberate
   (it stops `cargo fmt --all` dirtying the tree), but it also skips the
   file-size ratchet, clippy, `tsc --noEmit`, the fast unit tests **and** the
   `push-destination.sh` tripwire. `CHECK_FILE_SIZES_BASE` is therefore belt
   and braces, not the thing doing the work. If you want the ratchet's verdict
   this ceremony, run `just file-size-check` explicitly. (Checked: every new
   desktop file is under the 1000-line cap — largest is `pulseFold.ts` at 587 —
   and `desktop/src/testing/e2eBridge.ts`, which grows 13,686→13,700, is not
   under any ratchet root, so nothing here is at risk.)

7. **A completion report is not evidence** (CLAUDE.md). After the push, confirm
   the relay actually knows the new kind rather than trusting the pipeline —
   `docs/INTEGRATION.md:144-148`: publish a probe of kind 44240 and read the
   verdict; an older relay answers `restricted: unknown event kind`. Probes
   live in `/tmp/grant-proof` (`kindprobe`) per `docs/SESSION_STATE.md:227-229`.
   CI status without a login: `https://ci.agiterra.org/api/badges/1/cc.xml`.

8. **`feature/coding-sessions` is checked out in
   `/Users/brian/Projects/buzz-coding-sessions`.** `integrate.sh:100-109` will
   run its rebase *inside* that worktree and abort if it is dirty. Park it
   first (§3 step 1) — SESSION_STATE §3a says park, and parking also avoids
   the branch-derived instance-slug hazard at `docs/SESSION_STATE.md:190-194`
   (switching a worktree's branch gives the desktop app a different provider
   identity and a different session set).
