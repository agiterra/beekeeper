---
name: hire
description: "How a lead hires a seat into its umbrella: when to hire, the exact bee sessions hire command, the brief-as-first-turn rule, and what every refusal means."
---

# Hire a seat

Launching a team seats **you and nobody else**. The roster on the Team tab is
the list of seats you *may* hire, not seats that exist. Every other seat comes
from you, one hire at a time, after you have heard the mission.

## When to hire

Hire when a lane exists in your plan, has exclusive file ownership, and you can
brief it in one file. That is the whole test.

Do not hire when:

- a seat already in this umbrella holds that role — `bee sessions send --to <role>` instead;
- you want an answer, not a lane — ask the seat you have;
- two lanes would own the same file — that is one lane, not two hires.

One hire per lane, one brief per hire.

## Three decisions before the command

1. **Role.** A role pack installed on this computer. Role is fixed per identity:
   the host seats an installed agent whose *home role* is your slug. You cannot
   hire a builder and use it as an architect.
2. **Model and vendor.** Run `skills/choose-model` and write the one-clause
   reason into the brief. A refuter must not share its builder's vendor.
3. **The brief.** Write it with `skills/write-brief`, to a file. That file's
   text becomes the seat's first turn — see below.

## The command

```
bee sessions hire --channel <channel-uuid> --session-ref <umbrella-uuid> \
  --role <slug> --provider-instance <ref> --model <id> --brief <path>
```

- `--channel` / `--session-ref` — the same pair as `sessions send`: the channel
  the session lives in and the umbrella you lead. Both required; a role slug is
  unique only inside one umbrella.
- `--role` — lowercase slug, `[a-z0-9-]`, 1–64 chars, matching an installed pack.
- `--provider-instance` — optional. Omit it and the host uses the operator's
  default provider. Name one when `choose-model` picked a vendor your own
  runtime cannot run (a Codex architect beside Claude builders).
- `--model` — optional. Omit it and the seat runs the identity's own model.
- `--brief <path>` or `--content <text>` — one of the two, 1 byte to 12288 bytes.
  Prefer the file: it is the artefact you can cite later.
- `--no-wait` — publishes and returns. Use it only if you are not going to act
  on the outcome; by default the command waits up to 60 s for the seat's create
  receipt or the refusal.

Exit codes: **0** hired (prints the new seat's target and role), **1** refused
(prints the code), **2** relay error, **5** published but unconfirmed inside
60 s. On 5, do not re-run blind — `bee sessions list` first, or you hire twice.

## The brief is the first turn

The host publishes the seat's create with your brief text as its initial turn,
prefixed `[From the lead] `. The seat wakes holding the whole brief.

- **Do not send a second "start" or "here is your brief" turn.** It is a
  duplicate, it costs the seat context, and it teaches it to wait for a nudge.
- **End your turn after hiring**, exactly as after a dispatch. The report comes
  back as an addressed turn that wakes you. Polling `bee sessions inbox` inside
  the hiring turn buys nothing and invents duplicates.
- A correction to a brief you already sent is one `sessions send`, never a
  second hire.

## Refusals

A refusal comes back two ways: exit code 1 with the code on stdout, and a 44220
turn addressed to you reading `hire refused: <code> — <reason>`, which the
umbrella also shows as a system line. The seat was never created.

| code | what happened | what you do |
| --- | --- | --- |
| `HIRE_OFF` | the operator has hiring switched off | Ask the operator to turn hiring on. Do not retry. |
| `HIRE_ROLE_NOT_ALLOWED` | that role is outside the operator's allowed roles | Hire an allowed role, or ask the operator to allow this one. |
| `HIRE_LIMIT` | the umbrella is at its live-seat ceiling (default 4) | Close or finish a seat, or ask the operator to raise the limit. |
| `HIRE_NO_IDENTITY` | no installed agent has that home role, or the only one is already live here | Ask the operator to **install team roles** (Agents → Install team roles…), or hire a role that is installed and free. |
| `HIRE_PROVIDER_NOT_ALLOWED` | the named provider is outside the allowed providers | Re-run naming an allowed provider, or drop `--provider-instance` and take the default. |

Never retry a refusal unchanged. Put it on the ledger as a `blocker` Pulse entry
with its code, then either change the request or `BLOCK: missing-input = <the
one thing, and who fetches it>`.

## When the relay is too old

`session.hire` is a new lifecycle action and the relay validates lifecycle
payloads with exact keys. A relay that predates it rejects the request as
malformed, and the CLI says so plainly:

```
this relay does not accept hire requests yet
```

That is exit 2, not a policy refusal: nothing reached the operator and no seat
was considered. Report it — the operator has to deploy a relay that carries the
action — and seat by hand via **Add provider** meanwhile. Never read that
sentence as "the role is unavailable".

## What a hire cannot do

- It cannot mint an identity. The host seats an installed one; custody stays
  with the host and you never see, hold, or pass a key.
- It cannot choose a working directory. The host makes the seat its own
  worktree, named `<session-slug>-<role>-<n>`.
- It cannot reach an umbrella you do not lead. Authority is the session's
  founder or a granted operator — the same rule as steering.
