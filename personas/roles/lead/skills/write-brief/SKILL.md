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
Worktree: the host makes yours (this is a hire)
      | you already have one at <path> (this brief is a send) — reuse it.
Owns (exclusive): <paths>. Must not touch anything else; if it needs to, STOP and report.
Problem, with evidence: <2–3 file:line entry points, or reproduced output>.
Ledger: §3 Next, plus items <numbers> — read those and nothing else of SESSION_STATE.md.
Design (LOCKED): <decisions, numbered>. Deviations need a written reason in the report.
Contract changes: <exact wire/type deltas, with the doc that must change>.
Seat: <model + thinking level, and why> — see the choose-model skill.
      | hire: <role> on <provider>/<model> — when no seat holds this lane yet.
Tests you must add: <named>. Watch each fail before the fix where a defect is claimed.
Acceptance: <commands with expected counts / exit codes>.
Out of scope: <named temptations>.
Report format: see the write-report skill on the builder pack.
Dispatch: bee sessions send --channel <channel-uuid> --session-ref <umbrella-uuid> \
  --to <role> --content -
  | bee sessions hire --channel <channel-uuid> --session-ref <umbrella-uuid> \
      --role <slug> --brief <this file> — when the Seat line says hire.
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

## Say whether the seat gets a worktree

The two dispatch paths differ, and only one of them makes a directory:

- **A hire** — the host creates the seat's worktree, `<session-slug>-<role>-<n>`.
- **A send to a seat that already exists** — no new worktree, no new branch. It
  works in the one it is already in, and it must switch branches there itself.

Write which case applies. "The host makes your worktree" in a brief sent to a
standing seat is an instruction that cannot be followed; ledger item 88(j) is a
seat that switched branches inside its old worktree because its brief said
otherwise.

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

## Rules for a good brief

- Exclusive file ownership per lane — two lanes never own the same file.
- Name the tier and why; tier-2 briefs must name what makes them tier-2
  (provider runtime, custody/keys, relay ingest, durable state).
- Name the seat's model and the reason in one clause. "Sonnet, because this is a
  two-file mechanical edit" is a reason; "Sonnet" is not.
- Never brief a lane to run a gate you should have hired a runner for; the lane
  runs the tests its own change needs, and `skills/hire` says who runs the rest.
- A brief that turns out wrong on the ground is a report back from the lane, not
  a licence for the lane to improvise past it.
