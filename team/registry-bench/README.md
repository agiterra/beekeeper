# `team/registry-bench` — where a registry row's numbers come from

`team/model-registry.yaml` ships ten 1–5 priors per row, and its own head
comment says what they are: *"Every one of them is an opinion."* Live run 3
(finding 25) is what that costs. The router disclosed

> `claude-primary/opus[1m]` cleared the verifier gates (reasoning≥4.5,
> judgment≥4.5, verification≥4.7) … incumbent, nothing else cleared them

while a Codex target sat on the same bench. Nothing was rejected. Nothing was
*considered*. And every number in that sentence was a guess.

**The rule this directory implements: a row carries MEASURED scores or none.
No lane invents a number.**

## Layout

```
<role>/bench.yaml                       benchVersion + the ordered task list
<role>/<task-id>/task.md                the prompt, verbatim, as the seat gets it
<role>/<task-id>/rubric.yaml            the criteria, weights and checks
<role>/<task-id>/fixture/               copied to a scratch dir, once per run
```

`benchVersion` is bumped **by hand** on any change to a role's directory. The
proposal also carries a `benchHash` — sha256 over the whole role directory,
sorted by path — so a set that changed under a measurement is refused rather
than silently believed.

## What a check may ask

Seven kinds, and no more (`buzz_core::registry_bench::Check`). **No model
judges anything in v1**: a grader would put the opinion straight back into the
number this exists to remove, and it would make two runs of the same artifacts
disagree.

| check | asks |
|---|---|
| `!exitCode` | the run's own exit status |
| `!stdoutMatches` | a regex over everything the run printed |
| `!fileContains` | a literal substring in a file in the scratch dir |
| `!fileAbsent` | a file is **not** there (this is how "did not report a defect that is not there" is scored) |
| `!jsonPathEquals` | a JSON pointer into a file equals a value |
| `!diffApplies` | a file is a patch that applies (a **probe**: the harness answers it) |
| `!command` | a script in `fixture/` exits 0 (a **probe**) |

A criterion whose probe the harness did not answer **fails, by name**. An
unanswered check is never a pass.

## What the harness leaves behind

Before scoring, the harness writes into the scratch dir:

```
.bench/exit-code      the adapter's exit status, as text
.bench/duration-ms    wall-clock milliseconds, as text
.bench/stdout         everything the run printed
```

`.bench/duration-ms` is how a `velocity` criterion is scored mechanically. It
is not deterministic and nothing pretends it is: an unstable trait shows up as
spread, and a spread of 1.0 or more refuses the whole proposal.

## The arithmetic

Per run, per trait:

```
raw   = Σ(weight of passed criteria tagged with that trait)
      / Σ(weight of all criteria tagged with that trait)
score = round1(1.0 + 4.0 × raw)        # half-up, on the registry's 1–5 scale
```

A trait no criterion tags gets **no score** — never a zero. The row value is
the **median** over `--repeat` runs (default and minimum 3), with `n`, `min`
and `max` beside it.

## Stubs

`lead`, `architect`, `ui_designer`, `researcher` and `poker` ship with an empty
task list. `bee sessions registry propose --role lead` therefore refuses:
*"the bench for lead has no tasks: a row measured by nothing is not a measured
row."* That is the honest state, not an oversight.

## Known gap, named not glossed

Nothing here measures `taste` or `context`, so `ui_designer` and `researcher`
cannot be proposed even once they have tasks unless their rubrics evidence
every trait their class gate reads. `propose` refuses and says which trait.
