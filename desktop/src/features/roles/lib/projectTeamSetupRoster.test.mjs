import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";

// The end of project setup: the roster of installed agents, whether the lead
// can hire each of them on this computer, and the one next action.
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
const otherProject = `30621:${"b".repeat(64)}:beekeeper`;
const LOOM = "9".repeat(64);
const BOB = "8".repeat(64);
const GORDAN = "7".repeat(64);
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
  roles: ["lead", "builder", "runner"],
  createdAt: "2026-09-14",
  latestSnapshotId: "d".repeat(64),
};
const packRef = (role) => ({ repo: "r", sha: "s", role, path: `p/${role}` });
const INSTALLED = [
  { role: "lead", agentPubkey: LOOM, packRef: packRef("lead") },
  { role: "builder", agentPubkey: BOB, packRef: packRef("builder") },
  { role: "runner", agentPubkey: GORDAN, packRef: packRef("runner") },
];
const record = (pubkey, name, homeRole, projectRef) => ({
  pubkey,
  name,
  homeRole,
  projectRef,
  status: "stopped",
});

const NO_ACTIONS = {
  busy: false,
  creatingChannel: false,
  onRetryInstall: () => {},
  onStartLead: () => {},
  onRetryChannel: () => {},
};
function installedActivation(installedRoles, lead) {
  return {
    publicationId: "publication-1",
    source: { repoRef: "r", commit: "1".repeat(40), packPath: "p" },
    installation: { status: "installed", installedRoles, message: null },
    lead: { message: null, ...lead },
  };
}

test("the roster row shows the agent's current name, role and association; rename sends only pubkey and name", async () => {
  const pubkey = "ab".repeat(32);
  const rawAgent = (name) => ({
    pubkey,
    name,
    persona_id: null,
    home_role: "lead",
    relay_url: "wss://example.test",
    status: "stopped",
    project_ref: projectA,
  });
  let currentName = "Loom";
  answers.list_managed_agents = () => [rawAgent(currentName)];
  answers.update_managed_agent = ({ input }) => {
    currentName = input.name;
    return { agent: rawAgent(input.name), profile_sync_error: null };
  };
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { useProjectTeamSetupAgentDirectory } = await import(
    "./useProjectTeamSetupAgentDirectory.ts"
  );
  const { ProjectTeamSetupAgentsContext } = await import(
    "../ui/ProjectTeamSetupAgents.tsx"
  );
  const { ProjectTeamSetupRoster } = await import(
    "../ui/ProjectTeamSetupRoster.tsx"
  );
  const { render, waitFor, fireEvent } = await import("@testing-library/react");
  function Connected() {
    const agents = useProjectTeamSetupAgentDirectory(true);
    return React.createElement(
      ProjectTeamSetupAgentsContext.Provider,
      { value: agents },
      React.createElement(ProjectTeamSetupRoster, {
        projectRef: projectA,
        projectName: "Tank Loop",
        activation: installedActivation(
          [
            {
              role: "lead",
              agentPubkey: pubkey,
              packRef: { repo: "r", sha: "s", role: "lead", path: "p" },
            },
          ],
          {
            status: "ready",
            channelId: "c",
            sessionRef: null,
            leadPubkey: pubkey,
          },
        ),
        actions: NO_ACTIONS,
      }),
    );
  }
  // Infinite gcTime schedules no five-minute garbage-collection timers that
  // would otherwise hold this file's process open after its last test.
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
      mutations: { gcTime: Number.POSITIVE_INFINITY },
    },
  });
  try {
    const view = render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(Connected),
      ),
    );
    const row = () => view.getByTestId("project-team-setup-installed-role");
    await waitFor(() => assert.match(row().textContent, /Loom/));
    assert.match(row().textContent, /· lead/);
    await waitFor(() => assert.match(row().textContent, /· Project agent/));
    assert.match(
      view.getByTestId("project-team-setup-roster").querySelector("details")
        .textContent,
      /abababab…abab/,
    );
    fireEvent.click(view.getByRole("button", { name: "Rename" }));
    const input = view.getByLabelText("New name for the lead agent");
    fireEvent.change(input, { target: { value: "   " } });
    assert.equal(
      view.getByRole("button", { name: "Save name" }).disabled,
      true,
    );
    fireEvent.change(input, { target: { value: "  Tank Lead  " } });
    fireEvent.click(view.getByRole("button", { name: "Save name" }));
    await waitFor(() => assert.match(row().textContent, /Tank Lead/));
    assert.deepEqual(
      calls
        .filter(({ command }) => command === "update_managed_agent")
        .map(({ args }) => args),
      [{ input: { pubkey, name: "Tank Lead" } }],
    );
    answers.update_managed_agent = () => {
      throw "Agent is busy";
    };
    fireEvent.click(view.getByRole("button", { name: "Rename" }));
    fireEvent.change(view.getByLabelText("New name for the lead agent"), {
      target: { value: "Other" },
    });
    fireEvent.click(view.getByRole("button", { name: "Save name" }));
    await waitFor(() =>
      assert.match(
        view.getByRole("alert").textContent,
        /The name couldn't be changed\. Agent is busy/,
      ),
    );
    assert.match(
      row().textContent,
      /Tank Lead/,
      "a failed rename keeps the name",
    );
  } finally {
    client.clear();
  }
});

/** Mount setup already adopted and installed, with the given agent records. */
async function mountInstalled({ lead, agents, directory = {} }) {
  const activation = installedActivation(INSTALLED, {
    leadPubkey: LOOM,
    ...lead,
  });
  answers.project_team_setup_get = draftA;
  answers.get_coding_session_workdir_state = { byProject: {} };
  answers.project_team_setup_snapshot = {
    setupId: draftA.setupId,
    snapshotId: draftA.latestSnapshotId,
    rolesDirectory: "/saved/roles",
    manifestPath: "/saved/manifest.json",
    roles: draftA.roles,
  };
  answers.project_team_setup_get_publication_options = {
    currentSourceEventId: null,
    suggestedDestination: null,
    sourceExpectation: { kind: "if_unset" },
    publication: {
      publicationId: "publication-1",
      setupId: draftA.setupId,
      status: "adopted",
      snapshotId: draftA.latestSnapshotId,
      destination: {
        repoRef: "r",
        packPath: "p",
        baseCommit: null,
        createAnnouncement: null,
      },
      sourceExpectation: { kind: "if_unset" },
      candidateRef: "refs/heads/setup/publication-1",
      candidateCommit: "1".repeat(40),
      sourceEventId: "2".repeat(64),
      message: null,
    },
  };
  answers.project_team_setup_get_activation = activation;
  answers.project_team_setup_install_adopted_roles = activation;
  const React = (await import("react")).default;
  const { ProjectTeamSetupForm } = await import(
    "../ui/ProjectTeamSetupForm.tsx"
  );
  const { ProjectTeamSetupAgentsContext } = await import(
    "../ui/ProjectTeamSetupAgents.tsx"
  );
  const testing = await import("@testing-library/react");
  const names = new Map(
    (Array.isArray(agents) ? agents : []).map((agent) => [
      agent.pubkey,
      agent.name,
    ]),
  );
  const view = testing.render(
    React.createElement(
      ProjectTeamSetupAgentsContext.Provider,
      { value: { names, rename: null, agents, ...directory } },
      React.createElement(ProjectTeamSetupForm, {
        projectRef: projectA,
        projectName: "Tank Loop",
        relayUrl: draftA.relayUrl,
      }),
    ),
  );
  await testing.waitFor(() =>
    assert.ok(view.getByTestId("project-team-setup-roster")),
  );
  return { ...testing, view };
}

const heading = (view) =>
  view.getByTestId("project-team-setup-stage-heading").textContent;
const currentStep = (view) =>
  view
    .getByTestId("project-team-setup-stepper")
    .querySelector('[aria-current="step"]');

test("unassociated agents block starting the lead: the roster names them, offers one safe retry, and never reads ready", async () => {
  let refreshed = 0;
  let openedAgents = 0;
  const { view, waitFor, fireEvent } = await mountInstalled({
    lead: { status: "ready", channelId: "chan", sessionRef: null },
    agents: [
      record(LOOM, "Loom", "lead", projectA),
      record(BOB, "Bob", "builder", null),
      record(GORDAN, "Gordan", "runner", null),
    ],
    directory: {
      refreshAgents: () => {
        refreshed += 1;
      },
      openAgentsTab: () => {
        openedAgents += 1;
      },
    },
  });
  await waitFor(() =>
    assert.equal(heading(view), "Project agents aren't associated"),
  );
  const expected =
    "The lead can't hire Bob and Gordan yet: they aren't associated with this project on this computer. Retry installation (safe, keeps identities) or associate them on the project's Agents tab.";
  assert.equal(
    view.getByTestId("project-team-setup-next-action").textContent,
    expected,
  );
  const roster = view.getByTestId("project-team-setup-roster");
  assert.equal(roster.querySelector("h4").textContent, "Tank Loop agents");
  assert.equal(
    view.getByTestId("project-team-setup-roster-next").textContent,
    expected,
  );
  assert.deepEqual(
    view
      .getAllByTestId("project-team-setup-installed-role")
      .map((row) => row.dataset.association),
    ["associated", "not-associated", "not-associated"],
  );
  assert.equal(currentStep(view).dataset.state, "blocked");
  assert.equal(
    view.queryByRole("button", { name: "Start the project lead" }),
    null,
  );
  assert.doesNotMatch(roster.textContent, /complete/i);
  fireEvent.click(
    view.getByRole("button", { name: "Open the project's Agents tab" }),
  );
  assert.equal(openedAgents, 1);
  fireEvent.click(view.getByRole("button", { name: "Retry installation" }));
  await waitFor(() => assert.equal(refreshed, 1));
  assert.deepEqual(
    calls.find(
      ({ command }) => command === "project_team_setup_install_adopted_roles",
    )?.args,
    {
      projectRef: projectA,
      expectedRelayUrl: draftA.relayUrl,
      setupId: draftA.setupId,
      publicationId: "publication-1",
    },
  );
  assert.equal(
    calls.some(({ command }) =>
      [
        "project_team_setup_start_lead",
        "project_team_setup_ensure_lead_channel",
      ].includes(command),
    ),
    false,
  );
});

test("a started lead with another project's agent and a missing record is never complete", async () => {
  const { view, waitFor } = await mountInstalled({
    lead: { status: "started", channelId: "chan", sessionRef: "lead-1" },
    agents: [
      record(LOOM, "Loom", "lead", projectA),
      record(BOB, "Bob", "builder", otherProject),
    ],
    directory: { openLeadSession: () => {} },
  });
  await waitFor(() =>
    assert.equal(heading(view), "The lead can't hire every project agent"),
  );
  const next = view.getByTestId("project-team-setup-roster-next").textContent;
  assert.equal(
    next,
    "Lead started, but it can't hire Bob and the runner agent: they aren't associated with this project on this computer. Bob belongs to another project and isn't borrowed. The runner agent has no agent record on this computer. Reinstalling can't move another project's agent here; review this project's agents on its Agents tab.",
  );
  // No futile remedy: reinstalling cannot move another project's agent here.
  assert.equal(
    view.queryByRole("button", { name: "Retry installation" }),
    null,
  );
  assert.notEqual(currentStep(view).dataset.state, "done");
  assert.doesNotMatch(
    view.getByTestId("project-team-setup-draft").textContent,
    /Setup complete/,
  );
  assert.equal(view.queryByRole("button", { name: "Open session" }), null);
  const rows = view.getAllByTestId("project-team-setup-installed-role");
  assert.match(
    rows[1].textContent,
    /Bob\s*· builder\s*· Belongs to another project/,
  );
  assert.match(
    rows[2].textContent,
    /Name not available\s*· runner\s*· No agent record on this computer/,
  );
  assert.equal(
    rows[2].querySelector("button"),
    null,
    "no rename without a local record",
  );
});

test("an unread agent list is checking, and a failed read offers Check again, never a start", async () => {
  const checking = await mountInstalled({
    lead: { status: "ready", channelId: "chan", sessionRef: null },
    agents: null,
  });
  await checking.waitFor(() =>
    assert.equal(heading(checking.view), "Checking project agents"),
  );
  assert.equal(
    checking.view.queryByRole("button", { name: "Start the project lead" }),
    null,
  );
  checking.cleanup();
  let refreshed = 0;
  const failed = await mountInstalled({
    lead: { status: "ready", channelId: "chan", sessionRef: null },
    agents: "unreadable",
    directory: {
      refreshAgents: () => {
        refreshed += 1;
      },
    },
  });
  await failed.waitFor(() =>
    assert.equal(heading(failed.view), "Project agents not confirmed"),
  );
  failed.fireEvent.click(
    failed.view.getByRole("button", { name: "Check again" }),
  );
  assert.equal(refreshed, 1);
});

test("with every agent associated and the lead started, setup is complete with one Open session action", async () => {
  const opened = [];
  const { view, waitFor, fireEvent } = await mountInstalled({
    lead: { status: "started", channelId: "chan", sessionRef: "lead-1" },
    agents: [
      record(LOOM, "Loom", "lead", projectA),
      record(BOB, "Bob", "builder", projectA),
      record(GORDAN, "Gordan", "runner", projectA),
    ],
    directory: { openLeadSession: (lead) => opened.push(lead) },
  });
  await waitFor(() => assert.equal(heading(view), "Project lead started"));
  assert.equal(currentStep(view).dataset.state, "done");
  const next = view.getByTestId("project-team-setup-roster-next");
  assert.equal(
    next.textContent,
    "Setup complete. Loom leads Tank Loop. Give Loom a task in its session; it lists the team with bee projects agents and hires only these agents.",
  );
  assert.equal(next.querySelector("code").textContent, "bee projects agents");
  const roster = view.getByTestId("project-team-setup-roster");
  assert.deepEqual(
    [...roster.querySelectorAll("button")]
      .map((button) => button.textContent)
      .filter((label) => label !== "Rename"),
    ["Open session"],
    "exactly one next action; no Agents tab link without a route",
  );
  for (const row of view.getAllByTestId("project-team-setup-installed-role"))
    assert.match(row.textContent, /· Project agent/);
  fireEvent.click(view.getByRole("button", { name: "Open session" }));
  assert.deepEqual(opened, [{ channelId: "chan", sessionRef: "lead-1" }]);
});
