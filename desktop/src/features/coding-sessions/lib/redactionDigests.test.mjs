import assert from "node:assert/strict";
import test from "node:test";

import {
  collectRedactionLookupScope,
  MAX_REDACTION_DIGESTS,
} from "./redactionDigests.ts";

const SIGNER = "aa".repeat(32);
const OTHER_SIGNER = "bb".repeat(32);

function digest(seed) {
  return String(seed).padStart(64, "0");
}

function marker(bytes, seed) {
  return `[elided private context: ${bytes} bytes, sha256:${digest(seed)}]`;
}

function message(text, overrides = {}) {
  return {
    id: "item-1",
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Response",
    text,
    timestamp: "2026-08-26T00:00:00Z",
    sessionId: "session-1",
    bridgeSource: { pubkey: SIGNER, label: "provider" },
    ...overrides,
  };
}

test("a transcript with no marker is not worth asking about", () => {
  assert.equal(collectRedactionLookupScope([message("all clear")]), null);
});

test("markers yield their digests with the session and signer to ask", () => {
  const scope = collectRedactionLookupScope([
    message(`read ${marker(148, 1)} then ${marker(9, 2)}`),
  ]);
  assert.deepEqual(scope, {
    digests: [digest(1), digest(2)],
    sessionId: "session-1",
    signerPubkey: SIGNER,
  });
});

test("a digest repeated across items is asked for once", () => {
  const scope = collectRedactionLookupScope([
    message(`first ${marker(148, 1)}`),
    message(`again ${marker(148, 1)}`, { id: "item-2" }),
  ]);
  assert.deepEqual(scope.digests, [digest(1)]);
});

test("markers in tool titles, args, results, and previews are all found", () => {
  const scope = collectRedactionLookupScope([
    {
      id: "tool-1",
      type: "tool",
      renderClass: "shell",
      descriptor: {
        renderClass: "shell",
        label: `Ran ${marker(29, 4)}`,
        preview: `in ${marker(30, 5)}`,
        action: { verb: "Ran", object: `at ${marker(31, 6)}` },
      },
      title: `Ran ${marker(29, 1)}`,
      toolName: "Bash",
      buzzToolName: null,
      status: "completed",
      args: { command: `cd ${marker(12, 2)}` },
      result: `wrote ${marker(40, 3)}`,
      isError: false,
      timestamp: "2026-08-26T00:00:00Z",
      startedAt: null,
      completedAt: null,
      sessionId: "session-1",
      bridgeSource: { pubkey: SIGNER, label: "provider" },
    },
  ]);
  assert.deepEqual(scope.digests, [1, 2, 3, 4, 5, 6].map(digest));
});

// The vault is keyed by session id, and two machines can legitimately redact
// the same path — so a transcript that does not name exactly one signer and
// session has no single machine to ask, and asking anyway could resolve
// somebody else's value.

test("a transcript spanning two signers is refused rather than guessed", () => {
  const scope = collectRedactionLookupScope([
    message(`a ${marker(148, 1)}`),
    message(`b ${marker(9, 2)}`, {
      id: "item-2",
      bridgeSource: { pubkey: OTHER_SIGNER, label: "other" },
    }),
  ]);
  assert.equal(scope, null);
});

test("a transcript spanning two sessions is refused rather than guessed", () => {
  const scope = collectRedactionLookupScope([
    message(`a ${marker(148, 1)}`),
    message(`b ${marker(9, 2)}`, { id: "item-2", sessionId: "session-2" }),
  ]);
  assert.equal(scope, null);
});

test("an unattributed transcript has no machine to ask", () => {
  assert.equal(
    collectRedactionLookupScope([
      message(`a ${marker(148, 1)}`, { bridgeSource: null }),
    ]),
    null,
  );
  assert.equal(
    collectRedactionLookupScope([
      message(`a ${marker(148, 1)}`, { sessionId: null }),
    ]),
    null,
  );
});

test("the digest list is bounded", () => {
  const items = Array.from({ length: MAX_REDACTION_DIGESTS + 50 }, (_, index) =>
    message(`x ${marker(10, index)}`, { id: `item-${index}` }),
  );
  const scope = collectRedactionLookupScope(items);
  assert.equal(scope.digests.length, MAX_REDACTION_DIGESTS);
});

test("the digest list is order-stable so an unchanged transcript reuses its query", () => {
  const first = collectRedactionLookupScope([
    message(`${marker(1, 9)} ${marker(2, 3)}`),
  ]);
  const second = collectRedactionLookupScope([
    message(`${marker(2, 3)} ${marker(1, 9)}`),
  ]);
  assert.deepEqual(first.digests, second.digests);
});
