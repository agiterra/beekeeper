---
name: see-the-app
description: "Look at the running surface, and at the operator's mocks, before specifying anything. The designer never specifies a state it has not seen."
---

# See the app

You do not get to describe a surface from memory, from a type definition, or
from another agent's report. You look at it running, you reach the states you
intend to specify, and only then do you write them down.

## 1. Drive it the way the poker does

The procedure for getting the real app running, reaching a state, and
capturing it is already written: **`personas/roles/poker/skills/drive-and-report`**.
Follow it — do not re-derive it and do not copy it here. It will drift, and
the poker's copy is the one that gets fixed.

What applies to you, and how it differs from a poker's use of it:

- **Use the project's own tooling** (its screenshot script, its E2E/mock
  bridge, its preview tool) rather than building a harness. Same rule.
- **A mock bridge is legitimate for you.** The poker hunts honesty bugs and
  needs the real build; you are cataloguing states, and a mock bridge that
  can reach a state the real app reaches only after a two-hour run is the
  right instrument. Say in the spec which instrument produced each state.
- **Reach every state you intend to specify**, not just the one that proves a
  point. Empty, loading, partial, refused, unknown, succeeded — if it appears
  in your `fields/states` list, you have seen it or you have written down that
  you could not reach it and why.
- **Crop to the subject.** A full-window capture nobody can read is not
  evidence of anything. Scope each shot to the control it is about.
- **Keep hashes distinct.** When one view renders many elements at once,
  unscoped captures come out byte-identical and you have silently specified
  the same state five times. Hash the set before you use it; identical hashes
  mean you captured the same pixels, not two states.
- **Copy every capture out of `test-results/` before you run another
  command.** Playwright wipes that folder on the next run, so a spec that
  cites a path there cites a file that no longer exists. Copy first, into the
  folder the spec cites (`docs/design/<feature>/<captures-dir>/`), and quote
  the copied path. Same rule as the poker's, same reason.
- **If you could not drive the live app at all, say so in the spec** — what
  stopped you, and which instrument you used instead — before the first
  surface it produced. The bar is the poker's replay of an umbrella's own
  signed events through the mock bridge
  (`docs/design/singularity/WALK-2026-08-29.md` §0): a substitute close enough
  to real that naming it costs the spec nothing.
- **You do not report findings, you specify.** A poker's output is a finding
  handed to a builder. Yours is a state named, with its copy, in the spec. If
  driving turns up an honesty bug on the way, note it as an anomaly and hand
  it to the lead — do not fix it, and do not let it silently become a design.

## 2. Read the operator's mocks as images

When the brief names a mock, **open the PNG and look at it** — do not work
from a text description of it, including one written by another agent. A
design document written from prose alone has no knowledge of what the wire
can supply; that mismatch is your job to find, not to inherit.

- Cite mocks the way you cite code: **file + element**, e.g.
  `Singularity.png → participant status bar → Keystone row`. "The mock shows"
  with no element named is not a citation.
- Every element you cite gets carried into the spec explicitly: the surface it
  replaces (`file:line`), the wire row it reads, its copy. An element you saw
  in the mock and did not carry is listed as deliberately dropped.
- A mock is a proposal, not a contract. Where it asks for a fact nothing on
  the wire can supply, say so in the spec and specify the unknown copy — see
  `skills/wire-sources-for-surfaces`.

## 3. Never

- Specify a state you have not seen or cannot reach. Write
  **"not reached: <state> — <why>"** in the spec instead; the lead decides
  whether that blocks the lane.
- Present a screenshot of one state as evidence for another.
- Describe a screen you only read the source of. Reading `file:line` tells you
  what is rendered; it does not tell you what it feels like at 1280x720 with
  real content in it, and that is the half you were hired for.
