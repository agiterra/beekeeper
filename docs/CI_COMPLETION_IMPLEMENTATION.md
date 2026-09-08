# CI completion — first implementation contract

Astra-owned slice, base 7c5c6e50b. Product intent: VISION_COLLABORATION.md and
COLLABORATIVE_WORKSPACE_PLAN step 2. Status/findings remain SESSION_STATE.md.

## Outcome and boundaries

An existing authorized webhook workflow records a terminal CI fact; an agent
uses `bee ci wait` to suspend until the exact run answers. No model polling,
new daemon, UI, session wake command, role/Packs change or production deployment.

## Shared wire/API contract

New regular relay-signed kind KIND_CI_RESULT = 46008 (currently unused).
Module buzz_core::ci_result. Strict serde structs, deny unknown fields:

- CiResultIdentity: project: String (full30621 coordinate), repository: String
  (full30617 coordinate), commit: String (lowercase40hex), check: String
  (nonempty <=128 UTF8bytes), run: String (nonempty <=256bytes), attempt: u32
  (>0), workflow: String (canonical UUID), phase: CiPhase {Build, Deploy}.
- CiResult: schema: String exactly "buzz-ci-result/v1", identity:
  CiResultIdentity, conclusion: CiConclusion {Success, Failure, Cancelled},
  evidence_url: Option<String> (http/https only <=2048bytes),
  summary: Option<String> (<=4096bytes).
- JSON enums snake_case. Missing optional fields are accepted; canonical
  serialization is stable. Community is derived from the configured relay host
  and server tenant, not supplied by callback or duplicated in the payload.
- Identity correlation digest is SHA256 of serde_json::to_vec(identity) using
  fixed struct field order after strict validation. Same digest in different
  communities remains isolated by existing storage/ACL tenant boundary.
- Tags: exactly one d=digest, a=fullrepository, project=fullproject,
  workflow=UUID, schema=buzz-ci-result/v1. Other event tags may be rejected by
  strict decoder. Payload and tags must agree; signature and relay signer
  verification are separate mandatory consumer checks.
- Core functions: validate_identity(&CiResultIdentity)->Result<(),String>;
  correlation_id(&CiResultIdentity)->Result<String,String>;
  build_ci_result(&CiResult)->Result<(Vec<Vec<String>>,String),String>;
  decode_ci_result(&nostr::Event)->Result<CiResult,String>.
  Core decode verifies kind/content/tags, not trusted signer provenance.

Add kind to canonical registry and repo-project read visibility gate. It is
server-only: direct client ingest must reject it. It must not recursively
trigger workflow execution. No use of reserved44234 or repo-ref movement as CI.

## Producer

Workflow action record_ci_result has literal configured project, repository,
check and phase; templated commit, run, attempt, conclusion, evidence_url and
summary. Workflow UUID derives from executing stored workflow, never callback.
Reuse webhook host/secret/enabled/current-owner checks. Additionally resolve the
exact repo announcement, require its exact project back-reference, and require
workflow owner in existing RepositoryFounders when saving and executing.
No extra ci-workflow registry/tag. Match repo owner coordinate, not just d-name.

Use existing ActionSink and RelayActionSink, normal event store/fanout. Atomically
serialize same community+correlation with DB-backed locking/transaction so
concurrent duplicate callbacks cannot both insert. Same full canonical result
returns existing event ID without fanout. Different content for same identity
fails explicitly as ci_result_conflict and preserves existing accepted fact.
Workflow failure/run evidence records refused conflict; a refusal is not a
second accepted CI result. No claim that a returned result foresees future
contradictory reports. A new attempt is a new identity. Recheck authority inside
the actual action boundary; write failure must not announce completion.

## Wait CLI

bee ci wait --project --repo --commit --check --run --attempt --workflow
--phase build|deploy [--timeout SECONDS]. Relay URL is the global configured
community scope. No implicit HEAD, newest pipeline, shortened SHA or source
inference. Absent timeout waits until cancelled. Return structured normalized
result and event_id. Exit0 success, nonzero failure/cancelled; errors use existing
CLI conventions and explicitly distinguish timeout/unconfirmed and conflicts.

Fetch trusted relay self via existing NIP11 method; verify signature/event ID,
relay signer, exact identity and strict wire decode. Authenticate WS with current
CLI keys/auth tag. REQ exact kind+#d, collect replay through EOSE before returning
terminal so already-stored contradictory facts are detected. Empty EOSE keeps
waiting. After EOSE live terminal returns. Reconnect with bounded exponential
backoff and repeat same REQ; overall deadline survives reconnect. Permanent
AUTH/CLOSED refusal must not cause an infinite retry. Output must not claim
exactly-once downstream execution or catch future conflicting callbacks.

## Exclusive implementation lanes

Core lane: crates/buzz-core/src/ci_result.rs and its direct tests, lib.rs/kind.rs
registry+read-gate wiring. Own no relay/CLI/workflow files.
Producer lane: crates/buzz-workflow/ and relay workflow sink/definition auth,
minimal buzz-db atomic event insert helper and tests if needed. Own no core,
CLI, role/Packs/provider files. Before migration/new API beyond this contract,
report concrete necessity to Astra and continue independent work.
Wait lane: crates/buzz-cli/src/commands/ci.rs + tests, commands/mod.rs, lib.rs,
client.rs narrow accessors. Own no other crates or role/Packs files.
Astra owns this spec, ledger, integration, review, final commits. Builders do
not commit. All lanes share this isolated worktree; do not revert others' edits.

## Required targeted evidence

Core: canonical/framing-safe identity changes; strict fields/bounds/tags/phase.
Producer: wrong secret/configured repo/project/owner; revoked authority; foreign
community isolation; signature/storage/fanout; concurrent duplicate and conflict;
DB failure no completion; direct client submission denied; private read denied.
Wait: stored before EOSE; live after; empty EOSE; wrong tuple/signer/phase;
duplicate and contradictory replay; disconnect+authenticated resubscribe;
timeout across backoff; permanent auth refusal. Test real composition with local
isolated relay and Postgres/Redis where needed. Follow repository landing gates.
