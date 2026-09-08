# Role adoption evidence — Fable slice (2026-09-07 evening)

Scope: `docs/COLLABORATIVE_WORKSPACE_PLAN.md` § "Fable: shared role adoption
(step 1)". Orchestrator: Fable. Worktree
`/Users/brian/Projects/beekeeper/review-role-adoption-fable`, branch
`work/role-adoption-fable`, base `7c5c6e50b` (shipped main).

## 1. What already proves each fact (traced, with citations)

| Fact | Producer | Authority | Store | Consumer | Surface |
| --- | --- | --- | --- | --- | --- |
| Configured | kind 30624 (`crates/buzz-core/src/project_pack_source.rs`), published by `projectPackSource.ts:150-205`, `packs_repo.rs:414-433`, `bee packs set-source` | relay `handlers/pack_source.rs` (owner/founder), fail closed | relay | `role_packs_view.rs:427-470` (host), `projectPackSource.ts:69-148` (renderer) | Project settings Packs row; Packs tab source sentence |
| Resolved here | `packs_cache::sync_packs_checkout` (`packs_cache.rs:390-466`) via `list_project_role_packs` | n/a (local git) | `<app data>/packs/<owner8>-<id>` checkout | `useRolePacksQuery` (`useProjectPacksView.ts:52-92`) | "Available here" rows (`RolePackSnapshots.tsx`) |
| Staged | `stage_coding_session_actor_seat` (`actor_seats.rs:765-808`) | host-local | `actor-seats.json` (0600) | provider `lib.rs:2314-2352` (create), `lib.rs:3193-3241` (resume) | pending-session "staged from" line |
| Executed | provider-signed kind 44223 `packRef` (`lib.rs:4679-4688`), per generation | relay kind-family + `h` gate only; open-mode catalog labels every row unverified | relay | `useCodingSessionCatalog.ts:322-365` → `rolePackSnapshots.ts` | "Unverified channel metadata" rows |
| Invalidation | 30624 / 30618 live hints (`rolePacksLiveInvalidation.ts:52-107`) | n/a | React Query | `useProjectPacksLiveInvalidation` | resolver refetch |

**Resume adoption boundary (from provider behaviour, unchanged by this slice):**
a resume is a new generation with its own custody entry; the desktop re-resolves
the project's current 30624 and re-stages (`codingSessionSeatedCreate.ts:146-220`,
`codingSessionResumeSeat.ts:56-73`), and the provider writes that generation's
`pack_ref` and publishes it on the new generation's 44223
(`lib.rs:3220-3227`, `lib.rs:3325-3332`). So: **a running generation never
changes packs; the next launch or resume adopts the current project revision.**
Nothing in this slice resets an execution or restages its native context.

## 2. What is missing (the gap this slice closes)

1. The Packs tab compares a reported sha with this machine's resolution only
   as equal / not equal (`claims-match` / `claims-differ`). It cannot say that
   an old execution is on an *earlier* project revision, nor that this
   machine's own resolution is behind what another machine reports, nor that
   a reported commit is unknown to this machine at all.
2. Reported rows carry a timestamp but no age, no status word, and no
   statement of when the execution would adopt a newer revision.
3. No test exercises two hosts resolving one advancing source.

Not in scope (recorded as residuals): a persisted "last resolved" time in the
host; commissioning verification for open-ingress 44223 signers (the umbrella
fold's `operatorPubkey` exists but open-mode receipts are still forgeable by
two keys — the row stays "unverified"); the checkout rung in the view.

## 3. Design

### 3a. New read-only host command: `compare_project_pack_revisions`

Reads the packs checkout that `list_project_role_packs` already syncs. **No
fetch, no checkout, no write.** Answers, for each sha the renderer saw on a
44223, how it relates to the commit this machine's checkout is on.

```
compare_project_pack_revisions(project_ref: string, shas: string[])
  -> ProjectPackRevisionComparison
{
  "repo": string | null,        // 30617 coordinate of the project's 30624 source; null when the project names none
  "currentSha": string | null,  // HEAD of this machine's packs checkout (what the last list landed on); null when no checkout or no source
  "comparedAt": number,         // unix ms when git answered
  "reason": string | null,      // why currentSha is null (no source / no checkout yet / git error), verbatim
  "relations": [ { "sha": string,
                   "relation": "current" | "earlier" | "later" | "unrelated" | "unknown-here",
                   "behind": number | null,   // commits sha..HEAD when "earlier"
                   "ahead": number | null,    // commits HEAD..sha when "later"
                   "note": string | null } ]  // per-row disclosure for "unknown-here" (git's own words, shallow checkout, absent object); null otherwise
}
```

Ancestry is three-way (added at Astra's review): `merge-base --is-ancestor`
exit 0 = yes, exit 1 = no, anything else = indeterminate → `unknown-here`
with git's stderr in `note`. `unrelated` is reachable only after git answered
"no" both ways. `rev-parse --is-shallow-repository` runs once per comparison;
in a shallow checkout every present commit other than HEAD is `unknown-here`
with a shallow note (same-sha `current` stays knowable). Git status is read
through the additive `run_git_status` in
`desktop/src-tauri/src/commands/project_git_exec.rs`; `run_git` is a thin
wrapper over it with unchanged behaviour.

Rules:
- Every `sha` must be 40 lowercase hex; otherwise the whole call is an
  `Err` (input error). The renderer filters shipped (`app:shipped`) refs out
  before calling.
- `current`: sha == HEAD. `earlier`: `git merge-base --is-ancestor sha HEAD`
  → `behind = git rev-list --count sha..HEAD`. `later`: HEAD is an ancestor of
  sha → `ahead = rev-list --count HEAD..sha` (this machine's resolution is
  behind what an execution reports). `unrelated`: both commits present,
  neither is an ancestor. `unknown-here`: `git cat-file -e sha^{commit}`
  fails (not in this checkout's object store).
- When `currentSha` is null every relation is `unknown-here`, and `reason`
  says why; the call is still `Ok`.
- Git runs through `commands::project_git_exec::run_git` (sanitised env,
  `GIT_DIR` and friends removed, no credential helper). Local-only commands.
- The 30624 is read the way `list_project_role_packs` reads it
  (`fetch_project_pack_source`), and the checkout dir is derived the same way
  (`packs_cache::packs_checkout_dir`). No new store.

### 3b. Renderer model (`desktop/src/features/roles/`)

`ReportedRolePackSnapshot` gains:
- `relation`: `"current" | "earlier" | "later" | "unrelated" | "unknown-here" | "different-source" | "incomplete"`.
  `different-source` when the coordinate's repo differs from the project's
  current source repo (including shipped defaults); `incomplete` when no
  full coordinate. Replaces `comparison`.
- `behind` / `ahead`: numbers or null.
- `status`: the catalog record's status word verbatim; `ageSeconds` from
  `seatAgeSeconds(statusAt, nowSeconds)` (reuse `seatRows.ts`).
- `adoption`: `"keeps-until-next-generation"` for non-current relations while
  the execution is not closed/terminal; `"none"` otherwise. Copy: "Keeps this
  revision until its next launch or resume."

`RolePackSnapshots` gains `comparison: { currentSha, comparedAt, reason } | null`
and the "Available here" heading says which commit this machine's checkout is
on and when git answered.

`useProjectPacksView` adds one React Query keyed by
`rolePackRevisionsQueryKey` (`["role-pack-revisions", projectRef,
sourceEventId, sourceRepo, currentResolvedSha, packsUpdatedAt, sortedDistinctShas]`,
so a source change, a repository switch, or a packs refresh that only changed
object availability re-ranks; tested in `rolePacksLiveInvalidation.test.mjs`),
enabled when the reported rows carry at least one git-shaped sha, calling
`compareProjectPackRevisions`. A failed comparison leaves every row
`unknown-here` with the error sentence disclosed in the section (never
"current").

Copy lives in `rolesCopy.ts` as exported constants. Wording (final):
- current → "Same revision as this machine"
- earlier → "Earlier revision · N behind this machine"
- later → "Newer than this machine's copy · N ahead — this machine has not refreshed"
- unrelated → "Different history from this machine's copy"
- unknown-here → "Revision unknown to this machine"
- different-source → "Different pack source"
- incomplete → "Version claim incomplete"
- shipped-differs → "Shipped defaults from a different app version"
  (added at review: with no project source, a shipped claim is compared
  with what this machine would stage for the same role — same app version
  is `current`, another is `shipped-differs`, a role this machine does not
  bundle is `unknown-here`; while the project names a repository a shipped
  claim is `different-source`. A repository claim is only ever called
  `different-source` once the 30624 read succeeded (`sourceKnown`); a failed
  source read leaves it `unknown-here`.)
- age → "reported <age> ago" using `formatCoordinationAge`; null → "time not reported"

### 3c. Two-host proof

`pack_revisions_tests.rs`: one bare repo, two checkouts (host A, host B).
Commit 1 → both sync (`packs_cache::sync_packs_checkout`, file transport
allowed as the existing packs_cache tests do). Commit 2 on the branch → host A
syncs only. Assert on A: c1 = earlier/behind 1, c2 = current. Assert on B
(never refreshed): c1 = current, c2 = unknown-here. Assert an unknown random
sha = unknown-here on both, an invalid sha = Err, and a missing checkout =
`currentSha: null` with reason and all `unknown-here`.
**Label:** this simulates two isolated hosts on one machine; no real
second machine was available.

## 4. Lane ownership (strict; builders do not commit)

| Lane | Model | Owns (exclusive) | Gate |
| --- | --- | --- | --- |
| R (host) | opus | `desktop/src-tauri/src/managed_agents/pack_revisions.rs` (new), `pack_revisions_tests.rs` (new), `desktop/src-tauri/src/managed_agents/mod.rs` (one `mod` line), `desktop/src-tauri/src/commands/role_packs.rs` (add the command), `desktop/src-tauri/src/handlers.rs` (one registration line) | `cargo fmt`, `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path desktop/src-tauri/Cargo.toml pack_revisions role_packs` |
| T (renderer) | sonnet | `desktop/src/shared/api/types/rolePacks.ts`, `desktop/src/shared/api/tauriRolePacks.ts`, `desktop/src/testing/e2eBridge.ts` (one handler + switch arm), `desktop/src/features/roles/lib/rolePackSnapshots.ts` + test, `desktop/src/features/roles/lib/useProjectPacksView.ts`, `desktop/src/features/roles/ui/RolePackSnapshots.tsx` + test, `desktop/src/features/roles/ui/rolesCopy.ts` + test, `desktop/src/features/roles/ui/ProjectPacksScreen.tsx`, `desktop/tests/e2e/role-packs-project.spec.ts` | `pnpm typecheck`, `pnpm check`, `pnpm test` (roles + shared/api), `pnpm check:file-sizes` |
| Finalizer | Fable | review against §3, run the pre-push set, commit with `-s` | |

Shared-type needs for Astra: none. No `buzz-core` type changes, no provider
changes, no CLI changes, no `actor_seats` edits.

## 5. Acceptance mapping

| Plan acceptance | Evidence |
| --- | --- |
| authoritative source advances | existing 30624/30618 live refresh (`rolePacksLiveInvalidation.test.mjs`) + `list` resync |
| old execution remains truthfully old | reported row `earlier · N behind`, `keeps this revision until its next launch or resume` |
| subsequent launch reports the new artifact | provider re-stage per generation (`lib.rs:3325-3332`), row `current` |
| unavailable machine has unknown freshness | that machine publishes no new 44223; its rows show their real age and, on the other host, `unknown-here`/`earlier`; two-host Rust test |
| source failure keeps honest stale/error state | existing `packsResolutionIsStale` + comparison error → `unknown-here` with reason |
| no reset / no silent restage | no provider or seat code touched |
| one provider/subscription | no new authority, no reviewer requirement |

## 6. Validation record (2026-09-07 late evening)

Logs: `/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/`.

| Check | Command | Result | Log |
| --- | --- | --- | --- |
| Host tests | `cargo test --manifest-path desktop/src-tauri/Cargo.toml pack_revisions` | 6 passed, 0 failed | `final-pack-revisions-after-auth-edit.log` (lane run: `laneR-tests.log`, also `role_packs` 15 passed) |
| Tauri fmt / clippy | `cargo fmt … --check`; `cargo clippy … --all-targets -- -D warnings` | exit 0 / exit 0 | `final-tauri-fmt.log`, `final-tauri-clippy.log` |
| Renderer focused | roles + shared/api node tests | 338 passed, 0 failed | `final-roles-tests.log` |
| Renderer full | `pnpm test` | 8319 passed, 0 failed, 0 skipped | `final-desktop-tests.log` |
| Renderer checks | `pnpm typecheck`; `pnpm check`; `pnpm check:file-sizes` | exit 0 | `final-typecheck.log`, `final-desktop-check.log` |
| Ratchet | `just file-size-check` | exit 0 | `final-file-size.log` |
| E2E | `pnpm build:e2e` + `playwright test tests/e2e/role-packs-project.spec.ts --project=smoke` | 2 passed | `final-e2e.log` |

Review findings fixed before commit: (1) the blocking wrapper used the
signing-key git config for local reads; switched to
`build_local_git_auth_config` so a signed-out machine can still answer.
(2) The renderer called every claim `different-source` when the project
named no source or the source read failed; see §3b `shipped-differs` /
`sourceKnown`. Five model tests added for those cases.

Limitations stated: the two-host proof is two checkouts on one machine; the
E2E fixture carries no reported 44223 rows, so the relation rows are proven
by unit tests and the renderer test, not by a browser run; open-ingress rows
remain "unverified" (commissioning is not established); no real second
machine, no relay deployment, no app rebuild is part of this slice. Lane R
symlinked `desktop/src-tauri/binaries` to the main checkout's sidecars so the
worktree could compile; it is gitignored and read-only.

### 6a. Round two (Astra's source review + browser proof)

Fixed: ancestry indeterminacy and shallow history (§3a), query-key
invalidation (§3b), per-row `note` rendered on the Packs tab. Browser proof
added: `role-packs-project.spec.ts` seeds two signed 44223 generations for the
general project (fresh keys, the existing `__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__`
seam) and a per-sha mock ranking knob (`config.mock.packRevisionRelations`),
then asserts the `current` and `earlier · 1 behind` rows, the age, the status
word, the adoption sentence and the checkout header; screenshot
`desktop/test-results/role-pack-snapshots/ranked-rows.png` (hash distinct from
the empty-state capture, asserted in-test).

| Check | Result | Log |
| --- | --- | --- |
| Tauri fmt / clippy all-targets | exit 0 / exit 0 | `final2-tauri-fmt.log`, `final2-tauri-clippy.log` |
| `pack_revisions` / `role_packs` / `project_git_exec` tests | 8 / 15 / 10 passed, 0 failed | `final2-tauri-tests.log` |
| desktop typecheck, `pnpm check`, file sizes | exit 0 | `final2-typecheck.log`, `final2-desktop-check.log` |
| desktop unit suite | 8334 passed, 0 failed, 0 skipped | `final2-desktop-tests.log` |
| Packs E2E spec, rebuilt bundle, `--repeat-each 2` | 6 passed | `final2-e2e.log` |
| `just file-size-check` | exit 0 | `final2-file-size.log` |

## 7. Ledger text for Astra (factual, for SESSION_STATE)

Role adoption evidence (Fable, topic `work/role-adoption-fable`): the Packs
tab now ranks every execution-reported pack revision against this machine's
packs checkout through a new read-only host command
(`compare_project_pack_revisions`: current / earlier N behind / later N ahead
/ unrelated / unknown-here), shows each claim's age, status and "Keeps this
revision until its next launch or resume", and states which commit the
checkout is on and when git answered. No fetch, no new store, no polling, no
provider or seat change; the adoption boundary is the generation, as the
provider already behaves (`crates/buzz-session-provider/src/lib.rs:3325-3332`).
A simulated two-host Rust test shows the un-refreshed host answering
`unknown-here` for the advanced commit rather than "current"; a git failure
or a shallow checkout is disclosed per row as `unknown-here` with git's words,
never as "unrelated". A browser test seeds two signed 44223 claims and shows
the `current` and `earlier · 1 behind` rows. Open-ingress 44223 rows stay
labelled unverified. Validation in §6 of
`docs/ROLE_ADOPTION_EVIDENCE.md`.
