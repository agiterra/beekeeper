# Relay plugins

A Beekeeper relay can run **plugins**: WebAssembly components that implement
a product capability's operations in-process, without holding a key, a
secret, a socket or a database handle. Planning is the first. Beekeeper's own
Planning plugin is built, registered and called exactly the way a third
party's would be; the only thing first-party about it is that the relay image
ships it, registered, so it is on without anyone assembling anything.

This page is the contract. Code: `wit/plugin/world.wit` (the interface),
`crates/beekeeper-plugin-host` (the runtime), `crates/beekeeper-relay/src/api/plugins.rs`
(the route), `plugins/planning` (the first-party plugin).

## Who owns what

| Beekeeper core (the host) | A plugin |
| --- | --- |
| Identity: the caller is proven by NIP-98 on every call | What an operation means |
| Tenancy: the community is the request's host; a plugin never names one | Its own input and output JSON |
| Authorization: whether the caller may read or write the scope, decided before plugin code runs, by the same rule ingest applies | Which records it needs, within its grant |
| Reads: the only way in is `records.query`, bound to that caller and scope, with the caller's visibility applied | Which changes a write needs, in what order, and what to say if only part lands |
| Writes: none. A plugin returns **unsigned drafts**; the caller signs each with their own key and publishes it through ordinary ingest | |
| Time, entropy, limits, and every check on what comes back | |

So a plugin cannot let anyone see or change more than they could by
publishing and querying directly, and a member's edit stays that member's
signed event — no plugin and no relay key ever signs on their behalf.

## The interface: `beekeeper:plugin@1`, world `operation-provider`

A component exports `operations.describe() -> manifest` and
`operations.invoke(invocation) -> result<op-output, op-error>`, and may import
only `records.query` and `log` (plus the shared `types`). See
`wit/plugin/world.wit` for every field.

- **Manifest** — name (must equal the registered name), version, the
  operation APIs it implements (e.g. `beekeeper.planning/1`; clients bind to
  an API, never to a plugin name), its operations each marked `read` or
  `draft`, and the kinds it reads and drafts.
- **Invocation** — operation, input JSON, caller pubkey, scope (a canonical
  project coordinate), the access the host granted (`read`/`write`), the
  host's clock, 32 bytes of host entropy.
- **Output** — result JSON and, for a `draft` operation, ordered drafts.
- **Versioning** — a host accepts components built against 1.x. New host
  imports and new optional JSON fields stay within 1.x; a changed or removed
  export is 2.0. (wasmtime resolves semver-compatible imports; a component
  built against a later 1.x than the host has not been tested.)

## Registering a plugin

Registration is deployment configuration: a TOML file named by
`BEEKEEPER_PLUGINS_FILE`, the same for every community the relay serves.

```toml
[[plugin]]
name   = "planning"                 # [a-z0-9-]{1,63}; also its URL segment
world  = "operation-provider@1"
path   = "planning.wasm"            # relative to this file
sha256 = "<64 lowercase hex>"       # required; a mismatch refuses to start
[plugin.grants]
read_kinds  = [44248]
draft_kinds = [44248]
[plugin.limits]                     # all optional; defaults shown
memory_bytes     = 67108864         # linear memory per call, summed over every memory
fuel             = 4000000000       # instructions per call
deadline_ms      = 5000             # wall clock per call; also bounds each host read
max_concurrent   = 8                # more calls at once are refused as busy
max_queries      = 32               # records.query calls per call
max_records      = 20000
max_record_bytes = 33554432
max_input_bytes  = 65536
max_output_bytes = 4194304
max_drafts       = 16
```

At startup the relay reads the file, checks every pin, compiles each
component, refuses any import outside `beekeeper:plugin@1` (so no WASI: no
clock, files, network or environment), calls `describe()`, and checks the
manifest against the grants. **Any failure stops the relay**: an operator who
registered a plugin gets it or is told why not. A plugin that asks to read a
kind the relay cannot scope to a project (today only NIP-TD, 44248), or to
draft a kind the relay has no validator for, is refused the same way.
`BEEKEEPER_PLUGINS_FILE` unset means no plugins; the relay logs that Planning
is unavailable, and clients say so.

Every limit has a hard ceiling (`Limits::HARD_CEILINGS`) a registry cannot
exceed.

## Calling a plugin

Both routes are NIP-98 authenticated, tenant-bound by `Host`, and pass the
same admission, replay and relay-membership checks as `POST /query`.

- `GET /plugins` → `{"plugins":[{name, version, sha256, api, operations, readKinds, draftKinds}]}`
- `POST /plugins/{name}/invoke` with
  `{"operation": "...", "scope": {"project": "30621:<hex>:<d>"}, "input": …}` →
  `{"plugin":{…}, "operation", "access":"read"|"write", "result":…, "drafts":[…]}`

The relay decides access first: a private project admits its roster (writers
write, viewers read), a public one any relay member, an unknown coordinate
nobody (404). A `draft` operation needs write access. Then the call runs on a
blocking thread in a fresh instance.

| Status | `code` | Meaning |
| --- | --- | --- |
| 400 | `bad-request` | Malformed body, or a non-canonical coordinate |
| 401 | — | NIP-98 missing or for another request |
| 403 | `scope-forbidden` / `write-not-allowed` | Cannot read the project / may only read it |
| 404 | `unknown-host` / `unknown-project` / `plugin-not-registered` / `unknown-operation` / `not-found` | |
| 409 | `conflict` | e.g. the target's latest change is stamped too far ahead |
| 413 | `input-too-large` | |
| 422 | `invalid-input` | The plugin refused the input |
| 500 | `plugin-trapped` (with `trap`: `fuel`/`deadline`/`memory`/`trap`), `plugin-internal`, `host-failure` | |
| 501 | `unsupported` | The plugin does not do that (e.g. a read-only provider asked to write) |
| 502 | `contract-violation` | The plugin returned something the contract forbids; its output was discarded whole |
| 503 | `busy` / `host-refused` | At its concurrency limit / a host read failed |

**Why HTTP and not an event kind**: an invocation is a request with a reply
and no lasting fact. The facts are the member-signed events the caller
publishes afterwards through ordinary ingest, which fan out, are stored and
are gated like any other.

### Reads

`records.query({kinds, until, cursor, limit})` has no scope parameter: the host
applies the invocation's project (`a` tag) and the reader's visibility
(`event_visible_to_reader` — a personal to-do op reaches a plugin only when
the caller is its author). Kinds must be granted; each call, record and byte
counts against the invocation's budget, and a spent budget is an error the
plugin sees. Pages follow an opaque `cursor`: the host returns `next` while
older readable records may exist, and a page may come back short — even
empty — with a `next`, because the host skipped records the caller may not
see. A plugin pages by `next`, never by stopping at a short page. Each host
read is bounded by the call's deadline (a guest is interrupted by the epoch;
a thread blocked on the database is not, so the relay times the read out).

### Drafts: the plugin proposes data, the signer's side owns authority

This is the one invariant every plugin world shares, whatever signs in the
end (code: `crates/beekeeper-plugin-host/src/drafts.rs`). A plugin proposes
a draft's **data**. It never decides **authority** — whose scope the event
lands in, which community, whom it routes to or notifies — because nobody
reads a draft before it is signed: the Planning client signs automatically
when the user acts, so a person inspecting a draft is not a backstop.

The host seals every draft before returning it, and one bad draft discards
the whole output (`502 contract-violation`):

1. **Kind.** One the plugin declared and the embedder lets plugins draft —
   today NIP-TD (44248) only.
2. **Caps on the proposal**: at most 64 tags, each of 1–8 values, each value
   ≤ 1 KiB, ≤ 16 KiB of tag values in all; content ≤ 64 KiB.
3. **Scope is constructed.** The host writes the draft's one `a` tag itself
   from the invocation's authorized project. A plugin may leave it out or
   echo it exactly once; any other `a` — another project, a second copy, a
   relay hint — is refused, never replaced. So a signed event can never
   carry two scopes that a first-match and a last-match reader would resolve
   differently. The community is the request's host and is never a tag.
4. **Tags are a per-kind allowlist.** For 44248 a plugin may propose only
   `td-v`, `td-op`, `td-list`, `td-vis`, `td-item`
   (`PROJECT_TODO_DATA_TAG_KEYS`). `p`, `e`, `q`, `h`, `k`, provenance or any
   new key is refused, not passed through.
5. **Caps on the construction**: the same caps again on the sealed tags, so
   adding the scope cannot carry a draft past them (64 proposed tags with no
   `a` fail as 65).
6. **The kind's validator** — for 44248 the same `validate_project_todo_parts`
   ingest runs — over the sealed tags and content: tag and content agree
   (`td-list`/`td-item`/`td-op`/`td-vis` match the content), content keys are
   closed, an assignee is 64 lowercase hex.
7. **References.** The validator reports the people a draft names (an
   assignee) as `InvokeOutcome.people`; the relay logs the count and does
   not gate on it. A 44248 assignee is data: it sets one folded field, and
   grants no access, carries no `p` tag and notifies nobody. So a plugin's
   assignment gets exactly the checks a directly signed one gets from the
   To-Do tab or `bee todos` — a well-formed pubkey, from an author allowed
   to write — and nothing more: an agent associated with a project but not
   on its roster can be assigned through Planning as it can directly, and
   still cannot read a private project's issues. (The roster is not an
   exact read predicate anyway: a NIP-OA agent's relay membership comes
   through its owner, but a project read checks the agent's own pubkey;
   authors read their own ops; personal lists are their author's alone.)
   A to-do op's other references — its list and item ids — are local to the
   project its `a` tag names, because the fold keys lists by that
   coordinate; they cannot reach another project's objects. Free text
   (titles, descriptions) is data, not routing.

   A future kind whose references *bear authority* — a grant, a `p`
   notification, a cross-project link — must be checked where the effect
   happens (ingest, or whatever acts on the event), not by a lookup here
   before signing alone: a directly signed event never passes through the
   plugin route, and a pre-sign check the effect boundary does not repeat
   is a rule only one path obeys.
8. **`created_at`** is a whole number of seconds within
   `DRAFT_PAST_SECS` = 600 s behind and `DRAFT_FUTURE_SECS` = 890 s ahead of
   the checker's clock, both ends inclusive
   (`beekeeper_nip_td::check_draft_created_at`; the host's copy is
   `drafts::check_created_at`, tied to it by a test). Ingest takes ±900 s of
   the relay's clock but judges the event later — after the reply, the
   signing and the publish — so the past edge leaves 300 s for that; a
   later ingest only makes a future stamp safer, so the future edge sits
   just inside 900. A stamp outside the window is refused, never re-stamped:
   `created_at` orders an op against its rivals in the fold, so changing it
   would change what the op means. `group` is 1–64 bytes and
   `partialSummary` ≤ 512 bytes.

**Who signs, exactly.** An operation provider's drafts are signed by the
**member who invoked it, with their own key, in their own client** — never
by the relay, never by the plugin, which holds no key or secret of any kind.
That is why a member's issue edit stays that member's signed event, and it
is not changing: Planning does not move to relay signing. The plugin
proposes data; authority is never the plugin's.

The host also loads a second world whose events the **relay** signs; it is
not this seal — see [The webhook world](#the-webhook-world-webhook-plugin1).

The member's client is the last check, so it seals again rather than
trusting the relay — `bee` (`plugins::seal_draft`) and the desktop
(`shared/plugins/pluginDrafts.ts` `sealDraft`): a kind it signs for plugins,
the same tag allowlist, its own `a` tag written from the project *it* asked
for (a proposed one must echo it exactly, once), the caps before and after,
the same `created_at` window against its own clock (a missing or
non-integer stamp is refused, never read as 0 or as now), and the kind's
validator (the desktop re-derives the tags from the decoded content and
requires an exact match). Only the sealed event is signed. Neither client
repeats a roster check on assignees; there is none to repeat.

Every draft in a reply is sealed and then signed — by `bee` and by the
desktop alike — before the first is published, so a draft the client
refuses, or cannot sign, anywhere in the list means nothing is
published. That rules out a *preventable* partial write; it is not
atomicity. The publishes that follow are separate network writes,
and a relay refusal or a lost answer after the first is reported under the
rules below.

The client then publishes the signed drafts in order — `bee`
(`plugins::execute_drafts`) and the desktop implement the same rules:

- `abort` — stop. If nothing landed yet, the relay's refusal is the error. If
  something did, the write is partial: report the draft's `partialSummary`
  (or a generic line) so a half-applied change never reads as nothing saved
  or as success.
- `skip-group` — report the group failed (using `partialSummary` when part of
  the group landed), skip the rest of it, continue with the next group.
- **unconfirmed** — whatever the policy, a publish with no answer (a timeout,
  a connection closed or stalled mid-publish — anything but an explicit
  `OK false`) may still land. The run stops there and
  reports it as unconfirmed, never as refused, so nobody retries into a
  duplicate. A create still returns its issue key.

Within those checks a client does not narrow drafts to what the operation's
name suggests (a `set-title` could draft any NIP-TD op in the project). The
plugin owns what an operation means — that is the point — so registering a
plugin is trusting it with the caller's *data* writes in that kind and
project. It is never trusting it with scope, routing or anyone's key.

## The webhook world: `webhook-plugin@1`

The same host also loads a second world, `webhook-plugin`
(`wit/plugin/webhook.wit`; host side `crates/beekeeper-plugin-host/src/webhook.rs`
and `src/runtime/webhook.rs`). A webhook plugin turns one vendor delivery
(GitHub first) into at most one application event (kind 50000). It shares the
package, the `[[plugin]]` registry with its sha256 pin, the no-WASI rule and
the per-call fuel/deadline/memory limits with operation providers; it may
import only `log`, never `records`. It differs from this page in more than who
signs, and is not "the same seal, signed by the relay":

- **Fixed order: `authenticate` → host MAC check → `project`.**
  `WebhookCall::authenticate` hands the plugin the headers only; it returns an
  auth plan (scheme, signature, signed prefix, optional vendor timestamp,
  delivery id, vendor event). `WebhookCall::verify` — host code, not plugin
  and not embedder code — takes that plan, the installation's webhook secret
  (passed in by the embedder, never shown to the plugin), the raw body and the
  receive time, and computes HMAC-SHA256 over `signed-prefix || body` with the
  `hmac` crate, comparing with `Mac::verify_slice` (constant time). It also
  re-checks the plan's shape, refuses a receive time of 0 and an empty
  secret, and holds the vendor timestamp to ±300 s of that same receive
  time. Only it builds a `VerifiedDelivery` (private fields; not `Default`,
  `Clone` or `Deserialize`), which owns the body, the plan's vendor event and
  delivery id, and the receive time, and is bound to its plugin and
  installation. `WebhookCall::project` takes it by value, so projecting an
  unverified body does not compile, and the vendor event the plugin sees and
  the event's `created_at` can only be the verified ones.
- **The host owns the time.** There is no member's clock and no plugin clock
  to trust: the event's `created_at` is the receive time the delivery was
  verified at, never a plugin value. Rule 8's window is for operation
  providers' user-signed drafts and is not a webhook rule.
- **The host builds the event.** The plugin proposes an event type (which
  must start with the registry's granted `type_prefix` and a `.`), a schema
  ≥ 1, a JSON object `data` (≤ 64 KiB) and data tags. The host writes the
  installation's channel as `h` (a plugin may only echo it, once), the
  `L`/`l` namespace and type labels and the `{"schema", "data"}` content, holds
  the tag caps on the proposal and on the result, and then runs the
  embedder's kind-50000 validator — ingest's application-event rules
  (`docs/application-events.md`) — on the finished event. Any failure
  refuses the whole delivery's output; nothing is repaired. This is the only
  check: the relay will sign the result with its own key and store it
  through the plan's B1 path, which does not pass through ordinary ingest.
- **Reserved tags.** `RESERVED_TAG_KEYS` is one set for every world and
  kind: `a`, `h`, `p`, `e`, `q`, `k`, `A`, `E`, `K`, `P`, `L`, `l`, and `-`
  (NIP-70 protected), `expiration` (NIP-40), `delegation` (NIP-26), `proxy`
  (NIP-48). A host whose allowlist for any kind names one refuses to load;
  Planning's `td-*` keys name none.
- **Limits.** An auth plan's delivery id is 1–128 printable ASCII bytes, its
  vendor event 1–64 bytes of `[A-Za-z0-9_.:-]`, an HMAC-SHA256 signature
  exactly 32 bytes, a signed prefix ≤ 4 KiB; at most 128 headers and 64 KiB of
  them reach a plugin; settings are a JSON object ≤ 64 KiB; the body cap is
  the smaller of the registry's `max_body_bytes` (≤ 25 MiB) and the
  manifest's. Concurrency has three caps: the relay-wide
  `host.max_concurrent_calls` (shared with operation providers), the
  plugin's `max_concurrent`, and `max_concurrent_per_installation`, which
  must be ≤ both or the registry is refused. The slots are taken around each
  plugin call (`authenticate`, `project`) and freed when it returns — never
  across the embedder's body read or the MAC check — so a slow
  unauthenticated sender cannot starve Planning of relay-wide slots.

**Not built yet: the relay wiring.** Nothing serves a webhook route.
`crates/beekeeper-relay/src/api/plugins.rs` still loads the host without an
application-event validator, so a registry holding a webhook entry makes the
relay refuse to start — it fails closed. `scripts/build-plugins.sh` builds the
`test-webhook` fixture but puts it in neither registry. Secret storage,
installations, delivery-id dedupe, relay signing and B1 storage are the
GitHub plan's work, not this branch's.

## Planning: `beekeeper.planning/1`

`plugins/planning` keeps a project's issues as the items of its shared NIP-TD
lists (`docs/nips/NIP-TD.md`), folding with the same `beekeeper-nip-td` crate
the relay and `bee todos` use.

| Operation | Effect | Input → result |
| --- | --- | --- |
| `capabilities` | read | → statuses, priorities, `comments: false`, default container, limits, provider label |
| `read` | read | → issues, containers, `truncated`, `unreadable`. `truncated` is true when older history was not read: past ten pages, or when the call's query, record or byte budget ran out after the first page. A budget spent before any page, or the call's deadline, is still an error |
| `activity` | read | `{containerId, issueId}` → one entry per signed op, oldest first |
| `create-issue` | draft | `{title, description, statusId, priorityId, assignee, containerId}` → `{key, containerId, issueId}`; the ids are named before anything is published. A null `containerId` means the project's first shared list (the one `read` lists first); a list titled `defaultContainerTitle` is created only when the project has none |
| `set-title`, `set-description`, `set-status`, `set-priority`, `set-assignee` | draft | `{containerId, issueId, …}` |

Rules it owns: a personal or archived list is not an issue container;
`item.done` alone decides done-ness and a status write is always `item.done`
then `item.status`; stamps strictly increase within a call; a create reports
failed fields instead of failing (so a retry never mints a second issue).

Clients: the desktop Planning tab, `bee planning …`, and any agent with `bee`.
`bee plugins list|invoke [--publish]` reaches any plugin generically.

## Building and testing a plugin

```text
just plugins-build     # every plugins/* → target/plugins/*.wasm + registries
just test-plugins      # each plugin's native tests + the host's containment tests
```

A plugin is a Rust crate (its own Cargo workspace) using `wit-bindgen` against
`wit/plugin`, built for `wasm32-unknown-unknown` (no WASI), then packed into a
component with `beekeeper-plugin-pack <core.wasm> <out.wasm>`, which prints
the sha256 to pin. `beekeeper-plugin-run` runs one operation through the real
host over a JSON record file — the desktop's mock Planning specs use it, so
they drive the real component. `plugins/test-planning-fixture` (a second,
independently built `beekeeper.planning/1` provider sharing no code with the
first: it reads a fixed issue and drafts its own hand-written NIP-TD ops on
create, refusing other writes as `unsupported`) and `plugins/test-faults` are
the replaceability and containment fixtures. `scripts/planning-plugin-check.sh [--desktop]` runs the relay, CLI
and live UI gates against a scratch database with two tenants.

## Shipping

- The relay image builds the first-party plugins, bakes `planning.wasm` and
  its pinned `registry.toml` into `/usr/lib/beekeeper/plugins/`, and sets
  `BEEKEEPER_PLUGINS_FILE` there. An operator's own value replaces it.
- `just relay` builds them and defaults `BEEKEEPER_PLUGINS_FILE` to
  `target/plugins/registry.toml`.
- **Upgrading a plugin** without rebuilding the relay: put the new `.wasm`
  beside a registry that pins it, point `BEEKEEPER_PLUGINS_FILE` there, and
  restart. There is no hot reload.

## Limits — what this does not do

- **The UI is not runtime-installable.** The desktop's Planning UI is
  compiled into the app and lazy-loaded with its route. A new relay plugin
  can be installed without touching any client, and `bee plugins invoke`
  reaches it, but a new *UI* for it ships with an app release. A third-party
  provider of `beekeeper.planning/1` gets the existing Planning UI for free;
  a plugin with a new API gets no UI.
- **Core is still required for new record kinds.** A plugin can only read
  kinds the relay can scope to a project and only draft kinds the relay can
  validate; both are NIP-TD today. A plugin with a new record kind needs a
  core change (a kind, an ingest validator, a scoping rule) first.
- **Provider choice is not a project fact.** With several Planning providers
  the desktop shows the first by name and lets a viewer switch their own
  view; nothing is stored, nothing moves, and `bee planning` requires
  `--provider` when there is more than one.
- **Linear does not exist.** It is listed as unavailable; no installation,
  credential custody or outbound client is built.
- **One instance per call.** Each call instantiates the component afresh
  (no state survives a call); compilation happens once, at startup.
- **The deadline counts host reads**: a slow database fails the call
  (`host-refused`) rather than holding it open.
- **A row the database layer cannot decode ends a scan early.**
  `query_events` silently drops a stored row it cannot rebuild, so a page
  that contained one looks like the last page and the read can end without
  `next` or `truncated`. That predates plugins (every reader of
  `query_events` is affected); fixing it means the database layer reporting
  how many rows it fetched.
- **Concurrency is per plugin, not per community.** `max_concurrent` is one
  counter for the whole relay, so one busy community can make others see
  `busy`.
- **The desktop's mock specs grant write** to every call; the gate's
  `scope-forbidden` / `write-not-allowed` paths are proven by `e2e_plugins`
  and the live CLI check, not by the mock.
- **The To-Do tab, the mobile app and `bee todos`** still fold NIP-TD
  themselves; they are the to-do surfaces, not Planning.
- **Relay-mode desktop e2e** calls the route with the dev `X-Pubkey` header,
  as all its relay-mode reads do; real NIP-98 is exercised by
  `e2e_plugins` and `bee`.

## Relation to the GitHub webhook plan

`plans/GITHUB_PLUGIN_SLICE_PLAN.md` (agents repository) plans a
`webhook-plugin` world on the same foundation: in-process wasmtime components,
no WASI, a sha256-pinned registry under `BEEKEEPER_PLUGINS_FILE`, per-call
fuel/deadline/memory, a fresh store per call, and the host owning secrets,
HMAC and signing. This crate is that foundation (Wasmtime 48.0.6, no WASI,
one host for both worlds), and it now carries the host side of that world
([The webhook world](#the-webhook-world-webhook-plugin1)): both worlds are in
`SUPPORTED_WORLDS`, each with its own WIT world, import allowlist, grants and
limits, and the scope-aware seal (`drafts::seal_scoped_tags`, `a` or `h`)
serves both. The relay route, secrets and storage are still that plan's.
Agreed with that plan's owner (2026-10-21): one
shared host, two separate WIT worlds, and the generic `[[plugin]] world = "..."`
registry (with its required sha256 pin) rather than a `[[webhook_plugin]]`
table; this foundation lands first and the webhook adapter follows it. Unlike a
webhook plugin, an operation provider never causes the relay to sign anything.
Whether Planning itself runs unchanged under that work (the same component,
byte for byte) is that work's claim to prove; this branch has not checked it.

Three things an operator or that plan's implementer will meet:

- The registry file is strict. Every table refuses unknown fields, so a
  registry holding a `[[webhook_plugin]]` table makes this relay refuse to
  start. One shape (`[[plugin]]`) serves both worlds, not a parser that
  ignores what it does not know (a relay silently skipping a registered plugin would be a
  control that lies).
- The loader dispatches on `world`: an operation provider and a webhook
  plugin each get their own pre-instance, linker allowlist, `grants` and
  `limits` table, and a field from the other world's table is refused. A
  webhook entry needs an application-event validator the relay does not pass
  yet, so a relay registry with one refuses to start.
- The relay already has a compiled-in Rust webhook surface,
  `crates/beekeeper-relay/src/webhook_plugin.rs` (`/webhooks/<name>`, a
  `WebhookVerifier` trait). That plan's step 8 replaces its handler with a
  WASM one; this branch does not touch it.
