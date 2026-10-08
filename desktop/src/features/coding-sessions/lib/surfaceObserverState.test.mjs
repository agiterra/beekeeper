import assert from "node:assert/strict";
import test from "node:test";

import {
  initialSurfaceObserverState,
  parseSurfaceFrame,
  surfaceObserverApplyFrame,
  surfaceObserverBeat,
  surfaceObserverCanRequestSnapshot,
  surfaceObserverSnapshotAnswered,
  surfaceObserverSnapshotRequested,
  surfaceObserverTick,
  surfaceObserverWatchSent,
} from "./surfaceObserverState.ts";
import {
  CHANNEL,
  frame,
  OTHER,
  PROVIDER,
  SLOT,
} from "./surfaceFixtures.testFixtures.mjs";

const EXPECT = {
  channelId: CHANNEL,
  surface: "device",
  key: SLOT,
  producerPubkey: PROVIDER,
};
const NOW = 1_791_374_500_000;

function apply(state, options, now = NOW) {
  const parsed = parseSurfaceFrame(
    frame({ capturedAt: now - 1_000, ...options }),
    EXPECT,
  );
  assert.ok(parsed, "fixture frame parses");
  return surfaceObserverApplyFrame(state, parsed, now);
}

test("a frame parses to a data URL; a non-authority author, another key or a reordered tag is dropped", () => {
  const parsed = parseSurfaceFrame(frame({ capturedAt: NOW }), EXPECT);
  assert.equal(parsed.dataUrl, "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ==");
  assert.equal(parsed.cadenceMs, 3000);
  assert.equal(
    parseSurfaceFrame(frame({ capturedAt: NOW, author: OTHER }), EXPECT),
    null,
  );
  assert.equal(
    parseSurfaceFrame(
      frame({ capturedAt: NOW, key: "0123456789abcdef" }),
      EXPECT,
    ),
    null,
  );
  const reordered = frame({ capturedAt: NOW });
  [reordered.tags[4], reordered.tags[5]] = [
    reordered.tags[5],
    reordered.tags[4],
  ];
  assert.equal(parseSurfaceFrame(reordered, EXPECT), null);
  const leadingZero = frame({ capturedAt: NOW });
  leadingZero.tags[4] = ["seq", "01"];
  assert.equal(parseSurfaceFrame(leadingZero, EXPECT), null);
  const notJpeg = frame({ capturedAt: NOW });
  notJpeg.content = "iVBORw0KGgo=";
  assert.equal(parseSurfaceFrame(notJpeg, EXPECT), null);
  const pausedWithBody = frame({ capturedAt: NOW, t: "paused" });
  pausedWithBody.content = "x";
  assert.equal(parseSurfaceFrame(pausedWithBody, EXPECT), null);
});

test("connecting until the first frame; no frame 10 s after the first watch is not-streaming", () => {
  let state = surfaceObserverWatchSent(initialSurfaceObserverState(), NOW);
  assert.equal(surfaceObserverTick(state, NOW + 9_999).status, "connecting");
  state = surfaceObserverTick(state, NOW + 10_000);
  assert.equal(state.status, "not-streaming");
  // A later watch does not move the handshake.
  assert.equal(surfaceObserverWatchSent(state, NOW + 20_000).watchSentAt, NOW);
});

test("a fresh frame is live; over 30 s old it is stalled, never Live", () => {
  let state = apply(initialSurfaceObserverState(), { seq: 1 });
  assert.equal(state.status, "live");
  assert.equal(surfaceObserverTick(state, NOW + 29_000).status, "live");
  state = surfaceObserverTick(state, NOW + 31_001);
  assert.equal(state.status, "stalled");
  assert.ok(state.frame, "the stale frame is kept to show dimmed");
  // A frame captured 31 s ago is stalled the moment it arrives.
  const late = apply(initialSurfaceObserverState(), {
    seq: 1,
    capturedAt: NOW - 31_000,
  });
  assert.equal(late.status, "stalled");
  // A producer clock running ahead cannot make a frame newer than its arrival.
  const ahead = apply(initialSurfaceObserverState(), {
    seq: 1,
    capturedAt: NOW + 60_000,
  });
  assert.equal(ahead.frameAt, NOW);
  assert.equal(surfaceObserverTick(ahead, NOW + 31_000).status, "stalled");
});

test("seq at or below the last in an epoch is ignored; a newer epoch resets; an older epoch is dropped", () => {
  let state = apply(initialSurfaceObserverState(), { seq: 5, epoch: 100 });
  assert.equal(apply(state, { seq: 5, epoch: 100 }), state);
  assert.equal(apply(state, { seq: 4, epoch: 100 }), state);
  state = apply(state, { seq: 6, epoch: 100 });
  assert.equal(state.seq, 6);
  state = apply(state, { seq: 1, epoch: 200 });
  assert.equal(state.epoch, 200);
  assert.equal(state.seq, 1);
  assert.equal(apply(state, { seq: 9, epoch: 100 }), state);
});

test("t=paused is paused and t=end is ended, held across ticks until a frame", () => {
  let state = apply(initialSurfaceObserverState(), { seq: 1 });
  state = apply(state, { seq: 2, t: "paused" });
  assert.equal(state.status, "paused");
  assert.equal(surfaceObserverTick(state, NOW + 60_000).status, "paused");
  state = apply(state, { seq: 3 });
  assert.equal(state.status, "live");
  state = apply(state, { seq: 4, t: "end" });
  assert.equal(state.status, "ended");
  assert.equal(surfaceObserverTick(state, NOW + 60_000).status, "ended");
});

test("a beat with no arrival since the last one sends resync, else watch", () => {
  let state = initialSurfaceObserverState();
  assert.equal(surfaceObserverBeat(state).action, "resync");
  state = apply(state, { seq: 1 });
  const beat = surfaceObserverBeat(state);
  assert.equal(beat.action, "watch");
  assert.equal(surfaceObserverBeat(beat.state).action, "resync");
});

test("snapshot requests: at most one per 10 s; unanswered after 15 s; answered by a newer 44253", () => {
  let state = initialSurfaceObserverState();
  assert.ok(surfaceObserverCanRequestSnapshot(state, NOW));
  state = surfaceObserverSnapshotRequested(state, NOW);
  assert.equal(surfaceObserverCanRequestSnapshot(state, NOW + 9_999), false);
  assert.ok(surfaceObserverCanRequestSnapshot(state, NOW + 10_000));
  assert.equal(
    surfaceObserverTick(state, NOW + 14_000).snapshot.timedOut,
    false,
  );
  const timedOut = surfaceObserverTick(state, NOW + 15_000);
  assert.equal(timedOut.snapshot.timedOut, true);
  assert.equal(timedOut.snapshot.pendingSince, null);
  // An old snapshot does not answer; one taken after the request does.
  assert.equal(surfaceObserverSnapshotAnswered(state, NOW - 60_000), state);
  const answered = surfaceObserverSnapshotAnswered(state, NOW + 2_000);
  assert.equal(answered.snapshot.pendingSince, null);
  assert.equal(
    surfaceObserverTick(answered, NOW + 20_000).snapshot.timedOut,
    false,
  );
});
