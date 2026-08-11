# Integration workflow — agiterra/buzz

How this fork tracks upstream `block/buzz` while keeping each feature separately
maintainable and upstreamable. Quick version: [CONTRIBUTING-FORK.md](../CONTRIBUTING-FORK.md).

## Branch roles

| Branch | Meaning | History |
|---|---|---|
| `main` | Pure mirror of `block/buzz` main. **Never** carries local commits. | ff-only |
| `feature/<name>` | One upstreamable feature. Based on `main`, or stacked on another feature. No CI files, no agiterra-only bits. | rebased per sync |
| `integration/glue` | Cross-feature adaptation patches + `scripts/integrate.sh` + this doc + `.woodpecker/` CI. Merged last into `integrated`. | rebased per sync |
| `integrated` | The assembled product: `main` + every feature + glue. **Deploys and daily work use this.** | rebuilt + force-pushed; pin via `build/*` tags |

`integrated` is rewritten on every rebuild (like linux-next). Consumers re-fetch
rather than pull; anything that must not move pins a `build/YYYY-MM-DD[.n]` tag.
Relay images are tagged with the build tag they were built from.

Current stack:
- `feature/project-containers` — projects as containers (channels/forums/repos/workflows under projects; sidebar + management UI)
- `feature/project-access` (stacked on containers) — visibility levels (`buzz-access`), project ACL, private-repo gating

## The sync loop

```
scripts/integrate.sh              # full: sync main, rebase stack, rebuild, gate, tag, push
scripts/integrate.sh --no-push    # dry run locally
scripts/integrate.sh --skip-gate  # when you've just run the gate manually
```

`git rerere` is enabled by the script: conflict resolutions are recorded and
replayed automatically on the next rebuild, so recurring conflicts are
resolved once. A **new** conflict stops the script — resolve it, `git rebase
--continue`, and rerun.

## Adding a feature

1. `git checkout -b feature/<name> main` (or stack on another feature if it
   genuinely depends on it — prefer independence).
2. Keep it upstream-clean: no `.woodpecker/`, no deploy tooling, no references
   to other features. Cross-feature adaptation goes in `integration/glue`.
3. Add the branch to `FEATURES` in `scripts/integrate.sh` (on `integration/glue`),
   in merge order (a stacked branch after its base).
4. Run `scripts/integrate.sh`.

## Retiring a feature (accepted upstream)

When upstream merges a feature, at the next sync the rebase collapses to
nothing (or to a small residual diff). Delete the branch, remove it from
`FEATURES`, keep any residual as a glue patch until upstream releases it.

## Upstreaming a feature

1. Create (once) a **public** GitHub fork of `block/buzz` — private repos
   cannot open PRs against public upstream. Suggested: fork under the GitHub
   user proposing the PR.
2. `git push <public-fork> feature/<name>` (freshly rebased on `main`).
3. Open the PR from the public fork. Review feedback lands on the same branch;
   the private repo keeps consuming it via the normal sync loop.

## CI

`.woodpecker/` pipelines (on `integration/glue`, therefore present on
`integrated`) run the gate on pushes to `integrated` and PRs. Feature branches
are validated locally by `integrate.sh`'s gate — they intentionally carry no CI
files so they stay upstream-clean.

Known runner-environment limitations (excluded from the gate, still run on
dev machines; both are upstream-issue candidates):
- `buzz-relay` `api::mesh_demo::…round_trips_echo` — loopback QUIC cannot
  complete inside the docker-in-incus runner (10s ECHO_TIMEOUT expires; the
  test self-skips on redis-less dev machines anyway).
- `buzz-agent` `fake_llm::cancelled_turn_with_usage_emits_notification_before_response`
  — the test releases its gated LLM round right after sending cancel, assuming
  stdin wins the race; on loaded cores round 2 completes first and the turn
  legitimately ends `end_turn`, not `cancelled`. Test-design race.
Re-check both if the runner topology changes.

Mirrors + CI run on the `forge` incus container on agincus (bare mirrors at
`/srv/git`, Woodpecker at `ci.agiterra.org`). GitHub remains the canonical
host; the forge is additive infrastructure.

## Deploying

Build the relay image from an `integrated` build tag (image tag = build tag)
via the agincus in-container build loop; see the ops runbook. Desktop dev runs
`just desktop-standalone` from `integrated`.
