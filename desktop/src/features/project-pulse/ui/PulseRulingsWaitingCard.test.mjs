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

/** The same rows as a surface with no identity renders them, from Rust. */
function noIdentityFixture() {
  return JSON.parse(
    readFileSync(
      resolve(here, "..", "lib", "pulseMissionNoIdentity.fixture.json"),
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
  const { PulseRulingsWaitingCard } = await import(
    "@/features/project-pulse/ui/PulseRulingsWaitingCard"
  );
  return render(
    createElement(PulseRulingsWaitingCard, {
      rows: decodePulseMissionRows(payload),
    }),
  );
}

test("what waits on the reader comes first, and the rest of the queue after it", async () => {
  const payload = fixture();
  const screen = await renderCard(payload);
  assert.equal(
    screen.getByTestId("pulse-rulings-waiting-heading").textContent,
    "Waiting on you",
  );
  assert.equal(
    screen.getByTestId("pulse-rulings-open-heading").textContent,
    "Open rulings",
  );
  const waiting = screen.getAllByTestId("pulse-ruling-waiting");
  assert.equal(waiting.length, 1);
  assert.equal(
    waiting[0].getAttribute("data-request-id"),
    payload.rulingsWaitingOnViewer[0].requestId,
  );
  // The queue below is the *rest*: a ruling already shown as waiting on the
  // reader must not be counted a second time under a heading that implies
  // somebody else holds it.
  const open = screen.getAllByTestId("pulse-ruling-open");
  assert.equal(open.length, 1);
  assert.equal(
    open[0].getAttribute("data-request-id"),
    payload.openRulings[0].requestId,
  );
  // The waiting section is ordered before the open one in the DOM.
  const card = screen.getByTestId("pulse-rulings-card");
  assert.ok(
    card.innerHTML.indexOf("pulse-rulings-waiting-heading") <
      card.innerHTML.indexOf("pulse-rulings-open-heading"),
  );
});

test("a ruling's own question renders; a question-less one still names itself", async () => {
  const payload = fixture();
  const screen = await renderCard(payload);
  assert.equal(
    screen.getByTestId("pulse-ruling-question").textContent,
    payload.openRulings[0].question,
  );
  // `openRulings[1]` (the waiting one) carries `question: null`. It renders no
  // invented sentence — but it is still addressable, because a decision the
  // reader holds must never disappear for want of a question string.
  const waiting = screen.getAllByTestId("pulse-ruling-waiting")[0];
  assert.equal(waiting.textContent.includes("null"), false);
  assert.equal(
    waiting.getAttribute("data-held-on"),
    payload.rulingsWaitingOnViewer[0].heldOn,
  );
});

test("an unknown viewer is not zero rulings", async () => {
  // The real no-identity response from the Rust renderer — the sentence is not
  // injected here, which is what makes this a test of the model and not of
  // this test's own imagination (REVIEW-L9 §6).
  const payload = noIdentityFixture();
  const screen = await renderCard(payload);
  // No count, no empty list, no "0" — the three shapes of the lie.
  assert.equal(screen.queryByTestId("pulse-rulings-waiting"), null);
  assert.equal(screen.queryByTestId("pulse-ruling-waiting"), null);
  assert.equal(/(^|\D)0(\D|$)/.test(screen.container.textContent), false);
  assert.equal(
    screen.getByTestId("pulse-rulings-viewer-unknown").textContent,
    "No identity on this surface, so nothing here can be held on you",
  );
});

test("the no-identity sentence is rendered once, not once per mission", async () => {
  // Every mission row carries the `not-read` line, because a row is where the
  // model composes it. The card is one statement about the reader, so the
  // selector de-duplicates rather than repeating it per session.
  const payload = noIdentityFixture();
  const carriers = payload.missions.filter((mission) =>
    mission.lines.some((line) => line.id === "not-read"),
  );
  assert.ok(carriers.length > 1, "the fixture carries it on more than one row");
  const screen = await renderCard(payload);
  assert.equal(screen.getAllByTestId("pulse-mission-line-not-read").length, 1);
});

test("an unknown viewer with no sentence on the wire invents none", async () => {
  const payload = fixture();
  payload.viewerPubkey = null;
  payload.rulingsWaitingOnViewer = [];
  const screen = await renderCard(payload);
  assert.equal(screen.queryByTestId("pulse-rulings-waiting"), null);
  assert.equal(screen.queryByTestId("pulse-rulings-viewer-unknown"), null);
  // The open queue is still readable — not knowing the reader does not make
  // the other rulings disappear.
  assert.equal(screen.getAllByTestId("pulse-ruling-open").length, 2);
});

test("no rulings at all renders no card rather than an empty one", async () => {
  const payload = fixture();
  payload.openRulings = [];
  payload.rulingsWaitingOnViewer = [];
  const screen = await renderCard(payload);
  assert.equal(screen.queryByTestId("pulse-rulings-card"), null);
});
