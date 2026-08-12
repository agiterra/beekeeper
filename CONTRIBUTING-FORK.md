# Contributing to agiterra/buzz (this fork)

This repo is agiterra's integration fork of
[block/buzz](https://github.com/block/buzz). It tracks upstream continuously
while carrying our features as separately-upstreamable branches, assembled
linux-next-style into one product branch. This page is the front door for
working **in this repo**; upstream's own contributor guide is
[CONTRIBUTING.md](CONTRIBUTING.md), and the full mechanics live in
[docs/INTEGRATION.md](docs/INTEGRATION.md).

## Branches at a glance

| Branch | What it is | Commit here? |
|---|---|---|
| `integrated` (default) | The product: `main` + every feature + glue. Deploys and daily work use this. **Rebuilt and force-pushed** on every sync. | No — it's generated |
| `main` | Byte-for-byte, ff-only mirror of upstream `block/buzz` main. | Never |
| `feature/<name>` | One upstreamable feature, cut from `main` (occasionally stacked on another feature). Upstream-clean. | Yes — features live here |
| `integration/glue` | Cross-feature adaptation patches, `scripts/integrate.sh`, `.woodpecker/` CI, these docs. | Yes — tooling & glue |
| `build/YYYY-MM-DD[.n]` (tags) | Immutable pins of assembled `integrated` builds. | — |

## The two rules

1. **`integrated` moves by force-push — never `git pull` it.** Update with
   `git fetch origin && git reset --hard origin/integrated`, or check out a
   `build/*` tag. Anything durable (deploy scripts, images, bisects) pins a
   build tag, never the branch.
2. **Feature branches stay upstream-clean.** No `.woodpecker/`, no deploy
   tooling, no references to other features or fork-only infrastructure —
   the branch must be PR-able against `block/buzz` as-is. Cross-feature
   adaptation belongs on `integration/glue`.

## Adding a feature

1. `git checkout -b feature/<name> main` — branch from `main`, **not** from
   `integrated`.
2. Build it there. Commit with `git commit -s` (DCO sign-off, same rule as
   upstream).
3. Add the branch to the `FEATURES` array in `scripts/integrate.sh` (a commit
   on `integration/glue`), in merge order — a stacked branch after its base.
   That array is the source of truth for what `integrated` contains.
4. Run `scripts/integrate.sh` — it syncs `main`, rebases the stack, rebuilds
   `integrated`, runs the gate, tags, and pushes.

## Staying aware of what's integrated

- **`FEATURES` in `scripts/integrate.sh`** — the current feature stack, in
  merge order.
- **`build/*` tags** — every assembled build; relay images carry the tag of
  the build they were made from.
- **CI** — every push to `integrated` runs the Woodpecker gate at
  [ci.agiterra.org](https://ci.agiterra.org).

Everything deeper — the sync loop, conflict handling (`git rerere`), retiring
a feature once upstream absorbs it, the upstreaming flow, CI details and known
runner limitations — is in [docs/INTEGRATION.md](docs/INTEGRATION.md).

Running a daily-driver Buzz.app and a dev instance side by side on macOS
(distinct icons, no repeated keychain prompts):
[docs/local-desktop-instances.md](docs/local-desktop-instances.md).
