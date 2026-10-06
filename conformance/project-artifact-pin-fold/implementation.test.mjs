/**
 * Project artifact pin fold binder — runs the REAL Desktop fold against every
 * banked vector. `CONTRACT.md` states the law in prose; the fixtures state it
 * in data; this file binds the production TypeScript to that data, so the Rust
 * fold in `beekeeper-core` (behind `bee pins`) and the Dart fold in Mobile have one
 * shared, executable definition of "correct".
 *
 * Import constraint (load-bearing): the modules under test must stay free of
 * `@/` aliases and npm imports and use erasable TypeScript syntax only,
 * because this file runs under plain `node --test` (no loader) relying on
 * Node's native type stripping.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  foldProjectArtifactPins,
  pinnedOnly,
  PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA,
} from "../../desktop/src/features/agents-repo/lib/artifactPinFold.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const folds = JSON.parse(
  readFileSync(path.join(HERE, "fixtures", "fold-vectors.json"), "utf8"),
);

test("the corpus is the schema this binder expects", () => {
  assert.equal(folds.schema, "buzz-project-artifact-pin-fold-vectors/v1");
  assert.equal(
    PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA,
    "buzz-project-artifact-pin-digest/v1",
  );
  assert.ok(folds.cases.length > 0, "an empty corpus proves nothing");
});

for (const vector of folds.cases) {
  test(`fold vector: ${vector.name}`, () => {
    const digest = foldProjectArtifactPins(
      vector.project,
      vector.repo,
      vector.events,
    );
    assert.deepEqual(JSON.parse(JSON.stringify(digest)), vector.expected);
  });
}

test("pinnedOnly keeps the digest's order and drops the unpinned", () => {
  // Every row the corpus produces is pinned except the un-pin case, so the
  // helper is checked against that one by name rather than a hand-built
  // digest that could drift from the type.
  const vector = folds.cases.find((c) =>
    c.name.startsWith("un-pinning keeps the row"),
  );
  assert.ok(vector, "the un-pin vector is the one that exercises this");
  const digest = foldProjectArtifactPins(
    vector.project,
    vector.repo,
    vector.events,
  );
  assert.equal(digest.pins.length, 1);
  assert.equal(digest.pins[0].pinned, false);
  assert.deepEqual(pinnedOnly(digest), []);
});
