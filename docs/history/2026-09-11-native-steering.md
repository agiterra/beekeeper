# 2026-09-11/12 — native steering implemented for Claude; candidate on `work/native-steering-fable`

**Status:** implementation candidate, not landed. Built by Fable as
orchestrator per the September 11 brief below, on worktree
`/Users/brian/Projects/beekeeper/review-native-steering-fable`, branch
`work/native-steering-fable`, based on `main` at `77b792de9`. Astra reviews and
integrates; nothing was pushed, merged to `main`, or installed. The binding
contract the lanes built to is `docs/NATIVE_STEERING_IMPL.md`; the NIP
amendments are in `docs/nips/NIP-CSC.md` and `docs/nips/NIP-CSL.md`. Exact
commit ids are in the handoff note at the end of this entry.

### What an authorized participant can do now

While a Claude execution is working, a `thread.turn.start` with
`deliver: "steer"` is written into the running ACP prompt through the
adapter's `_session/steering` request and acknowledged. The original turn
keeps its `turnId`, its streaming translator, its spend and its team-wake
causality; the injected words are echoed as `user_prompt{steered:true}` on
that turn and the command gets a `turn_injected` receipt. Stop
(`thread.turn.interrupt`) is untouched. Codex and goose executions publish
`threadSteer: false` and keep the boundary downgrade (`turn_degraded` /
`STEER_UNSUPPORTED`, then `turn_queued`). No prompt is ever cancelled,
restarted or merged to simulate a steer.

### Adapter facts, verified against the installed packages

Installed under `~/Library/Application Support/Beekeeper/node-tools/`
(node v24.18.0 from `runtimes/node`); driven directly over stdio by
`review-2026-09-11-native-steering/steer_driver.py` and, after the transport
landed, through Beekeeper's own `AcpClient` by the ignored test
`native_steer_against_installed_claude_agent_acp`
(`crates/buzz-acp/src/acp.rs`). Every wire frame is in
`review-2026-09-11-native-steering/{claude,codex}-frames.jsonl` and
`acpclient-live-claude.log`.

| Adapter | Version | Mid-turn `_session/steering` | Idle with `_meta.steering.idleBehavior: promptRequired` | Unknown method |
| --- | --- | --- | --- | --- |
| `@agentclientprotocol/claude-agent-acp` | 0.70.0 | `{"outcome":"injected"}` 1 ms after the write; the marker reply streamed inside the **same** prompt; prompt ended `end_turn` | `{"outcome":"promptRequired","reason":"noRunningTurn"}`, no output followed | `-32601` |
| `@agentclientprotocol/codex-acp` | 1.6.2 | `{"outcome":"injected"}` 2 ms after the write; the model finished all 40 numbers before answering the marker | `{"outcome":"startedNewTurn"}` and a **detached turn ran** (its output was observed) | `-32601` |

Underlying CLIs: Claude Code 2.1.269, codex-cli 0.148.0. Runtime acceptance
is reported separately from model-visible use: in the direct Claude probe the
model stopped counting at 13 and answered the marker; in the `AcpClient` run
it had already produced all 40 numbers when the injected message was applied
and then answered the marker in the same prompt. Both are `injected`; neither
proves the model will obey a steer mid-list. Codex has no idle guard, so a
steer racing a turn end would start a native turn this provider never reads.
That is why Codex stays in boundary mode, by a declared runtime fact rather
than a driver-name inference: `RuntimeDescriptor.steerIdleGuard` is set to
`promptRequired` only on the claude row of the desktop host table
(`desktop/src-tauri/src/session_provider/runtimes.rs`), citing the adapter
lines that honour it.

### What shipped, by lane

- **Transport (`crates/buzz-acp`)**: public module `buzz_acp::steer`
  (`SteerInput`, `IdleGuard`, `SteerResolution::{Injected, StartedNewTurn,
  NotDelivered, Unknown}` with typed reasons, `LateSteerAck`,
  `STEER_ACK_DRAIN` = 1.5 s). `AcpClient::{install_steer_input,
  clear_steer_input, set_late_steer_sink, unresolved_steer_attempts}`. The
  read loop writes the idle guard on the ACP extension wire, decodes every
  acknowledgement positively (`{}` and `failed` are `Unknown`, never
  success), classifies a write error as `Unknown{WriteFailed}`, keeps reading
  for a bounded drain after the prompt's own answer, and correlates late
  answers by JSON-RPC id to the attempt from any later read loop. The cancel
  path drops admitted, unwritten inputs instead of writing them after
  `session/cancel`. The legacy channel harness is adapted, not changed.
- **Provider (`crates/buzz-session-provider`)**: `SessionCommand::Steer`,
  `SessionEvent::{SteerResolved, SteerReconciled}`, admission depth 4,
  durable `steer_attempts.jsonl` (intent written **before** the runtime
  write; dispositions `intent | injected | started_new_turn | not_delivered |
  unknown | prevented | reconciled_*`). Injected: consume, no
  `record_turn_spend`, `turn_injected`. Not delivered: authority and
  generation re-checked, then boundary delivery with `turn_degraded` +
  `turn_queued`. Unknown: refused ledger + `turn_delivery_unknown`, never
  replayed. Restart: open intents answered
  `STEER_UNRESOLVED_AT_RESTART` before any watermark replay. Takeover fence
  distinguishes queued (refused, zero writes) from dispatched (marked; a
  later non-injected answer is refused, an injected one stays truthful).
  `NATIVE_STEER_DELIVERABLE` is `true`; `threadSteer` is published only when
  the runtime advertised steering **and** the descriptor declares the idle
  guard **and** the provider can deliver.
- **Contract and clients**: `ReceiptStatus::{TurnInjected,
  TurnDeliveryUnknown}` and constructors in `buzz-core`; eleven `STEER_*`
  codes; strict decoder accepts `turnId` for `turn_started` or
  `turn_injected`. Desktop, mobile and `bee sessions` decode both statuses;
  the pending row says "Injected into the running turn" and settles on the
  steered echo, or "Delivery unknown — …" (kept, exempt from expiry,
  dismissable, draft not silently restored); `turn_injected` outranks an
  earlier `turn_delivery_unknown`. Transcripts show a `steered` marker. Older
  clients drop the two unknown statuses at decode and still settle on the
  echo. Playwright spec `desktop/tests/e2e/coding-session-native-steer.spec.ts`
  proves both rows in the browser.

### Outcome table as built

| Observed | Disposition | Wire |
| --- | --- | --- |
| No capability (not advertised, no idle guard declared) or `-32601` | boundary | `turn_degraded` `STEER_UNSUPPORTED`, `turn_queued`, `turn_started` |
| Attachments on a steer | boundary | `turn_degraded` `STEER_ATTACHMENTS_UNSUPPORTED`, then queued |
| `promptRequired`, prompt ended before the write, no turn in flight | boundary | `turn_degraded` `STEER_TURN_ENDED`, then queued |
| JSON-RPC error other than `-32601` | boundary | `turn_degraded` `STEER_REJECTED`, then queued |
| Admission channel full | terminal | `turn_dropped` `STEER_SATURATED` (sender resends; a fallback would reorder one operator's inputs) |
| `injected` with the correlated id | consumed | `user_prompt{steered:true}` on the active turn, `turn_injected` |
| `startedNewTurn` | consumed, unobserved | `turn_delivery_unknown` `STEER_UNOBSERVED_NEW_TURN` (never resent) |
| Write error / prompt or runtime ended before the ACK / drain expired / `{}` or `failed` | refused, never replayed | `turn_delivery_unknown` with `STEER_WRITE_FAILED` / `STEER_ACK_LOST` / `STEER_ACK_TIMEOUT` / `STEER_ACK_UNRECOGNIZED` |
| Open intent found at restart | refused | `turn_delivery_unknown` `STEER_UNRESOLVED_AT_RESTART` |
| Late ACK on an unknown attempt | reconciled | injected ⇒ steered echo then `turn_injected`; not delivered ⇒ `turn_dropped` `STEER_NOT_DELIVERED`; new turn ⇒ ledger only |
| Unauthorized, stale generation, fenced before dispatch | prevented | existing `turn_refused` codes, zero runtime writes, no attempt record |

### Adversarial review and what it changed

An independent reviewer attacked double delivery, silent loss,
mis-correlation, ownership/accounting, authority, capability truth, transport
edge cases and client behaviour, with throwaway proof tests. Three findings
were real and are fixed with ported regression tests:

1. **Blocker.** An ex-owner's dispatched steer answered `promptRequired` after
   a takeover fell back to the mailbox with no authority re-check and ran.
   Fixed: the fallback re-runs the handover fence, generation, closed-state
   and grant checks; a fenced-after-dispatch attempt that comes back
   undelivered is refused (`HANDOVER_FENCED`), never run.
2. **Major.** Stale dequeue evidence from the steer made a later fence read
   the fallback turn as already running, cancel a bystander turn, and still
   let the fallback turn start. Fixed: the actor releases the command's
   dequeue evidence before any resolution that did not enter the runtime.
3. **Minor.** An interrupt with a written steer pending produced
   `turn_delivery_unknown`, and the late `injected` reconciliation published
   `turn_injected` with no steered echo, so the row never settled. Fixed:
   the reconciliation publishes the echo on the recorded turn first, and
   clients let `turn_injected` outrank the unknown.

Saturation was changed from a boundary fallback to a terminal drop because
the fallback reordered one operator's inputs. The cancel path fix above also
came from this review. The reviewer's "attacked and held" list covered
double delivery, mis-correlation, accounting, pre-dispatch authority,
capability truth, transport drains and client decoders, each with a cited
seam.

### Tests and gates (exact result lines)

- `cargo test -p buzz-acp`: `test result: ok. 943 passed; 0 failed; 1 ignored`
  (the ignored test is the live adapter run; 25 new deterministic transport
  tests with wire assertions, including the red/green cancel-path proof).
- `cargo test -p buzz-session-provider`: `test result: ok. 785 passed; 0
  failed; 1 ignored` (30+ new tests: two ordered steers into one held-open
  prompt, capability on/off, saturation, every ACK variant, prompt ended
  before dispatch, ACK lost/timeout/mis-correlated, crash before/after
  intent/write/receipt with `FaultPlan`, revoke/takeover queued vs dispatched,
  stale generation, interrupt with ACK pending, `metadata_for` three facts,
  late-ACK reconciliation).
- `cargo test -p buzz-core -p buzz-sdk -p buzz-cli`: 1174 / 1094 / 323
  passed, 0 failed.
- `just desktop-test`: 9261 pass, 0 fail. `just desktop-tauri-test`: 3288
  passed, 0 failed. `just mobile-test`: `+2011: All tests passed!`.
  `just desktop-check`, `desktop-typecheck`, `mobile-check`,
  `pnpm check:px-text`: exit 0.
- Native adapter run through `AcpClient`
  (`cargo test -p buzz-acp native_steer_against_installed_claude_agent_acp --
  --ignored --nocapture`): `test result: ok. 1 passed`; log
  `review-2026-09-11-native-steering/acpclient-live-claude.log`.
- `just ci` on the final tree (after the review fixes): `EXIT ci=0`, started
  2026-09-12T08:14:36Z, ended 08:27:05Z; log
  `review-2026-09-11-native-steering/ci.log`.
- `just smoke`, run alone after that `just ci`: `1309 passed, 6 failed, 1
  skipped (1.4h)`, `EXIT smoke=1`; log
  `review-2026-09-11-native-steering/smoke.log`. The two new native-steer
  specs passed. The six failures (`coding-session-founder-acts.spec.ts:380`,
  `coding-session-observations.spec.ts:33/114/144`,
  `coding-sessions.spec.ts:458`, `project-packs.spec.ts:40`) are
  deterministic and **pre-existing**: rerun on this branch they fail 6/6
  (`smoke-rerun-branch.log`), and rerun on the untouched base `77b792de9`
  in the `beekeeper-app-from` worktree they fail the same 6/6 with the same
  assertions (`smoke-rerun-base-77b792de9.log`). None touches a file this
  branch changed; they are owed their own finding on `main`.

Full-gate history worth knowing: the first `just ci` failed only on Tauri
formatting; the second on the desktop file-size ratchet
(`session_provider/tests.rs` grew past 1000 lines), fixed by splitting the two
new tests into `steer_guard_tests.rs`; the third passed on the pre-review
tree. A `just smoke` run started while that `just ci` was executing produced
1011 failures with `Cannot read properties of undefined (reading
'transformCallback')`: the ci's plain desktop build overwrote the e2e bundle
under the running preview server, the exact failure mode AGENTS.md documents.
It is recorded here as an invalid run, not as evidence about the suite.

### Residual limitations

- Native steering is Claude-only, keyed on the declared idle guard; codex-acp
  1.6.2 has none. Every unsupported execution stays in boundary mode and says
  so.
- Late acknowledgements are correlated only while some read loop runs on the
  client; an idle actor holds a late answer on the pipe until its next prompt
  or cancel.
- A provider fold error leaves an attempt at `intent` until restart recovery
  answers it; `next_steer_attempt_id` scans the ledger per dispatch.
- The 1.5 s acknowledgement drain also runs on idle/hard-timeout exits with a
  native steer pending.
- Two "fenced while unanswered" provider tests and the saturation test use
  short scripted delays on the current-thread runtime.
- Delivery-unknown rows are recovered by dismissal; there is no one-click
  restore into the editor. The pending-row dismiss control sits inside the
  bubble because the workspace's constant dock reserve hides the last row's
  caption at full scroll (pre-existing; worth its own finding).
- `bee sessions` and the desktop still bound receipt codes at 256 bytes
  rather than 64 (pre-existing).

### Handoff to Astra

Written under `docs/history/` per the 2026-09-11 rules; the ledger items
are 110 (the candidate) and 111 (the six pre-existing smoke failures).

Branch `work/native-steering-fable`, rebased onto `main` `9aebb1262`:
implementation commit `d2bb6de8f` (signed, DCO) followed by one docs commit
(this report, ledger items 110–111, the map). Every gate line above was run
on the pre-rebase tree at `16e1a0516`; the rebase brought in four `main`
commits that touch no crate or file this branch changed, and `just ci` was
rerun on the rebased tree (result in the docs commit message). Do not read this entry
as "native steering shipped": it is a candidate whose acceptance in the
installed app with a real session has not been performed, because the app
was deliberately not rebuilt or installed from this branch.
