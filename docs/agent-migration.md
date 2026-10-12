# Desktop agent migration and recovery

The upgrade/setup boundary imports local desktop records into the existing
host/session-provider execution path. Remote provider records keep their
provider and settings. A failed import never starts a desktop executor or
converts a remote deployment into a local agent.

Before altering ownership, the importer reserves the original operation and
source configuration in a restricted journal. It captures the scoped identity,
owner attestation and record in `backup-before`, then verifies the manifest's
file hashes. Directory permissions are `0700` and secret files `0600`. Source custody
stays available until admission is verified.

The actual tracked legacy generation must match actor, community, PID, instance
and nonce before graceful drain. A matching clean generation ACK and observed generation exit establish that
the old harness drained its tracked admitted work. They do **not** certify that every message
before its file timestamp executed. Migration replay uses an inclusive capture
boundary and bounded backfill with the existing request/disposition ledger.
The verified selected native artifact metadata supplies query coverage for stopped
installations; its time is not proof an input executed. Only proven correlated
outcomes suppress old work. Uncertain older inputs need
an explicit blocked disposition and reissue, never automatic effect retry.

For a pre-export harness, the offline reader requires a single attributable
startup generation/worker, an exact clean-drain ACK, a full native conversation
cursor in the original log and uniquely selected native artifacts. Supported
Codex artifacts must prove that cursor and their actual workspace from
`session_meta`. Claude artifacts use the explicit configured
`CLAUDE_CONFIG_DIR/projects/<encoded-workspace>/<session-uuid>.jsonl` and
matching session/workspace metadata. For a verified running generation, an
unset Claude override resolves only from that actual child's captured HOME and
the adapter's documented `.claude` default; an explicit override takes precedence.
Stopped imports require preserved explicit configuration-home provenance.
Unsupported path formats, ambiguous
artifacts and missing configured custody are refused before a running owner is
drained. The importer copies and hashes the selected transcript into
`backup-drained` before handing it to strict native resume. A missing PID is
not evidence of a never-started agent. Stopped records with preserved artifacts
use the same offline reader without signalling a process.

The private pair journal lives under
`<host-state>/management/desktop-import/<actor>__<canonical-relay-sha256>/`.
`stage.json` retains the original import. Immutable
`enrollment-inputs/<operation-uuid>.json` files retain each predecessor and
reserved successor before effects; `active-stage.json` is a current-operation
pointer. Retry reuses the exact pending successor, even when a second restart
was interrupted before ownership advanced. Retry resolves the journal matching
the canonical ownership operation. It never mints a replacement identity. A partial
backup resumes only with identical source bytes; a complete manifest cannot be
overwritten with different custody or context.

For repair, first retain the whole pair directory and original installation
artifacts, then inspect the failure and verify the protected manifests. Supply
the **original** actor/community launch log, matching clean-drain ACK and native
transcript/configuration home to the importer. Do not fabricate an ACK, reuse a
PID or edit the ownership journal to bypass a refusal. Missing credentials must
be recovered through their original custodian. Ambiguous generations, truncated
thread roots, interrupted work without a clean boundary and unverified native
formats remain blocked; selecting a fresh context requires an explicit decision
that acknowledges the retained context rather than a silent fallback.

Backups support explicit offline recovery. They are not an in-product
restore-legacy-owner mode and must never run alongside the host owner. Starting
an older binary against live host-owned custody is outside the supported upgrade
path.

Local Stop/Delete require established host custody; incomplete migration is
reported before an empty relay inventory can be mistaken for a stopped legacy
process. Pending desktop Stop retries retain one protected delivery operation.
Authority withdrawal uses the same quiescence fence as explicit Stop; an
interrupted or uncertain child-group shutdown retains context for repair.
Accepted runtime records retain a private generation-bound process-group witness.
After worker death, a missing registry handle proves nothing. A retained
quiescence fact for that exact generation, or signal-free absence of its known
group, permits Stop or native restoration. Verified quiescence stays true if an
unrelated process later reuses the PID; reopening first replaces it with an
uncertain witness. Alive unproved groups, missing provenance and interrupted
reopening stay pending.
Persisted process IDs never authorize signals. Historical records without a
witness retain their context and require explicit quiescence repair.

An initial admission with `CREATE_START_OUTCOME_UNCONFIRMED` and no accepted
session/process witness has no automatic repair through Start or Stop today.
Stop remains pending and retains custody; absence of a session record does not
establish that startup effects never occurred. Retain the exact admission intent,
reserved target and protected native backup. Correcting a runtime prerequisite
does not authorize retrying that uncertain admission or choosing fresh context.
This is an unresolved recovery limitation, rather than a completed migration;
operator inspection alone is not a supported journal-editing procedure.

The existing standalone-definition conversion can change a legacy definition
revision during migration. New imports retain its original owner-signed event
before drain. If an older protected import lacks that event and the relay no
longer stores it, supply the original signed JSON through the supported local
evidence boundary:

```sh
bee agents supply-migration-definition --agent-pubkey <actor> \
  --event-file /restricted/original-definition.json --home /service-home
```

This retains immutable evidence only. Retry the same migration afterward. The
importer accepts only the exact existing definition/MCP format conversion with
unchanged effective settings and custody, paired signed revisions, and
affirmative absence of admission intent under the pair lock. Reserved,
consumed or uncertain startup, retained admission requests, changed settings
or an accepted target block revision repair. Supplying evidence cannot clear
startup uncertainty, replace protected stages or restart a legacy executor.

A retryable quiescence failure is an immutable outcome for that particular Stop
command. Shared management retains its logical operation and cumulative exact
targets, then journals a fresh delivery attempt only after verifying the exact
provider-signed retryable failure. It renews at most once per target per
management invocation. The original Failed receipt is never rewritten as
Stopped; terminal completion requires the renewed attempt's verified receipt.

Shared deletion first fences new work, retains every exact observed execution
target across polls, and waits for verified termination. Disappearing metadata
or membership does not prove termination. A pending remote teardown or missing
terminal evidence preserves custody. Revocation/tombstone completion and local
Keychain cleanup are separate outcomes; a partial failure is reported and can
be retried with the original deletion intent.

This describes implementation boundaries, not acceptance results. Current test
evidence and platform/native-format limits are tracked in the agents repository's
headless/extensibility execution report.
