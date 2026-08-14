# Handoff to Sol: coding sessions after build `.11`

**Written:** 2026-08-14  
**For:** a fresh Codex `gpt-5.6-sol` session  
**Purpose:** continue the active Buzz coding-session effort without restarting
the investigation or disturbing Andy's integration workflow.

Read `docs/SESSION_VISION.md` after this file. It remains the product authority.
Read `docs/SESSION_HANDOFF_SOL.md` for the earlier implementation history,
security findings, T3 Code study, and ranked roadmap. This document supersedes
that handoff only where the current state below differs.

## 1. State in one paragraph

The provider-neutral, multi-provider session surface shipped into the generated
assembly `build/2026-08-13.11` at `ca80dd17a`; the Git ceremony and Woodpecker
gate succeeded. Brian live-tested Claude and Codex together in one session,
visible prompts, provider transitions, multi-line prompts, and provider-neutral
session titles. The current blocker is a deployment skew: the `.11` desktop
sends the new 9-field `session.create` action containing `sessionRef`, while the
live Lightyear relay still behaves like the old 8-field validator and rejects
the request. The Git ceremony does not actually build or deploy a Lightyear
relay artifact. Separately, a first-membership subscription race was found and
fixed in one signed, unpushed feature commit (`dbc7eb89b`). Do not weaken the
client back to the legacy wire shape; get the matching relay deployed.

## 2. Worktrees and exact Git state

| Path | Branch / state | Rule |
| --- | --- | --- |
| `/Users/briansweet/agiterra/BuzzForkV2` | old generated `integrated`; user-owned local state | Do not commit or clean here. |
| `/Users/briansweet/agiterra/BuzzForkV2-coding-sessions` | `feature/coding-sessions` at `dbc7eb89b`, one commit ahead of origin, clean | Session implementation work goes here. |
| `/Users/briansweet/agiterra/BuzzForkV2-integration-glue` | `integration/glue` at `ca80dd17a`, equal to origin and `build/2026-08-13.11` before this handoff commit | Cross-feature docs/tooling and ceremony live here. |
| `/Users/briansweet/agiterra/t3code` | MIT reference implementation | Borrow UX/logical flow, not its local SQLite architecture. |

Feature tip:

```text
dbc7eb89b fix(coding-sessions): replay first command after membership
dc279564c origin/feature/coding-sessions
```

There is one intentional stash on the feature branch:

```text
stash@{0}: wip: relay-compat legacy create fallback (incomplete, stopped mid-run)
```

Do not apply or delete that stash casually. The preferred model is coordinated
relay deployment, not a partial legacy fallback.

Always activate Hermit before Git or Cargo:

```bash
cd <worktree> && . ./bin/activate-hermit && <command>
```

Commit signed (`git commit -s`). Do not push until Brian explicitly asks for a
new ceremony; he batches changes to avoid repeated builds.

## 3. What `.11` contains

`build/2026-08-13.11` / `ca80dd17a` is the Agiterra assembled fork, not vanilla
`block/buzz`. It includes:

- `feature/coding-sessions`, including provider-neutral umbrella sessions;
- `feature/builtin-shell`, including Andrew Bent's built-in terminal, project
  terminals, session broker, consent controls, shared terminal broadcast, and
  read-only observer UI;
- project containers/access and the integration glue;
- desktop version `0.5.11`;
- relay-side acceptance of the additive `sessionRef` lifecycle field.

The required `buzz-shell-host` source and Tauri external binary were present in
the local `.11` build. A launcher/build-path issue required the binary to be
copied manually during live testing; treat that as a separate packaging defect,
not proof that Andy's terminal work is absent.

## 4. Current production/deployment mismatch

The client error is:

```text
invalid: coding-session lifecycle command action has missing or unsupported fields
```

Verified facts:

1. The local `.11` client emits exactly the intended 9-key `session.create`
   action, including a canonical lowercase UUID `sessionRef`.
2. `crates/buzz-core/src/coding_session_lifecycle_command.rs` at `ca80dd17a`
   accepts both the historical 8-key action and the new 9-key action.
3. The live Lightyear relay rejected the 9-key action before the provider could
   receive it.
4. `scripts/integrate.sh` rebases/merges, gates, tags, and pushes. It contains no
   image build or deployment operation.
5. `.woodpecker/gate.yml` is a test gate only.
6. `.github/workflows/docker.yml` builds on `main` and `relay-v*`, not
   `integrated` or `build/*`. `main` intentionally mirrors vanilla upstream and
   cannot produce the Agiterra session relay.
7. No desktop or relay release workflow ran on the morning of 2026-08-14. The
   live NIP-11 document exposes only generic version `0.2.1`, not a commit or
   image digest, so an out-of-band deployment must be confirmed by its operator.

Andy owns deployment infrastructure. Brian sent or was given this message:

> Hey Andy — the Git ceremony completed successfully at
> `build/2026-08-13.11` / `ca80dd17a`, but Lightyear still appears to be
> running an older relay binary. The `.11` desktop sends the updated
> `session.create` lifecycle action containing `sessionRef`. Lightyear rejects
> it with `invalid: coding-session lifecycle command action has missing or
> unsupported fields`. The relay source at `ca80dd17a` accepts that field, and
> the same flow works against the updated local relay. Could you build/deploy
> the Lightyear relay from `build/2026-08-13.11` (`ca80dd17a`) and confirm the
> running artifact?

Before doing more client work, ask Brian whether Andy confirmed the deployed
commit/image digest, then verify one fresh create against Lightyear. Do not keep
retrying a stale durable request.

## 5. Subscription-race fix awaiting the next ceremony

Commit `dbc7eb89b` fixes a real provider-side race discovered after `.11`:

- The UI adds the provider to a channel, then publishes the create command.
- The membership notification made the provider queue a channel REQ.
- The relay background task could send that REQ a second later with
  `since=now`, skipping the command forever.
- The UI then spun on “Waiting for the session provider to accept this
  request…”.

The fix passes the membership event timestamp into the first subscription as a
safe replay floor until a consumed-command watermark exists. It changes only:

```text
crates/buzz-session-provider/src/lib.rs
```

Verification already completed:

- `cargo fmt --all`;
- targeted regression test;
- `cargo clippy -p buzz-session-provider --all-targets -- -D warnings`;
- full `cargo test -p buzz-session-provider`: 119 tests passed.

The commit is signed and the feature worktree is clean. It was cherry-picked
temporarily onto local `integrated-build` as `b61566573` for a live build, but
that generated branch was not pushed. The durable source of truth is
`dbc7eb89b` on `feature/coding-sessions`.

## 6. Local environment facts

### Desktop

The local desktop was run from the assembled worktree with:

```bash
cd /Users/briansweet/agiterra/BuzzForkV2-integration-glue
. ./bin/activate-hermit
just nokeyring=1 desktop-standalone
```

Run that command in Brian's own Terminal and leave the Terminal open. Do not
promise persistence when launching the app in a temporary Codex tool PTY. The
last “crash” was not a Buzz crash: there was no macOS crash report or Rust
panic; the temporary command session was reclaimed, all children exited
together, and the provider logged `shutdown requested`.

The local dev instance uses:

```text
~/Library/Application Support/xyz.block.buzz.app.dev.integrated-build
```

Use `nokeyring=1`; otherwise macOS keychain prompts caused repeated password
dialogs. Keychain prompts request the Mac login password, not a Buzz password.

### Local relay

A current integrated relay was cold-built successfully. The old local Buzz
database contained an obsolete SQLx migration `900`, so it was preserved. A
fresh isolated database was created instead:

```text
buzz_sessions_dev_20260813
```

To run the integrated relay against it:

```bash
cd /Users/briansweet/agiterra/BuzzForkV2-integration-glue
. ./bin/activate-hermit
set -a
source /Users/briansweet/agiterra/BuzzForkV2-coding-sessions/.env
set +a
export PGDATABASE=buzz_sessions_dev_20260813
export DATABASE_URL="${DATABASE_URL%/*}/buzz_sessions_dev_20260813"
BUZZ_AUTO_MIGRATE=true BUZZ_GIT_CONFORMANCE_PROBE=false \
  ./target/debug/buzz-relay
```

That relay reached `buzz-relay TCP listening` on `ws://localhost:3000`. Like the
desktop, it stopped when its temporary tool PTY was reclaimed. Start it in a
real Terminal for persistent testing. The fresh database has no Lightyear Hall
or project data; add it as a separate local community rather than expecting
Lightyear state to appear.

### Durable-create cleanup performed

During diagnosis, only the stuck Hall create row and matching pending workdir
hint were cleared. Identity, provider keys, community configuration, and project
state were preserved. A WebKit local-storage database backup was created beside
the original with suffix:

```text
.before-csl-clear-20260813
```

## 7. Other confirmed defects to batch

Do not trigger a new ceremony for every small fix. The following are suitable
to batch with `dbc7eb89b`:

1. **Dev launcher completeness:** `desktop-standalone` did not reliably produce
   or copy `buzz-shell-host`; live testing required a manual copy into Tauri's
   external-binary location.
2. **Duplicate React keys:** Claude and Codex catalog entries can share one
   provider signer pubkey. The create UI keys rows only by signer pubkey and
   repeatedly logs `Encountered two children with the same key`. Key catalog
   rows by the provider/runtime identity, not signer alone.
3. **Deployment observability:** Lightyear NIP-11 exposes no commit or image
   digest. An operator-visible revision would make client/relay skew obvious.
4. **Schema rollout contract:** the docs claim the ceremony updates Lightyear,
   but the implementation stops at Git push. Either automate the assembly
   image and rollout or document the mandatory operator handoff explicitly.

Do not conflate these with the already-fixed subscription race.

## 8. Immediate next actions, in order

1. Confirm with Brian whether Andy deployed Lightyear from `ca80dd17a` and get
   the running artifact identifier.
2. Start the `.11` desktop from Brian's Terminal, not a temporary tool session.
3. Clear/start fresh only if the create screen still holds an old durable
   transaction; then publish exactly one new create against Lightyear.
4. Monitor the provider log under the integrated-build app-data directory.
   Success is membership subscription, command consumption, receipt, session
   creation, and UI navigation.
5. If the relay still rejects `sessionRef`, stop client retries and return the
   exact running artifact mismatch to Andy.
6. After Lightyear is proven, implement/test the two local defects above
   (shell-host launcher and duplicate keys) on their correct feature/glue
   branches.
7. Run proportionate gates, signed commits, then ask Brian before the next full
   ceremony/push.

## 9. Product and working constraints to preserve

- A session is the work surface; providers are participants, not the session.
- One provider must remain effortless; multiple providers share one session.
- Preserve signed, attributable, provider-neutral transcript truth.
- Never copy T3 Code's single-user local architecture into Buzz's signed,
  multi-client relay model.
- Do not expose controls that the selected provider cannot honestly support.
- Brian sets direction and expects the agent to make reversible implementation
  decisions. Report outcomes tersely.
- Draft external messages for Brian; do not contact Andy or deploy production
  without explicit authority.
- Do not push merely to checkpoint work.

## 10. Kickoff behavior for the next Sol

Start with a short paragraph proving you understand this state. Then verify Git
ground truth and Andy's deployment status before writing code. Do not recreate
the subscription-race fix, do not re-diagnose the keychain prompts, and do not
call the temporary-launcher shutdown a product crash.
