import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { JSDOM } from "jsdom";

import {
  projectPacksResolutionNeedsRefresh,
  projectPacksLiveEffect,
  projectPacksLiveFilters,
  rolePackRevisionsQueryKey,
} from "./rolePacksLiveInvalidation.ts";

const PROJECT_REF = `30621:${"a".repeat(64)}:beekeeper`;
const OTHER_PROJECT_REF = `30621:${"a".repeat(64)}:other-project`;
const REPO_ID = "agiterra-packs";
const REPO_REF = `30617:${"b".repeat(64)}:${REPO_ID}`;
const OTHER_REPO_REF = `30617:${"b".repeat(64)}:other-packs`;

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

function revisionsKeyInput(overrides = {}) {
  return {
    projectRef: PROJECT_REF,
    sourceEventId: "source-1",
    sourceRepo: REPO_REF,
    currentResolvedSha: "a".repeat(40),
    packsUpdatedAt: 1_780_000_000_000,
    shas: ["b".repeat(40), "c".repeat(40)],
    ...overrides,
  };
}

test("rolePackRevisionsQueryKey's first element identifies the query", () => {
  assert.equal(
    rolePackRevisionsQueryKey(revisionsKeyInput())[0],
    "role-pack-revisions",
  );
});

test("rolePackRevisionsQueryKey is stable for identical inputs", () => {
  assert.deepEqual(
    rolePackRevisionsQueryKey(revisionsKeyInput()),
    rolePackRevisionsQueryKey(revisionsKeyInput()),
  );
});

test("rolePackRevisionsQueryKey changes when the source repo changes — switching repositories must never reuse old data", () => {
  assert.notDeepEqual(
    rolePackRevisionsQueryKey(revisionsKeyInput()),
    rolePackRevisionsQueryKey(
      revisionsKeyInput({ sourceRepo: OTHER_REPO_REF }),
    ),
  );
});

test("rolePackRevisionsQueryKey changes when the source event revision changes", () => {
  assert.notDeepEqual(
    rolePackRevisionsQueryKey(revisionsKeyInput()),
    rolePackRevisionsQueryKey(revisionsKeyInput({ sourceEventId: "source-2" })),
  );
});

test("rolePackRevisionsQueryKey changes when the packs list's refresh completion moves forward, even with HEAD unchanged — a manual refresh must re-rank", () => {
  const input = revisionsKeyInput();
  assert.notDeepEqual(
    rolePackRevisionsQueryKey(input),
    rolePackRevisionsQueryKey({
      ...input,
      packsUpdatedAt: input.packsUpdatedAt + 1,
    }),
  );
});

test("rolePackRevisionsQueryKey changes when the resolved sha changes", () => {
  assert.notDeepEqual(
    rolePackRevisionsQueryKey(revisionsKeyInput()),
    rolePackRevisionsQueryKey(
      revisionsKeyInput({ currentResolvedSha: "d".repeat(40) }),
    ),
  );
});

test("rolePackRevisionsQueryKey changes when the compared sha set changes", () => {
  assert.notDeepEqual(
    rolePackRevisionsQueryKey(revisionsKeyInput()),
    rolePackRevisionsQueryKey(revisionsKeyInput({ shas: ["b".repeat(40)] })),
  );
});

test("rolePackRevisionsQueryKey does not re-sort shas — callers pass revisionShas's already-sorted output", () => {
  const unsorted = ["c".repeat(40), "b".repeat(40)];
  assert.deepEqual(
    rolePackRevisionsQueryKey(revisionsKeyInput({ shas: unsorted })).at(-1),
    unsorted,
  );
});

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
    "source",
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

test("native packs refresh only after the authoritative source revision changes", () => {
  assert.equal(projectPacksResolutionNeedsRefresh(undefined, undefined), false);
  assert.equal(projectPacksResolutionNeedsRefresh(undefined, "source-1"), true);
  assert.equal(projectPacksResolutionNeedsRefresh(null, "source-1"), true);
  assert.equal(projectPacksResolutionNeedsRefresh("source-1", null), true);
  assert.equal(
    projectPacksResolutionNeedsRefresh("source-1", "source-1"),
    false,
  );
  // A failed or still-pending source read cannot supersede the revision that
  // produced the currently displayed native result.
  assert.equal(
    projectPacksResolutionNeedsRefresh("source-1", undefined),
    false,
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
    assert.deepEqual(invalidations.slice(-1), [
      ["project-pack-source", PROJECT_REF],
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

test("an authoritative source update supersedes an older in-flight native resolution", async () => {
  const React = await import("react");
  const { act, cleanup, renderHook, waitFor } = await import(
    "@testing-library/react"
  );
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );

  const pending = [];
  const tauriInternals = {
    invoke: (command) => {
      assert.equal(command, "list_project_role_packs");
      return new Promise((resolve) => pending.push(resolve));
    },
    transformCallback: () => Math.random(),
  };
  globalThis.__TAURI_INTERNALS__ = tauriInternals;
  window.__TAURI_INTERNALS__ = tauriInternals;
  const { useRolePacksQuery } = await import("./useProjectPacksView.ts");
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);

  try {
    const mounted = renderHook(
      ({ projectRef, sourceEventId }) =>
        useRolePacksQuery(projectRef, sourceEventId),
      {
        initialProps: { projectRef: PROJECT_REF, sourceEventId: "source-1" },
        wrapper,
      },
    );
    await waitFor(() => assert.equal(pending.length, 1));

    // The source query wins the race before the native resolver's old read
    // answers. No source event body is treated as pack data.
    mounted.rerender({
      projectRef: PROJECT_REF,
      sourceEventId: "source-2",
    });
    await act(async () => pending[0]([{ slug: "old-pack" }]));
    await waitFor(() => assert.equal(pending.length, 2));
    await act(async () => pending[1]([{ slug: "new-pack" }]));
    await waitFor(() =>
      assert.equal(mounted.result.current.data?.sourceEventId, "source-2"),
    );
    assert.deepEqual(mounted.result.current.data?.packs, [
      { slug: "new-pack" },
    ]);

    // The attempted revision belongs to its project. A fresh cached result
    // for another project can need the same revision and must still refresh.
    queryClient.setQueryData(["role-packs", OTHER_PROJECT_REF], {
      packs: [{ slug: "other-old-pack" }],
      sourceEventId: "source-1",
    });
    mounted.rerender({
      projectRef: OTHER_PROJECT_REF,
      sourceEventId: "source-2",
    });
    await waitFor(() => assert.equal(pending.length, 3));
    await act(async () => pending[2]([{ slug: "other-new-pack" }]));
    await waitFor(() =>
      assert.deepEqual(mounted.result.current.data?.packs, [
        { slug: "other-new-pack" },
      ]),
    );

    mounted.unmount();
  } finally {
    cleanup();
    queryClient.clear();
  }
});
