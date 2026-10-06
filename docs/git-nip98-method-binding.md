# Security debt: NIP-98 method binding on the git transport

**Status:** recorded debt, not a defect to fix in the sessions authority phase.
Ruling **R10** (`SESSION_EXECUTION_PLAN.md` §B.7a): accept as existing
constrained behaviour, document, do not redesign this phase.

## The finding

`beekeeper-auth`'s NIP-98 verifier is strict. `crates/beekeeper-auth/src/nip98.rs`
verifies, in order: event kind and signature, `created_at` within ±60 s, the
`u` tag against the expected URL, the `method` tag against the expected method,
and — when a `payload` tag is present and a body is supplied — the SHA-256 of
the body.

The git transport deliberately defeats two of those. At
`crates/beekeeper-relay/src/api/git/transport.rs:167` the expected method is taken
**from the event itself**, with the comment:

> We pass the method from the event itself so `verify_nip98_event` always accepts.

and the body is passed as `None`, so the payload check never runs.

The URL check does still hold, and is load-bearing: `git_expected_url` derives
the expected URL from the tenant host, with a test asserting a token signed for
community A is rejected when the request resolves to community B.

## The consequence

A token minted for `GET <repo>/info/refs` is accepted for
`POST <repo>/git-receive-pack` — read-to-write escalation — for the 60-second
validity window, because the two requests share the repo-root URL the helper
signs and the method is no longer bound.

Exploiting it requires already holding a valid token for that repo, so this is
defence-in-depth rather than an open door. It matters most wherever a token
could be observed: proxy logs, a compromised intermediary, or any future path
that forwards an `Authorization` header.

## Why it is probably not carelessness

Git's credential-helper protocol invokes the helper **once per authentication
challenge** and reuses the returned `Authorization` header across the requests
of a single operation — the `GET info/refs` advertisement and the `POST` that
follows it. The helper cannot know which method will be used, and signs the
repo-root URL. Binding the method strictly would break `git clone` outright.

So the relaxation is likely forced by the protocol, not chosen for convenience.
What is missing is the record of that reasoning: the code says only "so
`verify_nip98_event` always accepts," which reads as a shortcut rather than a
constraint.

## What depends on this

Bite **B1**'s relay-reachability probe must reproduce the *credential helper's*
signing contract — repo-root URL, method unbound — and not the REST bridge's
exact-path, method-bound contract. A probe signed the strict way will be
rejected by the git endpoints. This is the practical reason the debt is worth
documenting now even though it is not being repaid.

## The sibling: the freshness window (2026-08-24)

The same credential-helper contract breaks the *timestamp* check too, and this
one is not defence-in-depth — it stops pushes outright.

Git invokes the helper once per authentication challenge, at the ref
advertisement (`GET info/refs`). That is when the token is minted. Git then does
everything else — runs `pre-push`, enumerates and compresses the pack — and only
then sends `POST git-receive-pack` carrying the same header. Under `beekeeper-auth`'s
±60 s window, anything slow in that gap makes the push fail **deterministically**:

- Observed here: a `pre-push` hook running the desktop test suite took **99.9 s**,
  putting the token 40 s past expiry. Every push to hive died with
  `error: RPC failed; HTTP 401` / `send-pack: unexpected disconnect`. The same
  push with `--no-verify` succeeded instantly — the only variable was elapsed time.
- Not hook-specific: pack building for a large repository does the same thing.
  With `BUZZ_GIT_MAX_PACK_BYTES` at 500 MB, a pack that takes over a minute to
  produce is unpushable regardless of hooks.

The fix is `verify_nip98_event_within`, called from the git transport with
`git_nip98_tolerance_secs` (`BUZZ_GIT_NIP98_TOLERANCE_SECS`, default **600 s**).
The rest of the HTTP surface keeps ±60 s; values below 60 are clamped up.

**This widens the escalation window described above.** The token's lifetime is
exactly how long an observed token stays replayable, and because the method is
not bound, a captured read token is a write token for that span — now ten
minutes rather than one. The `u` tag still binds it to one repository on one
community host, and it still only crosses TLS. Anyone repaying the method
binding should treat the window as part of the same repair, not a separate one:
per-service tokens would let the write window shrink back below the read one.

## If it is ever repaid

Options, roughly in increasing order of disruption: scope the signed URL per
service endpoint so a read token cannot address `git-receive-pack`; shorten the
validity window below 60 s for write endpoints; or require a second,
write-specific token for `git-receive-pack` that the credential helper mints on
demand. Any of these changes the credential-helper contract and needs
`git-credential-nostr` changed in lockstep, so it is a coordinated change across
`crates/git-credential-nostr` and `crates/beekeeper-relay/src/api/git/`.

Owner: whoever owns the git transport (Andy), not the sessions phase.
