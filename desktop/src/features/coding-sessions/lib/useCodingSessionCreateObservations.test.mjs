/**
 * The create-observation hook is the seam that makes founder/operator
 * authority live: without it `groupCodingSessionCatalog(entries, creates)`
 * never receives a create and every umbrella resolves to "authority unknown".
 *
 * This mounts the REAL hook (real store, real authority resolution, real
 * signature verification) against a fake relay client, and checks the three
 * things production depends on: the subscription is its own (creates are
 * human-signed and therefore unfiltered by author), a create alone binds
 * nothing, and the provider's receipt arriving live completes the join.
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

const CHANNEL_ID = "channel-1";
const COMMAND_ID = "csl-1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OPERATOR_SECRET = generateSecretKey();
const OPERATOR_PUBKEY = getPublicKey(OPERATOR_SECRET);

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

test("cold creates load before live admission and a gap receipt joins in post-fence catch-up", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionCreateEvent } = await import(
    "./codingSessionLifecycleCommand.ts"
  );
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    lifecycleReceiptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  const { useCodingSessionCreateObservations } = await import(
    "./useCodingSessionCreateObservations.ts"
  );

  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: null,
    title: "Advance Buzz live sessions",
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

  ipcHandlers.set("get_global_agent_config", async () => ({
    env_vars: {},
    provider: null,
    model: null,
    preferred_runtime: null,
    "allowed-bridge-pubkeys": [
      { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
    ],
  }));

  let releaseLive;
  const historyCalls = [];
  const historyBatches = [];
  const liveSubscriptions = [];
  const client = {
    fetchEvents: async () => {
      throw new Error("history must go through the bundled read");
    },
    fetchEventsBatch: async (filters) => {
      historyBatches.push(filters);
      historyCalls.push(...filters);
      return historyBatches.length === 1 ? [createEvent] : [receiptEvent];
    },
    subscribeLive: async (filter, onEvent) => {
      const subscription = { filter, onEvent };
      liveSubscriptions.push(subscription);
      return new Promise((resolve) => {
        releaseLive = () => resolve(() => {});
      });
    },
    subscribeToReconnects: () => () => {},
  };

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const settle = async () => {
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };

  const { result, unmount } = renderHook(
    () => useCodingSessionCreateObservations([CHANNEL_ID], client),
    { wrapper },
  );
  await settle();

  // Its own subscription, deliberately not the trusted-ingress one: both
  // kinds, scoped to the channel, and no `authors` narrowing — any member may
  // found a session, so there is no allowlist a create could be scoped by.
  // The backfill is one read per kind: a relay filter's row budget is shared
  // across every kind it names, and 44224 now grows by two receipts per turn,
  // so a shared budget would eventually return only receipts and no creates.
  // The live subscription has no budget to share, so it stays a single filter.
  // The three history filters ride one bundled `POST /query`, not three REQs.
  assert.equal(historyBatches.length, 1);
  assert.deepEqual(
    historyCalls.map((filter) => filter.kinds),
    [[44221], [44224], [44226]],
  );
  for (const filter of historyCalls) {
    assert.deepEqual(filter["#h"], [CHANNEL_ID]);
    assert.equal("authors" in filter, false);
  }
  assert.equal(liveSubscriptions.length, 1);
  assert.deepEqual(liveSubscriptions[0].filter.kinds, [44221, 44224, 44226]);
  assert.equal(result.current.isLoading, false);

  // History held the create but no receipt: nothing is bound yet, so every
  // consumer keeps the permissive fallback.
  assert.deepEqual(result.current.observations, []);

  await act(async () => {
    releaseLive();
  });

  await settle();
  assert.equal(
    historyBatches.length,
    2,
    "a receipt in the admission gap requires a second history read",
  );
  assert.equal(result.current.observations.length, 1);
  assert.equal(result.current.observations[0].signerPubkey, OPERATOR_PUBKEY);
  assert.equal(result.current.observations[0].sessionRef, SESSION_REF);
  assert.deepEqual(result.current.observations[0].target, TARGET);

  const { fanOutObservedCodingSessionEvents } = await import(
    "./codingSessionObservedEvents.ts"
  );
  const settledSnapshot = result.current;
  await act(async () => {
    fanOutObservedCodingSessionEvents([createEvent, receiptEvent]);
    fanOutObservedCodingSessionEvents([
      { ...receiptEvent, id: "unrelated", kind: 1 },
    ]);
  });
  assert.equal(
    result.current,
    settledSnapshot,
    "replayed and irrelevant bus events must not publish a new snapshot",
  );

  unmount();
  queryClient.clear();
  ipcHandlers.clear();
});

test("a member who runs no providers still sees who founded the session", async () => {
  // The regression this replaces: the hook used to resolve the machine-local
  // `allowed-bridge-pubkeys` list and refuse to read anything while it was
  // empty. That list says what this computer may *run*, so on every session
  // founded by somebody else it is empty of the answering provider — and the
  // consequence was not a missing detail but a wrong claim: no create/receipt
  // join, `founderPubkey: null`, and a composer that called a governed session
  // "ungoverned" and enabled controls the provider then refused.
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionCreateEvent } = await import(
    "./codingSessionLifecycleCommand.ts"
  );
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    lifecycleReceiptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  const { useCodingSessionCreateObservations } = await import(
    "./useCodingSessionCreateObservations.ts"
  );

  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: null,
    title: "Founded on another machine",
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

  ipcHandlers.set("get_global_agent_config", async () => ({
    env_vars: {},
    provider: null,
    model: null,
    preferred_runtime: null,
    "allowed-bridge-pubkeys": [],
  }));

  const historyCalls = [];
  const client = {
    fetchEvents: async () => {
      throw new Error("history must go through the bundled read");
    },
    fetchEventsBatch: async (filters) => {
      historyCalls.push(...filters);
      return [createEvent, receiptEvent];
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);

  const { result, unmount } = renderHook(
    () => useCodingSessionCreateObservations([CHANNEL_ID], client),
    { wrapper },
  );
  for (let round = 0; round < 8; round += 1) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }

  assert.equal(historyCalls.length, 6);
  assert.equal(result.current.observations.length, 1);
  assert.equal(result.current.observations[0].signerPubkey, OPERATOR_PUBKEY);
  assert.deepEqual(result.current.observations[0].target, TARGET);

  unmount();
  queryClient.clear();
  ipcHandlers.clear();
});

test("a live fence during the pending cold read is not coalesced away", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionCreateObservations } = await import(
    "./useCodingSessionCreateObservations.ts"
  );
  let releaseCold;
  let releaseLive;
  let reads = 0;
  const client = {
    fetchEvents: async () => [],
    fetchEventsBatch: async () => {
      reads += 1;
      if (reads === 1)
        return new Promise((resolve) => {
          releaseCold = resolve;
        });
      return [];
    },
    subscribeLive: async () =>
      new Promise((resolve) => {
        releaseLive = () => resolve(() => {});
      }),
    subscribeToReconnects: () => () => {},
  };
  const mounted = renderHook(() =>
    useCodingSessionCreateObservations([CHANNEL_ID], client),
  );
  await act(async () => {});
  assert.equal(reads, 1, "cold history must start without admission");
  await act(async () => releaseLive());
  assert.equal(reads, 1, "reads remain serialized");
  await act(async () => releaseCold([]));
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  assert.equal(
    reads,
    2,
    "post-fence read survives coalescing with the cold read",
  );
  mounted.unmount();
});

test("a live 44226 alone yields one founded umbrella; a later create and receipt hide it", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionGenesisEvent } = await import(
    "./codingSessionGenesis.ts"
  );
  const { buildCodingSessionCreateEvent } = await import(
    "./codingSessionLifecycleCommand.ts"
  );
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    lifecycleReceiptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  const { resolveFoundedCodingSessions } = await import(
    "./codingSessionFoundedModel.ts"
  );
  const { useCodingSessionCreateObservations } = await import(
    "./useCodingSessionCreateObservations.ts"
  );

  const genesisBuilt = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesisEvent = finalizeEvent(
    {
      kind: genesisBuilt.kind,
      created_at: 1_799_999_990,
      tags: genesisBuilt.tags,
      content: genesisBuilt.content,
    },
    OPERATOR_SECRET,
  );
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: genesisEvent.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: null,
    title: "Founded first, started later",
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

  ipcHandlers.set("get_global_agent_config", async () => ({
    env_vars: {},
    provider: null,
    model: null,
    preferred_runtime: null,
    "allowed-bridge-pubkeys": [],
  }));

  let liveOnEvent = null;
  const client = {
    fetchEvents: async () => {
      throw new Error("history must go through the bundled read");
    },
    // Empty history: everything arrives live, the way a session founded a
    // moment ago on this desktop does.
    fetchEventsBatch: async () => [],
    subscribeLive: async (_filter, onEvent) => {
      liveOnEvent = onEvent;
      return () => {};
    },
    subscribeToReconnects: () => () => {},
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const settle = async () => {
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };
  const foundedNow = () =>
    resolveFoundedCodingSessions({
      channelId: CHANNEL_ID,
      geneses: result.current.geneses,
      entries: [],
      creates: result.current.observations,
    });

  const { result, unmount } = renderHook(
    () => useCodingSessionCreateObservations([CHANNEL_ID], client),
    { wrapper },
  );
  await settle();
  assert.equal(result.current.isLoading, false);
  assert.deepEqual(result.current.geneses, []);
  assert.notEqual(liveOnEvent, null);

  await act(async () => {
    liveOnEvent(genesisEvent);
  });
  await settle();
  assert.deepEqual(result.current.observations, []);
  assert.deepEqual(result.current.geneses, [
    {
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: genesisEvent.id,
      founderPubkey: OPERATOR_PUBKEY,
      foundedAt: 1_799_999_990,
    },
  ]);
  assert.equal(foundedNow().length, 1, "founded: a genesis and nothing else");

  // Start: the create alone binds nothing (no observation yet), so the
  // umbrella still reads founded to the projection — the pending overlay,
  // not this store, covers the gap between Start and the receipt.
  await act(async () => {
    liveOnEvent(createEvent);
  });
  await settle();
  assert.deepEqual(result.current.observations, []);
  assert.equal(foundedNow().length, 1);

  // The provider's receipt joins the create; the genesis is claimed.
  await act(async () => {
    liveOnEvent(receiptEvent);
  });
  await settle();
  assert.equal(result.current.observations.length, 1);
  assert.equal(result.current.observations[0].genesisRef, genesisEvent.id);
  assert.equal(result.current.geneses.length, 1, "the founding fact stays");
  assert.deepEqual(foundedNow(), [], "but nothing is founded-and-unstarted");

  unmount();
  queryClient.clear();
  ipcHandlers.clear();
});

test("a session deleted by this app, or announced deleted by the relay, leaves the live store at once", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { KIND_SYSTEM_MESSAGE } = await import("@/shared/constants/kinds.ts");
  const { buildCodingSessionGenesisEvent } = await import(
    "./codingSessionGenesis.ts"
  );
  const {
    CODING_SESSION_DELETION_RECEIPT_TYPE,
    publishForgottenCodingSession,
  } = await import("./codingSessionForgotten.ts");
  const { useCodingSessionCreateObservations } = await import(
    "./useCodingSessionCreateObservations.ts"
  );
  const RELAY_SECRET = generateSecretKey();
  const RELAY_PUBKEY = getPublicKey(RELAY_SECRET);
  const genesisFor = (sessionRef, createdAt) => {
    const built = buildCodingSessionGenesisEvent({
      channelId: CHANNEL_ID,
      sessionRef,
    });
    return finalizeEvent(
      {
        kind: built.kind,
        created_at: createdAt,
        tags: built.tags,
        content: built.content,
      },
      OPERATOR_SECRET,
    );
  };
  const first = genesisFor(SESSION_REF, 1_799_999_990);
  const OTHER_REF = "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21";
  const second = genesisFor(OTHER_REF, 1_799_999_991);

  const liveByKinds = new Map();
  const client = {
    fetchEvents: async () => {
      throw new Error("history must go through the bundled read");
    },
    fetchEventsBatch: async () => [],
    subscribeLive: async (filter, onEvent) => {
      liveByKinds.set(filter.kinds.join(","), { filter, onEvent });
      return () => {};
    },
    subscribeToReconnects: () => () => {},
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const settle = async () => {
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };
  const { result, unmount } = renderHook(
    () =>
      useCodingSessionCreateObservations(
        [CHANNEL_ID],
        client,
        async () => RELAY_PUBKEY,
      ),
    { wrapper },
  );
  await settle();
  const creates = liveByKinds.get([44221, 44224, 44226].join(","));
  const receipts = liveByKinds.get(String(KIND_SYSTEM_MESSAGE));
  assert.notEqual(creates, undefined);
  assert.notEqual(
    receipts,
    undefined,
    "the relay's receipts are watched once its key is known",
  );
  assert.deepEqual(receipts.filter.authors, [RELAY_PUBKEY]);

  await act(async () => {
    creates.onEvent(first);
    creates.onEvent(second);
  });
  await settle();
  assert.equal(result.current.geneses.length, 2);

  // This app's own delete: the dialog publishes the fact on acceptance.
  await act(async () => {
    publishForgottenCodingSession({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: first.id,
    });
  });
  await settle();
  assert.deepEqual(
    result.current.geneses.map((g) => g.sessionRef),
    [OTHER_REF],
  );

  // Somebody else's delete: the relay says so under its own key.
  const receipt = finalizeEvent(
    {
      kind: KIND_SYSTEM_MESSAGE,
      created_at: 1_800_000_000,
      tags: [["h", CHANNEL_ID]],
      content: JSON.stringify({
        type: CODING_SESSION_DELETION_RECEIPT_TYPE,
        genesisRef: second.id,
        sessionRef: OTHER_REF,
        deletionEventId: "cd".repeat(32),
        channelId: CHANNEL_ID,
      }),
    },
    RELAY_SECRET,
  );
  await act(async () => {
    receipts.onEvent(receipt);
  });
  await settle();
  assert.deepEqual(result.current.geneses, []);

  // A receipt a member forged hides nothing.
  const forged = finalizeEvent(
    { ...receipt, id: undefined, sig: undefined },
    OPERATOR_SECRET,
  );
  await act(async () => {
    creates.onEvent(second);
  });
  await settle();
  assert.equal(
    result.current.geneses.length,
    0,
    "a forgotten genesis is not re-admitted by an echo",
  );
  await act(async () => {
    receipts.onEvent(forged);
  });
  await settle();
  unmount();
});
