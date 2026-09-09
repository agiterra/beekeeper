import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { openContextMenuFromKeyboard } from "./ProjectChildRowItem.tsx";

/**
 * The sidebar session row's keyboard route to its own context menu.
 *
 * Radix's `ContextMenuTrigger` listens for a pointer's `contextmenu` and
 * nothing else, so every action on this row — close, archive, reopen, delete,
 * and now "New session in this workspace" — was reachable only by right-click.
 * The platform keys dispatch the very event the pointer would, at the row, so
 * there is one menu with one set of items rather than a second `⋯` menu that
 * would drift from it.
 *
 * What Radix then does with that event is Radix's, and the browser spec's.
 * What is asserted here is the half this component owns: which keys fire, that
 * the event reaches the trigger element itself and bubbles, and that nothing
 * else on the row is touched.
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

const originalDocument = globalThis.document;
const originalWindow = globalThis.window;
const originalMouseEvent = globalThis.MouseEvent;

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    MouseEvent: dom.window.MouseEvent,
    window: dom.window,
  });
});

after(() => {
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalMouseEvent === undefined) delete globalThis.MouseEvent;
  else globalThis.MouseEvent = originalMouseEvent;
  dom.window.close();
});

/** A row button in the document, and the contextmenu events it sees. */
function row() {
  const parent = dom.window.document.createElement("div");
  const button = dom.window.document.createElement("button");
  parent.append(button);
  dom.window.document.body.append(parent);
  const onElement = [];
  const onParent = [];
  button.addEventListener("contextmenu", (event) => onElement.push(event));
  parent.addEventListener("contextmenu", (event) => onParent.push(event));
  return { button, onElement, onParent, parent };
}

function press(button, key, overrides = {}) {
  let prevented = false;
  openContextMenuFromKeyboard({
    currentTarget: button,
    defaultPrevented: false,
    key,
    preventDefault: () => {
      prevented = true;
    },
    shiftKey: false,
    ...overrides,
  });
  return prevented;
}

test("Shift+F10 and the ContextMenu key open the row's own menu", () => {
  for (const [key, overrides] of [
    ["ContextMenu", {}],
    ["F10", { shiftKey: true }],
  ]) {
    const { button, onElement, onParent } = row();
    const prevented = press(button, key, overrides);
    assert.equal(prevented, true, `${key} must claim the keystroke`);
    assert.equal(onElement.length, 1, `${key} must reach the trigger`);
    // Radix listens through React's root delegation, so the event has to
    // bubble past the row, and it has to be cancelable for the trigger's own
    // `preventDefault()` to suppress the native menu.
    assert.equal(onParent.length, 1);
    assert.equal(onElement[0].bubbles, true);
    assert.equal(onElement[0].cancelable, true);
  }
});

test("the menu is anchored to the row, not to the last pointer position", () => {
  const { button, onElement } = row();
  button.getBoundingClientRect = () => ({
    bottom: 148,
    height: 32,
    left: 16,
    right: 272,
    top: 116,
    width: 256,
    x: 16,
    y: 116,
  });
  press(button, "ContextMenu");
  assert.equal(onElement[0].clientX, 28);
  assert.equal(onElement[0].clientY, 140);
});

test("no other key opens anything", () => {
  // Notably F10 alone: it is a menu-bar key on some platforms and stealing it
  // would take a keystroke this row has no claim on.
  for (const [key, overrides] of [
    ["F10", {}],
    ["Enter", {}],
    [" ", {}],
    ["ArrowDown", {}],
    ["m", { shiftKey: true }],
  ]) {
    const { button, onElement } = row();
    const prevented = press(button, key, overrides);
    assert.equal(prevented, false, `${key} must be left alone`);
    assert.equal(onElement.length, 0);
  }
});

test("a keystroke something else already handled is left alone", () => {
  const { button, onElement } = row();
  press(button, "ContextMenu", { defaultPrevented: true });
  assert.equal(onElement.length, 0);
});
