# Headless execution and project hooks

Beekeeper's relay holds shared identities, definitions, grants and work records.
The host owns local process supervision. Its session-provider worker owns
runtime sessions, replay state, native cursors, outbox and host-step execution.
The desktop is a management/viewing client; it is not a prerequisite for this
toolchain. This runbook describes the supported first extension path and the
evidence needed to call a setup usable. See [agent-host.md](agent-host.md) for
service registration, custody and local supervision.

## Install and commission

From a checkout with the pinned toolchain, on macOS or Linux:

```sh
. ./bin/activate-hermit
just install-headless
```

`CARGO_INSTALL_ROOT` selects the installation root (default `$HOME/.local`).
Put its `bin` directory on PATH. The recipe installs `bee`, `beekeeper-host`,
`beekeeper-session-provider`, `beekeeper-acp`, `beekeeper-agent`,
`beekeeper-dev-mcp` and `git-credential-nostr`; it uses the existing Cargo
packages and lockfile, without a desktop bundle or GUI resource lookup.
It does not install or authenticate a model runtime. Install the supported ACP
adapter/runtime using its own supported installation path, and authenticate
the selected account. Node is required by the Claude/Codex npm adapters; Python
is a prerequisite of the example hook below, not every Beekeeper workflow.

Commission with an authorized owner credential supplied through the existing
`BEEKEEPER_PRIVATE_KEY` input; do not put its value in command arguments, logs
or evidence. The service home is a new absolute directory, separate from a
desktop installation:

```sh
bee --relay "$TEST_RELAY" host commission \
  --home "$SERVICE_HOME" --runtime claude \
  --adapter "$CLAUDE_ACP_BINARY" --provider-command "$PROVIDER_BINARY"
BEEKEEPER_HOST_HOME="$SERVICE_HOME" beekeeper-host check
BEEKEEPER_HOST_HOME="$SERVICE_HOME" beekeeper-host run
```

`--runtime` supports `claude`, `codex` or `goose`; `--model` selects an explicit
model when supported. Use absolute adapter/provider paths. The commissioned
host resolves its own restricted credentials; it must not depend on reading
the owner's key or Keychain later. Do not launch a second host over the same
state. A successful file write, binary lookup, healthy process or relay socket
alone does not prove the selected model/account can perform a turn. Record a
real successful runtime turn separately. Missing dependencies and unknown
model readiness must remain visible.

Register a service only after checking the commissioned setup. Existing
`bee host install` supports macOS login registration and Linux systemd user
registration; servers need `loginctl enable-linger "$USER"` to survive logout.
Headless Macs can use the existing system LaunchDaemon path with an explicit
persona account; see [agent-host.md](agent-host.md#on-a-headless-mac). Service
registration retains the commissioned executable paths. Generated macOS login
and Linux user registrations carry the selected `BEEKEEPER_HOST_HOME`; the
existing macOS system LaunchDaemon uses the designated account's home instead.
Commission in that account's home for system registration. Foreground `run`
with an explicit service root was exercised in isolated acceptance; native
service-manager registration of a custom root was not independently exercised.

## Existing capability map

| Capability | Existing implementation | Limit to disclose |
| --- | --- | --- |
| Runtime execution | ACP adapters reused by session-provider | Adapter/account/model configuration must be checked on the executor |
| Native continuation | ACP resume/load and strict native-restore refusal | Runtime/account switch is not seamless continuation; injected memory is not native-cursor proof |
| Durable replay | Provider watermarks, consumed command IDs, durable outbox | Initial enrollment has a bounded replay floor; no indefinite recovery without persisted state |
| Interrupted turns | Provider crash reconciliation | Report uncertainty; do not blindly retry uncertain effects |
| Queued work | Existing boundary queue/turn dispositions and strict restoration | Mailbox is in memory; restart replays unstarted work and attempts strict native restore using persisted runtime state |
| Host supervision | Restart policy, provider state lock, local control socket | Process health differs from relay connectivity and usable runtime |
| Action execution | Workflow requests/claims/results and durable action-step store | No automatic crashed-step takeover or exactly-once arbitrary effects |
| Formal acceptance | Existing review/action/git-ref proof contracts | Software-specific proof types are not universal workflow requirements |

The standalone ACP harness currently configures one MCP server. This limits
that adapter's configuration; the first hook below requires no MCP server and
does not add a plugin framework. Graceful shutdown is not a guaranteed save
hook; SIGKILL cannot invoke one. Readiness, credential custody and execution
boundaries remain executor concerns.

An instance linked to an agent definition uses the authorized owner's current
shared kind30175 head, together with its kind30177 instance revision. Publish
the definition through the existing catalog sharing control before using it on
a separately commissioned executor. A private or missing definition is visibly
blocked; the executor does not read desktop memory or flatten private prompts
into a public instance. Runtime, model or account preferences that disagree
with the accepted execution require a new supported configuration boundary;
publication alone does not establish that they were applied.

Ordinary conversation admission respects the process ownership fence. During a
busy host transition, an authorized addressed request receives a durable,
correlated host-local blocked reply and is consumed; send a new request when
the host is available. It does not create a second queue or promise eventual
execution. When ownership is restored to the legacy client, the host abstains
and records that input as consumed so later adoption cannot replay it.

Strict native restoration also applies to a context created before its first
prompt. If the adapter has not persisted a transcript yet, restart may report
`NATIVE_RESTORE_REJECTED`. Explicitly stop/restart that responsibility to create
a fresh context; the refusal does not silently substitute one.

Management retries retain their operation UUID and exact signed publications.
An authenticated exact-event readback can establish that an older publication
was already accepted without submitting it again outside the relay timestamp
window. An unpublished expired event or a denied readback remains a refusal;
this is not an indefinite publication guarantee.

## Install one event hook

Use a project you own or are authorized to manage, an allowed workflow channel,
and recorded project/agents-repository checkouts on this executor. The worker
must have the existing project execution boundary and required resource grants.
Project actions are authored at the root of the project's **agents repository**
as `actions.yml`; executable code belongs in the project's code repository.
Commit both artifacts and publish the action definition. Uncommitted edits to
the actions file are not installed definitions.

Use the supported local materialization command after publishing the project's
agents source with `bee packs set-source`. It verifies the creator's signed
30624 source, the clone's canonical relay URL, and a fresh fetch of the selected
ref before writing executor-local paths:

```sh
bee host trust-project --project "$PROJECT" --checkout "$PROJECT_CHECKOUT" \
  --agents-repo "$AGENTS_CHECKOUT" --home "$SERVICE_HOME"
```

The source must put `actions.yml` at the repository root for this selected hook
path. Place `git-credential-nostr` beside `bee` and the provider; authenticated
fetches use the explicit caller or provider identity and reset ambient Git
credential helpers. No project path is published in the shared definition.
Successful materialization reports its agents commit and leaves execution
readiness unconfirmed. Add the provider to the workflow channel with
`bee channels add-member`; private project repositories also need the existing
project membership grant. `bee agents create --project` admits its actor and
provider before dispatching the lifecycle request.

Create `scripts/hooks/summarize.py` in the code repository:

```python
import json
import os

# Contract: no stdin; HOOK_OUTPUT_MODE is an executor-supplied prerequisite.
# Output: one JSON object on stdout; errors on stderr/nonzero exit.
if os.environ["HOOK_OUTPUT_MODE"] != "summary":
    raise SystemExit("unsupported hook output mode")
print(json.dumps({"schema": "example-summary/v1", "ok": True}))
```

Create this action in the agents repository:

```yaml
schema: buzz-project-actions/v1
actions:
  - name: summarize-push
    description: Emit the project summary contract after a push.
    trigger:
      on: ref_updated
      ref: refs/heads/hook-demo
    steps:
      - id: summarize
        action: run_on_host
        command: [python3, scripts/hooks/summarize.py]
        working_directory: .
        checkout: triggering_commit
        timeout: 30s
        env_from_host: [HOOK_OUTPUT_MODE]
        capture:
          tail_bytes: 8192
          upload: false
```

The trigger is a committed relay-hosted ref update, not a denied push. Its
`after` commit identifies the executable artifact. The host materializes a
fresh detached worktree at that exact commit and records its pre/post checkout
state. The worker reads committed `actions.yml`, compares its canonical hash
with the requested definition hash, and records the agents-repository commit.
Commands are argv, not injected shell text. These immutable git artifacts are
this example's distribution mechanism, not a requirement that all workflows
be software-delivery workflows.

```sh
bee actions status --project "$PROJECT" --channel "$CHANNEL" --file actions.yml
bee actions publish --project "$PROJECT" --channel "$CHANNEL" --file actions.yml
bee workflows runs --workflow "$WORKFLOW"
bee workflows run-status --run "$RUN"
bee workflows approve --token "$APPROVAL_REF" --allow-future-runs
```

`actions status` predicts publication/trigger authority, not installed runtime
or dependency readiness. A host command requires approval before execution;
`--allow-future-runs` grants future runs of the **exact definition hash**. Edits
re-arm that approval. Publication can require both project authority and channel
elevation. The worker needs channel membership to claim. Permission comes from
those grants and the existing project boundary; installing a script does not
give it the provider key or unrestricted secrets.

Configure `HOOK_OUTPUT_MODE=summary` in the worker's intended service environment
before its next invocation. Use a dedicated nonproduction ref, push it through
the existing authorized git path, then inspect the run and correlated result.
The input contract is the exact argv plus named environment, with no stdin;
the application output contract is `example-summary/v1` JSON captured in the
existing result tail. The platform does not validate this user-defined JSON
schema. Timeout is 30 seconds; **automatic effect retries are disabled**.
Allowed executors are authorized project-serving workers; this first contract
has no named-host/capability selector. Do not claim unsupported placement.

### Dependency refusal, update and rollback

Start the worker without `HOOK_OUTPUT_MODE` and trigger a new invocation.
Existing preparation refuses before claim with `ACTION_HOST_ENV_MISSING`.
Inspect the refusal echo/result and the run; another authorized executor may
still claim an unclaimed request. A missing host-installed executable now
refuses preparation before claim with `ACTION_HOST_EXECUTABLE_MISSING`;
the check respects the declared PATH and execute permissions without running
code. Project-relative programs are artifacts of the bound revision and remain
validated during materialization/spawn, so explicitly verify script installation
too. This preflight does not prove the eventual sandbox launch or account is
usable. Restoring the dependency does not retry that same
recorded request on this worker: trigger a new invocation after the service
environment is repaired. Do not call the earlier attempt ready or successful.

Wait until existing invocations are terminal before updating the hook. Commit
the new script and action, publish, and grant its new definition hash. A new
push invokes the new code revision. Existing invocation records retain their
hash and recorded agents/code commit. The workflow engine currently stops a
resumed run with `definition_changed` if its published definition changed;
it does not snapshot the old executable definition for indefinite resumption.
Updates between terminal invocations preserve that fence.

To roll back, select and verify the earlier script commit and earlier action
definition. Republish the earlier definition at the update boundary and regrant
it if necessary; push a new demo-ref update pointing at the selected earlier
code commit. Verify the resulting request hash and result `agentsCommit`,
`checkout`, `headSha`, exit status and output. Rollback is an explicit invocation
of a previous verified revision, not deletion of history or reuse of an old
operation ID.

### Replay and side-effect limits

The relay assigns `(runId, stepId)` and publishes request 46013. The worker
persists `requestedEventId` before claiming 46022 or spawning. Replay of the
same recorded request does not execute it again. One relay DB claim wins;
losers are named. A result 46023 must correlate to the winning claim and is
echoed as 46014. Durable state handles known exited-but-unpublished results.
An interrupted claimed/running command can be reported `lost_on_restart`:
its external effects may be unknown. No takeover or blind retry is promised.
Terminal local deduplication retention is bounded; relay claim state remains
the authoritative fence for a previously completed request. Application
effects should use their own operation ID/idempotency strategy where possible.

## External participation and proof

The independent [host-step fixture](../conformance/headless-host-step/README.md)
checks signed request/claim/result correlation without our host library or
desktop layout. It documents Nostr auth, scoped subscriptions/backfill and its
unsupported capabilities. It is offline conformance, not live acceptance.

On 2026-10-11, isolated relay acceptance additionally exercised an independent
NIP-98 HTTP participant using `nostr-tools`, with no Beekeeper SDK or host-layout
imports. It read and verified actual signed request/claim/result/relay-echo
records, then claimed an unclaimed request and submitted a truthful refused
result because command execution is unsupported by that client. The Python
fixture verifier accepted this actual exchange. This proves the selected wire
contract and admission; foreign runtime execution and recovery remain unproved.

The same isolated run executed a user hook at code revisions `bca85a54` and
`58f24864`, re-armed approval for its changed action hash, and rolled back to
`bca85a54` after terminal invocations. Each completed with exit 0 and one
recorded effect. Missing executable preparation produced
`ACTION_HOST_EXECUTABLE_MISSING` before any claim. Worker restart and subsequent
request backfill retained one claim and result per completed request. Signed
events, terminal states, commands and limits are in the agents repository under
`plans/archive/2026-10-11-headless-extensibility/hook-acceptance.md`.
These observations do not establish indefinite exactly-once external effects.

For acceptance, use an isolated real relay/Postgres/Redis, distinct authorized
and ungranted identities, a fresh service home, installed worker binaries and a
real model account. Keep the desktop closed while triggering the hook and
record exact SHAs, commands, request/claim/result IDs, grants, runtime results,
dependency refusal, safe update/rollback and recovery dispositions. Observe
that an ungranted client is refused. Never capture secret values. Native
Windows execution requires separate available hardware; Linux install/build
and Mac behavior need their own observed receipts. Packaging and this fixture
alone do not satisfy those stories.

Host-adopted agent deletion retains the existing revocation refusal. No supported
headless delete command is provided in this slice: the desktop refuses deletion
and preserves custody rather than reporting a deletion it cannot safely complete.
Stopping a responsibility and reversible ownership rollback use their existing
verified lifecycle receipts; they do not imply agent revocation or deletion.
