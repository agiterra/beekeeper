---
name: hire
description: "How a lead hires a seat into its umbrella: when to hire, what it runs itself, the exact bee sessions hire command (a class and a risk triple, never a model), the brief-as-first-turn rule, and what every refusal means."
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

## Runner by default: every long gate is a hire

**Any gate that takes longer than a hire round-trip — about two minutes — is a
runner's, never yours.** A full `just ci`, an e2e suite, a release build, a
full-workspace `cargo test`: hire a runner, hand it the exact command, end your
turn. What comes back is one line of exit code and counts, which is the entire
value those minutes produce. Hand it the command **bare** — no pipe, no
redirect, no trailing `; echo` of `$?` — and tell it to activate hermit first
on its own line: a wrapped gate returns a number to you and no observed row to
the push gate (live-run finding 77; `skills/beekeeper-project` § Gates the
host can see).

The reason is cost, not etiquette. Your context is the most expensive on the
team and the only one a hire cannot replace. On 2026-08-28 a lead spent its own
turns re-running a 594-test suite a lane had already run (ledger item 88(e)) and
learned nothing an exit code would not have told it.

Two things stay yours and are never dispatched:

- **The live check** — run the built binary, drive the app against the real
  relay, read the value the change actually produced. That is judgement, not a
  command; `skills/triage-report` makes it mandatory before any APPROVE.
- **The ruling** — the verdict, and the Pulse line that carries it.

A runner tells you *what the gate returned*; you find out *what the change is
worth*. Why a gate went red is the builder's lane, not a second runner's.

## Three decisions before the command

1. **Role.** A pack installed on this computer. Role is fixed per identity — the
   host seats an agent whose *home role* is your slug, so you cannot hire a
   builder and use it as an architect.
2. **Class, risk and review flags.** Run `skills/choose-model`: eleven
   properties, a risk triple, an execution class, the review triggers that
   fire. **You do not choose a model** — the router does, from the live catalog
   and `team/model-registry.yaml`, and it writes back what it chose.
3. **The brief.** Write it with `skills/write-brief`, to a file: that file's
   text becomes the seat's first turn — see below.

## The command

```
bee sessions hire --channel <channel-uuid> --session-ref <umbrella-uuid> \
  --role <slug> --class <class> --risk <impact>,<uncertainty>,<irreversibility> \
  [--profile <json>] [--review-flags <flag,...>] [--challenger-sample] \
  [--provider-instance <ref>] --brief <path>
```

- `--channel` / `--session-ref` — the same pair as `sessions send`; both
  required, because a role slug is unique only inside one umbrella.
- `--role` — lowercase slug, `[a-z0-9-]`, 1–64 chars, matching an installed pack.
- `--class` — the class you classified: `builder`, `architect`, `runner`,
  `verifier`, `ui_designer`, `researcher` (`poker` is lane-drafted, not spec §4).
- `--risk` — impact, uncertainty, irreversibility, each 1–5. The router
  multiplies them and derives the tier and the effort; **you never pass a tier
  and never pass an effort.**
- `--profile` — optional trait floors, your opinion and labelled as such.
- `--review-flags` — the spec §6 triggers you can judge (`securityBoundary`,
  `contractChange`, `outsidePlan`, `builderUncertain`, `testsInsufficient`,
  `leadRequests`); the router adds risk ≥ 40 and irreversibility ≥ 4 itself.
- `--challenger-sample` — every fifth STANDARD builder hire (seed policy).
- `--provider-instance` — optional; the operator's default otherwise.
- `--brief <path>` or `--content <text>` — one of the two, 1–12288 bytes. Prefer
  the file: it is the artefact you can cite later.
- `--no-wait` — publishes and returns; use it only if you will not act on the
  outcome. By default it waits up to 60 s for the create receipt or the refusal.

Exit codes: **0** hired (prints the new seat's target and role), **1** refused
(prints the code), **2** relay error, **5** published but unconfirmed inside
60 s. On 5, do not re-run blind — `bee sessions list` first, or you hire twice.

## Read `granted` before you end your turn

A seat that exists and a seat whose role is authorized are two different facts,
and the output prints both. `outcome: "created"` means the host seated the
agent; **`granted: true` means the hiring CLI appended an accepted NIP-CSAT
`grant-seat` for that exact actor/role after it verified the host's signed
create, receipt, and metadata.** The provider never grants authority.

So:

- **`granted: true`** — normal. End your turn; the report will wake you.
- **`granted: false`** — the seat is live but the exact role-seat transition was
  not accepted. **Do not hire again**: that would create a second lane on one
  brief. Put `seatGrantError`, actor, role, create id, and receipt id on the
  ledger and return it to the founder/operator for authority-chain repair. The
  current `sessions grant` command writes legacy operator/viewer grants; it is
  not a substitute for `grant-seat`, so do not tell anyone to use it as one.
- **`granted: null`** — no seat was created at all (refused, or unconfirmed).
  Read `outcome` and the refusal table below instead.

## Read the routing decision the host wrote

The create the host publishes carries the whole routing record back — the same
object you sent, filled in:

- `chosen` — `{ provider, model, effort }`, the execution target it seated.
- `runnerUp` — the target that would have run instead, or `null`.
- `reason` — one sentence naming the gates it cleared and why it was cheapest.
- `reviewRequired` / `reviewReasons` — including the two the router computes.
- `challengerSample`, `registryVersion`, `catalogRevision`.

Quote `chosen` and `reason` in the lane's Pulse line. That record is the only
honest answer to "why is this seat on this model", and a routing decision that
cannot be explained from the wire is a bug, not a detail.

**`--model` is a human-grade override, not a preference.** It requires
`--because`, it is recorded on the routing record and on the ledger, and it must
be justified in the brief in the same sentence that uses it. A model id is an id
a runtime's own kind:44222 catalog publishes in `allowedModels`, taken exactly as
`bee sessions catalog` prints it — there are no aliases anywhere in this path, so
a bracket-suffixed id and its bare form are two different ids, and an id the
catalog does not offer is disclosed and refused, never mapped onto anything
close.

## The brief is the first turn

The host publishes the seat's create with your brief text as its initial turn,
prefixed `[From the lead] `. The seat wakes holding the whole brief.

- **Do not send a second "start" or "here is your brief" turn.** It is a
  duplicate, it costs the seat context, and it teaches it to wait for a nudge.
- **End your turn after hiring**, exactly as after a dispatch: the report wakes
  you as an addressed turn, and polling `bee sessions inbox` inside the hiring
  turn buys nothing and invents duplicates.
- A correction to a brief already sent is one `sessions send`, never a re-hire.

## Refusals

A refusal comes back twice: exit 1 with the code on stdout, and a 44220 turn
reading `hire refused: <code> — <reason>`. The seat was never created.

| code | what happened | what you do |
| --- | --- | --- |
| `HIRE_OFF` | the operator has hiring switched off | Ask the operator to turn hiring on. Do not retry. |
| `HIRE_ROLE_NOT_ALLOWED` | that role is outside the operator's allowed roles | Hire an allowed role, or ask the operator to allow this one. |
| `HIRE_LIMIT` | the umbrella is at its live-seat ceiling (default 4) | Close or finish a seat, or ask the operator to raise the limit. |
| `HIRE_ROLE_BUSY` | an identity of that role exists, but it is already seated in this umbrella | Do not hire. Send this same brief to that seat: `bee sessions send --session-ref <umbrella-uuid> --to <role> --content -`. It keeps the worktree it is already in. |
| `HIRE_NO_IDENTITY` | no installed agent on this computer has that home role | Stop and tell the founder **which role to install** (Agents → *Install team roles…*). Never substitute another role for the lane. |
| `HIRE_PROVIDER_NOT_ALLOWED` | the named provider is outside the allowed providers | Re-run naming an allowed provider, or drop `--provider-instance` and take the default. |
| `HIRE_MODEL_NOT_OFFERED` | an overriding `--model` named an id that runtime's catalog does not offer, and nothing translates it into one | The reason lists every offered id. Re-run with one of them, or drop the override and let the router route. |
| `HIRE_NO_ROUTE` | no execution target cleared every gate for this class, risk and requirement set | Requirements are never silently weakened. Change the class, split the task (`skills/choose-model` §3), or put it to the founder. Never retry unchanged. |
| `HIRE_STALE` | the request sat unanswered longer than the host's window (15 minutes) — the operator's computer was shut or offline | Hire again. Do not assume the first one lands late; it will not be answered at all. |

`HIRE_ROLE_BUSY` is a code the host really answers with (ledger item 90): its
policy returns it whenever this computer holds the role and every identity of
it is already seated here, and the reason names those seats. It is not `HIRE_NO_IDENTITY`, and its remedy is never
"install a role you already have" — send this brief to the seat the reason
names.

Never retry a refusal unchanged. Put it on the ledger as a `blocker` Pulse entry
with its code, then either change the request or `BLOCK: missing-input = <the
one thing, and who fetches it>`.

## When the relay is too old

A relay predating `session.hire` rejects the request as malformed and the CLI
says `this relay does not accept hire requests yet`. That is exit 2, not a
policy refusal: nothing reached the operator, no seat was considered. Report it
— the operator deploys a relay carrying the action — and seat by hand via **Add
provider** meanwhile. Never read it as "the role is unavailable".

## What a hire cannot do

- It cannot mint an identity. The host seats an installed one; custody stays
  with the host and you never see, hold, or pass a key.
- **It cannot choose a working directory, and the worktree it makes belongs to a
  hire alone.** For a hire the host creates one, named
  `<session-slug>-<role>-<n>`. A brief *sent* to a seat that already exists
  reuses whatever worktree that seat is in — no new directory, no new branch —
  so name which of the two cases the brief is (`skills/write-brief`). Telling a
  re-briefed seat "the host makes your worktree" is a false instruction; ledger
  item 88(j) records a seat that switched branches inside its old worktree.
- It cannot reach an umbrella where you hold no hiring authority. The founder
  and an active operator may hire any role; an active `lead` seat may hire only
  non-lead roles. A revoked, stale, wrong-session, or wrong-genesis seat grants
  nothing.

## Assign first, then hire — in that order, on the wire

Publish the assignment before you hire the seat that answers it: bee sessions assign first, its event id in the brief, bee sessions hire second. A hire that arrives with no assignment to cite leaves the seat two bad options — invent a reference, or report nothing — and the live runs produced both.

This rule is written in `skills/write-brief` and in `skills/triage-report`, and
live run 3 still hired at 12:32:50 and assigned at 12:33:37 — because the skill
a lead loads *while hiring* is this one, and this one used to say nothing about
it. The order is visible on the wire forever: the Route rail draws
hire → assignment, and the seat's first turn cites an id that did not exist
when it was written.
