/**
 * Project Pulse fold implementation binder — runs the REAL Desktop fold
 * against every banked vector in this corpus. `CONTRACT.md` states the law in
 * prose; `fixtures/fold-vectors.json` states it in data; this file binds the
 * production TypeScript to that data, so the Rust fold in `buzz-cli` and the
 * Slice 2 relay fold have one shared, executable definition of "correct".
 *
 * Import constraint (load-bearing): the fold modules must stay free of runtime
 * imports outside their own directory and use erasable TypeScript syntax only,
 * because this file runs under plain `node --test` (no loader) relying on
 * Node's native type stripping. If that constraint breaks, this suite fails
 * loudly with a module-load error — never silently.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  PULSE_DIGEST_SCHEMA,
  PULSE_SESSIONS_SCOPE,
  foldProjectPulseDigest,
} from "../../desktop/src/features/project-pulse/lib/pulseFold.ts";
import { PULSE_ENTRY_SCHEMA } from "../../desktop/src/features/project-pulse/lib/pulseEntry.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const corpus = JSON.parse(
  readFileSync(path.join(HERE, "fixtures", "fold-vectors.json"), "utf8"),
);

test("the corpus pins the constants the fold codes", () => {
  assert.equal(corpus.schema, "buzz-project-pulse-fold-vectors/v2");
  assert.equal(corpus.digestSchema, PULSE_DIGEST_SCHEMA);
  assert.equal(corpus.entrySchema, PULSE_ENTRY_SCHEMA);
  assert.ok(corpus.vectors.length > 0, "the corpus must hold vectors");
});

for (const vector of corpus.vectors) {
  test(`fold vector: ${vector.name}`, () => {
    const digest = foldProjectPulseDigest({
      project: vector.input.project,
      now: vector.input.now,
      events: vector.input.events,
      sourceErrors: vector.input.sourceErrors,
    });
    // Byte-identity, not deep equality: the Rust fold asserts the same thing
    // against the same file, so key order is part of the contract.
    assert.equal(
      JSON.stringify(digest, null, 2),
      JSON.stringify(vector.expected, null, 2),
      vector.description,
    );
    assert.equal(digest.sessionsScope, PULSE_SESSIONS_SCOPE);
  });
}

test("a partial read is never printed as a complete digest", () => {
  const digest = foldProjectPulseDigest({
    project:
      "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo",
    now: 1785513037,
    events: [],
    sourceErrors: [{ scope: "entries", message: "relay unavailable" }],
  });
  assert.equal(digest.complete, false);
  assert.equal(digest.errors.length, 1);
});
