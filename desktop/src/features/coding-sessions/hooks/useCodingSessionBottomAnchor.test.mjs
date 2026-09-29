import assert from "node:assert/strict";
import { test } from "node:test";

import {
  BOTTOM_ANCHOR_SLOP_PX,
  bottomAnchorOnResize,
  bottomAnchorOnScroll,
  distanceFromBottom,
  scrollTopForDistance,
} from "./useCodingSessionBottomAnchor.ts";

const pinned = { distance: 0, restoring: false };

test("distance and scrollTop are inverses, clamped at the ends", () => {
  const metrics = { scrollHeight: 2_000, clientHeight: 600, scrollTop: 1_100 };
  assert.equal(distanceFromBottom(metrics), 300);
  assert.equal(scrollTopForDistance(metrics, 300), 1_100);
  assert.equal(scrollTopForDistance(metrics, 5_000), 0);
  assert.equal(distanceFromBottom({ ...metrics, scrollTop: 1_600 }), 0);
});

test("at the latest, a resize puts the view back at the bottom", () => {
  // A reply streams in, or the dock's reserve grows: 250 px opens below.
  const step = bottomAnchorOnResize(pinned, { distance: 250 });
  assert.equal(step.apply, true);
  assert.equal(step.state.distance, 0);
});

test("scrolled up, a resize leaves the view and follows the number", () => {
  const step = bottomAnchorOnResize(
    { distance: 400, restoring: false },
    { distance: 650 },
  );
  assert.equal(step.apply, false);
  assert.equal(step.state.distance, 650);
});

test("the reader scrolling records where they went, and ends a restore", () => {
  const up = bottomAnchorOnScroll(
    { distance: 0, restoring: true },
    { distance: 320, readerActive: true },
  );
  assert.deepEqual(up, {
    state: { distance: 320, restoring: false },
    apply: false,
  });
  const back = bottomAnchorOnScroll(up.state, {
    distance: BOTTOM_ANCHOR_SLOP_PX,
    readerActive: true,
  });
  assert.equal(back.state.distance, 0, "within the slop is at the latest");
});

test("while restoring, a scroll the reader did not make is undone", () => {
  // The router restores the old absolute scrollTop, which on a remount sits
  // ~200 px short of the bottom (Andy's third screenshot).
  const step = bottomAnchorOnScroll(
    { distance: 0, restoring: true },
    { distance: 200, readerActive: false },
  );
  assert.equal(step.apply, true);
  assert.equal(step.state.distance, 0);
  const held = bottomAnchorOnScroll(
    { distance: 480, restoring: true },
    { distance: 120, readerActive: false },
  );
  assert.equal(held.apply, true);
  assert.equal(held.state.distance, 480);
  const already = bottomAnchorOnScroll(
    { distance: 0, restoring: true },
    { distance: 0, readerActive: false },
  );
  assert.equal(already.apply, false, "its own correction is not re-applied");
});

test("at the latest, a scroll nobody made re-pins, even after the restore window", () => {
  // Measured: a 518 px burst moved scrollTop by the browser's scroll
  // anchoring, and that scroll event (553 px from the bottom) landed before
  // the resize callback for the same frame.
  const step = bottomAnchorOnScroll(pinned, {
    distance: 553,
    readerActive: false,
  });
  assert.equal(step.apply, true);
  assert.equal(step.state.distance, 0);
});

test("scrolled up, a programmatic scroll is recorded, not undone", () => {
  // A reveal (scrollIntoView) moves a reader who had already left the bottom.
  const step = bottomAnchorOnScroll(
    { distance: 400, restoring: false },
    { distance: 900, readerActive: false },
  );
  assert.equal(step.apply, false);
  assert.equal(step.state.distance, 900);
});
