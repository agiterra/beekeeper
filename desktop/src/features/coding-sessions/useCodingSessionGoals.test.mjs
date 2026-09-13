import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const CHANNEL = "chan-1";

/** A client whose history fetch the test drives by hand. */
function client({ history, live = () => () => {} }) {
  return {
    // Settled on a macrotask, so the state update it causes lands inside the
    // test's own `act` scope rather than in React's "not wrapped in act"
    // warning path.
    fetchEventsCoalesced: () =>
      history instanceof Promise
        ? history
        : new Promise((resolve) => setTimeout(() => resolve(history), 0)),
    subscribeLive: (_filter, _onEvent) => Promise.resolve(live()),
  };
}

async function renderGoals(channelIds, goalClient) {
  const { renderHook } = await import("@testing-library/react");
  const { useCodingSessionGoals } = await import("./useCodingSessionGoals.ts");
  return renderHook(({ ids, relay }) => useCodingSessionGoals(ids, relay), {
    initialProps: { ids: channelIds, relay: goalClient },
  });
}

/**
 * L4.1: three different facts reached one sentence — no record, a reader that
 * never settled, and a reader that errored. `resolved` is what tells them
 * apart, so it must be false until the history fetch actually settles.
 */
test("a history fetch that never settles leaves the reader unresolved", async () => {
  const { result } = await renderGoals(
    [CHANNEL],
    client({ history: new Promise(() => {}) }),
  );

  assert.equal(result.current.resolved, false);
  assert.equal(result.current.errorMessage, null);
  assert.equal(result.current.goals.size, 0);
});

test("an error settles the reader and keeps the reader's own message", async () => {
  const { act } = await import("@testing-library/react");
  const { result } = await renderGoals(
    [CHANNEL],
    client({
      history: new Promise((_resolve, reject) =>
        setTimeout(() => reject(new Error("relay refused the filter")), 0),
      ),
    }),
  );

  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

  assert.equal(result.current.resolved, true);
  assert.equal(result.current.errorMessage, "relay refused the filter");
});

test("a settled fetch resolves the reader and folds what it read", async () => {
  const { act } = await import("@testing-library/react");
  const { result } = await renderGoals([CHANNEL], client({ history: [] }));

  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

  assert.equal(result.current.resolved, true);
  assert.equal(result.current.errorMessage, null);
  assert.equal(result.current.goals.size, 0);
});

test("no channel to read is resolved, not pending forever", async () => {
  const { result } = await renderGoals(
    [],
    client({ history: new Promise(() => {}) }),
  );

  assert.equal(result.current.resolved, true);
  assert.equal(result.current.errorMessage, null);
});

test("a new channel scope makes the reader unresolved again", async () => {
  const { act } = await import("@testing-library/react");
  const settled = client({ history: [] });
  const { result, rerender } = await renderGoals([CHANNEL], settled);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(result.current.resolved, true);

  rerender({
    ids: ["chan-2"],
    relay: client({ history: new Promise(() => {}) }),
  });

  assert.equal(result.current.resolved, false);
});

test("coalesced goal history settles with a signed goal while live admission is pending", async () => {
  const { act } = await import("@testing-library/react");
  const { finalizeEvent } = await import("nostr-tools/pure");
  const { buildCodingSessionGoalEvent } = await import(
    "./lib/codingSessionGoal.ts"
  );
  const goal = finalizeEvent(
    {
      ...buildCodingSessionGoalEvent({
        channelId: CHANNEL,
        sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        content: "Ship a portable provider-neutral team loop.",
      }),
      created_at: 1789264500,
    },
    new Uint8Array(32).fill(1),
  );
  const filters = [];
  let finishHistory;
  let finishWatch;
  let unsubscribed = false;
  const { result, unmount } = await renderGoals([CHANNEL, CHANNEL], {
    fetchEventsCoalesced: (filter) => {
      filters.push(filter);
      return new Promise((resolve) => {
        finishHistory = resolve;
      });
    },
    subscribeLive: () =>
      new Promise((resolve) => {
        finishWatch = resolve;
      }),
  });
  assert.deepEqual(filters, [{ kinds: [44227], "#h": [CHANNEL], limit: 1000 }]);
  assert.equal(result.current.resolved, false);
  await act(async () => finishHistory([goal]));
  assert.equal(result.current.resolved, true);
  assert.equal(result.current.errorMessage, null);
  assert.equal([...result.current.goals.values()][0].eventId, goal.id);
  unmount();
  await act(async () =>
    finishWatch(() => {
      unsubscribed = true;
    }),
  );
  assert.equal(unsubscribed, true);
});
