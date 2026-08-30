import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
let calls = [];
let answers = {};
let clients = [];

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
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      const answer = answers[command];
      if (Array.isArray(answer)) return answer.shift();
      if (typeof answer === "function") return answer(args);
      return answer ?? null;
    },
  };
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  for (const client of clients) client.clear();
  clients = [];
  calls = [];
  answers = {};
  localStorage.clear();
});

after(() => dom.window.close());

const projectRef = `30621:${"b".repeat(64)}:hive`;
function readiness(overrides = {}) {
  return {
    schemaVersion: 2,
    projectRef,
    generatedAt: "2026-08-30T16:00:00Z",
    readyForFirstSession: false,
    ready: false,
    status: "unknown",
    hostClass: "partial",
    keyStoreSafe: true,
    source: {
      appCommit: "f2d61ce1",
      appSourceDirty: false,
      checkoutPath: "/repo",
      checkoutCommit: "abcd1234",
      checkoutDirty: true,
    },
    team: {
      selectedRoles: ["lead"],
      availableRoles: ["lead"],
      packs: [],
      identities: [],
    },
    runtimes: [],
    registry: {
      providerTargets: [],
      coveredTargets: [],
      uncoveredTargets: [],
      pendingTargets: [],
    },
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: false,
      process: "not_running",
    },
    policy: { hiringPolicy: "enabled" },
    relay: { state: "awaiting_first_session", source: "wire" },
    catalog: {
      state: "awaiting_first_session",
      targets: [],
      source: "wire",
    },
    facts: [
      {
        category: "identity",
        code: "SELECTED_ROLE_KEY_UNVERIFIED",
        scope: "local",
        state: "unknown",
        summary: "The lead identity key could not be verified.",
        remedy: "Prepare this project again after unlocking signing keys.",
      },
    ],
    blockingCodes: [],
    unknownCodes: ["SELECTED_ROLE_KEY_UNVERIFIED"],
    awaitingCodes: [],
    limitedCodes: [],
    ...overrides,
  };
}

async function mountHarness(initial = {}) {
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { render } = await import("@testing-library/react");
  const { useProjectTeamReadiness } = await import(
    "../lib/useProjectTeamReadiness.ts"
  );
  const { teamReadinessLaunchGate } = await import(
    "../lib/teamReadinessModel.ts"
  );
  const { TeamReadinessCard } = await import("./TeamReadinessCard.tsx");
  function Harness({ checkoutPath, roles }) {
    const state = useProjectTeamReadiness({
      projectRef,
      checkoutPath,
      selectedRoles: roles,
    });
    const [preflightResult, setPreflightResult] = React.useState("");
    const preflight = async () => {
      try {
        const fresh = await state.readFreshForLaunch();
        const gate = teamReadinessLaunchGate({
          projectRef,
          loading: false,
          error: null,
          readiness: fresh,
        });
        setPreflightResult(
          gate.allowed ? "allowed" : (gate.reason ?? "blocked"),
        );
      } catch (error) {
        setPreflightResult(
          error instanceof Error ? error.message : String(error),
        );
      }
    };
    return React.createElement(
      React.Fragment,
      null,
      React.createElement(TeamReadinessCard, {
        readiness: state.readiness,
        loading: state.isLoading,
        readError: state.readError,
        selectedRoles: roles,
        scan: state.scan,
        names: state.names,
        onNameChange: state.setName,
        onBeginPrepare: () => void state.beginPrepare(),
        onConfirmPrepare: () => void state.confirmPrepare(),
        onCancelPrepare: state.cancelPrepare,
        scanning: state.isScanning,
        preparing: state.isPreparing,
        prepareSteps: state.prepareSteps,
        prepareError: state.prepareError,
        externalBusy: state.isLaunchPreflighting,
      }),
      React.createElement(
        "button",
        {
          "data-testid": "launch-preflight",
          disabled:
            state.isScanning || state.isPreparing || state.isLaunchPreflighting,
          onClick: () => void preflight(),
          type: "button",
        },
        "Launch preflight",
      ),
      React.createElement(
        "button",
        {
          "data-testid": "force-launch-preflight",
          onClick: () => void preflight(),
          type: "button",
        },
        "Force launch preflight",
      ),
      React.createElement(
        "output",
        { "data-testid": "launch-preflight-result" },
        preflightResult,
      ),
    );
  }
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  clients.push(client);
  const renderTree = (props) =>
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(Harness, props),
    );
  const props = {
    checkoutPath: initial.checkoutPath ?? "/repo",
    roles: initial.roles ?? ["lead"],
  };
  const view = render(renderTree(props));
  return {
    rerender(next) {
      view.rerender(renderTree({ ...props, ...next }));
    },
  };
}

test("card keeps Unknown distinct and shows exact source facts and remedy", async () => {
  answers.team_readiness = readiness();
  await mountHarness();
  const { screen } = await import("@testing-library/react");
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  assert.ok(screen.getByRole("region", { name: "Unknown" }));
  assert.match(
    screen.getByRole("region", { name: "Unknown" }).textContent,
    /Prepare this project again after unlocking signing keys/,
  );
  assert.match(
    screen.getByTestId("team-readiness-card").textContent,
    /abcd1234 · dirty: yes/,
  );
  assert.match(
    screen.getByTestId("team-readiness-card").textContent,
    /Full: not ready/,
  );
});

test("real Tauri wrappers run Prepare in order, preserve names, and always re-read", async () => {
  answers.team_readiness = [
    readiness(),
    readiness({
      readyForFirstSession: true,
      status: "awaiting_first_session",
      hostClass: "prepared_for_first_session",
      provider: {
        relayUrl: "wss://hive.example",
        provisioned: true,
        process: "live",
      },
      facts: [
        {
          category: "catalog",
          code: "CATALOG_AWAITING_FIRST_SESSION",
          scope: "wire",
          state: "awaiting_first_session",
          summary: "No signed session catalog exists yet.",
          remedy: "Launch the first session to observe signed provider facts.",
        },
      ],
      unknownCodes: [],
      awaitingCodes: ["CATALOG_AWAITING_FIRST_SESSION"],
    }),
  ];
  answers.scan_project_role_packs_directory = {
    directory: "/repo/personas/roles",
    exists: true,
    packs: [
      {
        role: "lead",
        personaName: "lead",
        packDir: "/repo/personas/roles/lead",
        defaultName: "Helios",
        installed: false,
      },
      {
        role: "verifier",
        personaName: "verifier",
        packDir: "/repo/personas/roles/verifier",
        defaultName: "Parallax",
        installed: true,
      },
    ],
    skipped: [],
  };
  answers.install_crew_role_packs = { installed: [], skipped: [] };
  answers.provision_coding_session_provider = {
    provisioned: true,
    running: true,
  };
  answers.ensure_coding_session_provider_running = {
    provisioned: true,
    running: true,
  };
  await mountHarness();
  const { fireEvent, screen, waitFor } = await import("@testing-library/react");
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  fireEvent.click(screen.getByTestId("team-readiness-prepare"));
  const name = await screen.findByLabelText("lead agent name");
  const verifierName = screen.getByLabelText("verifier agent name");
  assert.equal(name.value, "Helios");
  assert.equal(verifierName.value, "Parallax");
  fireEvent.change(name, { target: { value: "Aurora" } });
  fireEvent.change(verifierName, { target: { value: "Prism" } });
  fireEvent.click(screen.getByTestId("team-readiness-prepare-confirm"));
  await screen.findByText(/Prepared for the first session/);
  await waitFor(() => {
    assert.equal(
      screen.getByTestId("team-readiness-prepare-steps").querySelectorAll("li")
        .length,
      4,
    );
  });
  assert.deepEqual(
    calls.map(({ command }) => command),
    [
      "team_readiness",
      "scan_project_role_packs_directory",
      "install_crew_role_packs",
      "provision_coding_session_provider",
      "ensure_coding_session_provider_running",
      "team_readiness",
    ],
  );
  assert.deepEqual(calls[0].args, {
    projectRef,
    selectedRoles: ["lead"],
    hiringPolicyEnabled: true,
  });
  assert.deepEqual(calls[2].args, {
    directory: "/repo/personas/roles",
    names: { lead: "Aurora", verifier: "Prism" },
  });
  assert.match(
    screen.getByTestId("team-readiness-card").textContent,
    /verifier· also refreshed/,
  );
  assert.match(
    screen.getByTestId("team-readiness-card").textContent,
    /Full readiness awaits signed session facts/,
  );
});

test("partial native failure keeps completed UI steps and re-reads readiness", async () => {
  answers.team_readiness = [readiness(), readiness()];
  answers.scan_project_role_packs_directory = {
    directory: "/repo/personas/roles",
    exists: true,
    packs: [
      {
        role: "lead",
        personaName: "lead",
        packDir: "/repo/personas/roles/lead",
        defaultName: "Helios",
        installed: false,
      },
    ],
    skipped: [],
  };
  answers.install_crew_role_packs = { installed: [], skipped: [] };
  answers.provision_coding_session_provider = () => {
    throw new Error("password prompt was cancelled");
  };
  await mountHarness();
  const { fireEvent, screen } = await import("@testing-library/react");
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  fireEvent.click(screen.getByTestId("team-readiness-prepare"));
  await screen.findByLabelText("lead agent name");
  fireEvent.click(screen.getByTestId("team-readiness-prepare-confirm"));
  await screen.findByText("password prompt was cancelled");
  assert.deepEqual(
    calls.map(({ command }) => command),
    [
      "team_readiness",
      "scan_project_role_packs_directory",
      "install_crew_role_packs",
      "provision_coding_session_provider",
      "team_readiness",
    ],
  );
  const steps = screen.getByTestId("team-readiness-prepare-steps").textContent;
  assert.match(steps, /Install or refresh discovered role packs: done/);
  assert.match(steps, /Provision the provider identity: failed/);
  assert.match(steps, /Start the provider: pending/);
  assert.match(steps, /Re-read Team Readiness: done/);
});

test("an empty scan cannot start Prepare and explains why", async () => {
  answers.team_readiness = readiness();
  answers.scan_project_role_packs_directory = {
    directory: "/repo/personas/roles",
    exists: true,
    packs: [],
    skipped: [],
  };
  await mountHarness();
  const { fireEvent, screen } = await import("@testing-library/react");
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  fireEvent.click(screen.getByTestId("team-readiness-prepare"));
  await screen.findByText(/No role packs were discovered/);
  assert.equal(
    screen.getByTestId("team-readiness-prepare-confirm").disabled,
    true,
  );
  assert.deepEqual(
    calls.map(({ command }) => command),
    ["team_readiness", "scan_project_role_packs_directory"],
  );
});

test("launch authority is a click-time read, so provider death defeats a ready cache", async () => {
  const initiallyReady = readiness({
    readyForFirstSession: true,
    status: "awaiting_first_session",
    hostClass: "prepared_for_first_session",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    facts: [
      {
        category: "catalog",
        code: "CATALOG_AWAITING_FIRST_SESSION",
        scope: "wire",
        state: "awaiting_first_session",
        summary: "No signed session catalog exists yet.",
        remedy: "Launch the first session.",
      },
    ],
    unknownCodes: [],
    awaitingCodes: ["CATALOG_AWAITING_FIRST_SESSION"],
  });
  const providerDied = readiness({
    status: "blocked",
    blockingCodes: ["PROVIDER_NOT_LIVE"],
    unknownCodes: [],
    facts: [
      {
        category: "provider",
        code: "PROVIDER_NOT_LIVE",
        scope: "local",
        state: "blocked",
        summary: "No provider process is live.",
        remedy: "Start the provider, then re-read readiness.",
      },
    ],
  });
  answers.team_readiness = [initiallyReady, providerDied];
  await mountHarness();
  const { fireEvent, screen, waitFor } = await import("@testing-library/react");
  await screen.findByText(/Prepared for the first session/);
  fireEvent.click(screen.getByTestId("launch-preflight"));
  await waitFor(() =>
    assert.match(
      screen.getByTestId("launch-preflight-result").textContent,
      /No provider process is live/,
    ),
  );
  assert.deepEqual(
    calls.map(({ command }) => command),
    ["team_readiness", "team_readiness"],
  );
  assert.match(
    screen.getByTestId("team-readiness-card").textContent,
    /Not prepared for the first team session/,
  );
});

test("Prepare excludes concurrent launch preflight and keeps all controls locked", async () => {
  let releaseInstall;
  answers.team_readiness = [readiness(), readiness()];
  answers.scan_project_role_packs_directory = {
    directory: "/repo/personas/roles",
    exists: true,
    packs: [
      {
        role: "lead",
        personaName: "lead",
        packDir: "/repo/personas/roles/lead",
        defaultName: "Helios",
        installed: false,
      },
    ],
    skipped: [],
  };
  answers.install_crew_role_packs = () =>
    new Promise((resolve) => {
      releaseInstall = resolve;
    });
  answers.provision_coding_session_provider = {
    provisioned: true,
    running: true,
  };
  answers.ensure_coding_session_provider_running = {
    provisioned: true,
    running: true,
  };
  await mountHarness();
  const { act, fireEvent, screen, waitFor } = await import(
    "@testing-library/react"
  );
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  fireEvent.click(screen.getByTestId("team-readiness-prepare"));
  await screen.findByLabelText("lead agent name");
  fireEvent.click(screen.getByTestId("team-readiness-prepare-confirm"));
  await waitFor(() => assert.equal(typeof releaseInstall, "function"));
  assert.equal(screen.getByLabelText("lead agent name").disabled, true);
  assert.equal(
    screen.getByTestId("team-readiness-prepare-confirm").disabled,
    true,
  );
  assert.equal(screen.getByTestId("launch-preflight").disabled, true);
  fireEvent.click(screen.getByTestId("force-launch-preflight"));
  await screen.findByText(/Project preparation is still running/);
  assert.equal(
    calls.filter(({ command }) => command === "team_readiness").length,
    1,
  );
  await act(async () => releaseInstall({ installed: [], skipped: [] }));
  await screen.findByText(/Re-read Team Readiness: done/);
});

test("changing checkout or role scope clears scan names and all later reads use it", async () => {
  answers.team_readiness = [readiness(), readiness(), readiness()];
  answers.scan_project_role_packs_directory = {
    directory: "/repo/personas/roles",
    exists: true,
    packs: [
      {
        role: "lead",
        personaName: "lead",
        packDir: "/repo/personas/roles/lead",
        defaultName: "Helios",
        installed: false,
      },
    ],
    skipped: [],
  };
  const view = await mountHarness();
  const { fireEvent, screen, waitFor } = await import("@testing-library/react");
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  fireEvent.click(screen.getByTestId("team-readiness-prepare"));
  await screen.findByLabelText("lead agent name");
  view.rerender({
    checkoutPath: "/repo-two",
    roles: [" Builder ", "LEAD", "lead"],
  });
  assert.equal(screen.queryByLabelText("lead agent name"), null);
  await waitFor(() => {
    assert.equal(
      calls.filter(({ command }) => command === "team_readiness").length,
      2,
    );
  });
  assert.deepEqual(
    calls.filter(({ command }) => command === "team_readiness").at(-1).args,
    {
      projectRef,
      selectedRoles: ["builder", "lead"],
      hiringPolicyEnabled: true,
    },
  );
  fireEvent.click(screen.getByTestId("launch-preflight"));
  await waitFor(() => {
    assert.equal(
      calls.filter(({ command }) => command === "team_readiness").length,
      3,
    );
  });
  assert.deepEqual(
    calls.filter(({ command }) => command === "team_readiness").at(-1).args,
    {
      projectRef,
      selectedRoles: ["builder", "lead"],
      hiringPolicyEnabled: true,
    },
  );
});
