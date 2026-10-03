import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    CSS: dom.window.CSS ?? { escape: (value) => value },
    CustomEvent: dom.window.CustomEvent,
    document: dom.window.document,
    HTMLDetailsElement: dom.window.HTMLDetailsElement,
    window: dom.window,
  });
  if (!globalThis.CSS.escape) globalThis.CSS = { escape: (value) => value };
});

after(() => dom.window.close());

test("a spawn not in the DOM is handed to the umbrella timeline's reveal", async () => {
  const { revealCodingSessionSubagentInStream } = await import(
    "./CodingSessionSubagentEntry.tsx"
  );
  const { UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT } = await import(
    "./CodingSessionUmbrellaTimelineWindow.ts"
  );

  // Nobody handles the request: the caller keeps its own inline view.
  assert.equal(revealCodingSessionSubagentInStream("call-1"), false);

  const requested = [];
  const handler = (event) => {
    requested.push(event.detail.itemId);
    event.preventDefault();
  };
  document.addEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
  try {
    assert.equal(revealCodingSessionSubagentInStream("call-1"), true);
    assert.deepEqual(requested, ["call-1"]);
  } finally {
    document.removeEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
  }
});

test("a mounted spawn row is opened in place without asking the timeline", async () => {
  const { revealCodingSessionSubagentInStream } = await import(
    "./CodingSessionSubagentEntry.tsx"
  );
  const { UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT } = await import(
    "./CodingSessionUmbrellaTimelineWindow.ts"
  );
  document.body.innerHTML =
    '<div data-testid="coding-session-transcript"><details data-subagent-call-ids="call-0 call-2"><summary>x</summary></details></div>';
  const details = document.querySelector("details");
  details.scrollIntoView = () => {};
  let asked = false;
  const handler = () => {
    asked = true;
  };
  document.addEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
  try {
    assert.equal(revealCodingSessionSubagentInStream("call-2"), true);
    assert.equal(details.open, true);
    assert.equal(asked, false);
  } finally {
    document.removeEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
    document.body.innerHTML = "";
  }
});
