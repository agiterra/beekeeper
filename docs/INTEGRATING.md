# Integrating from outside

This page is for a team that has never run Beekeeper: it takes you from a
relay invite to an agent that answers in a channel, and from there to a
client of your own that holds a Nostr key and
[`nostr-tools`](https://github.com/nbd-wtf/nostr-tools) and nothing of
Beekeeper's. Every command below is the real CLI at this commit; every kind
number is `crates/beekeeper-core/src/kind.rs`. Where a claim is proved by a
script rather than by this page, the script is named. Where something is not
promised, it is listed at the end rather than left for you to discover.

Pages this one leans on:

- [`agent-host.md`](agent-host.md) — the host process: install, supervise,
  status, logs, where the key lives.
- [`headless-extensions.md`](headless-extensions.md) — the operator runbook,
  event hooks, capability tags.
- [`runtime-descriptor.md`](runtime-descriptor.md) — plugging a runtime that
  is not Claude or Codex.
- [NIP-HC](nips/NIP-HC.md) commissioning · [NIP-CSL](nips/NIP-CSL.md)
  session wire · [NIP-CSE](nips/NIP-CSE.md) error codes ·
  [NIP-HO](nips/NIP-HO.md) handover · [NIP-HS](nips/NIP-HS.md) host steps ·
  [NIP-OA](nips/NIP-OA.md) owner attestation.

## Ten minutes to a replying agent

You need: a Linux or macOS machine (Windows is not supported, see the end),
an invite to a relay, an Anthropic credential, and a checkout for the agent
to work in. The executable form of this path is
`scripts/headless-acme-check.sh` (`just headless-acme-check`), which runs the
same commands against an isolated relay with a stub runtime.

**1. Install the binaries.** From a checkout of this repository:

```sh
. ./bin/activate-hermit
just install-headless          # bee, beekeeper-host, beekeeper-session-provider, … into ${CARGO_INSTALL_ROOT:-$HOME/.local}/bin
```

Install the ACP adapter for your runtime separately. For Claude Code the
adapter is the npm package `@agentclientprotocol/claude-agent-acp`
(`crates/beekeeper-session-provider/src/agent_fence.rs` names the version it
was measured against); it needs Node and the `claude` CLI. Nothing in
`install-headless` installs or authenticates a model.

**2. Claim your invite as the owner key.** The owner key is yours; it is
never written to the host.

```sh
export BEEKEEPER_RELAY_URL=https://hive.example.org
export BEEKEEPER_PRIVATE_KEY=<hex or nsec>        # from your secret custodian, never argv
bee community join-policy                         # read the relay's joining policy
bee community claim-invite --code '<invite>'      # add --policy-receipt '<receipt>' if the policy requires acceptance
```

A member mints invites with `bee community mint-invite --ttl-secs 3600
--max-uses 1`. If the policy requires acceptance, `bee community
accept-policy --code '<invite>' --policy-version <n>` returns the receipt.

**3. Commission the host.** This mints a provider key under your attestation
([NIP-HC](nips/NIP-HC.md)), writes `host.json`, and proves relay admission
with one read. The service home is an absolute, durable directory; export it
once and every later `bee host …` and `bee agents …` on this machine finds
the host's socket under it.

```sh
export BEEKEEPER_HOST_HOME=/srv/beekeeper-host
umask 077
cat > /etc/beekeeper/runtime-credentials.json <<'EOF'
{"runtimes":{"claude-primary":{"CLAUDE_CODE_OAUTH_TOKEN":"<token>"}}}
EOF
bee host commission --home "$BEEKEEPER_HOST_HOME" --runtime claude \
  --adapter /usr/local/bin/claude-agent-acp \
  --provider-command "$HOME/.local/bin/beekeeper-session-provider" \
  --credential-file /etc/beekeeper/runtime-credentials.json \
  --credential-source vault-claude
beekeeper-host check
```

The one JSON line it prints says `commissioned: true`, `relay_admitted:
true`, and `runtime_account: "unconfirmed"` — commissioning cannot tell
whether the token works; a real turn (step 6) is that proof. The credential
file must be mode `0600`; the host reads it afresh at every adapter start
and injects only `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or
`CLAUDE_CODE_OAUTH_TOKEN` for `claude` (`OPENAI_API_KEY`, `CODEX_API_KEY`
for `codex`). In Kubernetes, mount the Secret at that path with
`defaultMode: 0600` and the label you pass to `--credential-source` is what
shows up in metadata and auth-failure items when the token expires.

**4. Start the host under a supervisor.** The host is one foreground process
that spawns and restarts the provider. Run it as your container's main
process, or register it:

```sh
beekeeper-host run                        # foreground: PID 1 of a container, or ExecStart of a unit
# or, on a Linux box you log in to:
bee host install && loginctl enable-linger "$USER"    # systemd user unit
bee host start                            # starts the provider if the host is up and it is not
bee host status                           # provider.state must read "live"
```

`bee host start`, `stop`, `status` and `logs` talk to the running host's
socket; they do not start the host itself. On a Mac nobody logs in to, see
[`agent-host.md` § On a headless Mac](agent-host.md#on-a-headless-mac).

**5. Create a channel and an agent.**

```sh
CHANNEL=$(bee channels create --name agents --type stream --visibility private | jq -r .channel_id)
bee agents create --operation-id "$(uuidgen)" --channel "$CHANNEL" \
  --display-name helper --system-prompt 'You answer questions about this checkout.' \
  --home "$BEEKEEPER_HOST_HOME" --cwd /srv/checkout --wait-secs 120
```

The output carries `agent_pubkey`, `host_admitted: true` and
`receipt_event_id` — the provider's signed `created` receipt (kind 44224).
`--cwd` must exist and is never published. Add `--provider-instance
claude-primary` when the host serves more than one runtime.

**6. Message it, read the thread.**

```sh
MSG=$(bee messages send --channel "$CHANNEL" --mention "$AGENT" --content '@helper what is in this checkout?' | jq -r .event_id)
bee messages thread --channel "$CHANNEL" --event "$MSG"
```

The reply is a kind 9 under the agent's key in the same thread. Beside it
the provider narrates the turn under the same key (`Request turn_queued.`,
`Request turn_started.`, `Turn ended (…)`), so expect more than one post.

**7. Watch presence and receipts.** `--kinds` is mandatory on every raw
query; the relay refuses a filter without it.

```sh
bee events query --kinds 24224 --channel "$CHANNEL" --authors "$PROVIDER"   # heartbeat: state idle|busy, ttlSecs 90
bee events query --kinds 44224 --channel "$CHANNEL"                         # receipts keyed by (commandId, status)
bee events query --kinds 44223 --channel "$CHANNEL" --authors "$PROVIDER"   # session metadata incl. credentialSource
bee host logs --bytes 20000
```

`$PROVIDER` is `provider_pubkey` from step 3.

**8. Stop.**

```sh
bee agents stop --operation-id "$(uuidgen)" --agent-pubkey "$AGENT" --channel "$CHANNEL" --home "$BEEKEEPER_HOST_HOME"
bee host stop                              # the provider ends; the host keeps answering (provider.state "notSupervised")
```

`agents stop` waits for the provider's signed `stopped` receipt. Stopping is
not revocation: the identity and its custody stay on the host.

## Then: your own client

Everything a reader needs is signed events over the relay's HTTP bridge
(`POST /events`, `POST /query`) under NIP-98 (kind 27235), or the WebSocket
with NIP-42. Start with the relay's own statement of what it speaks:

```sh
node conformance/foreign-client/foreign-info.mjs 44221 44224 46013 46022 46023
```

NIP-11 is public and its `supported_kinds` array (`[{kind, name, schemas}]`,
generated from the kind registry; [`INTEGRATION.md`
§ NIP-11](INTEGRATION.md#nip-11)) names every kind and every `buzz-…/v1`
schema this build understands. `self` is the relay key that signs 46013,
46014, 46015 and NIP-29 metadata; witness it once through a trusted origin
and pin it. An unsigned `/query` is refused.

The scripts in [`conformance/foreign-client/`](../conformance/foreign-client/README.md)
are a complete client written only against Node and `nostr-tools`.
`just foreign-client-check` runs them against an isolated relay and asserts
every outcome the stories below claim for a relay with no provider attached.

## The nine stories, from outside

These are the acceptance stories of the headless plan (agents repository,
`plans/BEEKEEPER_HEADLESS_EXTENSIBILITY_PLAN.md` § 6) told as a foreign
client sees them. Each says which part the wire cannot show; those parts are
proved by the host-side compositions (`just test-handover`,
`just test-ci-continuation`, `just test-host`, `just headless-acme-check`).

### 1. Fresh install → commission → agent → readiness

**Visible:** the provider's events carry an `auth` tag naming the owner
([NIP-OA](nips/NIP-OA.md)); its kind 44222 catalog names the runtime
instances it serves; kind 24224 heartbeats say it is running.
`FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-session.mjs read`
reads 44222–44225.

**Not visible:** the home directory, the adapter path, whether the model
account can answer. Commissioning reports `runtime_account: "unconfirmed"`
for exactly that reason; readiness is a real turn.

### 2. Channel request → acknowledgement → one execution → result in thread

Post a kind 9, then `node conformance/foreign-client/collect-conversation.mjs
after-request`: it reads kinds 9, 44220/44221, 44223, 44224, 44225, verifies
every signature, and writes them. The story holds when one `turn_queued` →
`turn_started` → terminal receipt chain names the request's command id, the
transcript covers it, and the reply sits in the original thread (`e` tags).

### 3. Close the app during execution and while idle; work continues

The wire does not know whether a desktop is open. What it shows is
continuity: the same provider key and `sessionId`; after a new request, a
new `turn_queued`/`turn_started` pair under the same generation; no second
44223 claiming the same target from a different provider. Read 44223 with
`foreign-native-read.mjs '{"kinds":[44223],"#h":["<channel>"]}' metadata.json`
and count distinct `(session.instanceId, session.sessionId,
session.generation)` triples. Presence (24224) is what tells idle from
gone. Process tables are `just test-host`'s proof.

### 4. A second client with no host socket: inspect, update, steer, stop; ungranted refused

```sh
FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-session.mjs read
node conformance/foreign-client/foreign-native-control.mjs steer '{"operation":"<uuid>","channel":"<channel>","target":{...},"provider":"<authority hex>","text":"…","output":"steer.json"}'
node conformance/foreign-client/foreign-session.mjs stop <operation> '<target json>' <authority>
FOREIGN_CLIENT_KEY=<other> node conformance/foreign-client/foreign-session.mjs stop <operation> '<target>' <authority>
```

Asserted by `foreign-client-check` steps 2–4: a member's read is answered
with verified signatures; a member's signed `session.stop` is admitted
(`accepted: true`) — whether the target exists is the provider's answer, as
a 44224 `UNKNOWN_TARGET` ([NIP-CSE](nips/NIP-CSE.md)); a non-member's
`session.stop` is refused HTTP 400 with a `restricted: …` message naming
membership; a non-member's read of a private channel shows nothing (an open
channel is readable by any authenticated key, and its writes are refused the
same way). With a live provider the acceptance run also saw a foreign steer
answered `turn_degraded` / `STEER_UNSUPPORTED` and an exact-target stop
answered `stopped`. Public records carry no host path, environment or
secret; `coding_session_payload.rs` has no field for them.

### 5. Lost reply / replay / worker restart; operation identity preserved

Replay the same signed command twice and read 44224. Receipts are keyed by
`(commandId, status)`: one `turn_queued`, one `turn_started`, one terminal —
never two `turn_started`. A replay of a dropped turn is answered
`turn_refused` naming the original (`DUPLICATE_OPERATION`, or the "already
refused" path of `NO_LIVE_EXECUTION`). The relay stores one copy per event
id. The durable refusal record and the restart are `just
test-ci-continuation`'s proof.

### 6. Clean setup: legacy data disclosed as historical, never auto-imported

A 44223 whose `status` is `stopped` with an absent provider stays `stopped`;
nothing republishes an old session as live; a retired umbrella answers every
command `SESSION_RETIRED`. No execution reappears under a new provider
without a fresh `session.create` (44221) and its `created` receipt. The
retained journals on disk are [`agent-recovery.md`](agent-recovery.md).

### 7. Two responsibilities; thread-bound steer routes correctly; unavailable is never Idle

Two 44223 for one agent key with different `sessionId`s; a steer carrying
the thread's `e` tag produces receipts under exactly one target; an ambiguous
request produces a visible resolution request, not a guess. A missing
credential source produces a `failed` receipt with `MODEL_AUTH_UNAVAILABLE`
and a 44223 whose status is not `idle`; presence distinguishes a
live-but-unauthenticated provider from a gone one.

### 8. Event hook: recorded revision, grant, result; bad dependency stays unready; update/rollback without rebuild

`FOREIGN_CLIENT_CHANNEL=<channel> node conformance/foreign-client/foreign-hook.mjs correlate <runId>`
reads 46013/46014/46022/46023 ([NIP-HS](nips/NIP-HS.md)), verifies, and
proves request (relay-signed, `definitionHash`, no command text) → one
accepted claim → result → relay-signed echo. Update and rollback are two
more runs with a different `definitionHash` and a relay-signed 46015
between them; a bad dependency is a `refused` result with
`ACTION_HOST_EXECUTABLE_MISSING` before any claim. On a relay with no
workflow, a claim of a nonexistent request is refused `invalid: unknown
host step` and `correlate` exits 3 rather than inventing an exchange.

### 9. A minimal foreign client exchanges the selected contract honestly

`foreign-hook.mjs claim <runId> <stepId> <requestedEventId>` claims a real
request and answers it `disposition: "refused"`,
`FOREIGN_EXECUTION_UNSUPPORTED`. Both events are admitted and the echo
correlates. This is contract conformance, not a runtime.

## What you can rely on

| fact | where it is fixed |
| --- | --- |
| Kinds: 9 message · 44220 turn command · 44221 lifecycle command · 44222 provider catalog · 44223 session metadata · 44224 lifecycle receipt · 44225 transcript · 44247 handover · 24223 lease · 24224 provider presence · 46013/46014 host-step request/exit (relay-signed) · 46022 claim · 46023 result · 46015 autorun change · 27235 NIP-98 | `crates/beekeeper-core/src/kind.rs`; advertised in NIP-11 `supported_kinds` |
| Schemas: `buzz-coding-session-command/v1`, `…-lifecycle-command/v1`, `…-provider-catalog/v1`, `…-metadata/v1`, `…-lifecycle-receipt/v1`, `…-transcript/v1`, `…-provider-presence/v1`, `buzz-coding-session-handover/v1`, `buzz-host-step/v1` | the `schema` field of each content; a different version is a different contract |
| Strict decoders: unknown keys are refused at ingest (`deny_unknown_fields`); a 44224 `error` is `null` or `{code, message}`; a 44247 has exactly five tags | `coding_session_payload.rs`, `coding_session_handover.rs` |
| Presence: one 24224 per channel every 30 s; `ttlSecs` default 90 (`1..=3600`); live while `created_at + ttlSecs > now`; the relay retains it at most 600 s and admits at most 8 live `instanceId`s per (channel, key) | [NIP-CSL § Provider presence](nips/NIP-CSL.md#provider-presence) |
| Receipt statuses: `created`, `created_with_failed_initial_turn`, `failed`, `resumed`, `resumed_without_context`, `stopped`, `continuation_registered`, `model_applied`; turn stages `turn_queued`, `turn_started`, `turn_injected`, `turn_degraded`, `turn_delivery_unknown`, `turn_dropped`, `turn_refused`, `interrupt_delivered`, `turn_interrupted` | `ReceiptStatus` in `coding_session_payload.rs`; [NIP-CSL](nips/NIP-CSL.md) |
| `turn_interrupted` carries `quiescence: "unconfirmed"` and is not terminal; a `failed` with `TURN_INTERRUPTED_BY_RESTART` closes it under the same `commandId` once the process group is proved gone | [NIP-CSL § turn_interrupted](nips/NIP-CSL.md#fork-amendment-turn_interrupted--interrupted-then-terminal) |
| Error codes are a closed vocabulary; a drift test fails CI if the code and the page disagree | [NIP-CSE](nips/NIP-CSE.md) |
| Expired credentials: the turn's `result` item (44225) carries `errorCode: "MODEL_AUTH_FAILED"` and `credentialSource` — your `--credential-source` label, or `ambient-login` — and the 44223 carries the same `credentialSource` | `credential_source.rs`; [NIP-CSL § MODEL_AUTH_FAILED](nips/NIP-CSL.md#fork-amendment-model_auth_failed-and-credentialsource) |
| Unreadable or absent credential file: `failed` receipt, `MODEL_AUTH_UNAVAILABLE`, no adapter started | `runtime_credentials.rs`; [runtime-descriptor.md](runtime-descriptor.md) |
| Operation identity: a lost reply is retried with the same `--operation-id`; the relay stores one event per id and answers a byte-identical replay as a duplicate | `bee agents …`, `bee host commission` |
| Nothing host-local travels: no path, environment or secret has a field in any public payload | `coding_session_payload.rs` |

## What is not promised

- **No execution sandbox on Linux.** The project boundary (private home,
  scoped file grants) is a macOS backend. On any other platform
  `execution_scope::prepare` returns a disclosed `Legacy` plan with reason
  `no-backend-for-platform` (`crates/beekeeper-session-provider/src/execution_scope.rs`)
  and the adapter is spawned the ordinary way. Credential scrubbing still
  applies — inherited account variables are blanked and only the permitted
  names injected — but the adapter can read what the container user can
  read. Confine it with the container, not with Beekeeper.
- **No published image for the host.** `Dockerfile.sprig` builds the ACP
  harness (`beekeeper-acp`) only; the Kubernetes provider in
  [`remote-agents.md`](remote-agents.md) runs that harness as a bare pod.
  A pod that runs `beekeeper-host` plus the provider is yours to build from
  `just install-headless`; the path above is what it must do.
- **No credential refresh.** The credential file is read as-is at each
  adapter start. Nothing rotates or refreshes an OAuth token; when it
  expires you get `MODEL_AUTH_FAILED` naming your label, and you supply a
  fresh file and run `bee host restart`.
- **A descendant that leaves its process group is invisible.** The
  interrupted-turn witness is `killpg(group, 0)`; a child that calls
  `setsid` escapes it, so `turn_interrupted` can close as `failed` while
  such a process still runs.
- **Presence degrades past 300 channels.** The heartbeat period widens to
  stay under the relay's ephemeral admission rate; past 900 channels it
  exceeds the 90 s TTL and presence reads expired between beats. The
  provider logs this rather than exceed the rate.
- **Windows is not a host platform.** No service registration, no boundary,
  no acceptance evidence.
- **`runtime_account` stays `unconfirmed` until a turn runs.** No command
  on this page proves the model account except step 6.

## The refusal vocabulary a foreign client will meet

| where | string | meaning |
| --- | --- | --- |
| HTTP bridge | `401`/`403` without a valid NIP-98 header | sign the request |
| `/events` | `restricted: …membership…` | the signer is not in the channel the `h` tag names |
| `/events` | `invalid: …` | the envelope failed the kind's structural validator |
| `/events` | `duplicate: already processed` | the same event id was already stored |
| `/events` (24224) | `restricted: PROVIDER_PRESENCE_INSTANCE_CAP: …` | a ninth `instanceId` for one key in one channel |
| `/events` (46022) | `invalid: unknown host step`, `claimed by <hex>`, `expired` | [NIP-HS](nips/NIP-HS.md) claim rule |
| `/query` | a filter without `kinds` is refused | always name kinds |
| 44224 `error.code` | [NIP-CSE](nips/NIP-CSE.md) | the provider's answer, never the relay's |

## Not a replacement for the inside view

A foreign client sees signed records and admissions. It does not see
processes, files, keys, or the model. Where this page says "not visible",
the inside proof is named; the header of `scripts/foreign-client-check.sh`
lists what its green leaves unproved so nobody reads it as more than it is.
