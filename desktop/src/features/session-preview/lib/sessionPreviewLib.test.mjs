/**
 * SV-33 S1/S2 lane L2: the Browser surface's pure pieces: rect sync
 * (last-write-wins), lease intersection, the sentences, recents, the floating
 * box. The native view itself is Rust's and is not exercised here.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  createPreviewRectSync,
  nextPreviewRectSeq,
} from "./previewRectSync.ts";
import {
  previewRectsIntersect,
  previewSlotOccluded,
} from "./previewOcclusion.ts";
import {
  SESSION_PREVIEW_LOCAL_ONLY_LABEL,
  SESSION_PREVIEW_UNAVAILABLE_SENTENCES,
  sessionPreviewBinding,
  sessionPreviewDriverName,
  sessionPreviewDrivingText,
  sessionPreviewNormalizeUrl,
} from "./previewModel.ts";
import {
  parseSessionPreviewRecents,
  rememberSessionPreviewRecent,
  SESSION_PREVIEW_RECENTS_MAX,
  withSessionPreviewRecent,
} from "./previewRecents.ts";
import {
  clampPreviewFloatBox,
  PREVIEW_FLOAT_MIN,
  previewFloatDrag,
} from "./previewFloatGeometry.ts";

function fakeFrames() {
  const queue = new Map();
  let next = 1;
  return {
    requestFrame: (callback) => {
      const handle = next++;
      queue.set(handle, callback);
      return handle;
    },
    cancelFrame: (handle) => queue.delete(handle),
    flush() {
      const callbacks = [...queue.values()];
      queue.clear();
      for (const callback of callbacks) callback();
    },
    get pending() {
      return queue.size;
    },
  };
}

const tick = () => new Promise((resolve) => setImmediate(resolve));
const rect = (x, y, width = 100, height = 50) => ({ x, y, width, height });

test("rect sync sends once per frame, newest rect only", async () => {
  const frames = fakeFrames();
  const sent = [];
  const sync = createPreviewRectSync({
    ...frames,
    send: async (r, seq) => sent.push({ r, seq }),
  });
  sync.schedule(rect(1, 1));
  sync.schedule(rect(2, 2));
  sync.schedule(rect(3.4, 3.6));
  assert.equal(frames.pending, 1);
  frames.flush();
  await tick();
  assert.deepEqual(sent, [{ r: rect(3, 4), seq: 1 }]);
  // An unchanged rect (after rounding) is not resent.
  sync.schedule(rect(3.2, 4.1));
  frames.flush();
  await tick();
  assert.equal(sent.length, 1);
});

test("rect sync: while a send is in flight, only the last rect follows it", async () => {
  const frames = fakeFrames();
  const sent = [];
  const releases = [];
  const sync = createPreviewRectSync({
    ...frames,
    send: (r, seq) =>
      new Promise((resolve) => {
        sent.push({ r, seq });
        releases.push(resolve);
      }),
  });
  sync.schedule(rect(10, 10));
  frames.flush();
  for (let x = 11; x <= 20; x += 1) sync.schedule(rect(x, 10));
  frames.flush();
  assert.equal(sent.length, 1, "nothing else while the first is in flight");
  releases[0]();
  await tick();
  frames.flush();
  assert.deepEqual(
    sent.map((s) => s.r.x),
    [10, 20],
  );
  assert.ok(sent[1].seq > sent[0].seq);
  releases[1]();
  await tick();
  sync.schedule(null);
  frames.flush();
  await tick();
  assert.equal(sent.at(-1).r, null, "unmount sends rect: null");
});

test("rect sync resends after a failed send, and stops when disposed", async () => {
  const frames = fakeFrames();
  let calls = 0;
  const errors = [];
  const sync = createPreviewRectSync({
    ...frames,
    send: async () => {
      calls += 1;
      if (calls === 1) throw new Error("ipc down");
    },
    onError: (error) => errors.push(error.message),
  });
  sync.schedule(rect(5, 5));
  frames.flush();
  await tick();
  sync.schedule(rect(5, 5));
  frames.flush();
  await tick();
  assert.equal(calls, 2, "a failed rect is not treated as applied");
  assert.deepEqual(errors, ["ipc down"]);
  sync.schedule(rect(6, 6));
  sync.dispose();
  frames.flush();
  await tick();
  assert.equal(calls, 2);
});

test("seq keeps rising across slots and remounts", () => {
  const a = nextPreviewRectSeq(1000);
  const b = nextPreviewRectSeq(1000);
  const c = nextPreviewRectSeq(5);
  assert.ok(b > a && c > b);
});

test("a lease occludes only when it overlaps the slot", () => {
  const slot = rect(100, 100, 400, 300);
  const lease = (r, extra = {}) => ({
    rect: r,
    containsSlot: false,
    isPreviewChrome: false,
    ...extra,
  });
  assert.equal(previewSlotOccluded(slot, []), false);
  assert.equal(previewSlotOccluded(slot, [lease(rect(0, 0, 50, 50))]), false);
  // Edge-touching is not overlap.
  assert.equal(previewRectsIntersect(slot, rect(500, 100, 10, 10)), false);
  assert.equal(
    previewSlotOccluded(slot, [lease(rect(480, 380, 50, 50))]),
    true,
  );
  // A dialog backdrop over the whole window covers it.
  assert.equal(
    previewSlotOccluded(slot, [lease(rect(0, 0, 2000, 2000))]),
    true,
  );
  // The sheet hosting the panel, and the preview's own chrome, do not.
  assert.equal(
    previewSlotOccluded(slot, [
      lease(rect(0, 0, 2000, 2000), { containsSlot: true }),
      lease(rect(90, 90, 500, 400), { isPreviewChrome: true }),
    ]),
    false,
  );
  // Not laid out yet: no box, no lease (re-checked every frame).
  assert.equal(previewSlotOccluded(slot, [lease(rect(0, 0, 0, 0))]), false);
  assert.equal(
    previewSlotOccluded(null, [lease(rect(0, 0, 2000, 2000))]),
    false,
  );
});

test("the sentences are WIRE-C4's, word for word", () => {
  assert.deepEqual(SESSION_PREVIEW_UNAVAILABLE_SENTENCES, {
    not_macos: "The Browser runs on macOS in this version.",
    no_session: "Open a session to use the Browser.",
    content_filter_failed:
      "The Browser could not start its local-only filter, so it stays off.",
    webview_failed: "The Browser could not start a web view on this computer.",
  });
  assert.equal(SESSION_PREVIEW_LOCAL_ONLY_LABEL, "Local only · not shared yet");
  assert.equal(
    sessionPreviewDrivingText("Reviewer"),
    "Agent (Reviewer) is driving · synthetic input",
  );
});

test("binding: focused or chosen or the only one; several asks; none is said", () => {
  const option = (key, sessionId) => ({
    executionKey: key,
    label: key.toUpperCase(),
    target: { driver: "d", instanceId: "i", sessionId, generation: 1 },
  });
  const one = [option("a", "S1")];
  const two = [option("a", "S1"), option("b", "S2")];
  const none = { focusedExecutionKey: null, chosenExecutionKey: null };
  assert.equal(sessionPreviewBinding({ options: one, ...none }).kind, "bound");
  assert.equal(sessionPreviewBinding({ options: two, ...none }).kind, "choose");
  assert.equal(sessionPreviewBinding({ options: [], ...none }).kind, "none");
  // A focus that names no option does not bind silently.
  assert.equal(
    sessionPreviewBinding({
      options: two,
      focusedExecutionKey: "gone",
      chosenExecutionKey: null,
    }).kind,
    "choose",
  );
  assert.equal(
    sessionPreviewDriverName({ sessionId: "S2", executionId: "exec-9" }, two),
    "B",
  );
  assert.equal(
    sessionPreviewDriverName(
      { sessionId: "0123456789abcdef", executionId: "x" },
      two,
    ),
    "session 01234567",
  );
});

test("typed addresses spell a local URL; policy stays Rust's", () => {
  assert.equal(sessionPreviewNormalizeUrl("5173"), "http://localhost:5173/");
  assert.equal(
    sessionPreviewNormalizeUrl(":3000/a"),
    "http://localhost:3000/a",
  );
  assert.equal(
    sessionPreviewNormalizeUrl("localhost:5173/x"),
    "http://localhost:5173/x",
  );
  assert.equal(
    sessionPreviewNormalizeUrl("127.0.0.1:8000"),
    "http://127.0.0.1:8000",
  );
  assert.equal(sessionPreviewNormalizeUrl("example.com"), "http://example.com");
  assert.equal(
    sessionPreviewNormalizeUrl("https://example.com"),
    "https://example.com",
    "an external URL is passed through for Rust to refuse, never rewritten",
  );
  assert.equal(sessionPreviewNormalizeUrl("about:blank"), "about:blank");
});

test("recents: newest first, deduplicated, capped, malformed dropped", () => {
  const store = new Map();
  const storage = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => store.set(key, value),
  };
  for (let i = 0; i < SESSION_PREVIEW_RECENTS_MAX + 3; i += 1) {
    rememberSessionPreviewRecent(
      { url: `http://localhost:${3000 + i}/`, title: null, at: i },
      storage,
    );
  }
  const list = rememberSessionPreviewRecent(
    { url: "http://localhost:3004/", title: "Again", at: 99 },
    storage,
  );
  assert.equal(list.length, SESSION_PREVIEW_RECENTS_MAX);
  assert.equal(list[0].title, "Again");
  assert.equal(
    list.filter((r) => r.url === "http://localhost:3004/").length,
    1,
  );
  assert.deepEqual(parseSessionPreviewRecents("{nope"), []);
  assert.deepEqual(
    parseSessionPreviewRecents(
      JSON.stringify([{ url: 1 }, { url: "u", at: 1 }]),
    ),
    [{ url: "u", title: null, at: 1 }],
  );
  assert.equal(
    withSessionPreviewRecent([], { url: "u", title: null, at: 1 }).length,
    1,
  );
});

test("the floating box stays in the window and above its minimum", () => {
  const viewport = { width: 1280, height: 720 };
  const start = { x: 600, y: 300, width: 400, height: 300 };
  assert.deepEqual(previewFloatDrag(start, "move", 50, -20, viewport), {
    x: 650,
    y: 280,
    width: 400,
    height: 300,
  });
  const dragged = previewFloatDrag(start, "move", 5000, 5000, viewport);
  assert.equal(dragged.x + dragged.width, viewport.width - 8);
  assert.equal(dragged.y + dragged.height, viewport.height - 8);
  const shrunk = previewFloatDrag(start, "resize", -1000, -1000, viewport);
  assert.equal(shrunk.width, PREVIEW_FLOAT_MIN.width);
  assert.equal(shrunk.height, PREVIEW_FLOAT_MIN.height);
  const huge = clampPreviewFloatBox(
    { x: -50, y: -50, width: 9000, height: 9000 },
    viewport,
  );
  assert.deepEqual(huge, { x: 8, y: 8, width: 1264, height: 704 });
});
