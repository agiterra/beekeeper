/**
 * The other half of a sent turn.
 *
 * A 44220 the relay accepts can still be refused by the provider: an operator
 * who is neither the founder nor a granted operator gets a signed `failed`
 * 44224 and no turn. Nothing consumed that receipt, so the message a
 * non-granted member sent simply disappeared from their own composer.
 *
 * These mount the REAL hook against the REAL trusted ingress (real store, real
 * signature verification, real authority resolution) and drive the exact
 * production sequence: send (the editor is cleared), provider refuses, the
 * words come back with the provider's reason — and, when no receipt ever
 * comes, the watch expires without inventing a failure.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { CODING_SESSION_TURN_REFUSAL_DEADLINE_MS } from "../lib/codingSessionTurnRefusal.ts";

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

// The refusal deadline is the one clock in this feature, so the tests own it
// rather than waiting twenty real seconds for it.
const deadlineTimers = new Map();
let nextDeadlineTimerId = 1_000_000;

before(() => {
  const nativeSetTimeout = dom.window.setTimeout.bind(dom.window);
  const nativeClearTimeout = dom.window.clearTimeout.bind(dom.window);
  dom.window.setTimeout = (callback, delay, ...args) => {
    if (delay === CODING_SESSION_TURN_REFUSAL_DEADLINE_MS) {
      nextDeadlineTimerId += 1;
      deadlineTimers.set(nextDeadlineTimerId, callback);
      return nextDeadlineTimerId;
    }
    return nativeSetTimeout(callback, delay, ...args);
  };
  dom.window.clearTimeout = (id) => {
    if (deadlineTimers.delete(id)) return;
    nativeClearTimeout(id);
  };
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

const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const TURN_COMMAND_ID = "csc-turn-1";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OTHER_SECRET = generateSecretKey();
const REFUSAL_MESSAGE =
  "only the session founder or a granted operator may steer this execution";

const TRUSTED_CONFIG = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  "allowed-bridge-pubkeys": [
    { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
    { pubkey: getPublicKey(OTHER_SECRET), label: "Another provider" },
  ],
};

async function harness() {
  const { act, render } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { restoreCodingSessionDraft } = await import(
    "../lib/codingSessionTurnRefusal.ts"
  );
  const { useCodingSessionTurnRefusal } = await import(
    "./useCodingSessionTurnRefusal.tsx"
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

  const state = { send: null, isWatching: false, typeInto: null };
  function Harness() {
    // Exactly the composer's wiring: sending clears the editor, and only a
    // refusal ever puts words back into it.
    const [text, setText] = React.useState("");
    const restoreDraft = React.useCallback((refused) => {
      setText((current) => restoreCodingSessionDraft(current, refused));
    }, []);
    const refusal = useCodingSessionTurnRefusal({
      channelId: CHANNEL_ID,
      client,
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      restoreDraft,
    });
    state.isWatching = refusal.watcher !== null;
    state.typeInto = setText;
    state.send = (commandId) => {
      setText("");
      refusal.watch({ commandId, draft: text });
    };
    return React.createElement(
      "div",
      null,
      refusal.error
        ? React.createElement(
            "p",
            { "data-testid": "coding-session-composer-error" },
            refusal.error,
          )
        : null,
      React.createElement("textarea", {
        "aria-label": "Coding-session instruction",
        onChange: () => {},
        value: text,
      }),
      refusal.watcher,
    );
  }

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
      React.createElement(Harness),
    ),
  );
  await settle();

  return {
    act,
    draft: () =>
      view.container.querySelector('[aria-label="Coding-session instruction"]')
        .value,
    error: () =>
      view.container.querySelector(
        '[data-testid="coding-session-composer-error"]',
      )?.textContent ?? null,
    expireDeadlines: () => {
      const pending = [...deadlineTimers.entries()];
      deadlineTimers.clear();
      for (const [, callback] of pending) callback();
    },
    liveSubscriptions,
    settle,
    state,
    teardown: () => {
      view.unmount();
      queryClient.clear();
      ipcHandlers.clear();
      deadlineTimers.clear();
    },
  };
}

async function refusalEvent({
  commandId = TURN_COMMAND_ID,
  message = REFUSAL_MESSAGE,
  secret = PROVIDER_SECRET,
} = {}) {
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
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status: "failed",
        session: null,
        error: { code: "UNAUTHORIZED_OPERATOR", message },
      }),
    },
    secret,
  );
}

test("a refused turn says so and gives the person their words back", async () => {
  const scope = await harness();
  const receipt = await refusalEvent();

  await scope.act(async () => {
    scope.state.typeInto("ship the release notes");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  // Publishing is not consent: the editor is empty and the watch is live.
  assert.equal(scope.draft(), "");
  assert.equal(scope.error(), null);
  assert.equal(scope.state.isWatching, true);
  assert.equal(scope.liveSubscriptions.length, 1);

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(receipt);
  });
  await scope.settle();

  assert.equal(scope.error(), `Turn refused: ${REFUSAL_MESSAGE}`);
  assert.equal(scope.draft(), "ship the release notes");
  assert.equal(scope.state.isWatching, false);

  scope.teardown();
});

test("a refusal signed by another provider is not this turn's answer", async () => {
  const scope = await harness();
  const foreign = await refusalEvent({ secret: OTHER_SECRET });

  await scope.act(async () => {
    scope.state.typeInto("keep going");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(foreign);
  });
  await scope.settle();

  assert.equal(scope.error(), null);
  assert.equal(scope.draft(), "");
  assert.equal(scope.state.isWatching, true);

  scope.teardown();
});

test("a turn nobody refuses stops being watched, silently", async () => {
  const scope = await harness();

  await scope.act(async () => {
    scope.state.typeInto("run the tests");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  assert.equal(scope.state.isWatching, true);

  // A turn that ran publishes no receipt at all — its transcript items are the
  // acknowledgement — so the deadline must pass without a word.
  await scope.act(async () => {
    scope.expireDeadlines();
  });
  await scope.settle();

  assert.equal(scope.error(), null);
  assert.equal(scope.draft(), "");
  assert.equal(scope.state.isWatching, false);

  scope.teardown();
});
