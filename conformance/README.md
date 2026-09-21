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
| [`coding-session-records/`](coding-session-records/README.md) | kinds 44221, 44223, 44224, 44226, 44230 | `buzz-core` (the single Rust reader every crate calls), two desktop readers each for 44223 and 44224, the mobile Dart decoders, and — since lane 223 — the **web** client's own hand-copied decoder for 44223 and 44224 |
| [`project-pack-source/`](project-pack-source/) | the `packRef` half of kind:44223 | `buzz-core`, desktop |
| [`project-work/`](project-work/) | the work declaration / assignment / evidence records | `buzz-core`, `buzz-cli`, desktop |
| [`coding-session-team-transaction/`](coding-session-team-transaction/) | kind:44244 | `buzz-core`, `buzz-sdk`, desktop |
| [`team-settlement/`](team-settlement/README.md) | the settlement projection over kind:44244 dispositions | `buzz-core`, `buzz-cli`, desktop (Tauri fold + the strict frontend reader) |
| [`project-pulse-fold/`](project-pulse-fold/), [`project-todo-fold/`](project-todo-fold/), [`transcript-export/`](transcript-export/) | fold outputs, not wire key sets | as documented in each |

A record with exactly one strict reader in one language does not need vectors;
a record with two does.

**Count the readers by grepping for the decode, not by trusting this table.**
Lane 215 wrote that the web client "decodes none of these kinds; it is a repo
browser", and it was wrong: `web/src/features/coding-sessions/domain/ingressPayloads.ts`
is a hand-copy of the desktop ingress decoder, `trust.ts` drops every 44223 it
refuses, and no fixture answered for it. When Astra imported it and fed it the
shared vectors on 2026-09-21 it disagreed with `buzz-core` about **ten**
metadata vectors and **five** receipt vectors. A reader nobody listed is a
reader nobody checked.

## Raw-content vectors

Every fixture carries two suites, and the second exists because the first
cannot express one whole class of malformed payload.

- `vectors[]` hold `content` as a **parsed object**. Every loader serializes it
  again before handing it to its reader.
- `rawVectors[]` hold `raw`, the **exact string** a producer would sign.
  **Nothing re-serializes it.**

The class the first suite cannot reach is the duplicate key.
`serde_json::Value`, `JSON.parse` and `jsonDecode` all keep one of two
same-named keys and lose the fact that there were two, so a fixture stored as an
object can never round-trip to `{"status":"failed","status":"running"}`.
`buzz-core` refuses those bytes — it decodes the content a second time into its
typed payload precisely so serde's duplicate-field detection applies
(`crates/buzz-core/src/coding_session_payload.rs:775`, `:1813`) — and the
desktop's coordination gate has always refused them too, while the desktop
ingress decoder, the mobile decoder and the web decoder accepted them. Three
readers saying yes and two saying no about the same signed event, invisible to
a green suite.

A raw vector's verdict is never *pinned*. The suite is small on purpose: a
canonical positive control per record, so a refusal proves the decoder rather
than a broken loader, plus the duplicate-key cases. Every loader asserts that
the positive control is accepted for exactly that reason.
