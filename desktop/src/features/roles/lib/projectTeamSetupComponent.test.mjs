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
async function harness({ agents, renderAuthoring } = {}) {
  const React = (await import("react")).default;
  const { ProjectTeamSetupForm } = await import(
    "../ui/ProjectTeamSetupForm.tsx"
  );
  const { ProjectTeamSetupAgentsContext } = await import(
    "../ui/ProjectTeamSetupAgents.tsx"
  );
  const testing = await import("@testing-library/react");
  const element = (projectRef) => {
    const form = React.createElement(ProjectTeamSetupForm, {
      key: projectRef,
      projectRef,
      projectName: "Tank Loop",
      relayUrl: "wss://example.test",
      renderAuthoring,
    });
    return agents
      ? React.createElement(
          ProjectTeamSetupAgentsContext.Provider,
          { value: agents },
          form,
        )
      : form;
  };
  return { ...testing, React, element };
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
  assert.equal(view.queryByTestId("project-team-setup-draft"), null);
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
  assert.equal(view.queryByTestId("project-team-setup-draft"), null);
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
  await waitFor(() => assert.ok(view.getByText("Author project roles")));
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
  const technical = view.getByTestId("project-team-setup-technical-details");
  assert.equal(technical.open, false, "Technical details start collapsed");
  assert.match(technical.textContent, /Roles: lead, reviewer/);
  assert.ok(technical.textContent.includes("c".repeat(64)));
  assert.equal(
    view
      .getByTestId("project-team-setup-draft")
      .textContent.replace(technical.textContent, "")
      .includes("c".repeat(64)),
    false,
    "the snapshot id appears only under Technical details",
  );
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "Publishing is blocked",
    ),
  );
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
    throw {
      code: "source_changed",
      message: "Draft changed and is no longer valid.",
    };
  };
  fireEvent.click(view.getByRole("button", { name: "Save checked version" }));
  await waitFor(() =>
    assert.ok(
      view
        .getAllByRole("alert")
        .some((alert) => /shared roles changed/.test(alert.textContent)),
    ),
  );
  const alert = view
    .getAllByRole("alert")
    .find((entry) => /shared roles changed/.test(entry.textContent));
  assert.match(
    alert.querySelector("p").textContent,
    /^The project's shared roles changed since this draft was checked/,
  );
  assert.match(
    alert.querySelector("details").textContent,
    /no longer valid/,
    "the native message stays available under Details",
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
      leadPubkey: null,
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
      leadPubkey: "9".repeat(64),
      message: null,
    },
  };
  answers.project_team_setup_start_lead = {
    ...answers.project_team_setup_install_adopted_roles,
    lead: {
      status: "started",
      channelId: "project-session-channel",
      sessionRef: "lead-session-1",
      leadPubkey: "9".repeat(64),
      message: "The reserved project lead is running.",
    },
  };
  answers.project_team_setup_ensure_lead_channel = {
    ...answers.project_team_setup_install_adopted_roles,
    lead: {
      status: "ready",
      channelId: "project-session-channel",
      sessionRef: null,
      leadPubkey: "9".repeat(64),
      message: "Project session channel recorded for this publication.",
    },
  };
  const { render, waitFor, fireEvent, element } = await harness({
    agents: {
      names: new Map([["9".repeat(64), "Loom"]]),
      rename: null,
      agents: [
        {
          pubkey: "9".repeat(64),
          name: "Loom",
          homeRole: "lead",
          projectRef: projectA,
          status: "stopped",
        },
      ],
    },
  });
  const view = render(element(projectA));
  await waitFor(() =>
    assert.ok(view.getByRole("button", { name: "Publish checked version" })),
  );
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "Publish project roles",
    ),
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
  await waitFor(() => assert.ok(view.getByTestId("project-team-setup-roster")));
  const roster = view.getByTestId("project-team-setup-roster");
  assert.equal(roster.querySelector("h4").textContent, "Tank Loop agents");
  const installedRow = view.getByTestId("project-team-setup-installed-role");
  assert.match(installedRow.textContent, /Loom/);
  assert.match(installedRow.textContent, /· lead/);
  assert.match(installedRow.textContent, /· Project agent/);
  assert.doesNotMatch(
    installedRow.textContent,
    /99999999…9999/,
    "the pubkey stays under details, not on the row",
  );
  assert.match(
    roster.querySelector("details").textContent,
    /Loom · lead · 99999999…9999/,
  );
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "Start the project lead",
    ),
  );
  // One next action: the channel is prepared as part of starting the lead.
  assert.equal(
    view.queryByRole("button", { name: "Create project session channel" }),
    null,
  );
  fireEvent.click(view.getByRole("button", { name: "Start the project lead" }));
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "Project lead started",
    ),
  );
  assert.equal(
    calls.filter(({ command }) => command === "create_channel").length,
    0,
  );
  assert.deepEqual(
    calls
      .filter(({ command }) =>
        [
          "project_team_setup_ensure_lead_channel",
          "project_team_setup_start_lead",
        ].includes(command),
      )
      .map(({ command }) => command),
    ["project_team_setup_ensure_lead_channel", "project_team_setup_start_lead"],
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
  assert.equal(
    view.getByTestId("project-team-setup-roster-next").textContent,
    "Setup complete. Loom leads Tank Loop. Give Loom a task in its session; it lists the team with bee projects agents and hires only these agents.",
  );
  assert.match(
    view.getByTestId("project-team-setup-lead-session-details").textContent,
    /lead-session-1/,
  );
  assert.equal(
    view
      .getByTestId("project-team-setup-stepper")
      .querySelector('[aria-current="step"]').dataset.state,
    "done",
  );
  const technical = view.getByTestId("project-team-setup-technical-details");
  assert.ok(technical.textContent.includes("1".repeat(40)));
  assert.match(technical.textContent, /Installed from the adopted revision/);
  assert.match(technical.textContent, /lead-session-1/);
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
    assert.ok(
      view.getByText(/couldn't identify where the project's shared roles/),
    ),
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

test("authoring comes first; check and save appear once authoring is created; the brief shown is the native brief", async () => {
  answers.project_team_setup_get = draftA;
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_get_brief = { text: "NATIVE BRIEF: exact text" };
  let report;
  const { React, render, waitFor, fireEvent, act, element } = await harness({
    renderAuthoring: (_draft, _onDraftMayChange, onLaunchObserved) => {
      report = onLaunchObserved;
      return React.createElement(FakeAuthoring, { onLaunchObserved });
    },
  });
  function FakeAuthoring({ onLaunchObserved }) {
    React.useEffect(() => onLaunchObserved(null), [onLaunchObserved]);
    return React.createElement("section", {
      "aria-label": "Project roles authoring",
      "data-testid": "fake-authoring",
    });
  }
  const view = render(element(projectA));
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "Author project roles",
    ),
  );
  assert.equal(view.queryByTestId("project-team-setup-validation"), null);
  await waitFor(() =>
    assert.match(
      view.getByTestId("project-team-setup-next-action").textContent,
      /Start an authoring session/,
    ),
  );
  assert.match(
    view
      .getByTestId("project-team-setup-stepper")
      .querySelector('[aria-current="step"]').textContent,
    /Author/,
  );
  await act(async () => report({ status: "created" }));
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "Check the draft",
    ),
  );
  const authoring = view.getByTestId("fake-authoring");
  const validation = view.getByTestId("project-team-setup-validation");
  assert.ok(
    authoring.compareDocumentPosition(validation) &
      dom.window.Node.DOCUMENT_POSITION_FOLLOWING,
    "authoring renders before Check draft",
  );
  assert.ok(view.getByRole("button", { name: "Check draft" }));

  const brief = view
    .getByText("Setup brief sent to the authoring session")
    .closest("details");
  brief.open = true;
  fireEvent(brief, new dom.window.Event("toggle"));
  await waitFor(() =>
    assert.equal(
      view.getByLabelText("Setup brief").value,
      "NATIVE BRIEF: exact text",
    ),
  );
  assert.deepEqual(
    calls.find(({ command }) => command === "project_team_setup_get_brief")
      ?.args,
    {
      projectRef: projectA,
      expectedRelayUrl: draftA.relayUrl,
      setupId: draftA.setupId,
    },
  );
});

test("the Roles page mount path invokes only read-only commands and describes the last recorded status", async () => {
  const React = (await import("react")).default;
  const { render, waitFor } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { ProjectTeamSetupOpenButton } = await import(
    "../ui/ProjectTeamSetupOpenButton.tsx"
  );
  const { PROJECT_TEAM_SETUP_MOUNT_COMMANDS, useProjectTeamSetupSummaryQuery } =
    await import("./useProjectTeamSetupSummary.ts");
  const forbidden = [
    "project_team_setup_get_publication",
    "project_team_setup_get_publication_options",
    "project_team_setup_get_launch",
    "project_team_setup_get_activation",
  ];
  for (const command of forbidden)
    answers[command] = () => {
      throw new Error(`${command} must not run on mount`);
    };
  answers.project_team_setup_get = {
    ...draftA,
    latestSnapshotId: "d".repeat(64),
  };
  answers.project_team_setup_peek_publication = { status: "adopted" };
  answers.project_team_list_installed_roles = [
    {
      projectRef: projectA,
      setupId: draftA.setupId,
      publicationId: "publication-1",
      teamId: "team",
      source: null,
      leadChannelId: null,
      roles: [
        {
          role: "lead",
          agentPubkey: "9".repeat(64),
          packRef: { repo: "r", sha: "s", role: "lead", path: "p" },
        },
      ],
    },
  ];
  function Mounted() {
    const summary = useProjectTeamSetupSummaryQuery(
      projectA,
      "wss://example.test",
    );
    return React.createElement(ProjectTeamSetupOpenButton, {
      summary: summary.data,
      failed: summary.isError,
      unavailable: null,
      onOpen: () => {},
    });
  }
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
    },
  });
  const mount = () =>
    render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(Mounted),
      ),
    );
  try {
    const adopted = mount();
    await waitFor(() =>
      assert.equal(
        adopted.getByTestId("project-team-setup-status").textContent,
        "A project roles draft is saved on this computer. Last recorded: published and adopted, roles installed on this computer.",
      ),
    );
    assert.ok(
      adopted.getByRole("button", { name: "Continue project role setup" }),
    );
    assert.deepEqual(
      calls.filter(
        ({ command }) => !PROJECT_TEAM_SETUP_MOUNT_COMMANDS.includes(command),
      ),
      [],
      "the mount path invoked a command outside the read-only set",
    );
    assert.deepEqual(
      calls.find(
        ({ command }) => command === "project_team_setup_peek_publication",
      )?.args,
      {
        projectRef: projectA,
        expectedRelayUrl: "wss://example.test",
        setupId: draftA.setupId,
      },
    );
    adopted.unmount();
    client.clear();

    answers.project_team_setup_peek_publication = null;
    const saved = mount();
    await waitFor(() =>
      assert.equal(
        saved.getByTestId("project-team-setup-status").textContent,
        "A project roles draft is saved on this computer. Last recorded: a checked version is saved, not yet published.",
      ),
    );
    saved.unmount();
    client.clear();

    answers.project_team_setup_get = null;
    const none = mount();
    await waitFor(() =>
      assert.ok(none.getByRole("button", { name: "Set up project roles" })),
    );
    assert.equal(none.queryByTestId("project-team-setup-status"), null);
    none.unmount();
    client.clear();

    answers.project_team_setup_get = () => {
      throw { code: "filesystem", message: "unreadable" };
    };
    const failed = mount();
    await waitFor(() =>
      assert.match(
        failed.getByTestId("project-team-setup-status").textContent,
        /Couldn't check for a saved draft/,
      ),
    );
    assert.ok(failed.getByRole("button", { name: "Set up project roles" }));
    assert.equal(
      calls.some(({ command }) => forbidden.includes(command)),
      false,
    );
  } finally {
    client.clear();
  }
});

test("a saved version that can't be re-checked gets its own stage instead of falling back", async () => {
  answers.project_team_setup_get = {
    ...draftA,
    latestSnapshotId: "d".repeat(64),
  };
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_snapshot = () => {
    throw { code: "invalid_snapshot", message: "Manifest hash mismatch." };
  };
  const { render, waitFor, element } = await harness();
  const view = render(element(projectA));
  await waitFor(() =>
    assert.equal(
      view.getByTestId("project-team-setup-stage-heading").textContent,
      "The saved version couldn't be re-checked",
    ),
  );
  assert.match(
    view.getByRole("alert").textContent,
    /no longer matches its files/,
  );
  assert.ok(view.getByRole("button", { name: "Check draft" }));
  assert.equal(
    calls.some(
      ({ command }) => command === "project_team_setup_get_publication_options",
    ),
    false,
  );
});
