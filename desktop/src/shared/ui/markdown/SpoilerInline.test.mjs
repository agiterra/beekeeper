/**
 * A click on a spoiler must toggle it exactly once.
 *
 * `SpoilerInline` wired `onClickCapture` and `onClick` to the same toggle. The
 * capture handler stops propagation only while the spoiler is hidden, so the
 * second click — the one a person makes on a link they have just revealed —
 * reached the bubble handler and hid the spoiler again instead of letting the
 * link open. That is the flake behind `tests/e2e/spoiler.spec.ts` "hidden
 * spoiler links reveal without opening on the first click": `data-revealed`
 * was observed back at "false".
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    MouseEvent: dom.window.MouseEvent,
    Node: dom.window.Node,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
  // jsdom has no 2d canvas; the particle layer bails out on a null context.
  dom.window.HTMLCanvasElement.prototype.getContext = () => null;
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

async function renderSpoiler() {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { SpoilerInline } = await import("./SpoilerInline.tsx");

  const { container } = render(
    createElement(
      SpoilerInline,
      null,
      createElement("a", { href: "https://example.com" }, "secret"),
    ),
  );
  const spoiler = container.querySelector(".beekeeper-spoiler");
  const link = container.querySelector("a");
  assert.ok(spoiler && link);
  return { link, spoiler };
}

/** Dispatch a real bubbling click and report whether the default survived. */
function clickAndReport(node) {
  const event = new dom.window.MouseEvent("click", {
    bubbles: true,
    cancelable: true,
  });
  node.dispatchEvent(event);
  return { defaultPrevented: event.defaultPrevented };
}

test("one click on a hidden spoiler reveals it, and swallows the link's navigation", async () => {
  const { act } = await import("react");
  const { link, spoiler } = await renderSpoiler();

  assert.equal(spoiler.getAttribute("data-revealed"), "false");

  let outcome;
  await act(async () => {
    outcome = clickAndReport(link);
  });

  assert.equal(spoiler.getAttribute("data-revealed"), "true");
  // The click that reveals must not also open the link.
  assert.equal(outcome.defaultPrevented, true);
});

test("a click on a link inside a revealed spoiler navigates instead of re-hiding it", async () => {
  const { act } = await import("react");
  const { link, spoiler } = await renderSpoiler();

  await act(async () => {
    clickAndReport(link);
  });
  assert.equal(spoiler.getAttribute("data-revealed"), "true");

  let outcome;
  await act(async () => {
    outcome = clickAndReport(link);
  });

  assert.equal(spoiler.getAttribute("data-revealed"), "true");
  assert.equal(outcome.defaultPrevented, false);
});

test("clicking the spoiler surface itself still hides it again", async () => {
  const { act } = await import("react");
  const { spoiler } = await renderSpoiler();

  await act(async () => {
    clickAndReport(spoiler);
  });
  assert.equal(spoiler.getAttribute("data-revealed"), "true");

  await act(async () => {
    clickAndReport(spoiler);
  });
  assert.equal(spoiler.getAttribute("data-revealed"), "false");
});

test("clicking the revealed text — not a control — hides it again", async () => {
  const { act } = await import("react");
  const { spoiler } = await renderSpoiler();
  const content = spoiler.querySelector(".buzz-spoiler__content");
  assert.ok(content);

  await act(async () => {
    clickAndReport(content);
  });
  assert.equal(spoiler.getAttribute("data-revealed"), "true");

  await act(async () => {
    clickAndReport(content);
  });
  assert.equal(spoiler.getAttribute("data-revealed"), "false");
});
