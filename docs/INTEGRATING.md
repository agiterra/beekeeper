# Integrating from outside

This page tells the nine acceptance stories of the headless extensibility
plan (agents repository, `plans/BEEKEEPER_HEADLESS_EXTENSIBILITY_PLAN.md` § 6)
**from the outside**: as a client that holds a Nostr key and
[`nostr-tools`](https://github.com/nbd-wtf/nostr-tools), speaks NIP-98 to the
relay's HTTP bridge (`POST /events`, `POST /query`), and has no Beekeeper SDK,
host socket, service file or desktop. Each story points at the script in
[`conformance/foreign-client/`](../conformance/foreign-client/README.md) that
performs its outside half, and says plainly which part of the story a foreign
client **cannot** see — those parts are proved by the host-side compositions
(`just test-handover`, `just test-ci-continuation`, `just test-host`) and the
2026-10-11 acceptance run, not here.

`just foreign-client-check` (`scripts/foreign-client-check.sh`) runs the
scripts against an isolated relay and asserts every outcome this page claims
for a relay with no provider attached. Read that script's header for the
exact list of what it does not prove.

## Before any story: what the relay says it speaks

```sh
node conformance/foreign-client/foreign-info.mjs 44221 44224 46013 46022 46023
```

NIP-11 is public — no key — and its `supported_kinds` array
(`[{kind, name, schemas}]`, generated from the relay's kind registry; see
[`INTEGRATION.md` § NIP-11](INTEGRATION.md#nip-11)) tells a client which kinds
and which `buzz-…/v1` schemas this build understands before it signs anything.
`self` is the relay key every relay-signed record (46013, 46014, 46015, NIP-29
metadata) must verify against; witness it through a trusted origin once and
pin it. The same script shows that an unsigned `/query` is refused: every
bridge read and write is NIP-98 authenticated (kind 27235 over the exact URL,
method and payload hash).

## The nine stories

### 1. Fresh install → commission → agent and project → readiness

**Inside:** `bee host commission` ([NIP-HC](nips/NIP-HC.md)) mints a
provider key under an owner's [NIP-OA](nips/NIP-OA.md) attestation, writes
`host.json`, and proves relay admission with one authenticated read. No
desktop, no GUI copy, no handwritten config.

**Outside, what you can see:** the commissioned provider's events carry an
`auth` tag naming the owner; its `kind:44222` catalog names the runtime
instances it serves; once Lane B's presence lands, `kind:24224` heartbeats
say it is running. Read them:

```sh
FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-session.mjs read   # 44222–44225
```

**What you cannot see:** the home directory, the adapter path, whether the
model account can answer a turn. Commissioning reports
`runtime_account: "unconfirmed"` for exactly that reason; readiness is a real
turn (story 2), never a file write.

### 2. Channel request → acknowledgement → one execution → result in thread

**Outside:** post a kind 9 message in the channel the agent serves, then
collect the thread:

```sh
node conformance/foreign-client/collect-conversation.mjs after-request
```

`collect-conversation.mjs` reads kinds 9, 44220/44221 (commands), 44223
(metadata), 44224 (receipts), 44225 (transcript) and 44234, verifies every
signature, and writes them. The story holds when one `turn_queued` →
`turn_started` → terminal receipt chain names the request's command id, the
transcript items of that generation cover it, and the actor's reply is in
the original thread (`e` tags) rather than a new one. Signing requires
channel membership — the relay admits the message on the same rule as story 4.

**What you cannot see:** that the desktop, opened afterwards, shows the same
identity. That is the desktop's own test suite and the native screenshots in
the acceptance archive.

### 3. Close the app during execution and while idle; work continues

**Outside:** the wire does not know whether a desktop is open. What it shows
is continuity: the same provider key, the same `sessionId`, and — after the
app is closed and a new request is posted — a new `turn_queued`/`turn_started`
pair under the same generation with no duplicate provider instance in 44222
and no second 44223 claiming the same target with a different provider.

```sh
node conformance/foreign-client/foreign-native-read.mjs \
  '{"kinds":[44223],"#h":["<channel>"]}' metadata.json
```

Count distinct `(session.instanceId, session.sessionId, session.generation)`
triples; a duplicate process would show as a second provider claiming the
target. Presence (`kind:24224`, Lane B) is what distinguishes "idle" from
"gone" without a desktop.

**What you cannot see:** process tables. `just test-host` is the supervision
proof.

### 4. A second client with no host socket: inspect, update, steer, stop; ungranted refused

This is the story the foreign scripts were written for.

```sh
# a granted (channel member) key
FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-session.mjs read
node conformance/foreign-client/foreign-native-control.mjs steer \
  '{"operation":"<uuid>","channel":"<channel>","target":{...},"provider":"<authority hex>","text":"…","output":"steer.json"}'
node conformance/foreign-client/foreign-session.mjs stop <operation> '<target json>' <authority>

# an ungranted key (not a channel member)
FOREIGN_CLIENT_KEY=<other> node conformance/foreign-client/foreign-session.mjs stop <operation> '<target>' <authority>
```

Documented outcomes, asserted by `foreign-client-check` steps 2–4:

- a member's read is answered 200 with verified signatures;
- a member's signed `session.stop` is **admitted** (`accepted: true`) and
  stored — the relay validates shape and membership; whether the target
  exists is the provider's answer, and it says so in a 44224
  (`UNKNOWN_TARGET`, [NIP-CSE](nips/NIP-CSE.md)) rather than the relay
  guessing;
- an ungranted key's `session.stop` is refused with HTTP 400 and a
  `restricted:` message naming membership (`restricted: not a channel member`
  on a private channel, `restricted: coding-session events require channel
  membership` on an open one) — refused **by name**, not silently dropped;
- an ungranted key's read of a **private** channel shows nothing. On an
  **open** channel the same key can read — open means readable to any
  authenticated key — while its writes are refused exactly as above; the
  check script creates a private channel so that it asserts the stronger
  claim, and says so.

On a relay with a live provider the acceptance run additionally observed a
foreign steer answered `turn_degraded` / `STEER_UNSUPPORTED` (the runtime had
no native steering — a disclosed fallback, not a fake) and a foreign
exact-target stop answered `stopped` with new metadata. Safe public records
carried no host path, environment or secret; that boundary is enforced by
`coding_session_payload.rs` (nothing host-local is serialisable) and its
tests, not by a live leak audit.

### 5. Lost reply / replay / worker restart; operation identity preserved

**Outside:** replay the same signed command event twice and read the
receipts:

```sh
node conformance/foreign-client/foreign-native-read.mjs \
  '{"kinds":[44224],"#h":["<channel>"]}' receipts.json
```

Receipts are keyed by `(commandId, status)`. One command id yields one
`turn_queued`, one `turn_started`, one terminal — never two `turn_started` —
and a replay of a dropped turn is answered `turn_refused` naming the original
(`DUPLICATE_OPERATION`, or the "already refused" path of
`NO_LIVE_EXECUTION`). The relay stores one copy per event id and answers a
byte-identical replay as a duplicate rather than storing it twice.

**What you cannot see:** the worker's durable refusal record and the restart.
`just test-ci-continuation` and the provider's `*_replay` tests are that
proof.

### 6. Clean setup: legacy data disclosed as historical, never auto-imported

**Outside:** the only wire-visible part is honesty of status. A 44223 whose
`status` is `stopped` with a provider that is absent stays `stopped`; nothing
on the relay republishes an old session as live, and a retired umbrella
answers every command `SESSION_RETIRED` ([NIP-CSE](nips/NIP-CSE.md)). Read
44223 as in story 3 and check no execution reappears under a new provider
without a fresh `session.create` (44221) and its `created` receipt.

**What you cannot see:** the retained journals and keys on disk. See
[`agent-recovery.md`](agent-recovery.md).

### 7. Two responsibilities; thread-bound steer routes correctly; unavailable is never Idle

**Outside:** two 44223 records for one agent key with different `sessionId`s;
a steer posted with the thread's `e` tag produces receipts naming the command
under exactly one of the two targets (`foreign-native-read.mjs` over 44224
filtered by `commandId`); an ambiguous request produces a visible resolution
request in the thread rather than a guess. A missing credential source
produces a signed `MODEL_AUTH_UNAVAILABLE` receipt and a 44223 whose status
is not `idle`; with Lane B's presence, `providerPresence` distinguishes a
live-but-unauthenticated provider from a gone one.

### 8. Event hook: recorded revision, grant, result; bad dependency stays unready; update/rollback without rebuild

**Outside, with a workflow published:**

```sh
FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-hook.mjs correlate <runId>
```

This reads 46013/46014/46022/46023 ([NIP-HS](nips/NIP-HS.md)), verifies every
signature, and proves the chain: the relay-signed request (no command text,
a `definitionHash`), one accepted claim naming it, the result naming the
claim, and the relay-signed echo naming the result and the claiming host.
`hook-live-exchange.json` is the transcript; the standard-library Python
verifier in `conformance/headless-host-step/` checks the same correlation
offline. Update and rollback are two more runs with a different
`definitionHash` and the relay-signed `kind:46015` autorun changes between
them; a bad dependency is a `refused` result with
`ACTION_HOST_EXECUTABLE_MISSING` before any claim.

**On a relay with no workflow** (what `foreign-client-check` can run), the
documented outcomes are the honest ones: a claim of a nonexistent request is
refused `invalid: unknown host step`, and `correlate` reports the exchange
incomplete (exit 3) rather than inventing one.

### 9. A minimal foreign client exchanges the selected contract honestly

```sh
FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-hook.mjs claim <runId> <stepId> <requestedEventId>
```

The client claims a real request and, when the relay admits the claim, answers
it with `disposition: "refused"` and `FOREIGN_EXECUTION_UNSUPPORTED` — it does
not execute commands and does not pretend to. Both events are relay-admitted
and the echo correlates (the 2026-10-11 run recorded this with claim
`1efc24f7…` and result `4d1bdbb9…`). This is contract conformance, not a
foreign runtime: nothing here recovers a session or runs a model.

## The refusal vocabulary a foreign client will meet

| where | string | meaning |
| --- | --- | --- |
| HTTP bridge | `401`/`403` without a valid NIP-98 header | sign the request |
| `/events` | `restricted: …membership…` (e.g. `restricted: coding-session events require channel membership`) | the signer is not in the channel the `h` tag names |
| `/events` | `invalid: …` | the envelope failed the kind's structural validator (tags, schema, bounds) |
| `/events` | `duplicate: already processed` | the same event id was already stored |
| `/events` (46022) | `invalid: unknown host step`, `claimed by <hex>`, `expired` | [NIP-HS](nips/NIP-HS.md) claim rule |
| `/query` | a filter without `kinds` is refused | always name kinds |
| 44224 `error.code` | [NIP-CSE](nips/NIP-CSE.md) | the provider's answer, never the relay's |

## Not a replacement for the inside view

A foreign client sees signed records and admissions. It does not see
processes, files, keys, or the model. Where this page says "what you cannot
see", the inside proof is named; a claim that the outside view alone proves a
story is wrong, and `foreign-client-check`'s header lists what it leaves
unproved so nobody reads its green as more than it is.
