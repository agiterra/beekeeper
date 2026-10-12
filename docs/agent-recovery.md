# Clean headless setup and recovery

The headless host is the sole local executor. Desktop and CLI use the same
management implementation; the desktop supplies presentation, signing,
Keychain custody and local bootstrap adapters.

This release makes a clean break from the former all-in-one desktop executor.
Existing desktop agents are not automatically imported, handed off or started.
Their records, credentials, journals and native history remain retained for
reference. Recreate local agents through canonical Create/setup. The recreated
agent starts a fresh native session; old conversations are not resumed by that
operation. Remote deployments keep their provider and settings.

Do not run an old desktop executor alongside a host execution for the same
identity and community. Setup refuses a tracked live or unproved execution
conflict. A missing process handle is not proof of exit. Do not delete or edit
journals, keys or history to bypass a refusal. This boundary does not signal old
processes, convert native artifacts or restore a legacy owner.

Modern host sessions retain their own durable native cursor, runtime state and
execution-scope bindings. Recovery uses the existing strict native restore
path. Missing or incompatible context is reported; it never silently starts a
fresh session as a claimed resume. Credential sources are resolved at the
configured session boundary and stay private to the executor.

Stop and authority withdrawal share a truthful quiescence fence. Exact
observed targets remain cumulative across retries; disappearing metadata or
membership does not establish termination. A missing in-memory handle does not
prove that a child group exited. Unproved shutdown retains context and returns
pending evidence rather than publishing Stopped.

An initial admission with `CREATE_START_OUTCOME_UNCONFIRMED` is retained as
uncertain. A runtime prerequisite correction, missing session record or absent
accepted target does not authorize another execution. Retry the same operation
and inspect its recorded outcome; do not clear its reservation manually.

Shared deletion fences new work and waits for exact terminal evidence before
retiring the requested definition. A newer definition is preserved. Pending
teardown retains custody; relay retirement and local Keychain cleanup are
separate outcomes, and partial completion is reported. Retries retain the
original deletion intent and evidence. Terminal shared deletion retains scoped
executor custody for explicit recovery; it does not claim key erasure. Identity
archiving is a community visibility hint, not a ban or general relay/repository
permission revocation. Managed execution admission rejects the retired identity.

Historical importer and handoff attempts are archived in the agents repository.
Their failed or unperformed proofs are not retroactively successful and are no
longer prerequisites for this clean-start release. Modern recovery, shared
management and fresh-install acceptance remain required. Test results and GUI
or platform limits are recorded separately from these product boundaries.
