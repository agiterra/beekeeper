/**
 * The shared kind:44226 genesis vectors, run against this app's classifier.
 *
 * The same file `buzz-core` and the mobile decoder load
 * (`conformance/coding-session-records/44226-genesis/`). A lane that adds a
 * key to the genesis payload adds a vector first and watches every reader's
 * test fail — the rule ledger 204 cost us.
 *
 * Genesis is classified from a whole signed event, not from content alone, so
 * each vector is signed here and its `csg-session` tag is re-derived from the
 * payload exactly as the relay re-derives it at ingest. Nothing about the
 * reader changes: where it and buzz-core disagree, the fixture records the
 * disagreement and the assertion message names it.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import { finalizeEvent } from "nostr-tools/pure";

import { KIND_CODING_SESSION_GENESIS } from "@/shared/constants/kinds";

import { classifyCodingSessionGenesisEvent } from "./codingSessionCreateObservations.ts";
import { CODING_SESSION_GENESIS_TAG_VERSION } from "./codingSessionGenesis.ts";

const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const FOUNDER_SECRET = new Uint8Array(32).fill(3);
/** The placeholder a payload that names no sessionRef gets in its tag. */
const NO_SESSION_REF = "00000000-0000-4000-8000-000000000000";

test("desktop genesis classifier runs the shared 44226 vectors", () => {
  const fixture = JSON.parse(
    readFileSync(
      resolve(
        process.cwd(),
        "../conformance/coding-session-records/44226-genesis/fixtures/vectors.json",
      ),
      "utf8",
    ),
  );
  assert.equal(fixture.schema, "buzz-coding-session-record-conformance/v1");
  assert.equal(fixture.kind, 44226);
  assert.ok(fixture.vectors.length >= 10);

  const divergent = [];
  for (const vector of fixture.vectors) {
    assert.ok(vector.why, `vector ${vector.name} says why it exists`);
    const expected = vector.accepts.desktop;
    if (expected === null) continue;
    const content = JSON.stringify(vector.content);
    // Re-derived from the payload, never trusted — the relay's own rule.
    const sessionTag =
      typeof vector.content.sessionRef === "string"
        ? vector.content.sessionRef
        : NO_SESSION_REF;
    const event = finalizeEvent(
      {
        kind: KIND_CODING_SESSION_GENESIS,
        content,
        tags: [
          ["h", CHANNEL],
          ["csg-v", CODING_SESSION_GENESIS_TAG_VERSION],
          ["csg-session", sessionTag],
        ],
        created_at: 1,
      },
      FOUNDER_SECRET,
    );
    const actual =
      classifyCodingSessionGenesisEvent(event, new Set([CHANNEL])).kind ===
      "genesis";
    const pinned = vector.accepts.rust !== expected;
    assert.equal(
      actual,
      expected,
      pinned
        ? `PINNED DIVERGENCE moved: vector ${vector.name} is recorded as ${expected} ` +
            `here against buzz-core's ${vector.accepts.rust}, and now answers ${actual}. ` +
            `Update the fixture in the same commit as the reader.`
        : `vector ${vector.name} disagreed with the fixture (expected ${expected}, ` +
            `got ${actual}). See conformance/README.md.`,
    );
    if (pinned) divergent.push(vector.name);
  }
  // `"v": 1.0` is a float serde refuses for a `u64` and JavaScript cannot
  // tell from 1 — the one shape the two languages cannot agree on by
  // construction. Nothing signs it.
  assert.deepEqual(divergent, ["v-as-json-float"]);
});
