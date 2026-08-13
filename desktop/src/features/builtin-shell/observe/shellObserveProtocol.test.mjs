import assert from "node:assert/strict";
import { test } from "node:test";

import { ObserveStream, parseShellFrame } from "./shellObserveProtocol.ts";

const OWNER = "deadbeef".repeat(8);
const SESSION = "sess-1";

// parseShellFrame decodes via window.atob; give the node test run a shim
// that rejects invalid input the way the browser's atob does.
globalThis.window ??= {};
globalThis.window.atob ??= (b64) => {
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(b64)) {
    throw new Error("invalid base64");
  }
  return Buffer.from(b64, "base64").toString("binary");
};

const b64 = (text) => Buffer.from(text, "utf8").toString("base64");

function frameEvent(overrides = {}) {
  const {
    type = "snap",
    seq = 1,
    epoch = "e1",
    dims = "24x80",
    content = b64("hello"),
    pubkey = OWNER,
    sessionId = SESSION,
    kind = 24311,
  } = overrides;
  const tags = [
    ["d", sessionId],
    ["a", `30621:${OWNER}:proj`],
    ["t", type],
    ["seq", String(seq)],
    ["epoch", epoch],
  ];
  if (dims) tags.push(["dims", dims]);
  return {
    id: "x",
    pubkey,
    created_at: 1,
    kind,
    tags,
    content,
    sig: "00",
  };
}

const expected = { ownerPubkey: OWNER, sessionId: SESSION };

test("parses a well-formed frame", () => {
  const frame = parseShellFrame(frameEvent(), expected);
  assert.equal(frame.type, "snap");
  assert.equal(frame.seq, 1);
  assert.equal(frame.epoch, "e1");
  assert.deepEqual(frame.dims, { rows: 24, cols: 80 });
  assert.equal(Buffer.from(frame.bytes).toString("utf8"), "hello");
});

test("rejects frames from the wrong author, session, kind, or shape", () => {
  assert.equal(
    parseShellFrame(frameEvent({ pubkey: "feedface".repeat(8) }), expected),
    null,
  );
  assert.equal(
    parseShellFrame(frameEvent({ sessionId: "other" }), expected),
    null,
  );
  assert.equal(parseShellFrame(frameEvent({ kind: 24310 }), expected), null);
  assert.equal(
    parseShellFrame(frameEvent({ type: "mystery" }), expected),
    null,
  );
  assert.equal(parseShellFrame(frameEvent({ seq: "NaN" }), expected), null);
  assert.equal(
    parseShellFrame(frameEvent({ content: "!!not-base64!!" }), expected),
    null,
  );
});

test("in-order tail, snap, and diff frames write through", () => {
  const stream = new ObserveStream();
  const tail = stream.apply(
    parseShellFrame(frameEvent({ type: "tail", seq: 1 }), expected),
  );
  assert.ok(tail.write);
  assert.equal(tail.needsResync, false);

  const snap = stream.apply(
    parseShellFrame(frameEvent({ type: "snap", seq: 2 }), expected),
  );
  assert.ok(snap.write);
  assert.deepEqual(snap.resize, { rows: 24, cols: 80 });

  const diff = stream.apply(
    parseShellFrame(frameEvent({ type: "diff", seq: 3, dims: null }), expected),
  );
  assert.ok(diff.write);
  assert.equal(diff.needsResync, false);
});

test("a seq gap suppresses diffs and asks for resync until the next snap", () => {
  const stream = new ObserveStream();
  stream.apply(parseShellFrame(frameEvent({ type: "snap", seq: 1 }), expected));

  // seq 2 dropped; 3 arrives.
  const gapped = stream.apply(
    parseShellFrame(frameEvent({ type: "diff", seq: 3, dims: null }), expected),
  );
  assert.equal(gapped.write, null);
  assert.equal(gapped.needsResync, true);

  // A resync snapshot repaints (with a leading clear) and re-arms diffs.
  const snap = stream.apply(
    parseShellFrame(frameEvent({ type: "snap", seq: 4 }), expected),
  );
  assert.ok(snap.write);
  // Clear prefix: ESC [ H ESC [ 2 J
  assert.deepEqual(
    [...snap.write.slice(0, 7)],
    [0x1b, 0x5b, 0x48, 0x1b, 0x5b, 0x32, 0x4a],
  );
  const diff = stream.apply(
    parseShellFrame(frameEvent({ type: "diff", seq: 5, dims: null }), expected),
  );
  assert.ok(diff.write);
});

test("an epoch change (broadcaster restart) forces a snapshot", () => {
  const stream = new ObserveStream();
  stream.apply(parseShellFrame(frameEvent({ type: "snap", seq: 9 }), expected));
  const diff = stream.apply(
    parseShellFrame(
      frameEvent({ type: "diff", seq: 1, epoch: "e2", dims: null }),
      expected,
    ),
  );
  assert.equal(diff.write, null);
  assert.equal(diff.needsResync, true);
});

test("stale replays are dropped silently and end ends", () => {
  const stream = new ObserveStream();
  stream.apply(parseShellFrame(frameEvent({ type: "snap", seq: 5 }), expected));
  const stale = stream.apply(
    parseShellFrame(frameEvent({ type: "diff", seq: 4, dims: null }), expected),
  );
  assert.equal(stale.write, null);
  assert.equal(stale.needsResync, false);

  const end = stream.apply(
    parseShellFrame(frameEvent({ type: "end", seq: 6, dims: null }), expected),
  );
  assert.equal(end.ended, true);
});

test("resize invalidates diffs until the following snap", () => {
  const stream = new ObserveStream();
  stream.apply(parseShellFrame(frameEvent({ type: "snap", seq: 1 }), expected));
  const resize = stream.apply(
    parseShellFrame(
      frameEvent({ type: "resize", seq: 2, dims: "40x120" }),
      expected,
    ),
  );
  assert.deepEqual(resize.resize, { rows: 40, cols: 120 });
  const diff = stream.apply(
    parseShellFrame(frameEvent({ type: "diff", seq: 3, dims: null }), expected),
  );
  assert.equal(diff.write, null);
  assert.equal(diff.needsResync, true);
});
