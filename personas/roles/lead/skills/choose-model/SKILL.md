---
name: choose-model
description: "The rubric a lead uses to pick a hire's vendor, model and thinking level — and to say why in the brief."
---

# Choose a model for a seat

Two questions, in this order, then one sentence of justification. The rubric is
written in tiers and modalities, never in product slugs: a slug that is current
today is wrong next month, and a seat is hired from a vendor's *family*, not
from a runtime identifier.

## 1. Task class → minimum tier

| Task class | Minimum tier |
| --- | --- |
| Mechanical edit with the answer already in the brief (rename, field add, fixture) | **small** |
| Run a gate and report exit codes and counts | **small** |
| One-file implementation against a locked design, tests named | **mid** |
| Multi-file feature, a contract change, or anything where the design is still being discovered on the ground | **frontier** |
| Provider runtime, custody/keys, relay ingest, durable state (tier-2) | **frontier** |
| Adversarial pass over a tier-2 diff | **frontier**, and a different vendor from the builder |
| Orchestration — briefs, triage, verdicts across a whole run | **frontier** |

Minimum means minimum. Hiring under the class is how a lane returns a
confident report about work it could not do; hiring far over it is only money.

## 2. Modality → vendor

- Text, code, tools: any vendor whose family covers the tier above.
- Images in, or screenshots to read: a vendor whose family is genuinely
  multimodal. Do not hire a text-only seat and hope.
- A refuter must not share the builder's vendor. Same family, same blind spot.
- The operator's choice for the *lead* seat is theirs, made when the session
  starts; you choose only the hires.

## 3. Thinking level

Raise thinking, not tier, when the work is one hard decision inside an
otherwise ordinary task. Raise tier when the work is many decisions.

## 4. Say why, in the hire

Every hire and every brief names the seat and the reason in one clause:

```
Seat: <vendor> · <tier/model as the host names it> · thinking <level>
      — because <task class from the table>, <modality if it decided anything>.
```

"Because the brief is locked and this is a two-file mechanical edit" is a
reason. The model name alone is not. If you cannot write the clause, you have
not classified the task yet — do that first.
