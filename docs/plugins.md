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
memory_bytes     = 67108864         # linear memory per call
fuel             = 4000000000       # instructions per call
deadline_ms      = 5000             # wall clock per call, host reads included
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

`records.query({kinds, until, limit})` has no scope parameter: the host
applies the invocation's project (`a` tag) and the reader's visibility
(`event_visible_to_reader` — a personal to-do op reaches a plugin only when
the caller is its author). Kinds must be granted; each call, record and byte
counts against the invocation's budget, and a spent budget is an error the
plugin sees.

### Drafts

The host checks every draft before returning it: a kind the plugin drafts and
the host validates (for 44248, the same `validate_project_todo_parts` ingest
runs), exactly one `a` tag naming the invocation's project, `created_at`
within an hour behind and 890 s ahead of the host clock (inside ingest's
900 s drift rule), and size caps. One bad draft discards the whole output.

A client then signs and publishes them in order — `bee` (`plugins::execute_drafts`)
and the desktop (`shared/plugins/pluginDrafts.ts`) implement the same rules,
and both re-check kind and scope before signing anything:

- `abort` — stop. If nothing landed yet, the relay's refusal is the error. If
  something did, the write is partial: report the draft's `partialSummary`
  (or a generic line) so a half-applied change never reads as nothing saved
  or as success.
- `skip-group` — report the group failed (using `partialSummary` when part of
  the group landed), skip the rest of it, continue with the next group.

## Planning: `beekeeper.planning/1`

`plugins/planning` keeps a project's issues as the items of its shared NIP-TD
lists (`docs/nips/NIP-TD.md`), folding with the same `beekeeper-nip-td` crate
the relay and `bee todos` use.

| Operation | Effect | Input → result |
| --- | --- | --- |
| `capabilities` | read | → statuses, priorities, `comments: false`, default container, limits, provider label |
| `read` | read | → issues, containers, `truncated`, `unreadable` |
| `activity` | read | `{containerId, issueId}` → one entry per signed op, oldest first |
| `create-issue` | draft | `{title, description, statusId, priorityId, assignee, containerId}` → `{key, containerId, issueId}`; the ids are named before anything is published |
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
first) and `plugins/test-faults` are the replaceability and containment
fixtures. `scripts/planning-plugin-check.sh [--desktop]` runs the relay, CLI
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
- **The deadline counts host reads**, so a slow database can trap a call.
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
HMAC and signing. This crate is that foundation; `SUPPORTED_WORLDS` is where a
second world plugs in. Differences to settle: this registry uses
`[[plugin]] world = "..."` where that plan wrote `[[webhook_plugin]]`, and the
WIT package is `beekeeper:plugin` rather than `beekeeper:webhook`. Unlike a
webhook plugin, an operation provider never causes the relay to sign anything.
