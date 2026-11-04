# Beekeeper Host.app: the standalone macOS package

A package of the Host with no Desktop in it. The existing menu bar is the outer
app, and the Host, the session provider, `bee` and their helpers sit beside it.
This is **local, developer-signed distribution**. It is not a public release; see
[What a public release still needs](#what-a-public-release-still-needs).

Building a package starts nothing. It does not commission, register, install or
read a key. Moving a real Host onto a package is a separate cutover, and it needs
Brian's word.

## Build

Reuse the exact binaries of a built bundle, with no compile:

```bash
scripts/host-package.sh \
  --from-app "$HOME/Applications/Beekeeper Dev.app" --expect-commit <rev> --archive
```

Build from this checkout. Run `. ./bin/activate-hermit` first, for cargo. Neither
command needs Node, pnpm, the Tauri CLI or a frontend build:

```bash
scripts/host-package.sh --build --archive          # release profile
scripts/host-package.sh --build --debug --archive  # a Dev bundle's profile
```

The second builds what `scripts/app-from.sh` builds: debug assertions on, with
`CARGO_PROFILE_DEV_OPT_LEVEL=2`. The eight binaries come from these two commands,
which are what `--build` runs:

```bash
cargo build --release --locked -p beekeeper-host -p beekeeper-session-provider \
  -p beekeeper-cli -p beekeeper-acp -p beekeeper-agent -p beekeeper-dev-mcp -p git-credential-nostr
cargo build --release --locked --manifest-path desktop/src-tauri/Cargo.toml \
  -p beekeeper-menubar --features tauri/custom-protocol
```

`tauri/custom-protocol` is the flag `tauri build` passes. The Manage Host page is
tracked static files, embedded at compile time.

The script writes to `dist/standalone-host/`, which is ignored by git. `--out`
changes that, but it refuses any Applications directory. A `bee` stamped `-dirty` was
built from uncommitted files, so the script refuses it unless `--allow-dirty` is given,
and the manifest, receipt and verifier then all say so. `--identity` picks the
signer: `-` is ad-hoc, and the default is the local `Beekeeper Dev Local Signing`
(`just dev-signing-identity`) if it exists, else ad-hoc.

## What is in it

```
Beekeeper Host.app/Contents/
  Info.plist                 written by the packager
  MacOS/beekeeper-menubar    CFBundleExecutable
  MacOS/beekeeper-host, beekeeper-session-provider, bee, beekeeper-acp,
        beekeeper-agent, beekeeper-dev-mcp, git-credential-nostr
  Resources/icon.icns
  Resources/personas/{roles,templates}
  Resources/host-package.json  commit, instance, sha256 per file
  _CodeSignature/
Beekeeper Host.app.receipt.json  beside the bundle: cdhash, tree digest, what was not done
```

The seven helpers are read from `BINARIES` in `scripts/package-agent-stack.sh`, so
one list serves both packages. A binary added there is required here.

Left out, and refused by the verifier if present: `beekeeper-desktop`,
`beekeeper-shell-host`, `beekeeper-backend-kubernetes`, `Contents/Library`, and any
web bundle. `bee` and the Host name the backend binary only through an explicit
`beekeeper-backend-<id>` lookup on `PATH`, which refuses by name when it is absent,
so a Kubernetes backend is installed separately.

`beekeeper-agent` stays because `beekeeper-acp` can run it. Whether a Host path
needs it was not settled, and it stays on the shared list until that is known.

## Verify

```bash
scripts/host-package-verify.sh --expect-commit <rev> "dist/standalone-host/Beekeeper Host.app"
```

This is read-only and runs on its own, with or without the receipt. It checks:

- `Contents/MacOS` is exactly the list.
- No Desktop parts and no symlinks, and every binary links only system libraries.
- The plist's identity is right, and it claims no URL scheme.
- Resources are present.
- Every file matches its sha256, and none is unlisted.
- `bee`'s commit matches.
- `codesign --verify --deep --strict` passes on the bundle, and each binary verifies
  on its own.
- There are no entitlements.

It also says what it cannot vouch for. Four binaries carry the commit in their bytes,
and for `beekeeper-acp`, `beekeeper-agent`, `git-credential-nostr` and the menu bar
only the source vouches.

The manifest cannot hash the main executable, because signing the bundle rewrites
it. The receipt holds that hash, and the signature covers it.

## Test

```bash
scripts/host-package-test.sh       # fixtures only, about a minute, needs cc
scripts/host-package-test.sh --source-app "$HOME/Applications/Beekeeper Dev.app" \
  --expect-commit <rev> [--identity <name>] [--gui]
```

The header of that script lists what it never does. `--gui` puts the menu bar and a
Manage Host window on screen for a few seconds.

**Keep `HOME` and `BEEKEEPER_HOST_HOME` different in any probe of `bee host install`.**
Registration touches launchd only when the home being installed into is this user's
own (`home_is_ours`, `crates/beekeeper-host/src/install.rs`). With `HOME` and
`BEEKEEPER_HOST_HOME` set to the same temp directory, that check passes and the
install boots out the live Host.

## Identity and login: what is true

- **Bundle id:** `io.agiterra.beekeeper.host`, or `.dev` with `--instance dev`. It is
  distinct from Desktop's (`io.agiterra.beekeeper.app[.dev]`) and from the nested
  menu bar's (`io.agiterra.beekeeper.menubar`). The binary's embedded Tauri config
  still says `.menubar`; macOS reads the plist, and the test confirms what it sees.
- **Which instance a launch serves:** a launchd start reads the registration's
  `BEEKEEPER_HOST_INSTANCE`. A Finder start reads the plist's `LSEnvironment`, set to
  match the package. Only the first of those was run here. A `bee` in a terminal reads
  neither, so against a dev package it needs `--dev` or the variable, and otherwise
  asks the production socket and correctly reports nothing there.
- **`bee host install` registers the Host and only the Host.** Run from this package,
  it names this bundle's `beekeeper-host`. **`bee host install --menu-bar` registers
  this bundle's menu bar and only that**, leaving the Host's job untouched. See
  `docs/agent-host.md`, "From a standalone `Beekeeper Host.app`".
- **It is a LaunchAgent, not `SMAppService`.** `~/Library/LaunchAgents/io.agiterra.beekeeper.{host,menubar}[.dev].plist`.
  The comment in the menu bar's `Info.plist` saying otherwise is stale. How a
  registration appears under System Settings → Login Items for this bundle was not
  observed, because nothing was registered.
- **The labels are per instance, not per bundle.** Installing from this package
  replaces Desktop's registration of the same instance and restarts a running Host.
  `Registration` has one slot, and a running Desktop can take it back
  (`ensure_service`, `desktop/src-tauri/src/agent_host/autostart.rs`).
- **Two Hosts of one instance cannot both run.** The socket and the state-directory
  lock refuse the second.

## Where binaries are found

Inside a `.app`, `resolve_workspace_command` looks only in the bundle's own
`Contents/MacOS` (`crates/beekeeper-host-core/src/command_paths.rs`). The test proves
this against real binaries, from a directory holding `target/debug` and
`target/release` traps and with a poisoned `PATH`.

**A miss refuses too.** With a sidecar removed, the provider does not start and the
provider log names what is missing, whatever `PATH` offers. Only adapters like
`claude-agent-acp`, which a bundle never carries, go on to the wider lookup
(`crates/beekeeper-host/src/discovery.rs`). This is checked when the provider starts, so
a sidecar removed while it runs is noticed at the next start.

The menu bar has no such gap: with no `bee` beside it, Manage Host refuses with
`BEE_NOT_FOUND`.

## What it does not contain

These stay external, as in `docs/agent-host.md`:

- `claude-agent-acp`, `codex-acp`, the `claude` CLI and Node. The only installer for
  them is in Desktop.
- `git` 2.46 or newer, found on `PATH`. The credential helper needs it (ledger 168).
- A relay that speaks Beekeeper's kinds, named at commissioning.

`bee host manage adopt-identity` reads Desktop's keychain entry, and only when asked.
With no Desktop it has nothing to read and refuses. Its grant is tied to the binary's
signature, so an ad-hoc `bee` will not hold one across builds.

## Upgrade and rollback

Keep the previous bundle. A package is immutable: replace the whole `.app`, then restart
the Host explicitly. Replacing a bundle under a loaded job boots it out, so nothing
should be mid-turn. `--force` replaces only an output of this script, and refuses while
anything runs from it. State lives outside the bundle, so it survives; a setup journal
written by a newer build may still be refused by an older one.

## What a public release still needs

None of this exists, and `RELEASING.md` says the same about the signing lane.

- A Developer ID Application certificate. Buying and using it is Brian's call. The Apple
  Development identity on this Mac is not one.
- Hardened runtime and a secure timestamp. **How Seatbelt, TCC and the provider's child
  processes behave under hardened runtime is unknown.** It has never been run.
- A reviewed entitlements file per binary. Today there are none. `Entitlements.plist`
  is Desktop's and carries camera and microphone.
- Notarization and stapling, and a DMG or pkg to staple to. `spctl` rejects this package,
  and a quarantined copy on another Mac will be blocked or translocated.
- A publishing lane, an update path and a detached signature on the archive. The sha256s
  here prove integrity, not origin.
- An x86_64 or universal build. Builds so far are arm64 only.
