# CI continuation recovery — implementation spec (Fable, 2026-09-08 evening)

Base `86e435d05` (`work/team-role-evidence-astra` integration of the four CI continuation
commits plus root's repairs). Worktree `/Users/brian/Projects/beekeeper/review-ci-recovery-fable`,
branch `work/ci-recovery-fable`. Scope: Astra's "next recovery slice" (mailbox, September 8
commute checkpoint). Ownership: `crates/buzz-session-provider/`, the narrow host custody
integration in `desktop/src-tauri/src/managed_agents/actor_seats.rs` (+tests), and
`scripts/ci-continuation-acceptance.sh`; additionally named here before any edit:
`desktop/src-tauri/src/session_provider/supervisor.rs` (one call after each provider spawn)
and `crates/buzz-session-provider/src/session.rs` (strict native-open mode).

## 0. What is wrong today (traced)
- `Provider::recover` (`lib.rs:922-1010`) detaches every open execution and restores no
  actor. A stored CI registration later reaches `report_no_live_execution`
  (`lib.rs:4285-4318`) → `turn_dropped/NO_LIVE_EXECUTION`.
- The only re-open path is the explicit `session.resume` lifecycle command, which always
  mints `generation + 1` (`lib.rs:3138`), resets `next_seq`/`next_lease_sequence`
  (`lib.rs:3320-3325`), and needs its own one-shot seat entry keyed by the resume command
  (`lib.rs:3236-3330`); seat entries are consumed at spawn (`actor_seats.rs:342`).
- The native open helper (`session.rs:1443-1510`) tries `session/resume`, then
  `session/load`, then **falls back to `session/new`** — a fresh conversation.
- The host supervisor (`desktop/src-tauri/src/session_provider/supervisor.rs:306-420`)
  respawns the provider after an exit but stages no custody for the sessions it owned.
- I/O boundaries: `refuse_ci_continuation` writes the refusal ledger before the outbox
  (`ci_continuation.rs:666-689`); `TurnDropped` likewise (`lib.rs:5715-5750`);
  `admit_ci_turn_start` (`ci_continuation.rs:~500-600`) returns `Err` if store retirement
  fails after the durable operation+command claims (denying a turn that is already
  consumed) and its denial paths can return `Err` before `in_flight.remove`.

## 1. Guarantee and limits, stated
- At-most-once admission per exact target and CI digest is unchanged (operation ledger
  first-writer-wins, command ledger, retirement).
- After a provider restart, a `waiting` or `ready` registration whose target can be
  restored natively delivers **exactly one** ACP prompt to the **same generation** with
  the **same seated identity**; sequence and lease counters continue.
- If the target cannot be restored (no cursor, adapter rejects resume/load, driver gone,
  seat custody not re-staged before expiry), the registration ends in a **durable,
  visible refusal naming the obstacle** — never a new conversation, never a silently
  advanced generation, never a model switch, never provider credentials in place of the
  agent.
- A crash after the durable start claim but before the adapter prompt **loses that
  delivery**; it cannot admit a second one. That loss is now visible: the record stays in
  state `claimed` until `TurnStarted`, and startup reconciliation publishes
  `turn_dropped/LOST_AFTER_CLAIM` for a claimed record whose turn never started.

## 2. Strict native restore (provider; lane R)
New module `native_restore.rs` with `Provider::restore_generation(&mut self, session_id,
relay) -> Result<RestoreOutcome>`:
- Preconditions (each failure is a named obstacle, not an error): record open and not
  closed; no live handle; `resume_cursor` present (`NO_RESUME_CURSOR`); driver descriptor
  present (`PROVIDER_UNAVAILABLE`); adapter advertises `session/resume` or `session/load`
  (`NATIVE_RESTORE_UNSUPPORTED`); for a seated record, a seat entry in the actor-seats
  file keyed by the generation's command id (`generation_command_id`, else `command_id`)
  whose pubkey equals `record.actor` (`ACTOR_UNAVAILABLE`, retryable until the CI expiry).
- Opens with the existing `CreateRequest` for the **current** target (no generation bump),
  `resume_cursor`, no rehydration MCP, no briefing, seat identity/post-fence env/skills
  from the seat entry, and a new `strict_native: bool` on `CreateRequest`
  (`session.rs`): in strict mode the open helper tries `session/resume` then
  `session/load` and returns `CreateFailure{code: NATIVE_RESTORE_REJECTED}` instead of
  `session/new`. Non-strict callers are byte-for-byte unchanged.
- On success: `attach` the handle; consume the seat entry; leave `generation`,
  `generation_command_id`, `next_seq`, `next_lease_sequence`, `open_turn` untouched;
  store the adapter's returned cursor if it changed; publish a
  `status_item("session_restored_native")` transcript row (Priority::High), metadata
  `idle`, and the live lease. No lifecycle receipt (no command was issued).
- Trigger: on demand only — in `deliver_ci_continuation` when the target has no live
  handle. Not at boot (no fan-out of adapters for idle sessions). Uses
  `await_with_lease_maintenance` like resume.
- Obstacles: `ACTOR_UNAVAILABLE` (custody not yet re-staged) defers with bounded backoff
  (`note_attempt`), never refuses before expiry; at expiry the terminal record carries the
  last obstacle and the receipt is `turn_refused/ACTOR_UNAVAILABLE`. `NO_RESUME_CURSOR`,
  `NATIVE_RESTORE_UNSUPPORTED`, `NATIVE_RESTORE_REJECTED`, `PROVIDER_UNAVAILABLE` refuse
  immediately and durably (`refuse_ci_continuation`), record kept as terminal.
- Also: a human (unseated) execution restores with the cursor alone.

## 3. Host re-staging using existing custody (host; lane H)
- Provider maintains `seat-requests.json` in its state dir (module `seat_requests.rs`;
  atomic write; **no secrets**): one row per open seated generation
  `{ commandId: <generation command id>, actor, role, projectRef, sessionId, generation,
  packRef }`, rewritten on create, resume, stop/close, and recovery. Lane R writes it;
  lane H only reads it.
- Host: `managed_agents/actor_seats.rs` gains `restage_actor_seats_for_provider(app,
  state, state_dir)`: for each request row with no existing seat under that command id,
  load the managed-agent record (existing keyring hydration), plan the pack exactly as
  `stage_coding_session_actor_seat` does (project 30624 via the existing
  `fetch_project_pack_source`; a failed read → skip that row with a log line, never a
  substitute pack), build the entry with `build_actor_seat_entry`, and write it with
  `stage_actor_seat` under the request's command id. Refuses (skips) an actor that is not
  a managed agent on this computer. Idempotent; logs counts only, never keys.
- `session_provider/supervisor.rs`: after every `spawn_provider_child` (initial start and
  each respawn) call the re-stage once the child is alive (best-effort; errors logged).
  No new IPC, no polling: the supervisor already owns the restart event.
- Boundary: keys stay in the host keyring; the provider receives them only through the
  one-shot seat file it consumes on the restore spawn; membership and authority unchanged.

## 4. Failure-safe dispositions (provider; lane F)
- Ordering: every terminal disposition enqueues its receipt (and transcript item) into
  the crash-safe outbox **before** the refusal-ledger write: `refuse_ci_continuation`,
  `TurnDropped`, `report_no_live_execution`. A ledger failure after a successful enqueue
  leaves the command answerable on replay; the outbox's semantic key dedups the receipt.
- `admit_ci_turn_start`: after the durable operation and command claims succeed, a store
  retirement failure is logged and the actor is **permitted** (`Ok(true)`); the record is
  moved to `claimed` (not removed) and reconciled later. Every denial path removes the
  in-flight entry even when a write fails (guard/`finally` pattern), and returns the
  disposition it durably recorded.
- Store: `RecordState::Claimed { at, turn_pending: true }` set in `admit_ci_turn_start`
  after the claims; `retire_ci_continuation` at `TurnStarted` removes it; startup
  `recover_ci_continuations` publishes `turn_dropped/LOST_AFTER_CLAIM` (new code, doc'd)
  for a claimed record with no started turn, then removes it.
- Fault seams (`cfg(test)`): `StateStore::fault_plan` with `fail_next_refusal_append`,
  `fail_next_command_append`, `fail_next_operation_append`; `Outbox::fail_next_enqueue`;
  `CiContinuationStore::fail_next_write`. Tests prove each boundary.

## 5. Acceptance (lane A2, after R and F)
`scripts/ci-continuation-acceptance.sh` gains two restart scenarios with the real
provider: (a) register (waiting) → `kill -9` provider → supervisor-equivalent restart in
the script → produce the 46008 → exactly one ACP prompt under the **original** target
(generation unchanged, `agentRef`/attribution unchanged, `session/load` observed in the
stub's method log, no `session/new`); (b) result produced and record `ready` → kill before
delivery → restart → one prompt. The bash ACP stub gains `session/load` (returning the
same session) so strict restore is exercised; a run with a stub that rejects `load` must
show `NATIVE_RESTORE_REJECTED` and no `session/new`. Seated restore is proven in provider
tests (test writes the seat entry under the generation command id before restart — the
host's re-stage stand-in), plus the host unit tests for `restage_actor_seats_for_provider`.

## 6. Lanes (strict; lanes never commit)
| Lane | Model | Owns | Order |
| --- | --- | --- | --- |
| F (dispositions) | opus | `ci_continuation.rs` (only `admit_ci_turn_start`, `refuse_ci_continuation`, `retire_ci_continuation`, `recover_ci_continuations`, `settle_silent_delivery`), `ci_continuation_store.rs` (+tests: `Claimed`, `fail_next_write`), `lib.rs` (only `TurnDropped` arm, `report_no_live_execution`), `state.rs`, `publish.rs`, `coding_session_payload` code `LOST_AFTER_CLAIM` **in the provider as a local code** (no core edit), tests in `tests/ci_continuation_tests.rs` (append only) | now |
| H (host) | sonnet | `desktop/src-tauri/src/managed_agents/actor_seats.rs` + `actor_seats_tests.rs`, `desktop/src-tauri/src/session_provider/supervisor.rs` (one hook + test) | now |
| R (restore) | opus | new `native_restore.rs`, `seat_requests.rs` (+tests), `session.rs` (`strict_native`), `lib.rs` (create/resume/stop sites writing seat requests; `deliver_ci_continuation` restore hook in `ci_continuation.rs`), `tests/ci_continuation_restore_tests.rs` (new) | after F |
| A2 (acceptance) | sonnet | `scripts/ci-continuation-acceptance.sh`, the ignored composition test | after R |
| Finalizer | Fable | review, gates, commits with signoff, checkpoint | |

Cargo: `CARGO_TARGET_DIR=/tmp/fable-ci-recovery-target CARGO_BUILD_JOBS=4`; desktop
Tauri crate: `--manifest-path desktop/src-tauri/Cargo.toml` (binaries symlink from the main
checkout may be needed to compile; gitignored). Gates per lane: fmt, clippy all-targets
`-D warnings`, crate tests. No push, deploy or app replacement.

## 7. Validation record (2026-09-08 evening; composition row appended when lane A2 lands)

Logs: `/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/` (`finalREC-*.log`,
lanes `laneF-*`, `laneH-*`, `laneR2-*`).

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy -p buzz-session-provider --all-targets -- -D warnings` | exit 0 |
| `cargo test -p buzz-session-provider` | 684 passed, 0 failed, 2 ignored (composition + one pre-existing) — new: F 10, R 11 + 6 seat-request unit tests, store tests |
| Tauri crate fmt / clippy all-targets | exit 0 / exit 0 |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml actor_seats` / `supervisor` | 34 / 1 passed |
| `just file-size-check` | exit 0 |
| NUL-byte scan of changed files; binary diff rows | clean; 0 |

Accepted lane deviations: custody is checked before adapter capability (capability is
only knowable by spawning, and custody is the retryable obstacle); `Claimed { at }`
without a redundant `turn_pending` flag; `LOST_AFTER_CLAIM` lives beside the other
provider-local code in `ci_continuation.rs`; `mark_claimed` deliberately does not roll
back on a failed write (the ledgers are already spent); `ProviderUnavailable` doubles for
"no open record" with an exact message. `STORE_VERSION` stays 1 (old files remain
readable). No `buzz-core` change.
