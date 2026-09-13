# Steering integration corrections — 2026-09-12

Owner: Astra. Candidate starts at Opus's `f7d628b4a`, which includes Fable's
`d2bb6de8f` and `0963723b4`, plus Opus's `ecd4336f4` and report commit.
The original worktree remains unchanged. Corrections are developed on
`work/steering-integration-astra`; no push, installation or live-model
acceptance is claimed here.

## A queued steer remained executable after prevention

The provider recorded `dequeued` when it admitted a steer into its native
queue. The ACP transport waits for the previous steer acknowledgement before
writing the next. A queued input could therefore be classified as already
dispatched and survive an authority change while no bytes had been written.

`session_steer_guard.rs` now shares the existing fence mutex with the transport.
`acp_steer_write.rs` calls its guard immediately before beginning the runtime
write. Under the same mutex, either prevention wins or dispatch is recorded;
queue admission no longer records dispatch. An observed fence produces a
terminal `HANDOVER_FENCED`; unverifiable guard state produces
`ACTOR_UNAVAILABLE`. Neither becomes a boundary fallback. This does not claim
to recall bytes whose write has already begun.

The deterministic process test in `native_steer_fence_tests.rs` holds the first
ACK, queues a second steer, prevents it, then releases the ACK. It verifies no
second runtime write and preserves the original turn's identity, ownership
and spend. The new regression fails against the original `f7d628b4a` source
and passes with the guard. Final focused steering suites pass: 58 ACP and 46
provider tests; one separately invoked real-model test remains ignored.
These are fake-adapter process tests, not installed Claude acceptance.

The follow-up correction also enforces verified operator removal/downgrade
and a chain that cannot be reverified. Every affected native input, including
siblings, is fenced and receives a sticky reason before fallible persistence.
Regrant cannot revive a queued input whose channel closes before its guard
runs, or a dispatched input later acknowledged as not delivered. Already-written
Injected and Unknown outcomes stay truthful; ordinary turn policy is unchanged.

The terminal answer is durable before closing prevented attempts. Delivered
outcomes consume their operation/command and enqueue their result before
closing; failed projections leave an open intent for conservative restart
recovery. Native mailbox failures and saturation follow the same ordering.
Fault tests cover multiple queued inputs/siblings and snapshot, outbox, refusal
and attempt persistence, plus restart. The deterministic grant, unreadable-chain
and revoke/regrant regressions each have failing-before logs and pass with the
correction. The final scoped run passes 58 ACP and 46 provider tests, one
separate real-model test ignored; all-targets clippy and formatting pass.
An independent read-only review reports no remaining finding in this scope.

Complete raw logs are under `../review-steering-runtime-2026-09-12/`:
`grant-fence-red.log`, `unverified-fence-red.log`, `regrant-fence-red.log`,
`final-steer-tests.log`, `final-steer-clippy.log`. The first takeover guard
red/green runs survive as tool-output excerpts in that directory, explicitly
labelled excerpts rather than complete stdout logs.

## Three browser failures, traced to their actual boundaries

All three were reproduced against the original candidate's unchanged E2E
bundle, with the original assertions and timeouts. Diagnostic copies recorded
IPC timestamps and sampled after failure; they did not extend assertions.

- **Mission goal read:** the correct signed goal was already returned by the
  catalog at 6.02 seconds. A separate goal-history WebSocket query waited
  behind startup sends; the inspector was unresolved at 12.48 seconds and
  resolved at 16.55. `useCodingSessionGoals.ts` now uses the existing coalesced
  one-shot reader with identical kind, channel and limit. Signature folding,
  live subscriptions, reconnect behavior and authorization are unchanged.
- **Branch creation:** the native create completed at 1.777 seconds, but its
  success path awaited broad project invalidation, including unrelated issue
  history, and then repository refresh. The screen still said Creating at
  6.88 seconds. `branchMutations.ts` now acknowledges the successful write and
  updates selection immediately; refresh proceeds separately. A failed refresh
  explicitly reports that the branch changed but the read failed. It does not
  report a successful write as a failed creation or deletion.
- **Declared work:** its read-only command allowlist omitted `get_relay_self`,
  the NIP-11 GET used to verify deletion receipts. That getter is added while
  preserving the test's no-write assertion.

The original report incorrectly identified PR's failure as a command guard.
Its actual failure is the branch-created toast assertion. A September 8
artifact already shows that timeout; attribution to the rebase or Andy's
changes is unsupported. The original report retains explicit corrections.

Ten scoped hook tests, desktop typecheck and scoped Biome checks pass. On one
fresh integration E2E build, all three original browser cases and all five
native-steering cases pass without relaxing their assertions. Six native
captures have distinct SHA256 hashes.

## Short windows and large text

At 250% text and 720px height, the standalone session's fixed header and
composer can consume the entire reading area. A fit-aware fallback puts
those elements into normal flow and lets the outer workspace scroll while
retaining the same bounded transcript scroller and virtualizer. Normal-size
layout retains the fixed dock.

The original failure exposed the handover warning outside the measured area.
The correction keeps that host in one stable shell slot across history-branch
changes, preserving its pending action and chosen checkout. The warning joins
normal scroll flow when needed; the title receives its own row at large text.
The final 900×720, 250% workflow passes full bounds and center hit tests for
recovery, title, session details and Queue next. The 120-turn virtualized case
also passes with the same bounded transcript scroller.

Two follow-up test failures were fixture issues, corrected with evidence:
Radix restores focus after closing details, so the test now waits for that
completion before scrolling to Queue next; and the protocol intentionally
omits the default boundary field, which the assertion now decodes using the
same default as the existing native-steering spec. Neither change relaxes
reachable-controls assertions. The separate umbrella workspace has an analogous
layout and remains outside this correction's verified coverage.

## Evidence and remaining work

Local-only diagnostics are under `../review-2026-09-12-steering-triage/`:
`original-three.log`, `probed-two.log`, `browser-triage.md`,
`integration-build.log`, `integration-targeted.log`, traces and screenshots.
The initial targeted run was 9 passed / 1 failed and retained the real
short-window failure. After the correction, all ten cases pass across focused
runs: three original cases, five native-steering cases, and two short-window
cases. The final seven-case run passed six and reached the valid boundary-send
assertion; its one corrected test then passed in 14.6 seconds on the same
bundle. Logs are `integration-final-targeted.log` and
`integration-final-short-retry.log`; ten final PNGs have ten distinct hashes. These artifacts are machine-local; the methods and
outcomes needed to assess them are recorded here.

The original full smoke result remains 1309 passed / 9 failed / 1 skipped.
The targeted corrections do not replace a fresh combined full gate. Full
`just ci`, relay/database integration tests after combining project setup,
full smoke run alone, and installed Claude steering/Queue next acceptance
remain owed. Delivery unknown must retain its evidence while Copy to draft
sends nothing. Codex remains boundary-only. `TESTING.md` now dates the observed
1.5-hour smoke duration rather than presenting the obsolete 47-minute figure.

## Combined candidate and gate environment

The four project-setup commits are now combined with the steering correction
`89ed8285e` on `work/steering-integration-astra`; no merge commit is used.
Both branches independently allocated ledger items 110/111. The original
steering entries remain verbatim in the linked historical branch register;
canonical entries 114/115 point to them, and 116–118 record integration findings.

The first commit hook entered pnpm 11.4.0 automatic dependency installation
because this isolated worktree reuses matching installed dependencies. It was
stopped before shared-directory removal. The invocation-local
`PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN=false` resolves to `false` and runs the
actual Biome hook without installation; no hook is edited or skipped. The
commit then passed pre-commit and signoff. Logs: `combine-setup-hooks.log` in
the triage directory above.

Full `just ci` passed on combined commit `0e4c2cd84` (exit 0), with the
invocation-local pnpm setting above. Raw log: `combined-ci.log` in the triage
directory. The completed Rust harnesses report 10,693 passed / 0 failed
including native desktop's 3,252; desktop JavaScript reports 9,316 / 0 and
mobile reports 2,011 / 0. Formatting, clippy, frontend checks, size gates and
desktop/web builds passed. Rust ignored tests remain ignored; this is not
live relay or installed-app evidence. Independent read-only integration review
confirmed the three automatically combined code/config files preserve both
patches, and the 3,490-byte archived steering findings plus the ledger's
54,646-byte historical suffix remain byte-identical to their inputs.

The six original baseline browser assertions remain current. An isolated
unchanged rerun and timestamped probes are next; no timeout or assertion was
weakened. Broad `just test`, full smoke alone and installed Claude acceptance
remain owed. Host publication beyond the conditional source transaction is a
separate unfinished project-setup milestone.

## Broad integration verification

`just test` passed on the combined Rust tree at `939607485` (exit 0),
with the same invocation-local pnpm setting. No Rust source changed during
the run. Raw log: `combined-integration.log` beside `combined-ci.log`. The
recipe ran its scratch genesis, push-gate and CI-completion prerequisites,
then workspace tests, database tests and workspace integration tests; the
runner reports all steps passed (421 seconds for its final `run-tests.sh`
portion). Ignored native continuation compositions stay ignored unless their
separate recipe is invoked. This is local infrastructure evidence, not a
production deployment or installed-app acceptance claim.

## Real installed-adapter verification

The ignored `buzz-acp` test
`native_steer_against_installed_claude_agent_acp` passed on the combined Rust
tree (1 passed, 0 failed, 16.64 seconds). It actually ran rather than taking
the missing-adapter skip. Claude ACP 0.70.0 reported steering support; after
41 characters streamed, the ACP extension answered `Injected`. Marker
`cef155a3c7564975a3beac002bd079a2` was echoed in that same prompt before
`EndTurn`. The idle guard then answered `NotDelivered(PromptRequired)` with
no additional agent text during the 12-second observation. The log ends
`LIVE PASS marker=cef155a3c7564975a3beac002bd079a2`.

Raw evidence: `combined-live-adapter.log` in the triage directory. This used
the installed adapter and a real model in an empty temporary folder, with
no MCP servers. It verifies the current ACP transport and idle guard, not
the installed sidecar, signed relay receipts or UI actions. Installed-app
Steer/Queue next acceptance remains owed. No safe deterministic installed-UI
delivery-unknown induction is established; the process fault tests and mock
Copy-to-draft proof cover complementary seams. Do not kill a shared provider
or claim that disconnecting network necessarily loses its local stdio ACK.

## Final CI exposed a test synchronization race

The final `just ci` on `3060ef8e0` stopped in
`buzz-agent/tests/regressions.rs::handoff_cap_binds_within_a_single_turn`: the
test finished its second turn before observing the steer acknowledgement.
The same Rust source passed the earlier combined CI and broad integration
run. Log: `combined-final-ci.log`. This run is failed, not counted as green.

The fixture used immediate canned model responses and treated an active-run
metadata notification as if it kept that run open. The test-only correction
holds the third model response until the same-run steer acknowledgement is
received, after the handoff attempt budget has been consumed. It keeps the
four-model-request count, accepted-steer assertion and cap warning, and also
asserts the final model request contains the steer canary. No production
steering path changes. The original isolated test passed 30/30 repetitions,
so the failed CI is the failing-before evidence. The corrected fixture passes
all 52 regression tests and 100/100 focused repetitions; formatting and diff
checks pass. A 15-second watchdog matches the existing harness response bound
and only fails a missing gate; the oneshot/ACK establishes ordering. A new
complete CI run remains owed. Raw logs: `handoff-cap-before-loops.log`,
`handoff-cap-regressions-green.log` and the final handoff-cap logs under
`../review-2026-09-12-smoke-read-triage/`.

A final changed-file scan found one additional literal NUL already present
in `codingSessionTeamDeliveryStatus.ts` on main and the Opus candidate. Its
source spelling is normalized to `\u0000`, preserving the runtime key. All
22 delivery-status tests pass with the repository test loader. The initial
plain-Node invocation failed to resolve the repo's `@/` aliases and is retained
as a harness error, not a product failure (`delivery-source-escape.log` and
`delivery-source-escape-green.log`).
