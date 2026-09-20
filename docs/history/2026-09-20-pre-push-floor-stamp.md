# The pre-push floor now runs ahead of the push, so its credential is fresh

2026-09-20, lane 191, build only (`work/lane-191-prepush-token`), not landed.
Fixes ledger 178(n).

## The defect

`git push` against the relay mints its NIP-98 credential (via
`git-credential-nostr`) at ref discovery — before any pre-push hook runs —
and that credential is only good for ±900s. `lefthook.yml`'s `pre-push` runs
`scripts/pre-push-floor.sh` (fmt, clippy, and the changed crates' tests)
*after* the credential is already minted. For a docs-only push the floor
takes ~2s and nobody notices. For a crate-touching push landing 184 measured
the floor at ~20 minutes; by the time it finished and git uploaded the pack,
the credential was long expired and the push failed `HTTP 401` with every
check green. The batch 180–187 was gated bare on its tip and pushed
`--no-verify` as a workaround.

## What was measured, and how

Before designing a fix, the actual git behaviour was established empirically
in a throwaway clone under `/tmp` — no real relay involved:

1. A bare repo (`git init --bare`) served over plain HTTP by a small Python
   CGI wrapper around `git http-backend` (`git --exec-path` on this machine),
   requiring every request to carry an `Authorization` header or answering
   `401` with a `WWW-Authenticate` challenge. Two challenge shapes were
   tried: plain `Basic realm="test"`, and `Nostr realm="buzz", method="GET"`
   with a stub credential helper shaped exactly like
   `git-credential-nostr`'s reply (`capability[]=authtype`,
   `authtype=Nostr`, `ephemeral=true`, `quit=true`) — matching
   `crates/git-credential-nostr/src/lib.rs` lines 513–517.
2. The stub helper minted a **fresh, timestamped credential value on every
   `get` invocation** and logged every call it received; the server logged
   every request's path and the `Authorization` value it was sent.
3. `git -c credential.helper=<stub> -c credential.useHttpPath=true push
   origin HEAD:refs/heads/main` was run against this server.

Result, both challenge shapes, identically:

```
server log:
  GET  /repo.git/info/refs?service=git-receive-pack   auth=<none>
  GET  /repo.git/info/refs?service=git-receive-pack   auth=Nostr fresh-nostr-cred-1789924190.099223000
  POST /repo.git/git-receive-pack                     auth=Nostr fresh-nostr-cred-1789924190.099223000

helper log:
  op=get   (once, in response to the 401)
  op=store (once, afterward — not a re-mint; `ephemeral=true` just tells git
            not to persist this into a durable credential store)
```

The credential helper's `get` fires **exactly once** per `git push`. The
value it returns is replayed verbatim on the retried `info/refs` GET and on
the `git-receive-pack` POST, ~46ms later in this test. `ephemeral=true` does
not cause git to re-ask per request — it only suppresses the later `store`
call from persisting into a cache for a *different* future git invocation.
No combination of `credential.useHttpPath`, `http.<url>.extraHeader`, or the
`authtype` capability changes this: it is how git's http transport reuses one
resolved auth context for the whole transport object, GET and POST alike.

**Conclusion:** there is no git-side knob that makes git re-ask the helper
mid-push. The only honest fix is to make sure the floor has already finished
*before* git opens the connection that mints the credential.

## The fix

`scripts/push-with-floor.sh` (`just push`) runs
`scripts/pre-push-floor.sh` directly — the exact same floor, same scope
derivation — with `BUZZ_PRE_PUSH_FLOOR_STAMP_WRITE=1`. On success it writes a
short-lived pass stamp via `scripts/pre-push-floor-stamp.mjs`
(`.git/buzz-pre-push-floor-stamp.json`) naming the current HEAD sha and a
hash of the exact changed-file set the floor ran against, then runs
`git push` for real. The credential git mints at that point is therefore
seconds old.

`lefthook.yml`'s `floor` step (still `scripts/pre-push-floor.sh`, run by
git's own pre-push hook) checks for a stamp before doing anything else: a
fresh stamp whose sha and scope hash match the current push returns
immediately; the stamp is consumed (deleted) whether it matched or not, so it
can only ever answer for the one push it was made for. A push without the
wrapper — plain `git push` — never writes a stamp, so its own hook invocation
finds none and runs the full floor exactly as before (and may still 401 on a
long floor; the floor's own over-budget message now names the wrapper).

Verified end to end in a throwaway clone of this repo (not this checkout):
wrong sha, wrong scope, and an expired (1s TTL) stamp all correctly fall
through to running the floor for real; a fresh matching stamp short-circuits
in under a second and is gone on the next check.

One bug caught during that verification, fixed before landing: the CLI's
"am I the entry point" check (`import.meta.url === file://${process.argv[1]}`)
silently failed for any invocation under `/tmp` because macOS resolves `/tmp`
to `/private/tmp` in `import.meta.url` but not in `process.argv[1]` — the
check now compares `realpathSync()` on both sides.

## Files

- `scripts/pre-push-floor-stamp.mjs` — stamp read/write/consume, pure
  functions plus a small CLI (`write` / `check` / `consume`).
- `scripts/pre-push-floor-stamp.test.mjs` — fresh / expired / wrong-sha /
  wrong-scope / malformed / missing coverage.
- `scripts/pre-push-floor.sh` — checks and consumes a stamp before deriving
  scope; writes one on success only when
  `BUZZ_PRE_PUSH_FLOOR_STAMP_WRITE=1` (only the wrapper sets it).
- `scripts/push-with-floor.sh` — the wrapper; `just push` runs it.
- `lefthook.yml` — `pre-push` § comment documents the stamp and the wrapper.
- `docs/INTEGRATION.md` § Pushing to the relay — "Landing a batch" now says
  to use `just push` instead of `--no-verify`.
