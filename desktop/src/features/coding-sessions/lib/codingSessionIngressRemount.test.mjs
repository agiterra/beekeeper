/**
 * Leave-and-come-back regression for the two coding-session subscriptions.
 *
 * A session view mounts both ingress hooks: `useTrustedCodingSessionIngress`
 * (provider-signed 44222/44224/44225, narrowed by the trust allowlist) and
 * `useCodingSessionCreateObservations` (human-signed 44221 creates joined to
 * their receipts, deliberately unfiltered by author). Navigating away unmounts
 * both and drops their stores; navigating back must rebuild them from scratch.
 *
 * Two things have to hold on that remount, and the second is what broke:
 *
 * 1. History is refetched and both live subscriptions are re-armed.
 * 2. The rebuild survives relay back-pressure. Re-entering a session fires a
 *    burst of REQ frames at once, and the relay's per-pubkey WebSocket
 *    admission budget (`human_ws_events_per_sec` × a 5s burst window) closes
 *    the losers with one of three `rate-limited:` reasons. The discovery
 *    controller recognised only `quota exceeded` and treated the other two as
 *    fatal, so the catalog settled at "no entries, not loading" — which
 *    `resolveCodingSessionWorkspace` renders as "Generation not found" for a
 *    session that exists and is still streaming.
 */
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

const ipcHandlers = new Map();
const tauriInternals = {
  invoke: (cmd, args) => {
    const handler = ipcHandlers.get(cmd);
    if (handler) return handler(args);
    return Promise.reject(new Error(`unmocked Tauri command: ${cmd}`));
  },
  transformCallback: () => Math.random(),
};

// Production code schedules retries on `window.setTimeout`; the harness uses
// Node's global `setTimeout`, which is untouched. Compressing only the window
// timer lets a multi-second backoff ladder run inside one test.
const nodeSetTimeout = setTimeout;

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  dom.window.setTimeout = (callback, delayMs) =>
    nodeSetTimeout(callback, Math.min(Number(delayMs) || 0, 1));
  dom.window.clearTimeout = (timer) => clearTimeout(timer);
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "channel-remount";
const COMMAND_ID = "csl-remount-1";
const SESSION_REF = "9c4e2b10-3f5a-4d72-8b16-2e9a7d4c1f08";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OPERATOR_SECRET = generateSecretKey();

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "22222222-3333-4444-5555-666666666666",
  generation: 1,
};

const TRUSTED_CONFIG = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  "allowed-bridge-pubkeys": [
    { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
  ],
};

async function buildRelayHistory() {
  const {
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA,
  } = await import("@/shared/constants/kinds.ts");
  const { buildCodingSessionTargetKey } = await import(
    "./codingSessionCommand.ts"
  );
  const { buildCodingSessionCreateEvent } = await import(
    "./codingSessionLifecycleCommand.ts"
  );
  const {
    BUZZ_CODING_SESSION_METADATA_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    CODING_SESSION_METADATA_TAG_VERSION,
    codingSessionMetadataSemanticKey,
    lifecycleReceiptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");

  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: null,
    title: "Umbrella session",
    initialTurn: null,
  });
  const createEvent = finalizeEvent(
    {
      kind: built.kind,
      created_at: 1_800_000_000,
      tags: built.tags,
      content: built.content,
    },
    OPERATOR_SECRET,
  );
  const receiptEvent = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: 1_800_000_005,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
    },
    PROVIDER_SECRET,
  );
  const metadataEvent = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: 1_800_000_006,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        sessionRef: SESSION_REF,
        title: "Umbrella session",
        agentRef: null,
        provider: "claude-primary",
        runtime: "claude",
        model: "claude-opus-5",
        status: "running",
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: false,
          context: false,
          diff: false,
          plan: true,
        },
      }),
    },
    PROVIDER_SECRET,
  );

  return { createEvent, metadataEvent, receiptEvent };
}

/** Both hooks under one root, exactly as `useCodingSessionCatalog` mounts them. */
async function loadHooks() {
  const { useCodingSessionCreateObservations } = await import(
    "./useCodingSessionCreateObservations.ts"
  );
  const { useTrustedCodingSessionIngress } = await import(
    "./useTrustedCodingSessionIngress.ts"
  );
  return (client) => ({
    creates: useCodingSessionCreateObservations([CHANNEL_ID], client),
    trusted: useTrustedCodingSessionIngress(
      [CHANNEL_ID],
      null,
      null,
      client,
      null,
    ),
  });
}

async function reactHarness() {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const settle = async (rounds = 8) => {
    for (let round = 0; round < rounds; round += 1) {
      await act(async () => {
        await new Promise((resolve) => nodeSetTimeout(resolve, 1));
      });
    }
  };
  return { act, queryClient, renderHook, settle, wrapper };
}

test("both ingress subscriptions rebuild when the session view is re-entered", async () => {
  const { createEvent, metadataEvent, receiptEvent } =
    await buildRelayHistory();
  const useBothIngressHooks = await loadHooks();
  const { queryClient, renderHook, settle, wrapper } = await reactHarness();

  ipcHandlers.set("get_global_agent_config", async () => TRUSTED_CONFIG);

  const historyCalls = [];
  const liveSubscriptions = [];
  const client = {
    fetchEvents: async (filter) => {
      historyCalls.push(filter);
      return [createEvent, receiptEvent, metadataEvent];
    },
    subscribeLive: async (filter, onEvent) => {
      const subscription = { filter, onEvent, closed: false };
      liveSubscriptions.push(subscription);
      return () => {
        subscription.closed = true;
      };
    },
    subscribeToReconnects: () => () => {},
  };

  const first = renderHook(() => useBothIngressHooks(client), { wrapper });
  await settle();

  assert.equal(historyCalls.length, 2, "one history fetch per hook");
  assert.equal(liveSubscriptions.length, 2, "one live subscription per hook");
  assert.equal(first.result.current.trusted.metadata.length, 1);
  assert.equal(first.result.current.creates.observations.length, 1);

  // Navigate away: both subscriptions must actually be released, or the
  // relay accumulates a subscription per visit.
  first.unmount();
  assert.deepEqual(
    liveSubscriptions.map((subscription) => subscription.closed),
    [true, true],
    "both live subscriptions must close on unmount",
  );

  // ...and back. Fresh hook instances, fresh stores, nothing carried over.
  const second = renderHook(() => useBothIngressHooks(client), { wrapper });
  await settle();

  assert.equal(historyCalls.length, 4, "history must be refetched on remount");
  assert.equal(liveSubscriptions.length, 4, "live subs must be re-armed");
  assert.equal(
    liveSubscriptions.filter((subscription) => !subscription.closed).length,
    2,
  );
  assert.equal(second.result.current.trusted.metadata.length, 1);
  assert.equal(second.result.current.creates.observations.length, 1);

  // The re-armed live subscriptions actually feed the rebuilt stores.
  const { KIND_CODING_SESSION_TRANSCRIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionTargetKey } = await import(
    "./codingSessionCommand.ts"
  );
  const {
    BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    CODING_SESSION_TRANSCRIPT_TAG_VERSION,
    codingSessionTranscriptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  const transcriptEvent = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: 1_800_000_010,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["cst-seq", "1"],
        ["cst-key", codingSessionTranscriptSemanticKey(TARGET, 1)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: TARGET,
        eventSeq: 1,
        timestamp: 1_800_000_010_000,
        turnId: null,
        item: { kind: "status", status: "running" },
      }),
    },
    PROVIDER_SECRET,
  );
  const { act } = await import("@testing-library/react");
  await act(async () => {
    for (const subscription of liveSubscriptions.slice(2)) {
      subscription.onEvent(transcriptEvent);
    }
  });
  assert.equal(second.result.current.trusted.transcripts.length, 1);

  second.unmount();
  queryClient.clear();
  ipcHandlers.clear();
});

test("a back-pressure CLOSED on the re-entry history REQ converges instead of latching", async () => {
  const { createEvent, metadataEvent, receiptEvent } =
    await buildRelayHistory();
  const useBothIngressHooks = await loadHooks();
  const { queryClient, renderHook, settle, wrapper } = await reactHarness();

  ipcHandlers.set("get_global_agent_config", async () => TRUSTED_CONFIG);

  // The two reasons the relay emits that carry no `retry in Ns` hint. Both
  // were misclassified as fatal, so a single unlucky re-entry stranded the
  // workspace at "Generation not found" until the socket happened to drop.
  const backPressure = [
    "rate-limited: too many concurrent requests",
    "rate-limited: shared admission unavailable",
  ];
  const attemptsByFilterKind = new Map();
  const client = {
    fetchEvents: async (filter) => {
      const key = filter.kinds.join(",");
      const attempt = (attemptsByFilterKind.get(key) ?? 0) + 1;
      attemptsByFilterKind.set(key, attempt);
      // Five straight rejections — comfortably past the controller's bounded
      // transport budget of three.
      if (attempt <= 5) {
        throw new Error(backPressure[attempt % backPressure.length]);
      }
      return [createEvent, receiptEvent, metadataEvent];
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };

  const { result, unmount } = renderHook(() => useBothIngressHooks(client), {
    wrapper,
  });
  // One round is enough to land the first rejection.
  await settle(1);

  // At this observation point the retry may still be backing off, or a busy
  // suite may have already let the compressed retry ladder converge. Either
  // state is valid; a resolved empty catalog is not, because the workspace
  // turns that into "Generation not found".
  assert.equal(
    result.current.trusted.isLoading ||
      result.current.trusted.metadata.length === 1,
    true,
  );
  assert.equal(result.current.trusted.errorMessage, null);
  assert.equal(
    result.current.creates.isLoading ||
      result.current.creates.observations.length === 1,
    true,
  );

  await settle(40);

  assert.ok(
    attemptsByFilterKind.get("44223,44224,44225") >= 6,
    `trusted ingress must retry past the bounded transport budget (attempts: ${JSON.stringify([...attemptsByFilterKind])})`,
  );
  assert.ok(
    attemptsByFilterKind.get("44221,44224,44226") >= 6,
    "create observations must retry past the bounded transport budget",
  );
  assert.equal(result.current.trusted.errorMessage, null);
  assert.equal(result.current.trusted.isLoading, false);
  assert.equal(result.current.trusted.metadata.length, 1);
  assert.equal(result.current.creates.observations.length, 1);

  unmount();
  queryClient.clear();
  ipcHandlers.clear();
});

test("the umbrella conversation lane rides out the same back-pressure", async () => {
  const { useCodingSessionLane } = await import("../useCodingSessionLane.ts");
  const { KIND_STREAM_MESSAGE } = await import("@/shared/constants/kinds.ts");
  const { queryClient, renderHook, settle, wrapper } = await reactHarness();

  const laneMessage = finalizeEvent(
    {
      kind: KIND_STREAM_MESSAGE,
      created_at: 1_800_000_020,
      tags: [
        ["h", CHANNEL_ID],
        ["cs-session", SESSION_REF],
      ],
      content: "ship it",
    },
    OPERATOR_SECRET,
  );

  let attempts = 0;
  const client = {
    fetchEvents: async () => {
      attempts += 1;
      if (attempts <= 5) {
        throw new Error("rate-limited: too many concurrent requests");
      }
      return [laneMessage];
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };

  const { result, unmount } = renderHook(
    () => useCodingSessionLane(CHANNEL_ID, SESSION_REF, client),
    { wrapper },
  );
  await settle(40);

  // Before the fix the lane had no retry at all: one rejected history frame
  // left the umbrella's chat pane permanently empty until the next reconnect.
  assert.ok(attempts >= 6, `lane history must retry (attempts: ${attempts})`);
  assert.equal(result.current.errorMessage, null);
  assert.equal(result.current.isLoading, false);
  assert.equal(result.current.messages.length, 1);
  assert.equal(result.current.messages[0].content, "ship it");

  unmount();
  queryClient.clear();
});
