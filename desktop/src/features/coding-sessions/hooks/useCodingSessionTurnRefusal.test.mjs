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

/**
 * The member most likely to be refused: someone who joined a session founded
 * by another person. Their machine's run-permission list knows nothing about
 * the provider that will answer them.
 */
const FOREIGN_MEMBER_CONFIG = {
  ...TRUSTED_CONFIG,
  "allowed-bridge-pubkeys": [],
};

async function harness({
  config = TRUSTED_CONFIG,
  history = [],
  targetKey,
} = {}) {
  const { act, render } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { restoreCodingSessionDraft } = await import(
    "../lib/codingSessionTurnRefusal.ts"
  );
  const { forgetPendingCodingSessionTurn } = await import(
    "../lib/codingSessionPendingTurns.ts"
  );
  const { useCodingSessionTurnRefusal } = await import(
    "./useCodingSessionTurnRefusal.tsx"
  );

  ipcHandlers.set("get_global_agent_config", async () => config);

  const liveSubscriptions = [];
  const client = {
    fetchEvents: async () => history,
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
    // Exactly the composer's `restoreRefusedDraft`: the words come back and
    // the optimistic row they belonged to is retired.
    const restoreDraft = React.useCallback((refused, refusedCommandId) => {
      setText((current) => restoreCodingSessionDraft(current, refused));
      if (refusedCommandId)
        forgetPendingCodingSessionTurn(CHANNEL_ID, refusedCommandId);
    }, []);
    const refusal = useCodingSessionTurnRefusal({
      channelId: CHANNEL_ID,
      client,
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      restoreDraft,
      targetKey,
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
    pendingDeadlines: () => deadlineTimers.size,
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

/** A per-stage turn receipt, keyed the way the publish queue fences it. */
async function turnStageEvent({
  commandId = TURN_COMMAND_ID,
  status,
  secret = PROVIDER_SECRET,
  error = null,
} = {}) {
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
        session: {
          driver: "claude-agent-acp",
          instanceId: "0123456789abcdef",
          sessionId: "11111111-2222-3333-4444-555555555555",
          generation: 1,
        },
        error,
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

  assert.equal(
    scope.error(),
    `Turn refused (UNAUTHORIZED_OPERATOR): ${REFUSAL_MESSAGE}`,
  );
  assert.equal(scope.draft(), "ship the release notes");
  assert.equal(scope.state.isWatching, false);

  scope.teardown();
});

test("a member whose machine runs no such provider still hears the refusal", async () => {
  // This is the case the refusal exists for. The person is neither founder nor
  // granted operator, so the provider says no — and that provider belongs to
  // the founder's machine, not theirs. Resolving the receipt through their own
  // `allowed-bridge-pubkeys` (empty here) asked whether they may *run* that
  // provider, subscribed for nobody, and let a signed "you were not allowed to
  // say that" expire in silence with the person's words gone.
  const scope = await harness({ config: FOREIGN_MEMBER_CONFIG });
  const receipt = await refusalEvent();

  await scope.act(async () => {
    scope.state.typeInto("take over this session");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  assert.equal(scope.draft(), "");
  assert.equal(scope.liveSubscriptions.length, 1);
  assert.deepEqual(scope.liveSubscriptions[0].filter.authors, [
    PROVIDER_PUBKEY,
  ]);
  assert.equal(scope.pendingDeadlines(), 1);

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(receipt);
  });
  await scope.settle();

  // Before the deadline could expire the watch in silence.
  assert.equal(
    scope.error(),
    `Turn refused (UNAUTHORIZED_OPERATOR): ${REFUSAL_MESSAGE}`,
  );
  assert.equal(scope.draft(), "take over this session");
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

test("a dropped turn is returned to the person as a drop, not a refusal", async () => {
  const scope = await harness();
  const dropped = await turnStageEvent({
    status: "turn_dropped",
    error: { code: "QUEUE_FULL", message: "the session queue is full" },
  });

  await scope.act(async () => {
    scope.state.typeInto("and then run the linter");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  assert.equal(scope.draft(), "");

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(dropped);
  });
  await scope.settle();

  assert.equal(
    scope.error(),
    "Turn dropped (QUEUE_FULL): the session queue is full",
  );
  assert.equal(scope.draft(), "and then run the linter");
  assert.equal(scope.state.isWatching, false);

  scope.teardown();
});

test("a turn refused for a stale generation names that code", async () => {
  const scope = await harness();
  const refused = await turnStageEvent({
    status: "turn_refused",
    error: {
      code: "STALE_GENERATION",
      message: "this execution has been superseded",
    },
  });

  await scope.act(async () => {
    scope.state.typeInto("keep going");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(refused);
  });
  await scope.settle();

  assert.equal(
    scope.error(),
    "Turn refused (STALE_GENERATION): this execution has been superseded",
  );
  assert.equal(scope.draft(), "keep going");

  scope.teardown();
});

test("a queued turn marks its optimistic row without touching the editor", async () => {
  const {
    markPendingCodingSessionTurnPublished,
    readPendingCodingSessionTurns,
    recordPendingCodingSessionTurn,
    resetPendingCodingSessionTurns,
  } = await import("../lib/codingSessionPendingTurns.ts");
  resetPendingCodingSessionTurns();
  const scope = await harness();
  const queued = await turnStageEvent({ status: "turn_queued" });

  recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey: "coding-session/v1|whatever",
    commandId: TURN_COMMAND_ID,
    text: "run the tests",
    operatorPubkey: null,
    recordedAt: Date.now(),
    published: false,
  });
  markPendingCodingSessionTurnPublished(CHANNEL_ID, TURN_COMMAND_ID);

  await scope.act(async () => {
    scope.state.typeInto("run the tests");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(queued);
  });
  await scope.settle();

  assert.equal(readPendingCodingSessionTurns()[0].queuedByProvider, true);
  // Queued is not refused: nothing goes back into the editor and no error
  // line appears.
  assert.equal(scope.error(), null);
  assert.equal(scope.draft(), "");

  resetPendingCodingSessionTurns();
  scope.teardown();
});

test("a degraded steer relabels the row and leaves the editor alone", async () => {
  const {
    markPendingCodingSessionTurnPublished,
    readPendingCodingSessionTurns,
    recordPendingCodingSessionTurn,
    resetPendingCodingSessionTurns,
  } = await import("../lib/codingSessionPendingTurns.ts");
  resetPendingCodingSessionTurns();
  const scope = await harness();
  const degraded = await turnStageEvent({
    status: "turn_degraded",
    error: {
      code: "STEER_UNSUPPORTED",
      message: "this runtime advertised no native steering",
    },
  });

  recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey: "coding-session/v1|whatever",
    commandId: TURN_COMMAND_ID,
    text: "look at the second failure first",
    operatorPubkey: null,
    recordedAt: Date.now(),
    published: false,
  });
  markPendingCodingSessionTurnPublished(CHANNEL_ID, TURN_COMMAND_ID);

  await scope.act(async () => {
    scope.state.typeInto("look at the second failure first");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(degraded);
  });
  await scope.settle();

  assert.equal(readPendingCodingSessionTurns()[0].degradedByProvider, true);
  // The turn still runs, so the words stay sent: a downgrade is not a refusal
  // and must never hand the draft back as though nothing was published.
  assert.equal(scope.error(), null);
  assert.equal(scope.draft(), "");

  resetPendingCodingSessionTurns();
  scope.teardown();
});

test("a turn the provider is holding stays watched past the refusal deadline", async () => {
  const { resetPendingCodingSessionTurns } = await import(
    "../lib/codingSessionPendingTurns.ts"
  );
  resetPendingCodingSessionTurns();
  const scope = await harness();
  const queued = await turnStageEvent({ status: "turn_queued" });
  const dropped = await turnStageEvent({
    status: "turn_dropped",
    error: {
      code: "NO_LIVE_EXECUTION",
      message: "no execution is running for this session",
    },
  });

  await scope.act(async () => {
    scope.state.typeInto("run the whole suite");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(queued);
  });
  await scope.settle();

  // A queued turn can wait behind an hour of work. Twenty seconds of silence
  // is not evidence it ran, so the watch is not allowed to expire on it —
  // otherwise the drop below would arrive to nobody listening.
  assert.equal(scope.pendingDeadlines(), 0);
  await scope.act(async () => {
    scope.expireDeadlines();
  });
  await scope.settle();
  assert.equal(scope.state.isWatching, true);

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(dropped);
  });
  await scope.settle();
  assert.equal(
    scope.error(),
    "Turn dropped (NO_LIVE_EXECUTION): no execution is running for this session",
  );
  assert.equal(scope.draft(), "run the whole suite");

  resetPendingCodingSessionTurns();
  scope.teardown();
});

test("a started turn hands the row to the transcript and stops watching", async () => {
  const { resetPendingCodingSessionTurns } = await import(
    "../lib/codingSessionPendingTurns.ts"
  );
  resetPendingCodingSessionTurns();
  const scope = await harness();
  const queued = await turnStageEvent({ status: "turn_queued" });
  const started = await turnStageEvent({ status: "turn_started" });
  started.content = JSON.stringify({
    ...JSON.parse(started.content),
    turnId: "turn-abc",
  });
  const { finalizeEvent: sign } = await import("nostr-tools/pure");
  const signedStarted = sign(
    {
      kind: started.kind,
      created_at: started.created_at,
      tags: started.tags,
      content: started.content,
    },
    PROVIDER_SECRET,
  );

  await scope.act(async () => {
    scope.state.typeInto("go");
  });
  await scope.act(async () => {
    scope.state.send(TURN_COMMAND_ID);
  });
  await scope.settle();
  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(queued);
  });
  await scope.settle();
  assert.equal(scope.state.isWatching, true);

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(signedStarted);
  });
  await scope.settle();

  assert.equal(scope.state.isWatching, false);
  assert.equal(scope.error(), null);
  assert.equal(scope.draft(), "");

  resetPendingCodingSessionTurns();
  scope.teardown();
});

test("holding five turns at once does not silence the first of them", async () => {
  // The composer's local queue retired this slice: every draft sent mid-turn
  // is published immediately, so a person can have as many turns held by the
  // provider as the pending-row bound allows. A held row is exempt from the
  // pending TTL *and* from the refusal deadline, so a row whose watch was
  // evicted can never retire — it sits above the composer forever, and the
  // `turn_dropped` that would have retired it arrives to nobody.
  const { resetPendingCodingSessionTurns } = await import(
    "../lib/codingSessionPendingTurns.ts"
  );
  const { MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET } = await import(
    "../lib/codingSessionPendingTurns.ts"
  );
  resetPendingCodingSessionTurns();
  const scope = await harness();
  const held = ["a", "b", "c", "d", "e"].map((suffix) => `csc-hold-${suffix}`);
  assert.ok(
    held.length <= MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET,
    "this test must stay inside the number of rows a person can really have",
  );

  const deliver = async (event) => {
    for (const subscription of scope.liveSubscriptions) {
      if (subscription.closed) continue;
      await scope.act(async () => {
        subscription.onEvent(event);
      });
    }
    await scope.settle();
  };

  for (const commandId of held) {
    await scope.act(async () => {
      scope.state.typeInto(`draft for ${commandId}`);
    });
    await scope.act(async () => {
      scope.state.send(commandId);
    });
    await scope.settle();
  }
  for (const commandId of held) {
    await deliver(await turnStageEvent({ commandId, status: "turn_queued" }));
  }

  // The oldest of the five is the one the provider gives up on.
  await deliver(
    await turnStageEvent({
      commandId: held[0],
      status: "turn_dropped",
      error: {
        code: "NO_LIVE_EXECUTION",
        message: "no execution is running for this session",
      },
    }),
  );
  assert.equal(
    scope.error(),
    "Turn dropped (NO_LIVE_EXECUTION): no execution is running for this session",
  );
  assert.equal(scope.draft(), `draft for ${held[0]}`);

  resetPendingCodingSessionTurns();
  scope.teardown();
});

const TARGET_KEY = "coding-session/v1|claude-agent-acp|instance|session|1";

test("a turn the provider is holding is watched again when the composer remounts", async () => {
  // The hazard this pins: a held row is exempt from the pending TTL *and* from
  // the refusal deadline, so the watch is the only thing left that can retire
  // it — and the watch lives in a component. Unmount the composer (switch
  // execution, close the panel, remount on a community switch) while a queued
  // turn waits behind an hour of work, let the execution die, and the
  // `turn_dropped` arrives with nobody listening: the row reads "Queued by the
  // provider" forever and the person's words are never given back.
  const store = await import("../lib/codingSessionPendingTurns.ts");
  store.resetPendingCodingSessionTurns();
  store.recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey: TARGET_KEY,
    commandId: TURN_COMMAND_ID,
    text: "run the whole suite",
    draft: "run the whole suite",
    operatorPubkey: null,
    recordedAt: Date.now(),
    published: true,
  });
  store.markPendingCodingSessionTurnQueued(CHANNEL_ID, TURN_COMMAND_ID);

  const queued = await turnStageEvent({ status: "turn_queued" });
  const dropped = await turnStageEvent({
    status: "turn_dropped",
    error: {
      code: "NO_LIVE_EXECUTION",
      message: "this execution has no live process",
    },
  });
  // The queued receipt is on the relay, which is where a remounted watch finds
  // it — that is what keeps the refusal deadline disarmed.
  const scope = await harness({ history: [queued], targetKey: TARGET_KEY });

  // Nothing was sent from this mount, and the turn is watched anyway.
  assert.equal(scope.state.isWatching, true);
  assert.equal(scope.liveSubscriptions.length, 1);

  await scope.act(async () => {
    scope.liveSubscriptions[0].onEvent(dropped);
  });
  await scope.settle();

  assert.equal(
    scope.error(),
    "Turn dropped (NO_LIVE_EXECUTION): this execution has no live process",
  );
  assert.equal(scope.draft(), "run the whole suite");
  assert.deepEqual(
    store.readPendingCodingSessionTurns(),
    [],
    "a terminal receipt retires the row it was published for, watcher or not",
  );

  store.resetPendingCodingSessionTurns();
  scope.teardown();
});
