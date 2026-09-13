# Startup smoke corrections — 2026-09-13

Owner: Astra. Candidate `work/steering-integration-astra`, starting at
`1669550d2`. Nothing from this integration has been pushed or installed.
The relay and GitHub main were both checked on September 13 and remain
`9aebb1262`. That initial check found no newer commit. A later September 13 fetch found Andy’s `343ea8bd9`; the candidate still needs rebasing onto it.

## Complete CI and interrupted browser coverage

`just ci` on `1669550d2` exited 0. The completed Rust harnesses total
10,693 passed and zero failed, including 3,252 Tauri tests; desktop reports
9,332 passed and mobile 2,011. Formatting, clippy, static/size checks and
desktop/web builds passed. The earlier broad `just test` and real Claude
adapter proof are recorded in the September 12 integration report.

`just smoke` ran alone on a fresh bundle from the same commit, port 4193,
one worker. It reached 1,252 passes, four normal failures and one skip before
the Mac entered low-power sleep at 1% battery. `pmset -g log` records sleep
at 01:21:16 -0400 and wake at 07:56:52, a 23,736-second interruption. The
active thread-unread test then reported a 6.6-hour failure. After waking,
its worker consumed CPU without advancing, so the finalizer stopped the
runner with SIGINT (exit 130), preserving its artifacts.

The entire interrupted spec and all remaining specs were rerun on that same
bundle: 77 passed, zero failed or skipped, 451.9 seconds. The interrupted
thread-unread case passed. Five already-passed cases overlap this remainder;
deduplicated coverage is **1,324 passed, four failed, one skipped**, all
1,329 cases accounted for. This is coverage across two runs, not a single
green full-smoke result. The first attempted remainder used the wrong
relative Hermit activation path; it was stopped and its artifacts retained
before rerunning with the correct environment.

All nine cases in Opus's failure report passed in the original combined run.
All five native-steering cases, both short-window cases and seven project-team
setup cases passed. This remains mock-bridge browser evidence.

## Findings reproduced on the frozen bundle

- **Name blur before history:** the unchanged case and passive diagnostic
  both fail with zero signed name events. Input occurred at 1,353 ms; actual
  blur at 1,365 ms saw Name unresolved, Goal resolved, zero name events and
  no field error. Name history's WebSocket request was not sent until
  10,588 ms. The text handler returned success without publishing or telling
  the person their name was not saved. Ordinary blank optional names must
  remain compatible with Solo start; unresolved dirty names must not silently
  overwrite unread history or report a successful save.
- **Accepted goal absent from header:** the unchanged Solo case and passive
  diagnostic both fail before Start. The goal was signed at 1,379.9 ms and
  its correlated positive relay OK arrived at 1,380 ms. At 6,468.7 ms the
  header still showed no goal. Initial HTTP history had returned empty at
  1,343.4 ms; the first captured live goal REQ arrived at 5,608.6 ms. The
  child retained the accepted text, but the parent reader received no local
  accepted-publication notification. Signing alone is not the evidence:
  this probe captured the positive OK. The mock bridge registers `live-*`
  subscriptions without replaying their positive history limit, so it does
  not prove indefinite loss against a real relay.
- **Accepted goal correction:** the goal publisher now shares the relay's
  accepted receipt with registrations captured before signing/publishing
  awaits. A registration token, rather than callback identity, prevents an
  unsubscribe/resubscribe of the same callback from receiving an old result.
  The mounted reader only admits that receipt through its existing signed
  founder fold, current channel scope and injected relay-client identity.
  The held-ack registration red case and focused 34-test green run cover that
  isolation alongside refused publication, local listener failure, late
  remount, read failure and live-subscription lag.
- **Blank Solo auto-name correction:** blank remains a permitted Solo Start;
  after a model returns a name, the client reads coalesced exact `44229`
  history scoped to channel, session `d` tag and lower-cased founder. It keeps
  a signed founder name found there and refuses to infer absence after a read
  error or a saturated invalid result. The same focused 34-test run covers an
  older exact name behind 1,000 other channel names, read refusal and capped
  history. It adds no role or permission requirement.
- **Discard appears usable but retains the failed create:** the original
  directory-recovery assertion reproduces. The display retains the busy
  blocker and one create after Discard. Initial passive diagnostics failed
  to retain an inline attachment, then failed to reach the committed React
  root because the diagnostic ancestor bound was too small. Those attempts
  are retained and do not prove guard state. The corrected diagnostic reads
  that state: at 5,764 ms the exact failed transaction is published, not
  publishing, with history loading and no history error. Its guard is false.
  History settles at 10,517 ms; the original assertion still fails. A
  separately recorded second click at 10,804 ms clears the transaction and
  durable record, enabling Start by 11,056 ms. Counts remain one create,
  one goal and zero names. UI and handler now share the same readiness
  result; there is no automatic replay of an earlier click or longer timeout.
- **Dense-history measurement:** an earlier frozen-dist instrumented run
  returned, retained, mounted and collected all 450 seeded rows; three
  uninstrumented repeats also passed with all 450 IDs and ten continuation
  requests. That driver correction is **not established**: Sol's fresh E2E
  build then observed 286/450 in the selected matrix, and its isolated repeat
  observed 336/450. Diagnosis has resumed; the threshold is unchanged.

## Focused startup corrections

Sol's focused names/Discard suite passed **105 tests, zero failed**; the
read-only review found no additional in-scope issue. Sol's fresh combined
verification is complete: TypeScript, scoped Biome, repository/desktop size
and px-text checks passed; `build:e2e` finished in 18.2 seconds. Its selected
browser matrix passed 44 tests and failed one dense-history case in six
minutes. Founded, crew, goal, name, worktree, workspace-reuse, native-steer,
short-window and project-team setup cases all passed. The isolated dense repeat
also failed, so only its diagnosis remains pending.

## Landing decision

Brian explicitly accepted landing on 2026-09-13 with the known dense-history
limitation: the fresh selected matrix saw 286/450 dense rows and the isolated
repeat saw 336/450. Final CI completed successfully; [the recovered transcript](2026-09-13-final-ci-recovery.md) accounts for every recipe leg without rerunning the gate. Push, installed-bundle rebuild
and installed UI acceptance remain pending; this report does not claim the
candidate is shipped.

## Evidence locations and current limits

Complete raw logs and artifacts are machine-local under
`../review-2026-09-12-steering-triage/`: `combined-final-ci-2.log`,
`combined-full-smoke.log`, the archived `combined-full-smoke/interrupted-*`,
`resumed-tail-report.json`, and the `solo-start/`, `founded-name/`,
`workdir-recovery/`, `dense-second/` probes. Probe actions/assertions were
preserved, with source and bundle identity recorded before diagnostics.
Separate after-failure experiments cannot turn an original failure green.

The accepted-goal/auto-name red and green evidence is in
`solo-start/accepted-goal-registration-red.log`, `accepted-goal-isolation-green.log`,
`auto-name-red.log`, `auto-name-green.log` and
`startup-finish-terra-2026-09-13.md`. Dense evidence is
`dense-second/artifacts/current-probe-20260913/report.json` and
`dense-second/artifacts/current-uninstrumented-20260913/report.json`; the
earlier intermittent failures remain in `dense-second/corrected-driver-report.json`.
Sol's [static, build and fresh-browser verification report](/Users/brian/Projects/beekeeper/review-2026-09-13-combined-verification-sol/verification-report.md)
links to its machine-local evidence; `playwright-results/` and
`playwright-dense-isolated-results/` there hold the two fresh dense failures.

Dense diagnosis continues after the accepted landing decision. No main landing,
installed UI acceptance, production relay deployment or complete project-pack
publication workflow is claimed.
