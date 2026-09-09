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

## 6. Handover — continuing an absent participant's work

The mechanisms are in `docs/HANDOVER_IMPL.md`. The automated composition is
`just test-handover` (`scripts/handover-acceptance.sh`): it runs two providers
as two processes on one host, against a stub ACP adapter. That is what it can
prove. This section is the part it cannot — two machines, two people, two
clocks, and a real agent — so run it here rather than reading the script's PASS
lines as cross-machine evidence.

Use A = Brian's Mac and B = the Windows machine signed into hive, on a
deliberately chosen test project and channel, with a relay-hosted repository
both can push to (`bee git setup`; `bee git status` says whether NIP-98
credentials are wired). Record both installed builds and the relay build
before starting. Use an isolated provider on A: several steps kill it.

**6.1 Found and grant.** On A, start a session in the test project and send it
one turn. From A, grant B `collaborator` on that session. Expected: B can see
the session, and the grant is visible on the accepted chain
(`bee sessions roster`), not merely in a UI. Record the session reference, the
genesis id, and the exact target of A's execution.

**6.2 Checkpoint.** On A's checkout, leave the work genuinely unfinished: a
commit, a staged file, an unstaged edit, an untracked file, and a binary file.
Run `bee sessions handover checkpoint` with a real `--task`, `--next` and at
least one `--unresolved`. Expected: the wip ref lands on the relay (the
printed sha appears in the repository's kind:30618 ref state), the checkpoint
reports `preserved: all`, and every artifact line names something you can
fetch. Add a file larger than 256 KiB and checkpoint again: expected
`preserved: partial` with that path named under `missing`. **A checkpoint that
says `all` while a path is missing is a defect, not a rounding error** — the
whole point of the field is that the next participant can trust it.

**6.3 A goes away.** Force-quit A's provider (do not close the session
cleanly). Wait for the session's execution to stop reading `live` — the
relay serves lease state from a snapshot with a three-minute TTL, so B will
see `live` for a while after A's machine is gone, and acting before it lapses
tests nothing.

**6.4 B continues.** Set B up so the answer cannot be accidental: B's provider
must already be configured to run this project and channel in **some other
folder** — its ordinary working directory, a plain clone with none of A's work
— and B recovers into a *different*, fresh checkout. If both are the same
folder, every check below passes whether or not the reconstruction placed
anything, which is exactly how this went unnoticed once already.

On B, run `bee sessions handover continue --cwd <fresh checkout>
--projects-file <B's provider projects file>` (or with `BUZZ_CSP_PROJECTS_FILE`
set). Pass **no** mode flag: the point is that the default picks reconstruction
because nothing is reachable.

Expected: the claim is accepted (one relay receipt naming B as claimant and
B's provider as the body); B's checkout lands on `handover/<first 8 of the
session reference>` at the checkpoint's head with the staged, unstaged,
untracked and binary bytes back; a new execution joins the *same* session
reference on B's provider; and its first turn carries the checkpoint's task and
next action. Open the file with the binary content and confirm the bytes, not
the file name.

Then confirm the work is where the agent can see it, which is a separate
question from whether it was fetched:

- a one-shot hint file appeared at
  `<directory of the projects file>/pending-hints/<the create's commandId>.json`
  naming the recovered checkout, while the projects file's own
  `projects`/`channels` entries still point at B's ordinary folder — and the
  **projects file itself is unchanged**, byte for byte. The CLI does not write
  to it. That is deliberate: the binding used to live inside `projects.json`,
  where any unrelated desktop save between writing it and the provider
  admitting the create erased it, after which the model opened in B's default
  folder and the continuation still said "recovered". If you want to see the
  fix work, **save something in the desktop while the reconstruction is
  running** — a project rename, anything that rewrites that file — and check
  the outcome below is unaffected;
- after the session appears, the hint file is **gone**. It is consumed on
  admission; one left behind would bind some later create nobody pointed at
  that folder;
- the session's own view reports the recovered branch and commit — not B's
  default folder's — in the execution's status;
- ask the agent, in its first reply, what branch it is on and whether it can
  see the uncommitted file. **A continuation that says "recovered" while the
  model is looking at an untouched tree is the failure this step exists for**,
  and it looks like success from every other angle.

Run it once **without** the projects file too. Expected: a refusal that names
the remedy, and no claim — check the authority chain did not grow. A run that
fences the absent participant and then discovers it cannot place the work has
taken the session away for nothing.

Then work in it. The reconstruction is a **new execution** — the original
agent's native context stayed on A's disk — so read the first agent reply for
whether the checkpoint was actually enough to carry on from. That judgement is
the part no harness makes.

**6.5 A comes back.** Restart A's provider. Expected: A's session shows as
disconnected **and says who took it over**, in the same view, without a second
lookup. A turn A queued while its provider was down, and a fresh turn A sends
now, are both refused `HANDOVER_FENCED` with B named. A sibling execution of
the same session — one the handover never mentioned — is fenced too: v1 hands
over the whole session, and the surfaces should say so rather than leaving A
to discover it by being refused.

**6.6 Racing claims.** With both providers up, have A and B claim the same
session at the same moment. Expected: exactly one accepted claim, and the
loser is told who won by name. A second accepted claim, or a loser told only
"failed", is a finding.

**6.7 Revoke and regrant.** From A, revoke B's grant. Expected: the session
reads voided — not "back to normal". Regrant B the same tier. Expected: still
voided, and a real turn from B to the execution it built is still refused. A
regrant must never silently restore a claim. A fresh takeover by A then lifts
the fence for A and keeps it up for B.

**6.8 Retirement.** Delete the session from A. Expected: B's provider, on
restart, publishes nothing for its execution and answers `SESSION_RETIRED`;
A's already-running provider answers the same without a restart; and the
handover view says deleted, offering nothing. Nothing is republished to make
a deleted session resumable.

**6.9 The native leg.** With A's provider alive and B holding a grant, run
`bee sessions handover continue --native` from B. Expected: B's next action
runs on A's own execution, **in A's own working directory** — nothing is
fetched, nothing is placed, and no folder changes — and the record says
`native-resume`, never `reconstructed`. A's own turn is then fenced until A
takes the session back.

Note what taking it back does and does not do: the fence lifts, but an
execution whose provider died does not come back to life with it. Expect a
named refusal (`NO_LIVE_EXECUTION`) rather than a running turn, and re-address
the owed turn rather than assuming the session resumed.

**6.10 Native Windows — DEFERRED.** Everything above assumes B's provider runs
where its checkout is. A native Windows provider (paths, credential helper,
git line endings on the reconstructed patch) is **not accepted** and is not
claimed to work; run 6.4 from Windows only to record what happens, and file
what you find. Do not mark this section passed on a WSL or mock-UI run —
`docs/HANDOVER_IMPL.md` §9 lists it as deferred and it stays deferred until
somebody has done it on the metal.

Record, for every step: the event ids (checkpoint, claim receipt,
continuation), the exact targets before and after, the branch and sha B's
checkout landed on, **the directory the agent actually ran in**, and the
refusal codes you saw with your own eyes. A UI
that renders a fence is not evidence that a provider enforced one.

## Report the result

For each case record: build/relay versions, test accounts and project, steps,
observed result, exact source IDs, and the smallest useful screenshot or log.
Mark not-run cases as not run. Failed UI acceptance becomes a ledger finding;
it does not erase passing protocol evidence or become a reason to repeat all
unrelated tests. Correct the specific failure and repeat the affected path.
