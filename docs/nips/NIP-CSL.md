# NIP-CSL: Coding-Session Lifecycle Commands

`kind:44221` is a durable, channel-scoped lifecycle command from a Buzz
operator to a coding-session provider adapter. It creates a new session without
exposing a host path, environment, secret, provider session identifier, or
generation in signed operator intent.

This contract complements [NIP-CSC](NIP-CSC.md): NIP-CSL creates a session;
NIP-CSC sends turns to an existing exact session generation. Both are storage
and fan-out events. The relay validates and stores them but never routes them
through `command_executor` or executes them.

Provider-neutral rendering after creation uses the signed
[NIP-CST transcript contract](NIP-CST.md) (`kind:44225`).

> **Fork amendments.**
>
> 1. **Native kinds only.** No kind-9 compatibility fallback exists anywhere in
>    this fork — not for 44221, and not for the 44223/44224 lifecycle facts
>    below. Every publication uses exactly one native kind; kind-44223 metadata
>    may publish repeatedly as append-only observations on state transitions.
> 2. **`projectRef` is optional.** A session may stand alone, owned by the
>    channel it is published into rather than by a project. See below.
> 3. **`projectRef` coordinates are `30621:` only.** The donor's example used
>    the `30178:` team-catalog kind; sessions here bind to NIP-MP projects
>    (`kind:30621`) and nothing else.
> 4. **`sessionRef` groups executions into an umbrella session.** A nullable,
>    client-minted UUID added after v1 shipped. Authority-aware creates also
>    carry `genesisRef`, the exact founder event id. The action has exactly the
>    historical 8-key, 9-key `sessionRef`, or 10-key linked form. See below.
> 5. **Continuation is generation-fenced.** `session.resume` and
>    `session.stop` address an exact published `cs-target`. Resume never carries
>    the provider's opaque ACP cursor; that cursor remains host-private. A
>    successful reattachment publishes a new generation, while stop is durable
>    intent that survives provider restart.
> 6. **Liveness is an ephemeral lease, not metadata freshness.** Kind 24223
>    proves recent provider reachability for one exact generation. It does not
>    replace durable metadata and is never written to Postgres.
> 7. **A turn gets its own receipts.** `turn_queued`, `turn_started`,
>    `turn_degraded`, `turn_dropped`, `turn_refused`, and
>    `interrupt_delivered` — keyed by `commandId` and `status`
>    together, never confirming or ending a generation on their own. Their
>    `commandId` normally names a `kind:44220` turn command, but for the
>    initial turn embedded in a `kind:44221` `session.create` it names that
>    **create**. See "Fork amendment: turn-stage receipts" below.

## Wire contract

Event content has exactly this JSON shape. Nullable fields remain present with
an explicit `null`, and additional or missing fields are invalid:

```json
{
  "schema": "buzz-coding-session-lifecycle-command/v1",
  "commandId": "client-idempotency-id",
  "action": {
    "type": "session.create",
    "projectRef": "30621:<lowercase-64-hex-owner>:<project-d>",
    "repoRef": "30617:owner:repository",
    "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    "genesisRef": "64-lowercase-hex-genesis-event-id",
    "providerInstanceRef": "capability-advertised-instance",
    "providerAuthorityPubkey": "64-lowercase-hex-catalog-signer",
    "model": "provider-neutral-model-id",
    "title": "Operator-facing session title",
    "initialTurn": "Inspect the project and begin."
  }
}
```

The same v1 envelope also admits exactly these two lifecycle action shapes
(and, per the fork amendment below, a fourth: `session.hire`):

```json
{
  "schema": "buzz-coding-session-lifecycle-command/v1",
  "commandId": "client-idempotency-id",
  "action": {
    "type": "session.resume",
    "session": {
      "driver": "codex-acp",
      "instanceId": "provider-instance-id",
      "sessionId": "provider-minted-buzz-session-id",
      "generation": 1
    },
    "providerAuthorityPubkey": "64-lowercase-hex-catalog-signer"
  }
}
```

`session.stop` has the identical three-key action with `type` set to
`session.stop`. Adding a discriminated action is an additive v1 evolution:
older consumers reject an unknown action and therefore fail closed; they must
not reinterpret it as `session.create`. That fail-closed rule is what makes
`session.hire` (fork amendment below) safe to add and what obliges a publisher
to name an old relay as old rather than blame its own request.

`session.resume` addresses the exact disconnected generation the operator
observed. The provider resolves its persisted ACP cursor, working directory,
runtime, and model locally. On success it keeps the Buzz `sessionId`, advances
`generation` by one, resets the per-generation transcript sequence, and emits
new metadata. ACP `session/resume` is preferred when advertised;
`session/load` is a compatibility fallback. If neither recovers context, a new
ACP session may still attach as the next generation, but its receipt and
transcript must say `CONTEXT_NOT_RECOVERED` /
`session_restarted_without_context` rather than claiming continuity.

Every ACP session-open method (`session/new`, `session/resume`, and
`session/load`) receives the same launcher-selected private, read-only context
MCP when a verified package is available. Native resume/load does not receive a
reconstructed-context system prompt because it already supplies
provider-native context for that execution; the MCP remains available as an
evidence surface for sibling work observed in the snapshot.

For a reconstructed `session/new`, the launcher MUST push a bounded
`coding-session-first-turn-brief/v1` before the model's first token, using the
adapter's system-prompt transport where supported and a first-turn preamble
only as fallback. The brief is deterministic, not model-authored: session
identity and safe goal/name, snapshot provenance, recent turn transport
outcomes, plan counts, tool attempt outcomes, and signed evidence ids. It MUST
NOT carry tool arguments/results, reasoning, host paths, credentials, or the
opaque ACP cursor. `ended_normally` means only that ACP ended the turn normally;
it MUST NOT be presented as semantic task completion.

Depth stays pull-based through `session_overview`, `session_history`, and
`search_session`. `session_overview` repeats the exact first-turn brief. A
complete package is complete only through its additive `completeAsOf`
watermark; later concurrent work may exist. Historical packages without that
field remain readable but MUST be described as having an unknown legacy
watermark. `sourceEventCount` counts the complete verification proof graph
(identity, authority, lifecycle, metadata, and transcript), while
`totalHistoryItems` counts transcript items only.

`session.stop` addresses the exact current generation. Once consumed, the
provider records the execution as closed before releasing its process. Restart
recovery must not attach or make resumable a closed record. This is different
from `thread.turn.interrupt`, which cancels only the in-flight turn and leaves
the execution available. The stopped generation's final metadata status is
`stopped`; consumers must treat that lifecycle status as terminal. It is
distinct from `completed`, which remains the degraded transcript-only inference
for a successful turn when metadata is absent.

`providerInstanceRef` and `providerAuthorityPubkey` are required. The provider
authority is exactly the lowercase 64-hex signer of the selected
[provider-catalog](NIP-CSPC.md) event; it addresses the command to one adapter
even when several share a channel. `projectRef`, `repoRef`, `sessionRef`, `model`,
`title`, and `initialTurn` are nullable; `genesisRef`, when its key is present,
must be a non-null lowercase 64-hex event id. A present string must be nonempty
after trimming (`sessionRef` carries its own stricter shape, below). Limits are
UTF-8 byte limits:

- `commandId`: 256 bytes
- all references, `model`, and `title`: 2 KiB (2,048 bytes) each
- `initialTurn`: 12 KiB (12,288 bytes)
- complete event content: 16 KiB (16,384 bytes)

### `projectRef`: optional, but structurally required

`projectRef` may be `null`, which creates a **standalone session**. Sessions
organize under projects when the project-containers feature is present and are
fully usable without it.

Optional is not the same as omissible. The key must still be written — an
explicit `null` — exactly like every other nullable field. A payload that
simply lacks the key is a truncation or a partial serialization, and reading it
as a deliberate standalone session would silently detach a session from the
project its operator chose. Decoding rejects it.

Optional is also not the same as unvalidated. A **present** `projectRef` must
be a canonical NIP-MP project coordinate:

```
30621:<lowercase-64-hex-owner>:<project-d>
```

parsed by splitting on the first two colons only, so a project whose `d` tag
contains a colon stays addressable. Owner hex must be lowercase: `#a` filter
matching is byte-exact, so an uppercase-owner coordinate would be invisible to
the queries readers actually issue. A `30178:` team-catalog coordinate, a
`30617:` repository coordinate, and a bare slug are all rejected.

### Fork amendment: `sessionRef` umbrella reference

One user-facing session may contain several provider executions — a Claude
execution and a Codex execution as co-participants in one surface. The grouping
identity is `sessionRef`: a client-minted canonical UUID (36 characters,
`8-4-4-4-12`, lowercase hex), deliberately distinct from every
provider-runtime identifier. A later create carrying the same `sessionRef`
**joins** the umbrella as a new execution with its own `cs-target` and its own
fact streams; nothing about receipts, metadata keys, generations, or
transcripts changes shape. `null` claims no umbrella — the pre-amendment
semantics, an implicit umbrella of one.

**Decode discipline — where this differs from `projectRef`.** `projectRef` was
in the schema from v1, so its key is structurally required and only its value
may be `null`. `sessionRef` was added to an already-deployed schema: signed
v1 events without the key exist and must stay valid forever. The action is
therefore **exactly the 8-key v1 set, exactly the 9-key set including
`sessionRef`, or exactly the 10-key set including both `sessionRef` and
`genesisRef`** — nothing between, nothing beyond. `genesisRef` without a
non-null `sessionRef` is invalid. New producers always write `sessionRef`
(explicit `null` or a UUID); authority-aware producers add `genesisRef`. The
8-key form is accepted only as historical replay.

Optional is still not unvalidated. The reference travels in no tag, so nothing
downstream normalizes it; two clients agree on umbrella membership only if the
bytes are byte-exact. A present `sessionRef` must therefore be canonical —
uppercase hex, braces, URN prefixes, and truncations are rejected rather than
coerced, because a non-canonical spelling would silently split an umbrella in
two.

No tag carries either reference. A `genesisRef` is resolved only by its event
id; consumers MUST NOT select authority by querying a genesis tag or by
choosing among events with the same `sessionRef`. The resolved genesis must be
signature-valid, kind 44226, scoped to the create's channel, and carry the
same `sessionRef`. Its signer is the founder. Creates without `genesisRef`
retain the interim legacy projection: founder is the founding create signer.

The provider echoes a claimed reference into `kind:44223` metadata as an
*optional* `sessionRef` key — emitted only when non-null, never as an explicit
`null` — so pre-amendment consumers' exact-key metadata check keeps accepting
every session that never claimed an umbrella. The echo is a projection
convenience for catalog grouping; the operator-signed create remains the
authoritative membership claim. If they ever disagree, consumers trust the
create and flag the record.

### Fork amendment: `actor` and `role` (agent seats)

A `session.create` may seat an **agent identity** on the execution it creates,
rather than leaving it operated only by whoever steers it. Two new keys,
always **both present or both absent** — never one without the other:

- `actor`: the agent's pubkey, lowercase 64-hex.
- `role`: a slug naming the seat's function within its crew (`lead`,
  `architect`, `builder`, `verifier`, `runner`, `poker`, ...), 1–64 bytes,
  matching `[a-z0-9-]+`.

A create carrying exactly one of the two is malformed — refused with
`ACTOR_ROLE_PAIR` — the same "no partial shape" discipline `sessionRef` and
`genesisRef` already follow above. `actor`/`role` compose with every existing
form: the action is exactly one of the historical 8-key, 9-key (`sessionRef`),
or 10-key (`sessionRef` + `genesisRef`) sets described above, **or** those same
three sets with `actor` and `role` also present (10-, 11-, and 12-key sets
respectively) — nothing between, nothing beyond. A create with no `actor`
(and no `role`) behaves byte-for-byte as it does today; this amendment adds
shapes, it does not change any existing one.

An unresolvable `actor` — no host-local record identifies key material for
that pubkey — refuses the create with a lifecycle receipt
`failed` / `ACTOR_UNAVAILABLE`; no execution, metadata, or transcript is ever
produced for it.

The provider echoes the seat into `kind:44223` metadata: the existing
`agentRef` key (previously always `null` — no execution held an identity)
carries the create's `actor`, and a **new optional key**, `role`, echoes the
create's `role` — present only when `agentRef` is non-null, never as an
explicit `null`, mirroring the `sessionRef` echo's optionality above. A
metadata event for an execution with no seated actor keeps today's shapes
byte-for-byte: `agentRef` stays present-but-`null` (a base field since v1),
and `role` is simply absent. Decoders reject `role` present alongside a
`null` `agentRef`, and reject `agentRef` non-null with no `role` — the pair
travels together on the metadata echo exactly as it does on the create.

### Fork amendment: actor custody is host-local, never on the wire

Seating an actor is not a key-distribution protocol. The `actor` key above
names *whose* identity the seat holds; it never carries *how* the provider
gets that identity's signing material. Custody, private-key injection, and
channel membership for the seat are decided and executed entirely on the
founder's own machine, before or alongside publishing the create — none of
it is a signed event, a wire field, or a fact this NIP's contract can express,
and that is deliberate:

- **Key material never crosses the wire.** The provider resolves `actor`'s
  private key, relay URL, and (if any) auth tag from a host-local record
  (keyed by the create's `commandId`, written by the desktop *before* it
  publishes the create) — never from the signed `session.create` content,
  never from a relay query, never logged. A `session.create` names an actor
  the same way a `providerAuthorityPubkey` names an adapter: as a public
  reference to resolve locally, not as a credential to carry.
- **The env fence still strips everything by default.** Past that fence, an
  actor seat's spawned process additionally receives exactly
  `BUZZ_PRIVATE_KEY`, `BUZZ_RELAY_URL`, `BUZZ_AUTH_TAG` (omitted when null),
  and a `NOSTR_PRIVATE_KEY` mirror — the same three-and-a-mirror shape managed
  agents already receive at spawn, and nothing else. A create with no `actor`
  is fenced exactly as before: the fence's exemption list does not grow.
- **Channel membership is a separate, host-executed step, not a wire
  consequence of the create.** The relay only accepts a `kind:44220` from a
  channel member (per NIP-CSC), so before publishing an actor create the
  desktop adds the actor's pubkey to the session's channel as its own,
  independently observable membership fact — never inferred from the create
  event itself. A membership failure leaves the create unpublished with a
  named reason; an unauthorized actor pubkey is never silently seated.

None of this — the host-local custody record, the post-fence env injection,
or the membership addition — is part of this contract's wire shape. A relay,
a sibling execution, or any other reader of the signed record sees only
`actor` (a pubkey) and `role` (a slug); it never sees, stores, or transmits
the key material or the local record that resolved it.

### Fork amendment: `beeStamp` — which `bee` the seat actually ran

A seat runs whatever `bee` its harness puts on `PATH`. On 2026-09-01 that was
the desktop app's bundled sidecar, three fixes behind the checkout, and the
run produced two binaries answering about one channel with neither of them
saying so (item 103 finding 1). The host therefore resolves exactly one `bee`
before a seat starts — the sidecar beside its own executable, failing that the
first on the inherited `PATH` — injects it as `BEE` with that binary's own
directory prepended once to the seat's `PATH`, and **itself** runs
`$BEE --version` and records the answer. Nothing is asked of the agent: an
agent's account of which binary it ran is a claim, and this has to be a
record. The provider echoes it into `kind:44223` metadata as a **new optional
key**, `beeStamp`, present only for a seated execution:

```json
"beeStamp": {
  "path": "/Applications/Beekeeper.app/Contents/MacOS/bee",
  "source": "bundled",
  "version": "0.1.0",
  "sha": "23728227b",
  "dirty": false
}
```

`source` is exactly `bundled` or `path` — the two outcomes the resolution has;
a third would be a guess. `sha` is the abbreviated commit the binary was built
from, lowercase hex, and never carries the `-dirty` suffix `bee --version`
prints: that is the separate `dirty` flag, so no reader has to string-strip a
commit name before comparing it. All five keys are always present when the key
is, with `version`, `sha` and `dirty` all JSON `null` together when
`--version` was unparseable or exited non-zero — the surfaces then read
**unknown**, never blank. The key is omitted entirely, never emitted as an
explicit `null`, for an unseated execution and for every pre-amendment
generation; a metadata event that never claims a stamp keeps today's shape
byte-for-byte, mirroring `sessionRef`, `role`, `turnBudget` and `routing`
above. A reader MUST reject `"beeStamp": null` rather than read it as absence.

### Fork amendment: umbrella turn budget (`turnBudget`) and `BUDGET_EXHAUSTED`

A crew's umbrella (plan D9) carries a turn budget: a ceiling on how many
agent-originated turns it may run, set alongside the session ceiling and
disclosed the same way. The provider counts consumed turns per `sessionRef`,
durably (its own state, not derived from a fold at read time — a restart must
not reset the count), and refuses every further turn-starting `kind:44220`
(`thread.turn.start`) whose signer is not the founder once the count reaches
the limit, with a lifecycle receipt `turn_refused` / `BUDGET_EXHAUSTED`.
`thread.turn.interrupt` is **never** refused by this rule: it starts no work,
spends nothing, and is the only way to stop a turn already running — refusing
it would leave a runaway turn with no brake at exactly the moment the budget
says the crew has gone too far. **Founder turns are never refused by this rule** — the
budget bounds crew traffic, not the human who owns the umbrella. A generation
with no `sessionRef` has no umbrella to count against and is never refused
this way.

`BUDGET_EXHAUSTED`'s `error.message` states the count that caused the refusal
and the ceiling it hit (the same `used`/`limit` pair the metadata echo below
carries), so a refused sender can read *why* without a second query. It joins
the open code set the turn-stage table above documents — no closed list, 64
UTF-8 bytes, the same rules as every other code in that table.

The provider echoes the running count into `kind:44223` metadata as a
**new optional key**, `turnBudget`, present only when the umbrella carries a
configured budget:

```json
"turnBudget": { "used": 7, "limit": 20 }
```

Both `used` and `limit` are non-negative integers; `used` may equal or exceed
`limit` (the exhausted state itself is a fact worth publishing, not a shape to
avoid). The key is omitted entirely — never emitted as an explicit `null` —
for an umbrella with no configured budget and for every pre-amendment
generation, mirroring the `sessionRef` and `role` echoes' optionality above:
a metadata event that never claims a budget keeps today's shape byte-for-byte,
so this amendment adds one more optional shape to the exact-key set rather
than changing an existing one. `used` and `limit` are never split across two
keys, and neither travels without the other.

`bee sessions status` prints this as a `turnBudget` line per execution
(`used/limit`, or absent when the key has never been seen) — see
`crates/buzz-cli/TESTING.md` § Coding Sessions for the exact shape.

### Fork amendment: `session.hire` — a fourth action, answered by a host

A lead seat cannot create another seat: an agent never holds key material, and
a create names an `actor` whose custody is host-local (see *actor custody is
host-local* above). Plan D14 makes the lead's verb a **request** instead. A
fourth action, `session.hire`, asks the umbrella's host to seat a role; the
host decides, and the seat it produces is an ordinary seated `session.create`.

```json
{
  "schema": "buzz-coding-session-lifecycle-command/v1",
  "commandId": "client-idempotency-id",
  "action": {
    "type": "session.hire",
    "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    "genesisRef": "64-lowercase-hex-genesis-event-id",
    "role": "builder",
    "providerInstanceRef": "capability-advertised-instance",
    "model": "provider-neutral-model-id",
    "brief": "Rebase the lane and run the gate."
  }
}
```

Exactly these seven keys, or those seven plus `routing` — the routing
**request**, never the routing record (the fork amendment below). `providerInstanceRef` and `model` are nullable and
**structurally required** — written as explicit `null` when the requester
leaves the choice to the host — for the same reason `projectRef` is. The
others are required non-null strings: `sessionRef` is a canonical lowercase
UUID, `genesisRef` a lowercase 64-hex event id, `role` a
`[a-z0-9-]{1,64}` slug (the same slug a seated create writes), and `brief`
1..12288 bytes, the `initialTurn` ceiling — because the brief *becomes* the
seat's first turn.

`genesisRef` is required here although a create's is optional: a hire is
authorized against the umbrella's genesis, so a hire that names none names
nothing that can authorize it.

**Authority is checked by the relay, on ingest.** Unlike every other lifecycle
action — whose founder-onlyness the provider enforces — a hire never reaches a
provider at all, so the relay resolves the action's exact `genesisRef` in the
same channel, verifies that genesis names the action's `sessionRef`, and
requires the signer to be its founder, to hold a live NIP-CSAT `operator`
grant, or to hold an active accepted `lead` seat. Founder and operator may hire
any role. A lead may hire only a non-lead role and cannot create or revive lead
authority. Revoked, stale, wrong-channel, and wrong-genesis seats confer
nothing; lifecycle metadata never substitutes for the accepted 44228 chain.
An umbrella no exact genesis in that channel claims is refused by name rather
than admitted on channel membership.

**The host answers, two ways.** On acceptance it applies its own standing
policy (hiring on/off, allowed roles, a maximum number of live seats per
umbrella, allowed providers), chooses an installed identity whose home role is
`role` and that is not already live in this umbrella, gives that seat its own
worktree, stages custody exactly as a desktop-initiated seat does, and
publishes a seated `session.create` — with `providerInstanceRef` from the
request or the policy default, `model` from the request or the identity's, the
umbrella's title inherited, and `initialTurn` set to the brief prefixed
`"[From the lead] "`. **That create's receipts are the hire's receipts**; the
hire itself is answered by no receipt of its own, and its `commandId` never
appears in a `kind:44224`.

After that receipt exists, the process holding the hire signer's key MUST
verify the exact signed genesis, seated create, provider receipt, and provider
metadata signatures. The create signer MUST equal the genesis founder; a
matching create/receipt/metadata triplet from a member-controlled provider is
not host authority. Their channel, session, genesis, command, provider
authority, provider instance, receipt target instance, metadata provider and
runtime, actor, and role MUST agree. A non-null provider instance requested by
the hire MUST equal the create's answer; when the request left routing to the
host, the create, receipt target, and metadata must still agree exactly. Only
then may that signer append a NIP-CSAT
`grant-seat` for the exact actor-role pair. This is signer-owned follow-up,
never provider self-minting. The accepted 44228 receipt chain is the authority
proof: an already-active exact pair is idempotent and returns its existing
accepted grant; an active different role requires an explicit revoke/change.
If another accepted transition advances the head between read and write, the
signer refetches the receipt-backed projection, rebuilds, and retries once.
If execution creation succeeds but the grant fails, the execution remains
live and the client reports a non-success partial outcome with the create and
receipt ids. It MUST NOT hide, delete, or re-hire that seat.

On refusal the host answers the requesting seat with a `kind:44220` turn whose
text is exactly `hire refused: <CODE> — <reason>`, where `<CODE>` is one of
`HIRE_OFF`, `HIRE_ROLE_NOT_ALLOWED`, `HIRE_LIMIT`, `HIRE_NO_IDENTITY`,
`HIRE_ROLE_BUSY`, `HIRE_PROVIDER_NOT_ALLOWED`, `HIRE_MODEL_NOT_OFFERED`,
`HIRE_NO_ROUTE`, `HIRE_MALFORMED`, or
`HIRE_STALE`, and shows the same line in the umbrella as a
system row. The codes are the constants
`HIRE_REFUSAL_CODES` in `crates/buzz-core/src/coding_session_lifecycle_command.rs`;
the prefix is `HIRE_REFUSAL_PREFIX` in the same file.

`HIRE_ROLE_BUSY` and `HIRE_NO_IDENTITY` are two codes for what was one until
2026-08-28, and the difference is whose remedy it is. `HIRE_NO_IDENTITY` means
no installed agent on that computer holds the role at all — only the operator
can fix it, by installing the role. `HIRE_ROLE_BUSY` means the computer holds
the role and every identity that *is* it is already seated in this umbrella:
nothing is broken and nothing needs installing, so the reason names that seat
and the remedy is for the requesting seat to address it — `bee sessions send
--session-ref <umbrella-uuid> --to <role>` — rather than hire again.

### Fork amendment: `routing` — why *this* execution target

Brian's ruling of 2026-08-30: **"The lead chooses the capability required. The
router chooses the execution target."** The thing chosen is a provider + model +
effort, chosen by a stated procedure, and the record of that choice travels with
the work. `session.hire` therefore takes one additive key, `routing`; the
resulting `session.create` echoes the decision, and so does the seat's
kind:44223 metadata. A seat can always be asked why it is the model it is, from
the wire alone.

**`routing` is two different shapes, and which one is legal depends on the
action.** A `session.hire` carries the routing **REQUEST** — the question. A
`session.create` and a kind:44223 carry the routing **RECORD** — the answer.
Only the founder's host may write the record, because only the host can see its
own live kind:44222 catalog. Publishing the record on a hire is refused, by the
name of the key that does not belong, and vice versa.

That was one shape until 2026-08-30, and the cost was exact: a CLI emitted the
record on a hire, a host accepted only the request, and every routed hire was
classified malformed and **dropped with no answer of any kind** — no
kind:44220, no log line, nothing rendered — while the lead waited. Two shapes
is how that stops being possible to write.

#### The request, on `session.hire`

```json
{
  "class": "builder",
  "risk": { "impact": 3, "uncertainty": 3, "irreversibility": 2 },
  "profile": { "taste": 4.5 },
  "override": { "model": "gpt-5.6-terra[medium]", "effort": null, "because": "…" },
  "challengerSample": true,
  "reviewFlags": ["contractChange", "leadRequests"],
  "proposed": {
    "chosen": { "provider": "claude-primary", "model": "sonnet", "effort": "medium" },
    "runnerUp": { "provider": "codex-primary", "model": "gpt-5.6-luna[medium]", "effort": "medium" },
    "reason": "…",
    "registryVersion": 1,
    "catalogRevision": 7
  }
}
```

* `class` and `risk` are **required**. Everything else is **omitted** when it
  has nothing to say — never written as an explicit `null`. This is the
  opposite convention from the record below, and deliberately so: a question
  states only what was asked.
* `risk` here carries **three** keys and no `score`. The product is arithmetic
  the host does; a `score` a requester could set is a number that can disagree
  with its own factors.
* `reviewFlags` are tokens from the §6 trigger list —
  `securityBoundary`, `contractChange`, `outsidePlan`, `builderUncertain`,
  `testsInsufficient`, `leadRequests`. A token nobody defined is refused **by
  name**, never dropped: a trigger the wire swallows is a review the lead
  believes it asked for and did not get.
* `proposed` is the requester's own local routing decision. It is
  **informational**. It never binds the host, whose live catalog may
  legitimately differ; when the host lands elsewhere it says so on the create
  in `routing.proposedDisagreement`.
* `override` is the **only** way a hire dictates an execution target, and
  `because` is required. The hire's top-level `model` and `providerInstanceRef`
  are set only when `override` is present, and then they equal `override.model`.
  A routed hire that filled them in with the requester's own proposal would be
  dictating a target while claiming to ask a question.

#### The record, on `session.create` and kind:44223

```json
{
  "class": "builder",
  "tier": "standard",
  "risk": { "impact": 3, "uncertainty": 3, "irreversibility": 2, "score": 18 },
  "profile": null,
  "chosen": { "provider": "claude-primary", "model": "sonnet", "effort": "medium" },
  "runnerUp": { "provider": "codex-primary", "model": "gpt-5.6-luna[medium]", "effort": "medium" },
  "reason": "claude-primary/sonnet cleared the builder gate (…) and is the cheapest expected accepted completion at standard/medium (2.9 vs 2.0 …)",
  "reviewRequired": false,
  "reviewReasons": [],
  "challengerSample": false,
  "override": null,
  "registryVersion": 1,
  "catalogRevision": 7
}
```

**Exactly these thirteen keys, always all of them, plus an optional
fourteenth.** Unlike the additive keys elsewhere in this document, the record's
*own* fields are never omitted: a field whose answer is not known is written as
an explicit `null`. That gives a strict observer one shape to accept instead of
a family of them. The key `routing` itself **is** omitted — never written as
`null` — when nothing routed, so a hire or create signed before the router
existed stays byte-valid forever.

The fourteenth key is `proposedDisagreement`: one sentence, written by the host
**only** when its own choice differs from the `proposed` the hire carried, and
omitted entirely otherwise, so a record with nothing to disclose is
byte-identical to the thirteen-key form. Honouring an `override` is not a
disagreement — that difference is the requester's own instruction, not the
host's judgment. A host that routes elsewhere and says nothing leaves a lead
reading its own proposal back as though it had been honoured, which is the
whole reason `proposed` is on the wire.

* `class` is the capability the lead named. The lead never names a model.
* `tier` is `fast` | `standard` | `deep`, **derived** from `risk`: 1–8, 9–39,
  40–125. It is never independently asserted, and `risk.score` must equal
  `impact × uncertainty × irreversibility` or the payload is refused.
* `chosen.effort` is `low` | `medium` | `high` and nothing else. `xhigh`, `max`
  and `ultra` are human override only; a payload naming one is refused at the
  wire, not merely discouraged. Where the catalog publishes an effort variant
  (`gpt-5.6-sol[high]`) the chosen `model` carries the effort itself; where it
  does not (`sonnet`), `effort` is the tier's policy and the `reason` says so
  rather than implying the wire carries it.
* `reviewRequired` follows the review trigger list, **not** the tier. A record
  claiming `reviewRequired: false` while `reviewReasons` lists triggers that
  fired is refused: it disagrees with itself.
* `reviewReasons` is an **open vocabulary of bounded tokens** — at most 16
  entries, each 1..=256 non-blank bytes — and an observer must not police it
  against a closed set. Six of the §6 triggers ride as the flag names the
  caller passed (`securityBoundary`, `contractChange`, `outsidePlan`,
  `builderUncertain`, `testsInsufficient`, `leadRequests`); the two the router
  computes carry the number that fired them, `risk 80 >= 40` and
  `irreversibility 4 >= 4`, because a reader told only `risk>=40` has to redo
  the arithmetic to learn what happened. An implementation that enumerated the
  slugs would reject the router's own record as malformed.
* `challengerSample` marks a decision that deliberately routed a challenger, so
  its outcome can be attributed later rather than read as a normal route.
* `override` is a human overruling the router. `because` is required and
  non-blank — an unexplained override is indistinguishable from a bug — and the
  router's own pick survives as `runnerUp` so nothing is hidden.
* `registryVersion` and `catalogRevision` say which registry and which catalog
  produced this. `catalogRevision` is `null` when more than one signer published,
  because two hosts share no revision counter.

**A hire carries the question; a create must carry the answer.** A
`session.hire` carries a routing request and nothing else — it has no `tier`,
no `chosen`, no `reason`, and a `risk` of three factors. On a `session.create`
the record must be *complete*: `chosen`, `reason`, `reviewRequired` and
`registryVersion` all non-null. A create is the record of a decision that was
made; a null `chosen` there would claim a decision nobody made.

**`HIRE_MALFORMED`.** A hire whose `routing` does not parse against the request
shape is answered `HIRE_MALFORMED`, and the reason names the failing key. It is
never dropped and never left unanswered: a request that gets no answer is a
crash with better manners, and that is exactly what happened on 2026-08-30
before this code existed.

**`HIRE_NO_ROUTE`.** When nothing the live catalog offers clears the class gate
at that risk tier, the host answers `HIRE_NO_ROUTE` and the reason names the
binding trait, the minimum it wanted, and the best score anything available
actually has. It is deliberately a refusal rather than a quiet demotion: *"the
smartest available model"* and *"the cheapest that fits"* are both wrong answers
to a requirement nothing meets. `bee sessions hire --class …` runs the router
locally too, but only to attach `proposed`: the host's catalog is the one that
decides, so a local no-route is reported rather than used to refuse a hire the
host might well have served.

The schema, the gates and the selection order live in
`crates/buzz-core/src/coding_session_routing.rs`; the rows and their provenance
live in `team/model-registry.yaml`. The two shapes are pinned byte-for-byte by
`testdata/routing/hire-request-fixture.json` and
`testdata/routing/create-record-fixture.json`, which `buzz-core`'s validator,
the CLI's emitter and the desktop's parser all read.

**Deployment order.** The relay validates 44221 with `deny_unknown_fields` and
a closed action list, so a `session.hire` is only valid once the relay carrying
it is deployed. A client that publishes one to an older relay is refused
`invalid: coding-session lifecycle command action type is unsupported`, and it
must say so — `bee sessions hire` renders that as *"this relay does not accept
hire requests yet"* rather than passing a shape error through as if the request
were malformed.

### Tags

Exactly these three two-field tags, in this order:

1. `["h", "<channel UUID>"]`
2. `["csl-v", "csl1-1"]`
3. `["csl-command", "<commandId>"]`

The `csl-command` value must equal the payload `commandId`. Order is
load-bearing — adapters read these positionally. No tag carries the project
reference, so a standalone session and a project-bound one produce identically
shaped envelopes.

The command author is the signed event pubkey. Consumers must never use content
as claimed operator attribution.

## Authority and provider resolution

The relay requires `messages:write`, a valid `h` channel scope, and an active
channel membership row. Open-channel visibility is not lifecycle authority.
Kind 44221 is never global and is not a relay-executed command.

`projectRef`, `repoRef`, and `providerInstanceRef` are signed logical
references. `providerAuthorityPubkey` is the selected catalog signer, not an
operator identity claim; a consumer must compare it to its own current signing
pubkey before reserving a command or causing provider side effects, and ignore
commands addressed to another authority.

**No host-local runtime state travels in signed content.** Not paths,
environment variables, secrets, process identifiers, or opaque ACP session
ids. The provider-neutral Buzz `cs-target` is the deliberate exception for
`session.resume` and `session.stop`: its public session id and generation fence
already identify signed lifecycle facts. The working directory in particular
is machine-local configuration the producer resolves for itself;
`deny_unknown_fields` on the action means a `cwd` smuggled into the payload is
a decode failure, and the relay rejects the event.

`commandId` is the idempotency key for lifecycle consumers. A consumer should
bind a successful creation result to the signed project and channel before it
publishes provider-neutral session projections.

## Lifecycle facts

The provider publishes generation metadata observations as `kind:44223` and
lifecycle receipts as `kind:44224`, using only those native kinds. A receipt is
one immutable outcome for one command. Metadata is append-only observation
history: the provider publishes a new event on a state transition, and a
generation may therefore have several events with the same semantic grouping
key.

Metadata tags, in order: `h`, `csm-v` (`csm1-1`), `cs-target`, `csm-key`.
Receipt tags, in order: `h`, `cslr-v` (`cslr1-1`), `csl-command`, `csl-key`.

Semantic keys are the same length-prefixed encoding NIP-CSC describes:

```
coding-session-metadata/v1|<driver><instanceId><sessionId><generation>
coding-session-lifecycle-receipt/v1|<commandId>
coding-session-lifecycle-receipt/v1|<commandId>|<status>
```

A **lifecycle** receipt (`created`, `created_with_failed_initial_turn`,
`failed`, `resumed`, `resumed_without_context`, `stopped`) is keyed by
`commandId` alone: one lifecycle command has exactly one outcome, so a second
receipt for the same command is a duplicate to drop, never a revision to
apply. A **turn** receipt (below) is keyed by `commandId` *and* `status`,
because one `kind:44220` turn command legitimately produces several of them in
sequence (for example `turn_degraded`, then `turn_queued`, then
`turn_started`) and keying by `commandId` alone would drop everything after
the first. Consumers retain all metadata rows and fold the newest valid
observation per exact generation by `(created_at, event id)`. A new generation
fences resume identity; it is not required for an ordinary status transition
or corrected observation.

The receipt keeps the same exact five-key v1 object. In addition to the create
statuses, lifecycle continuation uses:

- `resumed`: `session` is the new-generation target and `error` is `null`;
- `resumed_without_context`: `session` is the new-generation target and
  `error` is `{ "code": "CONTEXT_NOT_RECOVERED", "message": "..." }`;
- `stopped`: `session` is the stopped exact target and `error` is `null`;
- `failed`: unchanged, with `session: null` and a stable error object.

Authority refusals use stable codes: `GENESIS_NOT_FOUND` when an exact genesis
cannot be resolved and verified, and `UNAUTHORIZED_OPERATOR` when a turn,
interrupt, stop, or resume signer is not the cached founder. Both are durable
receipts; the provider must never execute or silently discard these cases.

An old consumer that does not recognize a new status rejects that receipt; it
must never coerce the outcome into `created`. The new generation's metadata and
transcript remain independently verifiable facts.

### Fork amendment: turn-stage receipts

A [NIP-CSC](NIP-CSC.md) turn gets its own receipts, distinct from the six
lifecycle statuses above, so an operator or a sibling agent can watch a turn
land without polling the transcript. Six statuses:

- `turn_queued` — the command was accepted into the session's mailbox and has
  not started yet.
- `turn_started` — the provider began running the turn. **This is the only
  turn status with a sixth key**, `turnId`: the provider's own identifier for
  the started turn.
- `turn_degraded` — the command asked for `deliver: "steer"` and this
  execution's runtime advertised no native steering, so it will be delivered
  at the next turn boundary instead (`STEER_UNSUPPORTED`). The turn is not
  refused, not merged into the running turn, and not lost: a `turn_queued`
  follows. See the downgrade rule in [NIP-CSC](NIP-CSC.md).
- `turn_dropped` — the provider will never run this command and nobody was
  refused: the mailbox was full (`QUEUE_FULL`), the mailbox had no room for
  both halves of an interrupt-class delivery so the running turn was left
  untouched (`QUEUE_FULL_TURN_KEPT`), or the session has no live execution to
  deliver into (`NO_LIVE_EXECUTION`). All are **terminal**: the
  command is never consumed (it did not run) and it is recorded as refused, so
  the answer is given once and no redelivery repeats it. A dropped turn is not
  re-delivered by resuming the session — `session.resume` mints a new
  generation, and a replayed command still addresses the old one, so it would
  be refused as `STALE_GENERATION`. The sender has to send it again. What this
  contract guarantees is that a turn is never *silently* lost, not that every
  accepted turn eventually runs.
- `turn_refused` — the provider will never run this command: an unauthorized
  operator, or a target this provider owns that no longer accepts turns.
- `interrupt_delivered` — a `thread.turn.interrupt` reached a live turn and
  the cancel was issued. An interrupt that finds no live turn gets
  `turn_refused` (`NO_TURN_IN_FLIGHT`) instead, and one from a signer who may
  not steer the session at all gets `turn_refused`
  (`UNAUTHORIZED_OPERATOR`). A `thread.turn.interrupt` is **not** founder-only:
  any signer who may steer the execution may send one. Only the
  `deliver: "interrupt"` *class* on a `thread.turn.start` is founder-only, and
  a granted operator can reach the same effect in two commands (interrupt,
  then start). That gap is recorded here rather than papered over; closing it
  is authority work, not delivery work.

Every turn status except `turn_started` keeps the exact five-key v1 object
(`schema`, `commandId`, `status`, `session`, `error`) — no `turnId` key at
all, present or `null`. `turn_started` has exactly six keys: the five plus
`turnId`.

`commandId` names the signed command that caused the turn, and that command is
one of **two** kinds. Normally it is a `kind:44220` `thread.turn.start` or
`thread.turn.interrupt`. For the initial turn embedded in a `kind:44221`
`session.create`'s `initialTurn` there is no `kind:44220`, so the provider
publishes that turn's stage receipts under the **create's own** `commandId` —
which is what makes the first prompt joinable to the command that asked for it
(see [NIP-CST](NIP-CST.md)). A consumer MUST resolve a turn receipt's
`commandId` against both kinds and MUST NOT reject a `turn_started` whose
`commandId` names a `kind:44221` as a malformed cross-kind receipt. It follows
that one create's `commandId` can carry both a lifecycle receipt (`created`)
and turn receipts; the `status` discriminates them, and the semantic key keeps
them distinct on the wire.

`session` is the target the command addressed — `driver`, `instanceId`,
`sessionId`, `generation` — for **every** turn status, including
`turn_refused`/`turn_dropped`: unlike a failed `session.create` receipt, a turn
receipt's session is never `null`, because by the time a turn receipt is
published the exact generation is known.

`error` is `null` for `turn_queued`, `turn_started`, and
`interrupt_delivered`, and a `{ "code", "message" }` object for
`turn_degraded`, `turn_dropped`, and `turn_refused`.

**The code set is open, and the bound is exactly 64 UTF-8 bytes.** A validator
accepts any nonblank code of **at most 64 UTF-8 bytes**
(`MAX_RECEIPT_ERROR_CODE_BYTES`, `crates/buzz-core/src/coding_session_payload.rs:103`,
checked at `coding_session_payload.rs:520-524`) containing no control
characters, and MUST NOT pin `turn_dropped`, `turn_degraded`, or `turn_refused`
to a closed list — a provider that grows a new reason must not be decoded as
malformed by a client that predates it. 64 is normative here so the decoders
converge, because today no reader enforces it. The desktop reader bounds the
same field at 256 (`MAX_ERROR_CODE_BYTES`,
`desktop/src/features/coding-sessions/lib/codingSessionIngressPayloads.ts:35`),
and `bee sessions` applies no bound at all — it decodes receipt content with a
plain `serde_json::from_str::<LifecycleReceipt>`
(`crates/buzz-cli/src/commands/sessions.rs:257`) and never calls the strict
decoder — so a 65-byte code from a future provider renders in both clients.
The only readers that enforce 64 are the strict decoder's two callers, the
pulse fold (`crates/buzz-core/src/pulse_fold.rs:739`) and the generation mint
check (`crates/buzz-db/src/coding_session_generation.rs:250`), and both drop
such a receipt *silently*: it surfaces nowhere as malformed, it simply never
counts. Producers MUST stay within 64; the desktop and `bee` bounds are owed a
narrowing, and the two silent drops are owed a diagnostic.

The codes in use today are documented, not enforced:

| status | code | means |
| --- | --- | --- |
| `turn_degraded` | `STEER_UNSUPPORTED` | this execution's runtime offers no native steering; delivered at the boundary |
| `turn_dropped` | `QUEUE_FULL` | the in-actor turn queue is at `SESSION_QUEUE_DEPTH` |
| `turn_dropped` | `QUEUE_FULL_TURN_KEPT` | a `deliver: "interrupt"` turn needed two mailbox slots (the cancel, then the turn replacing what was cancelled) and there was room for fewer; nothing was cancelled |
| `turn_dropped` | `NO_LIVE_EXECUTION` | the session is persisted but nothing is running to deliver into |
| `turn_refused` | `UNAUTHORIZED_OPERATOR` | the signer may not steer this session — including a non-founder asking for `deliver: "interrupt"` on a `thread.turn.start` |
| `turn_refused` | `UNKNOWN_TARGET` | this provider owns the session id but not that target |
| `turn_refused` | `STALE_GENERATION` | the addressed generation has been superseded |
| `turn_refused` | `SESSION_CLOSED` | the session no longer accepts turns |
| `turn_refused` | `NO_TURN_IN_FLIGHT` | a `thread.turn.interrupt` reached a live execution that had no turn running or awaiting start |
| `turn_refused` | `NO_LIVE_EXECUTION` | a `thread.turn.interrupt` addressed a session with no live process, so there was nothing to cancel |
| `turn_refused` | `QUEUE_FULL` | a `thread.turn.interrupt` could not be delivered because the execution's mailbox is full |
| `turn_refused` | `BUDGET_EXHAUSTED` | the umbrella's turn budget (plan D9) is used up and the signer is not the founder |

**Accepted contract delta, 2026-08-26.** Three of the codes above did not exist
before this fork's delivery-class work and are recorded here as a ratified
extension, not as a pre-existing set: `NO_LIVE_EXECUTION`
(`crates/buzz-core/src/coding_session_payload.rs:85`), `NO_TURN_IN_FLIGHT`
(`coding_session_payload.rs:94`) and `QUEUE_FULL_TURN_KEPT`
(`crates/buzz-session-provider/src/lib.rs:159`). They are legal only because the
same change opened the code list, above. They exist because the alternatives
would have been false statements: `UNKNOWN_TARGET` and `SESSION_CLOSED` both
claim something untrue about a live, open execution that simply has nothing
running. Known and not fixed: **no client can infer finality from the status
alone** — `turn_dropped` is terminal for `QUEUE_FULL`, `QUEUE_FULL_TURN_KEPT`
and `NO_LIVE_EXECUTION`, with nothing in the status saying so, and a future
non-terminal drop code would be indistinguishable. That is a wire-shape
question for a later slice; no key is added for it here.

**Publish points** (provider-side):

- `turn_queued` — when a `TurnDecision::Start` is accepted into the session's
  mailbox.
- `turn_degraded` — when a `deliver: "steer"` command has been accepted into
  the mailbox of an execution that cannot take a mid-turn steer, immediately
  before that command's `turn_queued`. Never before the delivery is known to
  have succeeded: a degrade in front of a delivery that then fails publishes
  two receipts contradicting each other about one command. Its `message` MUST
  be a function of the command, not of what a particular process learned at
  `initialize` — a redelivery answered by a different process must not publish
  a second payload under the same `(commandId, turn_degraded)` semantic key.
- `turn_dropped` — when the mailbox itself is full (`QueueFull`), when the
  in-actor turn queue overflows (`SESSION_QUEUE_DEPTH`), when an
  interrupt-class turn cannot have both of its sends
  (`QUEUE_FULL_TURN_KEPT`), or when the addressed session has no live
  execution to deliver into (`NO_LIVE_EXECUTION`).

  An **interrupt-class turn is two sends into one bounded mailbox** — the
  cancel, then the turn that replaces what was cancelled — and a provider MUST
  check that both fit before issuing the cancel. Cancelling and then failing to
  deliver destroys work the sender did not ask to lose *and* loses the words
  meant to replace it. When there is not room for both, nothing is cancelled
  and the command is dropped as `QUEUE_FULL_TURN_KEPT`. The
  queue-overflow case already publishes a `turn_dropped` transcript item, and
  this receipt is additive to that item, not a replacement for it. The
  `NO_LIVE_EXECUTION` case must not *consume* the command — dropping a turn and
  marking it delivered is the silent loss this contract exists to remove — but
  it MUST record it as refused. Those are two ledgers answering two questions:
  "did it run" (no) and "has it been answered" (yes, terminally). Recording the
  refusal is what stops a relay redelivery republishing a byte-identical
  `turn_dropped`, and what lets the channel watermark move past a command
  nothing is waiting on. **It is not redeliverable**: see the `turn_dropped`
  status above — a resume mints a new generation the replayed command no longer
  addresses, so the sender has to send it again.
- `turn_started` — when the run loop actually begins the turn, carrying the
  `turnId` the provider mints for it. **This is also the point the command is
  consumed** — never on receipt — so a turn that was accepted and never started
  is still unconsumed when the process dies, and a restart replays every such
  command from its watermark in `(created_at, id)` order and answers each one
  exactly once. *Answers*, not runs: whether a replayed turn runs depends on
  what is live when it arrives, and after a crash the executions died with the
  process, so the replay is typically answered `turn_dropped` /
  `NO_LIVE_EXECUTION` (above). What consume-at-start guarantees is that no
  accepted turn disappears without an answer — not that every accepted turn
  eventually runs.
- `interrupt_delivered` — when a `thread.turn.interrupt` caused a cancel to be
  issued to a turn the provider is running or has taken custody of. A provider
  answers this from what it holds, not from a lagging fold of its own session
  reports: a turn and an interrupt sent back to back must not be answered
  "nothing to cancel" by a cancel that in fact lands.
- `turn_refused` with `NO_TURN_IN_FLIGHT`, `NO_LIVE_EXECUTION`, or
  `QUEUE_FULL` — the three ways a `thread.turn.interrupt` finds nothing to
  cancel or cannot be handed over. All three are terminal and recorded as
  refused.
- `turn_refused` — for every `TurnDecision::Fail` (`UNAUTHORIZED_OPERATOR`),
  and for a `TurnDecision::Ignore` whose reason names a target this provider
  owns: `UnknownTarget`, `StaleGeneration`, `SessionClosed`. An `Ignore` for
  `NotAddressed`, `AlreadyConsumed`, `PastHorizon`, or a malformed command
  stays silent — no receipt — because those reasons say the command was never
  this provider's to answer, and publishing one would be cross-provider
  chatter about someone else's command.

A provider that refuses a command records that command id durably alongside
its consumed ids (pruned at the same horizon), so a relay redelivery of the
same 44220 does not republish a byte-identical `turn_refused` under the same
semantic key.

**A turn receipt never creates, confirms, or ends a generation.** Only the six
lifecycle statuses do that (`created`, `created_with_failed_initial_turn`,
`failed`, `resumed`, `resumed_without_context`, `stopped`). Any fold that reads
`kind:44224` by `commandId` to decide whether a generation exists, is
confirmed, or has ended — the catalog, create observations, `bee sessions`
generation resolution — MUST ignore every turn status for that purpose. A turn receipt is evidence
about one turn, never about the generation's existence or lifecycle.

The pending-turn UI settles a queued command on the `user_prompt` transcript
echo whose `commandId` equals the pending command's `commandId` (see
[NIP-CST](NIP-CST.md)), not on `turn_started` alone — the receipt says the
provider began a turn; the echo says what it began. `turn_queued` updates a
pending row to say the provider has queued it, and exempts that row from the
client's unanswered-row expiry — a turn queued behind an hour of work is still
coming, and the row ages visibly instead of vanishing. `turn_degraded`
relabels the row to say the steer was downgraded to a boundary delivery and
leaves the words sent. `turn_dropped` and `turn_refused` remove the row,
restore the draft, and surface `error.code`/`error.message`.

The ACP session id used as a resume cursor is sensitive host-local state. It
MUST NOT appear in commands, receipts, metadata, transcripts, adapter
environment variables, or logs. Provider state containing it MUST be
owner-readable only on platforms that expose filesystem permissions.

Both kinds are provider-authored, so the relay applies scope, `h` scope, strict
membership, and a size cap (32 KiB metadata, 16 KiB receipt) and nothing more.
It does not parse the **durable 44223/44224 provider-fact content**: that content
is the provider's account of what its own session did, and a relay that
validated those observations would be asserting authority over facts it never
observed. Consumers verify signature, trusted signer, channel visibility,
exact tags, target, and semantic key at their own ingress boundary. This rule
does not apply to kind-24223 leases: their deliberately narrow liveness
envelope is parsed and authority-checked by the relay before it updates the
ephemeral register.

Provider outboxes fence each publication by semantic key, current provider
signing pubkey, and exact event kind, so a signing-key rotation cannot reuse an
event signed by the previous key.

## Ephemeral generation leases

Kind `24223` (`KIND_CODING_SESSION_LEASE`) is a provider-signed, channel-scoped
ephemeral assertion about one exact generation. It is not durable session state
and does not prove human attention, current code-area conflict, or continuous
transport connectivity. A current `live` lease proves only that the authorized
provider owned a live actor when it most recently renewed.

Strict content has exactly four fields, with no unknown or duplicate keys:

```json
{
  "schema": "buzz-coding-session-lease/v1",
  "target": {
    "driver": "codex-acp",
    "instanceId": "provider-instance",
    "sessionId": "provider-session-id",
    "generation": 1
  },
  "state": "live",
  "leaseSequence": 42
}
```

`state` is exactly `live` or `released`. `leaseSequence` is a positive
JavaScript-safe integer, monotonically reserved and persisted before signing.
Gaps are permitted; reuse is forbidden. Signed content is capped at 2 KiB;
target and lifecycle-command identifiers retain the 256-byte NIP-CSC limit.

Tags are closed, exactly two fields each, and appear in this exact order:

```text
["h", "<canonical channel UUID>"]
["cslease-v", "cslease1-1"]
["cs-target", "<coding_session_target_key(target)>"]
["csl-command", "<commandId that minted this exact generation>"]
["cslease-seq", "<canonical decimal leaseSequence>"]
```

The relay rejects malformed content, tag/content target or sequence mismatch,
non-canonical values, an event more than 180 seconds old on first acceptance,
or a timestamp more than 30 seconds ahead of relay time. These timestamp rules
are replay admission only. The 180-second Redis TTL starts from the relay's
acceptance time (Redis `TIME`), never the provider timestamp. Providers renew
eligible `live` actors every 60 seconds and publish a higher-sequence
`released` tombstone before a clean terminal transition.

Lease signing authority comes from the accepted lifecycle chain, never from
metadata authorship. For the tagged channel, `csl-command`, and `cs-target`, the
relay requires exactly one strictly valid accepted kind-44221 create/resume
command and exactly one successful kind-44224 receipt. The receipt target must
equal the lease target, and both the receipt signer and lease signer must equal
the command's `providerAuthorityPubkey`. Missing, conflicting, stop-minted, or
otherwise ambiguous evidence fails closed.

Leases are WebSocket-published only and are never inserted into Postgres. Redis
retains the full original signed event plus relay acceptance/expiry and
command/receipt provenance. Public visibility and the channel expiry index use
the 180-second lease TTL. The per-target monotonic register is retained out of
band for 211 seconds (the 180-second replay window plus 30 seconds of allowed
future skew and a one-second boundary fence), so an expired `released` event
still rejects every delayed lower-sequence `live` event that could pass replay
admission. Higher sequence replaces lower; an exact duplicate is idempotent
without refreshing either lifetime; equal-sequence different-event conflicts
and lower sequences are rejected. Cold queries require explicit channel scope
and return the original provider-signed event only while its public lease is
unexpired.

## Implementation

| Concern | Location |
| --- | --- |
| Kind constants | `crates/buzz-core/src/kind.rs` |
| Payload + `projectRef` / `sessionRef` validation | `crates/buzz-core/src/coding_session_lifecycle_command.rs` |
| Lease payload + strict envelope / replay validation | `crates/buzz-core/src/coding_session_lease.rs` |
| Envelope validation, membership, size caps | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builders | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |
