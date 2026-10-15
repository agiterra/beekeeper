# NIP-HS — Host-executed workflow steps

`draft` `optional` `client` `relay`

Six kinds carry one exchange: a workflow step that does not run on the relay
but on an operator's machine. The relay suspends the run and publishes a
**request** (`kind:46013`); a host **claims** it (`kind:46022`), executes the
command it recompiles from the project's own `actions.yml`, and publishes a
**result** (`kind:46023`); the relay records the result, **echoes** it
(`kind:46014`) and resumes the run. Two more kinds govern whether a host step
needs an operator's approval each time: the relay-signed **autorun change**
(`kind:46015`) and the operator-signed **autorun revoke** (`kind:46032`).

Every payload names `buzz-host-step/v1` (46013/46014/46022/46023) or
`buzz-workflow-autorun/v1` (46015/46032) in its `schema` field and its
`schema` tag. The wire structs are
[`crates/beekeeper-core/src/host_step.rs`](../../crates/beekeeper-core/src/host_step.rs)
and
[`crates/beekeeper-core/src/workflow_autorun.rs`](../../crates/beekeeper-core/src/workflow_autorun.rs);
the relay's claim and result rules are
[`crates/beekeeper-relay/src/handlers/host_steps.rs`](../../crates/beekeeper-relay/src/handlers/host_steps.rs).
Where this page and that code disagree, the code is the contract and this page
is the bug.

## The one rule that makes it safe

**The request carries no command.** A `kind:46013` names the workflow, the
step, the project, and the SHA-256 of the canonical definition the relay
holds (`definitionHash`). The host resolves the entry from its own checkout of
the project's agents repository, hashes what it finds, and refuses with
`ACTION_DEFINITION_DRIFT` when the two differ. A relay — or anyone who can
make the relay publish a 46013 — cannot inject a command the repository does
not hold.

A request is authorized by the approval that released it (`approval` on the
request, [NIP-WP](NIP-WP.md)), or by a standing autorun grant for that exact
`(workflowId, definitionHash)` (below). The host still verifies the hash
either way.

## Kinds

| kind | signer | stored | names |
| --- | --- | --- | --- |
| `46013` | the relay (`self`) | yes | `WORKFLOW_HOST_STEP_REQUESTED` |
| `46022` | a host | yes | `HOST_STEP_CLAIM` |
| `46023` | the claiming host (or a refusing one) | yes | `HOST_STEP_RESULT` |
| `46014` | the relay (`self`) | yes | `WORKFLOW_HOST_STEP_EXITED` |
| `46015` | the relay (`self`) | yes | `WORKFLOW_AUTORUN_CHANGED` |
| `46032` | the workflow owner or a project owner | yes | `WORKFLOW_AUTORUN_REVOKE` |

All six are regular stored events scoped to a channel by `h`. A reader
verifies a relay-signed kind against the NIP-11 `self` pubkey it witnessed
through a trusted origin; a host-signed kind is verified as any Nostr event
and judged by the relay's admission, not by its presence on the wire.

## 46013 — request

Tags: `d` = `<runId>:<stepId>`, `h` = channel UUID, `a` = the project's
canonical `30621:<owner>:<id>` coordinate, `schema`. Every host-step kind is
decoded with exact tag agreement (`require_exact_tags`): exactly these tags,
each present once, in any order.

Content (`HostStepRequested`, exact keys, camelCase):

```json
{
  "schema": "buzz-host-step/v1",
  "runId": "<lowercase uuid>",
  "workflowId": "<lowercase uuid>",
  "workflowName": "on-push",
  "stepId": "summarize",
  "stepIndex": 1,
  "definitionHash": "<64 lowercase hex>",
  "stepKind": "run_on_host",
  "channelId": "<lowercase uuid>",
  "project": "30621:<64 hex>:<id>",
  "approval": {"approvalRef": "<64 hex>", "scope": "run"},
  "triggerContext": {},
  "inputs": {},
  "expiresAt": 1791800000
}
```

- `stepKind` is `run_on_host`, `wake_agent` (deliver a brief to an agent's
  execution) or `hire_agent` (seat a role into an agent's umbrella).
- `approval` is present only when an approval released the step; `scope` is
  `run` or `action`.
- `inputs` is the outputs of the run's earlier steps keyed by step id, so a
  `wake_agent` brief's `{{steps.<id>.output.*}}` resolves against it. An
  object, empty when nothing precedes the step.
- `expiresAt` is Unix seconds after which the relay accepts no claim:
  `HOST_STEP_CLAIM_WINDOW_SECS` (24 hours) after suspension.

## 46022 — claim

Tags: `d` = `<runId>:<stepId>`, `e` = the 46013 event id, `h`, `schema`.

```json
{
  "schema": "buzz-host-step/v1",
  "runId": "...", "stepId": "...",
  "requestedEventId": "<64 hex: the 46013>",
  "channelId": "...",
  "host": {"name": "optional display name"},
  "capabilities": ["gpu", "toolchain:rust"]
}
```

- `capabilities` is the list of **capability tags** the claiming host
  declares — `host.json` `capabilities`, which the host launcher exports to
  the provider as `BEEKEEPER_CSP_CAPABILITIES`. Each tag is 1–64 bytes of
  lowercase ASCII letters, digits, `.`, `_`, `-` and `:`, not starting with
  `:`, `.` or `-`; at most 32, no duplicates
  (`beekeeper_core::host_step::is_host_capability_tag`). Tags are free
  strings: there is no registry, and a project agrees with its hosts on the
  words. The key is omitted when the host declares nothing, so an older
  host's claim reads exactly as it did.

### The claim rule

Exactly one claim wins the row. The relay admits a 46022 only when all of
these hold, in this order (`admit`, then the claim handler):

1. the row for `(runId, stepId)` exists (`invalid: unknown host step`);
2. the event's `h` is the workflow's channel and the signer is a member of
   it;
3. `requestedEventId` equals the request event the row records
   (`invalid: claim does not name the recorded request event`);
4. the claim's `capabilities` cover the step's `requires` — every tag a
   `run_on_host` step lists under `requires` in the definition the run was
   created against, matched by exact spelling (`toolchain:rust` does not
   cover `toolchain:rust-nightly`). A claim that falls short is rejected
   `HOST_CAPABILITY_MISSING: the step requires [..] and this host declares
   [..]; missing [..]`, the claim event is stored, and the relay **signs the
   refusal**: one kind:46014 whose `result` is `disposition: refused` with
   `refusal.code: HOST_CAPABILITY_MISSING`, no `claimEventId`, `claimedBy`
   the refused host and `resultEventId` the refused 46022 — the same shape a
   host's own pre-claim refusal takes, so a reader sees one vocabulary. The
   row is untouched and stays open for a host that can run the step. This
   check runs **before** the conditional update, so a host that could never
   run the step is told that, never that it lost a race. A step without
   `requires`, or one that is not `run_on_host`, requires nothing;
5. the row is still open and unexpired. Winning is a single conditional
   `UPDATE` (`beekeeper_db::workflow::claim_host_step`): the first claim to
   land takes the row. A later claim by the **same** host is an accepted
   duplicate (`duplicate: already processed`); a claim by **another** host is
   rejected with `claimed by <64 hex>` naming the winner, so every loser can
   read who executes. A claim after `expiresAt` is rejected `expired`.

A host reads `requires` from its own `actions.yml` when it verifies the
definition, so a host that does not declare what the step requires declines
**before claiming** with a 46023 `refused` carrying the same code,
`HOST_CAPABILITY_MISSING`; the relay's check is the gate for a host that
claims anyway (an older provider, or a foreign client).

The winning claim is answered `{"status":"claimed"}`. A host MUST wait for
that admission before producing any effect: a signed claim alone proves
nothing about having won.

## 46023 — result

Tags: `d`, `e` = the 46013 event id, `h`, `schema` (same shape as the claim).

```json
{
  "schema": "buzz-host-step/v1",
  "runId": "...", "stepId": "...",
  "requestedEventId": "<64 hex>",
  "claimEventId": "<64 hex: this host's accepted 46022>",
  "channelId": "...",
  "disposition": "exited",
  "exitCode": 0,
  "timedOut": false,
  "durationMs": 1234,
  "headSha": "<40 hex>",
  "agentsCommit": "<40 hex>",
  "dirty": true,
  "checkout": {"mode": "commit <40 hex>", "sha": "<40 hex>", "headShaBefore": "<40 hex>", "dirtyBefore": false},
  "stdoutTail": "...", "stderrTail": "...", "truncated": false,
  "artifactPath": "/host-local/path",
  "artifacts": [{"name": "stdout.log", "url": "...", "sha256": "<64 hex>", "bytes": 10}]
}
```

### Dispositions

`disposition` is one of exactly four words:

| disposition | meaning | `exitCode` | `refusal` |
| --- | --- | --- | --- |
| `exited` | the command ran to completion | its status | absent |
| `timed_out` | the command exceeded its timeout and was killed | `124` | absent |
| `lost_on_restart` | the host restarted while the command ran and cannot say how it ended | absent | absent |
| `refused` | the host declined to run the step | absent | required: `{code, message}` |

A `lost_on_restart` result reports uncertainty; nothing may synthesize
success from it. `refusal.code` is a stable string the host chooses — the
provider's codes today are `ACTION_DEFINITION_DRIFT`,
`ACTION_HOST_EXECUTABLE_MISSING`, `ACTION_CHECKOUT_NOT_RECORDED`,
`ACTION_AGENTS_REPO_NOT_RECORDED`, `ACTION_AGENTS_FETCH_FAILED`,
`ACTION_NO_TRIGGER_COMMIT`, `ACTION_UNKNOWN` and the other `ACTION_*`
constants in `crates/beekeeper-session-provider`, and
`HOST_CAPABILITY_MISSING` (the step's `requires` is not covered by what the
host declares; the relay signs the same code on a kind:46014 when a host
claims anyway — § The claim rule, step 4) — but the field is an open string
on the wire, not a closed vocabulary, and a foreign host may use its own (the
foreign conformance client answers `FOREIGN_EXECUTION_UNSUPPORTED`).

Bounds the relay enforces: `stdoutTail`/`stderrTail` ≤ 65 536 bytes each, at
most 8 artifacts, SHAs lowercase 40-hex, event ids lowercase 64-hex, a
refusal exactly when the disposition is `refused`, a `routed` block never on a
refusal, and `claimEventId` optional **only** on a refusal.

`checkout` says how the working tree was established before the command ran:
`mode` is `working directory as found` or `commit <sha>`; a reader never has
to infer the commit from the post-execution `headSha` alone.

`routed` is present on a `wake_agent`/`hire_agent` result and names the agent,
role, provider session and the 44220 (or 44221 `session.hire`) command id the
host minted.

### The result rule

- A result **with** `claimEventId` closes the row only when the signer is the
  claiming host and `claimEventId` is the recorded claim
  (`not the claiming host`,
  `invalid: result does not name the recorded claim event`). The first such
  result wins; a repeat from the same host is an accepted duplicate; the
  relay answers `{"status": "<disposition>", "run_id", "step_id"}`, echoes it
  as 46014, and resumes the run at the next step with the result as the host
  step's output.
- A result **without** `claimEventId` and `disposition: refused` — the host
  declined before claiming, e.g. definition drift — is stored, echoed as a
  46014 so the seat that triggered the run learns it was refused, and answered
  `{"status":"refusal_recorded"}`. The row stays `requested`; another host may
  still claim.

## 46014 — echo

Tags: `d`, `e` = the **result** event id, `h`, `p` = the claiming (or
refusing) host, `schema`. There is no `a` tag: a reader subscribing by project
coordinate must add a separate `#h` filter for 46014.

```json
{
  "schema": "buzz-host-step/v1",
  "result": { ...the accepted 46023 content, verbatim... },
  "claimedBy": "<64 hex host pubkey>",
  "resultEventId": "<64 hex>"
}
```

Correlation, for a reader: `46014.content.result.claimEventId` must equal the
accepted claim's id and `46014.content.resultEventId` the result's id, and the
echo must be signed by the witnessed relay `self`. The offline verifier in
[`conformance/headless-host-step/`](../../conformance/headless-host-step/README.md)
checks exactly this; `conformance/foreign-client/foreign-hook.mjs` does it
against a live relay.

## 46015 / 46032 — autorun

Every `run_on_host` step is gated on approval. A `kind:46030` grant whose
content says `scope: "action"` ([NIP-WP](NIP-WP.md)) also records an
**autorun grant** bound to the workflow's current `definitionHash`; later runs
of that exact definition skip the gate. Editing the action changes the hash
and re-arms approval. A standing grant (a 46030 with `workflow` and
`definitionHash` tags and no run) does the same without parking a run.

The relay records both directions as a relay-signed `kind:46015`:

```json
{
  "schema": "buzz-workflow-autorun/v1",
  "workflowId": "<uuid>",
  "definitionHash": "<64 hex>",
  "change": "granted",
  "by": "<64 hex: the operator>",
  "sourceEventId": "<64 hex: the 46030 or 46032>",
  "channelId": "<uuid>"
}
```

Tags: `d` = `workflowId`, `h`, `schema`. `change` is `granted` or `revoked`.

`kind:46032` revokes by hand — content `{schema, workflowId, channelId}`,
same tags. The relay admits it only from the workflow's owner or a project
owner (`forbidden: only the workflow owner or a project owner may revoke
autorun`).

## Reading from outside

```json
["REQ","hooks",
  {"kinds":[46013],"#h":["<channel>"],"#a":["30621:<owner>:<id>"]},
  {"kinds":[46014,46022,46023,46015],"#h":["<channel>"]}]
```

Persist consumed request ids before any effect; on reconnect, backfill with
explicit kinds (the relay refuses filters without `kinds`). A foreign host
that cannot execute MUST still answer a claim it won with a `refused` result
so the row does not sit claimed until expiry.

## What the relay does not check

Signatures and the "is this the witnessed relay" question are the reader's.
The relay does not run the command, does not know `actions.yml`, and does not
verify `definitionHash` against anything a host reports — the host is the
only party that holds the repository, which is the point. It does not verify
that a host *has* the capability it declares, either: `capabilities` is the
host's own word, taken at face value, and the operator who wrote `host.json`
is the one it binds.
