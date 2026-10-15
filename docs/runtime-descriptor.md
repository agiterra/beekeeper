# The runtime descriptor

A runtime is plugged into a host by a descriptor, not by a build. The host
writes the list of descriptors it offers into `host.json` (`runtimes`) and
hands it to the session provider as one environment variable,
`BEEKEEPER_CSP_RUNTIMES`, a JSON array of these objects. The provider starts
whatever ACP adapter the descriptor names; nothing in it checks the runtime's
name. `claude`, `codex` and `goose` are words with a built-in driver
(`claude-agent-acp`, `codex-acp`, `goose-acp`; `crates/beekeeper-cli/src/commands/host_commission.rs`);
`claude` and `codex` also have a built-in credential list. None is a special
case of the loader.

Definition: `crates/beekeeper-core/src/coding_session_runtime.rs`. Unknown
keys are refused — a key the sidecar does not know is a contract violation,
not an extension point. How a descriptor is minted is
[NIP-HC](nips/NIP-HC.md); the zero-to-reply path that uses one is
[`INTEGRATING.md`](INTEGRATING.md).

## Fields

| Key | Required | Default | Meaning |
| --- | --- | --- | --- |
| `instanceRef` | yes | — | Unique within the host; a `session.create` names it. Convention `<runtime>-primary`. |
| `driver` | yes | — | Slug minted into every session target (`cs-target`) for this runtime, e.g. `acme-acp`. |
| `runtime` | yes | — | Runtime word reported in the catalog (44222) and metadata (44223): `claude`, `codex`, or any other slug. |
| `agentCommand` | yes | — | The ACP adapter executable (absolute path, or a bare name for `PATH` lookup). |
| `agentArgs` | no | `[]` | Adapter argv after the command. |
| `cliEnv` | no | absent | One `{name, value}` pair injected into every adapter spawn (e.g. `CLAUDE_CODE_EXECUTABLE`). |
| `defaultModel` | no | `"default"` | Model advertised as the default. |
| `allowedModels` | no | `[defaultModel]` | Models a create may ask for. |
| `discoverModels` | no | `false` | Whether the provider probes the adapter's live model list at startup. |
| `capabilities` | no | absent | Capability vector. Absent means the v1 vector for the runtime word: Claude's for `claude`, the **baseline** for every other word (start and interrupt true; steer, context, diff, plan false). |
| `steerIdleGuard` | no | absent | The idle guard the installed adapter honours on `_session/steering`; only `promptRequired` is known. |
| `titleModel` | no | absent | Model for the auto-title one-shot; `null` disables titling for this runtime. |
| `credentialEnv` | no | `[]` | The environment variable **names** this runtime accepts from the credential file. Names only. |
| `credentialSource` | no | absent | An opaque operator label for where the credential comes from (`"acme-vault"`). Echoed into session metadata and auth-failure receipts. A label, never a secret or a path. |
| `execution` | no | absent (`"local"`) | `"remote"` declares that the adapter delegates the work to a process Beekeeper does not launch (a runtime's own server). The transcript then discloses the boundary as the adapter's only — see [Execution profile](#execution-profile). `bee host commission --execution remote`. |
| `context` | no | absent (`"beekeeper"`) | `"runtime"` declares that the runtime keeps its own conversation across Beekeeper sessions (e.g. an adapter pinned to one thread of its server). A new session is then never published as `session_fresh`, nor a resume as `session_restarted_without_context`; every open publishes `session_runtime_context` ("may carry earlier context Beekeeper did not provide"), carrying the withheld row's `reason`. A resume Beekeeper could not restore context for still answers `resumed_without_context` / `CONTEXT_NOT_RECOVERED` (Beekeeper recovered nothing), with a message saying the runtime may still hold its own. A `session.rewind` is refused `NOT_RESTORABLE`: the runtime's memory is not Beekeeper's to cut. `bee host commission --context runtime`. |

### Credentials

With `bee host commission --credential-file <path>` the provider reads that
file (owner-only, `{"runtimes": {"<instanceRef>": {"VAR": "value"}}}`) afresh
at every adapter start and injects only the variables the descriptor permits;
every other account variable the host could have inherited is blanked first.
The permitted names are:

- the descriptor's `credentialEnv` when it names any;
- else the built-in list for the runtime word — `claude`:
  `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`;
  `codex`: `OPENAI_API_KEY`, `CODEX_API_KEY`;
- else nothing: the start is refused (see below). An ambient login is never
  used in place of a configured file.

Without a credential file the descriptor's `cliEnv` is all the adapter gets
from the host, whatever the runtime word.

### Execution profile

The driver slug decides how the adapter is confined: `claude-agent-acp` and
`codex-acp` get their vendor profiles; **every other slug runs as a generic
managed adapter** — the project boundary, a private `HOME`, and only the
environment the descriptor and credential file supply. A launch is refused
as unsupported only when its command is empty or the managed-actor
placeholder, never because the slug is unfamiliar.

The boundary itself exists on macOS only. On any other platform
`execution_scope::prepare` returns a disclosed `Legacy` plan
(`reason: "no-backend-for-platform"`,
`crates/beekeeper-session-provider/src/execution_scope.rs`) and the adapter
is spawned without confinement. The environment rule still holds there —
inherited account variables are blanked and only the permitted names are
injected (`runtime_credentials.rs`) — but nothing restricts what the
adapter can read. See [`INTEGRATING.md` § What is not
promised](INTEGRATING.md#what-is-not-promised).

The boundary wraps the adapter's process tree and nothing else. A runtime
whose adapter forwards the work to a server Beekeeper did not start declares
`"execution": "remote"`; its sessions then publish
`execution_boundary_adapter_only` (with the backend as `reason`) instead of
`execution_boundary_enforced`, and none of the isolation rows (Git
credentials withheld, egress proxy only), because those describe the adapter,
not the work. The desktop shows it as **Not sandboxed**. Confine the remote
process yourself; Beekeeper makes no claim about it.

### What the boundary means for an adapter author

- **Ship your own interpreter inside the install root.** On macOS the
  boundary does not let the adapter read your home directory, so a Node (or
  Python, Ruby…) installed under `$HOME` by mise, nvm, volta, asdf or pyenv is
  unreadable to it and the adapter fails to start. Put the runtime the
  adapter needs inside the directory you pass as `--runtime-install-root`
  (and point the adapter's shebang or `--driver` at it), or install it
  system-wide.
- **Never log your environment.** The adapter's environment carries the
  participant's private key (`BEEKEEPER_PRIVATE_KEY`) by design — it is how
  the seat signs as itself — alongside whatever the credential file
  injected. An adapter that prints `env`, dumps `process.env` into a crash
  report, or forwards its environment to a remote server publishes that key.

## Reporting a cancel that did not take

When the operator cancels, Beekeeper sends `session/cancel` and waits for the
held `session/prompt` to answer. Answer it `{"stopReason": "cancelled"}` only
when the work has stopped. If it could not be stopped (it is still pending in
your server), answer the prompt with a JSON-RPC error saying so: the turn is
then published as failed with `errorCode: "CANCEL_NOT_CONFIRMED"` (also when the provider's own shutdown asked for the stop) and your
message (1024 bytes at most, plus an ellipsis when cut) as `detail`, never as cancelled. An adapter that
answers nothing within the drain grace is still reported cancelled.

## Reporting a credential failure

Beekeeper recognises the Claude CLI's own authentication messages, but it
cannot know every runtime's wording. Any adapter can instead **declare** a
credential failure on a failed `session/prompt` (or `session/new`) by
answering with a JSON-RPC error whose `data.reason` is one of three strings:

```json
{"jsonrpc": "2.0", "id": 7,
 "error": {"code": -32000,
           "message": "the upstream rejected the token",
           "data": {"reason": "credential_expired"}}}
```

| `data.reason` | Meaning |
| --- | --- |
| `credential_expired` | The credential was accepted once and has lapsed. |
| `credential_invalid` | A credential was present and refused. |
| `credential_missing` | The runtime found no credential to present. |

Matched exactly; any other value, a non-string, or a `data` that is not an
object is ignored and the failure is published as it was before. A declared
reason fails the turn with the same outcome as the Claude path: the `result`
item (44225) carries `errorCode: "MODEL_AUTH_FAILED"`, `credentialReason`
(the declared string), `credentialSource` (the descriptor's label — omitted,
and the sentence says the credential source is not reported, when the
descriptor has none; never guessed as `ambient-login`), the remedy as
`result`, and your `message` as `detail`. At `session/new` it makes the create
fail `PROVIDER_AUTH_REQUIRED` instead of `PROVIDER_UNAVAILABLE`. Keep
`message` free of secrets: it is published. Transcript items are shrunk to
the 44225 size cap, so a long message is elided, not refused.

## Commissioning

```bash
# A known word keeps its built-in driver, arguments and model discovery.
bee host commission --runtime claude --adapter /opt/beekeeper/bin/claude-agent-acp

# Any other word names the adapter that drives it. The command's file name
# becomes the driver slug (`acme-acp`); --driver-arg and --credential-env
# repeat; --credential-source is the label that is echoed, never a secret.
bee host commission --runtime acme \
  --driver /opt/acme/bin/acme-acp --driver-arg --acp \
  --credential-file /etc/beekeeper/acme-credentials.json \
  --credential-env ACME_TOKEN --credential-source acme-vault
```

Both write `runtimes[]` in `host.json`; `--runtime-install-root` and
`--model` apply as before. Runtime words and driver slugs are lowercase
letters, digits, `-` and `_`, at most 32 bytes.

## What an operator can see refused

Both are published as a `failed` lifecycle receipt (44224) on the create, with
a message that names the fix; no adapter is started.

| Code | When |
| --- | --- |
| `MODEL_AUTH_UNAVAILABLE` | A credential file is configured and is unreadable, not owner-only, has no entry for this `instanceRef`, carries a variable outside the permitted names — or the runtime word has no built-in list and the descriptor names no `credentialEnv` (the message says to add it or commission without a file). |
| `EXECUTION_BOUNDARY_UNAVAILABLE` | The launch has nothing to run: `agentCommand` is empty or the managed-actor placeholder. On macOS the boundary itself can also be refused for the usual reasons (no home, no sandbox backend state). |

## Conformance vector

`crates/beekeeper-session-provider/src/runtime_descriptor_plug_tests.rs`: a
stub ACP adapter `acme-acp` is commissioned by descriptor alone with
`credentialEnv: ["ACME_TOKEN"]`, started under file custody, answers one turn,
and its metadata advertises the baseline vector; the companion test proves the
refusal above for a descriptor that names no variables. Breaking any of the
three legs of the plug fails it.
