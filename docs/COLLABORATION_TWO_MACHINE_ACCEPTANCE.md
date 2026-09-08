# Two-machine collaboration acceptance

This runbook tests the combined candidate after Fable's recovery work is
integrated. It is not a record of passing tests. Actual results, versions and
findings go in `SESSION_STATE.md` with evidence paths or event IDs.

## Establish the test boundary once

Use Brian's Mac and the Windows machine signed into hive as the other test
account. Record the exact installed build on each and the relay build. A matching
display name is not account identity, and source HEAD is not installed build.
Confirm both accounts can open the same deliberately chosen test project/channel.
Record any existing grant used to address or operate the test agent.

Use a read-only task first. Preserve existing sessions and worktrees. Do not test
shutdown on a provider carrying unrelated active work; use an isolated provider
for forced-process/crash acceptance. Tests never require Andy to be present.

## 1. Shared Roles and identity

On both machines, open the project's Roles page and use Check again.

Expected: the same shared participant has the same identity and role reference;
local availability and installed procedures may legitimately differ. A Windows
view of a Mac-owned agent must not label it as running on Windows. An unavailable
or unreported revision remains unknown, not automatically “up to date”.

Capture both views and the relevant reported revision/source IDs. A screenshot
of equal hashes alone does not prove that instructions ran.

## 2. Channel request and session discovery

From an account with the existing instruction grant, mention the test agent:
“Start a read-only session in this project and summarize its structure. Do not
modify files. Reply here with a link to the session.”

Expected: the request gets a visible disposition in its own thread; the linked
session belongs to that project and is discoverable by the other authorized
account. Send a simple follow-up from the authorized account and verify exactly
one response. Repeat in another thread to check that the contexts stay separate.
An account without the grant must not gain it merely by viewing the session.

Record message, session, target/generation and response IDs. Do not infer success
from a typing indicator, receipt, or spinner alone.

## 3. CI continuation with the provider still running

The test operator registers an exact test CI run and a continuation such as
“Report the CI conclusion and evidence link. Do not modify files.” Confirm the
registration acknowledgement, allow the initiating turn to finish, then publish
the test workflow's terminal result through its genuine producer path.

Expected: the addressed session receives the result and requested continuation
once. Replay the same result and a second registration for the same operation;
neither starts another turn. The CLI status must authenticate the expected
provider and target. This is a developer-driven check; Brian need not assemble
correlation IDs or run the acceptance harness by hand.

## 4. Restart recovery — isolated provider only

Run both cases: restart after registration but before the result; restart after
the result is ready but before continuation admission. Use the real provider,
relay and CLI with an instrumented ACP adapter to establish delivery precisely.

Expected: native restoration retains the original exact target and identity,
followed by one actual prompt. No new-conversation fallback, silent generation
change, duplicate prompt, or provider-key substitution is allowed. Missing
native cursor/custody and refused native restore must produce their documented
outcome. Test the documented post-claim crash window separately; it must not be
mistaken for guaranteed exactly-once delivery.

After automated proof, perform a native UI reconnect test using the packaged
candidate. Opening the old transcript alone does not prove continued execution.

## 5. Declared work visibility — when step 3 is implemented

Publish two scoped work declarations before either participant commits. Open
Pulse from both accounts. Verify participant attribution, declared paths,
branch/base when present, source navigation and age/limitations. Resolve one
declaration with its existing supersession or disposition mechanism and verify
the other work stays visible.

Repeat with identical path names in separate repositories and with a partial
read. Neither case may be reported as a proven shared-file conflict. Opening
evidence or seeing an overlap must not send a turn or create an approval queue.

## Report the result

For each case record: build/relay versions, test accounts and project, steps,
observed result, exact source IDs, and the smallest useful screenshot or log.
Mark not-run cases as not run. Failed UI acceptance becomes a ledger finding;
it does not erase passing protocol evidence or become a reason to repeat all
unrelated tests. Correct the specific failure and repeat the affected path.
