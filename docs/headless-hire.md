# Isolated owner-authenticated hire host

`bee sessions hire-host` answers ordinary `sessions hire` requests through
`beekeeper-agent-management`. It uses an existing commissioned host for
execution. The owner signs only in this management process; the provider and
workers retain their existing scoped credentials. No new wire kind is used.

This first adapter binds one owner, project, channel, genesis, installed actor,
runtime alias and model. Use a fresh isolated identity/project unavailable to
legacy Desktop responders and pass `--isolated-cutover`. This is an explicit
cutover acknowledgment, not distributed arbitration. Do not enable it alongside
legacy Desktop for the same scope. Same-host callers share OS locks and the
management operation journal; mixed old clients and multiple machines are not
fenced.

## Installation and execution

1. Commission the isolated host with `bee host commission`, create the project
   using `bee projects create`, and register its code checkout and canonical
   agents repository using `bee host trust-project`.
2. Run `bee sessions hire-host install-role --operation-id <uuid> --home <home>
   --project <coordinate> --channel <uuid> --role builder --name <name>
   --provider-instance <alias> --model <model> --parallelism 2
   --templates <shipped-template-directory>`.
   Installation creates fresh canonical custody without starting execution.
   Reuse the operation UUID and identical inputs after an uncertain reply.
3. Start the installed identity with the existing `bee agents start` operation,
   naming its returned actor, the same project/channel/runtime/model/name and
   system prompt `Follow the canonical role pack staged for this responsibility.`
   This primary context has no initial model turn. The existing ownership fence
   intentionally refuses hires of stopped identities. Parallelism includes this
   primary context: two allows it plus one hired responsibility.
4. Found an owner-signed umbrella and obtain its provider-created receipt using
   the existing genesis/create surface. Run `bee sessions hire-host run --home
   <home> --project <coordinate> --channel <uuid> --genesis <event-id> --actor
   <installed-actor> --provider-instance <alias> --model <model>
   --policy-file <protected-json> --isolated-cutover`.
5. Publish `bee sessions hire` normally. After receipt-backed grants, use
   `bee sessions assign --wake-to <role>`; the existing software wake delivers
   the canonical brief. Reports must be authored by the worker through its
   actor-scoped CLI.

The policy file must be a regular owner-only file, at most 64 KiB:

```json
{"enabled":true,"allowedRoles":["builder"],"maxSeatsPerUmbrella":2,"allowedProviderInstanceRefs":["acme-primary"]}
```

Without the file, Desktop's defaults apply: enabled, all installed roles,
10 seats, all commissioned providers. Explicit actor/runtime/model selection
still applies. `--once` reconciles one history pass; the normal listener polls
with durable unresolved requests first. This is not a general scheduler.

The installer composes the signed project source using the canonical role
resolver and stages its complete pack, skills, runtime environment and source
provenance in protected configuration. Roles requiring MCP servers or hooks
are explicitly refused by this first installer. Automatic registry routing is
not configured by this adapter: a routing request is refused, never silently
rerouted. Explicit runtime/model choices must match installed role material,
host commissioning and the current signed catalog.

## Recovery and evidence

A SHA256-scoped journal key includes relay, owner, channel, genesis and the
original command ID, including valid non-UUID IDs. Wire command/event IDs are
unchanged. A different signed request reusing an ID conflicts. The journal
persists actor, source, tree and exact signed create before staging; replay
never selects a replacement. Unknown publication or startup keeps its
reservation. Terminal authorized policy/selection refusals are also journaled:
the existing kind:44220 host-answer carries the refusal to the requester, with
exact signed bytes retained through ACK loss. Enabling policy or restarting does
not revive a refused request; a new request is required. Target/transport
uncertainty remains explicitly pending, and unauthorized requests receive no
owner-signed disclosure. A verified terminal refusal releases selection but permanently
answers the old request. Receipt success followed by grant failure is
`created_awaiting_grants`, not ready. Grant repair retains signed attempts and
advances only using accepted authority-chain evidence.

Checkout materialization uses the existing host boundary for repository
filters. A protected preparation record precedes each Git creation step. A known
unlaunched partial code worktree or agents clone can finish its pinned checkout;
HEAD, index and tracked content must agree before publication. Completed trees
are verified, never force-reset. Preexisting trees without preparation evidence
require inspection rather than an automatic reset. Identity and role custody
are staged together under the same short actor-seat lock used by provider
consumption. Declared sandbox seeding uses the shared host seeder and records its
outcome in the worktree record; interrupted seeding is disclosed on recovery,
not blindly rerun. Owner stop/quiescence and responsibility admission continue
to use existing provider contracts. A completed old hire stays answered after
stop; a new responsibility requires a new hire request.

## Deterministic acceptance

Build `beekeeper-cli`, `beekeeper-host`, `beekeeper-session-provider`,
`beekeeper-relay` and `git-credential-nostr`, then run:

```sh
. ./bin/activate-hermit
python3 scripts/headless-hire-check.py
```

The harness extends `scripts/headless-acme-check.sh`: unique containers, ports
(including health/metrics), database, bucket, home and socket; fresh owner and
managed identity; deterministic acme runtime; independent `nostr-tools` signed
readers. It retains binary/source hashes, signed events, dispositions, faults
and cleanup evidence under the printed `/tmp/hire-ab2-*` directory. Those
restricted fixture directories also contain disposable test credentials: do
not publish them wholesale. It removes only its own resources. No real host,
login registration, shared Docker reset or real model is involved. The acme
adapter does not support native resume. Transport acceptance is distinct from
full mission verification and real-model acceptance.
