import assert from "node:assert/strict";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

// SV-59: leaving the Mission lens is the view's move, not the person's. It
// must not record `userActed`, and re-entering Mission reopens the Mission
// surfaces that were open when it left — but none the person closed in it.

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const saved = {
  document: globalThis.document,
  window: globalThis.window,
  HTMLElement: globalThis.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: globalThis.IS_REACT_ACT_ENVIRONMENT,
};
Object.assign(globalThis, {
  document: dom.window.document,
  window: dom.window,
  HTMLElement: dom.window.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: true,
});
after(() => {
  for (const [key, value] of Object.entries(saved)) {
    if (value === undefined) delete globalThis[key];
    else globalThis[key] = value;
  }
  dom.window.close();
});

const {
  reduceCodingSessionSurfacePanels,
  CODING_SESSION_SURFACE_PANELS_CLOSED,
  useCodingSessionSurfacePanels,
} = await import("./surfaces/useCodingSessionSurfacePanels.ts");
const { useCodingSessionMissionSurfaceActivation } = await import(
  "./useCodingSessionMissionSurface.tsx"
);

const MISSION = ["mission-inspector", "mission-context", "mission-audit"];
const KNOWN = new Set([...MISSION, "agents", "diff"]);
const MISSION_VISIBLE = KNOWN;
const CONVERSATION_VISIBLE = new Set(["agents", "diff"]);
const NO_DRAWERS = new Set();

/**
 * The umbrella workspace's wiring, minus everything that is not panels:
 * the shell's Mission callbacks and `handleLensChange`, as written there.
 */
function Harness({ api, multi }) {
  const [lens, setLens] = React.useState("mission");
  const mission = multi && lens === "mission";
  const panels = useCodingSessionSurfacePanels({
    storageKey: "sv59",
    knownIds: KNOWN,
    visibleIds: mission ? MISSION_VISIBLE : CONVERSATION_VISIBLE,
    drawerIds: NO_DRAWERS,
  });
  const { actions } = panels;
  const openMissionSurfaces = React.useCallback(() => {
    for (const id of MISSION) actions.open(id);
    actions.activate("mission-inspector");
  }, [actions]);
  const openMissionSurfacesProactively = React.useCallback(
    () => actions.openProactive(MISSION, "mission-inspector"),
    [actions],
  );
  const closeMissionSurfaces = React.useCallback(
    () => actions.closeForLens(MISSION),
    [actions],
  );
  const reopenMissionSurfaces = React.useCallback(
    (ids) => actions.reopenForLens(ids, "mission-inspector"),
    [actions],
  );
  const openTabs = panels.state.tabs;
  const openMissionSurfaceIds = React.useMemo(
    () => MISSION.filter((id) => openTabs.includes(id)),
    [openTabs],
  );
  useCodingSessionMissionSurfaceActivation({
    bodyWidthPx: 800,
    closeMissionSurfaces,
    isMultiExecution: multi,
    mission,
    openProactive: actions.openProactive,
    openMissionSurfaces: openMissionSurfacesProactively,
    openMissionSurfaceIds,
    reopenMissionSurfaces,
  });
  api.state = panels.state;
  api.actions = actions;
  api.changeLens = (next) => {
    setLens(next);
    if (next === "mission") openMissionSurfaces();
    else closeMissionSurfaces();
  };
  return null;
}

async function mount(multi = true) {
  dom.window.localStorage.clear();
  const api = {};
  const root = createRoot(document.createElement("div"));
  const render = (nextMulti) =>
    act(async () =>
      root.render(React.createElement(Harness, { api, multi: nextMulti })),
    );
  await render(multi);
  return { api, render, unmount: () => act(async () => root.unmount()) };
}

test("leaving Mission is not the person's choice: re-entry reopens its surfaces", async () => {
  const { api, render, unmount } = await mount();
  assert.deepEqual(api.state.tabs, MISSION);
  assert.equal(api.state.userActed, false);

  // The lens leaves Mission without the person touching the control (the
  // session briefly reads as one execution), then comes back.
  await render(false);
  assert.deepEqual(api.state.tabs, []);
  assert.equal(api.state.rightOpen, false);
  assert.equal(api.state.userActed, false, "a lens leave is not a person act");
  await render(true);
  assert.deepEqual(api.state.tabs, MISSION);
  assert.equal(api.state.rightOpen, true);
  assert.equal(api.state.active, "mission-inspector");
  await unmount();
});

test("re-entry keeps open what was open even after the person acted elsewhere", async () => {
  const { api, render, unmount } = await mount();
  // Any person choice sets `userActed`, which refuses a proactive open; the
  // re-entry restore must not be refused by it.
  await act(async () => api.actions.activate("mission-context"));
  assert.equal(api.state.userActed, true);
  await render(false);
  await render(true);
  assert.deepEqual(api.state.tabs, MISSION);
  assert.equal(api.state.rightOpen, true);
  await unmount();
});

test("a surface the person closed in Mission stays closed on re-entry", async () => {
  const { api, render, unmount } = await mount();
  await act(async () => api.actions.close("mission-audit"));
  assert.deepEqual(api.state.tabs, ["mission-inspector", "mission-context"]);
  await render(false);
  await render(true);
  assert.deepEqual(api.state.tabs, ["mission-inspector", "mission-context"]);
  assert.equal(api.state.active, "mission-inspector");

  // All three closed by the person: re-entry opens none.
  await act(async () => api.actions.closeMany(MISSION));
  await render(false);
  await render(true);
  assert.deepEqual(api.state.tabs, []);
  assert.equal(api.state.rightOpen, false);
  await unmount();
});

test("the lens control leaves and re-enters Mission with its surfaces", async () => {
  const { api, unmount } = await mount();
  await act(async () => api.changeLens("conversation"));
  assert.deepEqual(api.state.tabs, []);
  assert.equal(api.state.userActed, false);
  await act(async () => api.changeLens("mission"));
  assert.deepEqual(api.state.tabs, MISSION);
  assert.equal(api.state.active, "mission-inspector");
  await act(async () => api.changeLens("conversation"));
  await act(async () => api.changeLens("mission"));
  assert.deepEqual(api.state.tabs, MISSION);
  await unmount();
});

test("lens moves never mark userActed; reopen is not refused by it", () => {
  const open = reduceCodingSessionSurfacePanels(
    CODING_SESSION_SURFACE_PANELS_CLOSED,
    { type: "openProactive", ids: MISSION, activate: "mission-inspector" },
  );
  const left = reduceCodingSessionSurfacePanels(open, {
    type: "closeForLens",
    ids: MISSION,
    visibleIds: MISSION_VISIBLE,
  });
  assert.deepEqual(left.tabs, []);
  assert.equal(left.rightOpen, false);
  assert.equal(left.userActed, false);

  const back = reduceCodingSessionSurfacePanels(
    { ...left, userActed: true },
    {
      type: "reopenForLens",
      ids: ["mission-context"],
      activate: "mission-inspector",
    },
  );
  assert.deepEqual(back.tabs, ["mission-context"]);
  assert.equal(back.active, "mission-context");
  assert.equal(back.rightOpen, true);
});
