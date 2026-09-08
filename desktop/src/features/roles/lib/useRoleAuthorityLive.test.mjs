import assert from "node:assert/strict";
import { after, before, test } from "node:test";

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

after(() => dom.window.close());

function event({ id, kind, channel, content = "{}" }) {
  return {
    id,
    pubkey: "a".repeat(64),
    created_at: 1_800_000_000,
    kind,
    tags: [["h", channel]],
    content,
    sig: "b".repeat(128),
  };
}

async function settle(act, rounds = 4) {
  for (let round = 0; round < rounds; round += 1) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
}

test("subscribes in stable channel chunks, backfills after establishment, and filters live hints", async () => {
  const { act, cleanup, renderHook } = await import("@testing-library/react");
  const { useRoleAuthorityLive } = await import("./useRoleAuthorityLive.ts");
  const channels = Array.from(
    { length: 129 },
    (_unused, index) => `channel-${String(index).padStart(3, "0")}`,
  );
  const subscriptions = [];
  const client = {
    async subscribeLive(filter, onEvent) {
      const subscription = { filter, onEvent, disposed: 0 };
      subscriptions.push(subscription);
      return () => {
        subscription.disposed += 1;
      };
    },
    subscribeToReconnects: () => () => {},
  };
  const evidenceCalls = [];

  try {
    const mounted = renderHook(
      ({ onEvidence, relayUrl }) =>
        useRoleAuthorityLive(
          {
            relayUrl,
            channelIds: [...channels].reverse(),
            enabled: true,
            onEvidence,
          },
          client,
        ),
      {
        initialProps: {
          onEvidence: () => evidenceCalls.push("initial"),
          relayUrl: "wss://relay-a",
        },
      },
    );
    await settle(act);

    assert.equal(mounted.result.current, null);
    assert.equal(subscriptions.length, 2);
    assert.deepEqual(subscriptions[0].filter.kinds, [44228, 40099]);
    assert.equal(subscriptions[0].filter["#h"].length, 128);
    assert.deepEqual(subscriptions[1].filter["#h"], ["channel-128"]);
    assert.equal(subscriptions[0].filter.limit, 0);
    assert.deepEqual(evidenceCalls, ["initial"]);

    mounted.rerender({
      onEvidence: () => evidenceCalls.push("latest"),
      relayUrl: "wss://relay-a",
    });
    await settle(act, 1);
    assert.equal(
      subscriptions.length,
      2,
      "callback changes do not resubscribe",
    );

    await act(async () => {
      subscriptions[0].onEvent(
        event({ id: "transition", kind: 44228, channel: "channel-000" }),
      );
      subscriptions[0].onEvent(
        event({
          id: "receipt",
          kind: 40099,
          channel: "channel-001",
          content: JSON.stringify({
            type: "coding_session_authority_transition_accepted",
          }),
        }),
      );
    });
    assert.deepEqual(evidenceCalls.slice(-1), ["latest"]);

    const beforeIrrelevant = evidenceCalls.length;
    await act(async () => {
      subscriptions[0].onEvent(
        event({
          id: "join",
          kind: 40099,
          channel: "channel-001",
          content: JSON.stringify({ type: "member_joined" }),
        }),
      );
      subscriptions[0].onEvent(
        event({ id: "wrong-kind", kind: 1, channel: "channel-001" }),
      );
      subscriptions[0].onEvent(
        event({ id: "wrong-channel", kind: 44228, channel: "elsewhere" }),
      );
      subscriptions[0].onEvent(
        event({
          id: "bad-json",
          kind: 40099,
          channel: "channel-001",
          content: "{",
        }),
      );
    });
    assert.equal(evidenceCalls.length, beforeIrrelevant);

    mounted.rerender({
      onEvidence: () => evidenceCalls.push("latest"),
      relayUrl: "wss://relay-b",
    });
    await settle(act);
    assert.equal(subscriptions.length, 4);
    assert.ok(
      subscriptions
        .slice(0, 2)
        .every((subscription) => subscription.disposed === 1),
      "switching community disposes every old-scope subscription",
    );
    const afterCommunityBackfill = evidenceCalls.length;
    await act(async () => {
      subscriptions[0].onEvent(
        event({ id: "stale-scope", kind: 44228, channel: "channel-000" }),
      );
    });
    assert.equal(
      evidenceCalls.length,
      afterCommunityBackfill,
      "a callback retained by the old client cannot refresh the new community",
    );

    mounted.unmount();
    assert.ok(
      subscriptions.every((subscription) => subscription.disposed === 1),
    );
  } finally {
    cleanup();
  }
});

test("unmount disposes a late subscription and ignores its stale callback", async () => {
  const { act, cleanup, renderHook } = await import("@testing-library/react");
  const { useRoleAuthorityLive } = await import("./useRoleAuthorityLive.ts");
  let resolveSubscription;
  let liveCallback;
  let disposed = 0;
  let evidenceCalls = 0;
  const client = {
    subscribeLive: (_filter, onEvent) => {
      liveCallback = onEvent;
      return new Promise((resolve) => {
        resolveSubscription = resolve;
      });
    },
    subscribeToReconnects: () => () => {},
  };

  try {
    const mounted = renderHook(() =>
      useRoleAuthorityLive(
        {
          relayUrl: "wss://relay-a",
          channelIds: ["channel-a"],
          enabled: true,
          onEvidence: () => {
            evidenceCalls += 1;
          },
        },
        client,
      ),
    );
    mounted.unmount();
    await act(async () => {
      resolveSubscription(() => {
        disposed += 1;
      });
      await Promise.resolve();
    });
    liveCallback(event({ id: "late", kind: 44228, channel: "channel-a" }));

    assert.equal(disposed, 1);
    assert.equal(evidenceCalls, 0);
  } finally {
    cleanup();
  }
});

test("a failed subscription is readable and reconnect retries then backfills", async () => {
  const { act, cleanup, renderHook } = await import("@testing-library/react");
  const { useRoleAuthorityLive } = await import("./useRoleAuthorityLive.ts");
  let attempts = 0;
  let reconnect;
  let evidenceCalls = 0;
  const client = {
    async subscribeLive() {
      attempts += 1;
      if (attempts === 1) throw new Error("relay refused authority watch.");
      return () => {};
    },
    subscribeToReconnects(listener) {
      reconnect = listener;
      return () => {};
    },
  };

  try {
    const mounted = renderHook(() =>
      useRoleAuthorityLive(
        {
          relayUrl: "wss://relay-a",
          channelIds: ["channel-a"],
          enabled: true,
          onEvidence: () => {
            evidenceCalls += 1;
          },
        },
        client,
      ),
    );
    await settle(act);
    assert.match(mounted.result.current, /relay refused authority watch/);
    assert.match(mounted.result.current, /Reconnect or reopen Roles/);
    assert.equal(evidenceCalls, 0);

    await act(async () => reconnect());
    await settle(act);
    assert.equal(attempts, 2);
    assert.equal(mounted.result.current, null);
    assert.equal(evidenceCalls, 1);
    mounted.unmount();
  } finally {
    cleanup();
  }
});

test("reconnect re-backfills an established watcher without resubscribing", async () => {
  const { act, cleanup, renderHook } = await import("@testing-library/react");
  const { useRoleAuthorityLive } = await import("./useRoleAuthorityLive.ts");
  let subscriptions = 0;
  let reconnect;
  let reconnectDisposed = 0;
  let evidenceCalls = 0;
  const client = {
    async subscribeLive() {
      subscriptions += 1;
      return () => {};
    },
    subscribeToReconnects(listener) {
      reconnect = listener;
      return () => {
        reconnectDisposed += 1;
      };
    },
  };

  try {
    const mounted = renderHook(() =>
      useRoleAuthorityLive(
        {
          relayUrl: "wss://relay-a",
          channelIds: ["channel-a"],
          enabled: true,
          onEvidence: () => {
            evidenceCalls += 1;
          },
        },
        client,
      ),
    );
    await settle(act);
    assert.equal(evidenceCalls, 1);

    await act(async () => reconnect());
    await settle(act, 1);
    assert.equal(subscriptions, 1);
    assert.equal(evidenceCalls, 2);

    mounted.unmount();
    assert.equal(reconnectDisposed, 1);
  } finally {
    cleanup();
  }
});
