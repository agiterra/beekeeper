import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

// =============================================================================
// Regression — a live broadcast reply must enter the authoritative channel
// window store (PR #1500 review, blocker 2)
// =============================================================================
//
// NIP-CW §Top-level Classification: a depth-1 reply carrying `["broadcast","1"]`
// is a channel-window row — it belongs on the timeline as well as in its thread.
// The relay serves it as a row (`buzz-db/src/thread.rs`: "top-level rows =
// depth 0, missing metadata, or broadcast depth-1 replies").
//
// The client's live-append path drops it from the WINDOW STORE. `appendMessage`
// (hooks.ts) routes every parented timeline event into the thread cache and
// returns BEFORE the window-store merge, gating only on `parentId !== null`
// with no broadcast check. So a live broadcast reply never reaches the
// `channel-window` store's `liveOverlay` — it survives on the timeline only via
// the incidental `useLiveChannelUpdates` write to `channel-messages`, which any
// window-store rebuild (page-zero refresh, later top-level append) erases.
//
// We assert the durable invariant — the broadcast reply IS in the window
// store's `liveOverlay` — rather than a DOM row, because the timeline render is
// masked by the second (unfiltered) subscriber. The window store is the
// authoritative source `flattenChannelWindowEvents` rebuilds from.
//
// RED at f2a551f2 (appendMessage returns before the overlay merge → liveOverlay
// omits the broadcast reply). GREEN on wren/review-live-window-fixes @ 9a533a9e
// (broadcast replies fall through into `mergeLiveChannelWindowEvent`).
//
// An ordinary (non-broadcast) depth-1 reply is emitted as a control: it is NOT
// a window row and MUST stay out of the overlay, so a naive "merge every
// parented event" fix can't false-green this spec.

const CHANNEL = "general";

// Observe completion at the IPC boundary, after the mock installed the exact
// channel-window subscription. Any channel/global subscription (even kind 9)
// can belong to unread tracking rather than appendMessage's window store.
async function observeChannelWindowSubscription(
  page: import("@playwright/test").Page,
  hold = false,
) {
  const channelId = await page
    .getByTestId(`channel-${CHANNEL}`)
    .getAttribute("data-channel-id");
  if (!channelId) throw new Error("mock channel row has no channel id");
  await page.evaluate(
    ({ channelId, hold }) => {
      const target = window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string, payload?: unknown) => Promise<unknown>;
        };
        __WINDOW_SUBSCRIPTION_PROBE__?: {
          requested: boolean;
          ready: boolean;
          release: () => void;
        };
      };
      let release = () => {};
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      const probe = { requested: false, ready: false, release };
      target.__WINDOW_SUBSCRIPTION_PROBE__ = probe;
      const invoke = target.__TAURI_INTERNALS__.invoke;
      target.__TAURI_INTERNALS__.invoke = async (command, payload) => {
        const message = (
          payload as { message?: { type?: string; data?: string } } | undefined
        )?.message;
        let isWindowRequest = false;
        if (
          command === "plugin:websocket|send" &&
          message?.type === "Text" &&
          message.data
        ) {
          const [type, id, ...filters] = JSON.parse(message.data) as [
            string,
            string,
            ...Array<{
              kinds?: number[];
              "#h"?: string[];
              "#e"?: string[];
            }>,
          ];
          isWindowRequest =
            type === "REQ" &&
            id.startsWith("live-") &&
            filters.length === 1 &&
            filters[0].kinds?.includes(9) === true &&
            filters[0].kinds?.includes(39005) === true &&
            filters[0]["#h"]?.length === 1 &&
            filters[0]["#h"][0] === channelId &&
            !filters[0]["#e"];
        }
        if (isWindowRequest) {
          probe.requested = true;
          if (hold) await held;
        }
        const result = await invoke(command, payload);
        // Mock live REQ installs the subscription and emits EOSE before its
        // invoke resolves. Do not announce readiness merely on invocation.
        if (isWindowRequest) probe.ready = true;
        return result;
      };
    },
    { channelId, hold },
  );
}

async function waitForChannelWindow(
  page: import("@playwright/test").Page,
  phase: "requested" | "ready" = "ready",
) {
  await expect
    .poll(
      () =>
        page.evaluate(
          (phase) =>
            (
              window as typeof window & {
                __WINDOW_SUBSCRIPTION_PROBE__?: {
                  requested: boolean;
                  ready: boolean;
                };
              }
            ).__WINDOW_SUBSCRIPTION_PROBE__?.[phase] ?? false,
          phase,
        ),
      // Actual per-device admission remains enabled (17 reads / 5 seconds).
      // Wait for this relevant REQ, not every background startup request.
      { timeout: 20_000 },
    )
    .toBe(true);
}

async function emit(
  page: import("@playwright/test").Page,
  input: {
    content: string;
    parentEventId?: string | null;
    createdAt?: number;
    extraTags?: string[][];
  },
) {
  const event = await page.evaluate(
    (payload) =>
      window.__BUZZ_E2E_EMIT_MOCK_MESSAGE__?.({
        channelName: payload.channel,
        content: payload.content,
        parentEventId: payload.parentEventId,
        createdAt: payload.createdAt,
        extraTags: payload.extraTags,
      }),
    {
      channel: CHANNEL,
      content: input.content,
      parentEventId: input.parentEventId ?? null,
      createdAt: input.createdAt,
      extraTags: input.extraTags,
    },
  );
  if (!event) throw new Error("mock message emitter is not installed");
  return event;
}

async function liveOverlayContents(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const qc = window.__BUZZ_E2E_QUERY_CLIENT__ as unknown as {
      getQueriesData: (f: unknown) => Array<[readonly unknown[], unknown]>;
    };
    const win = qc
      .getQueriesData({ queryKey: [] })
      .find(([key]) => JSON.stringify(key).includes("channel-window"));
    const store = win?.[1] as
      | { liveOverlay?: Array<{ content?: string }> }
      | undefined;
    return (store?.liveOverlay ?? []).map((event) => event.content ?? "");
  });
}

test("a live broadcast depth-1 reply enters the authoritative channel window store", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/");
  await page.waitForFunction(
    () => typeof window.__BUZZ_E2E_EMIT_MOCK_MESSAGE__ === "function",
  );

  // Seed a top-level root into the cold window before opening the channel, so
  // there is a thread for the broadcast reply to descend from.
  const now = Math.floor(Date.now() / 1000);
  const root = await emit(page, { content: "timeline root", createdAt: now });

  await observeChannelWindowSubscription(page);
  await page.getByTestId("channel-general").click();
  await expect(page.getByTestId("chat-title")).toHaveText(CHANNEL);
  await expect(
    page.getByTestId("message-timeline").getByText("timeline root"),
  ).toBeVisible();

  // The live subscription must be established before we emit, or the event is
  // delivered before appendMessage is listening — that would be a cold-load
  // test, not a live-append test.
  await waitForChannelWindow(page);

  // LIVE broadcast depth-1 reply: parent is the root, carries ["broadcast","1"].
  await emit(page, {
    content: "broadcast to the channel",
    parentEventId: root.id,
    createdAt: now + 1,
    extraTags: [["broadcast", "1"]],
  });

  // CONTROL — an ordinary (non-broadcast) depth-1 reply: NOT a window row, MUST
  // stay out of the overlay.
  await emit(page, {
    content: "ordinary thread reply",
    parentEventId: root.id,
    createdAt: now + 2,
  });

  // The broadcast reply must land in the authoritative window-store overlay via
  // live append alone — the invariant that survives any window-store rebuild.
  await expect
    .poll(() => liveOverlayContents(page))
    .toContain("broadcast to the channel");

  // The ordinary reply is a thread reply, never a window row.
  expect(await liveOverlayContents(page)).not.toContain(
    "ordinary thread reply",
  );
});

test("a broadcast received before the channel window subscribes is recovered by catch-up", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/");
  await page.waitForFunction(
    () => typeof window.__BUZZ_E2E_EMIT_MOCK_MESSAGE__ === "function",
  );
  const root = await emit(page, { content: "catch-up root" });
  await observeChannelWindowSubscription(page, true);
  await page.getByTestId("channel-general").click();
  await expect(page.getByText("catch-up root", { exact: true })).toBeVisible();
  await waitForChannelWindow(page, "requested");

  // The exact window REQ is held before the mock sees it. This is a real
  // snapshot/subscription gap, regardless of how quickly startup completes.
  await emit(page, {
    content: "broadcast during subscription gap",
    parentEventId: root.id,
    extraTags: [["broadcast", "1"]],
  });
  await emit(page, {
    content: "ordinary reply during subscription gap",
    parentEventId: root.id,
  });
  expect(await liveOverlayContents(page)).not.toContain(
    "broadcast during subscription gap",
  );
  await page.evaluate(() => {
    (
      window as typeof window & {
        __WINDOW_SUBSCRIPTION_PROBE__?: { release: () => void };
      }
    ).__WINDOW_SUBSCRIPTION_PROBE__?.release();
  });
  await waitForChannelWindow(page);

  // Catch-up belongs in history pages, not the liveOverlay assertion above.
  // Verify the ordinary reply still stays out of the channel's top-level rows.
  const pageContents = () =>
    page.evaluate(() => {
      const client = window.__BUZZ_E2E_QUERY_CLIENT__ as unknown as {
        getQueriesData: (
          filter: unknown,
        ) => Array<[readonly unknown[], unknown]>;
      };
      const store = client
        .getQueriesData({ queryKey: [] })
        .find(([key]) => key[0] === "channel-window")?.[1] as
        | { pages?: Array<{ rows?: Array<{ event: { content: string } }> }> }
        | undefined;
      return (store?.pages ?? []).flatMap((page) =>
        (page.rows ?? []).map((row) => row.event.content),
      );
    });
  await expect
    .poll(pageContents)
    .toContain("broadcast during subscription gap");
  expect(await pageContents()).not.toContain(
    "ordinary reply during subscription gap",
  );
  await expect(
    page
      .getByTestId("message-timeline")
      .getByText("broadcast during subscription gap", { exact: true }),
  ).toBeVisible();
});
