import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { managedAgentHostStatusQueryKey } from "@/features/agents/hooks";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const actor = "ab".repeat(32);
let calls = [];
let client;
let info;
let failure = false;
let runtimes = [];
const status = () => ({
  relayUrl: "ws://127.0.0.1:3037",
  revision: null,
  revisionAvailable: false,
  commissioned: true,
  ownershipPhase: "adopted",
  legacyRecreationRequired: false,
  legacyConflict: null,
  runtimeAvailable: null,
});
before(() => {
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    localStorage: dom.window.localStorage,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "get_managed_agent_host_status") {
        if (failure) throw Error("provider observation unavailable");
        return info;
      }
      if (command === "list_managed_agent_runtimes") return runtimes;
      if (command === "set_managed_agent_start_on_app_launch") {
        info = {
          ...info,
          ownershipPhase: args.startOnAppLaunch ? "adopted" : "hostStopped",
        };
        return {
          pubkey: actor,
          name: "Scoped",
          backend: { type: "local" },
          start_on_app_launch: args.startOnAppLaunch,
        };
      }
      if (command === "start_managed_agent")
        throw Error("Create receipt pending");
      throw Error(`Unexpected effect ${command}`);
    },
  };
});
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  client?.clear();
  calls = [];
  failure = false;
  runtimes = [];
});
after(() => dom.window.close());
async function mount(callback) {
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "isolated",
        relayUrl: "ws://localhost:3037",
        addedAt: new Date(0).toISOString(),
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "isolated");
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { render } = await import("@testing-library/react");
  const { HostManagementPanel } = await import("./HostManagementPanel.tsx");
  return render(
    React.createElement(HostManagementPanel, {
      pubkey: actor,
      onStatusChange: callback,
    }),
    {
      wrapper: ({ children }) =>
        React.createElement(QueryClientProvider, { client }, children),
    },
  );
}
test("legacy notice retains history and dispatches no migration or lifecycle effects", async () => {
  info = {
    ...status(),
    ownershipPhase: null,
    legacyRecreationRequired: true,
    legacyConflict: "Previous executor is still running",
  };
  await mount();
  const { screen, waitFor } = await import("@testing-library/react");
  await waitFor(() =>
    assert.match(
      screen.getByTestId("agent-clean-start-notice").textContent,
      /Recreate.*headless/,
    ),
  );
  assert.match(
    screen.getByTestId("agent-clean-start-notice").textContent,
    /history, settings and credentials remain retained/,
  );
  assert.match(screen.getByRole("alert").textContent, /still running/);
  assert.deepEqual(
    [...new Set(calls.map((row) => row.command))],
    ["get_managed_agent_host_status"],
  );
  assert.equal(
    screen.queryByText(/Continue migration|Retry migration|Restore legacy/i),
    null,
  );
});
test("modern adopted status keeps ownership distinct from runtime availability", async () => {
  info = status();
  const observed = [];
  await mount((value) => observed.push(value));
  const { screen, waitFor } = await import("@testing-library/react");
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-management").dataset.observedPhase,
      "adopted",
    ),
  );
  assert.match(
    screen.getByTestId("agent-host-status").textContent,
    /independently of this window/,
  );
  assert.equal(
    screen.queryByText("Runtime availability is unconfirmed.") === null,
    false,
  );
  assert.equal(observed.at(-1).ownershipPhase, "adopted");
  assert.equal(observed.at(-1).runtimeAvailable, null);
});
test("an adopted pair with a host-managed runtime row shows the presence pill and its evidence", async () => {
  info = status();
  runtimes = [
    {
      pubkey: actor,
      relayUrl: "ws://127.0.0.1:3037",
      localSetup: false,
      lifecycle: "host_managed",
      runtimeAvailable: false,
      providerPresence: "live",
      execution: "idle",
      pid: null,
      error: null,
      logPath: null,
    },
  ];
  await mount();
  const { screen, waitFor } = await import("@testing-library/react");
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-availability").textContent,
      "Idle",
    ),
  );
  const pill = screen.getByTestId("agent-host-availability");
  assert.equal(pill.dataset.providerPresence, "live");
  assert.equal(pill.dataset.execution, "idle");
  assert.match(
    screen.getByTestId("agent-host-availability-detail").textContent,
    /Here and idle: the host's presence heartbeat is live/,
  );
  // The pill replaces the one-sentence fallback rather than sitting beside it.
  assert.equal(
    screen.queryByText("Runtime availability is unconfirmed."),
    null,
  );
  // Same panel, host gone quiet: the word and the colour change together.
  runtimes = [
    { ...runtimes[0], providerPresence: "expired", execution: "disconnected" },
  ];
  const { act } = await import("@testing-library/react");
  await act(async () => client.invalidateQueries());
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-availability").textContent,
      "Provider not heard from",
    ),
  );
  assert.match(
    screen.getByTestId("agent-host-availability-detail").textContent,
    /heartbeat expired/,
  );
});
test("shared status prefix invalidation updates a mounted host panel after other surface stop", async () => {
  info = status();
  await mount();
  const { screen, waitFor, act } = await import("@testing-library/react");
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-management").dataset.observedPhase,
      "adopted",
    ),
  );
  info = { ...info, ownershipPhase: "hostStopped" };
  await act(async () =>
    client.invalidateQueries({
      queryKey: managedAgentHostStatusQueryKey(actor),
    }),
  );
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-management").dataset.observedPhase,
      "hostStopped",
    ),
  );
  assert.match(
    screen.getByTestId("agent-host-status").textContent,
    /same identity/,
  );
});
test("foreign community or failed status cannot retain mutable ownership controls", async () => {
  info = { ...status(), relayUrl: "ws://foreign-relay" };
  const values = [];
  await mount((value) => values.push(value));
  const { waitFor, screen, act } = await import("@testing-library/react");
  await waitFor(() => assert.ok(calls.length > 0));
  assert.equal(
    screen.getByTestId("agent-host-management").dataset.observedPhase,
    "unknown",
  );
  assert.equal(values.at(-1), undefined);
  failure = true;
  await act(async () =>
    client.invalidateQueries({
      queryKey: managedAgentHostStatusQueryKey(actor),
    }),
  );
  await waitFor(() =>
    assert.match(screen.getByRole("alert").textContent, /unavailable/),
  );
  assert.equal(values.at(-1), undefined);
});

async function mountDirectory(backendType = "local", actions = {}) {
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "isolated",
        relayUrl: "ws://localhost:3037",
        addedAt: new Date(0).toISOString(),
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "isolated");
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { render } = await import("@testing-library/react");
  const { AgentDirectoryDetail } = await import("./AgentDirectoryDetail.tsx");
  return render(
    React.createElement(AgentDirectoryDetail, {
      row: {
        pubkey: actor,
        name: "Mira",
        backendType,
        isInstalled: true,
        isRunning: false,
        modelLabel: "configured",
        homeRole: null,
        project: null,
        projectKnown: true,
        seats: [],
      },
      onClose() {},
      onSelectSeat() {},
      onStart: actions.start ?? (() => {}),
      onStop: actions.stop ?? (() => {}),
      onRestart: actions.restart ?? (() => {}),
      onEdit() {},
      canEdit: true,
      isStartPending: false,
      isRestartPending: false,
      isPending: false,
    }),
    {
      wrapper: ({ children }) =>
        React.createElement(QueryClientProvider, { client }, children),
    },
  );
}
test("directory disables old local executor controls and modern ownership updates despite no local PID", async () => {
  info = { ...status(), ownershipPhase: null, legacyRecreationRequired: true };
  let stops = 0;
  await mountDirectory("local", { stop: () => stops++ });
  const { screen, waitFor, act, fireEvent } = await import(
    "@testing-library/react"
  );
  const {
    AGENT_DETAIL_START_TESTID,
    AGENT_DETAIL_STOP_TESTID,
    AGENT_DETAIL_RESTART_TESTID,
  } = await import("./agentDirectoryCopy.ts");
  await waitFor(() =>
    assert.ok(screen.getByTestId("agent-clean-start-notice")),
  );
  for (const id of [
    AGENT_DETAIL_START_TESTID,
    AGENT_DETAIL_STOP_TESTID,
    AGENT_DETAIL_RESTART_TESTID,
  ])
    assert.equal(screen.getByTestId(id).disabled, true);
  info = status();
  await act(async () =>
    client.invalidateQueries({
      queryKey: managedAgentHostStatusQueryKey(actor),
    }),
  );
  await waitFor(() =>
    assert.equal(screen.getByTestId(AGENT_DETAIL_STOP_TESTID).disabled, false),
  );
  assert.equal(screen.getByTestId(AGENT_DETAIL_START_TESTID).disabled, true);
  fireEvent.click(screen.getByTestId(AGENT_DETAIL_STOP_TESTID));
  assert.equal(stops, 1);
});
test("remote directory uses its provider controls without a local host ownership query", async () => {
  info = { ...status(), ownershipPhase: null, legacyRecreationRequired: true };
  await mountDirectory("provider");
  const { screen } = await import("@testing-library/react");
  const { AGENT_DETAIL_START_TESTID } = await import("./agentDirectoryCopy.ts");
  assert.equal(screen.getByTestId(AGENT_DETAIL_START_TESTID).disabled, false);
  assert.equal(screen.queryByTestId("agent-host-management"), null);
  assert.equal(calls.length, 0);
});

test("only an affirmed fresh pending operation exposes retry without inventing client operation IDs", async () => {
  info = {
    ...status(),
    ownershipPhase: null,
    ownershipPendingReason: "Retry fresh Create with retained operation",
    freshCreationPending: true,
  };
  await mount();
  const { screen, fireEvent, waitFor, act } = await import(
    "@testing-library/react"
  );
  const retry = await screen.findByRole("button", {
    name: "Retry fresh Create",
  });
  for (let i = 0; i < 2; i++) {
    await waitFor(() => assert.equal(retry.disabled, false));
    await act(async () => {
      fireEvent.click(retry);
    });
    await waitFor(() =>
      assert.equal(
        calls.filter((row) => row.command === "start_managed_agent").length,
        i + 1,
      ),
    );
    await screen.findByText(/Create receipt pending/);
  }
  for (const call of calls.filter(
    (row) => row.command === "start_managed_agent",
  )) {
    assert.equal(call.args.pubkey, actor);
    assert.equal(call.args.expectedRelayUrl, info.relayUrl);
    assert.equal(Object.hasOwn(call.args, "operationId"), false);
  }
  info = {
    ...info,
    freshCreationPending: false,
    ownershipPendingReason: "Retained ownership is unresolved",
  };
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Refresh host status" }),
    );
  });
  await screen.findByText("Retained ownership is unresolved");
  assert.equal(
    screen.queryByRole("button", { name: "Retry fresh Create" }),
    null,
  );
});

test("profile host-enabled mutation refreshes a concurrently mounted directory host observation", async () => {
  info = status();
  await mount();
  const { screen, waitFor, renderHook, act } = await import(
    "@testing-library/react"
  );
  const { useSetManagedAgentStartOnAppLaunchMutation } = await import(
    "@/features/agents/hooks"
  );
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-management").dataset.observedPhase,
      "adopted",
    ),
  );
  const { result } = renderHook(
    () => useSetManagedAgentStartOnAppLaunchMutation(),
    {
      wrapper: ({ children }) =>
        React.createElement(QueryClientProvider, { client }, children),
    },
  );
  await act(async () => {
    await result.current.mutateAsync({
      pubkey: actor,
      startOnAppLaunch: false,
    });
  });
  await waitFor(() =>
    assert.equal(
      screen.getByTestId("agent-host-management").dataset.observedPhase,
      "hostStopped",
    ),
  );
  await screen.findByText(/Host work is stopped/);
  assert.equal(
    calls.filter(
      (row) => row.command === "set_managed_agent_start_on_app_launch",
    ).length,
    1,
  );
});
