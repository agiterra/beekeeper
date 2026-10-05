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

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

// A channel UUID: the 44229 envelope rule (relay and every reader) requires one.
const CHANNEL_ID = "6f1d2c3b-4a59-4e8d-9c7b-1a2b3c4d5e6f";
const SESSION_REF = "9c4e2b10-3f5a-4d72-8b16-2e9a7d4c1f08";
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);

async function nameEvent(content, createdAt) {
  const { buildCodingSessionNameEvent } = await import(
    "./lib/codingSessionName.ts"
  );
  const built = buildCodingSessionNameEvent({
    channelId: CHANNEL_ID,
    content,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  );
}

test("an accepted local rename updates every mounted name consumer without a live echo", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionNameKey, publishCodingSessionName } = await import(
    "./lib/codingSessionName.ts"
  );
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Initial name", 1_800_000_000);
  const renamed = await nameEvent("Renamed everywhere", 1_800_000_001);
  const subscriptions = [];
  const client = {
    publishEvent: async () => renamed,
    fetchEventsCoalesced: async () => [initial],
    subscribeLive: async (_filter, onEvent) => {
      subscriptions.push(onEvent);
      return () => {};
    },
    subscribeToReconnects: () => () => {},
  };
  const first = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));
  const second = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));

  await act(async () => {});
  const key = codingSessionNameKey(CHANNEL_ID, SESSION_REF, FOUNDER_PUBKEY);
  assert.equal(first.result.current.names.get(key)?.content, "Initial name");
  assert.equal(second.result.current.names.get(key)?.content, "Initial name");
  assert.equal(subscriptions.length, 2);

  await act(async () => {
    await publishCodingSessionName(
      {
        channelId: CHANNEL_ID,
        content: "Renamed everywhere",
        sessionRef: SESSION_REF,
      },
      {
        signer: async () => renamed,
        publisher: client,
      },
    );
  });

  assert.equal(
    first.result.current.names.get(key)?.content,
    "Renamed everywhere",
  );
  assert.equal(
    second.result.current.names.get(key)?.content,
    "Renamed everywhere",
  );

  first.unmount();
  second.unmount();
});

test("immediate history and bounded live replay cover an attaching watch", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { codingSessionNameKey } = await import("./lib/codingSessionName.ts");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Between history and watch", 1_800_000_010);
  let deliverLive;
  const filters = [];
  const client = {
    fetchEventsCoalesced: async (filter) => {
      filters.push(filter);
      return [];
    },
    subscribeLive: (filter, onEvent) => {
      filters.push(filter);
      deliverLive = onEvent;
      return new Promise(() => {});
    },
  };
  const { result, unmount } = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], client),
  );
  await act(async () => {});
  assert.equal(
    result.current.resolved,
    true,
    "HTTP history does not wait on live admission",
  );
  assert.deepEqual(filters, [
    { kinds: [44229, 44252], "#h": [CHANNEL_ID], limit: 1000 },
    { kinds: [44229, 44252], "#h": [CHANNEL_ID], limit: 1000 },
  ]);
  await act(async () => deliverLive(initial));
  assert.equal(
    result.current.names.get(
      codingSessionNameKey(CHANNEL_ID, SESSION_REF, FOUNDER_PUBKEY),
    )?.content,
    "Between history and watch",
  );
  unmount();
});

test("resolved is false until the history read settles, and true on success or on error", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Settled name", 1_800_000_020);
  let finishHistory;
  const okClient = {
    fetchEventsCoalesced: () =>
      new Promise((resolve) => {
        finishHistory = resolve;
      }),
    subscribeLive: () => new Promise(() => {}),
    subscribeToReconnects: () => () => {},
  };
  const ok = renderHook(() => useCodingSessionNames([CHANNEL_ID], okClient));
  // First render: nothing has been read, so nothing may be published over it.
  assert.equal(ok.result.current.resolved, false);
  await act(async () => {});
  assert.equal(ok.result.current.resolved, false);
  await act(async () => finishHistory([initial]));
  assert.equal(ok.result.current.resolved, true);
  assert.equal(ok.result.current.errorMessage, null);
  ok.unmount();

  // A refusal is an answer: the read is over either way.
  const failingClient = {
    fetchEventsCoalesced: async () => {
      throw new Error("forbidden: not a member of this channel");
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };
  const failed = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], failingClient),
  );
  assert.equal(failed.result.current.resolved, false);
  await act(async () => {});
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(failed.result.current.resolved, true);
  assert.match(failed.result.current.errorMessage ?? "", /forbidden/);
  assert.match(failed.result.current.readErrorMessage ?? "", /forbidden/);
  failed.unmount();

  // Nothing to read is the one scope that starts settled.
  const empty = renderHook(() => useCodingSessionNames([], okClient));
  await act(async () => {});
  assert.equal(empty.result.current.resolved, true);
  empty.unmount();
});

test("refresh retries a failed watch, preserves signed names, and discloses a full history window", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Known name", 1_800_000_020);
  const fullHistory = Array.from({ length: 1000 }, (_, index) =>
    finalizeEvent(
      { ...initial, created_at: initial.created_at + index },
      FOUNDER_SECRET,
    ),
  );
  let full = true,
    attempts = 0;
  const client = {
    fetchEventsCoalesced: async () => (full ? fullHistory : []),
    subscribeLive: async () => {
      attempts++;
      if (attempts === 1) throw new Error("watch failed");
      return () => {};
    },
  };
  const { result, unmount } = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], client),
  );
  await act(async () => {});
  assert.match(result.current.readErrorMessage, /latest 1000/);
  assert.match(result.current.errorMessage, /watch failed/);
  assert.equal(result.current.names.size, 1);
  const refresh = result.current.refresh;
  full = false;
  await act(async () => refresh());
  assert.equal(result.current.refresh, refresh);
  assert.equal(attempts, 2);
  assert.equal(result.current.readErrorMessage, null);
  assert.equal(result.current.errorMessage, null);
  assert.equal(result.current.names.size, 1);
  unmount();
});

test("scope disposal ignores late history and unsubscribes a pending old watch", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const initial = await nameEvent("Old channel", 1_800_000_020);
  const reads = [],
    watches = [];
  let closed = false,
    connected;
  const client = {
    fetchEventsCoalesced: (filter) =>
      new Promise((resolve) => reads.push({ filter, resolve })),
    subscribeLive: () => new Promise((resolve) => watches.push(resolve)),
    subscribeToConnectionState: (fn) => {
      connected = fn;
      return () => {};
    },
  };
  const { result, rerender, unmount } = renderHook(
    ({ channel }) => useCodingSessionNames([channel], client),
    { initialProps: { channel: CHANNEL_ID } },
  );
  rerender({ channel: "other-channel" });
  await act(async () => {
    reads[0].resolve([initial]);
    watches[0](() => {
      closed = true;
    });
    reads[1].resolve([]);
  });
  assert.equal(closed, true);
  assert.equal(result.current.names.size, 0);
  await act(async () => connected("connected"));
  assert.deepEqual(reads[2].filter["#h"], ["other-channel"]);
  unmount();
});

test("a held rename cannot enter a remounted community with the same client and coordinates", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const { publishCodingSessionName } = await import(
    "./lib/codingSessionName.ts"
  );
  const signed = await nameEvent("Old community result", 1_800_000_030);
  let finish, notifyStarted;
  const started = new Promise((resolve) => (notifyStarted = resolve));
  const client = {
    fetchEventsCoalesced: async () => [],
    subscribeLive: async () => () => {},
    publishEvent: () => {
      notifyStarted();
      return new Promise((resolve) => (finish = resolve));
    },
  };
  const old = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));
  await act(async () => {});
  const pending = publishCodingSessionName(
    { channelId: CHANNEL_ID, sessionRef: SESSION_REF, content: signed.content },
    { signer: async () => signed, publisher: client },
  );
  await started;
  old.unmount();
  const next = renderHook(() => useCodingSessionNames([CHANNEL_ID], client));
  await act(async () => {
    finish(signed);
    await pending;
  });
  assert.equal(next.result.current.names.size, 0);
  next.unmount();
});

// ── SV-31: generated titles beside a person's name ─────────────────────────

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const SECOND_PROVIDER_SECRET = generateSecretKey();
const STRANGER_SECRET = generateSecretKey();
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "inst-1",
  sessionId: "sess-1",
  generation: 1,
};
const SECOND_TARGET = { ...TARGET, driver: "codex-acp", sessionId: "sess-2" };

function targetKey(target) {
  return `coding-session/v1|${[
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  ]
    .map((field) => `${new TextEncoder().encode(field).byteLength}:${field}`)
    .join("")}`;
}

function titleEvent(secret, title, createdAt, target = TARGET) {
  return finalizeEvent(
    {
      kind: 44252,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["cstl-v", "cstl1-1"],
        ["cs-target", targetKey(target)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-title/v1",
        title,
        model: "claude-haiku-4-5",
        basis: "first-message",
        sourceCommand: null,
        createEventId: "ca".repeat(32),
      }),
    },
    secret,
  );
}

async function metadataEvent(secret, target = TARGET) {
  const { codingSessionMetadataSemanticKey } = await import(
    "./lib/codingSessionIngressPayloads.ts"
  );
  return finalizeEvent(
    {
      kind: 44223,
      created_at: 1_800_000_000,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", "csm1-1"],
        ["cs-target", targetKey(target)],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-metadata/v1",
        session: target,
        projectRef: null,
        repoRef: null,
        title: "Founding execution title",
        agentRef: null,
        provider: "claude",
        runtime: "claude",
        model: null,
        status: "running",
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: true,
          plan: true,
        },
        sessionRef: SESSION_REF,
      }),
    },
    secret,
  );
}

async function receiptEvent(secret, target = TARGET, status = "created") {
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    codingSessionReceiptSemanticKey,
  } = await import("./lib/codingSessionIngressPayloads.ts");
  const commandId = `cmd-${status}`;
  return finalizeEvent(
    {
      kind: 44224,
      created_at: 1_799_999_999,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", "cslr1-1"],
        ["csl-command", commandId],
        ["csl-key", codingSessionReceiptSemanticKey(commandId, status)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: target,
        error: null,
      }),
    },
    secret,
  );
}

/** A confirmed execution: the signer's 44223 and its `created` receipt. */
async function executionEvents(secret, target = TARGET) {
  return [
    await metadataEvent(secret, target),
    await receiptEvent(secret, target),
  ];
}

/** A client serving names/titles, and 44223/44224 standing by kind and author. */
function titleClient({ names, metadata }) {
  const filters = [];
  return {
    filters,
    fetchEventsCoalesced: async (filter) => {
      filters.push(filter);
      if (filter.kinds.includes(44223) || filter.kinds.includes(44224)) {
        return metadata
          .filter(
            (event) =>
              filter.kinds.includes(event.kind) &&
              (filter.authors ?? []).includes(event.pubkey) &&
              (filter.until === undefined || event.created_at <= filter.until),
          )
          .sort((left, right) => right.created_at - left.created_at)
          .slice(0, filter.limit);
      }
      return names;
    },
    subscribeLive: async () => () => {},
    subscribeToReconnects: () => () => {},
  };
}

async function renderNames(client) {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionNames } = await import("./useCodingSessionNames.ts");
  const rendered = renderHook(() =>
    useCodingSessionNames([CHANNEL_ID], client),
  );
  await act(async () => {});
  await act(async () => {});
  return rendered;
}

async function founderKey() {
  const { codingSessionNameKey } = await import("./lib/codingSessionName.ts");
  return codingSessionNameKey(CHANNEL_ID, SESSION_REF, FOUNDER_PUBKEY);
}

test("an older person name beats a newer generated title, and personNames holds only it", async () => {
  const person = await nameEvent("Auth rework", 1_800_000_100);
  const title = titleEvent(
    PROVIDER_SECRET,
    "Login redirect fix",
    1_800_000_200,
  );
  const client = titleClient({
    names: [title, person],
    metadata: [...(await executionEvents(PROVIDER_SECRET))],
  });
  const { result, unmount } = await renderNames(client);
  const key = await founderKey();
  assert.equal(result.current.names.get(key)?.content, "Auth rework");
  assert.equal(result.current.names.get(key)?.origin, "person");
  assert.equal(result.current.names.get(key)?.model, null);
  assert.equal(result.current.personNames.get(key)?.content, "Auth rework");
  // The standing reads name their kinds and only the signer they vouch for,
  // in the title's channel, ending at the title: metadata, then the
  // lifecycle receipts that confirm it.
  assert.deepEqual(client.filters.at(-2), {
    kinds: [44223],
    "#h": [CHANNEL_ID],
    authors: [PROVIDER_PUBKEY],
    until: 1_800_000_260,
    limit: 1000,
  });
  assert.deepEqual(client.filters.at(-1), {
    kinds: [44224],
    "#h": [CHANNEL_ID],
    authors: [PROVIDER_PUBKEY],
    until: 1_800_000_260,
    limit: 1000,
  });
  unmount();
});

test("a generated title shows with its model and signer, but is never a person's name", async () => {
  const title = titleEvent(
    PROVIDER_SECRET,
    "Login redirect fix",
    1_800_000_200,
  );
  const { result, unmount } = await renderNames(
    titleClient({
      names: [title],
      metadata: [...(await executionEvents(PROVIDER_SECRET))],
    }),
  );
  const key = await founderKey();
  const shown = result.current.names.get(key);
  assert.equal(shown?.content, "Login redirect fix");
  assert.equal(shown?.origin, "generated");
  assert.equal(shown?.model, "claude-haiku-4-5");
  assert.equal(shown?.signerPubkey, PROVIDER_PUBKEY);
  assert.equal(shown?.eventId, title.id);
  assert.equal(result.current.personNames.get(key), undefined);
  assert.equal(result.current.personNames.size, 0);
  unmount();
});

test("the earliest generated title wins, so a shown title never flips", async () => {
  const later = titleEvent(PROVIDER_SECRET, "Later title", 1_800_000_300);
  const earlier = titleEvent(
    SECOND_PROVIDER_SECRET,
    "Earlier title",
    1_800_000_200,
    SECOND_TARGET,
  );
  const { result, unmount } = await renderNames(
    titleClient({
      names: [later, earlier],
      metadata: [
        ...(await executionEvents(PROVIDER_SECRET)),
        ...(await executionEvents(SECOND_PROVIDER_SECRET, SECOND_TARGET)),
      ],
    }),
  );
  assert.equal(
    result.current.names.get(await founderKey())?.content,
    "Earlier title",
  );
  unmount();
});

test("a foreign or non-execution signer's title is ignored and counted", async () => {
  // The stranger signed no 44223 at all; the second provider signed one for
  // its own target, not for the target this title claims.
  const stranger = titleEvent(STRANGER_SECRET, "Stranger title", 1_800_000_100);
  const crossed = titleEvent(
    SECOND_PROVIDER_SECRET,
    "Crossed title",
    1_800_000_150,
  );
  const { result, unmount } = await renderNames(
    titleClient({
      names: [stranger, crossed],
      metadata: [
        ...(await executionEvents(SECOND_PROVIDER_SECRET, SECOND_TARGET)),
      ],
    }),
  );
  assert.equal(result.current.names.get(await founderKey()), undefined);
  assert.equal(result.current.generatedTitles.size, 0);
  assert.deepEqual(result.current.titleDiagnostics, {
    foreignTitles: 2,
    malformed: 0,
  });
  unmount();
});

test("a signer's 44223 with no lifecycle receipt is no execution: its title is counted, never shown", async () => {
  const title = titleEvent(
    PROVIDER_SECRET,
    "Login redirect fix",
    1_800_000_200,
  );
  const { result, unmount } = await renderNames(
    titleClient({
      names: [title],
      metadata: [await metadataEvent(PROVIDER_SECRET)],
    }),
  );
  assert.equal(result.current.names.get(await founderKey()), undefined);
  assert.equal(result.current.generatedTitles.size, 0);
  assert.deepEqual(result.current.titleDiagnostics, {
    foreignTitles: 1,
    malformed: 0,
  });
  unmount();
});

test("a failed standing read hides titles and says so", async () => {
  const title = titleEvent(
    PROVIDER_SECRET,
    "Login redirect fix",
    1_800_000_200,
  );
  const client = {
    fetchEventsCoalesced: async (filter) => {
      if (filter.kinds.includes(44223)) throw new Error("relay refused");
      return [title];
    },
    subscribeLive: async () => () => {},
  };
  const { result, unmount } = await renderNames(client);
  assert.equal(result.current.names.get(await founderKey()), undefined);
  assert.match(result.current.errorMessage ?? "", /relay refused/);
  // The names read itself settled cleanly: nothing gates on standing.
  assert.equal(result.current.readErrorMessage, null);
  assert.equal(result.current.resolved, true);
  unmount();
});

test("a busy provider's 1000+ later turn receipts never unseat an old session's title", async () => {
  const title = titleEvent(
    PROVIDER_SECRET,
    "Login redirect fix",
    1_800_000_200,
  );
  const later = [];
  for (let index = 0; index < 1_200; index += 1) {
    const turn = {
      id: index.toString(16).padStart(64, "0"),
      pubkey: PROVIDER_PUBKEY,
      kind: 44224,
      created_at: 1_800_100_000 + index,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", "cslr1-1"],
        ["csl-command", `turn-${index}`],
        ["csl-key", "k"],
      ],
      content: "{}",
      sig: "00".repeat(64),
    };
    later.push(turn, { ...turn, id: `m${turn.id.slice(1)}`, kind: 44223 });
  }
  const client = titleClient({
    names: [title],
    metadata: [...(await executionEvents(PROVIDER_SECRET)), ...later],
  });
  const { result, unmount } = await renderNames(client);
  assert.equal(
    result.current.names.get(await founderKey())?.content,
    "Login redirect fix",
  );
  assert.deepEqual(result.current.titleDiagnostics, {
    foreignTitles: 0,
    malformed: 0,
  });
  assert.equal(result.current.errorMessage, null);
  unmount();
});
