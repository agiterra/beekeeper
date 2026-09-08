# CI completion usage

This describes the CI-completion implementation candidate. Deployment and
acceptance status are recorded in SESSION_STATE.md. It does not configure the
production relay or CI service automatically.

A workflow records one immutable terminal result. `bee ci wait` returns when
that exact result is available, including a result stored before the wait began.
An empty query is not completion. The relay URL selects the community.

## Configure the existing webhook workflow

Create a workflow in a channel where its owner can run workflows. The owner
must also be a founder of the exact repository under the existing repository
rules, and the repository must belong to the configured project. The workflow creation response includes its ID and generated webhook secret
(the latter is inside the relay response message). Preserve the secret
in the CI service's secret store; do not put it into project files or logs.

Use this YAML with the two coordinates replaced by real full coordinates:

```yaml
name: Record build result
trigger:
  on: webhook
steps:
  - id: record
    action: record_ci_result
    project: "30621:<project-owner-key>:<project-id>"
    repository: "30617:<repo-owner-key>:<repo-id>"
    check: "main-validation"
    phase: build
    commit: "{{trigger.commit}}"
    run: "{{trigger.run}}"
    attempt: "{{trigger.attempt}}"
    conclusion: "{{trigger.conclusion}}"
    evidence_url: "{{trigger.evidence_url}}"
```

The existing CLI reads YAML from stdin:

```sh
bee workflows create --channel <uuid> --yaml - < path/to/workflow.yaml
```
Configure the CI service to POST normalized terminal fields to the configured relay
`/hooks/<workflow-uuid>` using `X-Webhook-Secret`. The current action consumes
flat normalized fields; it is not an automatic parser for every vendor's native
webhook payload. For example:

```json
{
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "run": "136",
  "attempt": 1,
  "conclusion": "success",
  "evidence_url": "https://ci.example/project/pipeline/136"
}
```

Project, repository, check and phase come from the stored workflow, not the
callback. The workflow ID comes from its actual execution. Successful webhook
admission means a workflow run was accepted; it does not by itself prove the CI
result was recorded. Missing configured callback fields fail the workflow before
any result is recorded. Recording can also fail if the workflow is disabled, its owner
loses authority, its binding changes, or a contradictory result already exists.

## Wait without model polling

```sh
bee ci wait \
  --project '30621:<project-owner-key>:<project-id>' \
  --repo '30617:<repo-owner-key>:<repo-id>' \
  --commit 0123456789abcdef0123456789abcdef01234567 \
  --check main-validation \
  --run 136 \
  --attempt 1 \
  --workflow '<workflow-uuid>' \
  --phase build \
  --timeout 1800
```

The 30-minute example is for a host terminal or a caller that genuinely supports
that process lifetime. Beekeeper's `buzz-dev-mcp` shell currently defaults to
120 seconds and caps requests at 600 seconds, then cleans up the entire process
group; backgrounding with `&` does not preserve the waiter. The ACP/provider
also impose prompt lifetime limits. This slice does not extend those limits or
bridge a CI result to a new coding-session turn.

Keep that one process waiting through the calling tool's supported long-running
execution mechanism. The wait handles authenticated subscription, stored replay
and reconnect in software. It does not launch a model, send a channel message,
or create a new coding-session turn. A tool runner with a shorter timeout must
supply its existing long-running-process mechanism; this command does not extend
the runner's own execution lifetime.

Success prints `{event_id,result}` and exits zero. A recorded failure or
cancellation prints the result and exits nonzero. Timeout explicitly means
unconfirmed, not failed CI. A build result cannot satisfy a deploy wait. A retry
of the external run must name its actual new attempt; do not increment attempt
merely to bypass a conflicting result for an existing run.

Identical callbacks reuse the stored event. Different content for the same
identity is refused, preserving the first accepted fact. A wait checks the
entire stored replay before returning, but cannot anticipate a contradictory
callback that arrives after it exits. Downstream work still needs its ordinary
idempotence; this command does not promise exactly-once effects.

## Continue a managed session after CI

`bee ci wait` above suspends one waiting process; `bee ci continue` instead
registers a turn with the relay and returns immediately, so the caller's own
turn can end while the CI-managed continuation is delivered later, out of
band, by the session provider. See `docs/CI_MANAGED_CONTINUATION_IMPL.md` for
the full contract (§0 the once-admission guarantee, §1 the wire shapes, §2
this CLI surface, §3f the private-read prerequisite below).

**Private-read prerequisite.** The provider reads the recorded CI result with
its own relay key, at delivery time — not at registration time. A public
project's result is visible to it. A private project's result is visible only
if that project has explicitly admitted the provider's identity. Otherwise a
real, hidden result is indistinguishable from "not finished yet" until the
registration expires, and the eventual refusal is
`CI_RESULT_UNAVAILABLE_OR_HIDDEN`. Neither `bee ci continue` nor `bee ci
continuation status` can detect or work around this from the caller's side —
if a registration against a private project keeps expiring, check that the
project admits the provider first.

**Once-admission guarantee.** For one exact target (driver, instanceId,
sessionId, generation) and one CI correlation digest, the provider admits at
most one continuation turn, durably, across restarts, duplicate result
events, reconnect replays, and any number of registration command ids.
`commandId` is derived from the registration's own inputs (channel, CI
identity, target, `--expires-in`, continuation text), so an exact retry of
the identical command reproduces the same `commandId` and names the same
registration rather than minting a second one.

Register a turn to run once the named CI run attempt records a result:

```sh
bee ci continue \
  --channel <uuid> \
  --driver claude-code --instance-id <instance-id> --session-id <session-id> --generation 1 \
  --project '30621:<project-owner-key>:<project-id>' \
  --repository '30617:<repo-owner-key>:<repo-id>' \
  --commit 0123456789abcdef0123456789abcdef01234567 \
  --check main-validation \
  --run 136 \
  --attempt 1 \
  --workflow '<workflow-uuid>' \
  --phase build \
  --continuation 'CI passed; open the PR.' \
  --expires-in 86400 \
  --ack-timeout 60
```

The target may be given as the four flags above, or as one `--target
'coding-session/v1|...'` `cs-target` key in their place. `--continuation` may
also be `@path/to/file` (or `@-` for stdin) instead of literal text.

This publishes the registration and then waits, bounded by `--ack-timeout`,
for the relay's own answer to it — never for the eventual CI-triggered turn,
which can arrive hours later:

- **Registered** (exit 0) — the registration is durably stored. Prints
  `{commandId, target, operationId, expiresAt, registeredEventId,
  receiptEventId}`.
- **Refused** (exit 1) — a synchronous check rejected the registration (for
  example `COMMAND_ID_CONFLICT`: an exact retry's derived `commandId` already
  names a different registration, which means an input other than
  `--continuation`/`--expires-in` changed). Prints `{commandId, target, code,
  message, receiptEventId}`.
- **Unconfirmed** (exit 5) — no answer arrived within `--ack-timeout`. Prints
  `{commandId, target}`. This is not a failure: re-running the identical
  command reproduces the same `commandId`, so the retry names the same
  registration rather than minting a second one.

Read the latest stage of a registration at any later time, without waiting:

```sh
bee ci continuation status --channel <uuid> --command-id <commandId>
```

Read-only, stored replay only, always exit 0 when the read succeeds (exit 2
on relay failure). Prints `{commandId, stage, receiptEventIds, refusalCode}`,
where `stage` is one of `registered`, `queued`, `started`, `refused`,
`dropped`, or `none` — `none` means no receipt has named this `commandId` yet,
which is expected right after registering and is not itself an error.

## Repeat the integration checks

`just test-ci-completion` creates and migrates a throwaway database, runs the
atomic storage and relay callback acceptance cases, then drops that database.
It is included in `just test`; these Postgres tests are intentionally skipped
by the infrastructure-free unit run. Core and CLI subscription tests also run
in the normal workspace unit suite.
