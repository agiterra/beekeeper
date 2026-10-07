# Transcript prose join

A coding-session answer reaches the wire as several signed kind:44225
`assistant_text` items: the producer cuts prose at 24 KiB
(`COALESCE_FLUSH_BYTES`, `crates/beekeeper-session-provider/src/transcript.rs:109`)
whether or not paragraph streaming is on, at every tool call, and — with
`BUZZ_CSP_TRANSCRIPT_PARAGRAPH_FLUSH` — at paragraph boundaries of at least
512 bytes (`MIN_PARAGRAPH_FLUSH_BYTES`, `:122`, scanner at `:826-855`). Every
reader must put the pieces back into the one message the agent wrote. This
directory is that rule in executable form, and the rule every reader binds to.

The normative text is NIP-CST amendment 3, paragraph **Join key**
(`docs/nips/NIP-CST.md`). This file restates it with the fixture's shape and
names the readers.

## The rule

Within one **exact target** — signer + `driver` + `instanceId` + `sessionId` +
`generation` — first drop repeated deliveries, then sort items by `eventSeq`
(never by arrival). Walk them in order.

0. **One event, one piece.** A piece is identified by its event id (and so,
   within the exact target, by its `eventSeq`). A second copy of the same
   event — relay backfill and a live REQ subscription both delivering it, a
   reconnect replaying a window — is the same piece, not a second one. Readers
   deduplicate by event id before joining; otherwise the two copies sort
   adjacent with the same `turnId` and `parentToolId` and rule 3 repeats the
   paragraph. (NIP-CST forbids a duplicate `eventSeq` only on the producer
   side; repeated delivery is the reader's to absorb.)

1. Two `assistant_text` items are one message when they are adjacent, have the
   same `turnId` (`null` equals `null`), and the same `parentToolId` (both
   absent, or equal).
2. Any other item between them ends the message: tool call, tool result,
   reasoning, plan, status, prompt, result — anything that is not the same
   kind of prose. A lost sequence number (a gap) is not an item and does not.
3. Join by plain concatenation. Never insert a separator, never trim: the
   pieces are exact slices of one stream, so whatever blank line separated two
   paragraphs is already in the text.
4. Identity is the **first** piece's event id (`firstEventId`); the **last**
   piece's event id (`lastEventId`) is where the message currently ends. A row
   keyed on `firstEventId` stays mounted as later paragraphs arrive.
5. `messageId`: a message's `messageId` is the first one any of its pieces
   carries. A piece whose `messageId` is present and differs from the
   message's starts a new message. A piece without one joins.
6. `reasoning` items join by the same rule, with each other only.
7. **Arriving.** A message is `arriving` when its last piece is the last item
   of its turn in that exact target, the turn has no `result` or
   `interrupted` item yet, and the target has not **ended**. A target has
   ended when the same signer + `driver` + `instanceId` + `sessionId` has any
   item at a higher `generation` (the run was superseded: a provider crash, a
   host restart, a resume); when the target's latest kind:44223 metadata
   `status` ends the session — `completed`, `stopped` or `disconnected`; or
   when the reader holds no unexpired `live` kind:24223
   lease for that exact target (NIP-CSL, ephemeral generation leases) — no
   lease at all, a `released` one, or a `live` one past its TTL. The lease is
   the only one of the three that fires when the producer's machine sleeps,
   loses power or loses the network and never returns: nothing republishes a
   status (the `disconnected` recovery publish runs only when a provider
   restarts), the latest metadata stays `running`, and no new generation
   appears. `interrupted` and `failed` do **not** end the target: the
   producer publishes them at the end of an ordinary turn while the session
   goes on taking turns (a failure whose agent is gone publishes
   `disconnected`), and that turn's own `result` or `interrupted` item already
   ends its message. Counting them would hide the next turn's answer whenever
   its `running` metadata ties with or arrives after them. Any other status,
   or no metadata, leaves the answer to the lease and turn state. All of this is derived from wire facts, never from
   anything the producer asserts about the message. Items with `turnId: null`
   are never arriving.

The context brief (`recentTurns[]`, private context package) reads the same
joins, per exact target and `turnId`: `latestAssistantEventId` is the
`lastEventId` and `latestAssistantFirstEventId` the `firstEventId` of the
turn's latest **own** `assistant_text` message (no `parentToolId`); both
`null` when the turn has none. Subagent prose never counts.

## The fixture

`fixtures/vectors.json` (`schema: buzz.conformance/transcript-prose-join@1`)
is written **only** by
`crates/beekeeper-session-provider/src/transcript_prose_join_vectors_tests.rs`. Its
default run regenerates the vectors in memory and fails if the checked-in file
differs, so a producer change that moves a cut fails there first. To
regenerate on purpose:

```bash
BUZZ_REGEN_PROSE_JOIN_VECTORS=1 cargo test -p beekeeper-session-provider transcript_prose_join
```

then re-run every reader's binding test below. Keys are sorted and the file is
byte-stable.

Each vector:

| Field | Meaning |
| --- | --- |
| `name`, `description` | the case |
| `source` | `translator` — produced by the real `TranscriptTranslator::new(true).with_paragraph_flush(true)` from a streamed answer (7-byte chunks, so every blank line straddles two chunks somewhere), each item passed through `fit_item` unchanged; or `hand-written` — attribution cases the translator cannot produce alone |
| `input[]` | envelopes as a reader receives them: `{eventId, signer, content}`, where `content` is the exact NIP-CST content (`schema`, `session`, `eventSeq`, `timestamp`, `turnId`, `item`). Possibly out of order, and possibly with one event delivered more than once |
| `sessionStatus[]` | the latest kind:44223 metadata status the reader holds, per exact target: `{signer, target, status}`. Usually empty (no metadata: turn state alone decides `arriving`). Only rule 7 reads it |
| `sessionLease[]` | the reader's view, at its own `now`, of each exact target's winning kind:24223 lease: `{signer, target, lease}` with `lease` one of `live` (unexpired), `released`, `lapsed` (a `live` lease past `created_at` + TTL). A target with no entry has no lease the reader holds, which ends it. Usually empty, so most vectors have nothing `arriving`. Only rule 7 reads it |
| `expectedMessages[]` | `{kind, signer, target, turnId, parentToolId, firstEventId, lastEventId, text, arriving}`; `kind` is `assistant_text` or `reasoning`, `parentToolId` is `null` for the agent's own prose. Ordered by (signer, driver, instanceId, sessionId, generation), then `eventSeq` |
| `expectedBrief[]` | per exact target and turn: `{signer, target, turnId, latestAssistantEventId, latestAssistantFirstEventId}` |

Event ids and signers are deterministic synthetic labels, not signatures or
keys. Bind to them as opaque strings; never verify them.

A reader that does not render reasoning compares only the `assistant_text`
messages. A reader that renders a different order (newest first, say)
compares per message by `firstEventId`.

| Case | Source | What it pins |
| --- | --- | --- |
| `three-paragraphs-one-turn` | translator | three paragraph pieces → one message |
| `fence-straddles-size-cut` | translator | a 24 KiB cut inside an open code fence; still one message, every byte |
| `prose-tool-prose` | translator | reasoning, prose, tool call + result, prose → three messages |
| `own-prose-around-subagent-prose` | hand-written | own / subagent / own / subagent adjacent → four messages; brief names the last **own** one |
| `two-generations-share-turn-id` | hand-written | same `turnId`, generations 1 and 2 → never joined |
| `distinct-message-ids` | hand-written | differing `messageId`s split, absent ones join, nothing inserted |
| `out-of-order-arrival` | translator | the prose-tool-prose stream shuffled; sort by `eventSeq` first |
| `open-turn-arriving` | translator | a turn with no `result`, status `running`, lease `live`: its trailing message is `arriving` |
| `abandoned-generation-not-arriving` | translator | generation 1 dies mid-answer with no `result`; generation 2 publishes a prompt: generation 1's trailing message is **not** `arriving`, even with its last lease still `live` |
| `terminal-status-not-arriving` | translator | an open turn whose target's latest metadata status is `stopped`, lease still `live`: **not** `arriving` (`interrupted` / `failed` would leave it arriving) |
| `lapsed-lease-not-arriving` | translator | an open turn, status `running`, no higher generation, lease `lapsed` (the provider's machine slept): **not** `arriving`; the generator also asserts `released` and no lease do the same |
| `two-signers-one-target` | hand-written | another signer on the same session never joins |
| `reasoning-joins-like-prose` | hand-written | reasoning joins reasoning; each ends the other |
| `event-seq-gap-is-not-a-boundary` | hand-written | a lost `eventSeq` does not split |
| `duplicate-delivery-is-one-piece` | hand-written | one envelope delivered twice verbatim (backfill + live): rule 0, its text appears once |

## Readers

Found by grepping for `assistant_text` across `desktop/src`,
`desktop/src-tauri/src`, `mobile/lib`, `web/src` and `crates/*/src` on
2026-10-04 (non-test hits). **Re-run that grep before trusting this table**; a
reader nobody listed is a reader nobody checked.

| Reader | Joins today? | Binds to these vectors? | Owner (SV-36 slice) |
| --- | --- | --- | --- |
| `bee sessions transcript --format md` — `crates/beekeeper-cli/src/commands/sessions.rs` (`render_markdown_with_evidence`, `cmd_transcript`), join in `sessions/prose_join.rs` (`bee sessions export` stays raw signed JSONL) | **yes**; subagent prose headed `**Assistant (subagent …)**`; still-writing line only under rule 7 | **yes** — `crates/beekeeper-cli/src/commands/sessions/prose_join_tests.rs` | S2a |
| Context brief — `crates/beekeeper-core/src/coding_session_context.rs` (`coding_session_first_turn_brief`), join in `coding_session_context_prose_join.rs` | **yes**; one evidence slot per message; `latestAssistantEventId` / `latestAssistantFirstEventId` name the last **own** message | **yes** (`expectedBrief`) — `crates/beekeeper-core/src/coding_session_context_prose_join_tests.rs` | S2b |
| Mobile — `mobile/lib/features/coding_sessions/domain/coding_session_transcript.dart` (`_projectStream`), join in `coding_session_prose_join.dart`; leases wired at `coding_session_view.dart` (`transcriptFor`) | **yes** | **yes** — `mobile/test/features/coding_sessions/domain/coding_session_prose_join_conformance_test.dart` | S3 |
| Web — `web/src/features/coding-sessions/domain/transcriptProjection.ts`, join in `transcriptProseJoin.ts`; leases wired at `ui/observer-contract.ts` (`buildCodingSessionTranscriptBlocks`) | **yes** | **yes** — `web/src/features/coding-sessions/domain/transcriptProseJoin.test.mjs` | S4 |
| Desktop session view — `desktop/src/features/coding-sessions/lib/codingSessionTranscriptModelText.ts` (`joinConsecutiveCodingSessionProse`), called at `codingSessionTranscriptModel.ts:462`; `arriving` from leases via `lib/codingSessionProseArriving.ts` | **yes** — turn, `sessionId`, exact target, author, bridge signer, `parentToolId`, `messageId` | **yes** — `desktop/src/features/coding-sessions/lib/codingSessionTranscriptProseJoin.conformance.test.mjs` | S5 |
| Desktop transcript export — `desktop/src/features/coding-sessions/lib/transcriptExport/transcriptExportMessages.ts` (`mapCodingSessionTranscriptToExportMessages`), fed from `ui/useCodingSessionExport.ts` | **yes** — the same join, one `assistant_text` per message | **yes** — `desktop/src/features/coding-sessions/lib/transcriptExport/transcriptExportMessages.test.mjs` | S5 |

Rule 7 needs the reader's kind:24223 lease for the target. A reader that
does not hold leases has no evidence anyone is still writing, so it shows
nothing as `arriving` (`sessionLease[]` empty) — it never falls back to turn
state alone. Desktop already folds leases
(`desktop/src/shared/coordination/sessionCoordinationFold.ts`, expiry against
the caller's single `now`), as does mobile
(`mobile/lib/features/coding_sessions/state/coding_session_event_store.dart`);
S3–S5 reuse those folds rather than adding a second lease reader.

Not readers for this purpose: the producer itself (`buzz-session-provider`
`session.rs`, `lib.rs`, `context_projector.rs`, `config.rs`), the payload
type registry (`beekeeper-core/src/coding_session_payload.rs`), the decoders'
kind lists (`codingSessionTranscriptItemContract.ts`, `codingSessionDefensive.ts`,
`transcriptItemContract.ts`, `defensive.ts`), the export engine and its
viewer, which consume what `transcriptExportMessages.ts` hands them, and a
`buzz-dev-mcp` test fixture. `session_history` / `search_session` return
pieces, not messages; a hit is still cited by event id (spec risk 4, not
changed).

## Pinned divergences

The desktop session view joins today, but not exactly by this contract. S5
must make its binding test assert the contract, which moves these:

1. **Distinct `messageId`s.** Desktop joins them into one row and inserts a
   paragraph break if neither side has one
   (`codingSessionTranscriptModelText.ts:83-93`, `:104-114`). The contract keeps
   them two messages and inserts nothing. Desktop also compares only against
   the first piece's `messageId` (the joined item keeps `...left`), so a
   message whose first piece has none never adopts a later one.
2. **Hidden items.** Desktop joins over `visible`, built after diagnostic
   items (status and ceremonial diagnostics) are moved aside
   (`codingSessionTranscriptModel.ts:284-297`), so two prose pieces with a
   `status` between them join on desktop. The contract says any other item
   ends the message.
3. **Target.** Desktop compares `turnId`, `sessionId`, author and bridge
   signer; it has no explicit `driver` / `instanceId` / `generation` /
   signer comparison. Whether its `sessionId` already encodes the generation
   is for S5 to check against `two-generations-share-turn-id`.

## T3 Code, and where we differ on purpose

T3 Code models a streamed answer as one message that grows
(`packages/contracts/src/orchestrationV2.ts:1060-1068`: one `id`, `text`,
`streaming: boolean`; turn item `assistant_message` with `streaming`,
`:1308-1314`), fed by `content.delta` runtime events
(`apps/server/src/provider/acp/AcpCoreRuntimeEvents.ts:239`) and cleared to
`streaming: false` on `item.completed`. Beekeeper cannot copy the shape:
44225 items are immutable signed events, so the growing message is a run of
pieces the reader joins. The mapping:

- T3 `id` → `firstEventId`; T3's growing `text` → the concatenation.
- T3 `streaming` → `arriving`. T3 sets it from the provider's item lifecycle;
  we derive it from turn state (no `result` / `interrupted` yet), because a
  signed fact cannot be retracted when a provider stops mid-message. T3
  clears `streaming` on recovery from a crashed run
  (`apps/server/src/orchestration-v2/ProviderRuntimeRecoveryService.ts:395-406`
  for messages, `:560-571` for turn items); our equivalent is rule 7's
  ended target — a higher generation, a session-ending metadata status, or no
  unexpired `live` lease — so a dead answer stops showing a caret at the
  latest when its lease lapses (up to the lease TTL after the last renewal;
  NIP-CSL gives 180 s at the relay, renewed every 60 s).
- T3 joins nothing: a delta names its message. We join by the key above,
  because a piece names only its target, turn and attribution.
