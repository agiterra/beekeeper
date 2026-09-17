/**
 * Project to-do fold implementation binder — runs the REAL Desktop fold and
 * rank against every banked vector in this corpus. `CONTRACT.md` states the
 * law in prose; the fixtures state it in data; this file binds the production
 * TypeScript to that data, so the Rust fold in `buzz-core` (used by `bee
 * todos`) and the Dart fold in Mobile have one shared, executable definition
 * of "correct".
 *
 * Import constraint (load-bearing): the fold modules must stay free of runtime
 * imports outside their own directory and use erasable TypeScript syntax only,
 * because this file runs under plain `node --test` (no loader) relying on
 * Node's native type stripping.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  isValidRank,
  rankBetween,
} from "../../desktop/src/features/project-todos/lib/fractionalRank.ts";
import {
  PROJECT_TODO_DIGEST_SCHEMA,
  foldProjectTodos,
} from "../../desktop/src/features/project-todos/lib/todoFold.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const folds = JSON.parse(
  readFileSync(path.join(HERE, "fixtures", "fold-vectors.json"), "utf8"),
);
const ranks = JSON.parse(
  readFileSync(path.join(HERE, "fixtures", "rank-vectors.json"), "utf8"),
);

test("the corpus pins the constants the fold codes", () => {
  assert.equal(folds.schema, "buzz-project-todo-fold-vectors/v1");
  assert.equal(ranks.schema, "buzz-project-todo-rank-vectors/v1");
  assert.ok(folds.cases.length > 0, "the corpus must hold vectors");
  assert.equal(PROJECT_TODO_DIGEST_SCHEMA, "buzz-project-todo-digest/v1");
});

for (const vector of folds.cases) {
  test(`fold vector: ${vector.name}`, () => {
    const digest = foldProjectTodos(vector.project, vector.events);
    // Byte-identity, not deep equality: key order is part of the contract.
    assert.equal(
      JSON.stringify(digest, null, 2),
      JSON.stringify(vector.expected, null, 2),
    );
  });
}

test("rank vectors: rankBetween mints exactly the pinned key", () => {
  for (const [after, before, expected] of ranks.between) {
    assert.equal(rankBetween(after, before), expected, `${after}..${before}`);
  }
});

test("rank vectors: every invalid rank is refused", () => {
  for (const rank of ranks.invalid) {
    assert.equal(isValidRank(rank), false, JSON.stringify(rank));
  }
});
