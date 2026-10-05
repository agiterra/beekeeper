import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_SURFACE_PANELS_CLOSED,
  codingSessionSurfacePanelsStorageKey,
  isCodingSessionDrawerShortcut,
  isCodingSessionRightPanelShortcut,
  isTranscriptHiddenByPanel,
  parseCodingSessionSurfacePanels,
  reduceCodingSessionSurfacePanels,
  serializeCodingSessionSurfacePanels,
  visibleCodingSessionSurfacePanels,
} from "./useCodingSessionSurfacePanels.ts";

const CONVERSATION = new Set(["agents", "diff", "files", "terminal"]);
const KNOWN = new Set([...CONVERSATION, "mission-inspector", "mission-audit"]);

const reduce = (state, action) =>
  reduceCodingSessionSurfacePanels(state, action);
const open = (state, id, drawer = false) =>
  reduce(state, { type: "open", id, drawer });
const close = (state, id, visibleIds = CONVERSATION) =>
  reduce(state, { type: "close", ids: [id], visibleIds });

test("the storage key carries the community, channel and session", () => {
  assert.equal(
    codingSessionSurfacePanelsStorageKey({
      relayUrl: "wss://hive.example",
      channelId: "c1",
      sessionKey: "s1",
    }),
    "beekeeper:session-panels:v1:wss://hive.example:c1:s1",
  );
});

test("opening a surface adds and activates a tab; the panel opens", () => {
  const one = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "diff");
  assert.deepEqual(one.tabs, ["diff"]);
  assert.equal(one.active, "diff");
  assert.equal(one.rightOpen, true);
  const two = open(one, "agents");
  assert.deepEqual(two.tabs, ["diff", "agents"]);
  assert.equal(two.active, "agents");
  // Re-opening an open tab activates it without duplicating it.
  const again = open(two, "diff");
  assert.deepEqual(again.tabs, ["diff", "agents"]);
  assert.equal(again.active, "diff");
});

test("a drawer surface opens the drawer, never a tab", () => {
  const state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "terminal", true);
  assert.equal(state.bottomOpen, true);
  assert.deepEqual(state.tabs, []);
  assert.equal(state.rightOpen, false);
});

test("the drawer and an expanded panel never claim the screen together", () => {
  let state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "diff");
  state = reduce(state, { type: "toggleExpanded" });
  assert.equal(state.expanded, true);
  // ⌘J while expanded restores the panel, so the drawer it opens is seen.
  const drawer = reduce(state, { type: "toggleBottom" });
  assert.equal(drawer.bottomOpen, true);
  assert.equal(drawer.expanded, false);
  assert.equal(drawer.rightOpen, true);
  // Opening the terminal surface does the same.
  const terminal = open(state, "terminal", true);
  assert.equal(terminal.bottomOpen, true);
  assert.equal(terminal.expanded, false);
  // Expanding with the drawer open closes the drawer.
  const expanded = reduce(drawer, { type: "toggleExpanded" });
  assert.equal(expanded.expanded, true);
  assert.equal(expanded.bottomOpen, false);
});

test("closing the active tab falls back to its neighbour, as T3 does", () => {
  let state = CODING_SESSION_SURFACE_PANELS_CLOSED;
  for (const id of ["diff", "agents", "files"]) state = open(state, id);
  state = reduce(state, { type: "activate", id: "agents" });
  state = close(state, "agents");
  assert.deepEqual(state.tabs, ["diff", "files"]);
  assert.equal(state.active, "files");
  state = close(state, "files");
  assert.equal(state.active, "diff");
  assert.equal(state.rightOpen, true);
});

test("closing the last tab the lens can see closes the panel", () => {
  let state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "diff");
  state = reduce(state, { type: "toggleExpanded" });
  assert.equal(state.expanded, true);
  state = close(state, "diff");
  assert.equal(state.rightOpen, false);
  assert.equal(state.expanded, false);
  assert.equal(state.active, null);
});

test("leaving Mission closes its tabs and keeps Conversation's", () => {
  const mission = new Set(["mission-inspector", "mission-audit", "diff"]);
  let state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "agents");
  state = open(state, "mission-inspector");
  state = open(state, "mission-audit");
  state = reduce(state, {
    type: "close",
    ids: ["mission-inspector", "mission-audit"],
    visibleIds: mission,
  });
  assert.deepEqual(state.tabs, ["agents"]);
  // No Mission tab is left for Mission's lens, so the panel closes; the
  // Conversation tab is remembered for when that lens shows again.
  assert.equal(state.rightOpen, false);
});

test("the old header toggle closes the panel only on its own active tab", () => {
  let state = reduce(CODING_SESSION_SURFACE_PANELS_CLOSED, {
    type: "toggle",
    id: "diff",
    drawer: false,
  });
  assert.equal(state.rightOpen, true);
  assert.equal(state.active, "diff");
  state = reduce(state, { type: "toggle", id: "agents", drawer: false });
  assert.equal(state.active, "agents");
  state = reduce(state, { type: "toggle", id: "agents", drawer: false });
  assert.equal(state.rightOpen, false);
  assert.deepEqual(state.tabs, ["diff", "agents"]);
});

test("⌘⌥B hides and restores the panel with its tabs; ⌘J the drawer", () => {
  let state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "diff");
  state = reduce(state, { type: "toggleRight" });
  assert.equal(state.rightOpen, false);
  assert.deepEqual(state.tabs, ["diff"]);
  state = reduce(state, { type: "toggleRight" });
  assert.equal(state.rightOpen, true);
  assert.equal(state.active, "diff");
  // From nothing, the panel opens on the launcher (no active tab).
  const fresh = reduce(CODING_SESSION_SURFACE_PANELS_CLOSED, {
    type: "toggleRight",
  });
  assert.equal(fresh.rightOpen, true);
  assert.equal(fresh.active, null);
  assert.equal(
    reduce(state, { type: "toggleBottom" }).bottomOpen,
    true,
    "⌘J opens the drawer",
  );
});

test("a lens sees only its own tabs and an active tab among them", () => {
  let state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "diff");
  state = open(state, "mission-inspector");
  const conversation = visibleCodingSessionSurfacePanels(state, CONVERSATION);
  assert.deepEqual(conversation.tabs, ["diff"]);
  assert.equal(conversation.active, "diff");
  const none = visibleCodingSessionSurfacePanels(state, new Set(["files"]));
  assert.deepEqual(none.tabs, []);
  assert.equal(none.active, null);
});

test("a reload reopens the same tabs; corrupt or unknown entries are dropped", () => {
  let state = open(CODING_SESSION_SURFACE_PANELS_CLOSED, "diff");
  state = open(state, "agents");
  const restored = parseCodingSessionSurfacePanels(
    serializeCodingSessionSurfacePanels(state),
    KNOWN,
  );
  assert.deepEqual(restored, state);
  assert.deepEqual(
    parseCodingSessionSurfacePanels("{not json", KNOWN),
    CODING_SESSION_SURFACE_PANELS_CLOSED,
  );
  assert.deepEqual(
    parseCodingSessionSurfacePanels(null, KNOWN),
    CODING_SESSION_SURFACE_PANELS_CLOSED,
  );
  const pruned = parseCodingSessionSurfacePanels(
    JSON.stringify({
      rightOpen: true,
      tabs: ["diff", "retired-surface", 7, "diff"],
      active: "retired-surface",
      expanded: "yes",
      bottomOpen: true,
    }),
    KNOWN,
  );
  assert.deepEqual(pruned.tabs, ["diff"]);
  assert.equal(pruned.active, null);
  assert.equal(pruned.expanded, false);
  assert.equal(pruned.bottomOpen, true);
});

test("⌘J and ⌘⌥B are recognised by key code, alone", () => {
  const event = (overrides) => ({
    altKey: false,
    code: "KeyJ",
    ctrlKey: false,
    isComposing: false,
    metaKey: true,
    shiftKey: false,
    ...overrides,
  });
  assert.equal(isCodingSessionDrawerShortcut(event({})), true);
  assert.equal(isCodingSessionDrawerShortcut(event({ metaKey: false })), false);
  assert.equal(
    isCodingSessionDrawerShortcut(event({ metaKey: false, ctrlKey: true })),
    true,
  );
  assert.equal(isCodingSessionDrawerShortcut(event({ shiftKey: true })), false);
  assert.equal(isCodingSessionDrawerShortcut(event({ altKey: true })), false);
  // ⌥B types "∫" on macOS, so the code decides, not the key.
  assert.equal(
    isCodingSessionRightPanelShortcut(event({ code: "KeyB", altKey: true })),
    true,
  );
  assert.equal(
    isCodingSessionRightPanelShortcut(event({ code: "KeyB" })),
    false,
  );
});

test("an automatic open never overrides a stored panel choice (T3 openProactive)", () => {
  const proactive = (state) =>
    reduce(state, {
      type: "openProactive",
      ids: ["agents"],
      activate: "agents",
    });
  // Nobody has acted: the view may open Agents itself, and that open is not
  // recorded as the person's choice.
  const auto = proactive(CODING_SESSION_SURFACE_PANELS_CLOSED);
  assert.equal(auto.rightOpen, true);
  assert.equal(auto.active, "agents");
  assert.equal(auto.userActed, false);
  // The person closes the panel; that choice is persisted with the marker.
  const closed = reduce(auto, { type: "closeRight" });
  assert.equal(closed.userActed, true);
  const reloaded = parseCodingSessionSurfacePanels(
    serializeCodingSessionSurfacePanels(closed),
    KNOWN,
  );
  assert.equal(reloaded.userActed, true);
  // The next mount's automatic open is refused: the panel stays closed.
  assert.equal(proactive(reloaded), reloaded);
  assert.equal(proactive(reloaded).rightOpen, false);
  // A person's own open still works, of course.
  assert.equal(open(reloaded, "agents").rightOpen, true);
});

test("Mission's three tabs open proactively with Inspector active", () => {
  const state = reduce(CODING_SESSION_SURFACE_PANELS_CLOSED, {
    type: "openProactive",
    ids: ["mission-inspector", "mission-audit"],
    activate: "mission-inspector",
  });
  assert.deepEqual(state.tabs, ["mission-inspector", "mission-audit"]);
  assert.equal(state.active, "mission-inspector");
  assert.equal(state.userActed, false);
});

test("expand hides the transcript only inline, and only while the panel is open", () => {
  const expanded = {
    ...CODING_SESSION_SURFACE_PANELS_CLOSED,
    rightOpen: true,
    tabs: ["diff"],
    active: "diff",
    expanded: true,
  };
  assert.equal(isTranscriptHiddenByPanel(expanded, false), true);
  // A narrow window shows the panel as a sheet over the transcript.
  assert.equal(isTranscriptHiddenByPanel(expanded, true), false);
  // ⌘⌥B hides the panel and the transcript comes back.
  assert.equal(
    isTranscriptHiddenByPanel({ ...expanded, rightOpen: false }, false),
    false,
  );
  assert.equal(
    isTranscriptHiddenByPanel({ ...expanded, expanded: false }, false),
    false,
  );
});
