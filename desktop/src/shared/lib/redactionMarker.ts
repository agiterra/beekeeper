/**
 * Reader for the coding-session privacy redaction marker.
 *
 * The provider redacts host paths, credentials, and provider-native cursor
 * fields out of a transcript item *before* the item is signed, replacing each
 * unsafe value with a content-addressed marker:
 *
 * ```text
 * [elided private context: 148 bytes, sha256:eb7930a9…]
 * ```
 *
 * Produced by `context_elision_marker` in
 * `crates/buzz-core/src/coding_session_context.rs`. The marker is load-bearing
 * — it is how a reader tells "the provider had this and chose not to publish
 * it" apart from "nothing was there" — and it is already signed into events in
 * the field, so it is never rewritten on the wire. It is read here instead, so
 * a client can render 90 characters of hash as a pill.
 *
 * Two things this module deliberately does not do:
 *
 * - It does not match the *size-capping* markers (`…[elided N bytes]…` and the
 *   `{"kind":"elided"}` item). Those have a different cause — the 32 KiB event
 *   cap, not privacy — and conflating them would tell a reader something
 *   untrue about why content is missing.
 * - It does not accept a marker it cannot prove is one. A prefix with no
 *   closing bracket, a short digest, a non-numeric byte count: all stay text.
 *   Swallowing content that merely looks like a marker would be a silent
 *   deletion, which is the one failure mode worse than an ugly hash.
 */

/**
 * The exact marker shape, anchored to what the Rust writer emits: a decimal
 * byte count and a 64-character lowercase hex digest.
 *
 * `g`-flagged for `createRemarkPrefixPlugin`, which resets `lastIndex` itself.
 * Callers that `exec` this directly must do the same.
 */
export const REDACTION_MARKER_PATTERN =
  /\[elided private context: (\d+) bytes, sha256:([0-9a-f]{64})\]/g;

/** One redaction, as read off the wire. */
export type RedactionMarker = {
  /**
   * Serialized JSON byte count of the value that was redacted — so a string's
   * count covers its quotes and escaping, and reads a few bytes larger than
   * the text a human would have seen. Reported as-is; the tooltip says so.
   */
  bytes: number;
  /** SHA-256 of the value's JSON encoding, lowercase hex, no `sha256:` prefix. */
  digest: string;
  /** The marker exactly as it appeared, for copy-paste and for fallbacks. */
  raw: string;
};

/** A run of ordinary text, or one redaction, in source order. */
export type RedactionSegment =
  | { kind: "text"; text: string }
  | ({ kind: "redaction" } & RedactionMarker);

/**
 * Split `text` into alternating prose and redaction segments.
 *
 * Text with no marker returns a single `text` segment, so callers can cheaply
 * detect the common case with `segments.length === 1`.
 */
export function parseRedactionMarkers(text: string): RedactionSegment[] {
  REDACTION_MARKER_PATTERN.lastIndex = 0;
  const segments: RedactionSegment[] = [];
  let cursor = 0;

  for (
    let match = REDACTION_MARKER_PATTERN.exec(text);
    match !== null;
    match = REDACTION_MARKER_PATTERN.exec(text)
  ) {
    if (match.index > cursor) {
      segments.push({ kind: "text", text: text.slice(cursor, match.index) });
    }
    segments.push({
      kind: "redaction",
      bytes: Number(match[1]),
      digest: match[2],
      raw: match[0],
    });
    cursor = match.index + match[0].length;
  }

  if (cursor === 0) {
    return [{ kind: "text", text }];
  }
  if (cursor < text.length) {
    segments.push({ kind: "text", text: text.slice(cursor) });
  }
  return segments;
}

/** Does this text carry at least one redaction marker? */
export function hasRedactionMarker(text: string): boolean {
  REDACTION_MARKER_PATTERN.lastIndex = 0;
  return REDACTION_MARKER_PATTERN.test(text);
}

/**
 * Byte counts rendered for humans: `148 B`, `2.1 KB`, `3 MB`.
 *
 * Decimal units, because the number being described is a JSON serialization
 * length rather than an allocation, and a reader comparing it to a file size
 * in Finder should not be off by 2.4%.
 */
export function formatRedactedBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown size";
  if (bytes < 1_000) return `${bytes} B`;
  if (bytes < 1_000_000) {
    const kilobytes = bytes / 1_000;
    return `${kilobytes < 10 ? kilobytes.toFixed(1) : Math.round(kilobytes)} KB`;
  }
  const megabytes = bytes / 1_000_000;
  return `${megabytes < 10 ? megabytes.toFixed(1) : Math.round(megabytes)} MB`;
}

/**
 * Why content is missing. The two causes are unrelated and the reader is owed
 * the difference:
 *
 * - `redaction` — the provider redacted a host-private or credential-bearing
 *   value before signing (this module's marker).
 * - `cap` — the item exceeded the 32 KiB event cap and was dropped or
 *   truncated to fit.
 *
 * Sharing one pill vocabulary is what makes them comparable; sharing one
 * *label* would make them indistinguishable.
 */
export type ElisionCause = "redaction" | "cap";

/**
 * The pill's label: `redacted 148 B`, or `dropped 41 KB` for the cap.
 *
 * "redacted"/"dropped" rather than "elided" — the wire word is precise about
 * the mechanism and opaque about the meaning, and the reader needs the
 * meaning.
 */
export function formatElisionLabel(
  cause: ElisionCause,
  bytes: number | null,
): string {
  const verb = cause === "redaction" ? "redacted" : "dropped";
  return bytes === null ? verb : `${verb} ${formatRedactedBytes(bytes)}`;
}

/** `formatElisionLabel` for a parsed redaction marker. */
export function formatRedactionLabel(marker: RedactionMarker): string {
  return formatElisionLabel("redaction", marker.bytes);
}
