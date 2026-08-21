# Local desktop instances: production + dev, side by side (macOS)

Two coexisting desktop instances built from `integrated`:

| | Production | Dev |
|---|---|---|
| What | Installed `/Applications/Bee Keeper.app` | `just desktop-standalone` (tauri dev) |
| Source | newest `build/*` tag, dedicated worktree `~/Code/lightyear/buzz-prod` | main checkout (parked on `integrated`; re-fetch after ceremonies) |
| Identifier | `io.agiterra.beekeeper` | `io.agiterra.beekeeper.dev` (worktrees: `.dev.<slug>`) |
| Icon | stock Buzz | "dev"-badged (worktrees: branch-labelled) |
| Secrets | OS keychain, service `buzz-desktop` | 0600 files (with `nokeyring`), or keychain `buzz-desktop-dev[.slug]` |
| Data | `~/Library/Application Support/io.agiterra.beekeeper`, nest `~/.beekeeper` | `…/io.agiterra.beekeeper.dev*`, nest `~/.beekeeper-dev` (shared by all dev instances) |

They never collide: the single-instance lock, app-data dir, and keyring all key
off the identifier or build profile. `beekeeper://` deep links go to the installed
production bundle (tauri-dev instances don't register the scheme). Per-feature
worktree instances keep working unchanged alongside both.

## Production app

```bash
cd ~/Code/lightyear/buzz-integration
just prod-desktop                      # newest build/* tag
just prod-desktop tag=build/2026-08-12 # or pinned
```

`scripts/local-prod-build.sh` builds in the detached worktree
`~/Code/lightyear/buzz-prod` (override: `BUZZ_PROD_WORKTREE`): release
sidecars + `buzz-session-provider` (added to the bundle via
`desktop/src-tauri/tauri.local-prod.conf.json`), then
`pnpm tauri build --bundles app`, verifies the bundle, and installs to
`/Applications` (refuses while the app is running; `--no-install` to skip).

The bundle is unsigned (linker ad-hoc), like `just desktop-release-build`.
Consequence: **the first launch after every update asks for the login keychain
once** — the keychain ACL is bound to the binary's signature, which changes
per build. Allow it and move on. (If that ever gets annoying, a persistent
self-signed cert set as `bundle.macOS.signingIdentity` in the delta config
would make the ACL stick; deliberately not wired up.)

First run ever: import your nsec, allow the keychain prompt, add the community
relay. The worktree's `target/` dirs cost 15–25 GB; `cargo clean` in
`buzz-prod` reclaims them between updates.

## Dev instance (zero keychain prompts)

```bash
export BUZZ_DESKTOP_NOKEYRING=1   # once, in ~/.zshrc
cd ~/Code/lightyear/buzz          # main checkout, parked on integrated
just desktop-standalone
```

With `nokeyring` active the desktop is compiled without the `system-keyring`
cargo feature (via the `scripts/cargo-strip-keyring.sh` runner wrapper —
tauri-cli re-adds crate default features explicitly, so plain cargo flags
cannot drop one): identity lives in
`<app-data>/identity.key`, agent/provider keys inline in their record files —
all 0600, no keychain access, no prompts, regardless of how often you rebuild.
Toggling the env var flips the cargo feature fingerprint (full desktop-crate
rebuild), so set it once and leave it. `just fresh=1 desktop-standalone`
resets file-based instances too (it wipes the whole app-data dir).

**One-time migration** of an existing keychain-stored dev identity (app not
running; expect one final keychain prompt for the `security` read):

```bash
DIR="$HOME/Library/Application Support/io.agiterra.beekeeper.dev"; mkdir -p "$DIR"
for SVC in buzz-desktop-dev.main buzz-desktop-dev; do
  BLOB="$(security find-generic-password -s "$SVC" -a secrets -w 2>/dev/null)" && break; done
printf '%s' "$BLOB" | python3 -c 'import json,sys; sys.stdout.write(json.load(sys.stdin)["identity"])' > "$DIR/identity.key"
chmod 600 "$DIR/identity.key"
```

Without a seeded `identity.key` the app silently generates a fresh identity.
Managed agents / coding-session providers whose keys live in the old keychain
blob must be recreated once (their nsecs are not migrated).

The main checkout now also gets a "dev"-badged dock icon (previously
worktree-only), so the two instances are visually distinct.
