# H-08 — Transcript export viewer (conformance corpus)

**Ledger row:** H-08 transcript export viewer
(`docs/absorption/ABSORPTION-LEDGER.md`).
**Donor:** Hive `src/server/standalone-export.ts` + `.test.ts` (bundle
writer), `scripts/prepare-export-viewer-release-assets.ts` (release-manifest
prep, no donor test), and `src/export-viewer/main.tsx` (static viewer),
introduced around donor commit `7bd7ef4`, read at pin `e0b8198bd144` from
`to_import/hive`. No donor code was executed to produce any value here; <!-- absorption-verify-exempt: non-production reference -->
every expected value below is re-derived by `fixtures.test.mjs`'s own
independent implementations.
**Beekeeper status:** rendering is partial (`desktop/src/features/coding-sessions`
transcript model/view); Beekeeper had **no standalone export implementation** when
this corpus was banked, so per OPERATING-PLAN §5 this row banked the donor
contract as a corpus with the future Beekeeper implementation as its named
consumer. **Consumer landed 2026-08-06:** the implementation now exists —
pure law engine `desktop/src/features/coding-sessions/lib/transcriptExport/`,
Rust fs executor `desktop/src-tauri/src/transcript_export/` (refusal +
never-overwrite laws), embedded Beekeeper-owned static viewer, and the
release-manifest script `scripts/export-viewer-release-manifest.mjs` — and
`implementation.test.mjs` in this directory binds it to every fixture
vector, triangulating with `fixtures.test.mjs`'s independent
re-implementations.

## The bundle law (`transcript.json`, version 1)

1. **Shape:** `{ version: 1, chatId, title, localPath, exportedAt,
   viewerVersion, theme, attachmentMode, messages }`. `exportedAt` is the
   injected clock's ISO-8601 string; `viewerVersion` is the exporting app's
   version.
2. **No local-path leak.** `localPath` in the bundle is ALWAYS the share
   placeholder `/workspace`, never the real workspace path — and the real
   workspace path must not appear ANYWHERE in the serialized bundle: every
   string in every message is rewritten by deep traversal
   (strings, arrays, object values), replacing every occurrence of the
   workspace path with `/workspace`. The donor's own test asserts the
   serialized bundle does not contain the project directory; the corpus
   carries the same law.
3. **Attachment modes** (only `user_prompt` messages with attachments
   participate; everything else passes through untouched):
   - `metadata` — the attachment's `absolutePath`, `relativePath`, and
     `contentUrl` are ALL emptied. Metadata (`id`, `displayName`,
     `mimeType`, `size`) survives.
   - `bundle` — the attachment file is copied to
     `attachments/<sanitized-id>-<sanitized-basename>` inside the export,
     and all three fields are rewritten to that same `./attachments/…`
     relative path.
   - `bundle` with a missing or path-less source file **falls back to the
     metadata rewrite** for that attachment — never a broken reference.
   - Counters: `totalAttachmentCount` counts every attachment on
     `user_prompt` messages; `bundledAttachmentCount` counts only actual
     copies.
4. **Viewer bundle required:** export refuses to run when the viewer dist
   directory is absent (the export is a copy of the viewer plus
   `transcript.json` beside it — the viewer fetches `./transcript.json`
   relative to `document.baseURI`).

## The naming law

- `sanitizeFileNameSegment`: trim, collapse every run of characters outside
  `[A-Za-z0-9_.-]` to a single `-`, then strip leading/trailing `-`.
- Export directory: `<sanitized title or "chat">-<timestamp>` where the
  title falls back to `chatId` before sanitizing, and the timestamp is the
  ISO instant with `:` → `-` and the `.mmm` milliseconds stripped
  (`2026-04-23T12:34:56.000Z` → `2026-04-23T12-34-56Z`).
- Collisions append `-2`, `-3`, … — the first free candidate wins; an
  existing export is never overwritten.

## The release-manifest law

`prepare-export-viewer-release-assets.ts` flattens the viewer dist into
release assets plus `export-viewer-manifest.json`:

- Asset names: `export-viewer__` + the relative path with every `/` replaced
  by `__` (`assets/viewer.js` → `export-viewer__assets__viewer.js`).
- Cache control: paths ending `.html` get `public, max-age=300`; everything
  else gets `public, max-age=31536000, immutable`.
- Content types come from a closed extension table (case-insensitive
  extension match); unknown extensions fall back to
  `application/octet-stream`.
- `releaseTag` is `v` + the package version with any existing leading `v`
  stripped first (never `vv…`).
- Manifest shape: `{ viewerVersion, releaseTag, generatedAt, files:
  { <relativePath>: { assetName, cacheControl, contentType } } }`.

## Authority boundary

The export is a **static redacted bundle**: local-only output, no provider
credential, no upload anywhere (the donor's fork states this in the source).
Nothing in this corpus grants or models release authority — publishing the
prepared assets is H-05c's plane.

## Donor subtlety, recorded not inherited

`rewriteLocalPathsForShare` mutates arrays/objects in place but its return
value is discarded at the call site, so a hypothetical top-level *string*
argument would silently not be rewritten. The actual call passes the
messages array, so the donor behavior is correct for its only call shape; a
Beekeeper implementation should not copy the discard-the-return pattern.
