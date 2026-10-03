/**
 * A burst of relay events publishes once per animation frame, not once per
 * event — and a frame-less environment keeps the old, synchronous behaviour.
 *
 * The burst test mounts the REAL ingress hook (real store, real signature
 * verification) against a fake relay client and counts publishes by counting
 * `TrustedCodingSessionIngressStore#snapshot` calls: every publish takes
 * exactly one snapshot, and nothing else in the live path does.
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

/** A hand-stepped frame clock installed on globalThis. */
function installFakeFrames() {
  let nextId = 1;
  const queue = new Map();
  globalThis.requestAnimationFrame = (callback) => {
    const id = nextId++;
    queue.set(id, callback);
    return id;
  };
  globalThis.cancelAnimationFrame = (id) => {
    queue.delete(id);
  };
  return {
    pending: () => queue.size,
    step() {
      const callbacks = [...queue.values()];
      queue.clear();
      for (const callback of callbacks) callback(16);
    },
    uninstall() {
      delete globalThis.requestAnimationFrame;
      delete globalThis.cancelAnimationFrame;
    },
  };
}

test("the coalescer folds any number of schedules into one frame", async () => {
  const { createCodingSessionIngressPublishCoalescer } = await import(
    "./useTrustedCodingSessionIngressPublish.ts"
  );
  const frames = installFakeFrames();
  try {
    let published = 0;
    const coalescer = createCodingSessionIngressPublishCoalescer(() => {
      published += 1;
    });
    for (let index = 0; index < 50; index += 1) coalescer.schedule();
    assert.equal(published, 0, "nothing publishes before the frame");
    assert.equal(frames.pending(), 1, "fifty schedules request one frame");
    frames.step();
    assert.equal(published, 1);

    // flushNow publishes at once and absorbs the requested frame.
    coalescer.schedule();
    coalescer.flushNow();
    assert.equal(published, 2);
    assert.equal(frames.pending(), 0);

    // cancel drops the frame without publishing.
    coalescer.schedule();
    coalescer.cancel();
    frames.step();
    assert.equal(published, 2);
  } finally {
    frames.uninstall();
  }
});

test("without animation frames, every schedule publishes synchronously", async () => {
  const { createCodingSessionIngressPublishCoalescer } = await import(
    "./useTrustedCodingSessionIngressPublish.ts"
  );
  assert.equal(typeof globalThis.requestAnimationFrame, "undefined");
  let published = 0;
  const coalescer = createCodingSessionIngressPublishCoalescer(() => {
    published += 1;
  });
  coalescer.schedule();
  coalescer.schedule();
  assert.equal(published, 2);
});

test("a hidden window that never paints still publishes", async () => {
  const { createCodingSessionIngressPublishCoalescer } = await import(
    "./useTrustedCodingSessionIngressPublish.ts"
  );
  const frames = installFakeFrames();
  try {
    let published = 0;
    const coalescer = createCodingSessionIngressPublishCoalescer(() => {
      published += 1;
    });
    coalescer.schedule();
    await new Promise((resolve) => setTimeout(resolve, 300));
    assert.equal(published, 1, "the fallback timer published");
    frames.step();
    assert.equal(published, 1, "the late frame does not publish twice");
  } finally {
    frames.uninstall();
  }
});

test("an unchanged snapshot keeps its arrays and entries; an append keeps the prefix", async () => {
  const { reuseCodingSessionIngressSnapshotArrays } = await import(
    "./useTrustedCodingSessionIngressPublish.ts"
  );
  const transcript = (n) => ({ eventSeq: n });
  const payloads = [transcript(1), transcript(2), transcript(3)];
  const metadataPayload = { title: "t" };
  const build = (count) => ({
    metadata: [
      {
        channelId: "c",
        targetKey: "k",
        signerPubkey: "s",
        metadata: metadataPayload,
        eventId: "m1",
        createdAt: 1,
        conflictCount: 0,
      },
    ],
    transcripts: payloads.slice(0, count).map((payload, index) => ({
      channelId: "c",
      targetKey: "k",
      signerPubkey: "s",
      transcript: payload,
      eventId: `t${index}`,
      createdAt: index,
      conflictCount: 0,
    })),
    malformedCount: 0,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
  });
  const first = build(2);
  const same = reuseCodingSessionIngressSnapshotArrays(first, build(2));
  assert.equal(same.metadata, first.metadata);
  assert.equal(same.transcripts, first.transcripts);

  const appended = reuseCodingSessionIngressSnapshotArrays(first, build(3));
  assert.equal(appended.metadata, first.metadata, "metadata untouched");
  assert.notEqual(appended.transcripts, first.transcripts);
  assert.equal(appended.transcripts.length, 3);
  assert.equal(appended.transcripts[0], first.transcripts[0]);
  assert.equal(appended.transcripts[1], first.transcripts[1]);
});

test("a burst of live events: one publish per event without frames, one per frame with them", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { KIND_CODING_SESSION_TRANSCRIPT } = await import(
    "@/shared/constants/kinds.ts"
  );
  const { buildCodingSessionTargetKey } = await import(
    "./codingSessionCommand.ts"
  );
  const {
    BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    CODING_SESSION_TRANSCRIPT_TAG_VERSION,
    codingSessionTranscriptSemanticKey,
    TrustedCodingSessionIngressStore,
  } = await import("./codingSessionTrustedIngress.ts");
  const { useTrustedCodingSessionIngress } = await import(
    "./useTrustedCodingSessionIngress.ts"
  );

  ipcHandlers.set("get_global_agent_config", async () => ({
    env_vars: {},
    provider: null,
    model: null,
    preferred_runtime: null,
    "allowed-bridge-pubkeys": [],
  }));

  const BURST = 40;
  const FRAMES = 4;
  let snapshots = 0;
  const original = TrustedCodingSessionIngressStore.prototype.snapshot;
  TrustedCodingSessionIngressStore.prototype.snapshot = function (...args) {
    snapshots += 1;
    return original.apply(this, args);
  };

  const run = async (channelId) => {
    const secret = generateSecretKey();
    const pubkey = getPublicKey(secret);
    const target = {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "11111111-2222-3333-4444-555555555555",
      generation: 1,
    };
    const events = Array.from({ length: BURST }, (_, index) =>
      finalizeEvent(
        {
          kind: KIND_CODING_SESSION_TRANSCRIPT,
          created_at: 1_800_000_010 + index,
          tags: [
            ["h", channelId],
            ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
            ["cs-target", buildCodingSessionTargetKey(target)],
            ["cst-seq", String(index + 1)],
            ["cst-key", codingSessionTranscriptSemanticKey(target, index + 1)],
          ],
          content: JSON.stringify({
            schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
            session: target,
            eventSeq: index + 1,
            timestamp: 1_800_000_010_000 + index,
            turnId: "turn-1",
            item: { kind: "assistant_text", text: `paragraph ${index}` },
          }),
        },
        secret,
      ),
    );
    const live = [];
    const client = {
      fetchEvents: async () => [],
      subscribeLive: async (_filter, onEvent) => {
        live.push(onEvent);
        return () => {};
      },
      subscribeToReconnects: () => () => {},
    };
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });
    const wrapper = ({ children }) =>
      React.createElement(
        QueryClientProvider,
        { client: queryClient },
        children,
      );
    let renders = 0;
    const { result, unmount } = renderHook(
      () => {
        renders += 1;
        return useTrustedCodingSessionIngress(
          [channelId],
          null,
          pubkey,
          client,
          null,
          "pinned",
        );
      },
      { wrapper },
    );
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
    assert.equal(live.length, 1, "live subscription armed");
    const snapshotsBefore = snapshots;
    const rendersBefore = renders;
    return {
      /** Deliver the burst in `frameCount` frames, `onFrame` between them. */
      async burst(frameCount, onFrame) {
        const perFrame = Math.ceil(events.length / frameCount);
        for (let frame = 0; frame < frameCount; frame += 1) {
          await act(async () => {
            for (const event of events.slice(
              frame * perFrame,
              (frame + 1) * perFrame,
            )) {
              live[0](event);
            }
            onFrame?.();
          });
        }
        return {
          publishes: snapshots - snapshotsBefore,
          renders: renders - rendersBefore,
          transcripts: result.current.transcripts.length,
        };
      },
      done() {
        unmount();
        queryClient.clear();
      },
    };
  };

  try {
    // Before: no frame clock, the synchronous fallback — the behaviour this
    // change replaced, one publish per event.
    const before = await run("burst-channel-sync");
    const sync = await before.burst(FRAMES);
    before.done();
    assert.equal(sync.publishes, BURST);
    assert.equal(sync.transcripts, BURST);

    // After: the same burst publishes once per frame.
    const frames = installFakeFrames();
    try {
      const afterRun = await run("burst-channel-frame");
      const framed = await afterRun.burst(FRAMES, () => frames.step());
      afterRun.done();
      assert.equal(framed.publishes, FRAMES);
      assert.equal(framed.transcripts, BURST, "the last publish has them all");
      console.log(
        `ingress burst of ${BURST} events over ${FRAMES} frames: publishes (store snapshots + setState) ${sync.publishes} -> ${framed.publishes}`,
      );
    } finally {
      frames.uninstall();
    }
  } finally {
    TrustedCodingSessionIngressStore.prototype.snapshot = original;
    ipcHandlers.clear();
  }
});
