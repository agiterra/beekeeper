import assert from "node:assert/strict";
import { beforeEach, test } from "node:test";

import {
  MAX_RETAINED_CODING_SESSION_INGRESS_STORES,
  acquireCodingSessionIngressStore,
  peekCodingSessionIngressStore,
  resetCodingSessionIngressStores,
  retainedCodingSessionIngressStoreCount,
} from "./codingSessionIngressStoreCache.ts";

beforeEach(() => resetCodingSessionIngressStores());

test("the same scope gets the same store back", () => {
  const first = acquireCodingSessionIngressStore("authority-a|channel-1");
  const again = acquireCodingSessionIngressStore("authority-a|channel-1");
  assert.equal(again, first);
});

test("a different authority is a different store, never a shared one", () => {
  const open = acquireCodingSessionIngressStore("authority-a|channel-1");
  const restricted = acquireCodingSessionIngressStore("authority-b|channel-1");
  assert.notEqual(
    restricted,
    open,
    "facts verified under one allowlist must never be served to another",
  );
});

test("peek never mints a store", () => {
  assert.equal(peekCodingSessionIngressStore("authority-a|channel-1"), null);
  assert.equal(retainedCodingSessionIngressStoreCount(), 0);
  const store = acquireCodingSessionIngressStore("authority-a|channel-1");
  assert.equal(peekCodingSessionIngressStore("authority-a|channel-1"), store);
});

test("the least recently used scope is the one evicted", () => {
  const identity = (index) => `authority-a|channel-${index}`;
  for (
    let index = 0;
    index < MAX_RETAINED_CODING_SESSION_INGRESS_STORES;
    index += 1
  ) {
    acquireCodingSessionIngressStore(identity(index));
  }
  assert.equal(
    retainedCodingSessionIngressStoreCount(),
    MAX_RETAINED_CODING_SESSION_INGRESS_STORES,
  );

  // Touch the oldest so it is no longer the least recently used, then overflow.
  const revisited = acquireCodingSessionIngressStore(identity(0));
  acquireCodingSessionIngressStore(identity(999));

  assert.equal(
    retainedCodingSessionIngressStoreCount(),
    MAX_RETAINED_CODING_SESSION_INGRESS_STORES,
  );
  assert.equal(
    peekCodingSessionIngressStore(identity(0)),
    revisited,
    "the scope just switched to must never be the one evicted",
  );
  assert.equal(peekCodingSessionIngressStore(identity(1)), null);
});

test("a community switch drops every warm scope", () => {
  acquireCodingSessionIngressStore("authority-a|channel-1");
  acquireCodingSessionIngressStore("authority-a|channel-2");
  assert.equal(retainedCodingSessionIngressStoreCount(), 2);
  resetCodingSessionIngressStores();
  assert.equal(retainedCodingSessionIngressStoreCount(), 0);
  assert.equal(peekCodingSessionIngressStore("authority-a|channel-1"), null);
});
