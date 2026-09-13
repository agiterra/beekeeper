# Native steering — implementation contract (2026-09-11)

Binding for the lanes building the September 11 plan in
`docs/SESSION_STATE.md` ("native steering plan for Claude"). Where this file
and a lane's judgement disagree, this file wins; where this file and
`docs/nips/NIP-CSC.md` / `NIP-CSL.md` disagree about wire shape, the NIPs are
amended by the client/contract lane to match this file in the same change.

Base: `main` at `77b792de9`. Worktree `review-native-steering-fable`, branch
`work/native-steering-fable`.

## 1. Adapter facts (verified 2026-09-11 against the installed packages)

Installed under `~/Library/Application Support/Beekeeper/node-tools/lib/node_modules/@agentclientprotocol/`:

| Adapter | Version | `_meta.steering.supported` | Idle behaviour of `_session/steering` | Verdict for this slice |
| --- | --- | --- | --- | --- |
| `claude-agent-acp` | 0.70.0 | `true` (`dist/acp-agent.js:734-738`) | Default: starts a **detached** new turn and answers `startedNewTurn` (`:1150-1158`). With request `_meta.steering.idleBehavior: "promptRequired"` it answers `{outcome:"promptRequired", reason:"noRunningTurn"}` **without** touching the session (`:1146-1150`). While a turn is in flight it pushes the message into the same SDK input stream and answers `injected` (`:1160-1184`). | **Native steering enabled** with the `promptRequired` idle guard. |
| `codex-acp` | 1.6.2 | `true` (`dist/index.js:30553`) | No idle guard: when no turn is steerable it starts a new turn (`startNewTurnFromSteering`, `:31387-31400`) and answers `startedNewTurn` only once that turn is running, i.e. **after** the previous prompt's response has already been written. Catch-all failures answer `{outcome:"failed"}` (`:31292-31300`). | **Boundary mode.** A steer racing a turn end would start a native turn whose updates and completion Beekeeper's actor does not read. Stays degraded until codex-acp honours an idle guard. |
| `goose acp` | not installed | — | — | Boundary (unchanged). |

`injected` from claude-agent-acp means: the SDK aborts the current generation
cycle, emits its `result`, and runs the steered message as a second cycle
inside the **same** ACP prompt; the prompt response arrives at SDK idle. The
Beekeeper prompt stays open; nothing is cancelled.

Neither adapter advertises whether it honours `idleBehavior`. That fact is
therefore a **declared runtime fact** (`RuntimeDescriptor.steerIdleGuard`,
§3.3), set by the component that installs and pins the adapter, never inferred
from the driver slug inside the provider.

## 2. Outcome names (locked)

| Transport outcome (`buzz_acp::steer::SteerResolution`) | Provider disposition | Wire (44224 receipt under the command's `commandId`) |
| --- | --- | --- |
| `Injected { .. }` | `injected` | `turn_injected` (six keys, `turnId` = the active turn the input joined) preceded by a `user_prompt{steered:true}` transcript item on that turn |
| `StartedNewTurn` | `started_new_turn` | `turn_delivery_unknown` / `STEER_UNOBSERVED_NEW_TURN` — delivered, never resent, output unobserved |
| `NotDelivered { reason }` | `not_delivered` → boundary fallback | `turn_degraded` (code per reason, §4) then `turn_queued`, then the ordinary `turn_started` |
| `Unknown { reason }` | `unknown` (terminal, refused ledger) | `turn_delivery_unknown` (code per reason, §4). Never replayed automatically. |
| Prevented before dispatch (authority/generation/fence) | `prevented` | existing `turn_refused` codes; zero runtime writes |
| Admission channel full (saturation) | `prevented` | `turn_dropped` / `STEER_SATURATED` — terminal like `QUEUE_FULL`; nothing written; the sender sends again. A mailbox fallback would reorder one operator's inputs. |
| Not delivered, but the attempt was fenced (takeover) or its generation superseded after dispatch | `prevented` | `turn_refused` (`HANDOVER_FENCED` / `STALE_GENERATION`); the boundary fallback re-checks authority before it delivers |
| Unresolved intent found at restart | `unknown` | `turn_delivery_unknown` / `STEER_UNRESOLVED_AT_RESTART` |
| Late ACK for an `unknown` attempt: `Injected` | `reconciled_injected` | `user_prompt{steered:true}` echo on the recorded turn, then `turn_injected` |
| Late ACK for an `unknown` attempt: `NotDelivered` | `reconciled_not_delivered` | `turn_dropped` / `STEER_NOT_DELIVERED` (sender resends; no automatic re-queue) |
| Late ACK for an `unknown` attempt: `StartedNewTurn` | `reconciled_new_turn` | ledger + log only — the earlier `turn_delivery_unknown` stands; a second receipt under the same semantic key would be dropped by consumers |

Rules that bind every row:

- Exactly one path owns each command: native attempt, boundary mailbox, or a
  terminal disposition. A command with an open attempt is not re-admitted by
  `decide_turn` (treated as already consumed while open).
- A `turn_injected` never replaces `open_turn`, never resets the translator,
  never charges `record_turn_spend`, never changes team-wake causality.
- A `turn_delivery_unknown` records the command as **refused** (terminal
  answer) so a relay redelivery is silent. The sender decides whether to send
  again.
- `turn_degraded` is published only after the boundary mailbox accepted the
  fallback turn (NIP-CSL publish points), and always followed by `turn_queued`.
- Receipts are keyed to the **attempt** and its target generation, never to
  the newest pending command.

## 3. Interfaces (locked)

### 3.1 `buzz-acp` — public module `buzz_acp::steer` (transport lane)

```rust
pub mod steer;                       // crates/buzz-acp/src/lib.rs

/// One mid-turn input the caller wants delivered into the running turn.
pub struct SteerInput {
    pub attempt_id: String,          // caller-minted; echoed on every outcome
    pub prompt_blocks: Vec<String>,  // each becomes one ACP `text` block
    pub idle_guard: IdleGuard,
    pub write_guard: Option<Arc<dyn SteerWriteGuard>>,
    pub outcome_tx: tokio::sync::oneshot::Sender<SteerResolution>,
}

pub enum IdleGuard { PromptRequired, AdapterDefault }
pub enum SteerWriteRefusal { Fenced, OperatorRevoked, AuthorityUnverified, Unavailable }
pub trait SteerWriteGuard: Send + Sync + std::fmt::Debug {
    fn begin_write(&self) -> Result<(), SteerWriteRefusal>;
}

pub enum SteerWire { AcpExtension, Goose }

pub enum SteerResolution {
    Injected { wire: SteerWire, native_run_id: Option<String> },
    StartedNewTurn { wire: SteerWire },
    NotDelivered { reason: NotDeliveredReason },
    Unknown { reason: UnknownReason, wire_request_id: Option<u64> },
}

pub enum NotDeliveredReason {
    Unsupported,                       // no transport available at write time
    PromptRequired,                    // adapter answered promptRequired
    MethodNotFound { message: String },// JSON-RPC -32601
    Rejected { code: i64, message: String }, // any other JSON-RPC error
    PromptEndedBeforeWrite,            // request never left the channel
    DispatchPrevented { reason: SteerWriteRefusal }, // terminal; no fallback
}

pub enum UnknownReason {
    WriteFailed { message: String },   // write error; bytes may have gone out
    PromptEndedBeforeAck,              // written; prompt finished/errored/timed out first
    AckTimeout,                        // bounded post-prompt drain expired
    UnrecognizedAck { outcome: String }, // success with absent/unknown outcome (`{}`)
    AdapterReportedFailure { outcome: String }, // success with `failed`
    RuntimeExited,                     // EOF after the write
}

pub struct LateSteerAck { pub attempt_id: String, pub resolution: SteerResolution }

impl AcpClient {
    pub fn install_steer_input(&mut self, rx: mpsc::Receiver<SteerInput>);  // panics if one is installed
    pub fn clear_steer_input(&mut self);
    pub fn set_late_steer_sink(&mut self, tx: mpsc::UnboundedSender<LateSteerAck>);
    pub fn unresolved_steer_attempts(&self) -> Vec<String>;   // attempt ids awaiting a late ACK
}

pub const STEER_ACK_DRAIN: Duration = Duration::from_millis(1500);
```

Transport behaviour (read loop of `session_prompt_*`):

1. Inputs are taken one at a time, in order; a second input is not taken
   while one is pending (existing gate).
2. `_session/steering` params carry
   `"_meta": {"steering": {"idleBehavior": "promptRequired"}}` when
   `idle_guard == PromptRequired` (ACP extension wire only; goose keeps its run
   id form).
3. Outcome decoding: `injected` → `Injected`; `startedNewTurn` →
   `StartedNewTurn`; `promptRequired` → `NotDelivered{PromptRequired}`;
   `failed` → `Unknown{AdapterReportedFailure}`; anything else / absent →
   `Unknown{UnrecognizedAck}`; JSON-RPC error `-32601` →
   `NotDelivered{MethodNotFound}`; other error → `NotDelivered{Rejected}`.
4. Write error → `Unknown{WriteFailed}` (a partial write cannot be excluded).
5. If the prompt's own response (or error/EOF/timeout) arrives while a steer
   is pending: keep reading up to `STEER_ACK_DRAIN` for that ACK. If it
   arrives, resolve it. If not, resolve `Unknown{PromptEndedBeforeAck}` (or
   `RuntimeExited` on EOF, `AckTimeout` when the drain expired) **and** remember
   `(wire_request_id → attempt_id, wire)` in an unresolved map.
6. Every read loop in `AcpClient` (prompt loop and `send_request`) checks a
   response id against the unresolved map before treating it as stray; a match
   is decoded exactly as in (3) and sent to the late sink as `LateSteerAck`,
   then removed from the map.
7. Inputs still in the channel when the loop exits are dropped; the dropped
   oneshot is the caller's `NotDelivered{PromptEndedBeforeWrite}` signal.
8. `clear_steer_input` is idempotent; the legacy pool keeps working through an
   adapter in `pool.rs` that maps `SteerResolution` onto the private
   `SteerAck`/`SteerError` (`Injected|StartedNewTurn → Success`,
   `NotDelivered{Unsupported} → ExpectedRunIdMissing`, `MethodNotFound|Rejected
   → AgentError`, `Unknown{Unrecognized|AdapterReportedFailure} →
   OutcomeRejected`, other `Unknown → PromptCompletedNeutral`,
   `PromptEndedBeforeWrite → PromptCompletedNeutral`). Legacy behaviour must
   not change; its tests stay green.

### 3.2 `buzz-core` — receipts and codes (client/contract lane; skeleton by finalizer)

```rust
ReceiptStatus::TurnInjected          // "turn_injected"   six keys, turnId required
ReceiptStatus::TurnDeliveryUnknown   // "turn_delivery_unknown" five keys, error required
LifecycleReceipt::turn_injected(command_id, target, turn_id)
LifecycleReceipt::turn_delivery_unknown(command_id, target, code, message)
// codes (≤ 64 bytes, documented in NIP-CSL table):
STEER_TURN_ENDED, STEER_SATURATED, STEER_REJECTED, STEER_ATTACHMENTS_UNSUPPORTED,
STEER_NOT_DELIVERED, STEER_WRITE_FAILED, STEER_ACK_LOST, STEER_ACK_TIMEOUT,
STEER_ACK_UNRECOGNIZED, STEER_UNRESOLVED_AT_RESTART, STEER_UNOBSERVED_NEW_TURN
```

`is_turn_stage` includes both. The strict decoder accepts `turnId` exactly
when status ∈ {`turn_started`, `turn_injected`}. `pulse_fold` and the
generation mint ignore both for generation status (they already skip turn
stages).

### 3.3 `buzz-core` — runtime descriptor

```rust
#[serde(rename_all = "camelCase")]
pub enum SteerIdleGuard { PromptRequired }
// RuntimeDescriptor:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub steer_idle_guard: Option<SteerIdleGuard>,
```

Desktop `session_provider/runtimes.rs` sets `Some(PromptRequired)` for the
`claude-agent-acp` row only, with a comment citing the adapter version and the
dist lines above. Codex and goose rows leave it `None`.

### 3.4 `buzz-session-provider` (provider lane)

Actor (`session.rs`):

```rust
SessionCommand::Steer {
    command_id: String,
    attempt_id: String,
    text: String,                 // signed original text
    operator_pubkey: Option<String>,
    framing: Option<TurnFraming>, // delivery == Steer
}
SessionEvent::SteerResolved {
    session_id, turn_id: Option<String>, command_id, attempt_id,
    resolution: buzz_acp::steer::SteerResolution,
}
SessionEvent::SteerReconciled { session_id, attempt_id, resolution }
pub const STEER_ADMISSION_DEPTH: usize = 4;
```

- Install the steer channel and late sink **before** the prompt borrows the
  client; hand `steer_tx` to the select loop.
- Steer arm: pass a shared write guard into `steer_tx.try_send`. Native queue admission is not dequeue/dispatch
  evidence. Immediately before the transport starts writing, the guard checks
  the fence and records dispatch under the same mutex; no await separates
  admission from beginning the write. A queued input waiting behind an ACK
  therefore remains preventable. An unverifiable/poisoned guard refuses as
  `ACTOR_UNAVAILABLE`; an observed fence refuses as `HANDOVER_FENCED`. Neither
  falls back to an ordinary turn. A full native queue →
  `SteerResolved{Saturated}` (a provider-side wrapper variant, not in
  `buzz-acp`); the provider answers it terminally (`turn_dropped` /
  `STEER_SATURATED`, §2). Before emitting any `SteerResolved` whose resolution
  did not enter the runtime (`NotDelivered`, `Saturated`, `Idle`), the actor
  removes the command from `dequeued`, so the fallback turn's later dequeue
  is recorded truthfully. Pending oneshots live in a
  `FuturesUnordered` arm of the same `select!`.
- On `Injected`: emit `TranscriptItems` with
  `user_prompt_item(text, true, operator, command_id, sender_role, 0)` under
  the **current** `turn_id`, then `SteerResolved`. The translator is not
  touched (`begin_turn` is not called).
- A steer that arrives while idle (no prompt in flight) →
  `SteerResolved{NotDelivered{PromptRequired}}`-equivalent without any runtime
  write; the provider boundary-delivers it.
- Before `TurnFinished`: drop `steer_tx`, await every pending oneshot (dropped
  channel ⇒ `PromptEndedBeforeWrite`), emit their `SteerResolved`s, then
  `clear_steer_input`. Late sink events map through the actor's
  `attempt_id → command_id` table to `SteerReconciled`.

Provider (`lib.rs` + `state.rs`):

- New durable ledger `steer_attempts.jsonl` (append-only, last record per
  `attempt_id` wins) with `{attempt_id, command_id, session_id, generation,
  channel_id, target, operation_key, operator_pubkey, text, disposition, at,
  turn_id?}`; dispositions `intent | injected | started_new_turn |
  not_delivered | unknown | prevented | reconciled_injected |
  reconciled_not_delivered | reconciled_new_turn`. API:
  `stage_steer_intent`, `resolve_steer_attempt`, `open_steer_attempts`,
  `steer_attempt_for_command`, `unresolved_unknown_attempts`.
- `attempt_id = format!("{command_id}#{n}")`, `n` = 1 + prior attempts for the
  command (deterministic; no timestamps).
- Dispatch (`on_turn`, `TurnDecision::Start` with `deliver == Steer`):
  `native_steer_deliverable(session)` = advertised at initialize **and**
  descriptor `steer_idle_guard == Some(PromptRequired)` **and**
  `NATIVE_STEER_DELIVERABLE` (now `true`; the `const` assertion in
  `inject_native_steer` is removed with the stub). Attachments present ⇒ not
  a native attempt (`turn_degraded`/`STEER_ATTACHMENTS_UNSUPPORTED` + boundary).
  Otherwise: `stage_steer_intent` (durable) **before** `handle.deliver(Steer)`;
  keep an `InFlightTurn` entry with `steer_attempt: Some(attempt_id)` and
  `text: Some(text)`; `DeliverError` ⇒ persist the terminal undelivered answer
  before resolving `prevented`.
- `decide_turn`: a command whose attempt is open or resolved is not re-admitted
  (open ⇒ silent ignore; resolved ⇒ already in consumed/refused ledgers).
- `SteerResolved` fold, keyed by `attempt_id`:
  - `Injected` ⇒ consume the operation when keyed, then the command; enqueue
    `turn_injected(command, target, turn_id)` before resolving `injected`.
    **No** `record_turn_spend`: ownership and accounting remain with the
    original turn. Release actor bookkeeping and remove in-flight only after
    the durable projections succeed.
  - `StartedNewTurn` ⇒ likewise consume operation/command and enqueue
    `turn_delivery_unknown(STEER_UNOBSERVED_NEW_TURN)` before resolving
    `started_new_turn`. The runtime accepted it but its output is unobserved.
  - `NotDelivered{DispatchPrevented}` ⇒ a durable terminal refusal before
    resolving `prevented`: `HANDOVER_FENCED`, `UNAUTHORIZED_OPERATOR`,
    `AUTHORITY_NOT_REVERIFIED` or `ACTOR_UNAVAILABLE` according to the guard.
    Zero runtime writes; no boundary fallback.
  - Other `NotDelivered{reason}` ⇒ recheck generation, current authority and
    any sticky authority-loss reason recorded after dispatch. A refused
    fallback saves its terminal answer before resolving `prevented`. Restoring
    a grant does not resurrect an input whose authority was lost while its
    ACK was pending. Otherwise resolve `not_delivered` and hand the saved text
    to the ordinary boundary-turn path. On delivery, publish `turn_degraded`
    then `turn_queued`; the attempt no longer owns the ordinary queued turn.
    Codes: `Unsupported | MethodNotFound → STEER_UNSUPPORTED`;
    `PromptRequired | PromptEndedBeforeWrite | idle → STEER_TURN_ENDED`;
    `Rejected → STEER_REJECTED`.
  - `Unknown{reason}` ⇒ stage the durable terminal
    `turn_delivery_unknown(code)` before resolving `unknown`;
    `WriteFailed → STEER_WRITE_FAILED`; `PromptEndedBeforeAck | RuntimeExited →
    STEER_ACK_LOST`; `AckTimeout → STEER_ACK_TIMEOUT`;
    `UnrecognizedAck | AdapterReportedFailure → STEER_ACK_UNRECOGNIZED`.
  - Saturation ⇒ stage the durable `turn_dropped(STEER_SATURATED)` before
    resolving `prevented`. Never reorder it into the boundary queue.
  - If a projection fails before an attempt closes, its open intent remains
    available to restart recovery. A stored command/operation claim or terminal
    answer still fences replay; recovery reports unknown where delivery cannot
    be established, rather than replaying a possible runtime write.
- `SteerReconciled` fold: only for attempts in `unknown`; per §2 table.
- Restart: before replaying the watermark, every `intent` attempt ⇒
  `record_refusal` + `turn_delivery_unknown(STEER_UNRESOLVED_AT_RESTART)` +
  resolve `unknown`.
- Authority enforcement first fences/latches every affected native input
  across the genesis's local records before any fallible terminal persistence.
  `FencedAt::Queued` means zero writes; persist the terminal answer before
  resolving `prevented`. `AlreadyDequeued` preserves a typed sticky reason:
  `Injected` remains truthful, `Unknown` remains unknown, and `NotDelivered`
  cannot fall back. This covers verified takeover, grant removal/downgrade
  and a chain that cannot be reverified. Ordinary turn cancellation policy is
  unchanged by the native grant correction.
- `metadata_for` publishes `threadSteer = native_steer_deliverable(session)`.
- Turn framing for a native steer keeps `Delivery: steer`.

### 3.5 Clients (client/contract lane)

- Desktop `codingSessionIngressPayloads.ts`: accept `turn_injected` (turnId)
  and `turn_delivery_unknown` (error). Pending rows: `turn_injected` relabels
  "Injected into the running turn" (row still settles on the echo);
  `turn_delivery_unknown` relabels "Delivery unknown — <message>", exempt from
  expiry, dismissable, draft recoverable. Receipt index precedence:
  `turn_injected` ≥ `turn_started` > `turn_degraded` > `turn_queued`.
  Transcript rendering of `steered:true` prompts shows a visible "steered"
  marker beside the sender.
- Mobile decoders/models: accept both statuses (fail-closed otherwise is the
  current behaviour; keep it for unknown strings).
- `bee sessions` decodes via `buzz_core` and needs only wording where it
  renders statuses.
- NIP-CSC/NIP-CSL amendments: the implementation-status paragraphs, the code
  table, the six-key rule (now `turn_started` **or** `turn_injected`), and
  §"Fork amendment: delivery classes" describing the idle guard.
- No launcher/dialog edits. No automatic project-context work.

**Extended 2026-09-12** (see `history/2026-09-12-steering-experience.md`).
Three additions to this client section; the runtime design above is unchanged.

- **The degrade reason is carried, not dropped.** The progress stage is
  `{stage: "degraded", code, message}` and the pending row stores
  `degradedByProvider: {code, message}` (required). A row renders the
  provider's own reason: `STEER_UNSUPPORTED` is a statement about the
  runtime's capabilities, `STEER_TURN_ENDED` is not, and an unrecognized code
  repeats the provider's message rather than inventing one. No caption may
  say a degraded turn was *delivered* — `turn_degraded` precedes the
  `turn_queued`, so the turn is queued.
- **The delivery class is the sender's choice, resolved at submit.** Working
  and steering advertised: primary `steer`, secondary explicit `boundary`.
  Working without steering: `boundary` alone. Idle: `boundary`. The class is
  computed from the render's own target and capability, never carried from
  when a control was drawn, and the difference is stated on screen rather
  than in a `title`. A composer must never offer an enabled control promising
  steering to an execution whose `threadSteer` is false.
- **A delivery-unknown row offers recovery that claims nothing.** Copy to
  draft appends the sender's words to the composer, publishes nothing, leaves
  the row and its `deliveryUnknown` state untouched, and discloses both that
  the input may already have arrived and any attachments it is not bringing
  back. Dismiss stays local. No client control may cancel, edit or recall a
  published command.

## 4. Composition fixtures (finalizer runs; lanes may reuse)

Scripted fake agents in the provider tests (shell style as `STEERING_AGENT`):

- `HELD_OPEN_STEER_AGENT`: `initialize` advertises steering; `session/prompt`
  emits a chunk and keeps reading; each `_session/steering` line with
  `promptRequired` in it answers `{"outcome":"injected"}` and emits a chunk
  `steered:<n>`; after two steers, answers the prompt `end_turn`.
- Variants: answers `{}` / `promptRequired` / `startedNewTurn` / `-32601` /
  JSON error / no answer then `end_turn` (ACK lost) / exits after reading the
  steer (EOF) / answers after `STEER_ACK_DRAIN` (timeout) / answers the second
  steer with the first steer's id (mis-correlation must be refused).

## 5. Gates

Lanes: `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets`,
`cargo fmt`, plus `just desktop-check`/`desktop-test`/`mobile-check`/`mobile-test`
for the client lane. Finalizer: `just ci`, `just smoke` (desktop changed),
adapter validation against the installed claude-agent-acp with a unique marker.

## September 12 integration correction

The native queue and actual runtime dispatch are distinct boundaries. The
original candidate recorded dequeue before placing a steer behind a pending
ACK, so an authority change could no longer prevent an unwritten queued input.
The shared guard now linearizes prevention versus beginning a runtime write.
`native_steer_fence_tests.rs` holds the first ACK, queues and revokes a second
input, releases the ACK and proves the second input never reaches the adapter.
It also preserves the original open turn and its spend. The same regression
fails against `f7d628b4a` and passes with the correction. These are fake-adapter
process tests; installed real-adapter acceptance remains separate.
