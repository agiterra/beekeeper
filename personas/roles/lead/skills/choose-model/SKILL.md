---
name: choose-model
description: "Classify and route: score the task on eleven properties and a risk triple, pick the execution class, emit the routing record — and never name a model, because the router chooses the execution target."
---

# Classify and route

Two rules, and everything below is only how to obey them:

> **The lead chooses the capability required. The router chooses the execution target.**
>
> **Select the least expensive execution target whose expected failure mode is acceptable for the task.**

**You never name a model.** No `--model`, no vendor string, no alias, no tier
table of ids. You classify: the capability class the task needs, how expensive
being wrong is, and which review triggers fire. The host's router intersects the
live kind:44222 catalog with `team/model-registry.yaml`, enforces the hard gates,
picks the cheapest expected accepted completion among what is left, and writes
what it chose, the runner-up and the reason onto the create for you to read back.

*(The directory keeps the name `choose-model` so links hold; the rubric table it
used to carry is gone.)*

## 1. Classify the task — eleven properties

Answer all eleven before anything else — they are what the brief has to defend:

- **domain**, **reasoning depth**, **taste**, **judgment risk** — which class,
  and the trait minimums that class carries.
- **ambiguity** (1–5) — ≥ 3 means the design is still being discovered on the
  ground; the runner-class constraints read this number literally.
- **tool dependence**, **context size** — hard requirements the router cannot
  infer from prose: search/browser, shell, multimodal, context window.
- **execution autonomy**, **verification need** — the `agency` and
  `verification` minimums, and whether a verifier is its own seat.
- **latency**, **cost sensitivity** — tie-breakers only.

Cost and speed **choose only among targets that already cleared every gate**;
they never compensate for a capability deficit. A weighted product of capability
× cost × velocity is exactly what this skill replaced.

## 2. Score risk, derive the tier

**Risk = impact × uncertainty × irreversibility**, each 1–5.

| risk | tier | effort the router buys |
| --- | --- | --- |
| 1–8 | FAST | low |
| 9–39 | STANDARD | medium |
| 40–125 | DEEP | high |

Those thresholds are **seed policy, not truth** — say so when you cite them.
Traits answer *what capability is required*; risk answers *how expensive being
wrong is*: a hard but disposable experiment is still STANDARD, a simple
irreversible migration is DEEP.

The router never buys xhigh/max/ultra — human override only. And **never
auto-escalate effort after a failure**: a failed high seat gets a *different
execution target* or a *reviewer*, never more thinking on the same model.

## 3. Pick the execution class

`lead`, `architect`, `builder`, `runner`, `ui_designer`, `researcher`,
`verifier` — the registry carries each one's numeric minimums (spec §4):

- an outcome already understood, design locked → **builder**
- a boundary, schema, migration or contract still being decided → **architect**
- repo search, a gate, a mechanical edit, an extraction → **runner**
- visual direction, hierarchy, screenshot→implementation → **ui_designer**
- an adversarial pass over someone else's diff → **verifier**, cross-provider
- `poker` exists as a walk-the-app class and is **lane-drafted, not in Brian's
  §4** — say so whenever you route one.

### Role splitting: a DEEP buildable task is three seats

When risk lands DEEP *and* the task is buildable, do not hire one deep builder.
Route three, and say why in the brief:

1. **architect** (DEEP) — decides the boundary, writes the locked design.
2. **builder** (STANDARD or FAST) — implements it; the decisions are already
   made, so the implementation's risk is not the decision's risk.
3. **verifier** (STANDARD) — cross-vendor of the builder: diversity of failure
   mode is the point of the pass.

Splitting stops a deep risk score buying deep effort on every keystroke of a job
whose hard part was one paragraph.

## 4. Emit the routing record

The record rides `session.hire` and is echoed on the resulting create and on the
seat's 44223 metadata. You fill `class`, `tier`, `risk`, optionally `profile`,
plus review flags, challenger sampling and any override; **the router fills
`chosen`, `runnerUp`, `reason`, `registryVersion` and `catalogRevision`.**

A FAST builder task — *"implement approved API endpoint"*:

```json
{
  "class": "builder",
  "tier": "fast",
  "risk": { "impact": 2, "uncertainty": 1, "irreversibility": 2, "score": 4 },
  "profile": { "coding": 4.2, "discipline": 4.5, "verification": 4.2 },
  "reviewRequired": false,
  "reviewReasons": [],
  "challengerSample": false,
  "override": null
}
```

A DEEP architect task — *"decide whether the orchestration layer belongs inside
the session runtime or above it"*:

```json
{
  "class": "architect",
  "tier": "deep",
  "risk": { "impact": 5, "uncertainty": 4, "irreversibility": 5, "score": 100 },
  "profile": { "reasoning": 4.8, "judgment": 4.8, "context": 4.8, "verification": 4.5 },
  "reviewRequired": true,
  "reviewReasons": ["risk 100 >= 40", "irreversibility 5 >= 4", "contractChange"],
  "challengerSample": false,
  "override": null
}
```

`profile` is optional and it is **your opinion, not a measurement** — a floor you
assert, not a score anything sampled. Omit it rather than invent one.

## 5. Review is a trigger list, not a tier

Require review — and pass the flag — when any of these hold. Cross-provider
review is preferred wherever an eligible target exists:

- risk ≥ 40, or irreversibility ≥ 4 — the router computes both off `risk`.
- `securityBoundary` — security, auth, custody or data-loss boundary touched.
- `contractChange` — architecture, schema or public contract changed.
- `outsidePlan` — the builder operated outside an approved plan.
- `builderUncertain` — the builder reports uncertainty.
- `testsInsufficient` — tests cannot adequately verify correctness.
- `leadRequests` — you want independent judgment.

DEEP does not imply review, and review does not imply DEEP. Pass the flags you
can judge; the router adds the two it computes itself.

The two it computes are written out with the number that fired them — `risk 100
>= 40`, `irreversibility 5 >= 4` — not as a bare rule, so a reader is told the
fact rather than being asked to redo the arithmetic. The other six ride on the
record under the flag names above, exactly as you passed them.

## 6. Challenger sampling — every 5th STANDARD builder job

A model does not earn an incumbent route by being new, or lose one by being old.
So **every fifth STANDARD builder hire carries `--challenger-sample`** and the
brief says so in one clause: "this lane is the challenger sample for this batch —
the routing record marks it." That cadence is **seed policy until telemetry
exists**, exactly like the risk thresholds. Count the standard builder hires of
the batch; never sample a DEEP, irreversible or security-boundary lane to make
the count.

## 7. Registry check, every batch

Before the first brief of a batch:

```
bee sessions registry check --channel <uuid>
```

- **exit 0** — every offered execution target has a registry row. Route.
- **exit 4 — stale.** A *live offered* target has **no row**. A row for a model
  the catalog does not offer today is **dormant, not stale** — the registry is
  allowed to know a model that is not live.

Stale is never a licence to guess. In the same turn, publish and ask:

```
bee pulse update --project <coordinate> --kind blocker --session <umbrella-uuid> \
  --content "registry stale: <ids offered with no row> / <rows unassigned to a class> — proposed rows: <class, minimums cleared, and the capability that earns it>"
```

Then carry on with any class the check confirmed is still satisfiable; a batch
is blocked only when the class you need is the stale one. A docs lane lands the
row — you do not edit the registry mid-batch, because a registry that changes
under a running batch cannot explain why any seat in it was hired.

## 8. Hire, then read the decision

```
bee sessions hire --channel <uuid> --session-ref <umbrella-uuid> --role <slug> \
  --class <class> --risk <impact>,<uncertainty>,<irreversibility> \
  [--profile <json>] [--review-flags <flag,...>] [--challenger-sample] \
  --brief <path>
```

The tier is **derived from the risk triple by the router** — you do not pass it.
The create comes back carrying the full record: `chosen` (provider/model/effort),
`runnerUp`, the one-sentence `reason` naming the gates cleared and why it was
cheapest, `registryVersion`, `catalogRevision`. Quote `chosen` and `reason` in
the lane's Pulse line. If nothing clears the bar the hire is refused
`HIRE_NO_ROUTE`: requirements are never silently weakened, so the answer is a
different class or a founder ruling, never a retry.

`--model` is an **override only**, it requires `--because`, and the override is
recorded on the record and on the ledger. Justify it in the brief in the same
sentence you use it, or do not use it.
