/**
 * SV-116 through the REAL hook: a session with more transcript events than
 * one relay page opens on its newest page (no loading screen behind the older
 * ones), says "loading earlier" while the rest arrives, and ends holding every
 * event — each one still signature-checked by the real store.
 *
 * Measured before this: tank-loop opened with 1000 of 4375 transcript events
 * and nothing said so (ledger 347).
 */
import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

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
  ipcHandlers.set("get_global_agent_config", async () => ({
    env_vars: {},
    provider: null,
    model: null,
    preferred_runtime: null,
    "allowed-bridge-pubkeys": [],
  }));
});

beforeEach(async () => {
  const { resetCodingSessionIngressStores } = await import(
    "./codingSessionIngressStoreCache.ts"
  );
  resetCodingSessionIngressStores();
});

after(() => dom.window.close());

const CHANNEL_ID = "channel-paging";
const PROVIDER_SECRET = generateSecretKey();
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const TRANSCRIPT_COUNT = 1_150;
const PAGE_CEILING = 1_000;
// A wall-clock bound, not a round count: every event here is checked by the
// JavaScript verifier (no native side under node), ~1.5 ms each locally and
// several times that on a loaded CI runner. A 400-round bound used 158 rounds
// on an idle laptop and ran out in the gate (pipeline 278).
const SETTLE_DEADLINE_MS = 60_000;

async function signedTranscripts(count) {
  const { KIND_CODING_SESSION_TRANSCRIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionTargetKey } = await import(
    "./codingSessionCommand.ts"
  );
  const {
    BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
    CODING_SESSION_TRANSCRIPT_TAG_VERSION,
    codingSessionTranscriptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  return Array.from({ length: count }, (_, index) => {
    const seq = index + 1;
    // Two events per second, so pages straddle a shared second.
    const createdAt = 1_800_000_000 + Math.floor(index / 2);
    return finalizeEvent(
      {
        kind: KIND_CODING_SESSION_TRANSCRIPT,
        created_at: createdAt,
        tags: [
          ["h", CHANNEL_ID],
          ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
          ["cs-target", buildCodingSessionTargetKey(TARGET)],
          ["cst-seq", String(seq)],
          ["cst-key", codingSessionTranscriptSemanticKey(TARGET, seq)],
        ],
        content: JSON.stringify({
          schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
          session: TARGET,
          eventSeq: seq,
          timestamp: createdAt * 1000,
          turnId: null,
          item: { kind: "status", status: "running" },
        }),
      },
      PROVIDER_SECRET,
    );
  });
}

/** beekeeper-relay's contract for these kinds: DESC order, inclusive bounds, clamp. */
function relayAnswer(events, filter) {
  return events
    .filter(
      (event) =>
        filter.kinds.includes(event.kind) &&
        (filter.until === undefined || event.created_at <= filter.until) &&
        (filter.since === undefined || event.created_at >= filter.since),
    )
    .sort((a, b) =>
      b.created_at !== a.created_at
        ? b.created_at - a.created_at
        : a.id < b.id
          ? -1
          : 1,
    )
    .slice(0, Math.min(filter.limit, PAGE_CEILING));
}

test("a session longer than one page renders its newest page, discloses the rest, then holds every event", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { useTrustedCodingSessionIngress } = await import(
    "./useTrustedCodingSessionIngress.ts"
  );
  const events = await signedTranscripts(TRANSCRIPT_COUNT);

  const historyCalls = [];
  let releaseOlder = () => {};
  const olderGate = new Promise((resolve) => {
    releaseOlder = resolve;
  });
  const client = {
    fetchEvents: async (filter) => {
      historyCalls.push(filter);
      // Hold every older page until the test has looked at the first paint.
      if (filter.until !== undefined) await olderGate;
      return relayAnswer(events, filter);
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const settleUntil = async (predicate, what) => {
    const deadline = Date.now() + SETTLE_DEADLINE_MS;
    while (Date.now() < deadline) {
      if (predicate()) return;
      await act(async () => {
        await new Promise((resolve) => nodeSetTimeout(resolve, 2));
      });
    }
    assert.fail(`timed out waiting for ${what}`);
  };

  const { result, unmount } = renderHook(
    () =>
      useTrustedCodingSessionIngress(
        [CHANNEL_ID],
        null,
        null,
        client,
        null,
        "open",
      ),
    { wrapper },
  );

  await settleUntil(
    () =>
      !result.current.isLoading &&
      result.current.historyCompleteness.state === "loading-earlier",
    "the newest page to render with older pages still loading",
  );
  assert.equal(
    result.current.transcripts.length,
    PAGE_CEILING,
    "the newest page renders before older pages arrive",
  );
  assert.equal(result.current.invalidSignatureCount, 0);

  await act(async () => {
    releaseOlder();
  });
  await settleUntil(
    () => result.current.historyCompleteness.state === "complete",
    "paging to finish",
  );
  assert.equal(result.current.transcripts.length, TRANSCRIPT_COUNT);
  assert.equal(result.current.invalidSignatureCount, 0);
  const seqs = new Set(
    result.current.transcripts.map((entry) => entry.transcript.eventSeq),
  );
  assert.equal(seqs.size, TRANSCRIPT_COUNT, "no event is missing or doubled");

  const older = historyCalls.filter((filter) => filter.until !== undefined);
  assert.ok(older.length >= 1, "older history was paged with until");
  for (const filter of older) {
    assert.deepEqual(filter.kinds, [44225], "only the full kind pages back");
  }
  unmount();
});

test("a forged event in an older page is still refused — paging grants nothing", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { useTrustedCodingSessionIngress } = await import(
    "./useTrustedCodingSessionIngress.ts"
  );
  const events = await signedTranscripts(1_010);
  // Tamper with the oldest event: same id, altered content, so its signature
  // no longer covers what it says.
  const oldest = events[0];
  events[0] = { ...oldest, content: oldest.content.replace("running", "idle") };

  const client = {
    fetchEvents: async (filter) => relayAnswer(events, filter),
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const { result, unmount } = renderHook(
    () =>
      useTrustedCodingSessionIngress(
        [CHANNEL_ID],
        null,
        null,
        client,
        null,
        "open",
      ),
    { wrapper },
  );
  const deadline = Date.now() + SETTLE_DEADLINE_MS;
  while (Date.now() < deadline) {
    if (result.current.historyCompleteness.state === "complete") break;
    await act(async () => {
      await new Promise((resolve) => nodeSetTimeout(resolve, 2));
    });
  }
  assert.equal(result.current.historyCompleteness.state, "complete");
  assert.equal(result.current.transcripts.length, 1_009);
  assert.ok(
    result.current.invalidSignatureCount + result.current.malformedCount >= 1,
  );
  unmount();
});
