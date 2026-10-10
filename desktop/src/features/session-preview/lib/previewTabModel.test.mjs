/**
 * Ledger 371 (c), (d), (i): the Browser tab's lifecycle rules. Closing the
 * tab closes the page; unmounts do not. An agent's open shows the tab. A
 * page drawn nowhere is disclosed. The switcher marks the open server.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  SESSION_PREVIEW_TAB_ID,
  sessionPreviewCurrentServer,
  sessionPreviewHiddenNotice,
  sessionPreviewIsOpen,
  sessionPreviewOpenRequestMove,
  sessionPreviewTabClosedByPerson,
} from "./previewTabModel.ts";

const base = {
  channelId: "c1",
  sessionKey: "s1",
  lens: "conversation",
  tabs: ["agents", SESSION_PREVIEW_TAB_ID],
  userActed: true,
};

test("the person closing the Browser tab is a close", () => {
  assert.equal(
    sessionPreviewTabClosedByPerson(base, { ...base, tabs: ["agents"] }),
    true,
  );
});

test("unmounts and the view's own moves are not a close", () => {
  // First render: nothing to compare.
  assert.equal(sessionPreviewTabClosedByPerson(null, base), false);
  // The tab is still listed (another tab selected, or the panel hidden).
  assert.equal(sessionPreviewTabClosedByPerson(base, { ...base }), false);
  // Switching session or channel: a different view's panels.
  assert.equal(
    sessionPreviewTabClosedByPerson(base, {
      ...base,
      sessionKey: "s2",
      tabs: [],
    }),
    false,
  );
  assert.equal(
    sessionPreviewTabClosedByPerson(base, {
      ...base,
      channelId: "c2",
      tabs: [],
    }),
    false,
  );
  // A lens change between the renders.
  assert.equal(
    sessionPreviewTabClosedByPerson(base, {
      ...base,
      lens: "mission",
      tabs: ["agents"],
    }),
    false,
  );
  // A removal the person did not make (userActed unset).
  assert.equal(
    sessionPreviewTabClosedByPerson(
      { ...base, userActed: false },
      { ...base, userActed: false, tabs: ["agents"] },
    ),
    false,
  );
  // The tab was never open.
  assert.equal(
    sessionPreviewTabClosedByPerson(
      { ...base, tabs: ["agents"] },
      { ...base, tabs: [] },
    ),
    false,
  );
});

test("an agent's open-request shows the Browser tab, never a window", () => {
  assert.equal(
    sessionPreviewOpenRequestMove({
      rightOpen: false,
      active: null,
      userActed: false,
    }),
    "proactive",
  );
  assert.equal(
    sessionPreviewOpenRequestMove({
      rightOpen: true,
      active: "diff",
      userActed: true,
    }),
    "open",
  );
  assert.equal(
    sessionPreviewOpenRequestMove({
      rightOpen: true,
      active: SESSION_PREVIEW_TAB_ID,
      userActed: true,
    }),
    "none",
  );
  // The tab is active but the panel is hidden: show it.
  assert.equal(
    sessionPreviewOpenRequestMove({
      rightOpen: false,
      active: SESSION_PREVIEW_TAB_ID,
      userActed: true,
    }),
    "open",
  );
});

test("a live page drawn nowhere is disclosed by its address", () => {
  const live = {
    status: "ready",
    placement: "hidden",
    url: "http://localhost:5000/",
  };
  assert.equal(
    sessionPreviewHiddenNotice(live),
    "localhost:5000 is open, hidden",
  );
  assert.equal(
    sessionPreviewHiddenNotice({ ...live, status: "loading", url: null }),
    "A page is open, hidden",
  );
  for (const quiet of [
    { ...live, placement: "docked" },
    { ...live, placement: "popped_out" },
    { ...live, status: "absent", placement: "none" },
    { ...live, status: "closed_by_person" },
    null,
  ]) {
    assert.equal(sessionPreviewHiddenNotice(quiet), null);
  }
  assert.equal(sessionPreviewIsOpen({ status: "loading" }), true);
  assert.equal(sessionPreviewIsOpen({ status: "absent" }), false);
});

test("the switcher marks the server the page is on", () => {
  const servers = [
    { port: 5173, url: "http://localhost:5173/", address: "127.0.0.1" },
    { port: 5174, url: "http://localhost:5174/", address: "127.0.0.1" },
  ].map((server) => ({ pid: null, process: null, title: null, ...server }));
  assert.equal(
    sessionPreviewCurrentServer(servers, "http://localhost:5174/app?x=1")?.port,
    5174,
  );
  assert.equal(
    sessionPreviewCurrentServer(servers, "http://127.0.0.1:5173/")?.port,
    5173,
  );
  assert.equal(
    sessionPreviewCurrentServer(servers, "http://localhost:9999/"),
    null,
  );
  assert.equal(sessionPreviewCurrentServer(servers, null), null);
  assert.equal(sessionPreviewCurrentServer(servers, "not a url"), null);
});
