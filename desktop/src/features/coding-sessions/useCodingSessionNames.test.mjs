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
    fetchEvents: async () => [initial],
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
        publisher: {
          publishEvent: async () => renamed,
        },
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

test("a newly-added channel backfills names only after its live fence is ready", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionNameKey } = await import("./lib/codingSessionName.ts");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("First session name", 1_800_000_010);
  let relayHistory = [];
  let historyCalls = 0;
  let markLiveReady = () => {};
  const client = {
    fetchEvents: async () => {
      historyCalls += 1;
      return relayHistory;
    },
    subscribeLive: () =>
      new Promise((resolve) => {
        markLiveReady = () => resolve(() => {});
      }),
    subscribeToReconnects: () => () => {},
  };
  const { result, unmount } = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], client),
  );

  await act(async () => {});
  assert.equal(historyCalls, 0);

  relayHistory = [initial];
  await act(async () => markLiveReady());
  const key = codingSessionNameKey(CHANNEL_ID, SESSION_REF, FOUNDER_PUBKEY);
  assert.equal(historyCalls, 1);
  assert.equal(result.current.names.get(key)?.content, "First session name");

  unmount();
});

test("resolved is false until the history read settles, and true on success or on error", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Settled name", 1_800_000_020);
  let markLiveReady = () => {};
  const okClient = {
    fetchEvents: async () => [initial],
    subscribeLive: () =>
      new Promise((resolve) => {
        markLiveReady = () => resolve(() => {});
      }),
    subscribeToReconnects: () => () => {},
  };
  const ok = renderHook(() => useCodingSessionNames([CHANNEL_ID], okClient));
  // First render: nothing has been read, so nothing may be published over it.
  assert.equal(ok.result.current.resolved, false);
  await act(async () => {});
  assert.equal(ok.result.current.resolved, false);
  await act(async () => markLiveReady());
  assert.equal(ok.result.current.resolved, true);
  assert.equal(ok.result.current.errorMessage, null);
  ok.unmount();

  // A refusal is an answer: the read is over either way.
  const failingClient = {
    fetchEvents: async () => {
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
  failed.unmount();

  // Nothing to read is the one scope that starts settled.
  const empty = renderHook(() => useCodingSessionNames([], okClient));
  await act(async () => {});
  assert.equal(empty.result.current.resolved, true);
  empty.unmount();
});
