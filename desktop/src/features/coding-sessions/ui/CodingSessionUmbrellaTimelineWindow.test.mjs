import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  isBottomAnchorAtLatest,
  recallCodingSessionNarrativeWindow,
  rememberCodingSessionNarrativeWindow,
  resetCodingSessionNarrativeMemory,
} from "../hooks/useCodingSessionBottomAnchor.ts";
import {
  CodingSessionUmbrellaLoadEarlier,
  umbrellaLoadEarlierCopy,
} from "./CodingSessionUmbrellaLoadEarlier.tsx";
import {
  UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT,
  UMBRELLA_TIMELINE_WINDOW_TURNS,
  latestUmbrellaTimelineTurnKey,
  requestCodingSessionUmbrellaItemReveal,
  umbrellaTimelineLiveBlockKeys,
  loadEarlierUmbrellaTimeline,
  resolveUmbrellaTimelineWindow,
  revealInUmbrellaTimeline,
  settleUmbrellaTimelinePin,
  umbrellaTimelineFollowStart,
} from "./CodingSessionUmbrellaTimelineWindow.ts";

/**
 * `count` turns, each preceded by the user message that prompted it:
 * [c0, t0, c1, t1, …]. Index of t<n> is 2n + 1; of c<n>, 2n.
 */
function timeline(count, { executionOf = () => "seat-a" } = {}) {
  const entries = [];
  for (let n = 0; n < count; n += 1) {
    entries.push({ kind: "conversation", key: `c${n}` });
    entries.push({ kind: "turn-block", key: `t${n}`, exec: executionOf(n) });
  }
  return entries;
}

function source(entries, isTurn = (entry) => entry.kind === "turn-block") {
  return { entries, keyOf: (entry) => entry.key, isTurn };
}

test("the window holds the last ten turns and the message before the first", () => {
  assert.equal(UMBRELLA_TIMELINE_WINDOW_TURNS, 10);
  const window = resolveUmbrellaTimelineWindow(source(timeline(25)), null);
  // t14 is the eleventh turn from the end; the window opens just after it,
  // so c15 — the message that prompted t15 — is inside.
  assert.equal(window.startKey, "c15");
  assert.equal(window.startIndex, 30);
  assert.equal(window.hiddenTurnCount, 15);
  assert.equal(window.hiddenEntryCount, 30);
});

test("a session with no more turns than the window renders everything", () => {
  for (const count of [0, 1, 10]) {
    const window = resolveUmbrellaTimelineWindow(source(timeline(count)), null);
    assert.equal(window.startIndex, 0, `count ${count}`);
    assert.equal(window.hiddenTurnCount, 0);
  }
});

test("load earlier adds ten turns, then the rest, then stops at the top", () => {
  const src = source(timeline(25));
  const first = loadEarlierUmbrellaTimeline(src, null);
  assert.equal(first, "c5");
  assert.equal(resolveUmbrellaTimelineWindow(src, first).hiddenTurnCount, 5);
  const second = loadEarlierUmbrellaTimeline(src, first);
  assert.equal(second, "c0");
  const window = resolveUmbrellaTimelineWindow(src, second);
  assert.equal(window.startIndex, 0);
  assert.equal(window.hiddenTurnCount, 0);
  assert.equal(loadEarlierUmbrellaTimeline(src, second), "c0");
});

test("a jump to a turn outside the window opens the window at that turn's message", () => {
  const src = source(timeline(25));
  const pin = revealInUmbrellaTimeline(src, null, "t3");
  assert.equal(pin, "c3");
  const window = resolveUmbrellaTimelineWindow(src, pin);
  assert.equal(window.startIndex, 6);
  assert.equal(window.hiddenTurnCount, 3);
});

test("a jump to a light row outside the window opens at that row", () => {
  const src = source(timeline(25));
  assert.equal(revealInUmbrellaTimeline(src, null, "c4"), "c4");
});

test("a jump inside the window, or to an unknown key, leaves the pin alone", () => {
  const src = source(timeline(25));
  const pin = resolveUmbrellaTimelineWindow(src, null).startKey;
  assert.equal(revealInUmbrellaTimeline(src, pin, "t20"), pin);
  assert.equal(revealInUmbrellaTimeline(src, pin, "no-such-row"), pin);
});

test("turns arriving while the reader is scrolled up grow the window and unmount nothing", () => {
  const before = source(timeline(15));
  const pinned = resolveUmbrellaTimelineWindow(before, null).startKey;
  assert.equal(pinned, "c5");
  const after = source(timeline(18));
  assert.notEqual(
    latestUmbrellaTimelineTurnKey(after),
    latestUmbrellaTimelineTurnKey(before),
  );
  const settled = settleUmbrellaTimelinePin(after, {
    pinnedStartKey: pinned,
    newTurnArrived: true,
    atLatest: false,
  });
  assert.equal(settled, "c5");
  const window = resolveUmbrellaTimelineWindow(after, settled);
  // Thirteen turns render: nothing above the reader moved.
  assert.equal(after.entries.length - window.startIndex, 26);
});

test("a new turn at the latest slides the window back to ten turns", () => {
  const after = source(timeline(18));
  const settled = settleUmbrellaTimelinePin(after, {
    pinnedStartKey: "c5",
    newTurnArrived: true,
    atLatest: true,
  });
  assert.equal(settled, "c8");
});

test("a turn growing (no new turn) never trims, even at the latest", () => {
  const src = source(timeline(18));
  const settled = settleUmbrellaTimelinePin(src, {
    pinnedStartKey: "c0",
    newTurnArrived: false,
    atLatest: true,
  });
  assert.equal(settled, "c0", "loaded-earlier turns stay until a new turn");
});

test("an unpinned window pins where it renders, and a vanished pin falls back", () => {
  const src = source(timeline(25));
  assert.equal(
    settleUmbrellaTimelinePin(src, {
      pinnedStartKey: null,
      newTurnArrived: false,
      atLatest: false,
    }),
    "c15",
  );
  const window = resolveUmbrellaTimelineWindow(src, "filtered-out-by-density");
  assert.equal(window.startKey, "c15");
});

test("a pin never shrinks the window below ten turns", () => {
  const src = source(timeline(25));
  const window = resolveUmbrellaTimelineWindow(src, "t22");
  assert.equal(window.startKey, "c15");
});

test("with a seat focused, only that seat's turns count toward the window", () => {
  const entries = timeline(30, {
    executionOf: (n) => (n % 2 === 0 ? "seat-a" : "seat-b"),
  });
  const focused = source(
    entries,
    (entry) => entry.kind === "turn-block" && entry.exec === "seat-a",
  );
  const window = resolveUmbrellaTimelineWindow(focused, null);
  // Ten seat-a turns are t10…t28; the eleventh is t8, so the window opens at c9.
  assert.equal(window.startKey, "c9");
  assert.equal(window.hiddenTurnCount, 5);
});

test("the follow start can be measured from any point", () => {
  const src = source(timeline(25));
  assert.equal(umbrellaTimelineFollowStart(src, 10, 30), 10);
  assert.equal(umbrellaTimelineFollowStart(src, 0, 7), 6, "c3: just after t2");
});

test("the window start is remembered beside the scroll distance", () => {
  resetCodingSessionNarrativeMemory();
  assert.equal(recallCodingSessionNarrativeWindow("channel:session"), null);
  rememberCodingSessionNarrativeWindow("channel:session", "c5");
  assert.equal(recallCodingSessionNarrativeWindow("channel:session"), "c5");
  resetCodingSessionNarrativeMemory();
  assert.equal(recallCodingSessionNarrativeWindow("channel:session"), null);
  assert.equal(isBottomAnchorAtLatest(null), false);
});

test("the control says how many turns are above and how many it will load", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaLoadEarlier, {
      hiddenEntryCount: 30,
      hiddenTurnCount: 15,
      onLoadEarlier: () => {},
    }),
  );
  assert.match(html, /15 earlier turns not shown \(30 rows\)/);
  assert.match(html, /Load 10 earlier/);
  const few = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaLoadEarlier, {
      hiddenEntryCount: 2,
      hiddenTurnCount: 1,
      onLoadEarlier: () => {},
    }),
  );
  assert.match(few, /1 earlier turn not shown/);
  assert.match(few, /Load 1 earlier/);
});

const DONE = { type: "lifecycle", renderClass: "status", title: "Turn result" };
const ASK = {
  type: "lifecycle",
  renderClass: "permission",
  title: "Permission requested",
};

/** Turn blocks with real items, for the live-key rule. */
function block(key, executionKey, items) {
  return { kind: "turn-block", key, executionKey, items };
}

test("a lead parked on an approval stays rendered while other seats finish ten turns", () => {
  // The lead's open block sorts at its newest item — the permission it is
  // waiting on — so eleven later builder turns push it past the window.
  const entries = [
    { kind: "conversation", key: "c-lead" },
    block("lead-0", "lead", [ASK]),
  ];
  for (let n = 0; n < 12; n += 1) {
    entries.push({ kind: "conversation", key: `c${n}` });
    entries.push(block(`b${n}`, "builder", [DONE]));
  }
  const blocks = entries.filter((entry) => entry.kind === "turn-block");
  const live = umbrellaTimelineLiveBlockKeys(blocks, (entry) => entry.key);
  assert.deepEqual([...live], ["lead-0"]);
  const src = {
    ...source(entries),
    isLive: (entry) => live.has(entry.key),
  };
  const window = resolveUmbrellaTimelineWindow(src, null);
  assert.equal(window.startKey, "c-lead", "the approval is not click-hidden");
  assert.equal(window.startIndex, 0);
  // Without the live rule the lead would be above the control.
  assert.equal(
    resolveUmbrellaTimelineWindow(source(entries), null).startKey,
    "c2",
  );
  // A new turn at the latest trims — but never past the live block.
  assert.equal(
    settleUmbrellaTimelinePin(src, {
      pinnedStartKey: "c-lead",
      newTurnArrived: true,
      atLatest: true,
    }),
    "c-lead",
  );
});

test("a seat's newest unterminated turn is live; a cut-off older one is not", () => {
  const blocks = [
    block("old-open", "seat-a", [{ type: "message", role: "assistant" }]),
    block("a-done", "seat-a", [DONE]),
    block("a-running", "seat-a", [{ type: "tool" }]),
    block("b-done", "seat-b", [DONE]),
  ];
  const live = umbrellaTimelineLiveBlockKeys(blocks, (entry) => entry.key);
  assert.deepEqual([...live], ["a-running"]);
});

test("an unanswered permission pins an open block anywhere; a terminated or answered one does not", () => {
  const blocks = [
    block("asked-open", "seat-a", [ASK]),
    block("asked-ended", "seat-a", [ASK, DONE]),
    block("answered", "seat-a", [{ ...ASK, outcome: "Approved (allow_once)" }]),
    block("latest", "seat-a", [DONE]),
  ];
  const live = umbrellaTimelineLiveBlockKeys(blocks, (entry) => entry.key);
  assert.deepEqual([...live], ["asked-open"]);
});

test("with a seat focused the control leads with every hidden row and names whose turns it counted", () => {
  const focused = umbrellaLoadEarlierCopy({
    hiddenEntryCount: 40,
    hiddenTurnCount: 5,
    turnsCountedFor: "Builder",
  });
  assert.equal(
    focused.summary,
    "40 earlier rows not shown, 5 turns from Builder",
  );
  assert.equal(focused.action, "Load 5 earlier from Builder");
  const none = umbrellaLoadEarlierCopy({
    hiddenEntryCount: 3,
    hiddenTurnCount: 0,
    turnsCountedFor: "Builder",
  });
  assert.equal(none.summary, "3 earlier rows not shown, none from Builder");
  assert.equal(none.action, "Load earlier");
});

test("an item reveal request is owned only by a mounted timeline that cancels it", () => {
  assert.equal(requestCodingSessionUmbrellaItemReveal("item-1"), false);
  const previous = globalThis.document;
  const target = new EventTarget();
  globalThis.document = target;
  try {
    assert.equal(requestCodingSessionUmbrellaItemReveal("item-1"), false);
    const seen = [];
    target.addEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, (event) => {
      seen.push(event.detail.itemId);
      event.preventDefault();
    });
    assert.equal(requestCodingSessionUmbrellaItemReveal("item-2"), true);
    assert.deepEqual(seen, ["item-2"]);
  } finally {
    if (previous === undefined) delete globalThis.document;
    else globalThis.document = previous;
  }
});
