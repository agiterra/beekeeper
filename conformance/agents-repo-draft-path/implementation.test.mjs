/**
 * Agents-repository draft path binder — runs the REAL Desktop grammar against
 * every banked vector. `README.md` states the law in prose; the fixtures state
 * it in data; this file binds the production TypeScript to that data, so
 * `buzz-core` (behind `bee agents-repo` and the Files tab) and the Dart reader
 * in Mobile have one shared, executable definition of "correct".
 *
 * Import constraint (load-bearing): the module must stay free of runtime
 * imports outside its own directory and use erasable TypeScript syntax only,
 * because this file runs under plain `node --test` (no loader) relying on
 * Node's native type stripping.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  draftPathClass,
  moveDestinationError,
} from "../../desktop/src/features/agents-repo/lib/agentsRepoDraftOp.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const corpus = JSON.parse(
  readFileSync(path.join(HERE, "fixtures", "path-vectors.json"), "utf8"),
);

test("the corpus is the schema this binder expects", () => {
  assert.equal(corpus.schema, "buzz-agents-repo-draft-path-vectors/v2");
  assert.ok(corpus.cases.length > 0, "an empty corpus proves nothing");
});

test("the Desktop grammar agrees with every vector", () => {
  for (const vector of corpus.cases) {
    const got = draftPathClass(vector.path);
    if (vector.class === null) {
      assert.equal(
        got.ok,
        false,
        `${JSON.stringify(vector.path)} should be refused${vector.note ? ` — ${vector.note}` : ""}`,
      );
    } else {
      assert.equal(
        got.ok,
        true,
        `${JSON.stringify(vector.path)} should be admitted as ${vector.class}: ${got.ok ? "" : got.error}`,
      );
      assert.equal(got.class, vector.class, JSON.stringify(vector.path));
    }
  }
});

test("the Desktop destination rule agrees with every move vector", () => {
  assert.ok(corpus.moves.length > 0, "the destination rule needs vectors too");
  for (const vector of corpus.moves) {
    const error = moveDestinationError(vector.from, vector.to);
    const note = vector.note ? ` — ${vector.note}` : "";
    assert.equal(
      error === null,
      vector.ok,
      `${vector.from} -> ${vector.to} should be ${vector.ok ? "legal" : "refused"}${note}${error ? `: ${error}` : ""}`,
    );
  }
});
