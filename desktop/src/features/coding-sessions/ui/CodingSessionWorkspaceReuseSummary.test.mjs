import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { JSDOM } from "jsdom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

import { CodingSessionWorkspaceReuseSummary } from "./CodingSessionWorkspaceReuseSummary.tsx";

/**
 * Mounted, because what this block has to get right is what a person can
 * actually read off it — the exact path, the branch with its provenance, the
 * two sentences, and nothing at all where there is nothing to say.
 */
const dom = new JSDOM(
  "<!doctype html><html><body><div id='root'></div></body></html>",
  { url: "http://localhost" },
);

const originalDocument = globalThis.document;
const originalWindow = globalThis.window;
const originalHTMLElement = globalThis.HTMLElement;

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => {
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalHTMLElement === undefined) delete globalThis.HTMLElement;
  else globalThis.HTMLElement = originalHTMLElement;
  dom.window.close();
});

const PATH = "/Users/brian/Projects/beekeeper/repo-wt-a";

async function render(props) {
  const host = dom.window.document.getElementById("root");
  const root = createRoot(host);
  await act(async () => {
    root.render(React.createElement(CodingSessionWorkspaceReuseSummary, props));
  });
  const query = (suffix) =>
    host.querySelector(
      `[data-testid="coding-session-workspace-reuse${suffix}"]`,
    );
  return {
    block: query(""),
    path: query("-path"),
    branch: query("-branch"),
    note: query("-note"),
    unmount: async () => {
      await act(async () => root.unmount());
    },
  };
}

test("the draft shows the absolute path, not a friendly name", async () => {
  // A base name is ambiguous the moment two checkouts share one: the person
  // is being asked to confirm a directory, so they get the directory.
  const view = await render({
    path: PATH,
    branch: "main",
    branchSource: "recorded",
  });
  assert.equal(view.path.textContent, PATH);
  await view.unmount();
});

test("the branch says which fact it is, recorded or on disk", async () => {
  const recorded = await render({
    path: PATH,
    branch: "work/contextual-sessions-fable",
    branchSource: "recorded",
  });
  assert.equal(
    recorded.branch.textContent,
    "work/contextual-sessions-fable · recorded at creation",
  );
  await recorded.unmount();

  const live = await render({
    path: PATH,
    branch: "main",
    branchSource: "live",
  });
  assert.equal(live.branch.textContent, "main · on disk now");
  await live.unmount();

  // An unknown branch is reported as unknown rather than left blank, which
  // would read as "no branch".
  const unknown = await render({ path: PATH });
  assert.equal(
    unknown.branch.textContent,
    "No branch recorded for this folder on this computer.",
  );
  await unknown.unmount();
});

test("the two sentences are present, and no other operation is claimed", async () => {
  const view = await render({
    path: PATH,
    branch: "main",
    branchSource: "live",
  });
  assert.match(view.note.textContent, /New conversation; uses these files\./);
  assert.match(
    view.note.textContent,
    /Includes uncommitted changes already in this folder\./,
  );
  const whole = view.block.textContent;
  for (const forbidden of [
    /isolat/i,
    /\bfork/i,
    /inherit/i,
    /continu/i,
    /resum/i,
    /take[n]? over/i,
  ]) {
    assert.doesNotMatch(whole, forbidden);
  }
  await view.unmount();
});

test("the draft says nothing about other sessions in this folder", async () => {
  // That count is fresh only where it is read: the menu item. This block is
  // rendered from a request restored out of sessionStorage, so a count
  // carried here could outlive the read that produced it.
  const view = await render({
    path: PATH,
    branch: "main",
    branchSource: "live",
  });
  assert.doesNotMatch(view.block.textContent, /other session/i);
  assert.equal(
    view.block.querySelector(
      '[data-testid="coding-session-workspace-reuse-also-here"]',
    ),
    null,
  );
  await view.unmount();
});

test("it is a labelled region and no meaning rides on colour", async () => {
  const view = await render({
    path: PATH,
    branch: "main",
    branchSource: "live",
  });
  assert.equal(view.block.tagName, "SECTION");
  assert.equal(
    view.block.getAttribute("aria-label"),
    "Folder this session will use",
  );
  // Every fact in the block is carried by words. Nothing here is a bare dot,
  // a tinted badge or an icon standing in for a state.
  assert.equal(view.block.querySelectorAll("svg").length, 0);
  for (const node of view.block.querySelectorAll("[data-testid]")) {
    assert.ok(
      node.textContent.trim().length > 0,
      `${node.getAttribute("data-testid")} renders no text`,
    );
  }
  await view.unmount();
});
