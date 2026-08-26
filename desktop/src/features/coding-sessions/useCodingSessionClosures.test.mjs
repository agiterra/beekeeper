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

const CHANNEL_ID = "channel-session-closures";
const SESSION_REF = "9c4e2b10-3f5a-4d72-8b16-2e9a7d4c1f08";
const GENESIS_REF = "a".repeat(64);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const MEMBER_SECRET = generateSecretKey();

async function closureEvent(action, createdAt, secret = FOUNDER_SECRET) {
  const { buildCodingSessionClosureEvent } = await import(
    "./lib/codingSessionClosure.ts"
  );
  const built = buildCodingSessionClosureEvent({
    action,
    channelId: CHANNEL_ID,
    genesisRef: GENESIS_REF,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    secret,
  );
}

test("an accepted reopen updates every mounted closure consumer without a live echo", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionClosureKey, publishCodingSessionClosure } = await import(
    "./lib/codingSessionClosure.ts"
  );
  const { useCodingSessionClosures } = await import(
    "./useCodingSessionClosures.ts"
  );
  const closed = await closureEvent("closed", 1_800_000_000);
  const reopened = await closureEvent("open", 1_800_000_001, MEMBER_SECRET);
  const subscriptions = [];
  const client = {
    fetchEvents: async () => [closed],
    subscribeLive: async (_filter, onEvent) => {
      subscriptions.push(onEvent);
      return () => {};
    },
    subscribeToReconnects: () => () => {},
  };
  const founders = new Map([[GENESIS_REF, FOUNDER_PUBKEY]]);
  const first = renderHook(() =>
    useCodingSessionClosures([CHANNEL_ID], founders, client),
  );
  const second = renderHook(() =>
    useCodingSessionClosures([CHANNEL_ID], founders, client),
  );

  await act(async () => {});
  const key = codingSessionClosureKey(CHANNEL_ID, SESSION_REF, GENESIS_REF);
  assert.equal(first.result.current.closures.get(key)?.action, "closed");
  assert.equal(second.result.current.closures.get(key)?.action, "closed");
  assert.equal(subscriptions.length, 2);

  await act(async () => {
    await publishCodingSessionClosure(
      {
        action: "open",
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        sessionRef: SESSION_REF,
      },
      {
        signer: async () => reopened,
        publisher: { publishEvent: async () => reopened },
      },
    );
  });

  assert.equal(first.result.current.closures.get(key)?.action, "open");
  assert.equal(second.result.current.closures.get(key)?.action, "open");

  first.unmount();
  second.unmount();
});

test("a newly-added channel backfills closures only after its live fence is ready", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionClosureKey } = await import(
    "./lib/codingSessionClosure.ts"
  );
  const { useCodingSessionClosures } = await import(
    "./useCodingSessionClosures.ts"
  );
  const closed = await closureEvent("closed", 1_800_000_010);
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
  const founders = new Map([[GENESIS_REF, FOUNDER_PUBKEY]]);
  const { result, unmount } = renderHook(() =>
    useCodingSessionClosures([CHANNEL_ID], founders, client),
  );

  await act(async () => {});
  assert.equal(historyCalls, 0);

  relayHistory = [closed];
  await act(async () => markLiveReady());
  const key = codingSessionClosureKey(CHANNEL_ID, SESSION_REF, GENESIS_REF);
  assert.equal(historyCalls, 1);
  assert.equal(result.current.closures.get(key)?.action, "closed");

  unmount();
});

test("a closure read that failed before the relay connected recovers on the first connect", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionClosureKey } = await import(
    "./lib/codingSessionClosure.ts"
  );
  const { useCodingSessionClosures } = await import(
    "./useCodingSessionClosures.ts"
  );
  const closed = await closureEvent("closed", 1_800_000_020);
  let connected = false;
  const connectionListeners = new Set();
  const client = {
    fetchEvents: async () => {
      // Fatal by classification, so nothing but a connect can repair it —
      // which is what makes this a test of the arming, not of the backoff.
      if (!connected) {
        throw new Error("Relay session is terminal; cannot reconnect.");
      }
      return [closed];
    },
    subscribeLive: async () => {
      if (!connected) throw new Error("Relay socket is not connected.");
      return () => {};
    },
    // Deliberately silent: the relay client emits reconnects only *after* a
    // first successful connect, which is exactly the boot the shelf lost.
    subscribeToReconnects: () => () => {},
    subscribeToConnectionState: (listener) => {
      connectionListeners.add(listener);
      listener(connected ? "connected" : "connecting");
      return () => connectionListeners.delete(listener);
    },
  };
  const founders = new Map([[GENESIS_REF, FOUNDER_PUBKEY]]);
  const { result, unmount } = renderHook(() =>
    useCodingSessionClosures([CHANNEL_ID], founders, client),
  );

  await act(async () => {});
  const key = codingSessionClosureKey(CHANNEL_ID, SESSION_REF, GENESIS_REF);
  assert.equal(result.current.closures.size, 0);
  assert.match(String(result.current.errorMessage), /terminal|not connected/i);

  connected = true;
  await act(async () => {
    for (const listener of connectionListeners) listener("connected");
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

  assert.equal(result.current.closures.get(key)?.action, "closed");
  assert.equal(result.current.errorMessage, null);
  unmount();
});
