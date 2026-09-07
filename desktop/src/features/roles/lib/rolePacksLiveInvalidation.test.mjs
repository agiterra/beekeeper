import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { JSDOM } from "jsdom";

import {
  projectPacksLiveEffect,
  projectPacksLiveFilters,
} from "./rolePacksLiveInvalidation.ts";

const PROJECT_REF = `30621:${"a".repeat(64)}:beekeeper`;
const REPO_ID = "agiterra-packs";
const REPO_REF = `30617:${"b".repeat(64)}:${REPO_ID}`;

function source(overrides = {}) {
  return {
    eventId: "source-1",
    author: "a".repeat(64),
    createdAt: 1_780_000_000,
    repo: REPO_REF,
    ref: "refs/heads/main",
    sha: null,
    path: "personas/roles",
    note: null,
    ...overrides,
  };
}

function event({ id, kind, d, tags = [] }) {
  return {
    id,
    pubkey: "c".repeat(64),
    created_at: 1_780_000_001,
    kind,
    tags: [["d", d], ...tags],
    content: "",
    sig: "d".repeat(128),
  };
}

test("live filters watch the source and only a configured moving ref", () => {
  assert.deepEqual(projectPacksLiveFilters(PROJECT_REF, source()), [
    { kinds: [30624], "#d": [PROJECT_REF], limit: 1 },
    { kinds: [30618], "#d": [REPO_ID], limit: 1 },
  ]);
  assert.deepEqual(
    projectPacksLiveFilters(
      PROJECT_REF,
      source({ ref: null, sha: "e".repeat(40) }),
    ),
    [{ kinds: [30624], "#d": [PROJECT_REF], limit: 1 }],
  );
});

test("events are refresh hints scoped to the project and moving repository", () => {
  const context = {
    projectRef: PROJECT_REF,
    sourceEventId: "source-1",
    sourceRef: "refs/heads/main",
    sourceRepoId: REPO_ID,
  };
  assert.equal(
    projectPacksLiveEffect({
      ...context,
      event: event({ id: "source-1", kind: 30624, d: PROJECT_REF }),
    }),
    "none",
  );
  assert.equal(
    projectPacksLiveEffect({
      ...context,
      event: event({ id: "source-2", kind: 30624, d: PROJECT_REF }),
    }),
    "source-and-packs",
  );
  assert.equal(
    projectPacksLiveEffect({
      ...context,
      // A deleted branch is absent from the new head; the repo event still
      // invalidates the resolver that must disclose the missing branch.
      event: event({ id: "ref-2", kind: 30618, d: REPO_ID }),
    }),
    "packs",
  );
  assert.equal(
    projectPacksLiveEffect({
      ...context,
      sourceRef: null,
      event: event({ id: "ref-2", kind: 30618, d: REPO_ID }),
    }),
    "none",
  );
  assert.equal(
    projectPacksLiveEffect({
      ...context,
      event: event({ id: "foreign", kind: 30618, d: "other-packs" }),
    }),
    "none",
  );
});

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

after(() => dom.window.close());

test("mounted invalidation dedupes replay, refreshes on reconnect, rewires, and cleans up", async () => {
  const React = await import("react");
  const { act, cleanup, renderHook } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { useProjectPacksLiveInvalidation } = await import(
    "./rolePacksLiveInvalidation.ts"
  );

  const subscriptions = [];
  let reconnect = null;
  let reconnectDisposed = 0;
  const client = {
    async subscribeLive(filter, onEvent) {
      const subscription = { filter, onEvent, disposed: 0 };
      subscriptions.push(subscription);
      return () => {
        subscription.disposed += 1;
      };
    },
    subscribeToReconnects(listener) {
      reconnect = listener;
      return () => {
        reconnectDisposed += 1;
      };
    },
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidations = [];
  queryClient.invalidateQueries = async (filters) => {
    invalidations.push(filters.queryKey);
  };
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);

  try {
    const mounted = renderHook(
      ({ configuredSource }) =>
        useProjectPacksLiveInvalidation(PROJECT_REF, configuredSource, client),
      { initialProps: { configuredSource: source() }, wrapper },
    );
    await act(async () => {});
    assert.equal(subscriptions.length, 2);

    const sourceSubscription = subscriptions.find(
      ({ filter }) => filter.kinds[0] === 30624,
    );
    const refSubscription = subscriptions.find(
      ({ filter }) => filter.kinds[0] === 30618,
    );
    assert.ok(sourceSubscription);
    assert.ok(refSubscription);

    await act(async () => {
      sourceSubscription.onEvent(
        event({ id: "source-1", kind: 30624, d: PROJECT_REF }),
      );
      refSubscription.onEvent(event({ id: "ref-2", kind: 30618, d: REPO_ID }));
      refSubscription.onEvent(event({ id: "ref-2", kind: 30618, d: REPO_ID }));
    });
    assert.deepEqual(invalidations, [["role-packs", PROJECT_REF]]);

    await act(async () => {
      sourceSubscription.onEvent(
        event({ id: "source-2", kind: 30624, d: PROJECT_REF }),
      );
    });
    assert.deepEqual(invalidations.slice(-2), [
      ["project-pack-source", PROJECT_REF],
      ["role-packs", PROJECT_REF],
    ]);

    await act(async () => reconnect?.());
    assert.deepEqual(invalidations.slice(-2), [
      ["project-pack-source", PROJECT_REF],
      ["role-packs", PROJECT_REF],
    ]);

    await act(async () => {
      mounted.rerender({
        configuredSource: source({
          eventId: "source-2",
          ref: null,
          sha: "e".repeat(40),
        }),
      });
    });
    assert.equal(subscriptions.length, 3);
    assert.deepEqual(subscriptions[2].filter, {
      kinds: [30624],
      "#d": [PROJECT_REF],
      limit: 1,
    });
    assert.equal(sourceSubscription.disposed, 1);
    assert.equal(refSubscription.disposed, 1);
    assert.equal(reconnectDisposed, 1);

    mounted.unmount();
    assert.equal(subscriptions[2].disposed, 1);
    assert.equal(reconnectDisposed, 2);
    const afterUnmount = invalidations.length;
    subscriptions[2].onEvent(
      event({ id: "after-unmount", kind: 30624, d: PROJECT_REF }),
    );
    reconnect?.();
    assert.equal(invalidations.length, afterUnmount);
  } finally {
    cleanup();
    queryClient.clear();
  }
});

test("a failed live watch is disclosed and restarted on reconnect", async () => {
  const React = await import("react");
  const { act, cleanup, renderHook } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { useProjectPacksLiveInvalidation } = await import(
    "./rolePacksLiveInvalidation.ts"
  );
  let attempts = 0;
  let reconnect = null;
  const client = {
    async subscribeLive() {
      attempts += 1;
      if (attempts === 1) throw new Error("relay refused the watch");
      return () => {};
    },
    subscribeToReconnects(listener) {
      reconnect = listener;
      return () => {};
    },
  };
  const queryClient = new QueryClient();
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);

  try {
    const mounted = renderHook(
      () => useProjectPacksLiveInvalidation(PROJECT_REF, null, client),
      { wrapper },
    );
    await act(async () => {});
    assert.match(mounted.result.current.error, /relay refused the watch/);
    assert.match(mounted.result.current.error, /Reconnect or reopen/);

    await act(async () => reconnect?.());
    assert.equal(attempts, 2);
    assert.equal(mounted.result.current.error, null);
    mounted.unmount();
  } finally {
    cleanup();
    queryClient.clear();
  }
});
