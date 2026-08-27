# Crew roles

Six persona packs under `personas/roles/<role>/` give a crew seat its role.
Each is a valid persona pack (`.plugin/plugin.json` + `personas/<role>.persona.md`
+ `skills/`) — see `crates/buzz-persona/PERSONA_PACK_SPEC.md` for the pack
format itself. This document explains what the six roles are for and how they
relate to each other; it does not restate the plan (`docs/CREW_SESSIONS_PLAN.md`
§1, §3, §4 S5) which remains the source of truth for the crew model.

Any model may fill any role. None of these six packs names a vendor or a
model — the seat does, at launch time. Each pack's `plugin.json` `description`
carries only a `model_min` capability note (the kind of model the role needs,
not a specific one). The *schema* does allow one: `PersonaConfig.model` is a
`provider:model-id` string (`crates/buzz-persona/src/persona.rs`) split into
`llm_provider` + `model` at resolve time. A crew seat's model comes from the
seat regardless, which is why these packs leave the field unset.
Verify a pack with `bee pack validate personas/roles/<role>` before relying
on it — all six pass clean today (`Valid.`, exit 0).

## The six roles

| role | does | never does |
| --- | --- | --- |
| **lead** | rules, writes briefs, reads reports and diffs, merges tier-0/1, keeps the ledger honest | writes feature code; reads a builder's exploration |
| **architect** | one-sitting shape verdicts: right design, or a simpler one | redesigns at length; writes code |
| **builder** | implements one locked brief inside its lane's exclusive files; self-verifies; reports raw facts | redesigns; touches files outside its lane; commits to `main` |
| **verifier** | one pass over a tier-2 diff against the brief's named constraints; a terminal verdict | re-argues a disposition; reviews tier-0/1 |
| **runner** | runs commands (`just ci`, e2e, builds); reports exit codes and counts | reasons about the diff; opines on whether a failure matters |
| **poker** | drives the built app through its real UI; reports honesty bugs with screenshots | fixes what it finds; reports from reading code alone |

This mirrors the seat table in `docs/CREW_SESSIONS_PLAN.md` §1, with two
seats the plan's operating model doesn't separately name: **architect**
(the plan's design-shape check, split out as its own seat rather than folded
into the lead) and **poker** (the plan's S6 honesty-bug driver, made
available from S5 on rather than only at the final live proof).

## Verdict vocabulary

Shared across roles that render a terminal verdict (see each pack's prompt
and skills for the full phrasing rules):

- `APPROVE` / `APPROVE-WITH-NOTES` — the lead's and the architect's disposals.
  Notes are record, not a condition on landing.
- `BLOCK: missing-input = <the one thing, and who fetches it>` — the lead's
  and the architect's refusal to proceed on an incomplete input. Never a
  list of concerns; never "I have concerns" — that is not a verdict.
- `CONFIRMED: <inputs/state -> wrong outcome>` / `NOT-REFUTED` — the
  verifier's terminal disposal of a tier-2 diff. Never re-argued once given.

Three rounds that reframe a question instead of refining an answer is one
missing input in disguise — every role that gives verdicts is instructed to
stop and name it rather than spin a fourth round.

## Brief and report templates

The lead's `write-brief` skill carries the brief template from plan §1.1; the
builder's `write-report` skill carries the report template from §1.2. Keeping
these as skills rather than inline pack prompt text is why the prompts stay
short (see below) — the templates are loaded on demand, not held in context
for the whole session.

## Family check (vendor diversity)

Contract D8 refuses a crew launch when the verifier's model vendor equals any
builder's — a launch check the desktop performs from the seat roster (vendor
declared per seat, or derived unambiguously from the model id), not something
a pack or its prompt can express or enforce. A pack has no vendor opinion;
two seats running this same `verifier` pack on different vendors are a valid
crew, and the same pack on the same vendor as a builder seat is refused at
launch.

## Prompt size and where the craft lives

Every pack's prompt (the `.persona.md` markdown body) is short — the current
six run 21–30 lines — carrying only the role's verbs, its verdict vocabulary,
and its "never" list. Anything procedural (a template to fill, a sequence of
steps to follow) is a skill under that pack's `skills/`, loaded on demand
rather than paid for on every turn:

| role | skills |
| --- | --- |
| lead | `write-brief`, `triage-report` |
| architect | `shape-verdict` |
| builder | `brief-is-law`, `write-report` |
| verifier | `refuter-pass` |
| runner | `run-and-report` |
| poker | `drive-and-report` |

## Materialization

A seated persona's resolved skills are written to that seat's own workdir
(`<workdir>/.agents/skills/<name>/SKILL.md`), never to a shared directory —
see contract D8-A and `crates/buzz-persona/src/pack.rs`'s `resolve_skills` for
the packs' half of that contract (which skill goes to which persona).

Where that happens today, precisely:

- **Crew seats.** The desktop stages the seat's pack coordinates in its
  host-local actor-seat entry (`packDir` / `personaId`, never on the wire) and
  the provider materializes them into the execution's working directory before
  the adapter is spawned (`crates/buzz-session-provider/src/session.rs`). A
  seat whose persona has no pack on this computer is staged without one, and
  the Crew tab says that seat carries no role skills.
- **Managed agents** (the channel-agent spawn path, not a crew seat) currently
  materialize nothing: the record's `persona_team_dir` /
  `persona_name_in_team` link is `None` on every record built today, and that
  spawn path runs its child in the *shared* nest, where
  `materialize_persona_skills` now refuses to write rather than putting one
  persona's skills where every agent — or the user's own home directory —
  would receive them.

## A note on the `role` slug

Contract D8-A gives `PersonaConfig` an optional `role` (slug) field so a
crew seat's role can be read off the persona itself
(`crates/buzz-persona/src/persona.rs`). All six packs declare it explicitly in
their `.persona.md` frontmatter, matching each persona's `name:` (`lead`,
`architect`, `builder`, `verifier`, `runner`, `poker`). The field is optional,
so a persona without one is an ordinary persona rather than a crew seat.
