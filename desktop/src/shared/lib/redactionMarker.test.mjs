import assert from "node:assert/strict";
import test from "node:test";

import {
  formatRedactedBytes,
  formatRedactionLabel,
  hasRedactionMarker,
  parseRedactionMarkers,
} from "./redactionMarker.ts";

const DIGEST =
  "eb7930a9a9209e69d829efa946d4ebea3f2e32c6b03f3317321c53bf33597e3f";
const OTHER_DIGEST =
  "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const MARKER = `[elided private context: 148 bytes, sha256:${DIGEST}]`;

test("text with no marker returns exactly one text segment", () => {
  const segments = parseRedactionMarkers("ran the tests and they passed");
  assert.deepEqual(segments, [
    { kind: "text", text: "ran the tests and they passed" },
  ]);
});

test("a marker mid-sentence splits into text, redaction, text", () => {
  const segments = parseRedactionMarkers(`I read ${MARKER} and found it.`);
  assert.equal(segments.length, 3);
  assert.deepEqual(segments[0], { kind: "text", text: "I read " });
  assert.deepEqual(segments[1], {
    kind: "redaction",
    bytes: 148,
    digest: DIGEST,
    raw: MARKER,
  });
  assert.deepEqual(segments[2], { kind: "text", text: " and found it." });
});

test("a marker alone yields a single redaction segment and no empty text", () => {
  const segments = parseRedactionMarkers(MARKER);
  assert.equal(segments.length, 1);
  assert.equal(segments[0].kind, "redaction");
});

test("two markers in one paragraph are both read, in order", () => {
  const second = `[elided private context: 9 bytes, sha256:${OTHER_DIGEST}]`;
  const segments = parseRedactionMarkers(`${MARKER} then ${second}`);
  const redactions = segments.filter((s) => s.kind === "redaction");
  assert.equal(redactions.length, 2);
  assert.equal(redactions[0].digest, DIGEST);
  assert.equal(redactions[1].digest, OTHER_DIGEST);
  assert.equal(redactions[1].bytes, 9);
});

test("a marker adjacent to punctuation keeps the punctuation as text", () => {
  const segments = parseRedactionMarkers(`(${MARKER}).`);
  assert.deepEqual(segments[0], { kind: "text", text: "(" });
  assert.equal(segments[1].kind, "redaction");
  assert.deepEqual(segments[2], { kind: "text", text: ")." });
});

// The failure mode worse than an ugly hash is silently eating content that
// merely resembled a marker. Each of these must survive byte-for-byte.

test("an unterminated marker prefix stays text", () => {
  const truncated = "[elided private context: 12";
  assert.deepEqual(parseRedactionMarkers(truncated), [
    { kind: "text", text: truncated },
  ]);
});

test("a short digest is not a marker", () => {
  const short = "[elided private context: 148 bytes, sha256:eb7930a9]";
  assert.deepEqual(parseRedactionMarkers(short), [
    { kind: "text", text: short },
  ]);
});

test("an uppercase digest is not a marker", () => {
  const upper = `[elided private context: 148 bytes, sha256:${DIGEST.toUpperCase()}]`;
  assert.deepEqual(parseRedactionMarkers(upper), [
    { kind: "text", text: upper },
  ]);
});

test("a non-numeric byte count is not a marker", () => {
  const bad = `[elided private context: many bytes, sha256:${DIGEST}]`;
  assert.deepEqual(parseRedactionMarkers(bad), [{ kind: "text", text: bad }]);
});

test("the size-capping markers are left alone", () => {
  // Different cause (the 32 KiB event cap, not privacy). Conflating the two
  // would tell a reader something untrue about why content is missing.
  for (const other of [
    "…[elided 41235 bytes]…",
    "byteCount: 41235, contentDigest: eb7930a9",
    "_[elided — 41235 bytes did not fit the event cap]_",
  ]) {
    assert.deepEqual(
      parseRedactionMarkers(other),
      [{ kind: "text", text: other }],
      other,
    );
  }
});

test("hasRedactionMarker agrees with the parser and is not sticky", () => {
  assert.equal(hasRedactionMarker(`x ${MARKER}`), true);
  assert.equal(hasRedactionMarker(`x ${MARKER}`), true);
  assert.equal(hasRedactionMarker("plain prose"), false);
});

test("parsing is not sticky across calls", () => {
  assert.equal(parseRedactionMarkers(MARKER).length, 1);
  assert.equal(parseRedactionMarkers(MARKER).length, 1);
});

test("byte counts render in decimal units", () => {
  assert.equal(formatRedactedBytes(0), "0 B");
  assert.equal(formatRedactedBytes(148), "148 B");
  assert.equal(formatRedactedBytes(999), "999 B");
  assert.equal(formatRedactedBytes(1_000), "1.0 KB");
  assert.equal(formatRedactedBytes(2_140), "2.1 KB");
  assert.equal(formatRedactedBytes(41_235), "41 KB");
  assert.equal(formatRedactedBytes(3_000_000), "3.0 MB");
  assert.equal(formatRedactedBytes(Number.NaN), "unknown size");
  assert.equal(formatRedactedBytes(-1), "unknown size");
});

test("the pill label says redacted, not elided", () => {
  assert.equal(
    formatRedactionLabel({ bytes: 148, digest: DIGEST, raw: MARKER }),
    "redacted 148 B",
  );
});
