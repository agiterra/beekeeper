# Historical steering branch findings — 2026-09-12

The following entries are copied verbatim from `docs/SESSION_STATE.md` at
`f7d628b4a` on `work/native-steering-fable`. That unlanded branch allocated
110 and 111 independently of the project-setup branch. Their original numbers
and content are preserved here; they refer to that branch's register, not the
combined ledger's items 110 and 111. The combined ledger links these historical
findings from new entries 114 and 115. Current status is in `CURRENT_STATE.md`
and [the integration report](2026-09-12-steering-integration.md).


Original relative links below were relative to `docs/`. Read the
[native steering report](2026-09-11-native-steering.md) and
[steering contract](../NATIVE_STEERING_IMPL.md) at their current paths.

## Verbatim source

```markdown
110. **Native mid-turn steering for Claude executions: candidate built
     2026-09-11/12 on `work/native-steering-fable` (implementation commit
     `d2bb6de8f`, rebased onto `main` `9aebb1262`); not landed, not
     installed.** Report: [`history/2026-09-11-native-steering.md`](history/2026-09-11-native-steering.md);
     contract: `NATIVE_STEERING_IMPL.md`. Supersedes the September 11 plan
     section's "planning only" status and the map's "blocked on proving
     adapter support": support was proven against the installed adapters
     before any provider change. claude-agent-acp 0.70.0 answers
     `_session/steering` with `injected` mid-turn and, under
     `_meta.steering.idleBehavior: promptRequired`, with `promptRequired` when
     idle without starting a turn; codex-acp 1.6.2 answers `injected` mid-turn
     but starts an unobserved detached turn when idle, so it stays in boundary
     mode (`../review-2026-09-11-native-steering/{claude,codex}-frames.jsonl`;
     through Beekeeper's own client,
     `cargo test -p buzz-acp native_steer_against_installed_claude_agent_acp
     -- --ignored`, `1 passed`). `crates/buzz-session-provider/src/session.rs`
     `NATIVE_STEER_DELIVERABLE` is `true`; `threadSteer` publishes only when
     the runtime advertised steering **and** `RuntimeDescriptor.steerIdleGuard`
     is declared **and** the provider can deliver. New wire: receipt statuses
     `turn_injected` (six keys) and `turn_delivery_unknown`; eleven `STEER_*`
     codes (NIP-CSL table). A dispatch intent is durable before the runtime
     write; an unacknowledged write is answered `turn_delivery_unknown` and
     never replayed. Adversarial review found and the branch fixed: an
     ex-owner's dispatched steer falling back to the mailbox after takeover
     without an authority re-check (blocker); stale dequeue evidence letting a
     refused fallback turn run (major); a reconciled late `injected` with no
     steered echo (minor). Gates on `16e1a0516`: `just ci` exit 0; `cargo
     test` buzz-acp 943, buzz-session-provider 785, core/sdk/cli 2591,
     desktop 9261, tauri 3288, mobile 2011, all 0 failed; `just smoke` 1309
     passed / 6 failed (item 111) / 1 skipped. Runtime acceptance is not
     model obedience: in one live run the model finished its list before
     honouring the injected instruction. Owed: live use in an installed build
     with a real session.

111. **Six desktop smoke specs fail deterministically on `main` `77b792de9`
     independent of any branch (found 2026-09-12).**
     `coding-session-founder-acts.spec.ts:380`,
     `coding-session-observations.spec.ts:33`, `:114`, `:144`,
     `coding-sessions.spec.ts:458`, `project-packs.spec.ts:40`: each 6/6
     failing on `work/native-steering-fable` and 6/6 with the same assertions
     on the untouched base in the `beekeeper-app-from` worktree
     (`../review-2026-09-11-native-steering/smoke-rerun-{branch,base-77b792de9}.log`).
     They wait on `mission-land-control`, gate rows, `coding-session-observations`,
     `coding-session-goal-catalog` and `project-packs-source-shipped` test ids
     that never appear. Unassigned. Related environment fact: a `just smoke`
     run while `just ci` executes is invalid, because the ci's plain desktop
     build overwrites the e2e bundle under the preview server and every later
     test fails with `Cannot read properties of undefined (reading
     'transformCallback')` (1011 such failures on 2026-09-12; §3a).

```
