# Conformance vectors

A **closed record** is one whose readers reject any key they do not know. When
a record has more than one strict reader, adding a key to it is only safe if
every reader ships the change in the same landing. That is
[`docs/UNIFIED_WORK_PLAN.md`](../docs/UNIFIED_WORK_PLAN.md) § 2 decision 2, and
§ 8 A3.3 states the mechanism this directory exists to provide:

> **Every additive key on a closed record lands with a shared conformance
> fixture loaded by every strict reader** — the 204 rule, after a `projectRef`
> echoed by the relay but unknown to the CLI's `deny_unknown_fields` reader made
> every owner-founded team session's authority chain unreadable. A lane that
> adds such a key names every reader in its report.

Ledger 204 is what it cost the last time: lane 186 taught the relay to echo
`projectRef` onto the kind:40099 acceptance receipt and did not teach one strict
reader the key, so from that landing every team session a project owner founded
with "Use roles" on had an unreadable authority chain and no seat on one could
publish a report. Nothing in CI could have caught it, because each reader's
tests used its own fixtures.

## The procedure for a lane that adds a key

1. **Add the vector first.** A canonical valid example carrying the new key,
   and an invalid one if the key has a shape rule of its own.
2. **Run every reader's test and watch them fail.** If any reader's test still
   passes, either that reader is not loading the vectors — fix that first — or
   the reader is not strict, which is a fact worth writing down.
3. **Update every reader**, in the same landing, and record in the fixture what
   each one now does.
4. **Name every reader in the lane's report**, with `file:line`.

Never loosen a reader to make a vector pass. Never delete a vector because a
reader disagrees with it: record the disagreement (below) and pin it.

## Recording a disagreement

Where two readers disagree about a vector today, the fixture states **both**
verdicts and each reader's test asserts its own. The suite stays green and the
divergence is named, greppable, and impossible to lose. A test that trips on a
pinned divergence says `PINNED DIVERGENCE moved` rather than "a test failed", so
the next reader is not tempted to "fix" the fixture.

A `null` verdict means *this reader does not read this variant at all* — it
answers `wrongKind`, or the variant belongs to a sibling decoder. That is
neither an accept nor a refusal, and collapsing it into either is how a
scope decision becomes an invisible defect.

## Guarded records

| Directory | Record | Readers |
| --- | --- | --- |
| [`authority-chain/`](authority-chain/README.md) | kind:44228 transition + kind:40099 acceptance receipt | `buzz-core`, `buzz-cli`, `buzz-session-provider`, desktop timeline |
| [`coding-session-records/`](coding-session-records/README.md) | kinds 44221, 44223, 44224, 44226, 44230 | `buzz-core` (the single Rust reader every crate calls), two desktop readers each for 44223 and 44224, the mobile Dart decoders |
| [`project-pack-source/`](project-pack-source/) | the `packRef` half of kind:44223 | `buzz-core`, desktop |
| [`project-work/`](project-work/) | the work declaration / assignment / evidence records | `buzz-core`, `buzz-cli`, desktop |
| [`coding-session-team-transaction/`](coding-session-team-transaction/) | kind:44244 | `buzz-core`, `buzz-sdk`, desktop |
| [`team-settlement/`](team-settlement/README.md) | the settlement projection over kind:44244 dispositions | `buzz-core`, `buzz-cli`, desktop (Tauri fold + the strict frontend reader) |
| [`project-pulse-fold/`](project-pulse-fold/), [`project-todo-fold/`](project-todo-fold/), [`transcript-export/`](transcript-export/) | fold outputs, not wire key sets | as documented in each |

A record with exactly one strict reader in one language does not need vectors;
a record with two does.
