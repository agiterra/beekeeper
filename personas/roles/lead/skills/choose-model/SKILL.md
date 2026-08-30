---
name: choose-model
description: "The versioned rubric a lead picks a hire's model from: real catalog ids, checked against the live catalog before every batch, never an alias and never \"default\"."
---

# Choose a model for a seat

One table, real catalog ids, one sentence of justification. The rubric names ids
the providers on this computer actually publish, because that is the only list a
hire can be answered from: there is no alias translation anywhere in the path,
so a name that is not in the catalog is refused, not mapped onto something
close. Read the rubric, run the check, then write the reason into the brief.

## The rubric

**Version 3 — 2026-08-30.** Ids as `claude-primary` and `codex-primary`
published them in their kind:44222 catalogs, recorded in ledger item 88(a)
(claude-primary, catalog revision 4) and item 88(i)/92 (codex-primary). **This
version was written from the ledger, not from a live read** — the relay refused
an unauthenticated 44222 query at the time it was written, so treat the first
`rubric check` of your batch as the thing that confirms it, not as a formality.

```rubric v3
| tier | role(s) | provider | model id | reason |
| --- | --- | --- | --- | --- |
| frontier | lead | claude-primary | claude-fable-5[1m] | every brief, report and diff of a whole batch has to sit in one context and still be ruled on; the million-token window is the capability that decides it |
| tier-2 | builder on a tier-2 lane — provider runtime, custody/keys, relay ingest, durable state | codex-primary | gpt-5.6-terra[high] | the design is still being discovered on the ground, over a long tool-use chain that must keep a wire contract straight; it is also the frontier row on the other provider, which is what lets a verifier be hired off the builder's blind spot |
| frontier | verifier over a tier-2 diff | claude-primary | opus[1m] | an adversarial pass needs frontier reasoning and a different provider from the builder it is checking; if the builder took a claude-primary row, the verifier takes the codex-primary one instead |
| tier-1 | builder on a one-file lane against a locked design, tests named | claude-primary | sonnet | the decisions are already made in the brief; what is left is the edit and the named tests, which is the cheapest seat that does both reliably |
| tier-0 | docs, ledger, runbook | claude-primary | haiku | it edits prose against a cited file:line and copies exact text; no design judgement is in the lane |
| tier-0 | runner | claude-primary | haiku | the entire answer is an exit code and a count, so the capability that matters is running a command and copying its output verbatim |
| frontier | poker | claude-primary | opus[1m] | it must read its own screenshots — a walk it cannot see is a walk it invents — so the row has to be multimodal, and a full walk is long enough to need the large window |
| frontier | designer | claude-primary | opus[1m] | it drives the real app and reads back what rendered, so multimodal is the gate, and a surfaces spec is one long context of screens, copy and event kinds |
```

Minimum means minimum. Hiring under the row is how a lane returns a confident
report about work it could not do; hiring over it is only money.

**The gate row is never you.** Any gate longer than a hire round-trip (~2 min)
— a full `just ci`, an e2e suite, a release build, a full-workspace `cargo test`
— goes to the runner row, because the whole answer is an exit code and a count.
Your own context is the frontier row and unhireable: spend it on the live check
(the built binary, the real relay, the value the change produced) and the
ruling. On 2026-08-28 a lead re-ran a 594-test suite a lane had already run and
bought nothing with those turns (ledger item 88(e)). `skills/hire` has the split.

## Which row: classify the task first

- Mechanical edit with the answer already in the brief (rename, field add,
  fixture) → the tier-1 row; a docs-only edit → the tier-0 docs row.
- Run a gate and report exit codes and counts → the runner row, always a hire.
- One-file implementation against a locked design, tests named → tier-1.
- Multi-file feature, a contract change, or a design still being discovered on
  the ground → the tier-2 row.
- Provider runtime, custody/keys, relay ingest, durable state → the tier-2 row,
  by definition.
- Adversarial pass over a tier-2 diff → the verifier row, on the provider the
  builder did not use.
- Orchestration — briefs, triage, verdicts across a run → the lead row, which is
  you, and is not hired.

## (a) Check the rubric against the catalog, every brief

Before the first brief of a batch:

```
bee sessions rubric check
```

- **exit 0 — clean.** Every model id in the table is offered by the live
  catalog, and every offered id is assigned to a row. Pick a row and hire.
- **exit 4 — stale.** The output names both halves: ids the table uses that no
  provider offers any more, and ids a provider offers that no row assigns.

**A bracket suffix is a variant of its base.** `gpt-5.6-sol[high]`, `[low]`,
`[max]`, `[ultra]` are one model at four effort levels; `opus[1m]` and `opus`
are one model at two context windows. The rubric decides at the base, so the row
naming `gpt-5.6-terra[high]` has assigned every `gpt-5.6-terra` row in the
catalog and `unassigned` reports one gap per base, not one per suffix. The check
still hides nothing: the offered ids no row names literally are listed under
`variants`, which is informational and never makes the rubric stale. `default`
is a runtime alias, not a model, so it is never a gap either — `defaultResolvesTo`
says which id it points at on each provider, and on `claude-primary` and
`goose-primary` today it points at the string `default`, meaning the catalog
declines to name a concrete id.

Read the `unassigned` entries literally: each one is an id the catalog really
offers, so it can be pasted into a new row as it stands.

Any other exit is the CLI's usual class (1 input, 2 relay, 3 auth) and is **not
a verdict on the rubric** — fix the call and re-run; do not read a network error
as "clean".

**Stale is not a licence to guess.** Do not substitute a nearby id, do not fall
back to the identity's model to dodge the question, and never write `default` —
that is a label, not a model. In the same turn:

```
bee pulse update --project <coordinate> --kind blocker --session <umbrella-uuid> \
  --content "rubric stale: <ids not offered> / <ids unassigned> — proposed: <the row you would add>"
```

and ask the founder, in that same turn, to rule on the proposed row. Then carry
on with any row whose id the check confirmed is still offered. A batch is only
blocked if the row you need is the stale one.

## (b) The identity's own record wins

If the host has set a model and runtime on the identity you are hiring, **that
record wins for that identity** and the rubric is not consulted. The rubric
decides only when the record is blank. The founder set that record deliberately
— the designer identity seated on `codex-primary` is exactly this case (ledger
item 92) — and a hire that overrode it with `--model` would be substituting your
guess for the founder's decision. Omit `--model` and say in the brief that the
seat runs the identity's own record.

## (c) A new model, and nobody has assigned it

The check reports an offered id no row assigns. Propose one row — tier, roles,
provider, id, and the capability that earns it — post it as the stale note
above, and let the founder rule. **A docs lane lands the row**; you do not edit
the rubric mid-batch, because a rubric that changes under a running batch cannot
explain why any seat in it was hired.

## (d) The trigger is the catalog revision

The rubric goes stale when a provider publishes a new kind:44222 revision — a
runtime upgrade, a model added or withdrawn, a provider added to this computer.
You cannot see that happen, so do not wait to be told: the check is one cheap
call, and it runs at the start of every batch, before the first brief. That is
the whole update trigger. A rubric nobody checked is a rubric that can be wrong
without saying so.

## Thinking level

Raise thinking, not tier, when the work is one hard decision inside an otherwise
ordinary task. Raise tier when the work is many decisions. On `codex-primary`
the level is part of the id (`[low|medium|high|xhigh|max|ultra]`), so raising it
means naming a different catalog id — and that id must be offered too.

## Say why, in the hire

Every hire and every brief names the row and the reason in one clause:

```
Seat: <tier> · <provider>/<model id, exactly as the catalog prints it> · thinking <level>
      — because <the task class that picked this row>, <modality if it decided anything>.
```

"Because the brief is locked and this is a two-file mechanical edit" is a
reason. The model name alone is not. If you cannot write the clause, you have
not classified the task yet — do that first. Never write an alias, a vendor
marketing string, or `default`: `skills/hire` has what a refusal will say.
