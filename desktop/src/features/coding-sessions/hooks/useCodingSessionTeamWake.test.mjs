/**
 * The cleantest replay, driven through the real hook.
 *
 * On 2026-08-31 a provider wake was accepted and receipted `turn_queued` at
 * 19:13:59; Desktop's 15-second grace expired at 19:14:08 and published a
 * second command for the same operation; the lead spent two turns on one
 * report. The provider's `turn_started` existed on the wire nine seconds
 * later — Desktop simply never read a receipt.
 *
 * These tests mount the real hook against a real evidence index built from
 * really-signed 44220/44224 events, and assert the one thing that matters:
 * `publishCodingSessionCommand` is not called.
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
  buildCodingSessionTeamWakeCommandId,
  codingSessionTeamWakePublishFailed,
  codingSessionTeamWakeStorageKey,
  readCodingSessionTeamWakeState,
} from "../lib/codingSessionTeamWake.ts";
import { useCodingSessionTeamWake } from "./useCodingSessionTeamWake.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const ipcHandlers = new Map();
const tauriInternals = {
  invoke(command, args) {
    const handler = ipcHandlers.get(command);
    if (!handler) return Promise.reject(new Error(`unmocked: ${command}`));
    return handler(args);
  },
  transformCallback: () => Math.random(),
};

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

const CHANNEL = "f4829942-15a8-4e74-accd-51c8448f250f";
const COMMUNITY = "hive.agiterra.org";
const SESSION_REF = "683d55b3-d34e-410d-874a-55f9082d2631";
const GENESIS = "ab".repeat(32);
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const RELAY_SECRET = new Uint8Array(32).fill(2);
const BUILDER_SECRET = new Uint8Array(32).fill(3);
const PROVIDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const RELAY = getPublicKey(RELAY_SECRET);
const BUILDER = getPublicKey(BUILDER_SECRET);
const LEAD_ACTOR = getPublicKey(new Uint8Array(32).fill(5));
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const TRANSACTION_SCHEMA = "buzz-coding-session-team-transaction/v1";
const LEAD_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-primary",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const LEAD_TARGET_KEY = buildCodingSessionTargetKey(LEAD_TARGET);
const STORAGE_KEY = codingSessionTeamWakeStorageKey({
  communityScope: COMMUNITY,
  channelId: CHANNEL,
  sessionRef: SESSION_REF,
});

function transaction({ type, body, secret, createdAt }) {
  return finalizeEvent(
    {
      kind: 44244,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL],
        ["d", SESSION_REF],
        ["cstx-v", TRANSACTION_SCHEMA],
        ["cstx-genesis", GENESIS],
        ["cstx-type", type],
      ],
      content: JSON.stringify({
        schema: TRANSACTION_SCHEMA,
        sessionRef: SESSION_REF,
        genesisRef: GENESIS,
        type,
        supersedes: null,
        deliveryCommandId: null,
        body,
      }),
    },
    secret,
  );
}

const ASSIGNMENT = transaction({
  type: "assignment",
  secret: FOUNDER_SECRET,
  createdAt: 1_788_289_989,
  body: {
    assigneeActor: BUILDER,
    assigneeRole: "builder",
    objective: "Land the fence",
    brief: "Exact brief.",
    branch: null,
    baseSha: null,
    fileOwnership: [],
    acceptanceSteps: ["run focused tests"],
  },
});

const REPORT = transaction({
  type: "report",
  secret: BUILDER_SECRET,
  createdAt: 1_788_290_033,
  body: {
    assignmentRef: ASSIGNMENT.id,
    summary: "Fence landed.",
    branch: null,
    baseSha: null,
    headSha: null,
    files: [],
    tests: [],
    redBeforeGreen: null,
    deviations: [],
    residuals: [],
    anomalies: [],
  },
});

const REPORT_2 = transaction({
  type: "report",
  secret: BUILDER_SECRET,
  createdAt: 1_788_290_034,
  body: {
    assignmentRef: ASSIGNMENT.id,
    summary: "Second lane reported.",
    branch: null,
    baseSha: null,
    headSha: null,
    files: [],
    tests: [],
    redBeforeGreen: null,
    deviations: [],
    residuals: [],
    anomalies: [],
  },
});

const POINTER = JSON.stringify({ operationId: REPORT.id, type: "report" });
const POINTER_2 = JSON.stringify({ operationId: REPORT_2.id, type: "report" });

function wakeCommand({ commandId, secret = PROVIDER_SECRET, text = POINTER }) {
  return finalizeEvent(
    {
      kind: 44220,
      created_at: 1_788_290_039,
      tags: [
        ["h", CHANNEL],
        ["cs-v", "csc1-1"],
        ["cs-target", LEAD_TARGET_KEY],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-command/v1",
        commandId,
        target: LEAD_TARGET,
        action: { type: "thread.turn.start", text },
      }),
    },
    PROVIDER_SECRET === secret ? PROVIDER_SECRET : secret,
  );
}

function turnReceipt({ commandId, status, error = null, turnId = null }) {
  return finalizeEvent(
    {
      kind: 44224,
      created_at: 1_788_290_040,
      tags: [
        ["h", CHANNEL],
        ["cs-target", LEAD_TARGET_KEY],
        ["csl-command", commandId],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lifecycle-receipt/v1",
        commandId,
        status,
        session: LEAD_TARGET,
        error,
        ...(turnId ? { turnId } : {}),
      }),
    },
    PROVIDER_SECRET,
  );
}

function generation({ role, actor, target }) {
  return {
    generationId: role,
    label: role,
    title: role,
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    metadataAuthorityPubkey: PROVIDER_PUBKEY,
    lastEventAt: "2026-08-31T19:13:53.000Z",
    status: "idle",
    statusAt: 1_788_290_033_000,
    statusEventId: "8".repeat(64),
    transcript: [],
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: null,
    runtime: null,
    model: null,
    agentRef: actor,
    role,
    turnBudget: null,
    routing: null,
    capabilities: null,
  };
}

function umbrella() {
  return {
    umbrellaKey: SESSION_REF,
    sessionRef: SESSION_REF,
    title: "cleantest",
    executions: [
      {
        executionKey: "lead",
        signerPubkey: PROVIDER_PUBKEY,
        activeGeneration: generation({
          role: "lead",
          actor: LEAD_ACTOR,
          target: LEAD_TARGET,
        }),
        priorGenerations: [],
        operatorPubkey: FOUNDER,
      },
      {
        executionKey: "builder",
        signerPubkey: PROVIDER_PUBKEY,
        activeGeneration: generation({
          role: "builder",
          actor: BUILDER,
          target: {
            driver: "claude-acp",
            instanceId: "claude-primary",
            sessionId: "66666666-7777-8888-9999-aaaaaaaaaaaa",
            generation: 1,
          },
        }),
        priorGenerations: [],
        operatorPubkey: FOUNDER,
      },
    ],
    founderPubkey: FOUNDER,
    genesisRef: GENESIS,
    genesisResolution: "governed",
    status: "idle",
    lastEventAt: "2026-08-31T19:13:53.000Z",
    conflictCount: 0,
    foreignAttachmentCount: 0,
  };
}

function foldResponse(request) {
  return {
    schema: "buzz-coding-session-team-fold-adapter/v1",
    implementation: "buzz-core",
    inputEventIds: [...request.inputEventIds],
    context: {
      channelRef: request.context.channelRef,
      sessionRef: request.context.sessionRef,
      genesisRef: request.context.genesisRef,
      founderPubkey: request.context.founderPubkey,
      authorityHeadEventId: request.context.authorityHeadEventId,
      authorityHeadSeq: request.context.authorityHeadSeq,
      // L8.3: the adapter echoes the `verifierRequired` it was asked with, and
      // the decoder requires every key. A caller that read no policy asks with
      // `false`, so that is what a fixture asking with none is echoed.
      verifierRequired: false,
    },
    includedEventIds: [...request.inputEventIds],
    excluded: [],
    conflicts: [],
    assignments: [
      {
        assignmentEventId: ASSIGNMENT.id,
        governedReportEventId: REPORT.id,
        dispositionEventId: null,
        acknowledgementEventId: null,
        settled: false,
        awaiting: {
          link: "disposition",
          owedByRole: "lead",
          owedByActor: null,
        },
      },
    ],
    unseatedReports: [],
    notes: [],
    decisions: [],
    waitingOnDecision: null,
    pendingCompletion: null,
    canonicalTerminal: null,
  };
}

function clientFor(history, { wakeFetchFails = false } = {}) {
  const fetches = [];
  const subscriptions = [];
  return {
    fetches,
    subscriptions,
    fetchEvents: async (filter) => {
      fetches.push(filter);
      const isWakeFilter =
        filter.kinds.includes(44220) || filter.kinds.includes(44224);
      if (isWakeFilter && wakeFetchFails) {
        throw new Error("relay refused the wake evidence query");
      }
      return history.filter((event) => filter.kinds.includes(event.kind));
    },
    subscribeLive: async (filter, onEvent) => {
      const subscription = { filter, onEvent, closed: false };
      subscriptions.push(subscription);
      return () => {
        subscription.closed = true;
      };
    },
    // The mission-evidence hook reads through the bundled surface: one
    // `POST /query` for its three history filters and one REQ for its three
    // live filters. Delegate to the per-filter fakes above so every existing
    // assertion on `fetches`/`subscriptions` keeps its meaning.
    async fetchEventsBatch(filters) {
      const pages = await Promise.all(filters.map((f) => this.fetchEvents(f)));
      return pages.flat();
    },
    async subscribeLiveMany(filters, onEvent) {
      const closes = await Promise.all(
        filters.map((f) => this.subscribeLive(f, onEvent)),
      );
      return () => {
        for (const close of closes) close();
      };
    },
  };
}

function seedState({
  observedAtMs = null,
  observedSources = [REPORT.id],
  extra = {},
} = {}) {
  window.localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      schema: "buzz-coding-session-team-wake-state/v2",
      cursor: null,
      pending: [],
      observed:
        observedAtMs === null
          ? []
          : observedSources.map((sourceEventId) => ({
              sourceEventId,
              fallbackNotBeforeMs: observedAtMs,
            })),
      resolvedSourceEventIds: [],
      reArmed: [],
      custodied: [],
      publishFailed: [],
      ...extra,
    }),
  );
}

function readState() {
  return readCodingSessionTeamWakeState(window.localStorage, STORAGE_KEY);
}

async function harness() {
  const { act, renderHook } = await import("@testing-library/react");
  const settleUntil = async (predicate, label) => {
    for (let round = 0; round < 400; round += 1) {
      if (predicate()) return;
      await act(async () => new Promise((resolve) => setTimeout(resolve, 1)));
    }
    throw new Error(`timed out waiting for ${label}`);
  };
  const settleFor = async (rounds) => {
    for (let round = 0; round < rounds; round += 1) {
      await act(async () => new Promise((resolve) => setTimeout(resolve, 1)));
    }
  };
  return { act, renderHook, settleUntil, settleFor };
}

function install() {
  ipcHandlers.set("get_relay_self", async () => RELAY);
  ipcHandlers.set(
    "fold_coding_session_team_transactions",
    async ({ request }) => foldResponse(request),
  );
}

function mountInput() {
  return {
    catalogSettled: true,
    channelId: CHANNEL,
    communityScope: COMMUNITY,
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
    umbrella: umbrella(),
  };
}

function publisher() {
  const calls = [];
  return {
    calls,
    publishCommand: async (input) => {
      calls.push(input);
      return {
        eventId: "f".repeat(64),
        kind: 44220,
        commandId: input.commandId,
      };
    },
  };
}

test("D-T1: a queued provider wake stops the Desktop fallback and resolves durably", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const client = clientFor([
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
    turnReceipt({ commandId: "team-wake-2abd9f9a", status: "turn_queued" }),
  ]);
  const { publishCommand, calls } = publisher();
  const { renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => (mounted.result.current.deliveries ?? []).length > 0,
    "delivery rows",
  );
  await settleFor(20);
  assert.deepEqual(calls, [], "no Desktop fallback is published");
  assert.equal(mounted.result.current.deliveries[0].kind, "provider-queued");
  assert.equal(
    mounted.result.current.deliveries[0].owningCommandId,
    "team-wake-2abd9f9a",
  );
  assert.deepEqual(
    readState().resolvedSourceEventIds,
    [],
    "queued is custody, never permanent resolution",
  );
  assert.deepEqual(readState().custodied, [
    {
      sourceEventId: REPORT.id,
      leadTargetKey: LEAD_TARGET_KEY,
      commandId: "team-wake-2abd9f9a",
      fromProvider: true,
    },
  ]);
});

test("D-T2: Desktop's first cover uses the base id and spends no re-arm", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const stranger = generateSecretKey();
  const client = clientFor([
    ASSIGNMENT,
    REPORT,
    // A channel member with no operator grant publishes the same pointer. The
    // runner refuses it outright; it never held authority, so it must not
    // shape any decision of Desktop's.
    wakeCommand({ commandId: "stranger-cmd", secret: stranger }),
    turnReceipt({
      commandId: "stranger-cmd",
      status: "turn_refused",
      error: { code: "UNAUTHORIZED_OPERATOR", message: "not authorised" },
    }),
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
    turnReceipt({
      commandId: "team-wake-2abd9f9a",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const { publishCommand, calls } = publisher();
  const { renderHook, settleUntil, settleFor } = await harness();
  renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(() => calls.length === 1, "Desktop's first cover");
  await settleFor(20);
  assert.equal(calls.length, 1);
  assert.match(
    calls[0].commandId,
    /^team-wake-v1:[0-9a-f]{64}:[0-9a-f]{24}$/,
    "the first cover is the base id, never the one-shot re-arm",
  );
  assert.equal(calls[0].text, POINTER);
  assert.deepEqual(
    readState().reArmed,
    [],
    "the second chance is still intact after a stranger's refusal",
  );
});

test("F4: a spent Desktop command earns exactly one :r1, and then it fails", async () => {
  install();
  const baseId = await buildCodingSessionTeamWakeCommandId(
    { sourceEventId: REPORT.id, preferredCommandId: null },
    LEAD_TARGET,
  );
  seedState({
    observedAtMs: Date.now() - 60_000,
    extra: {
      pending: [
        {
          sourceEventId: REPORT.id,
          commandId: baseId,
          leadTargetKey: LEAD_TARGET_KEY,
          candidate: {
            sourceEventId: REPORT.id,
            sourceCreatedAtMs: 1_788_290_033_000,
            sourceEventSeq: null,
            sourceTargetKey: null,
            sourceActorPubkey: BUILDER,
            kind: "operation_ready",
            operationType: "report",
            seatRole: "builder",
            causedByCommandId: null,
            preferredCommandId: null,
          },
        },
      ],
    },
  });
  const client = clientFor([
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
    turnReceipt({
      commandId: "team-wake-2abd9f9a",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
    wakeCommand({ commandId: baseId, secret: FOUNDER_SECRET }),
    turnReceipt({
      commandId: baseId,
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const { publishCommand, calls } = publisher();
  const { act, renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(() => calls.length === 1, "the single re-arm publish");
  await settleFor(20);
  assert.equal(calls.length, 1, "exactly one Desktop re-arm, ever");
  assert.equal(calls[0].commandId, `${baseId}:r1`);
  assert.equal(readState().reArmed.length, 1);
  assert.equal(readState().reArmed[0].commandId, calls[0].commandId);

  await act(async () => {
    for (const subscription of client.subscriptions) {
      subscription.onEvent(
        turnReceipt({
          commandId: calls[0].commandId,
          status: "turn_dropped",
          error: { code: "NO_LIVE_EXECUTION", message: "the lead is gone" },
        }),
      );
    }
  });
  await settleFor(20);
  assert.equal(calls.length, 1, "a spent re-arm is never spent twice");
  assert.equal(mounted.result.current.deliveries[0].kind, "failed");
});

test("D-T3: a start arriving on the publish boundary aborts the publish", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const client = clientFor([
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
  ]);
  const { publishCommand, calls } = publisher();
  const realDigest = globalThis.crypto.subtle.digest.bind(
    globalThis.crypto.subtle,
  );
  let raced = false;
  globalThis.crypto.subtle.digest = async (...args) => {
    if (!raced) {
      raced = true;
      for (const subscription of client.subscriptions) {
        subscription.onEvent(
          turnReceipt({
            commandId: "team-wake-2abd9f9a",
            status: "turn_started",
            turnId: "turn-9",
          }),
        );
      }
    }
    return realDigest(...args);
  };
  try {
    const { renderHook, settleUntil, settleFor } = await harness();
    const mounted = renderHook(() =>
      useCodingSessionTeamWake(mountInput(), {
        evidenceClient: client,
        publishCommand,
      }),
    );
    await settleUntil(
      () => (mounted.result.current.deliveries ?? []).length > 0,
      "delivery rows",
    );
    await settleFor(20);
    assert.equal(raced, true, "the boundary recheck really ran after a digest");
    assert.deepEqual(
      calls,
      [],
      "the receipt-backed recheck aborted the publish",
    );
    assert.equal(mounted.result.current.deliveries[0].kind, "provider-started");
  } finally {
    globalThis.crypto.subtle.digest = realDigest;
  }
});

test("D-T7: incomplete wake evidence publishes nothing and starts no grace", async () => {
  install();
  window.localStorage.removeItem(STORAGE_KEY);
  seedState();
  const client = clientFor([ASSIGNMENT, REPORT], { wakeFetchFails: true });
  const { publishCommand, calls } = publisher();
  const { renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => (mounted.result.current.deliveries ?? []).length > 0,
    "unknown delivery row",
  );
  await settleFor(20);
  assert.deepEqual(calls, []);
  assert.equal(mounted.result.current.deliveries[0].kind, "unknown");
  assert.deepEqual(readState().observed, [], "no grace clock is started");
});

test("D-T10: unchanged inputs return the same delivery references", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const client = clientFor([
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
    turnReceipt({ commandId: "team-wake-2abd9f9a", status: "turn_queued" }),
  ]);
  const { publishCommand } = publisher();
  const input = mountInput();
  const { renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(input, {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => (mounted.result.current.deliveries ?? []).length > 0,
    "delivery rows",
  );
  await settleFor(10);
  const first = mounted.result.current;
  mounted.rerender();
  await settleFor(5);
  assert.equal(mounted.result.current.deliveries, first.deliveries);
  assert.equal(mounted.result.current.seatAuthorities, first.seatAuthorities);
});

test("F1: custody becomes permanent resolution on a start and survives evidence loss", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const history = [
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
    turnReceipt({ commandId: "team-wake-2abd9f9a", status: "turn_queued" }),
  ];
  const client = clientFor(history);
  const { publishCommand, calls } = publisher();
  const { act, renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => readState().custodied.length === 1,
    "durable custody row",
  );
  await act(async () => {
    for (const subscription of client.subscriptions) {
      subscription.onEvent(
        turnReceipt({
          commandId: "team-wake-2abd9f9a",
          status: "turn_started",
          turnId: "turn-9",
        }),
      );
    }
  });
  await settleUntil(
    () => readState().resolvedSourceEventIds.length === 1,
    "permanent resolution",
  );
  assert.deepEqual(readState().resolvedSourceEventIds, [REPORT.id]);
  assert.deepEqual(readState().custodied, []);
  mounted.unmount();

  // Every receipt and command ages out of the relay's retention window.
  const forgetful = clientFor([ASSIGNMENT, REPORT]);
  const remounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: forgetful,
      publishCommand,
    }),
  );
  await settleFor(30);
  assert.deepEqual(calls, [], "the durable ledger outlives the evidence");
  remounted.unmount();
});

test("F1: a custody row alone suppresses across a remount", async () => {
  install();
  seedState({
    observedAtMs: Date.now() - 60_000,
    extra: {
      custodied: [
        {
          sourceEventId: REPORT.id,
          leadTargetKey: LEAD_TARGET_KEY,
          commandId: "team-wake-2abd9f9a",
        },
      ],
    },
  });
  const client = clientFor([ASSIGNMENT, REPORT]);
  const { publishCommand, calls } = publisher();
  const { renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => (mounted.result.current.deliveries ?? []).length > 0,
    "delivery rows",
  );
  await settleFor(20);
  assert.deepEqual(calls, []);
  assert.equal(mounted.result.current.deliveries[0].kind, "provider-queued");
});

test("F1: a duplicate refusal then a dropped custodian yields exactly one re-arm", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const client = clientFor([
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "provider-cmd" }),
    turnReceipt({ commandId: "provider-cmd", status: "turn_queued" }),
    wakeCommand({ commandId: "desktop-cmd", secret: FOUNDER_SECRET }),
    turnReceipt({
      commandId: "desktop-cmd",
      status: "turn_refused",
      error: { code: "DUPLICATE_OPERATION", message: "provider-cmd owns it" },
    }),
  ]);
  const { publishCommand, calls } = publisher();
  const { act, renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => readState().custodied.length === 1,
    "custody by the provider command",
  );
  await settleFor(10);
  assert.deepEqual(calls, [], "custody suppresses while it holds");

  await act(async () => {
    for (const subscription of client.subscriptions) {
      subscription.onEvent(
        turnReceipt({
          commandId: "provider-cmd",
          status: "turn_dropped",
          error: { code: "NO_LIVE_EXECUTION", message: "the lead is gone" },
        }),
      );
    }
  });
  await settleUntil(() => calls.length === 1, "the released re-arm publish");
  await settleFor(20);
  assert.equal(calls.length, 1, "exactly one re-arm, ever");
  assert.match(calls[0].commandId, /:r1$/);
  assert.deepEqual(readState().custodied, [], "released custody is not kept");
  assert.equal(readState().reArmed.length, 1);

  await act(async () => {
    for (const subscription of client.subscriptions) {
      subscription.onEvent(
        turnReceipt({
          commandId: calls[0].commandId,
          status: "turn_started",
          turnId: "turn-r1",
        }),
      );
    }
  });
  await settleUntil(
    () => readState().resolvedSourceEventIds.length === 1,
    "permanent resolution of the re-armed command",
  );
  assert.equal(mounted.result.current.deliveries[0].kind, "fallback-started");
});

test("F2: one source whose publish throws never blocks the next source", async () => {
  install();
  seedState({
    observedAtMs: Date.now() - 60_000,
    observedSources: [REPORT.id, REPORT_2.id],
  });
  const client = clientFor([ASSIGNMENT, REPORT, REPORT_2]);
  const calls = [];
  const publishCommand = async (input) => {
    calls.push(input);
    if (input.text === POINTER) {
      throw new Error("relay refused the Desktop fallback");
    }
    return { eventId: "f".repeat(64), kind: 44220, commandId: input.commandId };
  };
  const { renderHook, settleUntil, settleFor } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: client,
      publishCommand,
    }),
  );
  await settleUntil(
    () => calls.some((call) => call.text === POINTER_2),
    "the second source publishing behind the first source's failure",
  );
  await settleFor(20);
  const rows = new Map(
    mounted.result.current.deliveries.map((row) => [row.sourceEventId, row]),
  );
  assert.equal(rows.get(REPORT.id).kind, "failed");
  assert.equal(rows.get(REPORT.id).detail, "Wake delivery failed");
  assert.notEqual(
    rows.get(REPORT_2.id).kind,
    "fallback-grace",
    "a sibling's failure must never leave this row reading Waiting",
  );
  assert.match(
    rows.get(REPORT_2.id).kind,
    /^fallback-(unconfirmed|queued|started)$/,
  );
  assert.equal(
    codingSessionTeamWakePublishFailed(readState(), REPORT.id, LEAD_TARGET_KEY),
    true,
    "the failure is durable, so a remount does not read Waiting either",
  );
});

test("H4: a publish failure is disclosure — the next mount retries and clears it", async () => {
  install();
  seedState({ observedAtMs: Date.now() - 60_000 });
  const history = [
    ASSIGNMENT,
    REPORT,
    wakeCommand({ commandId: "team-wake-2abd9f9a" }),
    turnReceipt({
      commandId: "team-wake-2abd9f9a",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ];
  const attempts = [];
  let throwNext = true;
  const publishCommand = async (input) => {
    attempts.push(input);
    if (throwNext) throw new Error("relay refused the Desktop fallback");
    return { eventId: "f".repeat(64), kind: 44220, commandId: input.commandId };
  };
  const { renderHook, settleUntil, settleFor } = await harness();
  // One client per mount, hoisted: a fresh object per render would re-arm the
  // evidence subscriptions on every render.
  const firstClient = clientFor(history);
  const secondClient = clientFor(history);
  const thirdClient = clientFor(history);

  const first = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: firstClient,
      publishCommand,
    }),
  );
  await settleUntil(() => attempts.length === 1, "the first, throwing publish");
  await settleFor(20);
  assert.equal(attempts.length, 1, "one attempt per mount, never a tight loop");
  assert.equal(first.result.current.deliveries[0].kind, "failed");
  assert.equal(
    codingSessionTeamWakePublishFailed(readState(), REPORT.id, LEAD_TARGET_KEY),
    true,
  );
  first.unmount();

  throwNext = false;
  const second = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: secondClient,
      publishCommand,
    }),
  );
  await settleFor(60);
  assert.equal(
    attempts.length,
    2,
    "a mount that can publish must retry a source whose earlier publish threw",
  );
  assert.equal(
    codingSessionTeamWakePublishFailed(readState(), REPORT.id, LEAD_TARGET_KEY),
    false,
    "a success clears the disclosure",
  );
  assert.equal(readState().pending.length, 1);
  second.unmount();

  // The retry path: the durable attempt survives, its grace expires again.
  const stored = readState();
  window.localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      ...stored,
      observed: [
        { sourceEventId: REPORT.id, fallbackNotBeforeMs: Date.now() - 60_000 },
      ],
    }),
  );
  throwNext = true;
  const third = renderHook(() =>
    useCodingSessionTeamWake(mountInput(), {
      evidenceClient: thirdClient,
      publishCommand,
    }),
  );
  await settleFor(60);
  assert.equal(attempts.length, 3, "still exactly one publish per mount");
  assert.equal(
    codingSessionTeamWakePublishFailed(readState(), REPORT.id, LEAD_TARGET_KEY),
    true,
    "a repeat failure simply re-records the disclosure",
  );
  assert.equal(third.result.current.deliveries[0].kind, "failed");
});
