# Security debt: NIP-98 method binding on the git transport

**Status:** recorded debt, not a defect to fix in the sessions authority phase.
Ruling **R10** (`SESSION_EXECUTION_PLAN.md` §B.7a): accept as existing
constrained behaviour, document, do not redesign this phase.

## The finding

`buzz-auth`'s NIP-98 verifier is strict. `crates/buzz-auth/src/nip98.rs`
verifies, in order: event kind and signature, `created_at` within ±60 s, the
`u` tag against the expected URL, the `method` tag against the expected method,
and — when a `payload` tag is present and a body is supplied — the SHA-256 of
the body.

The git transport deliberately defeats two of those. At
`crates/buzz-relay/src/api/git/transport.rs:167` the expected method is taken
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

## If it is ever repaid

Options, roughly in increasing order of disruption: scope the signed URL per
service endpoint so a read token cannot address `git-receive-pack`; shorten the
validity window below 60 s for write endpoints; or require a second,
write-specific token for `git-receive-pack` that the credential helper mints on
demand. Any of these changes the credential-helper contract and needs
`git-credential-nostr` changed in lockstep, so it is a coordinated change across
`crates/git-credential-nostr` and `crates/buzz-relay/src/api/git/`.

Owner: whoever owns the git transport (Andy), not the sessions phase.
