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

## 5. Hand off

Every `Surfaces` section ends with:

- the tests you expect **red first** (one per state you claimed, named), and
- **done when**: the observable condition, not "the lane reports done".
