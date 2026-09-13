import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
let calls = [];
let answers = {};
before(() => {
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    HTMLInputElement: dom.window.HTMLInputElement,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      const answer = answers[command];
      return typeof answer === "function" ? answer(args) : (answer ?? null);
    },
  };
});
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  calls = [];
  answers = {};
});
after(() => dom.window.close());
const projectA = `30621:${"a".repeat(64)}:a`;
const projectB = `30621:${"a".repeat(64)}:b`;
const draftA = {
  setupId: "draft-a",
  projectRef: projectA,
  projectDirectory: "/repo/a",
  rolesDirectory: "/draft/a/roles",
  draftDirectory: "/draft/a",
  status: "draft",
  intent: "Project A",
  ownerPubkey: "a".repeat(64),
  relayUrl: "wss://example.test",
  roles: ["lead"],
  createdAt: "2026-09-11",
};
async function harness() {
  const React = (await import("react")).default;
  const { ProjectTeamSetupForm } = await import(
    "../ui/ProjectTeamSetupForm.tsx"
  );
  const testing = await import("@testing-library/react");
  const element = (projectRef) =>
    React.createElement(ProjectTeamSetupForm, {
      key: projectRef,
      projectRef,
      relayUrl: "wss://example.test",
    });
  return { ...testing, element };
}

test("importing the scoped form does not start application-wide IPC discovery", async () => {
  // The community wrapper imports mediaUrl's eager proxy polling. Keep this
  // reusable form independent: those delayed calls otherwise leak between
  // component tests and make the read-only contract depend on machine load.
  await harness();
  assert.deepEqual(calls, [], "Importing the form must not invoke native APIs");
});

test("a late saved-draft read cannot replace the newly selected project's form", async () => {
  let resolveA;
  answers.project_team_setup_get = ({ projectRef }) =>
    projectRef === projectA
      ? new Promise((resolve) => {
          resolveA = resolve;
        })
      : null;
  answers.get_coding_session_workdir_state = { byProject: {} };
  const { render, waitFor, act, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() => assert.equal(typeof resolveA, "function"));
  view.rerender(element(projectB));
  await waitFor(() =>
    assert.ok(view.getByLabelText("Local project repository")),
  );
  await act(async () => resolveA(draftA));
  assert.equal(view.queryByText("Draft ready"), null);
  assert.equal(
    calls.filter((call) => call.command === "project_team_setup_prepare")
      .length,
    0,
  );
});

test("an in-flight prepare stays bound to its original project after scope remount", async () => {
  let resolvePrepare;
  answers.project_team_setup_get = null;
  answers.get_coding_session_workdir_state = {
    byProject: { [projectA]: { path: "/repo/a" } },
  };
  answers.project_team_setup_prepare = () =>
    new Promise((resolve) => {
      resolvePrepare = resolve;
    });
  const { render, waitFor, act, fireEvent, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() =>
    assert.ok(view.getByLabelText("Local project repository")),
  );
  fireEvent.change(
    view.getByLabelText("What should this project accomplish?"),
    { target: { value: "Project A" } },
  );
  fireEvent.click(view.getByRole("button", { name: "Prepare draft" }));
  await waitFor(() => assert.equal(typeof resolvePrepare, "function"));
  view.rerender(element(projectB));
  await waitFor(() =>
    assert.ok(view.getByLabelText("Local project repository")),
  );
  await act(async () => resolvePrepare(draftA));
  assert.equal(view.queryByText("Draft ready"), null);
  const writes = calls.filter(
    (call) => call.command === "project_team_setup_prepare",
  );
  assert.equal(writes.length, 1);
  assert.equal(writes[0].args.projectRef, projectA);
  assert.equal(writes[0].args.expectedRelayUrl, "wss://example.test");
});

test("saving a checked version uses the scoped host snapshot and does not publish", async () => {
  answers.project_team_setup_get = draftA;
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_validate = {
    setupId: draftA.setupId,
    status: "draft",
    valid: true,
    roles: ["lead"],
    diagnostics: [],
  };
  answers.project_team_setup_snapshot = {
    setupId: draftA.setupId,
    snapshotId: "c".repeat(64),
    rolesDirectory: "/snapshots/c/roles",
    manifestPath: "/snapshots/c/manifest.json",
    roles: ["lead", "reviewer"],
  };
  answers.project_team_setup_get_publication_options = {
    currentSourceEventId: null,
    suggestedDestination: null,
    sourceExpectation: { kind: "if_unset" },
    publication: null,
  };
  const { render, waitFor, fireEvent, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() => assert.ok(view.getByText("Draft ready")));
  assert.equal(
    view.queryByRole("button", { name: "Save checked version" }),
    null,
  );
  fireEvent.click(view.getByRole("button", { name: "Check draft" }));
  await waitFor(() =>
    assert.ok(view.getByRole("button", { name: "Save checked version" })),
  );
  fireEvent.click(view.getByRole("button", { name: "Save checked version" }));
  await waitFor(() => assert.ok(view.getByText(/Checked version saved/)));
  assert.ok(view.getByText("Roles: lead, reviewer"));
  assert.deepEqual(
    calls.filter(({ command }) => command === "project_team_setup_snapshot"),
    [
      {
        command: "project_team_setup_snapshot",
        args: {
          projectRef: projectA,
          expectedRelayUrl: draftA.relayUrl,
          setupId: draftA.setupId,
        },
      },
    ],
  );
  assert.deepEqual(
    calls.filter(
      ({ command }) =>
        ![
          "project_team_setup_get",
          "get_coding_session_workdir_state",
          "project_team_setup_validate",
          "project_team_setup_snapshot",
          "project_team_setup_get_publication_options",
        ].includes(command),
    ),
    [],
    "Snapshot workbench made unexpected IPC calls",
  );
  answers.project_team_setup_snapshot = () => {
    throw { message: "Draft changed and is no longer valid." };
  };
  fireEvent.click(view.getByRole("button", { name: "Save checked version" }));
  await waitFor(() =>
    assert.match(view.getByRole("alert").textContent, /no longer valid/),
  );
  assert.equal(view.queryByText(/Checked version saved/), null);
});

test("publication, adopted-source installation, and lead handoff each use one scoped retry identity", async () => {
  answers.project_team_setup_get = {
    ...draftA,
    latestSnapshotId: "d".repeat(64),
  };
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_snapshot = {
    setupId: draftA.setupId,
    snapshotId: "d".repeat(64),
    rolesDirectory: "/saved/roles",
    manifestPath: "/saved/manifest.json",
    roles: ["lead"],
  };
  const destination = {
    repoRef: `30617:${"b".repeat(64)}:tankloop-packs`,
    packPath: "personas/roles",
    baseCommit: "e".repeat(40),
    createAnnouncement: null,
  };
  const sourceExpectation = { kind: "expected", eventId: "f".repeat(64) };
  answers.project_team_setup_get_publication_options = {
    currentSourceEventId: sourceExpectation.eventId,
    suggestedDestination: destination,
    sourceExpectation,
    publication: null,
  };
  answers.project_team_setup_start_publication = {
    publicationId: "publication-1",
    setupId: draftA.setupId,
    status: "source_unknown",
    snapshotId: "d".repeat(64),
    destination,
    sourceExpectation,
    candidateRef: "refs/heads/setup/publication-1",
    candidateCommit: "1".repeat(40),
    sourceEventId: null,
    message: "Checking whether the shared source was adopted.",
  };
  answers.project_team_setup_continue_publication = {
    publicationId: "publication-1",
    setupId: draftA.setupId,
    status: "adopted",
    snapshotId: "d".repeat(64),
    destination,
    sourceExpectation,
    candidateRef: "refs/heads/setup/publication-1",
    candidateCommit: "1".repeat(40),
    sourceEventId: "2".repeat(64),
    message: null,
  };
  const activation = {
    publicationId: "publication-1",
    source: {
      repoRef: destination.repoRef,
      commit: "1".repeat(40),
      packPath: destination.packPath,
    },
    installation: {
      status: "not_installed",
      installedRoles: [],
      message: null,
    },
    lead: {
      status: "needs_channel",
      channelId: null,
      sessionRef: null,
      message: "Create a project session channel before starting its lead.",
    },
  };
  answers.project_team_setup_get_activation = activation;
  answers.project_team_setup_install_adopted_roles = {
    ...activation,
    installation: {
      status: "installed",
      installedRoles: [
        {
          role: "lead",
          agentPubkey: "9".repeat(64),
          packRef: {
            repo: destination.repoRef,
            sha: "1".repeat(40),
            role: "lead",
            path: "personas/roles/lead",
          },
        },
      ],
      message: "Installed from the adopted revision.",
    },
    lead: {
      status: "needs_channel",
      channelId: null,
      sessionRef: null,
      message: null,
    },
  };
  answers.project_team_setup_start_lead = {
    ...answers.project_team_setup_install_adopted_roles,
    lead: {
      status: "started",
      channelId: "project-session-channel",
      sessionRef: "lead-session-1",
      message: "The reserved project lead is running.",
    },
  };
  answers.project_team_setup_ensure_lead_channel = {
    ...answers.project_team_setup_install_adopted_roles,
    lead: {
      status: "ready",
      channelId: "project-session-channel",
      sessionRef: null,
      message: "Project session channel recorded for this publication.",
    },
  };
  const { render, waitFor, fireEvent, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() =>
    assert.ok(view.getByRole("button", { name: "Publish checked version" })),
  );
  fireEvent.click(
    view.getByRole("button", { name: "Publish checked version" }),
  );
  await waitFor(() =>
    assert.ok(view.getByText("Shared source adoption needs confirmation")),
  );
  assert.deepEqual(
    calls.find(
      ({ command }) => command === "project_team_setup_start_publication",
    ),
    {
      command: "project_team_setup_start_publication",
      args: {
        projectRef: projectA,
        expectedRelayUrl: draftA.relayUrl,
        setupId: draftA.setupId,
        destination,
        sourceExpectation,
        output: { kind: "snapshot", snapshotId: "d".repeat(64) },
      },
    },
  );
  fireEvent.click(view.getByRole("button", { name: "Retry publication" }));
  await waitFor(() =>
    assert.ok(view.getByText("Shared project source adopted")),
  );
  assert.equal(
    calls.filter(
      ({ command }) => command === "project_team_setup_start_publication",
    ).length,
    1,
  );
  assert.deepEqual(
    calls.find(
      ({ command }) => command === "project_team_setup_continue_publication",
    ),
    {
      command: "project_team_setup_continue_publication",
      args: {
        projectRef: projectA,
        expectedRelayUrl: draftA.relayUrl,
        setupId: draftA.setupId,
        publicationId: "publication-1",
      },
    },
  );
  await waitFor(() =>
    assert.ok(view.getByRole("button", { name: "Install project roles" })),
  );
  fireEvent.click(view.getByRole("button", { name: "Install project roles" }));
  await waitFor(() =>
    assert.ok(view.getByText(/Roles available on this computer: lead/)),
  );
  fireEvent.click(
    view.getByRole("button", { name: "Create project session channel" }),
  );
  await waitFor(() =>
    assert.ok(view.getByRole("button", { name: "Start project lead" })),
  );
  assert.equal(
    calls.filter(({ command }) => command === "create_channel").length,
    0,
  );
  assert.deepEqual(
    calls.find(
      ({ command }) => command === "project_team_setup_ensure_lead_channel",
    )?.args,
    {
      projectRef: projectA,
      expectedRelayUrl: draftA.relayUrl,
      setupId: draftA.setupId,
      publicationId: "publication-1",
    },
  );
  fireEvent.click(view.getByRole("button", { name: "Start project lead" }));
  await waitFor(() =>
    assert.ok(
      view.getByText(/Project lead started in project-session-channel/),
    ),
  );
  assert.deepEqual(
    calls.find(
      ({ command }) => command === "project_team_setup_install_adopted_roles",
    ),
    {
      command: "project_team_setup_install_adopted_roles",
      args: {
        projectRef: projectA,
        expectedRelayUrl: draftA.relayUrl,
        setupId: draftA.setupId,
        publicationId: "publication-1",
      },
    },
  );
  assert.deepEqual(
    calls.find(({ command }) => command === "project_team_setup_start_lead"),
    {
      command: "project_team_setup_start_lead",
      args: {
        projectRef: projectA,
        expectedRelayUrl: draftA.relayUrl,
        setupId: draftA.setupId,
        publicationId: "publication-1",
        channelId: "project-session-channel",
      },
    },
  );
});

test("missing host destination blocks publication rather than inventing a new source", async () => {
  answers.project_team_setup_get = {
    ...draftA,
    latestSnapshotId: "d".repeat(64),
  };
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_snapshot = {
    setupId: draftA.setupId,
    snapshotId: "d".repeat(64),
    rolesDirectory: "/saved/roles",
    manifestPath: "/saved/manifest.json",
    roles: ["lead"],
  };
  answers.project_team_setup_get_publication_options = {
    currentSourceEventId: null,
    suggestedDestination: null,
    sourceExpectation: { kind: "if_unset" },
    publication: null,
  };
  const { render, waitFor, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() =>
    assert.ok(view.getByText(/could not identify a project destination/)),
  );
  assert.equal(
    view.queryByRole("button", { name: "Publish checked version" }),
    null,
  );
  assert.equal(
    calls.some(
      ({ command }) => command === "project_team_setup_start_publication",
    ),
    false,
  );
});

test("reopening reverifies the saved version without treating the current draft as checked", async () => {
  const snapshotId = "d".repeat(64);
  answers.project_team_setup_get = { ...draftA, latestSnapshotId: snapshotId };
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_snapshot = {
    setupId: draftA.setupId,
    snapshotId,
    rolesDirectory: "/saved/roles",
    manifestPath: "/saved/manifest.json",
    roles: ["lead"],
  };
  const { render, waitFor, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() => assert.ok(view.getByText(/Checked version saved/)));
  assert.ok(view.getByText("The draft has not been checked yet."));
  assert.deepEqual(
    calls
      .filter(({ command }) => command === "project_team_setup_snapshot")
      .map(({ args }) => args),
    [
      {
        projectRef: projectA,
        expectedRelayUrl: draftA.relayUrl,
        setupId: draftA.setupId,
        snapshotId,
      },
    ],
  );
  assert.equal(
    calls.some(({ command }) => command === "project_team_setup_prepare"),
    false,
  );
});
