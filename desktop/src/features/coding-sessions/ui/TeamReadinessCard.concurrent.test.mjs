import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const calls = [];
const answers = {};
const clients = [];
const projectRef = `30621:${"b".repeat(64)}:hive`;
const channelId = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

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
      if (answer !== undefined) return answer;
      if (command === "get_relay_http_url") return "https://hive.example";
      if (command === "get_media_proxy_port") return 3001;
      if (command === "restart_managed_agent") return { pubkey: args.pubkey };
      return null;
    },
  };
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  for (const client of clients.splice(0)) client.clear();
  calls.length = 0;
  for (const key of Object.keys(answers)) delete answers[key];
  localStorage.clear();
});

after(() => dom.window.close());

function readiness() {
  return {
    schemaVersion: 3,
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
      provenance: [],
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
  };
}

const roleScan = {
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

async function mountHarness(initial = {}) {
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "community-a",
        name: "Hive",
        relayUrl: "wss://hive.example",
        addedAt: "2026-08-30T00:00:00Z",
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "community-a");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { render } = await import("@testing-library/react");
  const { CommunitiesProvider } = await import(
    "@/features/communities/useCommunities"
  );
  const { useProjectTeamReadiness } = await import(
    "../lib/useProjectTeamReadiness.ts"
  );
  const { TeamReadinessCard } = await import("./TeamReadinessCard.tsx");
  function Harness({ checkoutPath, roles }) {
    const state = useProjectTeamReadiness({
      projectRef,
      checkoutPath,
      selectedRoles: roles,
      channelIds: [channelId],
      refreshRuntimeTargets: async () => {},
    });
    const [preflightResult, setPreflightResult] = React.useState("");
    const preflight = async () => {
      try {
        await state.readFreshForLaunch();
        setPreflightResult("allowed");
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
        prepareWarning: state.prepareWarning,
        externalBusy: state.isLaunchPreflighting,
        runtimeTarget: null,
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
    defaultOptions: { queries: { gcTime: Infinity, retry: false } },
  });
  clients.push(client);
  const renderTree = (props) =>
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(
        CommunitiesProvider,
        null,
        React.createElement(Harness, props),
      ),
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
    unmount: view.unmount,
  };
}

test("unmounting during Prepare prevents every later mutation", async () => {
  let releaseInstall;
  answers.team_readiness = () => readiness();
  answers.scan_project_role_packs_directory = roleScan;
  answers.install_crew_role_packs = () =>
    new Promise((resolve) => {
      releaseInstall = resolve;
    });
  const view = await mountHarness();
  const { act, fireEvent, screen, waitFor } = await import(
    "@testing-library/react"
  );
  await screen.findByText("SELECTED_ROLE_KEY_UNVERIFIED");
  fireEvent.click(screen.getByTestId("team-readiness-prepare"));
  await screen.findByLabelText("lead agent name");
  fireEvent.click(screen.getByTestId("team-readiness-prepare-confirm"));
  await waitFor(() => assert.equal(typeof releaseInstall, "function"));
  const readsBeforeUnmount = calls.filter(
    ({ command }) => command === "team_readiness",
  ).length;
  view.unmount();
  await act(async () => releaseInstall({ installed: [], skipped: [] }));
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(
    calls.some(
      ({ command }) => command === "provision_coding_session_provider",
    ),
    false,
  );
  assert.equal(
    calls.filter(({ command }) => command === "team_readiness").length,
    readsBeforeUnmount,
  );
});

test("Prepare excludes concurrent launch preflight and keeps controls locked", async () => {
  let releaseInstall;
  answers.team_readiness = [readiness(), readiness()];
  answers.scan_project_role_packs_directory = roleScan;
  answers.install_crew_role_packs = () =>
    new Promise((resolve) => {
      releaseInstall = resolve;
    });
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

test("changing checkout or role scope clears names and scopes later reads", async () => {
  answers.team_readiness = [readiness(), readiness(), readiness()];
  answers.scan_project_role_packs_directory = roleScan;
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
  await waitFor(() =>
    assert.equal(
      calls.filter(({ command }) => command === "team_readiness").length,
      2,
    ),
  );
  const expected = {
    projectRef,
    selectedRoles: ["builder", "lead"],
    hiringPolicyEnabled: true,
    expectedRelayUrl: "wss://hive.example",
    channelIds: [channelId],
  };
  assert.deepEqual(
    calls.filter(({ command }) => command === "team_readiness").at(-1).args,
    expected,
  );
  fireEvent.click(screen.getByTestId("launch-preflight"));
  await waitFor(() =>
    assert.equal(
      calls.filter(({ command }) => command === "team_readiness").length,
      3,
    ),
  );
  assert.deepEqual(
    calls.filter(({ command }) => command === "team_readiness").at(-1).args,
    expected,
  );
});
