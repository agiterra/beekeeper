# Recover role evidence from growing project history

Root slice, base985fca952, work/team-role-evidence-astra. Implements the history
recovery part of collaborative plan step1; no new role registry or execution gate.
Fable independently owns durable CI continuation in a separate worktree.

## Outcome

Valid recorded commissioning evidence beyond the first1,000 events is recoverable
without freezing the Roles page. A failed/incomplete scan never earns confirmation.
Local availability, a reported revision, and verified sender identity stay separate.

## Query contract

Keep fetchRolePackProvenanceEvents API and scoped sourceErrors compatible. Reuse
existing relay fetch and per-kind streams, channel chunks at128. The operator
projection extends the four lifecycle streams with44228 and40099 streams. Add bounded
backward pagination: page size1,000, at most8 requests per kind/chunk. Use inclusive
until boundaries and de-duplicate event IDs so events at one timestamp cannot be
skipped. A full same-timestamp boundary with no safe cursor progress is incomplete,
not permission to subtract a second. Reaching request budget remains incomplete.
Short final page proves scan exhaustion under existing relay query semantics.
Preserve already recovered events on failed later pages and scope incompleteness
to the kind and channel chunk that failed. Do not hide competing commands/receipts.
Reject malformed/out-of-range or non-progressing pagination responses with a clear
bounded diagnostic; use request and unique-event bounds. Yield to UI while verifying
cold signatures and between pages; retain exact-byte signature caching.

Do not add periodic model or browser polling. Requests may run with low bounded
concurrency if useful, but dependent pages must be sequential. No new module-level
community cache. Keep the current query key and invalidation contract unless root
finds a specific missing invalidation, and preserve paused/refetch withdrawal.

## Ownership

History worker: rolePackProvenanceQuery.ts, corresponding test.mjs, optional new
Roles-local pagination helper/tests. No other files, no commits.
Root: hook integration, existing rolePackProvenance tests or new integration tests,
focused browser acceptance, ledger and finalizer commits.
Independent authority audit: draft a separate implementation contract only; do
not project current operator grants backward in time or weaken founder proof.

## Acceptance

A valid lifecycle chain older than1,000 records becomes confirmed after complete
history recovery. A competing older command remains disputed. Page2 failure,
request-budget exhaustion, repeated/stalled pages and saturated timestamp boundary
remain incomplete; another channel chunk can still confirm. Duplicate overlap
across inclusive boundary does not duplicate model input. Signature verification
still checks changed signed bytes and yields during cold histories. Existing
community/paused-refetch/project-isolation tests remain green. Final desktop suite
and rebuilt focused browser spec precede packaging; no full landing claim without
repository landing checks.

## Recorded authority contract

A current operator roster cannot establish historical commissioning. Lifecycle commands carry no accepted authority-head binding.
A later grant must not authorize an older command and a later revoke must not erase
previously valid commissioning. ROLE_OPERATOR_COMMISSIONING_SPEC.md defines parity with the provider’s existing
recorded-timeline policy; it does not claim immutable real-time admission. This limits a confirmation badge; it does not block
agents from executing under their actual standing grants.
