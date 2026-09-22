# Wave 2 fourth look — lanes 227 and 228

**Hold. R2 and R4 remain partially closed.** The old mutex replacement and refusal-before-staging defects are fixed. Remaining paths still release custody before proving safety or discard responsibility before preserving an answer.

Reviewed `d209432aa`; checkout `bfd0b04ac` differs only in `docs/CURRENT_STATE.md`. A8 governs. Lane numbers are **227/228**, not their pre-landing reservations 224/225. Findings below cite implementation; ledger text was used only to identify the design choices to attack. No product code, running session or deployment was changed.

## R2 / 227

### P1 — the fence does not exclude waiters already waiting for custody

`crates/buzz-session-provider/src/assignment_custody.rs:202` checks `is_fenced` **before** awaiting the mutex at `:211`; success returns the guard without rechecking. Establishment similarly checks the fence at `:313`, then its detached task acquires through unconditional `hold` at `:355`.

**Executed against the current compiled provider:** hold the seat, start a waiting `hold_for_turn`, raise its fence, release the original guard. Result: **`fenced=true, queued waiter acquired=true`**. Probe: `/tmp/astra-fourth-fence-probe.rs`.

A real establishment can pass the idle/fence checks, lose the scheduling race to an actor acquiring custody, and wait behind that actor. Force-abort then fences and releases the actor's guard; the pre-existing establishment waiter acquires it while group termination is still being checked. The five-second watch or eventual Unproven result cannot retract the checkout already started.

**Required:** enforce the fence at successful acquisition for every turn and establishment, with acquisition/retirement ordering that cannot admit a new holder during the unproven interval. A pre-wait check is insufficient. **Blocks R2.**

### P1 — clean retirement and provider Drop still do not prove process-group quiescence

The clean join branch treats **every** completed join result as Joined, including task failure (`crates/buzz-session-provider/src/session.rs:1093–1094`). Normal actor exit calls `AcpClient::shutdown` (`:2874`), but that function signals the group and waits only for the direct child; child-wait errors and the five-second timeout are logged and returned as success (`crates/buzz-acp/src/acp.rs:1363–1375`). It does not require group disappearance. Restart then resumes unconditionally (`lib.rs:4517`). A joined task whose child wait timed out is therefore reported as proven quiescent without that proof.

The in-process-provider-drop path has a more direct gap: `SessionManager::drop` aborts tasks without fencing or joining/watching their groups (`session.rs:1328–1337`). Cancellation releases the turn's local guard (`:2852`) before the enclosing client's cleanup. The client destructor does issue a **best-effort group kill**, but does not await disappearance (`crates/buzz-acp/src/acp.rs:4001–4014`). A successor or queued establishment can acquire the released mutex while a grandchild can still write. This is not cured by retaining the same mutex.

The explicit force-abort branch is substantially better: fence first (`session.rs:1106`), abort, `killpg(SIGKILL)`, poll to ESRCH, clear only after success (`:903–937`, `:1115–1119`); failures retain Unproven. But `timeout(grace, handle)` consumes the JoinHandle (`:1093`); after timeout only an AbortHandle remains, so the aborted task is **not joined**. Its completion is still inferred.

**Required:** use the same confirmed-retirement contract on clean exit, timeout, panic and provider Drop; retain/join the task and prove group disappearance before making the seat available. Keep the seat fenced if either proof fails. These are source-derived interleavings; I did not crash Brian's provider. **Blocks R2.**

### P1 — the 240-second Git timeout kills only the parent

`crates/buzz-session-provider/src/assignment_inputs.rs:797` spawns an ordinary child; timeout calls only `child.kill()` and `child.wait()` (`:815–818`), ignores their results and returns an error (`:822`). No group is created or descendant termination proven. The establishment then releases custody after its blocking helper returns (`assignment_custody.rs:359–369`). A Git hook or helper descendant can continue writing after the parent is killed and the successor takes the seat.

**Executed isolated helper probe:** copied the actual Git helper into `/tmp/astra-fourth-git-probe.rs`, shortened only the timeout/poll constants, supplied a temporary Git stub with a finite writing descendant, and used no product worktree. After the helper returned “was killed,” the write count grew **5 → 15**. This proves the parent-only timeout behavior; it is not a live 240-second fetch test. Fixture directory is recorded in `/tmp/astra-fourth-git-probe-path`.

**Required:** terminate and establish quiescence of the Git invocation's owned descendants before releasing custody; termination failure must preserve the fence/uncertainty. Also, the helper drains stdout/stderr only after waiting for child exit (`assignment_inputs.rs:810`, `:831–835`): a full pipe can cause an artificial timeout, and an inherited pipe left open by a descendant can block a post-exit read beyond the nominal bound. **Blocks R2's claimed bounded, safe establishment.**

### What is sound, and the four choices

- **One mutex:** `assignment_custody.rs:122–132` inserts only if absent. I found no remaining replacement/removal path.
- **300 versus 240 seconds:** dequeue timeout returns SeatBusy rather than running without a guard (`assignment_custody.rs:211–223`; `session.rs:2800–2820`). It does not itself release the establishment's guard. However, 240 seconds is per Git command, not a total establishment deadline; several commands or pipe drains can exceed 300 seconds. The ordering alone proves neither completion nor safety.
- **Five seconds / ESRCH:** the explicit abort watch correctly withholds success on kill failure or a group still present at the deadline (`session.rs:903–937`). The choice is conservative within the named process group; it does not repair the bypasses above. PID reuse remains an acknowledged limitation, not a new blocker from this review.
- **Re-sendable SeatBusy:** the visible answer exists (`lib.rs:9403–9406`), but it goes through `enqueue_terminal_receipt` (`:9440`), which ultimately records refusal for that command (`state.rs:909–913`). “Send again” therefore requires a **new command identity**; replaying the same command is not a retry. The implementation does not match a literal “non-durable refusal” description.
- **Permanent fence visibility:** Unproven does stay fenced. Acquisition logs `seat_custody_fenced` (`assignment_custody.rs:202–208`); turns receive the generic SeatBusy message (`verification_input.rs:281–285`). Thus it is not wholly silent, but the recipient is told to wait/re-send rather than that predecessor death was unproven and the fence will not clear automatically. Restart logs Unproven and still creates a successor (`lib.rs:4509–4517`); assignment preparation instead defers until expiry (`:8676–8680`). This is a disclosure gap, not another minimum blocker.
- **Conflict key:** `{commandId}:conflict:{eventId}` correctly separates rejection of the competing event from the original promise (`lib.rs:4923`, `:4940`). See R4 below.

## R4 / 228

### The original staging fix passes its regression

Verification refusal (`crates/buzz-session-provider/src/lib.rs:4813`), the ignore refusals including unknown target/stale generation/session closed (`:4877`), and authority Fail (`:4947`) now call the staged helper. It preserves the signed event first (`:7665–7673`), records refusal second (`:7674`), then projects the outbox. The conflict path stages under its distinct event-specific key (`:4923`, `:4940`), leaving the original command's promise open. AlreadyRefused flushes a still-pending answer (`:4885–4898`).

**Ran the crash-equivalent sequence:** `tests/deferred_admission_tests.rs:518` injects an outbox failure after staging, then release/retry. The original UNAUTHORIZED_OPERATOR answer appears once; the third pass produces no duplicate (`:537–565`). All seven tests in that module passed. This exercises a failed projection in one process, not an actual process termination/reopen, and does not establish unconditional exactly-once network delivery.

### P1 — a verification refusal removes the held record before staging can succeed

`lib.rs:4794` removes the held wake; `:4813` attempts staging afterwards. If the state snapshot write fails, staging restores its previous snapshot (`state.rs:1465–1468`). Release logs “stays held” (`lib.rs:8453–8458`) but does not restore the already-deleted record.

**Failing sequence:** deferred verification wake → permanent input refusal → successful held-file removal → transient terminal-state write failure → newer channel event advances the now-unclamped watermark (`lib.rs:6237`, `:1853–1857`) → restart. There is no held record, staged answer, refusal or in-flight delivery left to recover the unanswered wake. An immediate crash alone might replay it from the old relay floor; the subsequent advance is what makes this sequence decisive.

**Required:** retain held custody until the answer is durably staged; release only after responsibility has transferred. The Open branch also removes the hold before downstream acceptance (`:4791`), so audit that transfer at the same boundary. **Blocks R4.** This failure-before-staging sequence is source-derived, not covered by the passing outbox-failure test.

### P1 — an aged staged answer can be discarded as though it reached the outbox

After more than 900 seconds, outbox enqueue rejects the original signed event with `Ok(false)` and only an in-memory parked count (`crates/buzz-session-provider/src/publish.rs:54`, `:411–422`). `flush_terminal_dispositions` ignores that boolean (`lib.rs:7681–7687`) and retires the intent; retirement records refusal and removes the original event (`state.rs:909–913`). A subsequent AlreadyRefused can be Silent, and release removes the held wake (`lib.rs:8430–8451`). The original answer was never durably queued or parked.

**Failing sequence:** staging succeeds, outbox projection fails, provider remains down beyond the timestamp window, recovery projects the old answer and silently retires it. **Required:** distinguish already-present/accepted projection from rejection; retain or durably expose an undeliverable terminal answer. The protocol may require a recovery publication rather than the original stale signature, but dropping the only answer is not recovery. **Blocks R4's durable-answer claim.** This timing case was checked statically, not executed with a fifteen-minute outage.

### Unreadable files and the rewritten failure test

On initial read, unreadable, unsupported-version and malformed files now return errors and are preserved (`deferred_turns.rs:138–158`, `:196`). `held` logs `deferred_store_unreadable` (`:284–290`); deferral refuses instead of replacing the file. This is verified log visibility, not a demonstrated UI banner. A cached mirror still avoids subsequent disk reads (`:193`), so this is not continuous file-integrity detection.

The rewritten test does exercise the provider: a directory occupies the held-file path (`tests/deferred_admission_tests.rs:313`), `handle_command_event` drives the failure (`:343`), and the test checks the actual UNHELD receipt and durable refusal (`:354–372`). That former test gap is closed.

## Verification and decision

Executed **12 custody-boundary tests** and **7 deferred-admission tests**, all passing, including the existing grandchild-abort regression. Additionally executed the queued-waiter/fence probe against the current provider and the isolated Git-helper descendant probe; both demonstrated the unsafe outcomes described above. No live session, production process or worktree was used. Only this report and temporary review probes were written.

Minimum blocking set: **R2 — make retirement/fencing cover pending acquisitions, every actor-exit path and Git descendants; R4 — retain held responsibility until staging succeeds and never retire a rejected outbox projection as delivered.**

Hold — R2 custody/quiescence; R4 durable-answer loss.
