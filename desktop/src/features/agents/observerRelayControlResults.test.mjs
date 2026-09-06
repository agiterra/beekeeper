import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import {
  _testProcessLiveObserverEvents,
  resetAgentObserverStore,
  subscribeControlResults,
} from "./observerRelayStore.ts";

const AGENT_PUBKEY = "a".repeat(64);

function controlResult({ channelId, payload, seq }) {
  return {
    seq,
    timestamp: new Date(1_760_000_000_000 + seq * 1000).toISOString(),
    kind: "control_result",
    agentIndex: null,
    channelId,
    sessionId: null,
    turnId: null,
    payload,
  };
}

afterEach(() => {
  resetAgentObserverStore();
});

test("control-result subscribers receive the observer envelope channel", () => {
  const received = [];
  const unsubscribe = subscribeControlResults(AGENT_PUBKEY, (frame) => {
    received.push(frame);
  });

  _testProcessLiveObserverEvents(AGENT_PUBKEY, [
    controlResult({
      channelId: "target-channel",
      payload: { type: "cancel_turn", status: "sent", requestId: "match" },
      seq: 1,
    }),
    controlResult({
      channelId: "foreign-channel",
      payload: { type: "cancel_turn", status: "sent", requestId: "foreign" },
      seq: 2,
    }),
    controlResult({
      channelId: "envelope-channel",
      payload: {
        type: "cancel_turn",
        status: "sent",
        requestId: "conflict",
        channelId: "payload-channel",
      },
      seq: 3,
    }),
  ]);
  unsubscribe();

  assert.deepEqual(received, [
    {
      type: "cancel_turn",
      status: "sent",
      requestId: "match",
      channelId: "target-channel",
    },
    {
      type: "cancel_turn",
      status: "sent",
      requestId: "foreign",
      channelId: "foreign-channel",
    },
    {
      type: "cancel_turn",
      status: "sent",
      requestId: "conflict",
      channelId: "envelope-channel",
    },
  ]);
});
