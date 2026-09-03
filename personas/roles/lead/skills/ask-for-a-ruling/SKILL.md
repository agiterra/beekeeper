---
name: ask-for-a-ruling
description: "How a lead asks a person for a decision and answers one it is holding: the two verbs, who holds what, and why a ruling names a condition rather than a commit."
---

# Ask for a ruling, and answer one

Two verbs and nothing else: `bee sessions decide request` asks a named party
for a ruling the mission needs, and `bee sessions decide answer` gives one.
Both are signed records the fold reads. A turn is neither.

## Ask when the mission cannot move without a person

Ask when the answer is not yours to give: an irreversible act the policy
reserves to the founder, a trade-off between two acceptable designs, a blocker
outside the repository. Do not ask for something a seat can measure — that is
an assignment, not a question.

Name who holds it. `--held-on founder` is the default; `--held-on <pubkey>`
puts it on a specific actor, and the CLI wakes the seat that actor is sitting
in. State a recommendation: a question with no recommendation makes the
answerer do the lead's work.

## Answer with the verb

Answer a request with bee sessions decide answer, never in a turn. Prose to the asker leaves the request open on the wire, the mission waiting on you, and the ruling somewhere no fold can read.

The fold reads only the signed answer. Until it lands, the request is the
mission's `waitingOnDecision` and a `mission.completed` naming an assignment
that request blocks is excluded.

## Rule on the class, not on the commit

If the honest answer would have to be given again for the next commit, ask for a condition rather than a commit: state the class the ruling covers and pass it back with bee sessions decide answer --condition. A per-SHA ruling is a question you have agreed to ask again.

```
bee sessions decide answer --channel <uuid> --session-ref <uuid> \
  --genesis <hex> --request <hex> --choice-index 0 \
  --condition 'any SHA whose buzz-acp diff against origin/main is empty'
```

`--condition` is bounded text, at most 512 bytes. It is **read by people, not
by the fold**: nothing evaluates it, nothing enforces it, and no surface parses
it into state. Its whole job is to be findable — the next seat that would have
asked reads the condition, checks it itself, and does not ask.

Live run 2, 11:33 (finding 21): the same blocker came back with a new SHA and
the founder was asked for the identical ruling a second time. The first answer
had been about a commit. It should have been about the class.
