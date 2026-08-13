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
 * straight into the live and mention subscription callbacks.
 */
async function mountLiveUpdates(channels, options) {
  const liveCallbacks = new Map();
  const mentionCallbacks = new Map();
  const originalSubscribeLive = relayClient.subscribeLive;
  const originalSubscribeMentions = relayClient.subscribeToChannelMentionEvents;

  relayClient.subscribeLive = async (filter, onEvent) => {
    for (const channelId of filter["#h"] ?? []) {
      liveCallbacks.set(channelId, onEvent);
    }
    return async () => {};
  };
  relayClient.subscribeToChannelMentionEvents = async (
    channelId,
    _pubkey,
    onEvent,
  ) => {
    mentionCallbacks.set(channelId, onEvent);
    return async () => {};
  };

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  function Inner() {
    useLiveChannelUpdates(channels, null, options);
    return null;
  }

  const root = createRoot(document.createElement("div"));
  await act(async () => {
    root.render(
      React.createElement(
        QueryClientProvider,
        { client: queryClient },
        React.createElement(Inner),
      ),
    );
  });

  return {
    emitLive: async (channelId, event) => {
      await act(async () => {
        liveCallbacks.get(channelId)?.(event);
      });
    },
    emitMention: async (channelId, event) => {
      await act(async () => {
        mentionCallbacks.get(channelId)?.(event);
      });
    },
    unmount: async () => {
      await act(async () => root.unmount());
      relayClient.subscribeLive = originalSubscribeLive;
      relayClient.subscribeToChannelMentionEvents = originalSubscribeMentions;
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
