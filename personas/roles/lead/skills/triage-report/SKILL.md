---
name: triage-report
description: "How to read a lane's report and refuter verdict, check the live value yourself, and dispose of it."
---

# Triage a report

Read the report, not the transcript. Read the diff, not the exploration.

## The report you're reading

```
Branch + HEAD SHA, rebased on main @ <sha>.
Files touched (each: added/modified, one line why).
Tests: names + counts, the command, exit code. Red-before-green: which test, what it said.
Deviations from the brief and why.
Residuals: what could not be verified on this host, named.
Anomalies: anything surprising, even if unrelated.
```

## The live-value check — mandatory before APPROVE

Before any `APPROVE` or `APPROVE-WITH-NOTES`, **run the lane's acceptance yourself and read the value it produced**: run the binary, hit the wire, look at the field. Not the lane's transcript of having done it — the output, on this host, now.

```
cargo run -p buzz-cli -- <the lane's own acceptance command>   # and read the field
bee events query --kinds <n> --channel <uuid> --format compact # and read the tag
```

This exists because a lead approved a founder column on a clean report and green tests; the column printed the *provider's* pubkey, and the operator caught it. Tests proved the code did what the code did. Nobody had looked at the value.

If the acceptance cannot be run on this host, that is not an approval with a note — it is `BLOCK: missing-input = <what would let it run, and who fetches it>`.

## Tiers

- Tier-0/1 (tests, fixtures, docs, types with no runtime change): your read of the report, a hunk-level look at the diff, and the live-value check are enough to merge.
- Tier-2 (provider runtime, custody/keys, relay ingest, durable state): wait for a refuter's terminal verdict before you merge, no exceptions.

## Verdicts

- `APPROVE` — merge. Only after the live-value check.
- `APPROVE-WITH-NOTES` — merge; the notes are record, not a condition on landing.
- `BLOCK: missing-input = <the one thing, and who fetches it>` — the lane stops here until that input exists.

A report without a command's exit code, a SHA, or a `file:line` did not happen — send it back.

## A wrong reference is replaced, never corrected

`--supersedes` changes an operation's **wording**, never what it is about. A
report that cites the wrong `assignmentRef` is replaced by a **new** report
citing the right one; a correction that moves the subject is excluded
`InvalidCorrection` and the original stays wrong beside it. On 2026-09-01 a
lead's acceptance step asked the runner for exactly that correction, the runner
published it (`46b03d08`), and the fold failed closed for every seat until the
session was repaired. `bee sessions report|verdict|acknowledge|block|complete`
now refuse the shape before signing, naming the id and the rule — when you see
that refusal, ask for a new record, not a rewritten one. The same holds for
every id an operation cites: an operation may only reference a record this
session actually holds and the fold includes.

## `mission.blocked` is a terminal, and a terminal never clears itself

Publish `mission.blocked` only when the mission has actually stopped. It is not
a status line and not a note: on 2026-09-01 it was used four times to say
"still working", the Mission rail read **Blocked** in red for 4 h 38 m while two
seats worked, and nothing in the vocabulary could take it back. To say something
without changing state, use `bee sessions note`. To clear something a person
must rule on, use `bee sessions decide request` and then `decide answer` — a
blocker is cleared by the ruling that unblocks it, never by another terminal.
When the mission really does finish after a blocked, `bee sessions complete`
now supersedes your own newest `mission.blocked` for you, so the fold sees one
corrected terminal rather than a conflict; a completion may correct a blocked,
and never the reverse.

## Every hire is preceded by an assignment the seat can cite

A seat's first report needs an `assignmentRef`, and a report whose assignment
was never published is excluded from the fold forever. So publish the
assignment (`bee sessions assign`) **before** you hire the seat that will
answer it, and put its event id in the brief. A hire that arrives with no
assignment to cite leaves the seat two bad options: invent a reference, or
report nothing at all — and the live run produced both.

## Publish the disposition — twice, or it did not happen

A verdict has two addressees and therefore two publishes. **The turn is not
over until both are on the wire.**

1. **To the seat**, so it can act on the ruling:

```
bee sessions send --channel <channel-uuid> --session-ref <umbrella-uuid> \
  --to <role> --content "APPROVE — <lane> @ <sha>. <next>"
```

2. **To the ledger**, so the next seat can read it:

```
bee pulse update --project <coordinate> --kind milestone --session <umbrella-uuid> \
  --cost-from <channel-uuid>:<umbrella-uuid> --cost-seat <builder-pubkey8> \
  --content "<lane> — <verdict> @ <sha> — <next>"
```

`--kind blocker` for a `BLOCK`. The team's ledger is the relay; a disposition
that lives only in your context is lost the moment the seat ends.

**Every milestone and blocker for a lane carries `--cost-seat <builder>`**, so
the ledger says what that lane cost as well as what it produced. The
mission-complete milestone carries `--cost-from <channel-uuid>:<umbrella-uuid>`
with **no** `--cost-seat`, so it totals the whole session's seats.

The numbers are folded from the turn usage the providers signed — nothing is
estimated. A read that finds none prints `"cost": null` with a `costNote` and
publishes the entry without a cost; do not substitute a number of your own.

Writing the verdict in your own transcript is neither publish. To the seat it
is indistinguishable from a lead that has stopped working — on 2026-08-29 an
`APPROVE` sat in a lead's transcript for ninety minutes while the lane it
approved waited, and it read as a stall (ledger draft 91(h)). Send first, then
Pulse, then end the turn.

Cite the `docs/SESSION_STATE.md` item number the disposition settles. That number is what the next seat reads — it reads §3 Next plus the items you cite, never the whole 5,500-line file (ledger item 80f).

## A ruling is a verb, not a sentence in a turn

Answer a request with bee sessions decide answer, never in a turn. Prose to the asker leaves the request open on the wire, the mission waiting on you, and the ruling somewhere no fold can read.

Live run 2, 11:33: a builder asked the same question twice; the lead had
answered the first in prose, so the request stayed open, `waitingOnDecision`
kept naming it, and the founder had to answer both. See
`skills/ask-for-a-ruling` for the shape of the answer itself.

## A report's gate claims are not evidence

A report whose gate claims are prose is not accepted. Ask for the signed row — bee sessions observe gate — and rule on that. This rule applies to any session where kind 44246 rows are on the wire; where none are, say in the disposition that the claim is unverified rather than accepting it.

Live run 3, finding 26: a builder's report said `cargo test -p buzz-cli` was
green after the rebase. A verifier reproduced two failures on the same SHA, and
the builder's "tests exit 0 (13 passed)" turned out to be one test file it had
chosen to run. A gate is the command, its exit code and its tail, signed —
or it did not happen.
