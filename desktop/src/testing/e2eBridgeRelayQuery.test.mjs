import assert from "node:assert/strict";
import test from "node:test";
import {
  mockChannelHistoryPage,
  queryMockRelayFilters,
} from "./e2eBridgeRelayQuery.ts";

test("shared channel pages retain timestamp ties, inclusive cursors and per-filter limits", () => {
  const events = [
    { id: "z", kind: 44221, created_at: 11 },
    { id: "c", kind: 44221, created_at: 10 },
    { id: "a", kind: 44221, created_at: 10 },
    { id: "b", kind: 44221, created_at: 10 },
    { id: "other", kind: 44224, created_at: 10 },
    { id: "old", kind: 44221, created_at: 9 },
  ];
  assert.deepEqual(
    mockChannelHistoryPage(events, {
      kinds: [44221],
      since: 10,
      until: 10,
      limit: 2,
    }).map((event) => event.id),
    ["a", "b"],
  );
});

test("HTTP bundles reuse each exact filter and deduplicate the responder's union", async () => {
  const filters = [
    { kinds: [44221], "#h": ["channel-a"], until: 100, limit: 1 },
    { kinds: [44221, 44224], "#h": ["channel-a", "channel-b"], limit: 2 },
  ];
  const seen = [];
  const a = { id: "a", sig: "signed-a" };
  const b = { id: "b", sig: "signed-b" };
  const result = await queryMockRelayFilters(filters, (filter, id, send) => {
    seen.push(filter);
    send(["EVENT", id, a]);
    if (filter === filters[1]) send(["EVENT", id, b]);
    send(["EOSE", id]);
  });
  assert.deepEqual(seen, filters);
  assert.deepEqual(result, [a, b]);
});

test("a hung fixture remains pending until every responder sends EOSE", async () => {
  const completions = [];
  let settled = false;
  const pending = queryMockRelayFilters(
    [{ kinds: [44221] }, { kinds: [44226] }],
    (_filter, id, send) => {
      completions.push(() => send(["EOSE", id]));
      send(["EOSE", "unrelated-subscription"]);
    },
  ).then(() => {
    settled = true;
  });
  completions[0]();
  await Promise.resolve();
  assert.equal(settled, false);
  completions[1]();
  await pending;
  assert.equal(settled, true);
});

test("a fixture CLOSED refuses the batch with the original message", async () => {
  await assert.rejects(
    queryMockRelayFilters([{ kinds: [44221] }], (_filter, id, send) => {
      send(["CLOSED", id, "mock project query failure"]);
    }),
    /mock project query failure/,
  );
});

test("synchronous and asynchronous responder errors reject instead of hanging", async () => {
  for (const respond of [
    () => {
      throw new Error("sync refusal");
    },
    () => Promise.reject(new Error("async refusal")),
  ]) {
    await assert.rejects(
      queryMockRelayFilters([{ kinds: [44221] }], respond),
      /refusal/,
    );
  }
});

test("empty bundles do not dispatch reads", async () => {
  assert.deepEqual(
    await queryMockRelayFilters([], () => assert.fail("unexpected read")),
    [],
  );
});
