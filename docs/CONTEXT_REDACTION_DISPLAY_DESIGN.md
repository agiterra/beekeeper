# Context redaction: readable markers and a local plaintext dictionary

**Status:** design agreed 2026-08-26 (Andy). Both parts landed 2026-08-26.
**Owner surface:** coding-session transcripts (kinds 44222/44223) and any
channel timeline that renders them.

Coding-session transcript items are redacted before they are signed, and the
redacted value is replaced by

```
[elided private context: 148 bytes, sha256:eb7930a9a9209e69d829efa946d4ebea3f2e32c6b03f3317321c53bf33597e3f]
```

That marker is correct and load-bearing — it is the difference between "the
provider had this and chose not to publish it" and "nothing was there" — but it
is 90 characters of hash dropped into the middle of a sentence, and the thing it
most often hides is a path on the operator's own machine.

This document specifies two changes:

1. **Render the marker as a pill** — `redacted 148 B`, with the digest behind a
   hover/click. Client-only; no wire change.
2. **A local plaintext dictionary** — on the machine that produced the
   redaction, show the operator the real value with a
   `redacted for other viewers` pill beside it. Never for secrets; never for a
   remote session; expired on a schedule.

---

## 1. Background: how a marker is produced

Two unrelated mechanisms both print the word "elided". Keep them apart.

| Marker | Produced by | Cause |
|---|---|---|
| `[elided private context: N bytes, sha256:…]` | `crates/buzz-core/src/coding_session_context.rs:1208` | **privacy redaction** |
| `…[elided N bytes]…` and `{"kind":"elided"}` | `crates/buzz-session-provider/src/transcript.rs:543`, `:342`; `crates/buzz-acp/src/lib.rs:1016` | **size capping** (32 KiB event cap) |

This document is about the first. The second is addressed only where the two
share a rendering surface (§3.4).

### 1.1 The five detection rules

`sanitize_coding_session_context_content`
(`crates/buzz-core/src/coding_session_context.rs:837`) walks the JSON tree of a
transcript item and applies:

| # | Rule | Predicate | Scope of the redaction |
|---|---|---|---|
| 1 | Sensitive JSON key | `sensitive_context_key` (`:1178`) | the **whole value** under that key |
| 2 | PEM key block | `redact_key_blocks` (`:912`) | `-----BEGIN … -----END`, whole; unterminated → to end of string |
| 3 | Shaped secret | `contains_shaped_secret` (`:958`) | the word |
| 4 | Host path | `contains_host_path` (`:1140`) | the word |
| 5 | Credential assignment | `redact_credential_assignments` (`:1004`) | the value side, line-scoped |

Rule 1 is key-driven and blind to content — `privatekey`, `token`,
`authorization`, `cookie`, `resumecursor`, `acpsessionid`, … Rules 3–5 were
narrowed on 2026-08-24 (`docs/SESSION_STATE.md:1188`) after the old
topic-word rule replaced entire messages: a session working on git ACLs could
not describe its own work. Rule 4 exempts stock POSIX interpreters via
`SYSTEM_COMMAND_PATHS` (`:1125`), because `codex-acp` puts whole argv in
`toolName`.

### 1.2 Where it runs

- `crates/buzz-session-provider/src/transcript.rs:306` — `fit_item`, the single
  seam every CST envelope passes through before signing. Redaction runs
  *first*, then size-fitting, because eliding grows a value.
- `crates/buzz-session-provider/src/context_projector.rs:1571` — the private
  rehydration package.
- `crates/buzz-core/src/coding_session_context.rs:820` (and `:399`, `:689`,
  `:765`, `:1260`) — used as a **validator**, sanitize-and-compare. These call
  sites are why the redactor must stay a pure function with no I/O.

### 1.3 Properties of the marker

- The digest is `sha256(serde_json::to_vec(value))` — of the **JSON encoding**,
  so a string's digest covers its quotes and escaping.
- `N bytes` is that same serialized length. It is a few bytes larger than the
  visible text. The pill reports it as-is; the tooltip says "serialized".
- The digest is **unsalted over a frequently short plaintext**. Anyone holding
  the event can confirm a guess: `sha256("\"/Users/andy\"")` is cheap. This is a
  pre-existing property. Hiding the digest behind a hover does not change it,
  and this design does not claim to fix it. Salting per session would, at the
  cost of a wire change; recorded here as a known limitation, not a task.

---

## 2. Part 1 — the pill

No wire change. The marker format is already signed into events in the field
and is load-bearing for `is_context_elision_marker` (`:1217`), which is how the
redactor avoids double-wrapping a value it already redacted. Parse it at render
time instead.

### 2.1 Parser

`desktop/src/shared/lib/redactionMarker.ts`

```ts
type RedactionMarkerSegment =
  | { kind: "text"; text: string }
  | { kind: "redaction"; bytes: number; digest: string; raw: string };

parseRedactionMarkers(text: string): RedactionMarkerSegment[]
```

Pure, exhaustively unit-tested, and deliberately mirroring the Rust predicate:
a marker is `[elided private context: <digits> bytes, sha256:<64 lowercase
hex>]`. Text that merely starts with the prefix and does not close is left as
text — the renderer must never eat content it cannot prove is a marker.

### 2.2 Pill component

`desktop/src/shared/ui/RedactedPill.tsx`

- Reads `redacted 148 B`, sized on the `text-2xs` token. **No px or arbitrary
  rem literals** — `desktop/scripts/check-px-text.mjs` fails the build on those,
  and the transcript is a zoom-sensitive surface.
- Hover/focus → tooltip carrying the full `sha256:…` and a copy affordance.
  Click does the same, for touch and keyboard.
- Follows the existing inline-chip precedent in
  `desktop/src/shared/ui/markdown/` (`MessageLinkPill`, `SpoilerInline`).

### 2.3 Prose (markdown) wiring

A remark plugin, `desktop/src/shared/lib/remarkRedactionMarkers.ts`, splitting
text nodes into a custom `redaction` mdast node — the same shape as
`remarkSpoilers`/`remarkMentions`, registered alongside them in
`desktop/src/shared/ui/markdown/nodeCache.ts`. Skips `code` and `inlineCode`
nodes, like `remarkSpoilers` does: inside a fence the literal marker is the
honest rendering.

`markdown.tsx` maps `redaction` → `RedactedPill` in `createMarkdownComponents`.

> **Ratchet note.** `desktop/src/shared/ui/markdown.tsx` is 1,945 lines against
> a 1,000-line ceiling, so the differential file-size gate forbids it *growing
> by one line*. Room is made by extracting the self-contained `ImageMosaic`
> component to `markdown/ImageMosaic.tsx` (−23 lines). Per
> `CLAUDE.md`, the answer to the guard is to split the file, never to raise the
> limit.

### 2.4 The other surfaces

Non-markdown text goes through `RedactedText`, which parses the string and
returns it unchanged when it holds no marker — so the overwhelmingly common
case adds nothing to the tree. Wired:

- **Tool args and results** — the `<pre>` blocks in
  `CodingSessionTranscriptParts.tsx`. Markers appear inside serialized JSON
  here, not markdown.
- **Active-tool labels**, **diagnostic rows**, and the **session-error card**
  (`CodingSessionTranscriptParts.tsx`, `CodingSessionTranscript.tsx`).
- **Activity row labels** — `ActivityRowLabel`'s `object`, which is where the
  `Ran [elided …] -lc "sed -n …"` case lands. Strings get the pill; a caller
  that already passed an element is left alone.
- **Lifecycle rows** — the error and permission branches of
  `LifecycleActivity.tsx`; permission prose quotes commands, so it quotes paths.

- **Failed and collapsed tool rows** — `CompactToolSummaryRow.tsx`. Its own
  renderer, not `ActivityRowLabel`; it was still printing the raw marker after
  the first pass, which the e2e spec caught.

### 2.5 The cap, in the same vocabulary

The 32 KiB cap is a *different cause* and the reader is owed the difference, so
it shares the pill and never the label:

| | Redaction | Cap |
|---|---|---|
| Verb | `redacted 148 B` | `dropped 41 KB` |
| Icon | eye-off | scissors |
| Tooltip | "removed before the transcript was signed" | "did not fit the event cap" |
| Row title | — | `Content dropped` |

`ElisionPill` takes the cause; `RedactedPill` is the redaction-cause wrapper
the markdown path uses. `buildElidedStatusItem` now carries
`elision: {bytes, digest, reason}` **structurally** on the lifecycle item
(`TranscriptItemElision`), beside the existing `durationMs`/`costUsd`
precedent, rather than formatting a `reason:` / `byteCount:` /
`contentDigest:` block into `text`. `text` keeps a one-line prose fallback for
consumers that read the item as prose.

The row title moved from `Content elided` to `Content dropped` — the wire word
is precise about the mechanism and opaque about the meaning. It stays absent
from `DIAGNOSTIC_LIFECYCLE_TITLES`, so a dropped item still shows where it was
dropped rather than in the telemetry rail.

### 2.6 `bee`

A terminal has no hover, so the digest moves to a footnote instead of a
tooltip. `render_markdown` post-processes its own output
(`condense_redaction_markers`): each marker becomes `[redacted 148 B · #1]` and
the document gains a `## Redactions` section listing each distinct digest once.
Identical values share a number, so a repeated redaction is visibly the same
value. Fenced blocks are skipped, matching the desktop. The cap line reads
`_[dropped 41 KB — did not fit the event cap]_ · sha256:…`.

---

## 3. Part 2 — the local dictionary

The provider redacts **before signing**, so the plaintext exists only in the
provider process. Recovering it later requires the host to have recorded it.
The desktop app is the provider's parent (`desktop/src-tauri/src/session_provider/mod.rs`)
and already owns its state directory, so the path is short.

### 3.1 What is recorded, and what is never recorded

This is the load-bearing decision. Writing redacted secrets to disk in
plaintext creates a liability strictly worse than the readability problem being
solved. So the vault records **private, non-secret** values only:

| Class | Source rule | Recorded? |
|---|---|---|
| `host-path` | rule 4 | **yes** — showing an operator their own home directory discloses nothing they do not already know |
| `structural` | rule 1, for `resumecursor` / `acpsessionid` | **yes** — opaque provider bookkeeping, useful when debugging a stall |
| `secret-key` | rule 1, for `privatekey` / `password` / `cookie` / `token` / `authorization` / `apikey` / … | **never** |
| `key-block` | rule 2 | **never** |
| `shaped-secret` | rule 3 | **never** |
| `credential-assignment` | rule 5 | **never** |

The classification happens where the redaction is decided, not by
re-inspecting the plaintext afterwards — a value is unrecoverable because of
*the rule that caught it*, which is a property the redactor knows for free and
a later reader would have to guess.

### 3.2 Capture without putting I/O in `buzz-core`

`buzz-core`'s redactor is called as a pure validator in six places
(§1.2); it cannot grow a side effect. Add a recording variant instead:

```rust
pub struct Redaction {
    pub digest: String,
    pub bytes: usize,
    pub class: RedactionClass,
    pub plaintext: String,
}

pub fn sanitize_coding_session_context_content_recording(
    value: &Value,
) -> (Value, Vec<Redaction>)
```

and reduce the existing `sanitize_coding_session_context_content` to a wrapper
that discards the vector. One implementation, so the published transcript and
the vault cannot drift on what was redacted.

`fit_item` (`transcript.rs:306`) — the publish seam — is the only caller that
records.

### 3.3 The vault

`crates/buzz-session-provider/src/redaction_vault.rs`

```
<BUZZ_CSP_STATE_DIR>/redactions/<session-id>.jsonl
```

Append-only JSONL, one entry per line:

```json
{"digest":"eb79…","bytes":148,"class":"host-path","plaintext":"/Users/…","recordedAt":"2026-08-26T…Z"}
```

`context_store.rs` is the precedent to copy verbatim: 0700 directories, 0600
files, symlink-refusing at both levels, identity-scoped so a rotated provider
key gets a structurally fresh directory rather than one full of another
identity's paths. **Never published, never placed in the agent environment.**

### 3.4 Expiry

The vault is a debugging convenience, not an archive, and every entry is a fact
about the host. Three independent reapers, because each catches what the others
miss:

1. **Session-scoped.** Stopping an execution deletes its `<session-id>.jsonl`,
   the same way stopping an execution already removes its context-package
   directory (`context_store.rs:511`). This is the common case.
2. **Age-scoped.** A `RedactionVaultRetention` sweep drops files whose mtime is
   older than **14 days**, run at provider startup and once daily thereafter.
   Fourteen days is chosen to outlive a weekend plus a week of not looking at a
   transcript, and to be short enough that an abandoned machine is not
   accumulating a map of its own filesystem indefinitely. Configurable via
   `BUZZ_CSP_REDACTION_RETENTION_DAYS`; `0` disables recording entirely.
3. **Size-scoped.** A per-session file is capped (1 MiB) and the vault root is
   capped (64 MiB); the oldest files are dropped first. A pathological session
   that redacts thousands of paths must not be able to fill a disk.

A startup sweep also removes any vault directory not belonging to a live
session, mirroring the existing context-package startup sweep
(`context_store.rs:573`).

Expiry is not a soft failure: a lookup that misses because the entry expired is
indistinguishable, to the UI, from a lookup on a remote machine. Both render
the plain pill from Part 1. The UI never says "expired" it cannot prove.

### 3.5 Lookup

`coding_session_resolve_redactions(digests: Vec<String>) -> Map<String, Entry>`
— a Tauri command in `session_provider/commands.rs`. Vault read only; it never
touches the network and never reads a file outside the resolved vault root.

**Locality gate.** Resolve only when the transcript event's signer matches a
locally-provisioned provider record. `newCodingSessionModel.ts:102` already
makes exactly this comparison for the composer; reuse it. A session running on
someone else's machine stays opaque, which is correct.

### 3.6 UI

`useRedactionDictionary()` — React Query, batched by digest, cached per
session, disabled outright when the locality gate says no.

- **Resolved** → render the plaintext inline, followed by a
  `redacted for other viewers` pill. The pill is not decoration: it is the
  operator's only signal that what they are reading is not what the channel
  shows, and it must survive copy-paste as text.
- **Unresolved** → the `redacted 148 B` pill from Part 1, unchanged.

There is no third state. The client cannot distinguish "unrecoverable class"
from "remote machine" from "expired", because the marker carries no class on
the wire, and inventing a label for a state we cannot prove is the kind of
comfortable guess this project treats as a bug. A class letter in the marker
would let the UI say `secret, not recoverable` honestly — that is a wire change
and is deliberately out of scope for v1.

---

## 3a. What Part 2 actually looks like

Built as designed, with these details settled during implementation.

**The recording redactor.** `sanitize_coding_session_context_content_recording`
in `buzz-core` is the recording twin, and `sanitize_coding_session_context_content`
is now a thin wrapper over the same walk — one implementation, so the published
transcript and the host's record cannot drift on what counts as private. The
non-recording path threads a `RedactionLog::off()`, which materializes **no**
plaintext copy: the validator call sites (`:399`, `:689`, `:765`, `:820`,
`:1260`) sanitize-and-compare on every item, and they must not clone a secret
just to drop it.

`RedactionClass` is assigned by the rule that caught the value, and
`is_recoverable()` is the single gate — enforced at production, not at
persistence, so no caller can opt out by forgetting to filter. `sensitive_context_key`
now returns `Option<RedactionClass>`, which is where `resumeCursor` and
`acpSessionId` split off from the credential keys.

**The vault.** `crates/buzz-session-provider/src/redaction_vault.rs`, following
`context_store.rs`: 0700 dirs, 0600 files, symlink-refusing, session ids proven
to be one safe path component before anything opens. `append` re-checks
`is_recoverable()` belt-and-braces, so a future caller hand-building a
`Redaction` still cannot write a credential.

Wired at `fit_item_recording` → `Provider::enqueue_transcript`, with a
per-session digest set (`Provider::recorded_redactions`) so a home directory
redacted out of every item writes one line, not one per item. **A vault error
never fails a publish** — the transcript is the product; a full disk costs a
warning and an unresolved pill.

**Expiry**, all three reapers as specified: `forget_redactions` on stop (not on
resume — there the session continues and the note is still wanted),
`sweep_redaction_vault` at startup for age and size, and `sweep_orphans` for
files no live session record owns. `BUZZ_CSP_REDACTION_RETENTION_DAYS=0` does
not merely stop writing: it sweeps away what is already there.

**Lookup.** `coding_session_resolve_redactions` in the Tauri surface, bounded at
512 digests, refusing unless `provider_pubkey` is an identity this desktop
provisioned for the active relay. A remote session returns an empty map rather
than an error — it is an ordinary state, not a failure.

**The client hook is deliberately not React Query.** A digest names one fixed
value read from a local file; there is no server state, nothing to refetch, and
nothing to retry. `useQuery` also needs a `QueryClientProvider`, and
`CodingSessionTranscript` is deliberately renderable from static markup so its
behaviour can be asserted without one — wiring React Query in broke 60 existing
tests. `useRedactionDictionary` is an effect plus a module-level cache,
registered in `resetCommunityState()` because a provider identity is minted per
relay.

One bug worth recording, because it is the kind that hides: the effect first
depended on the `scope` *object*. Every render rebuilt an equal scope, so the
cleanup fired and cancelled the in-flight lookup before it could land, while
`askedScopes` prevented a retry — the value never resolved, silently. The effect
now keys on `scopeKey`, a string, and reads the scope through a ref.

## 4. Sequencing

**Part 1 first, and it stands alone.** Client-only, ships to every viewer
including remote ones, and needs no on-disk-secrets story. Part 2 then reuses
the same pill component and adds the resolved state behind it.

## 4a. Environment note found while validating Part 1

`just desktop-screenshot` reuses whatever is already answering on port 4173.
With several worktrees checked out — the normal state of this repo — that is
usually a *sibling* worktree's `python3 -m http.server 4173 -d dist`, so the
screenshot is taken against someone else's build. The first capture of the pill
showed raw markers for exactly this reason and looked like a product bug.

`desktop/tests/helpers/screenshot.mjs` now honours
`BUZZ_SCREENSHOT_BASE_URL`. Serve your own `dist` on a free port and point the
helper at it:

```bash
python3 -m http.server 4273 -d dist &
BUZZ_SCREENSHOT_BASE_URL=http://127.0.0.1:4273 \
  node tests/helpers/screenshot.mjs --name whatever
```

`playwright.config.ts` honours `BUZZ_E2E_PORT` for the same reason — it drives
`use.baseURL`, the `webServer` command, and its readiness URL together:

```bash
BUZZ_E2E_PORT=4273 pnpm exec playwright test --project=smoke <spec>
```

## 4b. What the work is verified by

- `desktop/src/shared/lib/redactionMarker.test.mjs` — the parser, including
  every shape it must refuse.
- `desktop/src/shared/lib/remarkRedactionMarkers.test.mjs` — the mdast
  transform, fences included.
- `desktop/tests/e2e/coding-session-elision-screenshots.spec.ts` — the real
  transcript surface: both pills, the fence staying literal, the failed-tool
  row, and the digest reachable on hover. This is what caught
  `CompactToolSummaryRow` still printing a raw marker.
- `crates/buzz-cli/src/commands/sessions.rs` tests — the CLI condenser and its
  footnote, including one that goes through `render_markdown` end to end,
  because unit-testing the condenser alone still passes with it unhooked
  (verified by unhooking it and watching only that test fail).

Part 2:

- `crates/buzz-core/src/coding_session_context_tests.rs` — the recording twin
  redacts byte-for-byte identically to the pure one; **no secret class is ever
  recorded**; a path and a secret on one line are classified separately;
  re-redacting an already-redacted item records nothing. Verified by making
  `is_recoverable()` return `true` and watching three of them fail.
- `crates/buzz-session-provider/src/redaction_vault.rs` — 13 tests: resolve by
  digest, cross-session isolation, a hand-built secret still refused, a
  traversing session id failing closed, 0700/0600, a partial final line, the
  age reaper, the session cap, the orphan sweep, and retention disabled
  removing what already exists.
- `crates/buzz-session-provider/src/lib.rs` — end to end through
  `enqueue_transcript`: a host path is unreadable on the wire and readable from
  this machine's vault, a credential is neither. Verified by unhooking
  `record_redactions` and watching only the reachability test fail.
- `desktop/src/features/coding-sessions/lib/redactionDigests.test.mjs` — which
  digests are collected, and the refusals: two signers, two sessions, or no
  attribution means there is no single machine to ask.
- `desktop/tests/e2e/coding-session-elision-screenshots.spec.ts` — the revealed
  state with its badge, the credential still a pill beside it, another
  machine's transcript resolving nothing, and no vault at all resolving
  nothing without ever saying "expired".

## 5. Tests that must be watched fail

- Parser: a well-formed marker, a truncated one (`[elided private context: 12`),
  a marker inside a fenced code block, two markers in one paragraph, a marker
  adjacent to punctuation.
- Vault: a `shaped-secret` redaction produces **no** vault entry (the leak
  direction, pinned).
- Vault: an entry older than the retention window is gone after a sweep; one
  inside it survives.
- Locality: a transcript signed by a non-local provider resolves nothing even
  when a digest happens to collide with a local entry.
- Permissions: the vault directory is 0700 and every file 0600, mirroring
  `context_store.rs:587`.
