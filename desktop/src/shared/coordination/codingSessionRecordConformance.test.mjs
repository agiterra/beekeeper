/**
 * The shared closed-record vectors, run against this app's strict readers.
 *
 * One fixture per record under `conformance/coding-session-records/`, loaded
 * by every strict reader in every language (`conformance/README.md`). A lane
 * that adds a key to one of these records adds a vector first and watches
 * every reader's test fail — the rule ledger 204 cost us
 * (`docs/UNIFIED_WORK_PLAN.md` § 8 A3.3).
 *
 * Three of these records have **two** independent readers in this app, and the
 * fixtures record where they disagree with each other and with buzz-core.
 * Nothing here changes a reader: a divergence is asserted exactly as it stands
 * today, so the suite is green and the disagreement is pinned, named and
 * greppable rather than waiting to be found live.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import { parseBuzzCodingSessionMetadata } from "../../features/coding-sessions/lib/codingSessionIngressPayloads.ts";
import {
  hasStrictClosureJson,
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
  hasStrictLifecycleReceiptJson,
  hasStrictLifecycleReceiptValues,
  isStrictMetadataContent,
} from "./sessionCoordinationStrictJson.ts";
import { parseCodingSessionLifecycleReceipt } from "../../features/coding-sessions/lib/codingSessionIngressPayloads.ts";

/** Load one record's vectors and check the envelope it must carry. */
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
  assert.ok(parsed.vectors.length >= 10);
  return parsed;
}

/**
 * Run every vector through one named reader.
 *
 * `accepts[reader] === null` means this reader does not read the variant at
 * all, so there is nothing to agree or disagree with. A vector whose stated
 * verdict differs from `accepts.rust` is a pinned divergence: the assertion
 * message names it so a failure here is read as "the divergence moved", not
 * as "a test is flaky".
 */
function runReader(parsed, reader, accepts) {
  let checked = 0;
  const divergent = [];
  for (const vector of parsed.vectors) {
    assert.ok(vector.why, `vector ${vector.name} says why it exists`);
    assert.ok(
      Object.hasOwn(vector.accepts, reader),
      `vector ${vector.name} states a verdict for reader ${reader}`,
    );
    const expected = vector.accepts[reader];
    if (expected === null) continue;
    const actual = accepts(vector.content);
    const pinned =
      vector.accepts.rust !== null && vector.accepts.rust !== expected;
    assert.equal(
      actual,
      expected,
      pinned
        ? `PINNED DIVERGENCE moved: vector ${vector.name} — reader ${reader} is ` +
            `recorded as ${expected} against buzz-core's ${vector.accepts.rust}, and now ` +
            `answers ${actual}. Update the fixture in the same commit as the reader.`
        : `vector ${vector.name}: reader ${reader} disagreed with the fixture ` +
            `(expected ${expected}, got ${actual}). See conformance/README.md.`,
    );
    if (pinned) divergent.push(vector.name);
    checked += 1;
  }
  assert.ok(checked > 0, `reader ${reader} was exercised`);
  return divergent;
}

/** The two-step coordination readers take the source text and the value. */
const lifecycleCommand = (content) => {
  const source = JSON.stringify(content);
  return (
    hasStrictLifecycleCommandJson(source, content) &&
    hasStrictLifecycleCommandValues(content)
  );
};

const lifecycleReceipt = (content) => {
  const source = JSON.stringify(content);
  return (
    hasStrictLifecycleReceiptJson(source, content) &&
    hasStrictLifecycleReceiptValues(content)
  );
};

test("44221 lifecycle command — the coordination reader runs the shared vectors", () => {
  const parsed = fixture("44221-lifecycle-command", 44221);
  const divergent = runReader(parsed, "desktopCoordination", lifecycleCommand);
  // None left. `session.restart` — which this app writes itself — is now
  // read; `session.hire` names no `providerAuthorityPubkey` at all and is
  // read by the sibling `readLifecycleHire`, so its vectors state `null`
  // rather than a refusal (conformance/README.md).
  assert.deepEqual(divergent, []);
});

test("44223 metadata — the Pulse gate runs the shared vectors", () => {
  const parsed = fixture("44223-metadata", 44223);
  const divergent = runReader(parsed, "desktopGate", (content) =>
    isStrictMetadataContent(JSON.stringify(content)),
  );
  // Both closed in lane 216: `composeRef` is taught, and an explicit
  // `"routing": null` is refused rather than short-circuited past.
  assert.deepEqual(divergent, []);
});

test("44223 metadata — the session ingress decoder runs the shared vectors", () => {
  const parsed = fixture("44223-metadata", 44223);
  const divergent = runReader(
    parsed,
    "desktopIngress",
    (content) =>
      parseBuzzCodingSessionMetadata(JSON.stringify(content)) !== null,
  );
  // Both closed in lane 216: `handover` is taught, and the three summary keys
  // no writer has ever emitted are gone from its optional list.
  assert.deepEqual(divergent, []);
});

test("44224 lifecycle receipt — the coordination reader runs the shared vectors", () => {
  const parsed = fixture("44224-lifecycle-receipt", 44224);
  const divergent = runReader(parsed, "desktopCoordination", lifecycleReceipt);
  // Deliberate scope: this reader is the lifecycle vocabulary only. The turn
  // stages are read by the ingress decoder below, and the fold discriminates
  // by tag. Pinned so the scope stays a decision rather than an accident.
  assert.equal(divergent.length, 9);
});

test("44224 lifecycle receipt — the session ingress decoder runs the shared vectors", () => {
  const parsed = fixture("44224-lifecycle-receipt", 44224);
  const divergent = runReader(
    parsed,
    "desktopIngress",
    (content) =>
      parseCodingSessionLifecycleReceipt(JSON.stringify(content)) !== null,
  );
  // Both bounds now match buzz-core's exactly (64-byte code, 1027-byte
  // message), on the turn half and the lifecycle half alike.
  assert.deepEqual(divergent, []);
});

test("44230 closure — the coordination reader runs the shared vectors", () => {
  const parsed = fixture("44230-closure", 44230);
  const divergent = runReader(parsed, "desktopCoordination", (content) =>
    hasStrictClosureJson(JSON.stringify(content), content),
  );
  // `"v": 1.0` is a float serde refuses for a u64 and JavaScript cannot tell
  // from 1. Nothing signs it; it is pinned because it is the one shape where
  // the two languages cannot agree by construction.
  assert.deepEqual(divergent, ["v-as-json-float"]);
});
