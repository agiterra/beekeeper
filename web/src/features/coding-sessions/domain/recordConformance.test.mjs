/**
 * The shared closed-record vectors, run against **this** client's decoders.
 *
 * The same files `buzz-core`, the desktop and the mobile app load
 * (`conformance/coding-session-records/`, and `conformance/README.md` for the
 * rule). Until lane 223 nothing here loaded them, and the conformance README
 * asserted the web client "decodes none of these kinds" — which was wrong:
 * `ingressPayloads.ts` is a hand-copy of the desktop ingress decoder and
 * `trust.ts` drops every 44223 it refuses. Astra's 2026-09-21 re-check
 * imported these decoders and measured **10 metadata and 5 receipt
 * disagreements** with `buzz-core`. This file is what keeps that from
 * happening again: a fourth strict reader that no fixture answered for.
 *
 * Two suites per record, and the second is the point of the lane:
 *
 * - `vectors[]` hold a parsed `content` object, which is serialized again
 *   here. That can never express a duplicate key, because `JSON.parse` keeps
 *   one of two same-named keys and loses the fact that there were two.
 * - `rawVectors[]` hold the exact string a provider would sign, and
 *   **nothing re-serializes it**.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import {
  parseBeekeeperCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./ingressPayloads.ts";

/** Load one record's fixture and check the envelope it must carry. */
function fixture(recordDir, kind) {
  const parsed = JSON.parse(
    readFileSync(
      resolve(
        process.cwd(),
        `../conformance/coding-session-records/${recordDir}/fixtures/vectors.json`,
      ),
      "utf8",
    ),
  );
  assert.equal(parsed.schema, "buzz-coding-session-record-conformance/v1");
  assert.equal(parsed.kind, kind);
  assert.ok(
    typeof parsed.readers.web === "string",
    `${recordDir} names this client's reader, so it cannot be forgotten again`,
  );
  assert.ok(parsed.vectors.length >= 10);
  return parsed;
}

/**
 * Run every parsed vector through one source-string reader.
 *
 * No verdict here is allowed to be "pinned": this client is a copy of the
 * desktop decoder, so a divergence is a copy that drifted, not a design. The
 * fixture states `web` equal to `rust` for every vector and this asserts it.
 */
function runVectors(parsed, accepts) {
  let checked = 0;
  for (const vector of parsed.vectors) {
    assert.ok(vector.why, `vector ${vector.name} says why it exists`);
    assert.ok(
      Object.hasOwn(vector.accepts, "web"),
      `vector ${vector.name} states a verdict for this client`,
    );
    const expected = vector.accepts.web;
    if (expected === null) continue;
    assert.equal(
      accepts(JSON.stringify(vector.content)),
      expected,
      `vector ${vector.name}: this client disagreed with the fixture ` +
        `(expected ${expected}). buzz-core says ${vector.accepts.rust}. ` +
        `See conformance/README.md.`,
    );
    checked += 1;
  }
  assert.ok(checked > 0, "this client was exercised");
}

/** Run every raw vector through the same reader, byte for byte. */
function runRawVectors(parsed, accepts) {
  assert.ok(parsed.rawVectors.length >= 2);
  let accepted = 0;
  for (const vector of parsed.rawVectors) {
    assert.ok(vector.why, `raw vector ${vector.name} says why it exists`);
    const expected = vector.accepts.web;
    if (expected === null) continue;
    assert.equal(
      accepts(vector.raw),
      expected,
      `raw vector ${vector.name}: this client disagreed with the fixture ` +
        `(expected ${expected}). Bytes: ${vector.raw}`,
    );
    if (expected) accepted += 1;
  }
  assert.ok(
    accepted > 0,
    "the raw suite's positive control is accepted here, so this measures the " +
      "decoder rather than the loader",
  );
}

const metadata = (source) =>
  parseBeekeeperCodingSessionMetadata(source) !== null;
const receipt = (source) => parseCodingSessionLifecycleReceipt(source) !== null;

test("44223 metadata — this client runs the shared vectors", () => {
  const parsed = fixture("44223-metadata", 44223);
  runVectors(parsed, metadata);
});

test("44223 metadata — this client runs the raw vectors byte for byte", () => {
  runRawVectors(fixture("44223-metadata", 44223), metadata);
});

test("44224 lifecycle receipt — this client runs the shared vectors", () => {
  const parsed = fixture("44224-lifecycle-receipt", 44224);
  runVectors(parsed, receipt);
});

test("44224 lifecycle receipt — this client runs the raw vectors byte for byte", () => {
  runRawVectors(fixture("44224-lifecycle-receipt", 44224), receipt);
});
