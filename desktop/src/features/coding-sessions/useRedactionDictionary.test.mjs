import assert from "node:assert/strict";
import test from "node:test";

import {
  resetRedactionDictionary,
  subscribeRedactionResolution,
} from "./useRedactionDictionary.ts";

const DIGEST = "a".repeat(64);
const OTHER_DIGEST = "b".repeat(64);

function scope(overrides = {}) {
  return {
    digests: [DIGEST],
    sessionId: "11111111-2222-3333-4444-555555555555",
    signerPubkey: "f".repeat(64),
    ...overrides,
  };
}

function deferredResolver(answer) {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const calls = [];
  const resolver = (request) => {
    calls.push(request);
    return gate.then(() => answer);
  };
  return { resolver, release, calls };
}

/** Let the resolver's promise chain run to completion. */
async function settled() {
  await new Promise((resolve) => setImmediate(resolve));
}

test("a subscriber that arrives while the lookup is in flight still gets the answer", async () => {
  resetRedactionDictionary();
  const { resolver, release, calls } = deferredResolver({
    [DIGEST]: { plaintext: "/Users/alice/checkout", class: "host-path" },
  });

  // StrictMode's mount → cleanup → mount, with the answer landing only after
  // the second mount. The first subscription is cancelled before it lands.
  const first = [];
  const unsubscribeFirst = subscribeRedactionResolution(
    scope(),
    (map) => first.push(map),
    resolver,
  );
  unsubscribeFirst();

  const second = [];
  subscribeRedactionResolution(scope(), (map) => second.push(map), resolver);

  release();
  await settled();

  assert.equal(calls.length, 1, "both subscriptions share one lookup");
  assert.equal(first.length, 0, "a cancelled subscriber hears nothing");
  assert.equal(second.length, 1, "the live subscriber hears the shared answer");
  assert.equal(second[0].get(DIGEST).plaintext, "/Users/alice/checkout");
});

test("an answer that already landed is served synchronously from the cache", async () => {
  resetRedactionDictionary();
  const { resolver, release } = deferredResolver({
    [DIGEST]: { plaintext: "/Users/alice/checkout", class: "host-path" },
  });
  subscribeRedactionResolution(scope(), () => {}, resolver);
  release();
  await settled();

  const later = [];
  subscribeRedactionResolution(
    scope(),
    (map) => later.push(map),
    () => assert.fail("a cached scope must not ask again"),
  );
  assert.equal(later.length, 1);
  assert.equal(later[0].get(DIGEST).class, "host-path");
});

test("a scope that answered empty is not asked again", async () => {
  resetRedactionDictionary();
  const { resolver, release, calls } = deferredResolver({});
  subscribeRedactionResolution(scope(), () => {}, resolver);
  release();
  await settled();

  subscribeRedactionResolution(
    scope(),
    () => assert.fail("an empty scope resolves nothing"),
    resolver,
  );
  await settled();
  assert.equal(calls.length, 1);
});

test("a rejected lookup may be retried by a later subscriber", async () => {
  resetRedactionDictionary();
  let attempts = 0;
  const failing = () => {
    attempts += 1;
    return Promise.reject(new Error("vault unreadable"));
  };
  const originalWarn = console.warn;
  console.warn = () => {};
  try {
    subscribeRedactionResolution(scope(), () => {}, failing);
    await settled();
    subscribeRedactionResolution(scope(), () => {}, failing);
    await settled();
  } finally {
    console.warn = originalWarn;
  }
  assert.equal(attempts, 2, "a rejection does not poison the scope");
});

test("distinct scopes resolve independently", async () => {
  resetRedactionDictionary();
  const answers = {
    [DIGEST]: { plaintext: "/Users/alice/one", class: "host-path" },
    [OTHER_DIGEST]: { plaintext: "cursor-9", class: "structural" },
  };
  const resolver = (request) =>
    Promise.resolve(
      Object.fromEntries(
        request.digests
          .filter((digest) => digest in answers)
          .map((digest) => [digest, answers[digest]]),
      ),
    );

  const seen = [];
  subscribeRedactionResolution(scope(), (map) => seen.push(map), resolver);
  subscribeRedactionResolution(
    scope({ digests: [OTHER_DIGEST] }),
    (map) => seen.push(map),
    resolver,
  );
  await settled();

  assert.equal(seen.length, 2);
  const plaintexts = seen.flatMap((map) =>
    [...map.values()].map((entry) => entry.plaintext),
  );
  assert.deepEqual(plaintexts.sort(), ["/Users/alice/one", "cursor-9"]);
});
