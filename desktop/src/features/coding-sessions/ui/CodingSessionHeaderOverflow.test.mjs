import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionHeaderOverflow } from "./CodingSessionHeaderOverflow.tsx";
import { codingSessionStopAllSentence } from "../lib/codingSessionStopAllModel.ts";

// Radix's Popover renders its content into a portal on open, which
// `renderToStaticMarkup` never reaches. The trigger is what a static render
// proves; the six items in order, and the destructive item's accessible name,
// are proved through the Playwright spec that opens the menu. What is asserted
// here is the part a static render can settle honestly: that the Mission
// header offers exactly one control, and that the sentence it carries is the
// model's and not one this component wrote.
const props = {
  onAddProvider() {},
  onCloseSession() {},
  onExport() {},
  onPopout() {},
  onStopAll() {},
  stopAllLabel: "Stop all (2 seats)",
  stopAllSentence: codingSessionStopAllSentence({ liveCount: 1, seatCount: 2 }),
};

test("A7: the six actions collapse into exactly one control", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeaderOverflow, props),
  );
  const triggers = markup.match(/data-testid="coding-session-overflow"/g) ?? [];
  assert.equal(triggers.length, 1);
  // Not one of the six flat buttons survives at the top level in Mission —
  // A6's complaint was `Stop all` sitting beside `Close`, same size, same
  // ghost variant, one mis-click apart.
  assert.doesNotMatch(markup, /data-testid="coding-session-stop-all"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-close"/);
});

test("with nothing to offer there is no menu at all", () => {
  // A `⋯` that opens on an empty list is a control that lies about having
  // something behind it.
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeaderOverflow, {
      stopAllLabel: "Stop all (0 seats)",
      stopAllSentence: "unused",
    }),
  );
  assert.equal(markup, "");
});

test("the stop-all sentence is the model's, split and consequence both", () => {
  // L4.3: the header used to print `Stop 2 live seats` over a count of seats
  // that were merely not ended. The count was right; the word was the lie.
  assert.equal(
    codingSessionStopAllSentence({ liveCount: 1, seatCount: 2 }),
    "Stop 2 seats — 1 live, 1 idle. A stopped seat cannot be resumed.",
  );
  assert.equal(
    codingSessionStopAllSentence({ liveCount: 0, seatCount: 2 }),
    "Stop 2 seats — none live. A stopped seat cannot be resumed.",
  );
  assert.equal(
    codingSessionStopAllSentence({ liveCount: 1, seatCount: 1 }),
    "Stop 1 seat — live.",
  );
  // Nothing on this surface prints `live` over a number it did not get from
  // the W1 map: the sentence cannot report more live seats than it stops.
  assert.match(
    codingSessionStopAllSentence({ liveCount: 9, seatCount: 2 }),
    /Stop 2 seats — 2 live\./,
  );
});
