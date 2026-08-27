/**
 * The turn that was never run, offered back to the execution that resumed.
 *
 * Generation fencing means a turn sent to generation N cannot run once the
 * session has resumed as N+1: the provider answers it with a durable
 * `turn_dropped`/`NO_LIVE_EXECUTION` or `turn_refused`/`STALE_GENERATION` and
 * nothing else happens (crew plan ruling R1, 2026-08-26). That receipt is the
 * moment the sender learns their words did not run, so it is the moment they
 * are offered a way to re-address them — and the offer must resolve to the
 * execution's *current* generation, never the one that refused.
 *
 * These mount the real composer against the real trusted ingress and the real
 * refusal watch, and drive the exact production sequence: send to generation 1,
 * the provider drops it, nothing has resumed yet (no offer, and the row says
 * why), the execution comes back as generation 2, one click re-sends the same
 * words as exactly one fresh 44220 addressed to generation 2.
 */
import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

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
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const OPERATOR = "c".repeat(64);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const SESSION_ID = "11111111-2222-3333-4444-555555555555";

const TRUSTED_CONFIG = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  "allowed-bridge-pubkeys": [
    { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
  ],
};

const SIBLING_SESSION_ID = "99999999-8888-7777-6666-555555555555";

function targetAt(generation, sessionId = SESSION_ID) {
  return {
    driver: "claude-agent-acp",
    instanceId: "claude-instance",
    sessionId,
    generation,
  };
}

beforeEach(async () => {
  const store = await import("../lib/codingSessionPendingTurns.ts");
  store.resetPendingCodingSessionTurns();
});

/** A per-stage turn receipt, keyed the way the publish queue fences it. */
async function turnStageEvent({ commandId, status, error, generation }) {
  const { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    codingSessionReceiptSemanticKey,
  } = await import("../lib/codingSessionTrustedIngress.ts");
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: 1_800_000_001,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", codingSessionReceiptSemanticKey(commandId, status)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: targetAt(generation),
        error,
      }),
    },
    PROVIDER_SECRET,
  );
}

async function harness() {
  const React = (await import("react")).default;
  const { act, fireEvent, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");

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

  const published = [];
  const publishCommand = async (input) => {
    published.push(input);
    return {
      eventId: `event-${published.length}`,
      kind: 44220,
      commandId: input.commandId,
    };
  };

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });

  function view(generation, sessionId = SESSION_ID) {
    return React.createElement(
      QueryClientProvider,
      { client: queryClient },
      React.createElement(CodingSessionComposer, {
        canInterrupt: false,
        channelId: CHANNEL_ID,
        currentUserPubkey: OPERATOR,
        isMember: true,
        isWorking: false,
        providerAuthorityPubkey: PROVIDER_PUBKEY,
        publishCommand,
        refusalClient: client,
        target: targetAt(generation, sessionId),
      }),
    );
  }

  const settle = async () => {
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };

  const rendered = render(view(1));
  await settle();

  const query = (selector) => rendered.container.querySelector(selector);
  return {
    act,
    editor: () =>
      query('[aria-label="Coding-session instruction"]') ?? query("textarea"),
    error: () =>
      query('[data-testid="coding-session-composer-error"]')?.textContent ??
      null,
    fireEvent,
    liveSubscriptions,
    published,
    readdressButton: () =>
      query('[data-testid="coding-session-composer-readdress"]'),
    readdressNote: () =>
      query('[data-testid="coding-session-composer-readdress-unavailable"]')
        ?.textContent ?? null,
    resumeTo: async (generation, sessionId) => {
      await act(async () => {
        rendered.rerender(view(generation, sessionId));
      });
      await settle();
    },
    settle,
    teardown: () => {
      rendered.unmount();
      queryClient.clear();
      ipcHandlers.clear();
    },
  };
}

async function sendTurn(scope, words) {
  await scope.act(async () => {
    scope.fireEvent.change(scope.editor(), { target: { value: words } });
  });
  await scope.act(async () => {
    scope.fireEvent.click(
      scope
        .editor()
        .ownerDocument.querySelector(
          '[data-testid="coding-session-composer-primary"]',
        ),
    );
  });
  await scope.settle();
}

test("a dropped turn is re-addressed to the generation that resumed", async () => {
  const scope = await harness();
  await sendTurn(scope, "run the migration");

  assert.equal(scope.published.length, 1);
  assert.equal(scope.published[0].target.generation, 1);
  const first = scope.published[0].commandId;
  assert.equal(scope.editor().value, "");

  const dropped = await turnStageEvent({
    commandId: first,
    status: "turn_dropped",
    error: {
      code: "NO_LIVE_EXECUTION",
      message: "no live execution owns this target",
    },
    generation: 1,
  });
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(dropped);
  });
  await scope.settle();

  // The words came back, the provider's own sentence is on screen — and the
  // resend is NOT offered, because nothing has resumed this execution yet.
  assert.match(scope.error() ?? "", /NO_LIVE_EXECUTION/);
  assert.equal(scope.editor().value, "run the migration");
  assert.equal(scope.readdressButton(), null);
  assert.match(scope.readdressNote() ?? "", /resumed/i);

  // The provider comes back and the session resumes as generation 2.
  await scope.resumeTo(2);
  const button = scope.readdressButton();
  assert.notEqual(button, null, "a resumed generation must offer the resend");
  assert.equal(button.textContent, "Resend to the resumed execution");

  await scope.act(async () => {
    scope.fireEvent.click(button);
  });
  await scope.settle();

  // Exactly one fresh 44220, addressed to the generation that resumed, with a
  // command id of its own — a re-sent turn is a new command, not a replay.
  assert.equal(scope.published.length, 2);
  assert.equal(scope.published[1].target.generation, 2);
  assert.equal(scope.published[1].text, "run the migration");
  assert.equal(scope.published[1].deliver, "boundary");
  assert.notEqual(scope.published[1].commandId, first);
  assert.equal(scope.editor().value, "");
  assert.equal(scope.readdressButton(), null);

  scope.teardown();
});

test("a stale-generation refusal is re-addressed the same way", async () => {
  const scope = await harness();
  await sendTurn(scope, "rerun the failing test");
  const first = scope.published[0].commandId;

  const refused = await turnStageEvent({
    commandId: first,
    status: "turn_refused",
    error: {
      code: "STALE_GENERATION",
      message: "this command addresses generation 1; the session is at 2",
    },
    generation: 1,
  });
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(refused);
  });
  await scope.settle();
  await scope.resumeTo(2);

  const button = scope.readdressButton();
  assert.notEqual(button, null);
  await scope.act(async () => {
    scope.fireEvent.click(button);
  });
  await scope.settle();

  assert.equal(scope.published.length, 2);
  assert.equal(scope.published[1].target.generation, 2);
  assert.equal(scope.published[1].text, "rerun the failing test");

  scope.teardown();
});

test("an unauthorized refusal is never dressed up as a re-addressable turn", async () => {
  const scope = await harness();
  await sendTurn(scope, "take over this session");
  const first = scope.published[0].commandId;

  const refused = await turnStageEvent({
    commandId: first,
    status: "turn_refused",
    error: {
      code: "UNAUTHORIZED_OPERATOR",
      message: "only the session founder may steer this execution",
    },
    generation: 1,
  });
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(refused);
  });
  await scope.settle();
  await scope.resumeTo(2);

  assert.match(scope.error() ?? "", /UNAUTHORIZED_OPERATOR/);
  assert.equal(scope.readdressButton(), null);
  assert.equal(scope.readdressNote(), null);
  assert.equal(scope.published.length, 1);

  scope.teardown();
});

test("an offer never follows the composer to a different seat", async () => {
  // One composer instance serves whichever participant the umbrella has
  // selected, so a refusal from one execution can still be in hand while the
  // editor points at another. These words were written for the seat they were
  // sent to; re-addressing them to a sibling would publish a message to an
  // agent the person never addressed.
  const scope = await harness();
  await sendTurn(scope, "finish the rebase");
  const first = scope.published[0].commandId;

  const dropped = await turnStageEvent({
    commandId: first,
    status: "turn_dropped",
    error: {
      code: "NO_LIVE_EXECUTION",
      message: "no live execution owns this target",
    },
    generation: 1,
  });
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(dropped);
  });
  await scope.settle();

  // A sibling seat, at a higher generation than the one that refused.
  await scope.resumeTo(5, SIBLING_SESSION_ID);
  assert.equal(scope.readdressButton(), null);
  assert.equal(scope.readdressNote(), null);
  assert.equal(scope.published.length, 1);

  // Back on the seat the words were written for, the offer is there again.
  await scope.resumeTo(2);
  assert.notEqual(scope.readdressButton(), null);

  scope.teardown();
});
