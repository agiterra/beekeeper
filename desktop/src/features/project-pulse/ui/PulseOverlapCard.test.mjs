import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { after, afterEach, before, test } from "node:test";
import { fileURLToPath } from "node:url";

import { JSDOM } from "jsdom";

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
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const here = dirname(fileURLToPath(import.meta.url));

function fixture() {
  return JSON.parse(
    readFileSync(
      resolve(here, "..", "lib", "pulseMissionResponse.fixture.json"),
      "utf8",
    ),
  );
}

async function renderCard(payload) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { decodePulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseMissionWire"
  );
  const { PulseOverlapCard } = await import(
    "@/features/project-pulse/ui/PulseOverlapCard"
  );
  return render(
    createElement(PulseOverlapCard, {
      overlaps: decodePulseMissionRows(payload).overlaps,
    }),
  );
}

test("an overlap renders every line the model wrote", async () => {
  const payload = fixture();
  const screen = await renderCard(payload);
  assert.equal(
    screen.getByTestId("pulse-mission-line-overlap").textContent,
    payload.overlaps[0].lines[0].text,
  );
  const row = screen.getByTestId("pulse-overlap");
  // Both sides are named on the row, because an overlap that showed only one
  // seat would read as one lane's problem rather than two lanes' collision.
  assert.deepEqual(row.getAttribute("data-session-keys")?.split(","), [
    payload.overlaps[0].seats[0].sessionKey,
    payload.overlaps[0].seats[1].sessionKey,
  ]);
});

test("an overlap card offers no action, because there is no wake to send", async () => {
  const payload = fixture();
  const screen = await renderCard(payload);
  const card = screen.getByTestId("pulse-overlap-card");
  assert.equal(card.querySelectorAll("button").length, 0);
  assert.equal(card.querySelectorAll("a").length, 0);
  // The reason is written down where the next person to add a button will see
  // it, not only in a review comment.
  const source = readFileSync(resolve(here, "PulseOverlapCard.tsx"), "utf8");
  assert.match(source, /no cross-umbrella wake/i);
  assert.match(source, /note or message the other lead/i);
});

test("a truncated path list renders its own line, though this fixture has none", async () => {
  // `overlap-truncated` is in the closed id set and the renderer emits it
  // whenever `pathsTruncated > 0`; the current fixture simply has two whole
  // paths. Driven from a local variant rather than by editing the contract —
  // a line family that only the producer can currently exercise still has to
  // reach the screen when it arrives.
  const payload = fixture();
  payload.overlaps[0].pathsTruncated = 1;
  payload.overlaps[0].lines.push({
    id: "overlap-truncated",
    text: "1 more shared path not shown",
  });
  const screen = await renderCard(payload);
  assert.equal(
    screen.getByTestId("pulse-mission-line-overlap-truncated").textContent,
    "1 more shared path not shown",
  );
});

test("no overlaps renders no card", async () => {
  const payload = fixture();
  payload.overlaps = [];
  const screen = await renderCard(payload);
  assert.equal(screen.queryByTestId("pulse-overlap-card"), null);
});
