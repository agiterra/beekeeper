# Session display name

Every surface that shows an umbrella coding session's name — desktop header,
sidebar and channel menu, Pulse, mobile, `bee sessions` — must pick the same
text from the same events, and must say where it came from. A person's name
(kind:44229) and a provider's generated title (kind:44252) are two records;
this directory is the one rule that ranks them, in executable form, and the
rule every reader binds to.

The normative text is `docs/nips/NIP-CSG.md` § Generated title. The Rust
reference is `resolve_session_display_name` and
`validate_coding_session_title_parts` in
`crates/buzz-core/src/coding_session_title.rs`. Spec:
`plans/SESSION_PARITY_SPEC_AUTOTITLE.md` (agents repository), SV-31.

## The rule

Given one umbrella — its channel `h`, its `sessionRef` `d`, its founder (or
none), its founding execution's 44223 title (or none), and every execution
the reader holds as `{targetKey, providerAuthorityPubkey}` — and a bag of
events in any order:

0. **Scope.** Only kinds 44229 and 44252 are read. An event belongs to this
   session when it carries a tag `["h", channelId]` and a tag
   `["d", sessionRef]`, compared byte for byte. Anything else is not this
   session's and is neither used nor counted.
1. **Validity.** A 44229 must pass the NIP-CSN envelope (exactly `h`, `d`,
   `csnm-v=csnm1-1`; one line of at most 256 bytes). A 44252 must pass the
   generated-title envelope (exactly `h`, `d`, `cstl-v=cstl1-1`, `cs-target`
   holding a re-encodable `coding-session/v1` key; strict v1 JSON content of at
   most 2048 bytes; see NIP-CSG). An in-scope event that fails is counted in
   `malformed` and otherwise ignored.
2. **Person tier.** Valid 44229s whose `pubkey` equals the founder (hex,
   case-insensitive). The greatest `(created_at, id)` wins — id compared as a
   lowercase string. A valid 44229 from anyone else, or any 44229 when the
   founder is unknown, is counted in `foreignNames`.
3. **Generated tier**, only when tier 2 is empty. Valid 44252s whose
   `cs-target` equals, exactly, the `targetKey` of an execution in the scope
   **and** whose `pubkey` equals that execution's `providerAuthorityPubkey`.
   The **smallest** `(created_at, id)` wins, so a title never flips once
   shown. Every other valid 44252 — a signer that is another execution's
   provider, a signer with no execution at all, a target the umbrella does
   not hold — is counted in `foreignTitles`.
4. **Fallback**, when tiers 2 and 3 are empty: the founding execution's title
   when it has a non-whitespace character, else `Untitled session`.
5. **Ranked, never timed.** Tier 2 beats tier 3 however old the name and new
   the title. Nothing in this rule reads a clock.
6. **Output.** `{name, origin, model, signerPubkey, diagnostics}`. `origin`
   is `person`, `generated` or `fallback`. `model` and `signerPubkey`
   (lowercase hex) are set for `generated` only and `null` otherwise. `name`
   is the winning event's text exactly — the 44229 content or the 44252
   `title` — untrimmed.

The result does not depend on event order; every runner checks each vector
forwards and reversed.

## The fixture

`fixtures/vectors.json` (`schema: buzz.conformance/session-display-name@1`)
is **hand-written** and frozen: a reader change never edits it to pass. It was
produced once by a throwaway script and is now edited by hand; keys are sorted
and the file is pretty-printed with two-space indentation.

| Field | Meaning |
| --- | --- |
| `constants` | `kindName` 44229, `kindGeneratedTitle` 44252, `tagVersion`, `payloadSchema`, `maxTitleBytes`, `maxContentBytes`, `maxModelBytes`, `untitled`. A runner asserts each equals its own build's constant |
| `envelopes[]` | `{name, description, event, valid}` — one 44252 each; `valid` is the verdict of rule 1 alone, independent of any scope |
| `vectors[]` | `{name, description, scope, events, expected}` |
| `scope` | `{channelId, sessionRef, founderPubkey \| null, foundingExecutionTitle \| null, executions: [{targetKey, providerAuthorityPubkey}]}` |
| `events[]` | Nostr events without `sig`: `{id, pubkey, created_at, kind, tags, content}` |
| `expected` | `{name, origin, model, signerPubkey, diagnostics: {foreignNames, foreignTitles, malformed}}` |

Event ids and pubkeys are synthetic lowercase hex labels, not hashes of the
events or keys on a curve. Bind to them as opaque strings; never verify a
signature or recompute an id. A reader that receives signed events verifies
them before this rule, exactly as it does today.

| Vector | What it pins |
| --- | --- |
| `person-name-only` | a founder 44229 alone is the person's name |
| `older-person-name-beats-newer-title` | tiers are ranked, not timed |
| `latest-person-name-wins` | newest founder name; tie on `created_at` → greater id |
| `earliest-title-wins` | earliest title across executions; later titles never replace it |
| `earliest-title-tie-breaks-on-smaller-id` | tie on `created_at` → smaller id |
| `foreign-signer-ignored` | another execution's provider signing for A1 is counted, even when earlier |
| `signer-without-execution-ignored` | a stranger, for an umbrella target or an outside one: counted; fallback |
| `target-outside-umbrella-ignored` | the exact target key, generation included, must be listed |
| `title-survives-a-restart` | a generation-1 title stands beside a listed generation 2 |
| `foreign-name-never-a-person-name` | a non-founder 44229 never wins, however new (Pulse kept the newest from any signer before this) |
| `founder-unknown` | no founder → no person tier |
| `fallback-founding-execution-title` | nothing names it → founding execution title |
| `fallback-untitled` | no founding title → `Untitled session` |
| `fallback-blank-founding-title` | a blank founding title is none |
| `other-sessions-ignored` | other `d`, other `h`, other kinds: not used, not counted |
| `malformed-events-rejected` | every malformed shape below, plus a two-line founder 44229, counted in `malformed` |

The envelope vectors cover: an extra tag, reordered tags, a three-field tag, an
unknown `cstl-v`, a non-canonical `d`, a non-v1 `cs-target`, generation 0, a
two-line title, a 257-byte title, a blank title, bad JSON, an unknown content
key, an absent `sourceCommand` key, schema v2, an unknown `basis`, an
uppercase `createEventId`, a short `sourceCommand`, an empty `model`, and
content over 2048 bytes — and three valid shapes, including a title of
exactly 256 bytes.

## Readers

| Reader | Binds to these vectors? | Owner |
| --- | --- | --- |
| `buzz-core` — `coding_session_title.rs` (the relay's ingest validator, and the resolver `bee` and Pulse call) | **yes** — `crates/buzz-core/src/coding_session_title_tests.rs` (`conformance_*`) | SV-31 S1 |
| `buzz-relay` ingest — calls the `buzz-core` validator | through `buzz-core`; its own tests in `crates/buzz-relay/src/handlers/ingest_coding_session_title_tests.rs` | SV-31 S1 |
| `bee sessions list\|show` and `bee pulse` — `crates/buzz-cli/src/commands/{sessions.rs,pulse.rs}` | **yes** — `crates/buzz-cli/src/commands/sessions/display_name_tests.rs` (`shared_vectors_pass_through_the_cli_path`) | SV-31 S1 (CLI half) |
| `buzz-core` Pulse fold — `pulse_fold.rs` via `pulse_fold_names.rs` (founder read from the 44226 genesis) | **yes** — `crates/buzz-core/src/pulse_fold_names_tests.rs` (`shared_vectors_pass_through_the_json_record_path`) | SV-31 S1 (Rust readers) |
| Desktop — `desktop/src/features/coding-sessions/lib/codingSessionTitle.ts` via `useCodingSessionNames.ts` | **yes** — `desktop/src/features/coding-sessions/lib/codingSessionTitle.test.mjs` (envelopes and vectors, forwards and reversed) | SV-31 S2 |
| Mobile — `mobile/lib/features/coding_sessions/domain/coding_session_session_decoders.dart`, `coding_session_fold.dart` | **yes** — `mobile/test/features/coding_sessions/domain/coding_session_display_name_conformance_test.dart` (envelopes through `decodeCodingSessionGeneratedTitle`, vectors through `resolveCodingSessionDisplayNameFromEvents`, forwards and reversed) | SV-31 S3 |
| Web — `web/src/features/coding-sessions/domain/sessionTitle.ts` via `umbrella.ts` | **yes** — `web/src/features/coding-sessions/domain/sessionTitle.test.mjs` (constants, envelopes and vectors, forwards and reversed) | SV-31 W2 |

Re-grep for `44252` and `KIND_CODING_SESSION_GENERATED_TITLE` across
`crates/*/src`, `desktop/src`, `mobile/lib` and `web/src` before trusting this
table; a reader nobody listed is a reader nobody checked.

## T3 Code, and where we differ on purpose

T3 Code stores one mutable `title` per thread. A rename wins structurally:
an explicit title clears the in-flight regeneration marker
(`apps/server/src/orchestration-v2/Orchestrator.ts:2858-2864`), and a
generated title lands only while the marker's `requestId` still matches
(`:3049-3056`). A placeholder answer ("New thread") is discarded
(`ThreadTitleRegenerationService.ts:117-121`).

Beekeeper's records are immutable signed events from different authors, so
the marker becomes a ranking: a person's 44229 outranks every 44252, which
is the same "a rename wins structurally" without shared mutable state. The
earliest-title rule replaces T3's single in-flight request: two providers
racing cannot flip a shown title. `clean_generated_name` discards Beekeeper's
own placeholders the way T3 discards "New thread".
