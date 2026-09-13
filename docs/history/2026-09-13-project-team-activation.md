# Project team activation — September 13

The first-project setup path connects the checked draft to publication,
installation on this computer, and a project lead session. Ordinary Solo
sessions remain independent. This is implementation and focused verification
evidence; no live Tankloop mutation or installed-app acceptance is claimed.

## What the host now owns

- For a project without a source, derive an owner/project-qualified pack
  repository, reserve and retain its signed announcement, build Git objects
  from the verified snapshot bytes, and publish an isolated candidate ref.
  Git filters cannot rewrite those bytes. Adoption uses the conditional source
  transaction and the exact immutable commit.
- Reopening and matching retries recover the same journal before evaluating
  new-publication conditions. Uncertain pushes/source writes reconcile before
  replay; historical acceptance is distinct from current adoption.
- Install from the adopted repository/SHA/path. Persist an encrypted identity
  plan before updating local stores so an interrupted install recovers the same
  agents. A new source cannot silently change this operation's revision.
- Reserve and retain one project channel UUID and signed create request before
  submission. Reserve the lead session and its signed requests before sending,
  stage its explicit adopted pack, and verify the returned full PackRef.

The UI shows publication, installation and lead state separately. It calls the
host's channel recovery operation directly and recovers the same lead on reopen.
Its draft text no longer claims “not published” after a version is adopted.

## Verification

- Native focused filter `project_team_setup::publication`: 9 passed, zero
  failed. It includes exact Git blobs with hostile attributes, candidate
  recovery, project-qualified destinations, encrypted identity-plan recovery,
  saved channel request identity and immutable role staging.
- Setup UI focused suites: 12 passed, zero failed; TypeScript passed.
- Fresh E2E build and the single first-project workflow on port 4196: one
  passed in 3.8 seconds. This is a mock-bridge UI proof, not a native relay
  publication proof. The native tests above check the corresponding host seams.
- Final review found and closed eight concrete blockers. See
  [the review and corrective closure](2026-09-13-activation-final-review.md).
  No additional full smoke or CI run was used for this slice.

Commands and lane details are recorded in
[publication closure](2026-09-13-activation-publication-blockers.md),
[native progress](2026-09-13-activation-native-progress.md), and
[UI progress](2026-09-13-activation-ui-progress.md). Browser/build/UI logs are
local-only under `../review-2026-09-13-final-ci-sol/activation-*`.

## Limits

This publishes an explicitly selected checked snapshot; it does not infer
permission or completion from model prose. Fresh replacement of an existing
project source is refused because neutral drafts do not carry maintenance seed
provenance. Existing matching publication retries remain available. Automatic
agent-completion publication and existing-source maintenance are future work.

No production relay deployment, live Tankloop setup, or application installation
was performed during these checks. The separately accepted dense-history
browser failure is unaffected and remains unresolved.

## Finalization

Rebased the single activation commit onto published main `060052b33` on
2026-09-13 without conflicts. The first commit hook used stale Biome 2.4.7
from the desktop dependency tree; wiring the existing root dependencies made
pnpm use the lockfile-pinned 2.4.16. The normal commit hooks then passed.
No CSS behavior or lint rules were changed to accommodate the stale tool.

The first normal push ran all 9,359 desktop tests successfully, then found two
Tauri lint issues. The outside-pack predicate was simplified equivalently and
the command's injected-argument count was documented explicitly. Native format
and all-target Tauri clippy then passed; no additional full smoke run was used.

## Push-hook test isolation finding

The second normal push passed desktop tests and Tauri clippy, then failed eight
native tests. The temporary Git helpers inherited the hook's repository
selection environment: their `git add`/`commit` reached the activation worktree's
index instead of their temporary repositories. The log names that exact index
lock. One helper produced test commit `e8f230fc5` containing only a generated
README; another left the shared repository's `core.bare` setting true.

Root retained that commit under `recovery/activation-hook-test-e8f230fc5`, reset
only the generated commit back to intended `de47f5961`, and restored
`core.bare=false`. Main remained `060052b33`; its only untracked file was Brian's
existing Opus handoff. Nothing from the failed push reached the remote.
The correction isolates Git subprocesses in the fixture helpers and sanitizes
repository-selection variables before the floor invokes its checks.
See ledger item 124 and local log `activation-push-2.log`.

Isolation verification: the hook regression performs a real linked-worktree
push and rejects Git-local variables in every stub tool (82 checks passed).
The native helpers passed 15 worktree-prune and 22 seat-hook tests with
`GIT_DIR`, `GIT_WORK_TREE` and `GIT_COMMON_DIR` injected from a disposable
sentinel repository. Its HEAD, index tree, `core.bare=false` and clean status
were unchanged. Native formatting and diff checks passed.
