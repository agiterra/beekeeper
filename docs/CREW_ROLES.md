# Team roles

Seven persona packs under `personas/roles/<role>/` give a team seat its role.
Each is a valid persona pack (`.plugin/plugin.json` + `personas/<role>.persona.md`
+ `skills/`) — see `crates/buzz-persona/PERSONA_PACK_SPEC.md` for the pack
format itself. This document explains what the seven roles are for and how they
relate to each other; it does not restate the plan (`docs/CREW_SESSIONS_PLAN.md`
§1, §3, §4 S5) which remains the source of truth for the team model.

Any model may fill any role. None of these seven packs names a vendor or a
model — the seat does, at launch time. Each pack's `plugin.json` `description`
carries only a `model_min` capability note (the kind of model the role needs,
not a specific one). The *schema* does allow one: `PersonaConfig.model` is a
`provider:model-id` string (`crates/buzz-persona/src/persona.rs`) split into
`llm_provider` + `model` at resolve time. A team seat's model comes from the
seat regardless, which is why these packs leave the field unset.
Verify a pack with `bee pack validate personas/roles/<role>` before relying
on it — all seven pass clean today (`Valid.`, exit 0).

## The seven roles

| role | does | never does |
| --- | --- | --- |
| **lead** | rules on the wire (a `sessions send` to the seat **plus** a Pulse entry), never in its transcript; writes briefs, reads reports and diffs, hires a runner for every long gate, keeps the live check and the ruling for itself, merges tier-0/1, keeps the ledger honest, ends a mission out loud and stops there | writes feature code; reads a builder's exploration; runs a gate a runner could run; invents lanes the mission did not ask for; lets a finished mission read as a stalled one |
| **architect** | one-sitting shape verdicts: right design, or a simpler one | redesigns at length; writes code |
| **builder** | implements one locked brief inside its lane's exclusive files; self-verifies; reports raw facts | redesigns; touches files outside its lane; commits to `main` |
| **verifier** | one pass over a tier-2 diff against the brief's named constraints; a terminal verdict | re-argues a disposition; reviews tier-0/1 |
| **runner** | runs commands (`just ci`, e2e, builds); reports exit codes and counts | reasons about the diff; opines on whether a failure matters |
| **poker** | drives the built app through its real UI; reports honesty bugs with screenshots | fixes what it finds; reports from reading code alone |
| **designer** | names each feature's surfaces before builders start — entry point, fields, states, verbatim failure copy — or writes "no surface, by decision" | writes feature code; invents a surface the plan does not need |

This mirrors the seat table in `docs/CREW_SESSIONS_PLAN.md` §1, with three
seats the plan's operating model doesn't separately name: **architect**
(the plan's design-shape check, split out as its own seat rather than folded
into the lead), **poker** (the plan's S6 honesty-bug driver, made
available from S5 on rather than only at the final live proof), and
**designer** (added 2026-08-27 after six slices shipped wire contracts, CLIs
and a provider with almost no UI, because the brief template never asked for
one — it runs at brief time, before the builders, and its `Surfaces` section
is what the poker later walks).

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
for the whole session. Skill files stay under 200 lines: a skill nobody
finishes reading is a skill that does not apply.

## Family check (vendor diversity)

Contract D8 refuses a team launch when the verifier's model vendor equals any
builder's — a launch check the desktop performs from the seat roster (declared
per seat, read off the seat's ACP runtime when that runtime can run only one
vendor, or derived unambiguously from the model id), not something a pack or
its prompt can express or enforce. A pack has no vendor opinion; two seats
running this same `verifier` pack on different vendors are a valid team, and
the same pack on the same vendor as a builder seat is refused at launch.

**This is why the installer seats no verifier.** Every seat of one launch is
created against the single `providerInstanceRef` the dialog selected, so every
seat runs on that runtime's vendor — a verifier seated beside a builder there
can only ever share its vendor, and the launch would refuse the roster it just
installed (SESSION_STATE item 77, F7). The pack installs unseated; seat it by
hand in a roster launched across two providers.

## Prompt size and where the craft lives

Every pack's prompt (the `.persona.md` markdown body) carries only the role's
verbs, its verdict vocabulary, and its "never" list. Anything procedural (a
template to fill, a sequence of steps to follow) is a skill under that pack's
`skills/`, loaded on demand rather than paid for on every turn. Five of the
seven prompts run 20–25 body lines; `lead` (122) and `designer` (81) are longer
because both carry rules the team paid for in live runs — measure with the
frontmatter stripped before quoting a number here:

| role | prompt body | skills |
| --- | --- | --- |
| lead | 122 | `write-brief`, `hire`, `triage-report`, `choose-model`, `beekeeper-project` |
| architect | 25 | `shape-verdict` |
| builder | 23 | `brief-is-law`, `write-report` |
| verifier | 24 | `refuter-pass` |
| runner | 20 | `run-and-report` |
| poker | 22 | `drive-and-report` |
| designer | 81 | `see-the-app`, `wire-sources-for-surfaces`, `specify-surfaces` |

## Materialization

A seated persona's resolved skills are written to that seat's own workdir
(`<workdir>/.agents/skills/<name>/SKILL.md`), never to a shared directory —
see contract D8-A and `crates/buzz-persona/src/pack.rs`'s `resolve_skills` for
the packs' half of that contract (which skill goes to which persona).

Where that happens today, precisely:

- **Team seats.** The desktop stages the seat's pack coordinates in its
  host-local actor-seat entry (`packDir` / `personaId`, never on the wire) and
  the provider materializes them into the execution's working directory before
  the adapter is spawned (`crates/buzz-session-provider/src/session.rs`). A
  seat whose persona has no pack on this computer is staged without one, and
  the Team tab says that seat carries no role skills.
- **Managed agents installed from a role pack.** `install_crew_role_packs`
  (`desktop/src-tauri/src/managed_agents/crew_roles.rs`) writes the record's
  `persona_team_dir` / `persona_name_in_team` link and its `home_role`, so
  `resolve_seat_pack` resolves and a seat filled by one of these agents stages
  *with* its pack. `ManagedAgentSummary.has_role_pack` is exactly
  `resolve_seat_pack(record, &teams).is_some()`, and the agent row says
  "Role pack not installed here" when it is false.
- **Managed agents created any other way** (the channel-agent spawn path, not a
  team seat) still materialize nothing: `AgentDefinition::into_agent_record`
  writes `None` for both link fields, and that spawn path runs its child in the
  *shared* nest, where `materialize_persona_skills` refuses to write rather than
  putting one persona's skills where every agent — or the user's own home
  directory — would receive them.
- **The team the installer creates** (`Team roles`) carries the crew block the
  Team tab reads, with `lead, architect, builder, runner` seated in launch
  order, each declaring the `claude-agent-acp` runtime and the `anthropic`
  vendor it will launch on, and the lead taking the first turn. The lead is
  minted under whatever name the install dialog was given (plan D11; default
  `Lead`). Its `source_dir` is
  deliberately `None`: `delete_team_with_cascade` removes `source_dir`
  recursively, so a team pointed at `personas/roles` would delete the operator's
  checkout on "Delete team". The pack link lives on each agent instead.

## A note on the `role` slug

Contract D8-A gives `PersonaConfig` an optional `role` (slug) field so a
team seat's role can be read off the persona itself
(`crates/buzz-persona/src/persona.rs`). All seven packs declare it explicitly in
their `.persona.md` frontmatter, matching each persona's `name:` (`lead`,
`architect`, `builder`, `verifier`, `runner`, `poker`, `designer`). The field is optional,
so a persona without one is an ordinary persona rather than a team seat.
