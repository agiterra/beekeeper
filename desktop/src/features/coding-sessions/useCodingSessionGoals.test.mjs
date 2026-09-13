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

async function signedGoal(channelId = CHANNEL) {
  const { finalizeEvent } = await import("nostr-tools/pure");
  const { buildCodingSessionGoalEvent } = await import(
    "./lib/codingSessionGoal.ts"
  );
  return finalizeEvent(
    {
      ...buildCodingSessionGoalEvent({
        channelId,
        sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        content: "Catch a goal published while the watch is attaching.",
      }),
      created_at: 1789264500,
    },
    new Uint8Array(32).fill(1),
  );
}

test("bounded live history catches a signed goal missed by the initial read without assuming readiness", async () => {
  const { act } = await import("@testing-library/react");
  const goal = await signedGoal();
  let onEvent;
  let reads = 0;
  const { result } = await renderGoals([CHANNEL], {
    fetchEventsCoalesced: async () => {
      reads++;
      return [];
    },
    subscribeLive: (filter, event, onReady) => {
      assert.deepEqual(filter, {
        kinds: [44227],
        "#h": [CHANNEL],
        limit: 1000,
      });
      assert.equal(
        onReady,
        undefined,
        "neither a timeout nor a local write proves server admission",
      );
      onEvent = event;
      return new Promise(() => {});
    },
  });
  await act(async () => {});
  assert.equal(result.current.goals.size, 0);
  assert.equal(
    result.current.resolved,
    true,
    "history settles independently of live readiness",
  );
  // The subscription may first time out locally. Its eventual server-side
  // history replay still carries the goal published after the HTTP snapshot.
  await act(async () => onEvent(goal));
  assert.equal(
    reads,
    1,
    "no timer or local-write catch-up pretends to be a server acknowledgement",
  );
  assert.equal([...result.current.goals.values()][0]?.eventId, goal.id);
});

test("refresh is stable, reads immediately, and does not replace the live watch", async () => {
  const { act } = await import("@testing-library/react");
  const goal = await signedGoal();
  let history = [];
  let watches = 0;
  const { result } = await renderGoals([CHANNEL], {
    fetchEventsCoalesced: async () => history,
    subscribeLive: () => {
      watches++;
      return new Promise(() => {});
    },
  });
  await act(async () => {});
  const refresh = result.current.refresh;
  history = [goal];
  await act(async () => refresh());
  assert.equal(result.current.refresh, refresh);
  assert.equal(watches, 1);
  assert.equal([...result.current.goals.values()][0]?.eventId, goal.id);
});

test("disposed scope ignores late live history and pending reads, including a pending watch", async () => {
  const { act } = await import("@testing-library/react");
  const goal = await signedGoal();
  const reads = [];
  const liveEvents = [];
  const watches = [];
  let oldUnsubscribed = false;
  const relay = {
    fetchEventsCoalesced: (filter) =>
      new Promise((resolve) => reads.push({ filter, resolve })),
    subscribeLive: (_filter, onEvent) =>
      new Promise((resolve) => {
        liveEvents.push(onEvent);
        watches.push(resolve);
      }),
  };
  const { result, rerender } = await renderGoals([CHANNEL], relay);
  rerender({ ids: ["chan-2"], relay });
  await act(async () => {
    reads[1].resolve([]);
    reads[0].resolve([goal]);
    liveEvents[0](goal);
    watches[0](() => {
      oldUnsubscribed = true;
    });
  });
  assert.equal(
    reads.length,
    2,
    "late old live history must not start another read",
  );
  assert.equal(oldUnsubscribed, true);
  assert.equal(result.current.goals.size, 0);
  assert.equal(result.current.resolved, true);
  await act(async () => result.current.refresh());
  assert.deepEqual(reads[2].filter["#h"], ["chan-2"]);
});

test("late initial read cannot overwrite a newer refresh answer or its error state", async () => {
  const { act } = await import("@testing-library/react");
  const goal = await signedGoal();
  const reads = [];
  const { result } = await renderGoals([CHANNEL], {
    fetchEventsCoalesced: () =>
      new Promise((resolve, reject) => reads.push({ resolve, reject })),
    subscribeLive: () => new Promise(() => {}),
  });
  await act(async () => result.current.refresh());
  await act(async () => reads[1].resolve([goal]));
  await act(async () => reads[0].reject(new Error("stale initial failure")));
  assert.equal(result.current.errorMessage, null);
  assert.equal(result.current.resolved, true);
  assert.equal([...result.current.goals.values()][0]?.eventId, goal.id);
  await act(async () => result.current.refresh());
  await act(async () => reads[2].reject(new Error("refresh refused")));
  assert.equal(result.current.errorMessage, "refresh refused");
  assert.equal(result.current.resolved, true);
  assert.equal(
    [...result.current.goals.values()][0]?.eventId,
    goal.id,
    "failed refresh retains the signed goal",
  );
});

test("late older history cannot replace a newer signed goal from refresh", async () => {
  const { act } = await import("@testing-library/react");
  const { finalizeEvent } = await import("nostr-tools/pure");
  const older = await signedGoal();
  const newer = finalizeEvent(
    { ...older, created_at: older.created_at + 1 },
    new Uint8Array(32).fill(1),
  );
  const reads = [];
  const { result } = await renderGoals([CHANNEL], {
    fetchEventsCoalesced: () => new Promise((resolve) => reads.push(resolve)),
    subscribeLive: () => new Promise(() => {}),
  });
  await act(async () => result.current.refresh());
  await act(async () => reads[1]([newer]));
  await act(async () => reads[0]([older]));
  assert.equal([...result.current.goals.values()][0]?.eventId, newer.id);
});

test("successful history does not erase a failed live watch", async () => {
  const { act } = await import("@testing-library/react");
  const goal = await signedGoal();
  let finishHistory;
  const { result } = await renderGoals([CHANNEL], {
    fetchEventsCoalesced: () =>
      new Promise((resolve) => {
        finishHistory = resolve;
      }),
    subscribeLive: async () => {
      throw new Error("watch was refused");
    },
  });
  await act(async () => {});
  assert.equal(result.current.errorMessage, "watch was refused");
  await act(async () => finishHistory([goal]));
  assert.equal(result.current.resolved, true);
  assert.equal([...result.current.goals.values()][0]?.eventId, goal.id);
  assert.equal(result.current.errorMessage, "watch was refused");
});

test("a full history window is disclosed and a current nonfull or empty refresh clears that disclosure", async () => {
  const { act } = await import("@testing-library/react");
  const { finalizeEvent } = await import("nostr-tools/pure");
  const goal = await signedGoal();
  let history = Array.from({ length: 1000 }, (_, index) =>
    finalizeEvent(
      { ...goal, created_at: goal.created_at + index },
      new Uint8Array(32).fill(1),
    ),
  );
  const newest = history.at(-1);
  const { result } = await renderGoals([CHANNEL], {
    fetchEventsCoalesced: async () => history,
    subscribeLive: () => new Promise(() => {}),
  });
  await act(async () => {});
  assert.match(
    result.current.errorMessage,
    /latest 1000.*older session goals may be missing/,
  );
  assert.equal([...result.current.goals.values()][0]?.eventId, newest.id);
  history = [newest];
  await act(async () => result.current.refresh());
  assert.equal(result.current.errorMessage, null);
  history = [];
  await act(async () => result.current.refresh());
  assert.equal(result.current.errorMessage, null);
  assert.equal(
    [...result.current.goals.values()][0]?.eventId,
    newest.id,
    "an empty refresh does not discard signed evidence already read",
  );
});
