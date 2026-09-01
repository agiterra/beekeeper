/**
 * The lead's 44220/44224 evidence plane: one history fetch and one live
 * subscription per kind, scoped by `#cs-target` to the exact lead generation.
 *
 * There is no timer here on purpose. The relay already pushes these events;
 * a poll would only add latency and cost, and D7 forbids one.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "../lib/codingSessionCommand.ts";
import {
  CODING_SESSION_LEAD_WAKE_EVIDENCE_HISTORY_LIMIT,
  buildCodingSessionLeadWakeEvidenceFilters,
  useCodingSessionLeadWakeEvidence,
} from "./useCodingSessionLeadWakeEvidence.ts";

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

const CHANNEL = "f4829942-15a8-4e74-accd-51c8448f250f";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const LEAD_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-primary",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const LEAD_TARGET_KEY = buildCodingSessionTargetKey(LEAD_TARGET);
const POINTER = JSON.stringify({
  operationId: "e".repeat(64),
  type: "report",
});

function command(commandId) {
  return finalizeEvent(
    {
      kind: 44220,
      created_at: 1,
      tags: [
        ["h", CHANNEL],
        ["cs-v", "csc1-1"],
        ["cs-target", LEAD_TARGET_KEY],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-command/v1",
        commandId,
        target: LEAD_TARGET,
        action: { type: "thread.turn.start", text: POINTER },
      }),
    },
    PROVIDER_SECRET,
  );
}

function queuedReceipt(commandId) {
  return finalizeEvent(
    {
      kind: 44224,
      created_at: 2,
      tags: [
        ["h", CHANNEL],
        ["cs-target", LEAD_TARGET_KEY],
        ["csl-command", commandId],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lifecycle-receipt/v1",
        commandId,
        status: "turn_queued",
        session: LEAD_TARGET,
        error: null,
      }),
    },
    PROVIDER_SECRET,
  );
}

function clientFor(history, { failFetch = false } = {}) {
  const fetches = [];
  const subscriptions = [];
  return {
    fetches,
    subscriptions,
    fetchEvents: async (filter) => {
      fetches.push(filter);
      if (failFetch) throw new Error("relay refused the wake evidence query");
      return history.filter((event) => filter.kinds.includes(event.kind));
    },
    subscribeLive: async (filter, onEvent) => {
      const subscription = { filter, onEvent, closed: false };
      subscriptions.push(subscription);
      return () => {
        subscription.closed = true;
      };
    },
  };
}

async function harness() {
  const { act, renderHook } = await import("@testing-library/react");
  const settleUntil = async (predicate, label) => {
    for (let round = 0; round < 300; round += 1) {
      if (predicate()) return;
      await act(async () => new Promise((resolve) => setTimeout(resolve, 1)));
    }
    throw new Error(`timed out waiting for ${label}`);
  };
  return { act, renderHook, settleUntil };
}

const SCOPE = {
  channelId: CHANNEL,
  leadTargetKey: LEAD_TARGET_KEY,
  providerAuthorityPubkey: PROVIDER_PUBKEY,
};

test("D2: filters are exactly the two lead-scoped kinds, history then live", async () => {
  const client = clientFor([
    command("team-wake-1"),
    queuedReceipt("team-wake-1"),
  ]);
  const { renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionLeadWakeEvidence(SCOPE, client),
  );
  await settleUntil(
    () => !mounted.result.current.isLoading && client.fetches.length === 2,
    "wake evidence history",
  );
  assert.deepEqual(
    client.fetches,
    buildCodingSessionLeadWakeEvidenceFilters(
      SCOPE,
      CODING_SESSION_LEAD_WAKE_EVIDENCE_HISTORY_LIMIT,
    ),
  );
  assert.deepEqual(client.fetches[0], {
    kinds: [44220],
    "#h": [CHANNEL],
    "#cs-target": [LEAD_TARGET_KEY],
    limit: CODING_SESSION_LEAD_WAKE_EVIDENCE_HISTORY_LIMIT,
  });
  assert.deepEqual(client.fetches[1].kinds, [44224]);
  assert.equal(client.subscriptions.length, 2);
  assert.equal(mounted.result.current.errorMessage, null);
  assert.deepEqual(
    mounted.result.current.index.commandsFor(POINTER).map((c) => c.commandId),
    ["team-wake-1"],
  );
  assert.deepEqual(mounted.result.current.index.progressFor("team-wake-1"), {
    stage: "queued",
  });
});

test("D2: a live event after history settles is folded into the same index", async () => {
  const client = clientFor([command("team-wake-1")]);
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionLeadWakeEvidence(SCOPE, client),
  );
  await settleUntil(
    () => !mounted.result.current.isLoading,
    "wake evidence history",
  );
  const before = mounted.result.current.revision;
  await act(async () => {
    for (const subscription of client.subscriptions) {
      subscription.onEvent(queuedReceipt("team-wake-1"));
    }
  });
  assert.ok(mounted.result.current.revision > before);
  assert.deepEqual(mounted.result.current.index.progressFor("team-wake-1"), {
    stage: "queued",
  });
});

test("D2/D-T7: a failed history query is an error, never an empty index", async () => {
  const client = clientFor([], { failFetch: true });
  const { renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionLeadWakeEvidence(SCOPE, client),
  );
  await settleUntil(
    () => mounted.result.current.errorMessage !== null,
    "wake evidence failure",
  );
  assert.match(mounted.result.current.errorMessage, /relay refused/);
  assert.equal(mounted.result.current.index, null);
  assert.equal(mounted.result.current.isLoading, false);
});

test("D2: no lead scope means no query and no index", async () => {
  const client = clientFor([]);
  const { renderHook } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionLeadWakeEvidence(
      {
        channelId: CHANNEL,
        leadTargetKey: null,
        providerAuthorityPubkey: null,
      },
      client,
    ),
  );
  assert.equal(client.fetches.length, 0);
  assert.equal(client.subscriptions.length, 0);
  assert.equal(mounted.result.current.index, null);
  assert.equal(mounted.result.current.isLoading, false);
});

test("D2: re-rendering with the same scope never re-subscribes", async () => {
  const client = clientFor([command("team-wake-1")]);
  const { renderHook, settleUntil } = await harness();
  const mounted = renderHook(
    ({ scope }) => useCodingSessionLeadWakeEvidence(scope, client),
    { initialProps: { scope: { ...SCOPE } } },
  );
  await settleUntil(
    () => !mounted.result.current.isLoading,
    "wake evidence history",
  );
  mounted.rerender({ scope: { ...SCOPE } });
  await settleUntil(
    () => !mounted.result.current.isLoading,
    "wake evidence steady state",
  );
  assert.equal(client.subscriptions.length, 2);
  assert.equal(client.fetches.length, 2);
});
