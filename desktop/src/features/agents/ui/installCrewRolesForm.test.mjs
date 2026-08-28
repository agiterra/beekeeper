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

async function mountForm(onInstalled = () => {}, props = {}) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { InstallCrewRolesForm } = await import("./InstallCrewRolesDialog.tsx");
  await act(async () => {
    render(
      React.createElement(InstallCrewRolesForm, {
        onClose: () => {},
        onInstalled,
        ...props,
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

// ── The project's own role packs, already chosen (ledger 85) ─────────────────
//
// The installer opened on "No folder chosen", so a new operator had to know
// that a project's packs live in `<checkout>/personas/roles`. When the project
// showing has a checkout directory and that folder scans to something, the
// dialog opens on it. When it does not, it says which of the three reasons it
// is and leaves the picker exactly as it was — a folder nobody can find is not
// a folder to pre-fill with.

const PROJECT = { address: "30621:owner:beekeeper" };

/** The workdir store's answer with one checkout recorded for `PROJECT`. */
const workdirState = (path) => ({
  version: 1,
  byProject: path ? { [PROJECT.address]: { path, updatedAt: "now" } } : {},
  byChannel: {},
  mru: [],
  pending: {},
});

const PROJECT_SCAN = {
  directory: "/checkout/personas/roles",
  exists: true,
  packs: PICKED.packs,
  skipped: [],
};

test("a project whose checkout holds role packs opens on that folder, already chosen", async () => {
  const { screen } = await import("@testing-library/react");
  answers.get_coding_session_workdir_state = workdirState("/checkout");
  answers.scan_project_role_packs_directory = PROJECT_SCAN;
  await mountForm(() => {}, { project: PROJECT });

  assert.equal(
    screen.getByTestId("install-crew-roles-path").value,
    "/checkout/personas/roles",
    "the folder is already chosen, not left on 'No folder chosen'",
  );
  assert.equal(
    screen.getByTestId("install-crew-roles-folder-label").textContent,
    "The project's role packs",
    "and it says whose folder that is",
  );
  const inputs = [
    ...screen.getByTestId("install-crew-roles-names").querySelectorAll("input"),
  ];
  assert.deepEqual(
    inputs.map((input) => input.value),
    ["Keystone", "builder", "designer"],
    "the same name fields the picker path renders, from the same scan",
  );
  assert.equal(
    screen.getByTestId("install-crew-roles-submit").disabled,
    false,
    "there is something to install, and the button says so",
  );
  assert.equal(
    calls.filter((entry) => entry.command === "pick_crew_role_packs_directory")
      .length,
    0,
    "nothing opened the OS picker behind the operator's back",
  );
  const scan = calls.find(
    (entry) => entry.command === "scan_project_role_packs_directory",
  );
  assert.deepEqual(scan.args, { checkoutDir: "/checkout" });
  assert.equal(screen.queryByTestId("install-crew-roles-project-note"), null);
});

test("a project with no checkout directory says so, and the picker is untouched", async () => {
  const { screen } = await import("@testing-library/react");
  answers.get_coding_session_workdir_state = workdirState(null);
  await mountForm(() => {}, { project: PROJECT });

  assert.equal(
    screen.getByTestId("install-crew-roles-project-note").textContent,
    "This project has no checkout directory yet — set one in Project settings, or choose a folder",
  );
  assert.equal(screen.getByTestId("install-crew-roles-path").value, "");
  assert.equal(screen.queryByTestId("install-crew-roles-names"), null);
  assert.equal(screen.queryByTestId("install-crew-roles-folder-label"), null);
  assert.equal(
    calls.filter(
      (entry) => entry.command === "scan_project_role_packs_directory",
    ).length,
    0,
    "with no checkout there is nothing to scan",
  );
  assert.equal(screen.getByTestId("install-crew-roles-submit").disabled, true);
});

test("a checkout with no personas/roles folder names the folder it looked for", async () => {
  const { screen } = await import("@testing-library/react");
  answers.get_coding_session_workdir_state = workdirState("/checkout");
  answers.scan_project_role_packs_directory = {
    directory: "/checkout/personas/roles",
    exists: false,
    packs: [],
    skipped: [],
  };
  await mountForm(() => {}, { project: PROJECT });

  const note = screen.getByTestId(
    "install-crew-roles-project-note",
  ).textContent;
  assert.match(note, /\/checkout\/personas\/roles/);
  assert.match(note, /is not there/);
  assert.equal(screen.getByTestId("install-crew-roles-path").value, "");
  assert.equal(screen.queryByTestId("install-crew-roles-names"), null);
});

test("a personas/roles folder holding no packs is reported as empty, not as missing", async () => {
  const { screen } = await import("@testing-library/react");
  answers.get_coding_session_workdir_state = workdirState("/checkout");
  answers.scan_project_role_packs_directory = {
    directory: "/checkout/personas/roles",
    exists: true,
    packs: [],
    skipped: [
      { path: "/checkout/personas/roles/notes", reason: "no persona…" },
    ],
  };
  await mountForm(() => {}, { project: PROJECT });

  const note = screen.getByTestId(
    "install-crew-roles-project-note",
  ).textContent;
  assert.match(note, /\/checkout\/personas\/roles/);
  assert.match(note, /holds no role packs/);
  assert.doesNotMatch(note, /is not there/);
  assert.equal(screen.getByTestId("install-crew-roles-path").value, "");
});

test("outside a project the dialog is exactly what it was: no lookup, no note", async () => {
  const { screen } = await import("@testing-library/react");
  answers.get_coding_session_workdir_state = workdirState("/checkout");
  answers.scan_project_role_packs_directory = PROJECT_SCAN;
  await mountForm();

  assert.equal(screen.getByTestId("install-crew-roles-path").value, "");
  assert.equal(screen.queryByTestId("install-crew-roles-project-note"), null);
  assert.equal(screen.queryByTestId("install-crew-roles-folder-label"), null);
  assert.equal(
    calls.filter(
      (entry) =>
        entry.command === "scan_project_role_packs_directory" ||
        entry.command === "get_coding_session_workdir_state",
    ).length,
    0,
    "no project, no lookup",
  );
});

test("choosing another folder over the project's default drops the project label", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  answers.get_coding_session_workdir_state = workdirState("/checkout");
  answers.scan_project_role_packs_directory = PROJECT_SCAN;
  answers.pick_crew_role_packs_directory = PICKED;
  await mountForm(() => {}, { project: PROJECT });

  await act(async () => {
    fireEvent.click(screen.getByTestId("install-crew-roles-choose"));
  });

  assert.equal(screen.getByTestId("install-crew-roles-path").value, "/packs");
  assert.equal(
    screen.queryByTestId("install-crew-roles-folder-label"),
    null,
    "a folder the operator picked is not the project's role packs",
  );
});
