# Headless host-step exchange fixture

Run from the repository root, with Python 3.8 or later:

```sh
python3 conformance/headless-host-step/test_client.py
python3 conformance/headless-host-step/client.py conformance/headless-host-step/exchange.json
```

The checker depends only on Python's standard library. It reads an exported
Nostr exchange, verifies canonical event IDs and BIP-340 Schnorr signatures,
checks the witnessed relay signer, exact content/tag correlation, and verifies
that the result belongs to the admitted claim's signer and event ID. It imports
no Beekeeper, desktop, host, or runtime code. `exchange.json` is a reproducible
signed fixture using public disposable test keys (integers 3 and 4); never use
those identities on a community. `test_client.py --generate` regenerates it.

This is a **selected-contract fixture**, not a complete independent decoder for
all host-step variants. It supports claimed `run_on_host` results only. It does
not execute commands, submit events, verify relay membership, prove claim
admission, parse every optional nested artifact/checkout structure, enforce
relay timestamp windows, or demonstrate real recovery. The accepted claim ID
is an explicit input from the relay's response/state; a signed claim alone is
not evidence of winning it. Request expiry uses the fixture claim timestamp;
the relay itself checks its current clock. The Schnorr implementation is a
read-only reference verifier, not a production custody/signing library.

The tests reject tampered signatures/content, an unwitnessed relay, unknown
payload fields, mismatched request/channel/claim, a result from another host,
an unadmitted/expired claim, duplicate tags, and a lost result claiming a known
exit. A valid `lost_on_restart` reports uncertainty without synthesizing
success. Each fixture's signatures were also checked with the repository's
installed `nostr-tools`; offline fixture success is not live relay acceptance.

## Foreign participant boundary

Use your own Nostr client and key custody. Fetch NIP-11 `self` through a trusted
community origin and witness it before accepting relay-signed requests. Open a
NIP-42 authenticated WebSocket. Community deployments may additionally require
an existing NIP-OA authorization tag; use granted credentials, never a provider
key borrowed from its local files. NIP-98 authenticates the generic HTTP bridge.

Subscribe with explicit kinds and allowed scope, for example:

```json
["REQ","hooks",{"kinds":[46013],"#h":["<channel UUID>"],"#a":["<project coordinate>"]},{"kinds":[46014],"#h":["<channel UUID>"]}]
```

Kind 46014 has no `a` tag, hence its separate `#h`-scoped filter. Requests can use `#a` project scope; channel
membership still applies. Persist consumed requested event IDs before effects,
then reconnect with explicit filters and backfill. The current provider also
probes its request subscription because live `#a` delivery has a known gap.

The contract is `buzz-host-step/v1`, defined by
[`host_step.rs`](../../crates/beekeeper-core/src/host_step.rs). Kind 46013 carries
the run, step, project, definition hash, trigger context and expiry, **no command**.
The authorized executor resolves committed code locally. Kind 46022 names that
request in `e` and `requestedEventId`. Wait for relay claim acceptance before an
effect. Kind 46023 names both request and winning claim. The relay's 46014 echo
names the accepted result event and executor. Correlation is per community and
`(runId, stepId, requestedEventId)`, not an OS pid or display name.

No new wire fields or kinds are introduced here. Host-side prerequisites,
artifact installation, credentials and execution boundaries remain local;
shared records carry definitions, grants, invocations and results. Existing
workflow approval grants authorize exact definition hashes. Formal session
acceptance's software-specific review/action/git-ref proof types are a separate
contract, not universal requirements for every hook.
