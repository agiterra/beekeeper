/**
 * The Agents tab names the project whose role packs the installer will read.
 *
 * The tab's route names no project (ledger 85's reachability finding), so the
 * installer falls back to one. A fallback nobody can see is a guess, and a
 * guess that silently picks the wrong checkout installs the wrong packs — so
 * with a choice to make, the choice is on the surface and switchable.
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
    HTMLInputElement: dom.window.HTMLInputElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    CustomEvent: dom.window.CustomEvent,
    Event: dom.window.Event,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  // Radix's popper measures its trigger; jsdom ships neither observer.
  class NoopObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  globalThis.ResizeObserver = NoopObserver;
  dom.window.ResizeObserver = NoopObserver;
  globalThis.DOMRect = dom.window.DOMRect ?? class {};
  dom.window.HTMLElement.prototype.hasPointerCapture = () => false;
  dom.window.HTMLElement.prototype.setPointerCapture = () => {};
  dom.window.HTMLElement.prototype.releasePointerCapture = () => {};
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const OWNER = "a".repeat(64);

function makeProject(dtag, name) {
  return {
    id: `${OWNER}:${dtag}`,
    dtag,
    owner: OWNER,
    name,
    description: "",
    createdAt: 1,
    address: `30621:${OWNER}:${dtag}`,
    repoAddrs: [],
    agentAddrs: [],
    channelIds: [],
    visibility: "public",
    members: [],
    icon: null,
    color: null,
  };
}

const GENERAL = makeProject("general", "General");
const ATTIC = makeProject("attic", "Attic");

async function mountSelector(props) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { RolePacksProjectSelector } = await import(
    "./RolePacksProjectSelector.tsx"
  );
  await act(async () => {
    render(React.createElement(RolePacksProjectSelector, props));
  });
}

test("two projects: the selector renders and names the resolved one", async () => {
  const { screen } = await import("@testing-library/react");
  await mountSelector({
    onSelect: () => {},
    project: GENERAL,
    projects: [GENERAL, ATTIC],
  });

  const selector = screen.getByTestId("role-packs-project-selector");
  assert.match(selector.textContent, /Role packs for:/);
  assert.match(
    selector.textContent,
    /General/,
    "the project the installer would use is named, not left implied",
  );
});

test("one project: no dropdown, but the project is still named", async () => {
  const { screen } = await import("@testing-library/react");
  await mountSelector({
    onSelect: () => {},
    project: GENERAL,
    projects: [GENERAL],
  });

  assert.equal(screen.queryByTestId("role-packs-project-selector"), null);
  assert.equal(screen.queryByTestId("role-packs-project-trigger"), null);
  assert.equal(
    screen.getByTestId("role-packs-project-name").textContent,
    "General",
  );
  assert.match(
    screen.getByTestId("role-packs-project-static").textContent,
    /Role packs for:/,
  );
});

test("no project at all: no selector, and nothing claimed", async () => {
  const { screen } = await import("@testing-library/react");
  await mountSelector({
    onSelect: () => {},
    project: null,
    projects: [],
  });

  assert.equal(screen.queryByTestId("role-packs-project-selector"), null);
  assert.equal(screen.queryByTestId("role-packs-project-static"), null);
});

test("switching names another project and reports the switch once", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  const chosen = [];
  await mountSelector({
    onSelect: (id) => chosen.push(id),
    project: GENERAL,
    projects: [GENERAL, ATTIC],
  });

  await act(async () => {
    fireEvent.pointerDown(
      screen.getByTestId("role-packs-project-trigger"),
      new dom.window.MouseEvent("pointerdown", {
        bubbles: true,
        button: 0,
        ctrlKey: false,
      }),
    );
  });

  const option = screen.getByRole("menuitem", { name: /Attic/ });
  await act(async () => {
    fireEvent.click(option);
  });

  assert.deepEqual(chosen, [ATTIC.id]);
});

test("a fallback project is disclosed beside the selector, not shown as the selection", async () => {
  const { screen } = await import("@testing-library/react");
  await mountSelector({
    fallbackNote:
      "No project is chosen in the project filter, so this fell back to the first project in the list.",
    onSelect: () => {},
    project: GENERAL,
    projects: [GENERAL, ATTIC],
  });

  assert.equal(
    screen.getByTestId("role-packs-project-fallback").textContent,
    "No project is chosen in the project filter, so this fell back to the first project in the list.",
  );
});

test("a chosen project carries no fallback note", async () => {
  const { screen } = await import("@testing-library/react");
  await mountSelector({
    fallbackNote: null,
    onSelect: () => {},
    project: ATTIC,
    projects: [GENERAL, ATTIC],
  });

  assert.equal(screen.queryByTestId("role-packs-project-fallback"), null);
});

test("General plus one real project, directory on General: the other project and the fallback are both on screen", async () => {
  const { screen } = await import("@testing-library/react");
  const { resolveRolePacksProject, rolePacksProjectCandidates } = await import(
    "../lib/rolePacksProject.ts"
  );
  const { rolePacksProjectFallbackNote } = await import(
    "./installCrewRolesCopy.ts"
  );
  const localGeneral = {
    ...makeProject("general", "General"),
    id: "local:general",
    owner: "",
  };
  // The Agents tab's one list: the local General placeholder plus Attic.
  const candidates = rolePacksProjectCandidates([localGeneral, ATTIC]);
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: localGeneral.id,
    projects: candidates,
    workdirsByProject: undefined,
  });
  assert.equal(resolved.project?.id, ATTIC.id);

  await mountSelector({
    fallbackNote: rolePacksProjectFallbackNote(resolved.source, true),
    onSelect: () => {},
    project: resolved.project,
    projects: candidates,
  });

  assert.equal(
    screen.queryByTestId("role-packs-project-trigger"),
    null,
    "one selectable project: nothing to choose, so no dropdown",
  );
  assert.equal(
    screen.getByTestId("role-packs-project-name").textContent,
    "Attic",
  );
  assert.equal(
    screen.getByTestId("role-packs-project-fallback").textContent,
    "The project chosen in the project filter cannot hold role packs, so this fell back to the only project.",
  );
});
