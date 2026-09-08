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

## Repeat the integration checks

`just test-ci-completion` creates and migrates a throwaway database, runs the
atomic storage and relay callback acceptance cases, then drops that database.
It is included in `just test`; these Postgres tests are intentionally skipped
by the infrastructure-free unit run. Core and CLI subscription tests also run
in the normal workspace unit suite.
