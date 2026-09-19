# Pivot Test — first live proofs of the agents-repository pivot on hive, 2026-09-19

Run by Brian in the installed `edbc430dd` bundle (built 10:45Z from Andy's
main), Fable checking the relay and the disk. Project **Pivot Test**
(`30621:3d3b7169…:pivot-test`), code checkout `/Users/brian/Projects/pivot-test`.
Ledger item 171 holds the findings; this file is the run.

## Results

| Step | What | Result |
| --- | --- | --- |
| 1 | Create a project from the app | **Pass.** Head names both repositories; `pivot-test-beekeeper-agents` seeded on the relay (team.yml, actions.yml, eight roles, skills/, plans/, archives); source pinned `refs/heads/main` path `.`; eight default agents installed locally, Lead persistent, all pinned to Claude. |
| 2 | Team session, hire Builder and Verifier | **Pass on the second attempt.** First attempt refused `HIRE_CHECKOUT_NOT_RECORDED` from a stale workdir snapshot (ledger 167, fixed and landed as `29344c1ea`); every credentialed fetch then failed on Apple git 2.39 (ledger 168, lane in flight; workaround `launchctl setenv PATH`). After both: both hires first try, grants accepted, worktrees from the project checkout at `e682191`, bundles composed with `composeRef` from agents-repo commit `6405b21f`. |
| 3 | Hire a runner whose role has `agents_repo: read` | **Fail.** Worktree cut, then the agents clone from the packs cache was refused (`protocol.file.allow=never`) and the error never became a refusal; the lead waited 120 s (ledger 169, lane in flight). |
| 4 | Publish `describe-checkout` (manual `run_on_host`) and run it | **Pass, with three UI gaps.** Published via `bee actions publish`; the Actions tab never showed it; the inbox showed the approval request as raw JSON with no approve control; approved by a hand-signed kind 46030; the run then sat in `waiting_host` because the provider's outbox was poisoned by 150 stale rows (ledger 170, lane in flight); after clearing them and relaunching, the host ran the step: exit 0, `e682191 Initial commit`, head `e682191`, clean, 19 ms, artifact recorded. |
| 5 | Tank Loop regression hire | Not run today; the 2026-09-16 hire path is unchanged and passed on `7fcc45a04`. |

Also seen: hive answered 502 for about six seconds at 11:47:47Z and rate-gated
the provider's burst of catch-up publishes after the relaunch; both recovered.
