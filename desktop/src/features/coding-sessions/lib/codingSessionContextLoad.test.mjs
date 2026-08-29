import assert from "node:assert/strict";
import test from "node:test";

import {
  readCodingSessionContextLoad,
  renderCodingSessionContextLoad,
} from "./codingSessionContextLoad.ts";

function occupancy(text, timestamp = "2026-08-29T06:00:00.000Z") {
  return {
    id: `item-${timestamp}`,
    type: "lifecycle",
    renderClass: "status",
    title: "Context Window Updated",
    text,
    timestamp,
  };
}

test("the driver's own occupancy item renders the cell bee sessions status prints", () => {
  const load = readCodingSessionContextLoad([
    occupancy("size: 1000000\nused: 137498"),
  ]);

  assert.deepEqual(load, {
    usedTokens: 137498,
    contextWindow: 1000000,
    pct: 14,
  });
  assert.equal(renderCodingSessionContextLoad(load), "137498/1000000 (14%)");
});

test("percent is rounded half-up, matching ContextLoad::pct", () => {
  const load = readCodingSessionContextLoad([
    occupancy("size: 1000000\nused: 270000"),
  ]);
  assert.equal(load?.pct, 27);
  assert.equal(renderCodingSessionContextLoad(load), "270000/1000000 (27%)");
});

test("the newest occupancy item wins", () => {
  const load = readCodingSessionContextLoad([
    occupancy("size: 1000000\nused: 10", "2026-08-29T06:00:00.000Z"),
    occupancy("size: 1000000\nused: 500000", "2026-08-29T06:05:00.000Z"),
  ]);
  assert.equal(load?.usedTokens, 500000);
  assert.equal(load?.pct, 50);
});

test("an occupancy item with no window never invents a percentage", () => {
  const load = readCodingSessionContextLoad([occupancy("usedTokens: 137498")]);

  assert.deepEqual(load, {
    usedTokens: 137498,
    contextWindow: null,
    pct: null,
  });
  assert.equal(
    renderCodingSessionContextLoad(load),
    "137498 tokens (window unknown)",
  );
});

test("nothing reported is an em dash, never zero", () => {
  assert.equal(readCodingSessionContextLoad([]), null);
  assert.equal(
    readCodingSessionContextLoad([
      {
        id: "x",
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Done",
        timestamp: "2026-08-29T06:00:00.000Z",
      },
    ]),
    null,
  );
  assert.equal(renderCodingSessionContextLoad(null), "—");
});
