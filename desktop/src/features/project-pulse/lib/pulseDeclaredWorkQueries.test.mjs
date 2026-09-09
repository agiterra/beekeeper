import assert from "node:assert/strict";
import { test } from "node:test";
import { InfiniteQueryObserver, QueryClient } from "@tanstack/react-query";
import {
  pulseDeclaredWorkQueryKey,
  pulseDeclaredWorkSnapshotKey,
} from "./pulseDeclaredWorkQueries.ts";

const sessions = Array.from({ length: 9 }, (_, index) => ({
  sessionKey: `session-${index}`,
  lifecycle: "open",
  latestObservationAt: 100 - index,
}));

function options(snapshot) {
  return {
    queryKey: [
      ...pulseDeclaredWorkQueryKey("project", ["channel"]),
      pulseDeclaredWorkSnapshotKey(snapshot),
      "viewer",
    ],
    initialPageParam: 0,
    queryFn: async ({ pageParam }) =>
      snapshot.slice(pageParam * 8, (pageParam + 1) * 8),
    getNextPageParam: (_lastPage, pages) =>
      pages.length * 8 < snapshot.length ? pages.length : undefined,
  };
}

test("activity between pages cannot mix old page boundaries with the new order", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const observer = new InfiniteQueryObserver(client, options(sessions));
  try {
    await observer.refetch();
    assert.deepEqual(
      observer
        .getCurrentResult()
        .data.pages.flat()
        .map((s) => s.sessionKey),
      sessions.slice(0, 8).map((s) => s.sessionKey),
    );
    // The previously unread ninth session becomes the newest. Reusing the old
    // first page would repeat session-7 and omit session-8 completely.
    const reordered = [sessions[8], ...sessions.slice(0, 8)];
    observer.setOptions(options(reordered));
    await observer.refetch();
    await observer.fetchNextPage();
    const read = observer.getCurrentResult().data.pages.flat();
    assert.deepEqual(
      read.map((s) => s.sessionKey),
      reordered.map((s) => s.sessionKey),
    );
    assert.equal(new Set(read.map((s) => s.sessionKey)).size, 9);
  } finally {
    observer.destroy();
    client.clear();
  }
});

test("closure replaces cached lifecycle even when session identity and order stay the same", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const observer = new InfiniteQueryObserver(client, options(sessions));
  try {
    await observer.refetch();
    const closed = sessions.map((session, index) =>
      index === 0 ? { ...session, lifecycle: "closed" } : session,
    );
    observer.setOptions(options(closed));
    assert.equal(observer.getCurrentResult().data, undefined);
    await observer.refetch();
    assert.equal(
      observer.getCurrentResult().data.pages[0][0].lifecycle,
      "closed",
    );
  } finally {
    observer.destroy();
    client.clear();
  }
});

test("unchanged order and lifecycle retain pages across timestamp-only observations", () => {
  assert.equal(
    pulseDeclaredWorkSnapshotKey(sessions),
    pulseDeclaredWorkSnapshotKey(
      sessions.map((s) => ({
        ...s,
        latestObservationAt: s.latestObservationAt + 1,
      })),
    ),
  );
});
