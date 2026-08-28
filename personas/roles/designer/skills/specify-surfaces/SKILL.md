---
name: specify-surfaces
description: "The procedure for naming every surface of a feature before a lane builds it, including the surfaces you decide not to build."
---

# Specify the surfaces

Run this once per user-visible feature, before the brief is dispatched. The
output is a `Surfaces` section the builder treats as law and the poker treats
as a walkthrough.

## 1. Read first

Open the screens, dialogs, and commands this feature lands beside and cite
them by `file:line`. Reuse an existing component or copy pattern wherever one
fits — a second control that does the same job in a different voice is a
regression even when both are correct.

## 2. For each surface, answer four questions

Cover **desktop, mobile, web, CLI**. For each one, either write all four
answers or write the decision line in §3.

```
<surface> — <file path of the entry point>
  entry point: how a person reaches it (menu item, dialog, flag, route),
               named as a control that already exists or one you specify.
  fields/states: every field with its type and default, and every state the
               surface can be in (empty, loading, staged-without-X, refused,
               succeeded). A state with no rendering is a state that lies.
  failure copy: the exact string, verbatim, for each failure this surface can
               reach — including the honest disclosure ones.
  disclosure: what this surface must say out loud that the happy path would
               rather hide (no pack behind a seat, an unconfirmed publish, a
               role that is not the actor's home role).
```

## 3. When there is no surface, say so

```
<surface>: no surface, by decision — <reason>. (needs Brian's sign-off)
```

Reasons that are legitimate: the surface has no user this week; another
surface already answers the same question; the platform cannot reach the
state. "We ran out of time" is legitimate too — it is a decision, and written
down it can be reversed. What is never legitimate is silence.

## 4. Copy rules

- Write every string verbatim in the spec. A lane that invents copy invents a
  comfortable version.
- A control names what it actually enforces. If the button cannot guarantee
  the thing, the label says what it does guarantee.
- Prefer the unpleasant truth: "the relay never confirmed this signed request"
  beats "the provider has not accepted this request" when the second one
  blames a machine that never saw it.
- Desktop text uses rem tokens only (`text-base`, `text-sm`, `text-xs`,
  `text-2xs`, `text-3xs`) — never an arbitrary px or rem literal.

## 5. From a mock to a spec

A mock is a proposal drawn without wire knowledge. Turning it into a spec is a
per-element walk, not a paraphrase. Look at the image first
(`skills/see-the-app`), then take the elements **one at a time**, in reading
order, and write five lines for each:

```
<element> — cite the mock: <file> → <region> → <element>
  replaces: <file:line> of the surface it supersedes, or "new surface"
  reads: <row from skills/wire-sources-for-surfaces>
  copy: verbatim strings, including the empty and unknown ones
  walk: how the poker reaches this state, step by step
```

Rules for the walk:

- **Every element gets all five lines or a decision line.** An element you are
  dropping is written as `<element>: dropped — <reason>`; silently omitting it
  is how a mock's best idea disappears.
- **An element whose fact has no row is not designable yet.** Either it maps
  to a row, or the spec says "no signed source today" in the words
  `wire-sources-for-surfaces` gives, and the surface shows the unknown copy.
  Never let a mock's placeholder number become a rendered number.
- **A mock's label is a suggestion; the wire decides the truth.** Where the
  mock says "Idle" over something the wire only knows as unreachable, the spec
  says `No provider answering` and records that you overrode the mock.
- **Density and lifecycle are different axes.** A view mode is a lens on the
  same data; a lifecycle state is a fact about the work. If a mock uses one
  word for both, split them in the spec and name both.
- **Reuse before invention.** If the codebase already renders this fact, cite
  it and extend it. A second control doing the same job in a different voice
  is a regression even when both are correct.

### Naming: Singularity is the surface

The shared work surface a person opens is a **Singularity**. The underlying
runtime objects are still **sessions** — `sessionRef`, umbrella session,
execution, generation — on the wire, in the CLI, and in the code. Do not
rename them, and do not call a Singularity a session in user-facing copy. A
Singularity has one goal, one team and one stream; it may be led by an agent,
led by a human, or have no lead at all, so no copy may assume a lead exists or
name a particular one.

## 6. Hand off

Every `Surfaces` section ends with:

- the tests you expect **red first** (one per state you claimed, named), and
- **done when**: the observable condition, not "the lane reports done".
