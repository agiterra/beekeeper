/**
 * Notification-path tests for useLiveChannelUpdates.
 *
 * The hook fans one live event out to DM alerts, desktop notifications, the
 * mention chime, and the unread tracker. A coding-session lane message that
 * this client renders inside a session umbrella is not visible in the channel,
 * so none of those may fire for it — while an unresolved (or forged) lane ref
 * must keep behaving like ordinary chat.
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  installDOMShim,
  installFreshStorage,
} from "./observedUnreadTestHarness.mjs";

// DOM shim must run before any React import.
installDOMShim();
installFreshStorage();

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import {
  publishCodingSessionLaneRenderableRefs,
  resetCodingSessionLaneVisibility,
} from "@/features/messages/lib/codingSessionLaneVisibility";
import { relayClient } from "@/shared/api/relayClient";
import { CHANNEL_EVENT_KINDS } from "@/shared/constants/kinds";
import { useLiveChannelUpdates } from "./useLiveChannelUpdates.ts";

const CHANNEL_ID = "9f1c0b1a-2d3e-4f50-8a61-7b2c3d4e5f60";
const DM_CHANNEL_ID = "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const ME = "a".repeat(64);
const OTHER = "b".repeat(64);

let eventCounter = 0;

function chatEvent({ channelId = CHANNEL_ID, tags = [] } = {}) {
  eventCounter += 1;
  return {
    id: `event-${eventCounter}`.padEnd(64, "0"),
    pubkey: OTHER,
    created_at: Math.floor(Date.now() / 1_000) + 60,
    kind: 9,
    tags: [["h", channelId], ...tags],
    content: "hello",
    sig: "c".repeat(128),
  };
}

const laneTag = [["cs-session", SESSION_REF]];

/**
 * Mount the hook with the relay client stubbed so tests can push events
 * straight into the live subscription callback. There is one bundle per
 * 128 channels and no separate mention REQ — mentions are matched on the
 * live stream — so `emitMention` is the same delivery path as `emitLive`.
 * `requests` records every filter list handed to the transport and `log`
 * the open/close order per bundle, so tests can pin the REQ shapes and the
 * make-before-break sequence.
 */
async function mountLiveUpdates(channels, options) {
  const liveCallbacks = new Map();
  const requests = [];
  const log = [];
  const originalSubscribeLiveMany = relayClient.subscribeLiveMany;

  relayClient.subscribeLiveMany = async (filters, onEvent) => {
    requests.push(filters);
    const ids = filters.flatMap((filter) => filter["#h"] ?? []);
    for (const channelId of ids) {
      liveCallbacks.set(channelId, onEvent);
    }
    log.push(`open:${ids.join(",")}`);
    return async () => {
      log.push(`close:${ids.join(",")}`);
    };
  };

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  function Inner(props) {
    useLiveChannelUpdates(props.channels, null, options);
    return null;
  }

  const root = createRoot(document.createElement("div"));
  const render = (nextChannels) =>
    root.render(
      React.createElement(
        QueryClientProvider,
        { client: queryClient },
        React.createElement(Inner, { channels: nextChannels }),
      ),
    );
  await act(async () => {
    render(channels);
  });

  return {
    requests,
    log,
    rerender: async (nextChannels) => {
      await act(async () => {
        render(nextChannels);
      });
    },
    emitLive: async (channelId, event) => {
      await act(async () => {
        liveCallbacks.get(channelId)?.(event);
      });
    },
    emitMention: async (channelId, event) => {
      await act(async () => {
        liveCallbacks.get(channelId)?.(event);
      });
    },
    unmount: async () => {
      await act(async () => root.unmount());
      relayClient.subscribeLiveMany = originalSubscribeLiveMany;
      queryClient.clear();
    },
  };
}

function recorder() {
  const calls = {
    channelMessage: 0,
    dmMessage: 0,
    liveMention: 0,
    threadReplyDesktopNotification: 0,
    selfChannelMessage: 0,
  };
  return {
    calls,
    options: {
      currentPubkey: ME,
      onChannelMessage: () => {
        calls.channelMessage += 1;
      },
      onDmMessage: () => {
        calls.dmMessage += 1;
      },
      onLiveMention: () => {
        calls.liveMention += 1;
      },
      onThreadReplyDesktopNotification: () => {
        calls.threadReplyDesktopNotification += 1;
      },
      onSelfChannelMessage: () => {
        calls.selfChannelMessage += 1;
      },
    },
  };
}

test.beforeEach(() => {
  resetCodingSessionLaneVisibility();
});

test("a lane message with no openable lane still notifies like ordinary chat", async () => {
  const { calls, options } = recorder();
  const harness = await mountLiveUpdates(
    [{ id: CHANNEL_ID, name: "engineering", channelType: "stream" }],
    options,
  );

  await harness.emitLive(CHANNEL_ID, chatEvent());
  assert.equal(calls.channelMessage, 1);

  // Unresolved ref → the rule fails open, so this is ordinary attributable chat.
  await harness.emitLive(CHANNEL_ID, chatEvent({ tags: laneTag }));
  assert.equal(calls.channelMessage, 2);

  await harness.unmount();
});

test("a hidden lane message raises no desktop notification, DM alert, or unread", async () => {
  const { calls, options } = recorder();
  const harness = await mountLiveUpdates(
    [
      { id: CHANNEL_ID, name: "engineering", channelType: "stream" },
      { id: DM_CHANNEL_ID, name: "dm", channelType: "dm" },
    ],
    options,
  );

  publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF]));
  publishCodingSessionLaneRenderableRefs(DM_CHANNEL_ID, new Set([SESSION_REF]));

  await harness.emitLive(CHANNEL_ID, chatEvent({ tags: laneTag }));
  await harness.emitLive(
    DM_CHANNEL_ID,
    chatEvent({ channelId: DM_CHANNEL_ID, tags: laneTag }),
  );

  assert.deepEqual(calls, {
    channelMessage: 0,
    dmMessage: 0,
    liveMention: 0,
    threadReplyDesktopNotification: 0,
    selfChannelMessage: 0,
  });

  // Same channels, ordinary chat — proves the silence above was the lane rule
  // and not a broken harness.
  await harness.emitLive(CHANNEL_ID, chatEvent());
  await harness.emitLive(
    DM_CHANNEL_ID,
    chatEvent({ channelId: DM_CHANNEL_ID }),
  );
  assert.equal(calls.channelMessage, 2);
  assert.equal(calls.dmMessage, 1);

  await harness.unmount();
});

test("the mention subscription does not chime for a hidden lane mention", async () => {
  const { calls, options } = recorder();
  const harness = await mountLiveUpdates(
    [{ id: CHANNEL_ID, name: "engineering", channelType: "stream" }],
    options,
  );

  await harness.emitMention(CHANNEL_ID, chatEvent({ tags: [["p", ME]] }));
  assert.equal(calls.liveMention, 1);

  publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF]));
  await harness.emitMention(
    CHANNEL_ID,
    chatEvent({ tags: [["p", ME], ...laneTag] }),
  );
  assert.equal(calls.liveMention, 1);

  await harness.unmount();
});

test("every member channel rides one live REQ, limit 0 since now, and nothing else", async () => {
  const { options } = recorder();
  const before = Math.floor(Date.now() / 1_000);
  const harness = await mountLiveUpdates(
    [
      { id: CHANNEL_ID, name: "engineering", channelType: "stream" },
      { id: DM_CHANNEL_ID, name: "dm", channelType: "dm" },
    ],
    options,
  );
  const after = Math.floor(Date.now() / 1_000);

  assert.equal(harness.requests.length, 1);
  const live = harness.requests[0];
  assert.equal(live.length, 1);
  assert.ok(live[0].since >= before && live[0].since <= after);
  assert.deepEqual(
    { ...live[0], since: "now" },
    {
      kinds: [...CHANNEL_EVENT_KINDS],
      "#h": [CHANNEL_ID, DM_CHANNEL_ID].sort(),
      limit: 0,
      since: "now",
    },
  );

  await harness.unmount();
});

test("a mention on the live stream chimes once, and also counts as channel activity", async () => {
  const { calls, options } = recorder();
  const harness = await mountLiveUpdates(
    [{ id: CHANNEL_ID, name: "engineering", channelType: "stream" }],
    options,
  );

  const mention = chatEvent({ tags: [["p", ME]] });
  await harness.emitLive(CHANNEL_ID, mention);
  assert.equal(calls.liveMention, 1);
  assert.equal(calls.channelMessage, 1);

  // A reconnect replay of the same event neither chimes nor counts again.
  await harness.emitLive(CHANNEL_ID, mention);
  assert.equal(calls.liveMention, 1);
  assert.equal(calls.channelMessage, 1);

  // A message that mentions someone else, or nobody, never chimes.
  await harness.emitLive(CHANNEL_ID, chatEvent({ tags: [["p", OTHER]] }));
  await harness.emitLive(CHANNEL_ID, chatEvent());
  assert.equal(calls.liveMention, 1);
  assert.equal(calls.channelMessage, 3);

  await harness.unmount();
});

test("a membership change opens the new bundle before closing the old one", async () => {
  const { options } = recorder();
  const first = { id: CHANNEL_ID, name: "engineering", channelType: "stream" };
  const second = { id: DM_CHANNEL_ID, name: "dm", channelType: "dm" };
  const harness = await mountLiveUpdates([first], options);

  await harness.rerender([first, second]);

  const grown = [CHANNEL_ID, DM_CHANNEL_ID].sort().join(",");
  const opened = harness.log.indexOf(`open:${grown}`);
  const closed = harness.log.indexOf(`close:${CHANNEL_ID}`);
  assert.ok(opened >= 0, "bundle for both channels was opened");
  assert.ok(closed >= 0, "bundle for the lone channel was closed");
  assert.ok(opened < closed, "make before break");

  // Same membership again: nothing is reopened.
  const logLength = harness.log.length;
  await harness.rerender([second, first]);
  assert.equal(harness.log.length, logLength);

  await harness.unmount();
});

test("a live event without an h tag is dropped with a warning, never attributed", async () => {
  const { calls, options } = recorder();
  const harness = await mountLiveUpdates(
    [{ id: CHANNEL_ID, name: "engineering", channelType: "stream" }],
    options,
  );
  const originalWarn = console.warn;
  const warnings = [];
  console.warn = (...args) => warnings.push(args);
  try {
    await harness.emitLive(CHANNEL_ID, { ...chatEvent(), tags: [] });
  } finally {
    console.warn = originalWarn;
  }

  assert.equal(calls.channelMessage, 0);
  assert.equal(warnings.length, 1);

  await harness.unmount();
});
