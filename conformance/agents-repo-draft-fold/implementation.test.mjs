/**
 * Agents-repository draft fold implementation binder — runs the REAL
 * Desktop fold against every banked vector in this corpus. `CONTRACT.md`
 * states the law in prose; the fixtures state it in data; this file binds
 * the production TypeScript to that data, so the Rust fold in `buzz-core`
 * (used by `bee agents-repo`) and the Dart fold in Mobile have one shared,
 * executable definition of "correct".
 *
 * Import constraint (load-bearing): the fold modules must stay free of
 * runtime imports outside their own directory and use erasable TypeScript
 * syntax only, because this file runs under plain `node --test` (no loader)
 * relying on Node's native type stripping.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  AGENTS_REPO_DRAFT_DIGEST_SCHEMA,
  foldAgentsRepoDrafts,
} from "../../desktop/src/features/agents-repo/lib/agentsRepoDraftFold.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const folds = JSON.parse(
  readFileSync(path.join(HERE, "fixtures", "fold-vectors.json"), "utf8"),
);

test("the corpus pins the constants the fold codes", () => {
  assert.equal(folds.schema, "buzz-agents-repo-draft-fold-vectors/v2");
  assert.equal(AGENTS_REPO_DRAFT_DIGEST_SCHEMA, "buzz-agents-repo-draft-digest/v2");
  assert.ok(folds.cases.length > 0, "the corpus must hold vectors");
});

for (const vector of folds.cases) {
  test(`fold vector: ${vector.name}`, () => {
    const digest = foldAgentsRepoDrafts(vector.project, vector.repo, vector.events);
    assert.equal(JSON.stringify(digest), JSON.stringify(vector.expected));
  });
}
