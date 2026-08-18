/**
 * The other half of a reconnect.
 *
 * Provider-side resume works by minting the NEXT generation: a new target, new
 * 44223 metadata, a new catalog record, and therefore a new `generationId`.
 * The workspace route pins the exact generation it addresses, so a screen that
 * publishes the 44221 and awaits only the publish stays on the dead generation
 * — whose immutable metadata says `disconnected` — forever.
 *
 * These mount the REAL hook against the REAL trusted ingress (real store, real
 * signature verification, real authority resolution) and a REAL router, and
 * drive the exact production sequence: press Reconnect, provider answers with
 * a signed receipt naming generation N+1, the window must follow it.
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
    // The router reads `self` on its client path, which only this suite takes
    // (a jsdom `window` is what tells router-core it is not on a server).
    self: dom.window,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const COMMAND_ID = "csl-resume-1";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

const DEAD_GENERATION = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const RESUMED_GENERATION = { ...DEAD_GENERATION, generation: 2 };

const TRUSTED_CONFIG = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  "allowed-bridge-pubkeys": [
    { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
  ],
};

async function harness({ initialSurface = "main" } = {}) {
  const { act, render } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const {
    createMemoryHistory,
    createRootRoute,
    createRoute,
    createRouter,
    Outlet,
    RouterProvider,
  } = await import("@tanstack/react-router");
  const { buildCodingSessionTranscriptGenerationId } = await import(
    "../lib/codingSessionTranscriptPresentation.ts"
  );
  const { parseCodingSessionSurface } = await import(
    "../lib/codingSessionRoute.ts"
  );
  const { useCodingSessionResumeSettle } = await import(
    "./useCodingSessionResumeSettle.tsx"
  );

  ipcHandlers.set("get_global_agent_config", async () => TRUSTED_CONFIG);

  const liveSubscriptions = [];
  const client = {
    fetchEvents: async () => [],
    subscribeLive: async (filter, onEvent) => {
      const subscription = { filter, onEvent, closed: false };
      liveSubscriptions.push(subscription);
      return () => {
        subscription.closed = true;
      };
    },
    subscribeToReconnects: () => () => {},
  };

  const notices = [];
  const state = { begin: null, error: null, isPending: false };
  function Harness() {
    const settle = useCodingSessionResumeSettle({
      channelId: CHANNEL_ID,
      client,
      notify: (message) => notices.push(message),
      providerAuthorityPubkey: PROVIDER_PUBKEY,
    });
    state.begin = settle.begin;
    state.error = settle.error;
    state.isPending = settle.isPending;
    return React.createElement(React.Fragment, null, settle.watcher);
  }

  const rootRoute = createRootRoute({
    component: () => React.createElement(Outlet),
  });
  const sessionRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/coding-sessions/$channelId/$generationId",
    validateSearch: (search) => ({
      surface: parseCodingSessionSurface(search.surface),
    }),
    component: Harness,
  });
  const deadGenerationId = buildCodingSessionTranscriptGenerationId(
    CHANNEL_ID,
    PROVIDER_PUBKEY,
    DEAD_GENERATION,
  );
  const resumedGenerationId = buildCodingSessionTranscriptGenerationId(
    CHANNEL_ID,
    PROVIDER_PUBKEY,
    RESUMED_GENERATION,
  );
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => null,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute, sessionRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  // Land on the disconnected generation the way the app does — through route
  // params, which is what encodes a structured generation id into a single
  // path segment — before mounting, so the first render is the workspace.
  await router.navigate({
    to: "/coding-sessions/$channelId/$generationId",
    params: { channelId: CHANNEL_ID, generationId: deadGenerationId },
    search: { surface: initialSurface },
  });
  await router.load();

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const settle = async () => {
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };

  const view = render(
    React.createElement(
      QueryClientProvider,
      { client: queryClient },
      React.createElement(RouterProvider, { router }),
    ),
  );
  await settle();

  let navigations = 0;
  const routerNavigate = router.navigate.bind(router);
  router.navigate = (options) => {
    navigations += 1;
    return routerNavigate(options);
  };

  return {
    act,
    deadGenerationId,
    liveSubscriptions,
    navigationCount: () => navigations,
    notices,
    resumedGenerationId,
    routedGenerationId: () =>
      router.state.matches.at(-1)?.params.generationId ?? null,
    router,
    settle,
    state,
    teardown: () => {
      view.unmount();
      queryClient.clear();
      ipcHandlers.clear();
    },
  };
}

async function receiptEvent(payload) {
  const { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    lifecycleReceiptSemanticKey,
  } = await import("../lib/codingSessionTrustedIngress.ts");
  return finalizeEvent(
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
        ...payload,
      }),
    },
    PROVIDER_SECRET,
  );
}

async function metadataEvent(target) {
  const { KIND_CODING_SESSION_METADATA } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionTargetKey } = await import(
    "../lib/codingSessionCommand.ts"
  );
  const {
    BUZZ_CODING_SESSION_METADATA_SCHEMA,
    CODING_SESSION_METADATA_TAG_VERSION,
    codingSessionMetadataSemanticKey,
  } = await import("../lib/codingSessionTrustedIngress.ts");
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: 1_800_000_001,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: target,
        projectRef: null,
        repoRef: null,
        title: "Reconnected coding session",
        agentRef: null,
        provider: "claude-primary",
        runtime: "claude",
        model: "claude-opus-5",
        status: "idle",
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
}

test("a resumed receipt moves the window onto the generation it minted, exactly once", async () => {
  const scope = await harness({ initialSurface: "popout" });
  const receipt = await receiptEvent({
    status: "resumed",
    session: RESUMED_GENERATION,
    error: null,
  });
  const metadata = await metadataEvent(RESUMED_GENERATION);

  await scope.act(async () => {
    scope.state.begin(COMMAND_ID);
  });
  await scope.settle();
  // Publishing is not the outcome: the composer stays busy on the dead
  // generation until the provider's receipt says where to go.
  assert.equal(scope.state.isPending, true);
  assert.equal(scope.routedGenerationId(), scope.deadGenerationId);
  assert.equal(scope.navigationCount(), 0);
  assert.equal(scope.liveSubscriptions.length, 1);

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(receipt);
    scope.liveSubscriptions[0].onEvent(metadata);
  });
  await scope.settle();

  assert.equal(scope.routedGenerationId(), scope.resumedGenerationId);
  // A pop-out that reconnects stays a pop-out.
  assert.equal(scope.router.state.location.search.surface, "popout");
  assert.equal(scope.navigationCount(), 1);
  assert.equal(scope.state.isPending, false);
  assert.equal(scope.state.error, null);
  assert.deepEqual(scope.notices, []);

  // A replayed receipt (relay refetch, reconnect backfill) is the same fact,
  // not a second reconnect.
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(receipt);
    scope.liveSubscriptions[0].onEvent(metadata);
  });
  await scope.settle();
  assert.equal(scope.navigationCount(), 1);

  scope.teardown();
});

test("a refused resume surfaces the refusal and stays put", async () => {
  const scope = await harness();
  const receipt = await receiptEvent({
    status: "failed",
    session: null,
    error: {
      code: "STALE_GENERATION",
      message: "Generation 1 is not the active generation.",
    },
  });

  await scope.act(async () => {
    scope.state.begin(COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(receipt);
  });
  await scope.settle();

  // The exact silence this fixes: the provider publishes STALE_GENERATION,
  // the desktop verifies it, and the person is told.
  assert.equal(scope.state.error, "Generation 1 is not the active generation.");
  assert.equal(scope.state.isPending, false);
  assert.equal(scope.navigationCount(), 0);
  assert.equal(scope.routedGenerationId(), scope.deadGenerationId);

  scope.teardown();
});

test("a resume that recovered no context navigates and reports the loss", async () => {
  const scope = await harness();
  const receipt = await receiptEvent({
    status: "resumed_without_context",
    session: RESUMED_GENERATION,
    error: {
      code: "CONTEXT_NOT_RECOVERED",
      message: "The prior session file was pruned.",
    },
  });
  const metadata = await metadataEvent(RESUMED_GENERATION);

  await scope.act(async () => {
    scope.state.begin(COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(receipt);
    scope.liveSubscriptions[0].onEvent(metadata);
  });
  await scope.settle();

  // The session exists, so it opens; the agent starts from nothing, so it is
  // said out loud rather than swallowed by the navigation that caused it.
  assert.equal(scope.routedGenerationId(), scope.resumedGenerationId);
  assert.equal(scope.navigationCount(), 1);
  assert.equal(scope.notices.length, 1);
  assert.match(scope.notices[0], /Reconnected without prior context/);
  assert.match(scope.notices[0], /The prior session file was pruned\./);

  scope.teardown();
});
