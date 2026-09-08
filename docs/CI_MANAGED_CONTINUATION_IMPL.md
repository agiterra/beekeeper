# CI managed continuation — implementation contract (Fable, 2026-09-08)

Refines `docs/CI_MANAGED_CONTINUATION_SPEC.md` (the execution contract) and adopts Sol's
read-only design (`/Users/brian/Projects/beekeeper/review-2026-09-06-validation/ci-continuation-design.md`)
with two deliberate changes: (1) the provider watches results through **one bounded
authenticated relay listener** (stored replay + live) instead of a poll; (2) the
delivered turn **materializes** the verified result, registration and requested
continuation into the prompt, while the operation fence stays on the compact pointer so
duplicates and alternate command ids converge. Worktree
`/Users/brian/Projects/beekeeper/review-ci-managed-continuation-fable`, branch
`work/ci-managed-continuation-fable`, base `985fca952`.

## 0. Guarantee, stated precisely
For one exact target (driver, instanceId, sessionId, generation) and one CI correlation
digest, the provider admits **at most one** continuation turn, durably, across restarts,
duplicate result events, reconnect replays and any number of registration command ids —
enforced by the existing operation ledger. A CI-only actor handshake rechecks admission
and durably claims the operation and command before releasing the adapter prompt.
A crash after the claim but before prompt delivery can lose that execution; replay
must not admit another one. This is not exactly-once delivery or exactly-once model
effects; downstream work still needs ordinary idempotence.

## 1. Wire contract (buzz-core; kind numbers unchanged)

### 1a. Registration = kind 44220, new closed action
```json
{ "schema": "buzz-coding-session-command/v1",
  "commandId": "cic-<64 hex>",
  "target": { "driver": "...", "instanceId": "...", "sessionId": "...", "generation": 3 },
  "action": {
    "type": "thread.turn.continue_on_ci",
    "identity": { "project": "30621:<owner>:<id>", "repository": "30617:<owner>:<id>",
                  "commit": "<40 hex>", "check": "main-validation", "run": "136",
                  "attempt": 1, "workflow": "<uuid>", "phase": "build" },
    "continuation": "<text the agent wants delivered with the result, ≤ 12288 bytes>",
    "expiresAt": 1788800000 } }
```
Rust: `CodingSessionAction::ThreadTurnContinueOnCi { identity: CiResultIdentity,
continuation: String, expires_at: u64 }` (serde tag `thread.turn.continue_on_ci`,
`deny_unknown_fields`). `validate`: `validate_identity`, continuation non-empty and
≤ 12288 bytes, `expires_at > 0`. `commandId` is derived by the CLI:
`"cic-" + sha256("buzz-ci-continuation/v1\0" + channelId + "\0" + correlation_id +
"\0" + cs-target key + "\0" + expiresAt + "\0" + sha256(continuation))`; an exact retry
names the same registration. `coding_session_target_key` unchanged. The pointer
(operation fence text) is exactly `{"operationId":"<correlation digest>","type":"ci_result"}`.

### 1b. New receipt stage and error codes (kind 44224, `LifecycleReceipt` unchanged shape)
`ReceiptStatus::ContinuationRegistered` (`"continuation_registered"`): the provider
validated the command and **durably stored** the pending registration. It never claims
a mailbox turn. `turn_queued`/`turn_started`/`turn_dropped`/`turn_refused` keep their
meanings at delivery. New refusal codes (constants in `coding_session_payload.rs`):
`CI_CONTINUATION_EXPIRED`, `CI_RESULT_CONFLICT`, `CI_RESULT_UNAVAILABLE_OR_HIDDEN`,
`COMMAND_ID_CONFLICT`, `CI_CONTINUATION_STORE_FULL`. Existing codes reused at delivery:
`UNAUTHORIZED_OPERATOR`, `STALE_GENERATION`, `SESSION_CLOSED`, `QUEUE_FULL`,
`DUPLICATE_OPERATION`, `BUDGET_EXHAUSTED`.

### 1c. Delivered turn (materialized context)
When ready and admitted, the provider starts a turn under the **original commandId and
signer** whose continuation payload is this JSON (pretty, deterministic key order).
Existing actor framing for a non-founder operator or a first-turn continuity preamble
may surround this payload; those normal attribution/bootstrap rules still apply:
```json
{ "type": "ci_result", "operationId": "<digest>",
  "registration": { "commandId": "cic-…", "signer": "<hex>", "registeredAt": 0, "expiresAt": 0 },
  "result": { "eventId": "<hex>", "signer": "<relay self hex>", "observedAt": 0,
              "identity": { …8 fields… }, "conclusion": "success|failure|cancelled",
              "evidenceUrl": "…", "summary": "…" },
  "continuation": "<text from the registration>" }
```
Operation fence key = `operation_fence_key(target, pointer)` with the pointer from 1a,
**not** the materialized text (so duplicates and other command ids converge).

## 2. CLI (`crates/buzz-cli`)
- `bee ci continue --channel <uuid> --provider <expected-provider-pubkey> --target <cs-target key | --driver/--instance-id/--session-id/--generation> --project --repository --commit --check --run --attempt --workflow --phase --continuation <text|@file> [--expires-in <secs, default 86400> | --expires-at <absolute unix seconds>] [--ack-timeout <secs, default 60>]`.
  Publishes the 44220 (signed by the caller's key), then waits — bounded HTTP replay
  reads, the same helper `sessions create --wait` uses (`await_delivery`) — for a 44224
  for exactly that commandId and target:
  `continuation_registered` → exit 0 and print `{commandId, target, operationId, expiresAt,
  registeredEventId}`; `turn_refused` → exit 1 with the code; no receipt within
  `--ack-timeout` → exit 5 (`unconfirmed`) printing the commandId and expiresAt.
  Retry with that same absolute `--expires-at` and all other inputs unchanged to
  preserve the ID. Repeating a relative `--expires-in` later changes the ID. The
  caller's turn ends after this bounded handshake; no shell survives.
- `bee ci continuation status --channel <uuid> --provider <expected-provider-pubkey> --target <cs-target key> --command-id <id>`: reads the 44224s for
  that command and prints the latest stage (`registered|queued|started|refused|dropped|
  none`) with the receipt event ids. Read-only. Both receipt readers verify the
  Nostr signature, expected provider signer, exact target, command ID and envelope.
  Conflicting authenticated terminal facts are disclosed rather than hidden by
  event-ID ordering.
- Compact/JSON output like the rest of `ci.rs`; exit codes per `error.rs`.

## 3. Provider (`crates/buzz-session-provider`)
### 3a. Durable store — new `ci_continuation_store.rs`
File `ci-continuations.json` beside the other state files; `atomic_write`; schema v1
`{ version, registrations: [...] }`. One record: `command_id, registration_event_id,
payload_digest (sha256 of the signed content), channel_id, signer, target, identity,
correlation_id, continuation, expires_at, registered_at, state: waiting | ready {
result_event_id, result_signer, result_canonical_json, observed_at } | terminal {
code, at }, attempts: u32, next_check_at`. Caps: 256 total, 32 per channel; a
registration arriving when full is refused before ack with
`CI_CONTINUATION_STORE_FULL`. Records are removed only after the delivered turn's
command ledger entry is durable, or after a terminal refusal is durably recorded in the
refusal ledger; a terminal record is kept (bounded, oldest evicted) so a re-delivered
registration cannot resurrect a refused one.

### 3b. Admission (in `decide_turn` / `on_turn`)
For `ThreadTurnContinueOnCi`: apply the existing checks in the existing order (consumed,
refused, in-flight, horizon, unknown target, stale generation, **authority =
`operator_may_steer`**, closed session) — but do **not** spend or reserve turn budget and
do not touch the operation ledger. Then: `expires_at` must be > now and ≤ `created_at +
command_horizon`; a pending record with the same commandId and same payload digest →
`Ignore` (idempotent); same commandId, different digest → `turn_refused`
`COMMAND_ID_CONFLICT` (first durable record wins); store full → refusal. Persist the
record **before** enqueueing the `continuation_registered` receipt into the outbox
(semantic key = commandId + status). Do not consume the command id in the command ledger
(it is consumed by the CI start handshake of the eventual turn); the store is the
fence against re-registration on replay. A conflicting payload gets an event-specific
refusal without poisoning the original registration’s command refusal ledger. Immediately request one listener check.

### 3c. Result listener — new `ci_result_listener.rs`
One task, started with the provider: `NostrWsConnection::connect_authenticated` with the
provider's keys/auth tag against the relay WS; subscription `{kinds:[46008],
"#d":[pending correlation ids]}`; re-subscribe when the pending set changes; on
(re)connect the REQ replays stored results (EOSE fold per digest exactly like
`ci.rs:164-173`: 0 → pending, 1 canonical → ready, >1 canonical → `CI_RESULT_CONFLICT`);
bounded exponential backoff on disconnect; no per-registration task, no poll loop, no
shell. Each candidate: `event.verify()`, `pubkey == relay self` (from NIP-11 via the
existing `relay_self_from_nip11`/`trusted_relay_self`, cached), `decode_ci_result`,
exact identity byte-compare against the registration. Verified → mark `ready` (persist)
and post an internal event to the main loop (use the loop's existing mpsc plumbing —
`session_events`/select! at `lib.rs:334`; add an `Internal::CiResultReady{command_id}`
arm). Expiry: the main loop's existing tick checks `expires_at`. The provider cannot tell
"hidden from my key" from "CI never reported", so the code says what was observed: at
least one relay answer (EOSE) for the digest during the window with zero rows →
`CI_RESULT_UNAVAILABLE_OR_HIDDEN` ("no CI result for this identity was visible to this
provider's identity before expiry; the project may be private to it, or CI never
reported"); no relay answer for the digest during the whole window →
`CI_CONTINUATION_EXPIRED` ("the registration expired before this provider could check
the relay"). Both are durable (refusal ledger + outbox) and record which case applied.
Results after durable start admission: a later duplicate result does
nothing (debug log); a later conflicting canonical result does nothing to the started
turn (model effects are not undone; warn log); a conflict that arrives while the record
is `ready`, including a queued actor turn not yet admitted, converts it to
`CI_RESULT_CONFLICT` instead of starting.

### 3d. Delivery
On `CiResultReady`: rebuild the `TurnCommand` from the record (original commandId,
signer, target, `text` = materialized JSON of 1c, `operation_key` = pointer) and run it
through the **same** `decide_turn`/`on_turn` path so authority (`operator_may_steer`,
current `granted_operators`), generation, closed, budget, mailbox and the operation
ledger are re-evaluated **now**. Refusals are durable and published with the existing
codes; revoked → `UNAUTHORIZED_OPERATOR` terminal (regrant does not resurrect); moved
generation → `STALE_GENERATION` (never retarget); closed → `SESSION_CLOSED`. Mark the
record `terminal` or remove it after the durable CI start claim. The actor carries
a CI guard through its mailbox and pauses immediately before transcript/prompt delivery.
The provider rechecks expiry, readiness/conflict, exact project, current authority,
generation, closure, budget and operation ownership; only then does it persist the
operation ledger, command ledger, and store retirement and authorize the actor.
A denial or a dropped handshake never calls the adapter. Ordinary turns retain
their existing delivery path.
`TurnCommand` gains an optional `operation_key: Option<String>` used by the fence when
present (existing turns keep `text`).

### 3e. Recovery
On startup after the state lock: load the store; for each `waiting` record the listener's
first REQ replays stored results (covers "result stored before/while down"); for each
`ready` record re-run 3d (covers crash between ready and mailbox; the operation/command
ledgers make it once); reconcile records against the refusal/command/operation ledgers
first (a command id already consumed or refused → drop the record). `Provider::recover`
already synthesizes terminal results for an open turn; unchanged.

### 3f. Private read
Results are relay-signed, carry no `h`, and are private-project gated for the reader.
The source CI project must exactly match the target execution’s recorded project at
registration, when choosing listener subscriptions, and at delivery/start admission. The listener
reads with the provider's own key: public projects and private projects
that explicitly admit the provider identity work; otherwise a hidden result is
indistinguishable from "not finished" and expiry publishes
`CI_RESULT_UNAVAILABLE_OR_HIDDEN`. No delegation, no borrowed credentials, no inference
that the CI signer may steer. Documented in the CLI help and the receipt code.

## 4. Parity (named; minimal)
- Relay: `crates/buzz-relay/src/handlers/ingest.rs:3053-3100` decodes 44220 via the core
  type; the new variant must pass `validate` there — add one ingest test.
- Desktop strict readers must not reject the new action or receipt stage as malformed:
  `desktop/src/shared/coordination/sessionCoordinationStrictJson.ts` (receipt statuses),
  `desktop/src/features/coding-sessions/lib/codingSessionTrustedIngress.ts`
  (`CODING_SESSION_TURN_RECEIPT_STATUSES`), `desktop/src/features/coding-sessions/lib/codingSessionCommand.ts`
  and `codingSessionIngressPayloads.ts` (44220 action decode). Add `continuation_registered`
  as a non-terminal, non-mailbox stage and accept the action as an opaque known type.
  Tests beside each. Nothing else in desktop.
- SDK: `crates/buzz-sdk/src/builders.rs` gets `build_coding_session_ci_continuation`.
- Mobile: the strict receipt decoder recognizes `continuation_registered` as a
  per-turn stage, without resolving a normal pending user turn or changing execution
  lifecycle state. This is decoder compatibility, not a new mobile continuation UI.

## 5. Tests (all named; lanes add to these files)
- core: `crates/buzz-core/src/coding_session_command.rs` tests (decode/validate/limits),
  `coding_session_payload.rs` receipt status round-trip.
- provider: `crates/buzz-session-provider/src/tests/ci_continuation_tests.rs` using the
  existing harness (`spawn_recording_test_relay`, `CollectingSink`, `pump_until_*`; extend
  `test_filter_matches` with `#d`): success/failure/cancelled; stored-before-register;
  live-after-register; restart before result and after ready; duplicate result events;
  two command ids for one identity → one turn; wrong relay signer / bad signature /
  malformed 46008 / each identity field mismatch → no wake; two canonical results →
  `CI_RESULT_CONFLICT`; revoked signer → `UNAUTHORIZED_OPERATOR`, regrant no resurrect;
  generation moved → `STALE_GENERATION`; closed → `SESSION_CLOSED`; expiry →
  `CI_CONTINUATION_EXPIRED`; hidden → `CI_RESULT_UNAVAILABLE_OR_HIDDEN`; store full;
  crash boundaries (persist-before-ack, ready-before-mailbox, mailbox-before-start,
  ledger order) via the harness's restart pattern in `operation_fence_tests.rs`.
- CLI: `crates/buzz-cli/src/commands/ci/tests.rs` (`continue`: registered/refused/
  unconfirmed same-id retry; `status`).
- Composition: `scripts/ci-continuation-acceptance.sh` — local relay (existing
  `_ensure-services`, scratch DB `buzz_ci_continuation_<pid>_<ts>`, Redis DB 14), the
  built `bee`, the provider binary with the minimal agent, a public project: register,
  post a 46008 through the existing workflow producer path, observe
  `continuation_registered` then `turn_started` with the materialized text. Wired as
  `just test-ci-continuation` (ignored tests + script). If the composition cannot be
  completed in this slice, say exactly which step is missing.

## 6. Lanes (strict; lanes never commit)
| Lane | Model | Owns |
| --- | --- | --- |
| K (contract) | opus | `crates/buzz-core/src/coding_session_command.rs`, `crates/buzz-core/src/coding_session_payload.rs` (status + codes), `crates/buzz-sdk/src/builders.rs` (one builder), relay ingest test in `crates/buzz-relay/src/handlers/ingest.rs` tests, the four desktop parity files + their tests |
| P (provider) | opus | `crates/buzz-session-provider/src/{ci_continuation_store.rs, ci_result_listener.rs, ci_continuation.rs}` (new), edits in `commands.rs`, `lib.rs`, `state.rs`, `config.rs`, `tests/ci_continuation_tests.rs` (new), `tests/mod` registration |
| C (CLI) | sonnet | `crates/buzz-cli/src/commands/ci.rs`, `commands/ci/*.rs`, `crates/buzz-cli/src/lib.rs` (command surface), `commands/ci/tests.rs`, `docs/CI_COMPLETION_USAGE.md` (usage section) |
| A (composition) | sonnet, after K/P/C | `scripts/ci-continuation-acceptance.sh`, `justfile` recipe `test-ci-continuation`, ignored composition test file(s) named when written |
| Finalizer | Fable | review against §0–§3, gates, commit(s) with signoff, checkpoint |

Gates per lane: `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -- -D warnings`,
`cargo test -p <crate>`; desktop parity: `pnpm typecheck` + the touched tests. Cargo:
`CARGO_TARGET_DIR=/tmp/fable-ci-continuation-target CARGO_BUILD_JOBS=4`. No push, no
deploy, no production workflow/deployment settings.

## 7. Fable candidate validation record (before root integration)

These are the handoff’s original reported results on Fable’s base. Root integration
results and repairs are recorded in `docs/SESSION_STATE.md`. The original composition
checked transcript materialization; the strengthened integration script additionally
compares actual ACP prompt bytes for its founder-driven fixture.

Logs: `/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/` (`finalCI-*.log`,
`finalCI2-*.log`, lanes `laneK-*`, `laneP-*`, `laneC-*`, `laneA-*`).

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets -- -D warnings` for buzz-core, buzz-sdk, buzz-relay, buzz-cli, buzz-session-provider, buzz-test-client | exit 0 |
| `cargo test -p buzz-core -p buzz-sdk` | 1352 passed, 0 failed |
| `cargo test -p buzz-cli` | 1074 passed (+1 ordering test → 61 in the `ci` filter), 0 failed |
| `cargo test -p buzz-session-provider` | 645 passed, 0 failed, 1 pre-existing ignored (57 new: store 15, listener 14, materialize 5, provider-level 23) |
| `cargo test -p buzz-relay --lib ingest` | 238 passed |
| `just file-size-check` | exit 0 |
| desktop `pnpm typecheck` + the four parity test files | exit 0; 117 passed |
| `just test-ci-continuation` (real relay on a scratch DB + Redis 14, built `bee`, real provider with a bash ACP stub) | exit 0, three consecutive green runs; proves create → register → real webhook-produced 46008 → exactly one `turn_started` whose 44225 prompt is the §1c JSON byte-for-byte → duplicate webhook and a second command id admit nothing (`DUPLICATE_OPERATION`) |
| NUL-byte and binary-diff scans of every changed file | clean |

Not proven by the composition: the private-project read path (§3f, unit-tested only),
a real model behind the adapter. Not run here: `just ci` / `just test` whole-workspace
(root owns the aggregate). Reproducible acceptance command: `just test-ci-continuation`.
