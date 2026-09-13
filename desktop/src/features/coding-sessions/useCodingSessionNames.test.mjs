import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

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

const CHANNEL_ID = "channel-session-names";
const SESSION_REF = "9c4e2b10-3f5a-4d72-8b16-2e9a7d4c1f08";
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);

async function nameEvent(content, createdAt) {
  const { buildCodingSessionNameEvent } = await import(
    "./lib/codingSessionName.ts"
  );
  const built = buildCodingSessionNameEvent({
    channelId: CHANNEL_ID,
    content,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  );
}

test("an accepted local rename updates every mounted name consumer without a live echo", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionNameKey, publishCodingSessionName } = await import(
    "./lib/codingSessionName.ts"
  );
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Initial name", 1_800_000_000);
  const renamed = await nameEvent("Renamed everywhere", 1_800_000_001);
  const subscriptions = [];
  const client = {
    publishEvent: async () => renamed,
    fetchEventsCoalesced: async () => [initial],
    subscribeLive: async (_filter, onEvent) => {
      subscriptions.push(onEvent);
      return () => {};
    },
    subscribeToReconnects: () => () => {},
  };
  const first = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));
  const second = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));

  await act(async () => {});
  const key = codingSessionNameKey(CHANNEL_ID, SESSION_REF, FOUNDER_PUBKEY);
  assert.equal(first.result.current.names.get(key)?.content, "Initial name");
  assert.equal(second.result.current.names.get(key)?.content, "Initial name");
  assert.equal(subscriptions.length, 2);

  await act(async () => {
    await publishCodingSessionName(
      {
        channelId: CHANNEL_ID,
        content: "Renamed everywhere",
        sessionRef: SESSION_REF,
      },
      {
        signer: async () => renamed,
        publisher: client,
      },
    );
  });

  assert.equal(
    first.result.current.names.get(key)?.content,
    "Renamed everywhere",
  );
  assert.equal(
    second.result.current.names.get(key)?.content,
    "Renamed everywhere",
  );

  first.unmount();
  second.unmount();
});

test("immediate history and bounded live replay cover an attaching watch", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionNameKey } = await import("./lib/codingSessionName.ts");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Between history and watch", 1_800_000_010);
  let deliverLive;
  const filters = [];
  const client = {
    fetchEventsCoalesced: async (filter) => {
      filters.push(filter);
      return [];
    },
    subscribeLive: (filter, onEvent) => {
      filters.push(filter);
      deliverLive = onEvent;
      return new Promise(() => {});
    },
  };
  const { result, unmount } = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], client),
  );
  await act(async () => {});
  assert.equal(
    result.current.resolved,
    true,
    "HTTP history does not wait on live admission",
  );
  assert.deepEqual(filters, [
    { kinds: [44229], "#h": [CHANNEL_ID], limit: 1000 },
    { kinds: [44229], "#h": [CHANNEL_ID], limit: 1000 },
  ]);
  await act(async () => deliverLive(initial));
  assert.equal(
    result.current.names.get(
      codingSessionNameKey(CHANNEL_ID, SESSION_REF, FOUNDER_PUBKEY),
    )?.content,
    "Between history and watch",
  );
  unmount();
});

test("resolved is false until the history read settles, and true on success or on error", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Settled name", 1_800_000_020);
  let finishHistory;
  const okClient = {
    fetchEventsCoalesced: () =>
      new Promise((resolve) => {
        finishHistory = resolve;
      }),
    subscribeLive: () => new Promise(() => {}),
    subscribeToReconnects: () => () => {},
  };
  const ok = renderHook(() => useCodingSessionNames([CHANNEL_ID], okClient));
  // First render: nothing has been read, so nothing may be published over it.
  assert.equal(ok.result.current.resolved, false);
  await act(async () => {});
  assert.equal(ok.result.current.resolved, false);
  await act(async () => finishHistory([initial]));
  assert.equal(ok.result.current.resolved, true);
  assert.equal(ok.result.current.errorMessage, null);
  ok.unmount();

  // A refusal is an answer: the read is over either way.
  const failingClient = {
    fetchEventsCoalesced: async () => {
      throw new Error("forbidden: not a member of this channel");
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };
  const failed = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], failingClient),
  );
  assert.equal(failed.result.current.resolved, false);
  await act(async () => {});
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(failed.result.current.resolved, true);
  assert.match(failed.result.current.errorMessage ?? "", /forbidden/);
  assert.match(failed.result.current.readErrorMessage ?? "", /forbidden/);
  failed.unmount();

  // Nothing to read is the one scope that starts settled.
  const empty = renderHook(() => useCodingSessionNames([], okClient));
  await act(async () => {});
  assert.equal(empty.result.current.resolved, true);
  empty.unmount();
});

test("refresh retries a failed watch, preserves signed names, and discloses a full history window", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Known name", 1_800_000_020);
  const fullHistory = Array.from({ length: 1000 }, (_, index) =>
    finalizeEvent(
      { ...initial, created_at: initial.created_at + index },
      FOUNDER_SECRET,
    ),
  );
  let full = true,
    attempts = 0;
  const client = {
    fetchEventsCoalesced: async () => (full ? fullHistory : []),
    subscribeLive: async () => {
      attempts++;
      if (attempts === 1) throw new Error("watch failed");
      return () => {};
    },
  };
  const { result, unmount } = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], client),
  );
  await act(async () => {});
  assert.match(result.current.readErrorMessage, /latest 1000/);
  assert.match(result.current.errorMessage, /watch failed/);
  assert.equal(result.current.names.size, 1);
  const refresh = result.current.refresh;
  full = false;
  await act(async () => refresh());
  assert.equal(result.current.refresh, refresh);
  assert.equal(attempts, 2);
  assert.equal(result.current.readErrorMessage, null);
  assert.equal(result.current.errorMessage, null);
  assert.equal(result.current.names.size, 1);
  unmount();
});

test("scope disposal ignores late history and unsubscribes a pending old watch", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Old channel", 1_800_000_020);
  const reads = [],
    watches = [];
  let closed = false,
    connected;
  const client = {
    fetchEventsCoalesced: (filter) =>
      new Promise((resolve) => reads.push({ filter, resolve })),
    subscribeLive: () => new Promise((resolve) => watches.push(resolve)),
    subscribeToConnectionState: (fn) => {
      connected = fn;
      return () => {};
    },
  };
  const { result, rerender, unmount } = renderHook(
    ({ channel }) => useCodingSessionNames([channel], client),
    { initialProps: { channel: CHANNEL_ID } },
  );
  rerender({ channel: "other-channel" });
  await act(async () => {
    reads[0].resolve([initial]);
    watches[0](() => {
      closed = true;
    });
    reads[1].resolve([]);
  });
  assert.equal(closed, true);
  assert.equal(result.current.names.size, 0);
  await act(async () => connected("connected"));
  assert.deepEqual(reads[2].filter["#h"], ["other-channel"]);
  unmount();
});

test("a held rename cannot enter a remounted community with the same client and coordinates", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const { publishCodingSessionName } = await import(
    "./lib/codingSessionName.ts"
  );
  const signed = await nameEvent("Old community result", 1_800_000_030);
  let finish, notifyStarted;
  const started = new Promise((resolve) => (notifyStarted = resolve));
  const client = {
    fetchEventsCoalesced: async () => [],
    subscribeLive: async () => () => {},
    publishEvent: () => {
      notifyStarted();
      return new Promise((resolve) => (finish = resolve));
    },
  };
  const old = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));
  await act(async () => {});
  const pending = publishCodingSessionName(
    { channelId: CHANNEL_ID, sessionRef: SESSION_REF, content: signed.content },
    { signer: async () => signed, publisher: client },
  );
  await started;
  old.unmount();
  const next = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));
  await act(async () => {
    finish(signed);
    await pending;
  });
  assert.equal(next.result.current.names.size, 0);
  next.unmount();
});
