# Releasing Beekeeper

> **No desktop or mobile build is published or signed today.** Beekeeper began
> as a fork of block/buzz, and every lane that built, signed, tagged and
> published a release ran on Block's infrastructure: the `buzz-release-bot`
> GitHub App, the `block/apple-codesign-action` signing role, Block's
> tag-protection rulesets, the `ghcr.io/block/buzz` image registry and Block's
> private Buildkite mobile pipelines. Those workflows and the scripts that served
> them have been removed from this repository (§ What was removed). What remains
> prepares release pull requests on `agiterra/beekeeper`; **a publishing and
> signing lane of agiterra's own must be built before any release ships.**
>
> What does work today, and is exercised continuously: the relay deploys itself
> from the newest green `main` pipeline, building its own image on the relay
> host. See [docs/INTEGRATION.md](docs/INTEGRATION.md) § Deploying.

| Lane | Entry point | What it produces today |
|------|-------------|------------------------|
| Relay | none needed | hive runs the newest green `main` (`beekeeper-autodeploy.timer`); no image is pushed to a registry |
| Relay version | `just release-relay [X.Y.Z]` | A PR bumping `crates/buzz-relay` and its changelog. Nothing tags or builds from it |
| Desktop | `just release-desktop [X.Y.Z]` | A deterministic release-candidate PR on `agiterra/beekeeper`. Nothing tags, builds or publishes from it |
| Mobile | none | No release lane exists |

---

## Desktop

Run from a clean checkout that can fetch `main` (the remote defaults to
`origin`; set `RELEASE_REMOTE` to use another):

```sh
just release-desktop 0.5.3     # or `patch` / no argument for the next patch
```

`scripts/prepare-desktop-release.sh`:

1. Fetches `main` and the existing `desktop-v*` tags, and freezes the base at
   the fetched `main` tip.
2. Checks out `version-bump/<version>`, runs `just bump-desktop-version`, and
   has `scripts/desktop_release.py generate` write `CHANGELOG.md` and
   `.release/desktop-candidate.json` (frozen base, prior release, commit list).
   Changelog links point at `github.com/agiterra/beekeeper`.
3. Commits the candidate as you, signed off (`git commit -s`), and validates it
   with `scripts/desktop_release.py validate`.
4. Pushes the branch to hive through `scripts/push-with-floor.sh` (the same
   floor `just push` runs), waits for the hive-to-GitHub bridge to mirror it,
   and opens or updates the PR on `agiterra/beekeeper` with `gh`.

`scripts/prepare-desktop-release.sh <version> validate-only` stops after step
3, without pushing.

**The PR is for review only — do not merge it on GitHub.** GitHub is a
mirror of hive: a merge there writes the mirror, never reaches hive, and races
the bridge ([docs/INTEGRATION.md](docs/INTEGRATION.md) § Remotes). To land a
reviewed release, fast-forward `main` to it and push to hive with `just push`.
Landing records the version and changelog. **No tag is created and no build
follows**: the auto-tagger and `release.yml` that used to do that were
Block's and are removed. The candidate metadata stays useful as the ledger
boundary for the next release whenever a publishing lane exists.

To get an installable build today, build locally: `just desktop-release-build
[target]` (unsigned) or `just prod-desktop [rev]`
([docs/local-desktop-instances.md](docs/local-desktop-instances.md)).

The in-app updater has no endpoint configured
(`desktop/src-tauri/tauri.conf.json`), so update checks fall back to linking
<https://github.com/agiterra/beekeeper/releases/latest>, which has nothing on it
until a release is published there.

### Unsigned platform canaries

`.github/workflows/{linux,macos-intel,windows}-canary.yml` build unsigned
packages of `main` on manual dispatch and upload them as short-lived Actions
artifacts. They are guarded to run only on `agiterra/beekeeper`, and GitHub
Actions is currently disabled there, so they do not run today. The Linux
AppImage is post-processed by `desktop/scripts/fix-appimage.sh`, which strips
infra libraries over-bundled by linuxdeploy (they crash on Mesa 25+ / GLib 2.88
distros; see
[tauri-apps/tauri#15665](https://github.com/tauri-apps/tauri/issues/15665)), so
it relies on the host's Wayland/GStreamer/graphics stack and needs GLib >= 2.72.

---

## Relay

The relay needs no release step to reach hive: the deployer builds and runs the
newest `main` commit with a green Woodpecker pipeline. NIP-11 reports exactly
which commit is running (`software_commit`; see [AGENTS.md](AGENTS.md)).

`just release-relay [X.Y.Z]` remains for recording a version: on a clean,
up-to-date `main` it creates `relay-release/<version>`, bumps
`crates/buzz-relay/Cargo.toml`, regenerates `Cargo.lock`, prepends
`crates/buzz-relay/CHANGELOG.md`, pushes, and opens or updates the PR on
`agiterra/beekeeper` for review — land it on hive the same way, never by merging
on GitHub. Landing it tags nothing and publishes no image.

---

## Mobile

There is no mobile release lane. Candidate tags were published by a Block-owned
GitHub App and built by Block's private Buildkite pipeline; both are gone, and
`scripts/mobile-release.sh` was removed with them. `mobile/pubspec.yaml` keeps
`0.0.0+1` as a visibly non-release version for local builds, and
`mobile/CHANGELOG.md` is historical release data.

---

## Version sources

| Lane | Release version authority |
|------|---------------------------|
| Desktop | `desktop/package.json` and the synchronized desktop manifests (`just bump-desktop-version`) |
| Relay | `crates/buzz-relay/Cargo.toml` (`just bump-relay-version`) |
| Mobile | none |

---

## What was removed

Workflows (all ran only on, or published through, Block's infrastructure):
`release.yml` (desktop build, signing, GitHub Release), `docker.yml` (relay
images to `ghcr.io/block/buzz`), `sprig-image.yml`, `signed-macos-canary.yml`,
`desktop-release-cache-proof.yml`, `desktop-release-candidate.yml`,
`mobile-release-candidate.yml`, `auto-tag-on-release-pr-merge.yml` and
`promote-oss-desktop-release.yml`; and the scripts that served only them
(`mobile-release.sh`, `publish-mobile-release-candidate.sh`,
`promote-oss-desktop-release.sh`, `release-rulesets.sh`,
`verify-release-ref.sh`, `verify-desktop-release-merge.sh`, their `.jq`
filters and contract tests). The relay deployment-identity attestations that
`docker.yml` produced went with it.

## Building a publishing lane

Whoever builds one needs, at minimum, decisions on:

- **Where it runs.** Woodpecker (`ci.agiterra.org`) is the live CI; GitHub
  Actions is disabled on `agiterra/beekeeper`.
- **macOS signing and notarization** with an Apple Developer identity of
  agiterra's own. `desktop/scripts/build-release-config.mjs` still emits a
  release Tauri config with `--no-sign` in mind, and `scripts/stage-menubar.sh`
  notes the tray app's entitlements over-grant to fix on the way.
- **Tauri updater keys and an endpoint**, so installed apps can update.
- **Tag protection**, if releases are to be bound to immutable tags again.
- **A mobile build and store-submission path.**
