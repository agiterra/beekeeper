/**
 * The installer asked for exactly one name — the lead's — so every other
 * identity was called after its role, on this computer and on the relay
 * (ledger 84; ledger 80 (e) for the relay half). These mount the real form and
 * drive it the way a person does: choose a folder, read the fields that appear,
 * rename one, install.
 *
 * The Tauri bridge is stubbed at `window.__TAURI_INTERNALS__.invoke`, which is
 * the single function `@tauri-apps/api` calls — so the component under test is
 * the production one, wrappers and all.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

/** Every `invoke` this mount saw, in order. */
let calls = [];
/** Command name → what the stub answers with. */
let answers = {};

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
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      const answer = answers[command];
      if (typeof answer === "function") return answer(args);
      return answer ?? null;
    },
  };
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  calls = [];
  answers = {};
});

after(() => dom.window.close());

const PICKED = {
  directory: "/packs",
  packs: [
    {
      role: "lead",
      personaName: "lead",
      packDir: "/packs/lead",
      defaultName: "Keystone",
      installed: true,
    },
    {
      role: "builder",
      personaName: "builder",
      packDir: "/packs/builder",
      defaultName: "builder",
      installed: true,
    },
    {
      role: "designer",
      personaName: "designer",
      packDir: "/packs/designer",
      defaultName: "designer",
      installed: true,
    },
  ],
  skipped: [],
};

const installedRow = (role, name, overrides = {}) => ({
  personaId: `crew-role:${role}`,
  personaName: role,
  role,
  agentPubkey: role.padEnd(64, "0"),
  agentName: name,
  packDir: `/packs/${role}`,
  refreshed: true,
  renamed: false,
  seated: role !== "designer",
  ...overrides,
});

async function mountForm(onInstalled = () => {}) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { InstallCrewRolesForm } = await import("./InstallCrewRolesDialog.tsx");
  await act(async () => {
    render(
      React.createElement(InstallCrewRolesForm, {
        onClose: () => {},
        onInstalled,
      }),
    );
  });
  return { React, act };
}

test("no folder chosen means no name fields at all", async () => {
  const { screen } = await import("@testing-library/react");
  await mountForm();
  assert.equal(screen.queryByTestId("install-crew-roles-names"), null);
  assert.equal(
    screen.getByTestId("install-crew-roles-submit").disabled,
    true,
    "Install cannot run before a folder has been read",
  );
});

test("choosing a folder renders one field per pack, lead first, on the names those identities already carry", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.pick_crew_role_packs_directory = PICKED;
  await mountForm();

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });

  const list = screen.getByTestId("install-crew-roles-names");
  const inputs = [...list.querySelectorAll("input")];
  assert.deepEqual(
    inputs.map((input) => input.getAttribute("data-testid")),
    [
      "install-crew-roles-name-lead",
      "install-crew-roles-name-builder",
      "install-crew-roles-name-designer",
    ],
    "the field list is the scan's order — the lead's row first",
  );
  assert.deepEqual(
    inputs.map((input) => input.value),
    ["Keystone", "builder", "designer"],
    "a field starts on the name that identity carries, not on its role",
  );
  assert.deepEqual(
    [...list.querySelectorAll("label")].map((label) => label.textContent),
    ["Lead", "Builder", "Designer"],
  );
  assert.equal(
    screen.getByTestId("install-crew-roles-path").value,
    "/packs",
    "the chosen folder is shown",
  );
});

test("install submits a role→name map carrying the rename and every untouched name", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.pick_crew_role_packs_directory = PICKED;
  answers.install_crew_role_packs = {
    teamId: "t",
    teamName: "Team roles",
    installed: [
      installedRow("lead", "Keystone"),
      installedRow("builder", "builder"),
      installedRow("designer", "Banksy", { renamed: true }),
    ],
    skipped: [],
    seated: ["lead", "builder"],
    dropped: ["architect", "runner"],
    profileSyncError: null,
  };

  let installed = null;
  await mountForm((result) => {
    installed = result;
  });

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });
  await act(async () => {
    fireEvent.change(screen.getByTestId("install-crew-roles-name-designer"), {
      target: { value: "Banksy" },
    });
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-submit"));
  });

  const call = calls.find(
    (entry) => entry.command === "install_crew_role_packs",
  );
  assert.ok(call, "the install ran");
  assert.equal(call.args.directory, "/packs");
  assert.deepEqual(
    call.args.names,
    { lead: "Keystone", builder: "builder", designer: "Banksy" },
    "the map covers every scanned role, so the backend never guesses",
  );
  assert.ok(installed, "the caller was told the install landed");

  const rows = [
    ...screen.getByTestId("install-crew-roles-result").querySelectorAll("li"),
  ].map((item) => item.textContent);
  // The designer's row carries the rename *and* the fact that it holds no
  // seat: both are true of this install, and dropping either would let one of
  // them go unsaid.
  assert.deepEqual(rows, [
    "Lead — Keystone (already installed from this pack — role and pack link refreshed)",
    "Builder — builder (already installed from this pack — role and pack link refreshed)",
    "Designer — Banksy (renamed; profile republished; installed, but not seated in the team)",
  ]);
});

test("a blank field installs the name that identity already carries, never an empty one", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.pick_crew_role_packs_directory = PICKED;
  answers.install_crew_role_packs = {
    teamId: "t",
    teamName: "Team roles",
    installed: [installedRow("lead", "Keystone")],
    skipped: [],
    seated: ["lead"],
    dropped: [],
    profileSyncError: null,
  };
  await mountForm();

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });
  await act(async () => {
    fireEvent.change(screen.getByTestId("install-crew-roles-name-lead"), {
      target: { value: "   " },
    });
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-submit"));
  });

  const call = calls.find(
    (entry) => entry.command === "install_crew_role_packs",
  );
  assert.equal(call.args.names.lead, "Keystone");
});

test("a folder with no role packs says so before anything is installed", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.pick_crew_role_packs_directory = {
    directory: "/empty",
    packs: [],
    skipped: [{ path: "/empty/notes", reason: "no persona…" }],
  };
  await mountForm();

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });

  assert.ok(screen.getByTestId("install-crew-roles-empty"));
  assert.equal(screen.queryByTestId("install-crew-roles-names"), null);
  assert.equal(
    screen.getByTestId("install-crew-roles-submit").disabled,
    true,
    "there is nothing to install, so the button must not claim there is",
  );
  assert.equal(
    calls.filter((entry) => entry.command === "install_crew_role_packs").length,
    0,
  );
});

test("an unreadable folder is reported as the folder, and no names appear", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.pick_crew_role_packs_directory = () => {
    throw new Error("permission denied");
  };
  await mountForm();

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });

  assert.match(
    screen.getByTestId("install-crew-roles-error").textContent,
    /^That folder could not be read: /,
  );
  assert.equal(screen.queryByTestId("install-crew-roles-names"), null);
});

test("a failed profile publish is shown, and no renamed row claims a republish", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.pick_crew_role_packs_directory = PICKED;
  answers.install_crew_role_packs = {
    teamId: "t",
    teamName: "Team roles",
    installed: [installedRow("designer", "Banksy", { renamed: true })],
    skipped: [],
    seated: [],
    dropped: [],
    profileSyncError:
      "these identities were installed but the relay still knows them by their previous name — Banksy: relay unreachable",
  };
  await mountForm();

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-submit"));
  });

  assert.match(
    screen.getByTestId("install-crew-roles-profile-sync-error").textContent,
    /the relay still knows them by their previous name/,
  );
  const line = screen
    .getByTestId("install-crew-roles-result")
    .querySelector("li").textContent;
  assert.doesNotMatch(
    line,
    /republished/,
    "a row must not claim a publish that failed",
  );
  assert.match(line, /the relay may still know it by the old name/);
});
