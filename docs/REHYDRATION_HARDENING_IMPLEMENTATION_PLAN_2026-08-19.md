# Rehydration hardening — truth-first implementation plan

**Status:** implementation handoff, draft for Brian's review. No code has been
written for this plan.
**Audience:** an implementing Claude Code / Fable-class model working in
`/Users/brian/Projects/buzz`.
**Date:** 2026-08-19.
**Scope:** `docs/SESSION_STATE.md` §2 items **4**, **5**, **6**, and the
**disclosure half of item 1**. Nothing else. §3 item 2 names these as one bite;
this document is that bite.

This plan is written to be executed without further design choices. Every
constant, function name, file, and line anchor an implementer needs is fixed
below and was verified against the working tree on 2026-08-19 at
`wip/project-pulse` (HEAD `7fc415b0`). If you find yourself choosing a number,
an env var name, or a failure-disclosure slug, you are reading the wrong
document — the answer is here.

Where this document disagrees with `docs/SESSION_STATE.md` about the *current
state of the code*, §1 below states the disagreement explicitly and cites both
sides. `SESSION_STATE.md` still governs everything outside this bite.

---

## 1. Ledger corrections that ship with the code

The ledger's §2 items 4–6 were written before commit `b9de9a6d`
(`feat(sessions): orient new executions from evidence`, 2026-08-18 22:37:42
-0400, an ancestor of HEAD — `git merge-base --is-ancestor b9de9a6d HEAD`
exits 0). That commit is credited in the ledger for item 3 only
(`docs/SESSION_STATE.md:8`, `:54-61`), but its diff touches the very two files
items 4–6 cite (`git show b9de9a6d --stat`:
`crates/buzz-dev-mcp/src/session_context.rs | 70 ++-`,
`crates/buzz-session-provider/src/context_projector.rs | 80 ++-`).

**Rule for the implementer: these corrections land in `docs/SESSION_STATE.md`
in the same commit as the code, not afterward.** The ledger's own §5 rule
(`SESSION_STATE.md:252-257`) says findings land in §2 the day they are found.

### 1.1 §2 item 4 — half of it already shipped

`SESSION_STATE.md:62-66` reads "Fix: time-bound the provenance, then add
refresh," framing both halves as future work. The time-bounding half is wired
end to end today:

| Claim | Code |
| --- | --- |
| The provenance struct has a watermark field | `crates/buzz-core/src/coding_session_context.rs:98` (`pub complete_as_of: Option<i64>`) |
| Its doc comment already says it bounds `complete` | `coding_session_context.rs:92-96` — "This bounds `complete`: it never means \"current forever.\"" |
| The projector populates it | `crates/buzz-session-provider/src/context_projector.rs:884` — `complete_as_of: input.coverage.complete.then_some(input.generated_at)` |
| The sidecar explains it on every tool response | `crates/buzz-dev-mcp/src/session_context.rs:271-277` (`provenance_semantics()`) — "Complete only for the source snapshot begun at provenance.completeAsOf; later concurrent activity may exist." |
| The bootstrap prompt instructs the agent about it | `crates/buzz-session-provider/src/session.rs:60` — "Report completeAsOf, complete, and truncated honestly. Later concurrent work may exist after completeAsOf." |

**Correction to write:** item 4's time-bounding half shipped in `b9de9a6d` and
the ledger never recorded it. Only "add refresh" is open. This plan adds
read-time age (§8) and a bounded refresh (§9). **Item 4 is downgraded, not
closed** — a refreshed package is still a snapshot with a watermark.

### 1.2 §2 item 5 — the stated defect no longer exists

`SESSION_STATE.md:67-69` says the `session_history` page cap is 20 and cites
`session_context.rs:23-24`.

| Ledger claim | Code today |
| --- | --- |
| `DEFAULT_HISTORY_LIMIT` = 10, `MAX_HISTORY_LIMIT` = 20 at `:23-24` | `crates/buzz-dev-mcp/src/session_context.rs:24` = `100`, `:25` = `200` |
| "101 items took six calls" | `session_context.rs:580` is a regression test literally named `one_history_call_can_retrieve_the_observed_101_item_session` |
| "a hard `limit must be between 1 and 20` error" | `bounded_limit` interpolates the per-tool maximum (`session_context.rs:306-320`); the maxima are 200 (`:179-184` call site) and 50 (`:215-220`). The literal string "between 1 and 20" is unreachable from any current tool call. |

`git show b9de9a6d -- crates/buzz-dev-mcp/src/session_context.rs` shows
`-const DEFAULT_HISTORY_LIMIT: usize = 10;` / `-const MAX_HISTORY_LIMIT: usize
= 20;` replaced by `100` / `200`.

**Correction to write:** strike item 5's stated cap as fixed in code by
`b9de9a6d`, citing `session_context.rs:24-25` and the test at `:580`. Record
two traps for whoever re-tests: (a) the error string in the ledger cannot be
produced any more; (b) there is a *second, still-live* `20` in the same file —
`DEFAULT_SEARCH_LIMIT` (`:26`) belongs to `search_session`, a different tool —
so a re-test aimed at the wrong tool will look like a regression.

**What is genuinely still open under item 5**, and what §7 fixes:

1. **Ceiling mismatch.** A package may hold `MAX_CONTEXT_HISTORY_ITEMS =
   4_096` items (`coding_session_context.rs:24`) against a 200-item page, so a
   full walk is still 21 calls. The ledger's words — "Untenable as sessions
   grow" — still apply at that scale.
2. **A hard error instead of a self-describing boundary.** Overshooting the
   cap is `invalid_params` (`session_context.rs:313-318`), not a short page
   that names its own bound.
3. **Offsets are not stable under §9's refresh.** New, and created by this
   plan if left unaddressed — see §7.1.

### 1.3 §2 item 6 — the disclosure half shipped; the arithmetic did not

`SESSION_STATE.md:70-74` says "the package never says so."
`provenance_semantics()` (`session_context.rs:271-277`), shipped in
`b9de9a6d`, is returned on every `session_overview` / `session_history` /
`search_session` response and says `sourceEventCount` is "All signed facts
retained in the verified proof graph, including identity, authority,
lifecycle, metadata, and transcript events."

**Two corrections to write:**

1. The "never says so" half is fixed by `b9de9a6d`. What remains is that the
   two numbers still do not *reconcile*: there is no per-category breakdown and
   no note stating the arithmetic. `notes` starts as a copy of the source's own
   coverage notes (`context_projector.rs:846-851`, which `validate_coverage`
   permits up to `MAX_CONTEXT_PROVENANCE_NOTES` = 32 of, `:955-959`) and is then
   appended to by **three** `record_omission_note` call sites — `:852-859`
   (redaction), `:860-865` (item-bound truncation) and `:913-916` (package-byte
   truncation) — none of them about this delta.
2. **Citation drift.** The ledger cites `context_projector.rs:945-955` for the
   formula. `fn source_event_count` is at `context_projector.rs:970-980`;
   `:934-946` is `validate_limits`. The substance of the formula claim is
   unchanged — verified verbatim at `:970-980`:
   `1 + authority_links.len() * 2 + name_revisions.len() + goal_revisions.len()
   + Σ over generations (3 + transcript.len())`.

### 1.4 §2 item 1 — the disclosure gap is real and unchanged

Only item 1's **disclosure** half is in scope. Its cause (a duplicated create)
is `SESSION_STATE.md §3 item 1`'s live re-test and is **not** this plan's work.

Verified drop point for the reason:

- `prepare_rehydration_context` (`crates/buzz-session-provider/src/lib.rs:923-1028`)
  has **eight** `return None` bail-outs (the `return None` statements are at
  `:938`, `:946`, `:954`, `:962`, `:970`, `:993`, `:1005`, `:1020`). Six log and
  drop a reason (`:932-939`, `:940-947`, `:964-971`, `:987-994`, `:999-1006`,
  `:1014-1021`); the remaining pair (`:948-955`, `:956-963`) is not a failure at
  all — it is a session with no prior execution.
- The richest one — `"verified prior context unavailable; starting Fresh:
  {error}"` at `:987-994` — is the arm that swallows
  `ContextProjectionError::Conflict("command {id} has more than one provider
  receipt")` (`context_projector.rs:656`), the exact 2026-08-18 incident.
- The only operator-facing signal on the **create** path is
  `create_disclosure_status` (`lib.rs:2230-2238`) mapping
  `SessionContinuity::Fresh → Some("session_fresh")`, enqueued through
  `payload::status_item(status)` (`lib.rs:834-842`).
- The **resume** path never reaches that function at all: `resume_session` has
  its own inline match (`lib.rs:1136-1161`) emitting `session_resumed` /
  `session_loaded` / `session_rehydrated` / `session_restarted_without_context`,
  enqueued at `:1163-1169`. A resume that lost its verified package is therefore
  disclosed as the bare `session_restarted_without_context` — a *different*
  slug with the *same* defect: no reason. `session_fresh` is never published on
  resume.
- `status_item` is `serde_json::json!({ "kind": "status", "status": status })`
  (`crates/buzz-core/src/coding_session_payload.rs:579-581`). **It structurally
  cannot carry a reason.**
- Desktop renders it through `CODING_SESSION_CONTINUITY_STATUSES`
  (`desktop/src/features/coding-sessions/lib/codingSessionTranscriptItems.ts:504-520`)
  and `buildStatusLifecycleItem` (`:522-534`) as the bare string "Started
  fresh — no prior session context" (`:506`); the resume path's
  `session_restarted_without_context` renders equally bare as "Restarted
  without prior context" (`:519`).

So a missing sidecar, an unreachable relay, a duplicate-create conflict, an
unverifiable fact, a bound overflow, an encode failure and a disk failure all
render **identically** to the operator. That indistinguishability is the bug.

---

## 2. Facts already present in the product

Implementation inputs, each verified this session. Do not re-derive, do not
duplicate.

1. **Package generation is already live, not stale-by-construction.**
   `prepare_rehydration_context` (`lib.rs:923-1028`) calls
   `context_projector::fetch_and_project_session_context` (`lib.rs:980-985`)
   against the relay REST client and writes a fresh file every time it runs.
   `create_session` calls it at `lib.rs:760-769`; `resume_session` calls it
   independently at `lib.rs:1081-1090` with a fresh `package_id =
   Uuid::new_v4()` (`:1080`). Create, resume and reattach are already uniform
   (`session.rs:629`, `:646`, `:667` all forward the same `mcp_servers`).
2. **Package writes are OS-enforced write-once.**
   `context_store::write_context_package` opens with
   `OpenOptions::new().write(true).create_new(true)`
   (`crates/buzz-session-provider/src/context_store.rs:49-50`), proven by
   `never_overwrites_an_existing_execution_package` (`context_store.rs:183`).
   In-place refresh is refused by the kernel, not by convention. A write that
   fails partway already unlinks its own partial file (`:58-62`), so the only
   way a truncated package survives on disk is a process death mid-write.
3. **Packages are never deleted.** `context_store::` appears exactly once in
   the provider (`lib.rs:1008`, the write). There is no removal path anywhere,
   so every execution and every resume leaks one package file into
   `<state_dir>/context-packages/` forever. §9 fixes this as a side effect and
   must not be shipped without the cleanup.
4. **The provider already holds everything a background refresh needs.**
   `Provider.rest_client: Option<RestClient>` (`lib.rs:319`, set at `:361-363`)
   and `Provider.relay_self: Option<String>` (`lib.rs:326`, set at `:370-372`)
   are fields, not parameters. `spawn_git_probe` (`lib.rs:1739-1776`) is the
   shipped precedent for an off-loop `tokio::spawn` that clones
   `self.rest_client` and reports back without blocking the provider loop.
   **No `relay` threading through `on_turn` is required.**
5. **The sidecar's tool bodies are synchronous.** `overview` (`:161`),
   `history` (`:177`) and `search` (`:205`) take `&self` and return
   `Result<String, ErrorData>` with no `await`; `lib.rs:161`, `:171`, `:185`
   call them directly. A reload can be plain `std::fs` behind a
   `std::sync::RwLock`; no async conversion is needed.
6. **The sidecar's load path is already a full validation gauntlet.**
   `SessionContextState::load` (`session_context.rs:76-153`): absolute path,
   symlink/regular-file rejection, `validate_private_permissions`, size cap
   against `MAX_CONTEXT_PACKAGE_BYTES`, `reject_secret_material`, strict
   `deny_unknown_fields` decode **from the original bytes** (`:139-147`, whose
   comment explains why not from the `Value`), then `package.validate()`.
   Every reload must re-run this whole function, unchanged.
7. **The first-turn brief already addresses evidence by signed event id**, not
   by offset: `requestEventId`, `latestAssistantEventId`, `latestPlanEventId`,
   `terminalEventId`, `evidenceEventIds` are emitted by `BriefTurn::finish`
   (`coding_session_context.rs:465-479`). `first_history_offset` is held on
   `BriefTurn` (`:330`), set in `BriefTurn::new` (`:348-350`) and read **only**
   by the turn-ordering comparator (`:205-206`); it is never emitted. Event ids
   are this subsystem's existing stable address.
8. **Event ids are unique within a package.** `validate()` rejects "context
   history repeats event id" (`coding_session_context.rs:572-576`).
9. **Provenance already carries machine-checked invariants.**
   `CodingSessionContextProvenance::validate` (`coding_session_context.rs:623-671`)
   already enforces `includedHistoryItems == history.len()` (`:633-635`),
   `truncated == (omitted > 0)` (`:636-638`), `total == included + omitted`
   (`:639-649`), and `complete ⇒ total is Some` (`:650-652`). Add to this
   function; do not invent a second validation layer.
10. **The desktop transcript `item` is an open union; the receipt is not.**
    `status_item`'s consumer path
    (`codingSessionTranscriptItems.ts:89`, `:522-534`) tolerates unknown keys
    and unknown slugs by design (`:495-503`: "guessing prose for an unknown
    slug would be inventing a fact"). The lifecycle **receipt** decoder is
    strict — `resumed_without_context` requires
    `hasExactKeys(value.error, ["code","message"])` and an exact code
    (`codingSessionIngressPayloads.ts:202-216`). The transcript item is the
    cheap carrier; the receipt is not.
11. **The sidecar's isolation is deliberate, load-bearing, and told to the
    agent.** `session_context.rs:1-5`: "The package path is accepted only at
    process startup. Tool calls cannot redirect the server to another file,
    fetch relay data, sign events, or write provider-native state." The same
    promise is restated verbatim in the agent-visible instruction string:
    "This server is read-only and cannot access the relay, mutate files, or
    write provider-native state." (`crates/buzz-dev-mcp/src/lib.rs:197`).
12. **The agent cannot shell out to `buzz` to refresh anything.**
    `crates/buzz-session-provider/src/agent_fence.rs:39-44` records that the
    `BUZZ_` prefix fence deliberately removed the agent's ability to
    authenticate: "It used to work by accident… which made every session's
    agent indistinguishable from the provider itself on the wire." Project
    Pulse's live-CLI pattern (`crates/buzz-acp/src/pool.rs:1095-1137` —
    uncommitted work, so this anchor will drift; see §10) is
    therefore **not available** to rehydration.
13. **ACP has no mid-session capability push.** `mcpServers` is a parameter of
    `session/new`, `session/resume` and `session/load` only
    (`crates/buzz-acp/src/acp.rs:827-918`); `session/update` carries turn/UI
    frames. A new tool set requires a new provider-native session open.

---

## 3. Decisions fixed for this build

Not open questions.

1. **The sidecar gains no relay client, no credential, no signing key, and no
   write path.** Every clause of `session_context.rs:1-5` survives except one,
   named in decision 2.
2. **Exactly one guarantee is narrowed, deliberately and in writing:** the
   sidecar is pinned to a launcher-chosen *directory* instead of a
   launcher-chosen *file*, and serves the newest fully-validated package in
   it. Both the module doc (`session_context.rs:1-5`) **and** the agent-visible
   instruction string (`buzz-dev-mcp/src/lib.rs:197`) are rewritten in the
   same commit. Shipping the mechanism without both string edits is an honesty
   bug of the same severity as a crash.
3. **The refresh runs in the provider**, which already holds relay authority
   and already performs this exact projection at create and resume. It is a
   `tokio::spawn` modelled on `spawn_git_probe` (`lib.rs:1739-1776`), using
   `self.rest_client` / `self.relay_self` (§2 fact 4).
4. **Delivery is a file handoff, never an IPC socket.** No Unix-domain-socket
   broker, no new local auth boundary, no async conversion of the sidecar.
5. **Refresh writes a new generation file; nothing is ever overwritten.**
   `create_new(true)` stays on every write (§2 fact 2).
6. **The stable address of a history item is its signed source event id.** All
   cursor work uses it directly. There is no composite sort-key encoding — the
   projector's primary sort key is `CandidateHistory.timestamp_ms`
   (`context_projector.rs:814-816`), which is `envelope.timestamp`
   (`:1481`) and is **not** retained on the item (the item keeps `created_at`
   in *seconds*, `coding_session_context.rs:139-140`), so a sort-key cursor is not
   reconstructible in the sidecar. Do not attempt one.
7. **Offset paging keeps working**, and every offset-mode response says out
   loud that offsets are not stable across a refresh.
8. **`sourceEventCount` is reconciled by a structured breakdown that
   `validate()` enforces**, not by a prose note alone. A package whose numbers
   do not sum is rejected at write time.
9. **The package schema version bumps to 2** and readers accept `1..=2`. The
   provenance struct is `deny_unknown_fields`
   (`coding_session_context.rs:88`), so an *older* sidecar binary hard-fails on
   a package carrying the new field. The bump does not prevent that; it makes
   the failure message say "unsupported … package version 2" instead of an
   opaque unknown-field error. No version negotiation, no probe, no legacy
   dual-write path (see §10).
10. **Bail-out disclosure is a closed set of enumerated slugs, never free
    text.** Projector error strings interpolate event and command ids, and the
    storage arm formats an `io::Error` over a host path (`lib.rs:1014-1021`);
    publishing either into a signed 44225 is the same class of leak
    `SESSION_STATE.md:52` already flags for `toolName`.
11. **`SessionContinuity` (`session.rs`) is not changed.** The reason is known
    in `lib.rs` and never needs to cross into `session.rs`.
12. **Every added field on a *durable or signed* struct is additive-optional
    and omitted when absent.** This decision is scoped to the serde-serialized
    package and payload structs — `CodingSessionContextProvenance`
    (`coding_session_context.rs`), the transcript `item` bodies
    (`coding_session_payload.rs`) and `RehydrationMcpDescriptor` (`session.rs`).
    There, `Option` + `skip_serializing_if = "Option::is_none"`, matching the
    precedents at `coding_session_context.rs:97-98` (`complete_as_of`) and
    `coding_session_payload.rs:591-594` (`operator_pubkey`); keys are omitted,
    never emitted as `null`, because a `null` on a durable record is a claim
    that the provider *observed* an absence.

    **The MCP tool-response JSON built by `render(json!({…}))` in
    `session_context.rs` is deliberately the opposite**: it is a rendering for a
    reader, not a record, and it emits an **explicit `null`** wherever silence
    would be ambiguous — `nextCursor`, `nextOffset`, `cursorResolution` (§7.4),
    and `completeAsOfMs` / `ageSinceCompleteAsOfMs` (§8.1). H1 requires that
    last one: an omitted `completeAsOfMs` would read as "this response forgot to
    say", while an explicit `null` says "this package has no watermark." The
    existing envelope already sets this precedent — `nextOffset` is
    `(end < total).then_some(end)` and serializes as `null` at the end of a walk
    (`session_context.rs:200`), and the shipped test asserts exactly that
    (`page["nextOffset"].is_null()`, `:614`).
13. **Nothing in this plan generates prose about the session.** Every number,
    label and note is arithmetic over record fields, per the settled direction
    at `SESSION_STATE.md:153-163`.
14. **A refreshed package is written straight to its final generation path
    under `create_new(true)`; there is no temp-file-plus-`rename(2)` dance.**
    Chosen over an atomic-rename scheme because rename replaces its destination
    and would silently repeal the OS-enforced write-once guarantee (§2 fact 2)
    and break `never_overwrites_an_existing_execution_package`
    (`context_store.rs:183`), and because macOS has no portable
    `RENAME_NOREPLACE` — `renamex_np`/`link(2)` would be new platform-specific
    machinery for a window the reader already closes. The reader's own defence
    is the honest one: a candidate generation that does not fully validate is
    refused and the last good package keeps serving (H4), and every subsequent
    tool call retries. See §9.1 for the full crash argument.
15. **The `index` view shares the one 128 KiB page budget rather than getting
    a budget of its own.** One byte bound, one rule, one number to explain to
    the agent. The index view earns its keep by making each *item* small — a
    64-byte preview, no `author`, no constant `sourceKind`, no duplicated
    `cursor`, and a per-response `targets` legend instead of an inlined target
    struct on every row — not by making the *page* big. A dedicated larger
    budget was rejected because the only size that would deliver a
    whole-package walk in one call is >1 MB, which trades a hard error for a
    blown context window: the same trap §15.3 already names. §7.3 states the
    resulting per-call item count out loud instead of claiming a one-call walk.

### 3a. Honesty rules — requirements, not style

These are testable requirements. A slice that implements its mechanism and
skips its rule is not done.

- **H1.** Every number a context response prints is either reconciled inside
  that same response or stamped with the moment it was true.
- **H2.** A short page is never silent. A response that stopped early names
  which bound stopped it and how to continue.
- **H3.** A refresh that fails changes nothing on disk and is visible through
  a growing age, never through silence and never through a fresher-looking
  watermark.
- **H4.** A candidate package that fails any validation is refused and the
  last good package keeps serving. Never serve unvalidated data; never fall
  back to nothing.
- **H5.** Distinct failure causes produce distinct operator-visible strings.
  Ten bail-outs may not collapse to one slug.
- **H6.** An unknown slug renders its raw value rather than being dropped or
  guessed at (`codingSessionTranscriptItems.ts:495-503`).
- **H7.** Any guarantee string shown to an agent or a reader is updated in the
  same commit as the mechanism that narrows it.

---

## 4. Work order and branch discipline

Slices are ordered so that each is independently shippable and each earlier
one makes the next one honest.

| Slice | Ledger item | Depends on |
| --- | --- | --- |
| §5 — bail-out disclosure | item 1 (disclosure half) | nothing |
| §6 — provenance breakdown | item 6 | nothing |
| §7 — paging that scales and survives refresh | item 5 | §6, but only for the **v2 acceptance** (`coding_session_context.rs:554-559` must accept `1..=2` before a sidecar can serve a v2 package). §6 makes no sidecar-envelope edit; the shared `provenance_semantics()` rewrite lands in §8.2. |
| §8 — read-time staleness | item 4a | §6, §7 |
| §9 — bounded refresh | item 4b | §5 (needs the slug vocabulary), §7 (cursors), §8 (age reporting) |

**§5 and §6 have no dependency on the open session-stability work in
`SESSION_STATE.md §3` and can start immediately.** §9's live acceptance needs
two executions under one umbrella, which in turn wants `§3 item 1`'s
duplicate-create re-test to have happened first — otherwise a duplicated
create will confound the run.

Per `docs/INTEGRATION.md` and `CLAUDE.md`:

1. Work on a `wip/*` branch cut from the assembly. The current tree already
   carries an unrelated in-flight Project Pulse build — **59 dirty entries** as
   of 2026-08-19, verified with `git status --porcelain | wc -l`. It is much
   wider than "a new core module and a desktop feature": it spans
   `crates/buzz-core/src/{pulse.rs,kind.rs,lib.rs}`,
   `crates/buzz-acp/src/{pulse_fetch.rs,pool.rs,lib.rs,base_prompt.md}`,
   `crates/buzz-cli/src/{commands/pulse.rs,commands/mod.rs,lib.rs,links.rs}`,
   `crates/buzz-db/src/{event.rs,lib.rs,project_acl.rs}`,
   `crates/buzz-relay/src/{api/bridge.rs,handlers/{count,event,ingest,req}.rs}`,
   `crates/buzz-sdk/src/builders.rs`,
   `crates/buzz-test-client/tests/e2e_pulse.rs`,
   `conformance/project-pulse-fold/`, `desktop/src/features/project-pulse/`,
   `desktop/src/features/projects-container/`,
   `desktop/src/{app/routes*,shared/constants/kinds.ts,testing/e2eBridge.ts}`,
   `desktop/playwright.config.ts` and `preview-features.json`. **Preserve all of
   it.** This plan touches none of those files — verified zero overlap: no dirty
   entry under `buzz-session-provider/`, `buzz-dev-mcp/`, `coding_session_*`,
   `desktop/src/features/coding-sessions/lib/`, or `docs/SESSION_STATE.md`.
   `crates/buzz-core/src/lib.rs` is dirty (Pulse adds `pub mod pulse;` at `:63`)
   but it is module declarations only, with no re-export list, so §6.1's new
   struct needs **no edit there** and creates no conflict.
2. Let Brian exercise each slice before the next one starts.
3. Only the finalizer commits, with `git commit -s`.
4. Ledger edits (§1) land with the code, not after.

---

## 5. Slice A — item 1: name the reason the execution went Fresh

### 5.1 Carrier

The transcript status item, not the create receipt (§2 fact 10).

### 5.2 `crates/buzz-core/src/coding_session_payload.rs`

Add beside `status_item` (`:579-581`):

```rust
/// Closed set of reasons a create or resume lost its verified prior context.
pub const CONTEXT_UNAVAILABLE_REASONS: &[&str] = &[ /* the ten slugs in §5.5 */ ];

/// Build a status item that may name why continuity was lost.
///
/// `reason` is emitted only when it is a member of
/// [`CONTEXT_UNAVAILABLE_REASONS`]; anything else is dropped rather than
/// published, because this value enters a signed durable event.
pub fn status_item_with_reason(status: &str, reason: Option<&str>) -> serde_json::Value;

pub fn status_item(status: &str) -> serde_json::Value {
    status_item_with_reason(status, None)
}
```

`reason` is **omitted** when absent or unrecognized — never `null`
(decision 12). `status_item`'s **output shape is unchanged**, so nothing that
already reads a status item breaks. Of its three call sites, `:1956`
(`turn_dropped:queue_full`) keeps calling `status_item` untouched — it has no
continuity reason — while `:839` (create) and `:1167` (resume) migrate to
`status_item_with_reason` per §5.4, and produce byte-identical output whenever
the reason is `None`.

### 5.3 `crates/buzz-session-provider/src/context_projector.rs`

```rust
/// Map a projection failure to its stable, leak-free disclosure slug.
pub fn context_unavailable_reason(error: &ContextProjectionError) -> &'static str
```

Exhaustive over the four variants at `:183-196`:

| Variant | Slug |
| --- | --- |
| `Relay(_)` (`:186`) | `relay_query_failed` |
| `InvalidFact(_)` (`:189`) | `unverifiable_source_fact` |
| `Conflict(_)` (`:192`) | `context_fact_conflict` |
| `Bound(_)` (`:195`) | `source_exceeds_projection_bound` |

### 5.4 `crates/buzz-session-provider/src/lib.rs`

`prepare_rehydration_context` (`:923-1028`) returns a struct instead of an
`Option`:

```rust
struct RehydrationOutcome {
    descriptor: Option<RehydrationMcpDescriptor>,
    unavailable_reason: Option<&'static str>,
}
```

Each existing bail-out gets its slug; the happy path returns `None`. Note that
the `session_ref` / `genesis_ref` arms are **not failures**:

| Site | Slug |
| --- | --- |
| `:932-939` no `context_mcp_command` | `context_sidecar_unavailable` |
| `:940-947` command not absolute | `context_sidecar_path_invalid` |
| `:948-955` no `sessionRef` | `no_prior_execution` |
| `:956-963` no `genesisRef` | `no_prior_execution` |
| `:964-971` no relay handle | `relay_unavailable` |
| `:987-994` projection error | `context_unavailable_reason(&error)` (§5.3) |
| `:999-1006` brief encode | `brief_encode_failed` |
| `:1014-1021` package write | `package_write_failed` |

Then, on the **create** path:

- `create_disclosure_status` (`:2230-2238`) becomes
  `create_disclosure(continuity: &SessionContinuity, unavailable_reason:
  Option<&'static str>) -> Option<(&'static str, Option<&'static str>)>`.
  Only the `Fresh` arm ever carries a reason; `Rehydrated` never does.
- Call sites `:760-769` (outcome produced) and `:834-842` (outcome published)
  thread it into `payload::status_item_with_reason`.

And on the **resume** path, which is a genuinely different code path, not the
same one reached twice:

- `create_disclosure_status` **is not on the resume path at all.**
  `resume_session` publishes through its own inline
  `match startup.continuity` at `:1136-1161`, which never produces
  `session_fresh`: the `Fresh` and `RestartedWithoutContext` arms both emit
  `session_restarted_without_context`. **`:1136-1161` is therefore an edit
  site in its own right**, alongside `:1081-1090` (where the outcome is
  produced) and `:1163-1169` (where it is published).
- The reason is attached to `session_restarted_without_context` and **never**
  to `session_resumed` or `session_loaded`. On those two arms the adapter
  re-attached its own native conversation, so whether a verified package could
  be built is irrelevant to what the agent can see; naming a package failure
  there would disclose a fact the operator does not need and imply a loss that
  did not happen. `session_rehydrated` likewise never carries a reason —
  nothing was lost.
- Existing per-site `tracing` lines stay. The log keeps the detail; the wire
  gets the class.

### 5.5 Disclosure text

Two base prose rows can carry a reason clause, because two slugs disclose a
lost package: `session_fresh` → "Started fresh — no prior session context"
(create path), and `session_restarted_without_context` → "Restarted without
prior context" (resume path, `codingSessionTranscriptItems.ts:519`). The clause
below is appended to whichever base row applies; the clauses are written to
read correctly after either.

| Slug | Reader-facing clause |
| --- | --- |
| `no_prior_execution` | this is the session's first execution, so there was no prior work to carry |
| `context_fact_conflict` | conflicting signed facts were found for this session — for example two executions under one create — so verified history was withheld rather than guessed |
| `relay_unavailable` | the relay could not be reached to rebuild verified history |
| `relay_query_failed` | the relay query for verified history failed |
| `unverifiable_source_fact` | a source fact failed verification, so verified history was withheld |
| `source_exceeds_projection_bound` | this session's history is larger than the projector's bounds |
| `context_sidecar_unavailable` | no context sidecar is installed on this computer |
| `context_sidecar_path_invalid` | the configured context sidecar path is not absolute |
| `brief_encode_failed` | the verified brief could not be encoded |
| `package_write_failed` | the verified package could not be written to local storage |

### 5.6 `desktop/src/features/coding-sessions/lib/codingSessionTranscriptItems.ts`

- Export `CODING_SESSION_CONTINUITY_REASONS: ReadonlyMap<string, string>`
  beside `CODING_SESSION_CONTINUITY_STATUSES` (`:504-520`), holding §5.5.
- `buildStatusLifecycleItem` (`:522-534`) reads `item.reason` when it is a
  string, bounds it with the existing `safeString(..., 200)` helper (already
  applied to `status` at `:526-529`), and renders `${continuity} — ${clause}`.
  It applies to **both** reason-carrying base rows — `session_fresh` (`:506`)
  and `session_restarted_without_context` (`:519`).
- **H6:** a known status with an *unknown* reason renders the base continuity
  prose plus the raw slug in parentheses — never dropped, never guessed. An
  item with no `reason` renders exactly as it does today.

No mobile change: `mobile/lib` has no `session_fresh` consumer.

### 5.7 Slice A tests

`crates/buzz-core/src/coding_session_payload.rs` `#[cfg(test)]`:
- `a_status_item_without_a_reason_is_byte_identical_to_the_shipped_shape`
- `status_item_with_reason_emits_only_enumerated_reasons`
- `an_unrecognized_reason_is_omitted_not_published_as_null`

`context_projector.rs`:
- `every_projection_error_maps_to_a_stable_disclosure_slug` (exhaustive match
  over all four variants at `:183-196`)
- `no_disclosure_slug_contains_an_event_id_a_command_id_or_a_path_separator`

`crates/buzz-session-provider/src/lib.rs`:
- `every_bail_out_in_prepare_rehydration_context_names_a_reason`
- `a_conflict_bail_out_and_a_relay_outage_publish_different_slugs` — **the
  exact regression this slice exists to prevent**
- `a_first_ever_execution_discloses_no_prior_execution_not_a_failure`
- `a_resume_that_loses_context_names_its_reason_on_session_restarted_without_context`
- `a_native_resume_or_load_never_carries_a_package_reason`
- extend `every_create_continuity_maps_to_its_disclosure` (`:3028-3045`) to the
  new `(continuity, reason)` pairs

`desktop/src/features/coding-sessions/lib/codingSessionTranscriptItems.test.mjs`
— **this file does not exist yet; create it.** Every other module in that
directory has a sibling `*.test.mjs`, but `codingSessionTranscriptItems.ts`
does not, so there is no existing spec to extend:
- `a_fresh_status_with_a_known_reason_renders_the_reason_clause`
- `a_restarted_without_context_status_with_a_known_reason_renders_the_reason_clause`
- `a_fresh_status_with_an_unknown_reason_renders_the_raw_slug`
- `a_status_item_with_no_reason_renders_exactly_as_before`

---

## 6. Slice B — item 6: reconcile `sourceEventCount` against `totalHistoryItems`

Every term is already computed inline at `context_projector.rs:970-980` from
plain `input.*.len()` calls. **Nothing new is fetched.**

### 6.1 `crates/buzz-core/src/coding_session_context.rs`

```rust
/// Per-category accounting of the signed proof events behind one package.
///
/// The six terms sum to `CodingSessionContextProvenance::source_event_count`
/// by construction; only `transcript_events` can become history items, which
/// is why `sourceEventCount` exceeds `totalHistoryItems`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextSourceBreakdown {
    pub genesis_events: u64,                // always 1
    pub authority_link_events: u64,         // 2 x authority_links
    pub name_revision_events: u64,
    pub goal_revision_events: u64,
    pub generation_bookkeeping_events: u64, // 3 x generations
    pub transcript_events: u64,
}
```

- Add to `CodingSessionContextProvenance` (`:89-113`), immediately after
  `source_event_count` (`:104`):
  `#[serde(default, skip_serializing_if = "Option::is_none")] pub
  source_event_breakdown: Option<CodingSessionContextSourceBreakdown>`.
- `CODING_SESSION_CONTEXT_PACKAGE_VERSION` (`:22`) → `2`; add
  `pub const MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION: u64 = 1;` and widen the
  version check at `:554-559` to `1..=2` (decision 9).
- Extend `CodingSessionContextProvenance::validate` (`:623-671`): when the
  breakdown is present, the six terms must sum **exactly** to
  `source_event_count`, and `transcript_events >= included_history_items +
  omitted_history_items`. A breakdown that does not reconcile is a hard
  rejection, in the same function that already enforces the other four
  invariants.
- `coding_session_first_turn_brief` (`:190-262`): add
  `snapshot.sourceEventBreakdown`, and one rule line to the `rules` array
  (`:253-258`): `"sourceEventCount counts signed proof events, not content;
  sourceEventBreakdown reconciles it against totalHistoryItems"`.

### 6.2 `crates/buzz-session-provider/src/context_projector.rs`

- Replace `fn source_event_count` (`:970-980`) with
  `fn source_event_breakdown(input: &ContextProjectionInput) ->
  CodingSessionContextSourceBreakdown`, plus
  `impl CodingSessionContextSourceBreakdown { fn total(&self) -> usize }`.
  Keep a one-line `fn source_event_count` wrapper so `validate_source_bound`
  (`:982-1001`) is untouched.
- Populate `source_event_breakdown: Some(breakdown)` in the provenance literal
  at `:882-892`, alongside the existing `source_event_count` at `:887`.
- Add **one** synthesized note when `source_event_count != included + omitted`,
  stating real numbers only:

  > `sourceEventCount 214 includes 96 non-content proof events (1 genesis, 4 authority, 2 name, 1 goal, 88 per-generation bookkeeping) in addition to 118 transcript events; 118 became history items.`

  **Do not route this note through `record_omission_note` (`:920-932`).** That
  function opens with `notes.retain(|existing| !existing.starts_with("Omitted "))`
  (`:924`) — a deliberate de-duplication for the truncation note it owns, which
  is rewritten with a new count on every pass. Calling it for the delta note
  would **delete the truncation note recorded at `:860-865`**, turning a fix for
  one unexplained number into the silent loss of another disclosure. Add a
  sibling helper instead:

  ```rust
  /// Append a note that is not a truncation restatement.
  ///
  /// Unlike `record_omission_note`, this does not evict prior `Omitted …`
  /// notes; it only enforces the note budget.
  fn record_note(notes: &mut Vec<String>, note: String) -> bool
  ```

- **The delta note is computed inside the package-bytes retry loop
  (`:878-917`), not before it.** That loop rebuilds provenance every iteration
  and drops one more history item each pass (`:911-912`), so `included` and
  `omitted` change under a note computed once — it would state a stale "118
  became history items" on the package that actually shipped 96. `notes` is
  already cloned per iteration at `:891`; build the delta note from that
  iteration's `history.len()` and `omitted` and push it onto **that clone**, so
  the base `notes` vector is never mutated by a discarded iteration.
- **The delta note fails soft; it is the only thing in this plan that does.**
  The note budget is genuinely reachable: `MAX_CONTEXT_PROVENANCE_NOTES = 32`
  (`coding_session_context.rs:28`), and the source's own coverage notes are
  copied in first (`:846-851`) with `validate_coverage` permitting up to 32 of
  them (`:955-959`), on top of the three `record_omission_note` sites
  (`:852-859`, `:860-865`, `:913-916`). So `record_note` returns `false` rather
  than `Err` when no capacity remains, and the note is dropped. That is honest
  here and **only** here, because the reconciliation itself lives in the
  structured `sourceEventBreakdown` field, which `validate()` enforces (§6.1) —
  the prose note is a convenience restatement, not the disclosure. A truncation
  note has no structured backup, which is exactly why `record_omission_note`
  still hard-fails on overflow (`:925-929`) and must keep doing so.

### 6.3 Slice B tests

`crates/buzz-core/src/coding_session_context_tests.rs` (the file `b9de9a6d`
created for exactly this kind of coverage):
- `source_breakdown_terms_sum_to_source_event_count`
- `a_package_whose_breakdown_does_not_sum_is_rejected`
- `a_package_whose_transcript_events_are_fewer_than_included_plus_omitted_is_rejected`
- `a_version_1_package_without_a_breakdown_still_validates`
- `a_version_3_package_is_rejected_by_name`
- `the_first_turn_brief_carries_the_source_breakdown`

`context_projector.rs`:
- `the_breakdown_names_every_category_the_formula_counts` — fixture with a
  genesis, 2 authority links, 1 name revision, 1 goal revision and 3
  generations, so all six terms are non-trivial
- `the_delta_note_states_the_real_numbers_and_is_emitted_only_on_a_delta`
- `the_delta_note_does_not_evict_the_truncation_note` — the C3 regression:
  a fixture that both truncates (so `:860-865` fires) and has a delta, asserting
  **both** notes survive into the shipped package
- `a_package_trimmed_by_the_byte_loop_reports_the_trimmed_history_count_in_its_delta_note`
- `a_source_that_arrives_with_a_full_note_budget_still_projects_and_still_reconciles`
  — 32 coverage notes in, delta note dropped, `sourceEventBreakdown` present

---

## 7. Slice C — item 5: paging that scales and survives a refresh

`crates/buzz-dev-mcp/src/session_context.rs` and its tool descriptions in
`crates/buzz-dev-mcp/src/lib.rs`.

### 7.1 Why cursors are in scope

Slice E (§9) makes the served package change under a reader. The projector
truncates the **oldest** items to satisfy the item bound
(`context_projector.rs:837-844`, `retain_from = len - max_history_items`), so
a refresh that appends new items to a package already at the bound shifts every
offset downward. Offset paging across that refresh silently skips items — a
brand-new instance of exactly the class of defect this bite exists to remove
(H1). The minimum honest fix is a stable address; per decision 6 that address
is the signed source event id, which is already how the first-turn brief cites
evidence (§2 fact 7) and is already unique per package (§2 fact 8).

### 7.2 Constants

Replace `:24-27`:

```rust
const DEFAULT_HISTORY_LIMIT: usize = 200;
const MAX_HISTORY_LIMIT: usize = MAX_CONTEXT_HISTORY_ITEMS; // 4_096, imported from buzz-core
const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 200;
const MAX_HISTORY_PAGE_BYTES: usize = 128 * 1024;
const MAX_SEARCH_PAGE_BYTES: usize = 128 * 1024;
const MAX_INDEX_PREVIEW_BYTES: usize = 64;
```

Importing `MAX_CONTEXT_HISTORY_ITEMS` (`coding_session_context.rs:24`) instead
of restating `4096` is what stops the ceiling and the page cap drifting apart
again.

Note **which** `20` this removes. The surviving `20` in this file is
`DEFAULT_SEARCH_LIMIT` at `:26`, not `MAX_SEARCH_LIMIT` at `:27` (which is
already `50`) — §1.2 trap (b) states this correctly and it is easy to invert.
Raising `DEFAULT_SEARCH_LIMIT` to `50` is what retires the last literal `20`
a `search_session` limit error could ever name.

### 7.3 `session_history` contract

Params gain three optional fields; `offset` and `limit` keep working:

```
{ since?: string,            // cursor: a signed source event id
  view?: "full" | "index",   // default "full"
  offset?: usize, limit?: usize }
```

- `since` resolves by event-id lookup into `package.history` (build a
  `HashMap<&str, usize>` once at load). The page starts at the item **after**
  it.
- **Cursor miss is disclosed, never silently restarted (H2/H4).** If `since`
  is not in the package, return `cursorResolution: "not_in_package"` together
  with `provenance.omittedHistoryItems` and `completeAsOf`, and **no items** —
  the agent must learn that the truncation boundary moved past its cursor
  rather than silently re-reading from zero. That response is
  `stoppedBy: "cursorMiss"` (see §7.4) — **not** `"end"`, which would claim the
  walk finished, and not `"limit"`, which would claim a bound was hit.
- `view: "index"` returns metadata only per item:
  `{eventId, createdAt, eventSeq, turnId, targetIndex, role, itemKind,
  contentBytes, textPreview}`, with `textPreview` bounded by
  `MAX_INDEX_PREVIEW_BYTES` (64). `targetIndex` points into a per-response
  `targets` legend — the deduplicated `CodingSessionTarget`s appearing in the
  page — because a package spans sibling executions and generations, and an
  index that could not say *which execution* an item came from would not serve
  the sibling-visibility purpose this whole bite exists for. Deliberately
  **omitted** from the index item: `author` (the provider-authority pubkey, the
  same 64-hex value on essentially every item, and already in the full view),
  `sourceKind` (which `history_item_view` does emit at `:294`, but which
  `validate()` pins to exactly `44225` for every v1/v2 history item —
  `coding_session_context.rs:678-680` — so an index column for it would be a
  constant), and a separate `cursor` key (the cursor *is* `eventId`, per
  decision 6; emitting both would be 76 wasted bytes per item restating the same
  value).
- **The index-view arithmetic, stated rather than asserted.** A lean index item
  is still about **330–350 bytes** — the 64-hex `eventId` alone is 76 bytes on
  the wire, `turnId` is a 36-char UUID, and the preview is up to 64. Against the
  shared 128 KiB budget (decision 15) that is roughly **380 items per call**, so
  the 4 096-item ceiling is a **bounded ~11-call walk**, not one or two calls.
  Claiming otherwise would be the same species of comfortable-guess this plan
  exists to remove. What actually retires "untenable as sessions grow" is
  therefore narrower and true: the observed 101-item session is **one** call in
  either view; the wall is a byte budget the response *names* (`stoppedBy`)
  and hands you a cursor past, instead of a hard `invalid_params` error at 20;
  and the walk is monotonic across a refresh, which offsets are not.
- `view: "full"` uses the existing `history_item_view` (`:279-304`) and stops
  at `MAX_HISTORY_PAGE_BYTES` **after emitting at least one item**, so a single
  oversized item is never made unfetchable (preserve the intent of the existing
  oversize handling at `:281-289`).
- Every item in the `full` view carries `cursor` (its event id); in the `index`
  view `eventId` is the cursor and is labelled as such in the tool description.

### 7.4 Response envelope (all three tools)

Added keys:

```
"stoppedBy": "limit" | "pageBytes" | "end" | "cursorMiss",
"nextCursor": string | null,
"nextOffset": usize | null,            // unchanged semantics
"offsetStability": "unstable_across_refresh",
"cursorResolution": "resolved" | "not_in_package" | null,
"targets": [CodingSessionTarget]       // index view only, the §7.3 legend
```

- `packageVersion` is **not** in this list: `overview` already emits it
  (`session_context.rs:167`). The work is to add it to `history` and `search`,
  which do not, so all three tools agree.
- `stoppedBy` has four arms, not three. `"cursorMiss"` is the zero-item
  `cursorResolution: "not_in_package"` response from §7.3; without it that
  response would have to lie in one of the other three arms.
- `nextCursor`, `nextOffset` and `cursorResolution` are emitted as **explicit
  `null`**, not omitted, per decision 12's second paragraph — this is a
  rendering for a reader, and the shipped envelope already does exactly this
  for `nextOffset` (`session_context.rs:200`).
- `offsetStability` is emitted whenever `offset` was used, and is the honest
  label for what §9 does to positions (H1).

### 7.5 `search_session`

Same byte budget (`MAX_SEARCH_PAGE_BYTES`), same `stoppedBy`, same per-result
`cursor`. Search remains offset-paginated over *matches* — a match list is
recomputed per call and is not a durable sequence — but each result names its
item's cursor so a follow-up `session_history { since }` is exact.

### 7.6 Tool descriptions — `crates/buzz-dev-mcp/src/lib.rs`

Rewrite `:167` (history) and `:178` (search) to state the new limits, the
byte budget, `view`, and cursor semantics. `:167` says today "limit defaults to
100 and is capped at 200"; `:178` says "limit defaults to 20 and is capped at
50". Both become false the moment §7.2 lands, and both are read by the agent at
every session start (H7).

### 7.7 Slice C tests

`crates/buzz-dev-mcp/src/session_context.rs` `#[cfg(test)]`:
- keep `one_history_call_can_retrieve_the_observed_101_item_session`
  (`:580-615`) **unchanged** — it is the ledger's anchor for item 5
- `the_whole_package_ceiling_walks_by_cursor_with_no_error_and_a_bounded_call_count`
  (`MAX_CONTEXT_HISTORY_ITEMS` items, `view: "index"`; assert the walk completes,
  that every page names `stoppedBy` and hands back a usable `nextCursor`, that
  no call returns `invalid_params`, and that the call count is under a stated
  ceiling — **not** that it is one call, which §7.3's arithmetic refutes)
- `an_index_page_is_at_least_three_times_denser_than_a_full_page_for_the_same_items`
  — the honest form of the "index is cheaper" claim
- `a_full_view_page_that_hits_the_byte_budget_sets_stopped_by_page_bytes_and_a_next_cursor`
- `one_oversized_item_is_still_returned_alone_rather_than_made_unfetchable`
- `a_since_cursor_returns_only_the_items_after_it`
- `a_since_cursor_absent_from_the_package_reports_not_in_package_and_returns_no_items`
- `a_cursor_miss_is_stopped_by_cursor_miss_not_by_end`
- `an_index_item_names_which_execution_produced_it_through_the_targets_legend`
- `walking_a_package_by_cursor_yields_every_item_exactly_once`
- `an_offset_response_always_labels_offset_stability`
- `search_results_carry_a_cursor_and_share_the_byte_budget`
- `no_tool_can_produce_a_limit_error_naming_a_bound_it_does_not_enforce`

---

## 8. Slice D — item 4a: read-time staleness

`complete_as_of` is a watermark, not an age. A watermark alone does not tell
the agent **how old** the snapshot is at the moment it reads it. The sidecar
and the projector run on the same host, so the sidecar can compute the delta
with `SystemTime::now()` — no new dependency, no capability change.

### 8.1 Every tool response gains

```json
"staleness": {
  "readAtMs": 1755640000000,
  "projectedAtMs": 1755639100000,
  "completeAsOfMs": 1755639100000,
  "ageSinceProjectionMs": 900000,
  "ageSinceCompleteAsOfMs": 900000,
  "stale": true,
  "clock": "this server's wall clock; the projector ran on the same host"
}
```

- `completeAsOfMs` is `null` for a legacy or incomplete package, and then
  `ageSinceCompleteAsOfMs` is `null` too — **never** silently substituted with
  `projectedAtMs` (H1).
- `const STALE_AFTER_MS: i64 = 15 * 60 * 1000;`, documented in code as *a
  disclosure trigger, not a correctness boundary*. The raw ages sit beside it
  so a reader can disagree with the threshold.

### 8.2 `provenance_semantics()` (`session_context.rs:271-277`) grows

Keep `complete` and `totalHistoryItems` verbatim. Replace `sourceEventCount`
and add four entries:

- `sourceEventCount` — "All signed facts retained in the verified proof graph.
  provenance.sourceEventBreakdown reconciles it exactly: genesisEvents +
  authorityLinkEvents + nameRevisionEvents + goalRevisionEvents +
  generationBookkeepingEvents + transcriptEvents == sourceEventCount. Only
  transcriptEvents become history items, which is why sourceEventCount exceeds
  totalHistoryItems."
- `readTime` — "readAtMs is this server's wall clock when this response was
  rendered, on the same host that projected the package. ageSinceCompleteAsOfMs
  is how long ago the snapshot watermark was."
- `staleness` — "This package is a snapshot, not a live view. Nothing published
  after completeAsOf is represented here. When stale is true, say so to the
  operator before relying on this package to describe what the session is doing
  now."
- `pagination` — "A page ends at limit or at the response byte budget,
  whichever comes first; stoppedBy names which. Offsets are not stable across a
  package refresh; a cursor is."
- `packageGeneration` — "The launcher may write a newer verified package for
  this execution while it runs. Each response names the generation it served; a
  rising packageGeneration with a rising completeAsOf means this server picked
  up refreshed evidence."

### 8.3 `crates/buzz-session-provider/src/session.rs:60`

Append to `REHYDRATED_BOOTSTRAP_PREFIX`: *"Every context tool response carries
readAtMs and ageSinceCompleteAsOfMs. Treat the package as a snapshot of that
age, not as the session's current state, and call session_overview again before
making any claim about what a sibling execution is doing now."*

### 8.4 Slice D tests

- `every_response_reports_its_read_time_and_snapshot_age`
- `a_snapshot_older_than_the_stale_threshold_is_labelled_stale`
- `a_package_with_no_complete_as_of_reports_a_null_watermark_and_a_null_age`
- `provenance_semantics_reconciles_source_event_count_against_total_history_items`
- `the_bootstrap_prefix_instructs_the_agent_about_snapshot_age`
  (`session.rs` `#[cfg(test)]`)

---

## 9. Slice E — item 4b: bounded refresh

The narrowest mechanism that gives real mid-session freshness without a relay
client in the sidecar, without an IPC broker, and without a new ACP capability.

**Shape: generation directory, write-once per file, newest-valid-wins on read,
pull at tool-call time, push attempted at turn start.**

### 9.1 `crates/buzz-session-provider/src/context_store.rs`

```rust
pub const CONTEXT_PACKAGE_GENERATIONS_RETAINED: usize = 3;

/// Write generation `seq` of one execution's package.
pub fn write_context_package_generation(
    state_dir: &Path, package_id: &str, seq: u64,
    package: &CodingSessionContextPackage,
) -> Result<PathBuf, ContextStoreError>;

/// Newest generation sequence present, if any.
pub fn latest_context_package_generation(state_dir: &Path, package_id: &str)
    -> Result<Option<u64>, ContextStoreError>;

/// Keep the newest `keep` generations; unlink the rest.
pub fn prune_context_package_generations(state_dir: &Path, package_id: &str, keep: usize)
    -> Result<(), ContextStoreError>;

/// Remove one package directory entirely.
pub fn remove_context_packages(state_dir: &Path, package_id: &str)
    -> Result<(), ContextStoreError>;

/// Remove every package directory. Startup-only — see §9.3's restart mapping.
pub fn remove_all_context_packages(state_dir: &Path) -> Result<(), ContextStoreError>;
```

- Layout: `<state_dir>/context-packages/<package_id>/<seq:010>.json`, the
  directory 0700 and each file 0600, reusing the existing
  `reject_symlink_or_non_directory` (`:75-84`),
  `set_private_directory_permissions` (`:105-115`) and
  `set_private_file_permissions` (`:117-127`). `package_id` keeps the existing
  UUID guard (`:39-40`) — both ids that reach this function are UUIDs
  (`target.session_id` on create, the minted `package_id` on resume), so the
  guard needs no relaxation and a path component can never be attacker-shaped.
- Each generation is opened `create_new(true)` **at its final path**, so
  per-file write-once survives verbatim and
  `never_overwrites_an_existing_execution_package` (`:183`) keeps holding
  unchanged.
- **There is no temp-file-plus-`rename(2)` step** (decision 14). It is worth
  spelling out why, because "write to a temp name then rename atomically" is the
  reflex: `rename(2)` *replaces* its destination silently. Adopting it would
  quietly repeal the OS-enforced write-once guarantee this subsystem's safety
  argument rests on (§2 fact 2) and would make `context_store.rs:183` — which
  asserts a second write to the same path **errors** — fail. macOS also has no
  portable `RENAME_NOREPLACE`, so a no-clobber rename means `renamex_np` or a
  `link(2)`/`unlink(2)` dance: new platform-specific machinery bought for a
  window the reader already closes.
- **What closes that window instead.** A write that fails partway already
  unlinks its own partial file (`:58-62`), so the only way a truncated
  generation survives is process death mid-write. The sidecar's answer to that
  is the one it gives to every other bad candidate: the file fails the `load()`
  gauntlet, the candidate is **refused**, the last good generation keeps serving
  (H4), and the response says `refreshRefused: true` (§9.4). The next successful
  refresh writes a higher `seq` — `latest_context_package_generation` returns the
  highest sequence *present* on disk, including the corpse, so the next sequence
  is always above it — and the reader moves on by itself. Self-healing without a
  syscall trick.
- `write_context_package` becomes `…_generation(.., 0, ..)`. Generation 0 is
  the open-time package.
- **`remove_context_packages` is called from `stop_session`
  (`lib.rs:1181-1202`), keyed on the execution's package id (§9.2), and
  `remove_all_context_packages` is called from `Provider::recover`
  (`lib.rs:388`) at startup.** Today nothing ever deletes a package (§2 fact 3);
  shipping generations without this turns a slow leak into a fast one. The
  startup sweep is safe and total: no ACP subprocess from a previous incarnation
  survives a provider restart, so at the moment `recover()` runs no reader holds
  any package in that directory.

### 9.2 `crates/buzz-session-provider/src/session.rs`

- `RehydrationMcpDescriptor` (`:74-82`) gains `pub package_dir: PathBuf` **and
  `pub package_id: String`.**
- **The `package_id` field is not optional bookkeeping — without it the
  generation directory has no key.** The id that names the directory is
  *different on the two paths that create one*: `create_session` passes
  `&target.session_id` (`lib.rs:766`), while `resume_session` mints a fresh
  `Uuid` into a local `package_id` (`lib.rs:1080`) and passes that
  (`:1081-1090`). `SessionRecord` (`state.rs:100`+) persists **neither** — it
  has `session_id`, `session_ref`, `genesis_ref`, `resume_cursor` and no package
  field at all. So a `stop_session` cleanup keyed on `plan.target.session_id`
  would silently never find a resume-created directory, and the refresh task
  would have no directory to write generation *n+1* into. Carrying the id on the
  descriptor is what makes both reachable, and the descriptor is already the
  value that survives from `prepare_rehydration_context` into `CreateRequest`
  (`lib.rs:777`, `:1098`).
- `rehydration_mcp_servers` (`:714-745`) sets **both** env vars on the single
  `buzz-session-context` server: the existing `BUZZ_SESSION_CONTEXT_PACKAGE`
  (`:741`) and the new `BUZZ_SESSION_CONTEXT_PACKAGE_DIR`. The absolute-path
  guard at `:718-722` extends to the directory, fail-closed as it is today.
  `package_id` is **not** exported to the MCP subprocess — the directory path
  already carries it, and the fence's rule is that the sidecar learns paths, not
  identifiers it could use to construct new ones.

### 9.3 `crates/buzz-session-provider/src/lib.rs`

Modelled directly on `spawn_git_probe` (`:1739-1776`), which exists precisely
because a synchronous fetch on this loop delays every other session:

```rust
const CONTEXT_REFRESH_MIN_INTERVAL_MS: i64 = 60_000;

struct ContextRefreshState { package_id: String, next_seq: u64, last_refresh_ms: i64 }

fn spawn_context_refresh(&mut self, session_id: &str)
```

- Bookkeeping in `context_refresh: HashMap<String, ContextRefreshState>` on
  `Provider`, keyed by `session_id`, exactly like `git_probe_generation`
  (`:305`, `:348`). The entry is inserted when a session opens with a
  rehydration descriptor, carrying that descriptor's `package_id` (§9.2). **No
  provider state-file schema change.**
- **What happens to that mapping across a provider restart, said out loud:**
  `context_refresh` is in-memory only, and `SessionRecord` (`state.rs:100`+)
  does not persist a package id, so a restart loses every session→package-id
  binding. That is correct rather than a gap, and the reason is that a restart
  has already destroyed the thing the binding pointed at: no ACP subprocess
  survives it, so no sidecar is reading any of those directories. The recovery
  path is the one the code already takes — `Provider::recover` (`:388`) detaches
  every still-open session, and bringing one back is `resume_session`, which
  mints a **fresh** `package_id` (`:1080`), projects a fresh package, and writes
  generation 0 into a **new** directory. The old directories are therefore
  unreferenced garbage from the moment the provider dies, which is exactly why
  `remove_all_context_packages` runs inside `recover()` (§9.1) rather than
  trying to reconstruct a mapping that no longer means anything. **Do not add a
  package id to `SessionRecord` to "fix" this** — it would persist a pointer to
  a file whose only reader is already dead.
- Clones `self.rest_client` (`:319`) and `self.relay_self` (`:326`) plus the
  record's `channel_id` / `session_ref` / `genesis_ref`, builds the same
  `ContextProjectionRequest` as `:972-979` with `generated_at = now_ms()`, and
  calls the same `fetch_and_project_session_context` (`:980-985`). **No new
  projection logic and no new trust surface.**
- Then `write_context_package_generation(state_dir, &package_id, seq, ..)` +
  `prune_context_package_generations(.., CONTEXT_PACKAGE_GENERATIONS_RETAINED)`.
  Nothing is sent through `session_events_tx`; the sidecar picks it up off
  disk.
- **Trigger:** the `TurnDecision::Start` arm (`:1237-1252`), immediately before
  `self.state.consume_command` at `:1260`. Guards: the execution has a package
  directory, `session_ref` and `genesis_ref` are both present, and
  `now_ms() - last_refresh_ms >= CONTEXT_REFRESH_MIN_INTERVAL_MS`. A turn costs
  seconds to minutes; one bounded relay fetch plus proof-graph verification per
  minute per active rehydrated execution is well inside that.
- **A refresh that fails logs at `csp::context` and changes nothing on disk.**
  The previous generation keeps serving and its age keeps climbing in every
  response (H3). Reuse `context_unavailable_reason` (§5.3) for the log slug so
  create-time and refresh-time failures speak one vocabulary.
- `on_turn` (`:1204`) needs **no signature change** and `:636` needs no edit
  (§2 fact 4). Both the minimal and durable proposals threaded a `relay`
  parameter here; the fields make it unnecessary.

### 9.4 `crates/buzz-dev-mcp/src/session_context.rs`

- `pub const SESSION_CONTEXT_PACKAGE_DIR_ENV: &str =
  "BUZZ_SESSION_CONTEXT_PACKAGE_DIR";`
- `SessionContextState` (`:64-66`) becomes
  `{ loaded: RwLock<LoadedPackage>, dir: Option<PathBuf> }` with
  `LoadedPackage { seq: u64, package: CodingSessionContextPackage, index: HashMap<String, usize> }`
  (the event-id index from §7.3). `std::sync::RwLock`; the tool bodies stay
  synchronous (§2 fact 5).
- **Drop the `#[derive(Clone)]` at `:63` in the same edit.** `RwLock` is not
  `Clone`, so leaving the derive in place does not compile. Nothing needs it:
  the only construction site wraps the state in an `Arc` immediately
  (`buzz-dev-mcp/src/lib.rs:149`, `state: Arc::new(state)`), which is also why
  `RwLock` rather than `Arc<RwLock<…>>` is the right shape here — the sharing
  already happens one level up.
- `load_from_env` (`:69-74`) prefers `SESSION_CONTEXT_PACKAGE_DIR_ENV` (highest
  valid seq in it) and falls back to `SESSION_CONTEXT_PACKAGE_ENV` (`:22`).
- `fn reload_if_newer(&self)` runs at the head of `overview` (`:161`),
  `history` (`:177`) and `search` (`:205`): one directory read for the highest
  seq; if it exceeds the loaded seq, run the **entire existing `load()`
  gauntlet** (`:76-153`) on the candidate. **A candidate that fails any check
  is refused and the last good package keeps serving** (H4); the response then
  carries `"refreshRefused": true`.
- A refused candidate is **not** retried against lower generations within the
  same call, and the loaded seq does **not** advance past it. The next tool call
  simply tries again, and the next successful provider write lands at a higher
  seq and supersedes it (§9.1). One rule, no descent loop.
- Responses gain `"packageGeneration": <seq>` and
  `"generationChangedSincePreviousCall": bool` — an implicit reload mid-
  conversation changes provenance under an agent that may already have quoted
  `completeAsOf`, so it must be told (H1).

### 9.5 The guarantee delta, stated out loud (decision 2, H7)

Both strings change in the same commit:

- `session_context.rs:1-5` module doc → "The package **directory** is accepted
  only at process startup. Tool calls cannot redirect the server outside it,
  fetch relay data, sign events, or write provider-native state. Within that
  directory the server serves the newest package the launcher wrote,
  re-validated in full before it is served."
- `crates/buzz-dev-mcp/src/lib.rs:197` agent-visible instructions → "This
  server holds no relay credentials and cannot query the relay, sign events, or
  write provider-native state. The launching provider — which does hold relay
  authority — may write a newer verified package for this session while it
  runs; this server serves the newest one it can fully validate and names the
  generation in every response."

### 9.6 Slice E tests

`context_store.rs`:
- `a_refresh_writes_a_new_generation_and_never_overwrites_one`
- `writing_the_same_generation_sequence_twice_errors` — decision 14's guarantee
  at the *final* path, the thing a `rename(2)` implementation would silently
  lose
- `a_generation_whose_write_fails_partway_leaves_no_file_behind` (`:58-62`)
- `the_next_sequence_is_above_a_corrupt_generation_left_by_a_crash`
- `pruning_keeps_the_newest_generations_and_removes_the_rest`
- `stopping_an_execution_removes_its_package_directory`
- `provider_startup_sweeps_every_leftover_package_directory`
- `the_generation_directory_is_0700_and_every_generation_file_is_0600`
- keep `never_overwrites_an_existing_execution_package` (`:183`) unchanged

`lib.rs`:
- `a_refresh_that_fails_leaves_the_previous_generation_serving`
- `a_refresh_is_skipped_inside_the_minimum_interval`
- `a_turn_start_does_not_block_on_the_refresh`
- `an_execution_with_no_session_ref_never_schedules_a_refresh`
- `a_resume_created_package_directory_is_removed_on_stop` — the C5 regression:
  keyed on the minted `package_id` (`:1080`), **not** `target.session_id`, so a
  cleanup that used the session id would leak this directory forever

`session.rs`:
- extend the existing rehydration-MCP fence test:
  `the_rehydration_mcp_server_carries_the_package_directory_and_no_credential`

`session_context.rs`:
- `a_newer_generation_in_the_package_directory_is_picked_up_on_the_next_tool_call`
- `a_corrupt_newer_generation_is_refused_and_the_last_good_package_keeps_serving`
- `a_newer_generation_with_loose_permissions_is_refused`
- `a_tool_argument_can_never_name_a_package_outside_the_launcher_directory`
- `legacy_single_file_mode_still_serves_all_three_tools`
- `a_cursor_issued_before_a_generation_change_still_resolves_after_it`

---

## 10. Explicitly deferred

Do not implement any of these while executing this plan. Each is named here so
a reviewer can see it was considered and rejected on evidence, not missed.

| Deferred | Why |
| --- | --- |
| A relay-fetch tool or relay credential in the sidecar | Contradicts `session_context.rs:1-5` and the agent-visible promise at `buzz-dev-mcp/src/lib.rs:197`, and puts relay auth in a process whose stdio the agent drives. |
| A `refresh_session_context` tool the agent calls | Still needs relay access in the sidecar, or a broker; §9's pull-at-tool-call gives the same freshness without widening the tool surface or changing the `session_context_personality_lists_only_read_only_context_tools` guarantee (`buzz-dev-mcp/src/lib.rs:302-308`). |
| A Unix-domain-socket refresh broker (`context_refresh.rs`) | Rejected in favour of §9's file generations: `self.rest_client` (`lib.rs:319`) + the `spawn_git_probe` precedent (`:1739-1776`) already do the work, and a socket adds a new local IPC/auth surface and forces the sidecar's sync tool bodies async. |
| Copying Project Pulse's live shell-out (`BUZZ_PULSE_PROJECT` + `buzz pulse digest`, `crates/buzz-acp/src/pool.rs:1095-1137`) | Verified impossible: `agent_fence.rs:39-44` removed the agent's ability to authenticate `buzz` on purpose. Pulse content also need not be a signed proof graph; rehydration history must be. **Anchor warning:** `pool.rs` is *uncommitted* Project Pulse work (§4), so `:1095-1137` was exact on 2026-08-19 and will drift as that build lands. If the lines no longer match, re-find it by the `BUZZ_PULSE_PROJECT` literal rather than trusting the number. |
| Mid-session `mcpServers` push over ACP | `mcpServers` appears only on `session/new` / `session/resume` / `session/load` (`crates/buzz-acp/src/acp.rs:827-918`). Would need adapter changes we do not own. |
| Overwriting the package in place | Refused by the kernel (`context_store.rs:49-50`) and by a dedicated test (`:183`). |
| Atomic temp-file + `rename(2)` publication of a generation | Decision 14. `rename(2)` replaces its destination, repealing the write-once guarantee and breaking `context_store.rs:183`; macOS lacks a portable `RENAME_NOREPLACE`. The reader's validate-or-refuse path (H4) closes the same window with no new machinery — see §9.1. |
| Tearing down and re-opening the provider-native session to deliver a fresh package | The only refresh the code supports today, and it destroys the adapter's in-process conversation state — the exact thing rehydration exists to work around. |
| A model-written summary, or a summary-first package schema | `SESSION_STATE.md:153-163` settled the opposite: a generated summary manufactures an unverified claim. |
| Sidecar version negotiation (`--context-package-version` probe, v1 legacy dual-write) | The desktop resolves the sidecar from its own bundle (`desktop/src-tauri/src/session_provider/supervisor.rs:583`, `resolve_command("buzz-dev-mcp")`), so skew is a dev-override hazard on `BUZZ_CSP_CONTEXT_MCP_COMMAND` (`crates/buzz-session-provider/src/config.rs:127`), not a release one. Decision 9's version bump makes that failure legible; a negotiation protocol is not worth its cost. Record the trap in `SESSION_STATE.md §3a` instead. |
| A per-turn index in `session_overview` | `coding_session_first_turn_brief` (`coding_session_context.rs:190-262`) already returns a per-turn evidence index with event ids, and `overview` already returns it (`session_context.rs:167`). A second one would be a duplicate representation. |
| Publishing the raw projector error text as the bail-out reason | Those strings interpolate command/event ids (`context_projector.rs:656`) and the storage arm formats a host path (`lib.rs:1014-1021`). |
| Carrying the reason on the create receipt | The desktop receipt decoder is strict — `hasExactKeys(value.error, ["code","message"])` (`codingSessionIngressPayloads.ts:202-216`). |
| Relay-durable checkpoint kinds 44231 / 44232 | The longer-horizon track (`SESSION_STATE.md:185-186`); not a staleness-disclosure fix. |
| Everything in `SESSION_STATE.md §2` items 2, 3, 7–17 | Out of scope. Item 3's live re-test and item 1's *cause* re-test are `§3` work, not this plan's. |

---

## 11. Live acceptance

Green tests answer a different question than live use. Each step names the
artifact to cite; a completion report that asserts an outcome without one is
not evidence.

**L1 — item 1: the reason reaches the operator.** Read each row both in the
desktop transcript and on the wire:

```bash
buzz --format compact sessions transcript --channel <uuid> --session <id> \
  | jq 'select(.item.kind=="status")'
```

- **L1a** Create a session in a channel with no prior umbrella →
  `reason: "no_prior_execution"`; desktop reads "…— this is the session's
  first execution…".
- **L1b** Start the provider with `BUZZ_CSP_CONTEXT_MCP_COMMAND`
  (`config.rs:127`) unset — or pointed at a relative path — then Add provider on
  an existing umbrella → `context_sidecar_unavailable` (or
  `context_sidecar_path_invalid`), a **visibly different** row from L1a.
  **This replaces the obvious-looking "point the desktop at an unreachable
  community → `relay_unavailable`" test, which cannot fire and would be recorded
  as a failed slice.** The `relay_unavailable` arm (`lib.rs:964-971`) is reached
  only when `relay` is `None`, and the sole production call site passes
  `Some(&*relay)` (`lib.rs:598`) — worse, the create command *itself* arrives
  over the relay, so an unreachable community produces no create to disclose
  anything about. That arm is reachable from tests and offline paths only, and
  §5.7's `every_bail_out_in_prepare_rehydration_context_names_a_reason` is where
  it gets covered.
  - If a live `relay_unavailable`-class row is wanted anyway, the reachable
    cousin is a **REST projection failure** → `relay_query_failed`: let the
    create through, then make the projector's fetch fail (e.g. revoke the
    provider's read authority on the umbrella channel between create and
    projection).
- **L1c** If `SESSION_STATE.md §3 item 1`'s duplicate-create still reproduces
  on this build, the row must read the **conflict** clause, not the generic
  one. If it no longer reproduces, that is the §3 item 1 result and item 1's
  cause half closes on this run — record which happened.
- **L1d** Reattach an execution whose package cannot be rebuilt → the row must
  read "Restarted without prior context — <clause>", proving §5.4's resume-path
  edit (`lib.rs:1136-1161`) landed and that the reason is not create-only.
- Capture L1a/L1b with `just desktop-screenshot --name continuity-first-run`
  and `--name continuity-no-sidecar`; **verify distinct hashes before posting**
  (`shasum -a 256 test-results/screenshots/*.png`), per `CLAUDE.md`.

**L2 — item 4: staleness, then refresh. Two identities, two providers.**
Reproduce the exact 2026-08-18 scenario (`SESSION_STATE.md:64-66`: Codex
reported 16/16 complete while Claude had advanced to 6 turns). One umbrella
session, executions A (Claude) and B (Codex), driven by two operator
identities so the transcript proves who ran what.

1. Open B, then run three turns on A.
2. Ask B: *"call session_overview and tell me exactly how current your picture
   is."* It must quote `ageSinceCompleteAsOfMs`, report `stale`, and say it may
   be behind — **not** "16 of 16, complete."
3. Send B a turn (fires the refresh), wait past
   `CONTEXT_REFRESH_MIN_INTERVAL_MS`, send another, then ask again.
   `packageGeneration` and `completeAsOf` must both have advanced and A's three
   turns must appear in B's `session_history`.
4. Confirm on disk:
   `ls <app-data>/session-provider/context-packages/<package-id>/` shows ≥2
   generations and ≤3 retained. **The directory is named by the package id, not
   the execution id** — on a create they happen to be the same value
   (`lib.rs:766`), but B was opened by resume, so its directory is named by the
   minted `package_id` (`:1080`). Read the name from the provider log rather
   than assuming the session id (§9.2).
5. Stop the session and confirm the directory is gone (§9.1's cleanup, the
   pre-existing leak from §2 fact 3).
6. Restart the provider with a leftover directory present and confirm
   `recover()` swept it (§9.1's startup sweep).

**L3 — item 5: a bounded, error-free walk.** Find a session with >200 items
(`buzz --format compact sessions transcript … | wc -l`). Ask the agent to
retrieve the whole history and report how many `session_history` calls it made.
Expect: **no** `invalid_params` error at any point; every short page names its
`stoppedBy` and hands back a usable `nextCursor`; and the call count matches
§7.3's arithmetic (roughly one call per ~380 items in `view:"index"`). **Do not
score this as "one call"** — §7.3 shows that claim is false above ~380 items,
and an implementer who tunes the byte budget upward to hit it has traded a hard
error for a blown context window (§15.3).

**L4 — item 6: the delta explains itself.** Ask the agent: *"your provenance
says sourceEventCount N and totalHistoryItems M — account for the difference
exactly."* It must reconcile from `sourceEventBreakdown` with no "unexplained"
flag. Hand-check the authority term against
`buzz --format compact sessions roster --channel <uuid> --genesis <hex>`.

**L5 — the fence still holds (regression gate on Slice E).** Inside a
rehydrated session, ask the agent to run
`buzz sessions list --channel <uuid>`. It must fail for want of credentials.
This proves the refresh smuggled no relay access into the agent or the sidecar
(`agent_fence.rs:39-44`).

**L6 — cursor survives a refresh.** In B, take a `nextCursor` from a
`session_history` page, drive A far enough to trigger a refresh, then resume
paging from that cursor. Items must continue with no repeats and no gaps, and
the response must report `generationChangedSincePreviousCall: true`.

Report the build id, both machines/providers and both identities used for L2,
and the community, per `CLAUDE.md`'s working agreements.

---

## 12. Quality gates

During development, run the smallest focused tests. Before handing a slice to
Brian:

```bash
. ./bin/activate-hermit
cargo metadata --locked
cargo test -p buzz-core
cargo test -p buzz-dev-mcp
cargo test -p buzz-session-provider
just desktop-check
just desktop-typecheck
just desktop-test
just file-size-check
```

Notes that decide whether this change is actually covered:

- `just desktop-check` runs biome + px-text + pubkey-truncation only; it does
  **not** typecheck. Slice A edits a TypeScript map and a renderer, so
  `just desktop-typecheck` is the gate that catches those.
- This plan touches neither `buzz-relay` nor `buzz-db` nor `buzz-auth`, so
  `just test` is not required by `CLAUDE.md`'s rule. Run it anyway before the
  final integration: the projector's fetch path is the one thing here that
  talks to a relay.
- **⚠️ Every whole-repo gate on this branch is confounded, and a red result is
  not yours until you prove it.** The sentence above is true of *this plan*; it
  is **false of the branch you will run the gate on**. The in-flight Project
  Pulse build (§4) has uncommitted changes in `crates/buzz-relay/src/handlers/`,
  `crates/buzz-relay/src/api/bridge.rs`, `crates/buzz-db/src/`,
  `crates/buzz-sdk/`, `crates/buzz-cli/`, `crates/buzz-acp/`,
  `crates/buzz-test-client/tests/e2e_pulse.rs`, `desktop/playwright.config.ts`
  and `desktop/src/`. So `just ci`, `just test`, `just desktop-test` and
  `just file-size-check` all execute Pulse's code alongside yours.
  **Before reporting any whole-repo failure, reproduce it against the four
  package-scoped commands above** (`cargo test -p buzz-core`, `-p buzz-dev-mcp`,
  `-p buzz-session-provider`, plus `just desktop-typecheck`), which touch no
  Pulse file. A failure that appears only in the whole-repo gate belongs to the
  other build, and reporting it as this slice's regression will cost a review
  cycle. The converse trap is worse: **a green `just ci` on this branch does not
  prove this plan is green**, because Pulse's own failures could be masked or
  vice versa — cite the scoped runs as the evidence for a slice, and the
  whole-repo run only as a pre-integration smoke.

Before a PR or integration ceremony: `just ci`, then the §11 workflow.

---

## 13. Completion report required from the implementing agent

At the end of each slice, report:

1. Outcome first: what an operator or an agent can now do that it could not.
2. Changed files with `file:line` evidence.
3. Wire/schema decisions actually implemented, including the package version
   bump and the exact slug set published. Call out **decisions 14 and 15**
   explicitly — no-rename generation writes, and the shared page budget with the
   index view's real per-call item count. Both were chosen over a plausible
   alternative, and a silent deviation to the alternative reintroduces a defect
   this plan already priced.
4. **The two guarantee strings** (`session_context.rs:1-5` and
   `buzz-dev-mcp/src/lib.rs:197`) as they now read, if Slice E shipped.
5. Commands run and their results.
6. The live workflow exercised, with the build id, both identities and both
   providers for L2, and the observable result.
7. Every ledger edit made to `docs/SESSION_STATE.md`, quoting the before and
   after.
8. Any deviation from this plan, naming the section deviated from and the
   concrete source fact that required it.
9. Which ledger items are **fixed in code** versus **fixed and live-verified** —
   per `SESSION_STATE.md §3 item 3`'s standard, do not strike an item on a
   commit message or a passing test alone.

Do not claim a slice complete from code inspection alone. Do not start the
next slice until Brian confirms the current one in the prototype.

---

## 14. Ledger edits owed (`docs/SESSION_STATE.md`)

Land these with the code, not after (§1).

- **§2 item 1** (`:34-44`) — keep the item; its cause is `§3 item 1`'s
  re-test. Strike "The disclosure remains useless… Surface the bail-out
  reason" once L1 passes, citing the slug set and a transcript row.
- **§2 item 4** (`:62-66`) — rewrite. Record that the time-bounding half
  shipped in `b9de9a6d` (`coding_session_context.rs:92-98`,
  `context_projector.rs:884`, `session_context.rs:271-277`, `session.rs:60`)
  and the ledger never recorded it. Record what this bite added: read-time age
  plus a bounded refresh. **Downgrade, do not close** — a refreshed package is
  still a snapshot.
- **§2 item 5** (`:67-69`) — strike as fixed in code by `b9de9a6d`, citing
  `session_context.rs:24-25` and the test at `:580`. Record the two re-test
  traps from §1.2.
- **§2 item 6** (`:70-74`) — strike the "never says so" half as fixed by
  `b9de9a6d`'s `provenanceSemantics`; replace with the breakdown work. Fix the
  citation: the formula is at `context_projector.rs:970-980`, not `:945-955`
  (now `validate_limits`).
- **§2 header / §3 item 2** — record the process finding: `b9de9a6d` closed
  parts of items 4, 5 and 6 as well as item 3, and the 2026-08-18/19
  re-verification pass re-diffed that commit against item 3 only, even though
  it touches the two files items 4–6 cite. That is the finding worth keeping.
- **§3a** — add the environment fact: `BUZZ_CSP_CONTEXT_MCP_COMMAND`
  (`config.rs:127`) lets a stale `buzz-dev-mcp` binary be paired with a fresh
  provider; because the package structs are `deny_unknown_fields`
  (`coding_session_context.rs:88`), the old sidecar fails to start rather than
  degrading, and the failure surfaces to the agent as a missing MCP server.
- **§3a, first bullet** (`SESSION_STATE.md:190-194`) — **fix a wrong
  cross-reference while you are in the file.** The branch-switch bullet ends
  "Sessions created under one branch cannot be resumed or stopped from another
  (§2 item 5)". It is describing an execution whose provider identity is gone,
  which is **§2 item 7** (`:75-80`); §2 item 5 is the `session_history` page cap
  this plan strikes. Left alone, this bite's item-5 edit turns a stale
  cross-reference into an actively misleading one — a reader following the
  pointer will land on a struck item. Change it to "(§2 item 7)".
- **§2 item 1** (`:34-44`) — note when striking the disclosure half that the
  fix covers **both** disclosure surfaces, not just `session_fresh`: the resume
  path publishes `session_restarted_without_context` from its own inline match
  (`lib.rs:1136-1161`) and had the identical no-reason defect (§1.4).

---

## 15. Known tensions, stated rather than papered over

1. **Slice E narrows a deliberately-worded, agent-visible guarantee** from
   "one file" to "one directory". The sidecar keeps no relay client, no
   signing key and no write path, and re-runs the full validation gauntlet on
   every reload — but this is a real relaxation, and §9.5's two string edits
   are not optional.
2. **New recurring relay load.** One bounded fetch plus proof-graph
   verification per minute per *active rehydrated* execution, on a spawned
   task off the provider loop. Bounded, but it scales with concurrently active
   executions.
3. **`MAX_HISTORY_LIMIT` = 4 096 means an agent can ask for a page that would
   have been an 8 MiB response.** The 128 KiB byte budget is what makes that
   safe, so §7.2 and §7.3 are inseparable: shipping the raised cap without the
   budget trades a hard error for a blown context window, which is worse. The
   corollary is uncomfortable and is stated in §7.3 rather than hidden: because
   the budget always binds first at scale, the raised `limit` is mostly
   *permission to ask*, and a full 4 096-item walk is ~11 index calls, not one.
   Item 5's complaint about call count is **reduced and made self-describing,
   not eliminated** — the honest claim is that no walk ever errors, every short
   page names its bound, and the observed 101-item case is one call.
4. **`STALE_AFTER_MS` is arbitrary.** It is a disclosure trigger, not a
   correctness boundary; the raw ages sit beside it so a reader can disagree.
5. **The bail-out reason is a class, not a specific.** The 2026-08-18
   incident's actual detail — which command id had two receipts — stays in the
   provider log. That is the deliberate price of decision 10.
6. **Retained generations multiply private on-disk state by 3.** Bounded by
   the 8 MiB package ceiling and cleared on stop, but a provider that crashes
   mid-session leaves up to three packages per orphaned execution — and
   `SESSION_STATE.md §2 item 7` says orphaned executions already exist and are
   never reaped. Net, this is still strictly better than today, where *every*
   execution leaks one package forever (§2 fact 3).
7. **Item 4 does not close.** After this change the package is a snapshot
   refreshed at turn boundaries with a 60-second interval floor, so a sibling's
   work in the last minute is still invisible. This plan makes that gap visible
   and bounded; it does not eliminate it, and the ledger entry must say so.
8. **A crash can leave one corrupt generation on disk, and the reader will
   refuse it on every tool call until the next successful refresh supersedes
   it.** This is the deliberate price of decision 14 — the alternative was a
   no-clobber-rename scheme that repeals the write-once guarantee or needs
   macOS-specific syscalls. During that window every response honestly carries
   `refreshRefused: true` and a climbing age, which is the H3/H4 behaviour, but
   it is a window, and a session whose provider never refreshes again would sit
   in it until stop.
9. **The generation directory is keyed by a package id that lives only in
   memory.** §9.3 argues this is correct — a restart destroys every reader — but
   the consequence is real: between a provider crash and the next `recover()`,
   the on-disk directories are unreferenced, and anything that inspects that
   directory out of band (an operator, a support script) cannot map a directory
   back to a session without the provider log.
