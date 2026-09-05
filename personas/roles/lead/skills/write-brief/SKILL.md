---
name: write-brief
description: "What a lead reads before a batch, and the locked brief template it fills for every lane before dispatch."
---

# Write a brief

A brief is law until you change it in writing. Fill every field; leave nothing
implicit.

## Read the track before the first brief of a batch

Before you write the first brief of a new batch, read exactly two things:

```
sed -n '/^## 3\. Next/,/^## 3a\./p' docs/SESSION_STATE.md   # the ordered track
```

and any next-batch file the founder names. Nothing else of `SESSION_STATE.md` —
it is 5,500 lines and growing, and reading it whole spends a quarter of your
context before you have ruled on anything.

Do this **even when the founder just told you the batch in this session**. A
rule said mid-session is gone the moment your context compacts; a file that a
step of this skill reads is not. That asymmetry is the whole reason the track
lives in a file: what survives is what something re-reads, not what someone
said. If §3 and a remembered instruction disagree, the founder's live word wins
on *what to do* — but read §3 anyway, because it carries the order and the
items the batch depends on.

## The template

```
LANE <id> — <one line>
Tier: 0/1 | 2 — because it touches <what>.
Branch: <topic>/<lane>.
Base: GIT_TERMINAL_PROMPT=0 git fetch origin, then base on
      refs/remotes/origin/main (<sha>) — never local main.
      | you cannot fetch: base on the host's existing refs/remotes/origin/main
        (<sha>, last fetched <when>) and say so in your report.
Worktree: the host makes yours (this is a hire)
      | you already have one at <path> (this brief is a send) — reuse it.
Owns (exclusive): <paths>. Must not touch anything else; if it needs to, STOP and report.
Problem, with evidence: <2–3 file:line entry points, or reproduced output>.
Ledger: §3 Next, plus items <numbers> — read those and nothing else of SESSION_STATE.md.
Design (LOCKED): <decisions, numbered>. Deviations need a written reason in the report.
Contract changes: <exact wire/type deltas, with the doc that must change>.
Seat: <class> · <fast|standard|deep> (risk I×U×I = <n>) · routed by the host
      — model on the create. Review: <required, and which §6 triggers | not required>.
      <challenger sample for this batch, if this is the 5th standard builder job>
      | hire: <role> — when no seat holds this lane yet.
      | send: <role> — when a seat in this umbrella already holds it.
Tests you must add: <named>. Watch each fail before the fix where a defect is claimed.
Acceptance: <bare commands — hermit first on its own line, then one gate per
      command, scoped to the crate touched — with expected counts / exit codes>.
Nest: HOME is the operator's; write nowhere outside this worktree and your seat's state.
Out of scope: <named temptations>.
Report format: see the write-report skill on the builder pack.
Dispatch: bee sessions send --channel <channel-uuid> --session-ref <umbrella-uuid> \
  --to <role> --content -
  | bee sessions hire --channel <channel-uuid> --session-ref <umbrella-uuid> \
      --role <slug> --class <class> --risk <i>,<u>,<r> [--review-flags <f,...>] \
      [--challenger-sample] --brief <this file> — when the Seat line says hire.
```

## The base is `origin/main` after a fetch, never local main

Every lane bases on `refs/remotes/origin/main`, and fetches first
(`GIT_TERMINAL_PROMPT=0 git fetch origin` — the prompt guard, because pushing
and fetching authenticate over NIP-98 and an unset helper otherwise hangs on a
username prompt no agent can answer).

Local `main` is the wrong ref because the operator's hot checkout is where it
lives, and that checkout lags: on 2026-08-28 two lanes based on `5653fbe3` while
`origin/main` was already `4e94261c`, so the base was stale before the first
edit (ledger item 88(j)). Name the SHA you fetched in the brief so the lane can
tell whether its base moved under it.

### The seat that cannot fetch

"Fetch, then `origin/main`" assumes the seat's key can authenticate to the
relay's git host. The relay has no anonymous clone: every request is NIP-98
signed and the key must be a relay member (`BUZZ_REQUIRE_RELAY_MEMBERSHIP` on
hive, §3a) — and **a hired seat's minted identity is not necessarily one**. No
instruction you write changes that; the seat simply cannot fetch.

For such a seat, base on the `origin/main` ref **the host already has**: name
that SHA, say in the brief that it is the host's last fetch and not a fresh
one, and have the seat report the base it could not refresh rather than claim
one it did not fetch. Item 91's seat-git lane is what removes this case; until
it lands, every brief says which of the two it means.

## Say whether the seat gets a worktree

The two dispatch paths differ, and only one of them makes a directory:

- **A hire** — the host creates the seat's worktree, `<session-slug>-<role>-<n>`.
- **A send to a seat that already exists** — no new worktree, no new branch. It
  works in the one it is already in, and it must switch branches there itself.

Write which case applies. "The host makes your worktree" in a brief sent to a
standing seat is an instruction that cannot be followed; ledger item 88(j) is a
seat that switched branches inside its old worktree because its brief said
otherwise.

## Name the query that produced the instruction

Beside every fact a lane will act on, name the command that produced it. If you
cannot name one, you are passing on a memory, and the lane has no way to tell
the difference between that and a fact.

The query also has to answer the question you are actually asking. **"What
would landing this branch do?" is answered by the branch's own commits:**

```
git log --oneline refs/remotes/origin/main..<branch>   # what this branch adds
git diff refs/remotes/origin/main...<branch>           # ...and their net effect
```

Not `git diff main <branch>`, which also reports everything `main` gained since
the branch left — and reports it as a change the branch would *undo*. On
2026-08-29 a lead published a hazard that did not exist, from exactly that
wrong query (ledger draft 91(j)). Two-dot against a moved base is a different
question from the one you meant to ask.

## Carry the cross-lane facts into the brief

Seats cannot see each other's worktrees, branches, or reports. Anything lane A
produced that lane B needs — the SHA A landed at, a symbol A renamed, a
boundary A's verdict moved — exists for B only if you write it into B's brief
or send it. Never brief a lane to "check what the other lane did".

## Evidence is entry points, not an exploration

Two or three `file:line` pointers is the whole evidence budget. **Time-box it:
one read per file you name, and no grep sweep before the first brief.** A lead
spent three and a half minutes exploring before writing brief one, produced no
better a brief for it, and did the builder's job while doing it. If you cannot
name an entry point after one read, that is the missing input — say so in the
brief and let the lane find it.

## Name the ledger items; never send a lane at the whole file

Cite the numbered items the lane actually needs and let it read §3 Next plus
those:

```
grep -n '^79\. ' docs/SESSION_STATE.md               # where the item starts
sed -n '<start>,<start+100>p' docs/SESSION_STATE.md  # read that window only
```

A seat told to "read the ledger" spends about a quarter of its context before it
starts, and a codex seat is already ~25% used at boot (ledger item 80f). If you
cannot say which items a lane needs, that is a brief you are not ready to write
— not a licence to hand over the whole file.

## The dispatch line is part of the brief

End every brief with the exact command that dispatches it, `--session-ref`
included. A role slug is unique only inside one umbrella, so `--to lead` without
`--session-ref` is refused by the CLI — a brief that omits it is a brief nobody
can send.

If the `Seat:` line says `hire:`, the dispatch is `bee sessions hire --brief
<this file>` and the brief *is* the new seat's first turn — do not follow it
with a "start" message. See `skills/hire`.

## Publish the assignment before you hire the seat that answers it

Every hire is preceded by an assignment the seat can cite. A report carries an
`assignmentRef`, and a report whose assignment does not exist in the session is
excluded from the fold on its own account — permanently, no matter what is
published afterwards. So run `bee sessions assign` first, put the returned event
id in the brief, and dispatch the hire second. Live run 2026-09-01: a runner
that had nothing to cite published a report naming an id nobody had signed
(`c737be4c`), and until the fold learned to exclude one bad record on its own
account, that single report made `bee sessions operation list` fail for every
seat in the session.

Two rules the brief should state outright, because a seat will otherwise reach
for the wrong verb:

- **A wrong reference is replaced, never corrected.** `--supersedes` changes an
  operation's wording, never its subject. A report with the wrong
  `assignmentRef` is replaced by a **new** report; a correction that moves the
  subject is refused before signing and, if it reaches the wire, excluded
  `InvalidCorrection`.
- **`mission.blocked` never clears a blocker, and is not a status line.** It is
  a terminal, published only when the mission has actually stopped. Say things
  with `bee sessions note`; ask for a ruling with `bee sessions decide request`
  and clear it with `decide answer`. A `mission.completed` may correct a
  `mission.blocked` by the same author; the reverse is refused.

## Rules for a good brief

- Exclusive file ownership per lane — two lanes never own the same file.
- Name the tier and why; tier-2 briefs must name what makes them tier-2
  (provider runtime, custody/keys, relay ingest, durable state).
- **The Seat line names a class and a risk triple, never a model** — and its
  fast/standard/deep is the routing tier, not the lane's tier-0/1/2 above.
  "builder · fast (risk 2×1×2 = 4) · routed by the host" is a Seat line; a model
  id is not, and neither is a tier on its own. You classify the capability
  required; the router chooses the execution target and writes what it chose
  onto the create — quote that back in the lane's Pulse line.
- Never brief a lane to run a gate you should have hired a runner for; the lane
  runs the tests its own change needs, and `skills/hire` says who runs the rest.
- Acceptance commands are bare and scoped. A pipe, a redirect, a `$(…)` or a
  trailing `; echo` of `$?` earns no observed row, so the push gate cannot read
  the run (live-run finding 77); `cargo test -p <touched crate> --lib` for a
  small change, not the workspace (finding 78). The shape with a worked
  example is `skills/beekeeper-project` § Gates the host can see.
- The brief never names a founder's push as the way anything lands. A refusal
  from the push gate is a fact the lane produces or a blocker it publishes.
- A brief that turns out wrong on the ground is a report back from the lane, not
  a licence for the lane to improvise past it.
