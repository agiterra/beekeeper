/**
 * First-run race regression: the trusted ingress must re-arm when the trust
 * list changes.
 *
 * On a brand-new instance `allowed-bridge-pubkeys` is empty until the New
 * Coding Session screen provisions the local provider from Rust
 * (`session_provider/trust.rs`). The shared config query caches with
 * `staleTime: Infinity`, so unless provisioning invalidates it, the mounted
 * ingress hook keeps its pre-provision (empty, fail-closed) authority forever:
 * the provider publishes its 44224 receipt, nobody is subscribed with its
 * pubkey in `authors`, and the create flow spins at "Waiting for the session
 * provider to accept this request..." until the window reloads.
 *
 * This mounts the REAL hook (real store, real authority resolution, real
 * signature verification) against a fake relay client and a real QueryClient,
 * and drives the exact production sequence: empty trust -> provision seeds the
 * first entry -> `refreshGlobalAgentConfig` -> the hook must fetch history and
 * re-subscribe live with the provider's pubkey, and the pending receipt wait
 * must resolve.
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

// @tauri-apps/api/core resolves window.__TAURI_INTERNALS__.invoke — install
// the stub before any production module is imported.
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
const COMMAND_ID = "create-1";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

const EMPTY_CONFIG = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  "allowed-bridge-pubkeys": [],
};

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

test("ingress re-arms when the trust set gains its first entry, and the pending receipt resolves", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { refreshGlobalAgentConfig } = await import(
    "@/features/agents/useGlobalAgentConfig.ts"
  );
  const {
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_TRANSCRIPT,
  } = await import("@/shared/constants/kinds.ts");
  const { buildCodingSessionTargetKey } = await import(
    "./codingSessionCommand.ts"
  );
  const {
    BUZZ_CODING_SESSION_METADATA_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    CODING_SESSION_METADATA_TAG_VERSION,
    codingSessionMetadataSemanticKey,
    lifecycleReceiptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  const { useTrustedCodingSessionIngress } = await import(
    "./useTrustedCodingSessionIngress.ts"
  );

  const receiptEvent = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: 1_800_000_000,
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
      created_at: 1_800_000_001,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        title: "First-ever coding session",
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

  // The "backend config file". Provisioning mutates it behind the UI's back.
  let backendConfig = EMPTY_CONFIG;
  ipcHandlers.set("get_global_agent_config", async () => backendConfig);

  // The receipt is already on the relay by the time the ingress re-arms —
  // exactly the live failure, where the provider accepted the create while
  // nobody trusted was listening. It must be recovered via the history fetch.
  let relayHistory = [];
  const historyCalls = [];
  const liveSubscriptions = [];
  const client = {
    fetchEvents: async (filter) => {
      historyCalls.push(filter);
      return relayHistory;
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

  // gcTime: 0 — the default 5-minute garbage-collection timer would keep the
  // node:test process alive long after the test finishes.
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
      useTrustedCodingSessionIngress(
        [CHANNEL_ID],
        COMMAND_ID,
        PROVIDER_PUBKEY,
        client,
      ),
    { wrapper },
  );
  await settle();

  // Fail-closed before provisioning: no trusted pubkeys means no relay
  // traffic at all, and the create sits in "pending".
  assert.equal(historyCalls.length, 0, "no history fetch with empty trust");
  assert.equal(liveSubscriptions.length, 0, "no live sub with empty trust");
  assert.ok(result.current.authorityErrorMessage);
  assert.deepEqual(result.current.lifecycle, {
    state: "pending",
    commandId: COMMAND_ID,
  });

  // Provisioning seeds the first trust entry in the config file (Rust), the
  // provider's receipt lands on the relay, and the create-flow fix calls
  // refreshGlobalAgentConfig. Nothing else changes — no remount, no reload.
  backendConfig = {
    ...EMPTY_CONFIG,
    "allowed-bridge-pubkeys": [
      { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
    ],
  };
  relayHistory = [receiptEvent];
  await act(async () => {
    refreshGlobalAgentConfig(queryClient);
  });
  await settle();

  // The ingress re-armed under the new authority: history refetched and a
  // live subscription established, both scoped to the provider's pubkey.
  assert.equal(result.current.authorityErrorMessage, null);
  assert.ok(historyCalls.length >= 1, "history must be refetched on re-arm");
  assert.deepEqual(historyCalls.at(-1).authors, [PROVIDER_PUBKEY]);
  assert.equal(liveSubscriptions.length, 1, "live sub must re-arm");
  assert.deepEqual(liveSubscriptions[0].filter.authors, [PROVIDER_PUBKEY]);
  assert.deepEqual(
    [...liveSubscriptions[0].filter.kinds].sort(),
    [
      KIND_CODING_SESSION_METADATA,
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      KIND_CODING_SESSION_TRANSCRIPT,
    ].sort(),
  );

  // The pending receipt wait resolved from the history recovery...
  assert.equal(result.current.lifecycle?.state, "awaiting-metadata");
  assert.deepEqual(result.current.lifecycle?.target, TARGET);

  // ...and the re-armed live subscription is actually ingesting: the
  // provider's metadata completes the create.
  await act(async () => {
    liveSubscriptions[0].onEvent(metadataEvent);
  });
  assert.equal(result.current.lifecycle?.state, "created");
  assert.equal(
    result.current.lifecycle?.metadata.title,
    "First-ever coding session",
  );

  const { fanOutObservedCodingSessionEvents } = await import(
    "./codingSessionObservedEvents.ts"
  );
  const settledSnapshot = result.current;
  await act(async () => {
    fanOutObservedCodingSessionEvents([receiptEvent, metadataEvent]);
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

/**
 * Pinned mode: the command names its own provider.
 *
 * A command-scoped resolution has exactly one possible answerer — the pubkey
 * written into the command — and on any session founded by another member that
 * pubkey is absent from this machine's `allowed-bridge-pubkeys`, which lists
 * what this computer may *run*. Resolving through the config therefore asked
 * the wrong question and answered "nobody": no `authors` for the receipt on
 * the wire, and `rejected-author` for the copy that arrived anyway.
 */
test("pinned mode reads the provider the command names, not this machine's allowlist", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    lifecycleReceiptSemanticKey,
  } = await import("./codingSessionTrustedIngress.ts");
  const { useTrustedCodingSessionIngress } = await import(
    "./useTrustedCodingSessionIngress.ts"
  );

  const foreignSecret = generateSecretKey();
  const foreignPubkey = getPublicKey(foreignSecret);
  const strangerSecret = generateSecretKey();
  const otherPinnedSecret = generateSecretKey();
  const otherPinnedPubkey = getPublicKey(otherPinnedSecret);
  const receipt = (secret, target, createdAt = 1_800_000_000) =>
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        created_at: createdAt,
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
          session: target,
          error: null,
        }),
      },
      secret,
    );

  // This machine runs its own provider and has never heard of the foreign one.
  ipcHandlers.set("get_global_agent_config", async () => ({
    ...EMPTY_CONFIG,
    "allowed-bridge-pubkeys": [
      {
        pubkey: getPublicKey(generateSecretKey()),
        label: "This computer (coding sessions)",
      },
    ],
  }));

  const historyCalls = [];
  const liveSubscriptions = [];
  const client = {
    fetchEvents: async (filter) => {
      historyCalls.push(filter);
      return [];
    },
    subscribeLive: async (filter, onEvent) => {
      liveSubscriptions.push({ filter, onEvent });
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

  const { result, rerender, unmount } = renderHook(
    ({ pin }) =>
      useTrustedCodingSessionIngress(
        [CHANNEL_ID],
        COMMAND_ID,
        pin,
        client,
        null,
        "pinned",
      ),
    { initialProps: { pin: foreignPubkey }, wrapper },
  );
  await settle();

  // Subscribed for the command's own provider, and not delayed by a config it
  // does not read.
  assert.equal(result.current.authorityErrorMessage, null);
  assert.equal(historyCalls.length, 1);
  assert.deepEqual(historyCalls[0].authors, [foreignPubkey]);
  assert.equal(liveSubscriptions.length, 1);
  assert.deepEqual(liveSubscriptions[0].filter.authors, [foreignPubkey]);

  // A third party's receipt for this command is not this command's answer...
  await act(async () => {
    liveSubscriptions[0].onEvent(receipt(strangerSecret, TARGET));
  });
  assert.deepEqual(result.current.lifecycle, {
    state: "pending",
    commandId: COMMAND_ID,
  });
  assert.equal(result.current.rejectedAuthorCount, 1);

  // ...and the pinned provider's is.
  await act(async () => {
    liveSubscriptions[0].onEvent(receipt(foreignSecret, TARGET));
  });
  assert.equal(result.current.lifecycle?.state, "awaiting-metadata");
  assert.deepEqual(result.current.lifecycle?.target, TARGET);

  // Two disagreeing payloads from the pinned provider itself are a conflict,
  // not a pick: pinning narrows who may answer, it never softens the
  // single-agreeing-answer rule.
  await act(async () => {
    liveSubscriptions[0].onEvent(
      receipt(foreignSecret, { ...TARGET, generation: 2 }, 1_800_000_001),
    );
  });
  assert.deepEqual(result.current.lifecycle, {
    state: "conflict",
    commandId: COMMAND_ID,
  });

  // Re-pinning is a different question, so it gets a different store: nothing
  // admitted under the old pin survives into the new one.
  await act(async () => {
    rerender({ pin: otherPinnedPubkey });
  });
  await settle();
  assert.deepEqual(result.current.lifecycle, {
    state: "pending",
    commandId: COMMAND_ID,
  });
  assert.deepEqual(historyCalls.at(-1).authors, [otherPinnedPubkey]);
  assert.deepEqual(liveSubscriptions.at(-1).filter.authors, [
    otherPinnedPubkey,
  ]);

  unmount();
  queryClient.clear();
  ipcHandlers.clear();
});
