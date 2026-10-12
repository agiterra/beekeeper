import assert from "node:assert/strict";
import { before, after, afterEach, test } from "node:test";
import { JSDOM } from "jsdom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { managedAgentsQueryKey } from "@/features/agents/hooks";
import { managedAgentHostTransferQueryKey } from "@/features/agents/hooks";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const actor = "ab".repeat(32);
const operation = "87173c50-92e0-4a74-a6e6-38373f6f0a60";
let calls = [];
let phase = "quiesced";
let dropStart = false;
let queryClient = new QueryClient();
let currentOperation = operation;
let getterUnavailable = false;
let eligible = true;
let commissioned = true;
let headAvailable = true;
function seedCommunity() {
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
}
before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    localStorage: dom.window.localStorage,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  seedCommunity();
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "get_managed_agent_host_transfer") {
        if (getterUnavailable) throw new Error("community head unavailable");
        return {
          relayUrl: "ws://127.0.0.1:3037",
          revision: args.localOnly || !headAvailable ? "" : "cd".repeat(32),
          revisionAvailable: !args.localOnly && headAvailable,
          commissioned,
          migrationEligible: eligible,
          channel: "9202c9e9-8bfc-4a20-8a5f-ae134e563edb",
          cwd: "/isolated/project",
          transfer:
            phase === null
              ? null
              : {
                  operationId: currentOperation,
                  actorPubkey: actor,
                  relayUrl: "ws://127.0.0.1:3037",
                  phase,
                },
        };
      }
      if (command === "start_managed_agent") {
        if (dropStart) throw new Error("acceptance reply lost");
        phase = "adopted";
        return {
          pubkey: actor,
          name: "Mira",
          agent_args: [],
          backend: { type: "local" },
        };
      }
      throw new Error(`unexpected command ${command}`);
    },
  };
});
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  calls = [];
  phase = "quiesced";
  dropStart = false;
  currentOperation = operation;
  getterUnavailable = false;
  eligible = true;
  commissioned = true;
  headAvailable = true;
  localStorage.clear();
  seedCommunity();
  queryClient.clear();
  queryClient = new QueryClient();
});
after(() => dom.window.close());
async function mount() {
  const React = await import("react");
  const { render, act, fireEvent, screen, waitFor } = await import(
    "@testing-library/react"
  );
  const { HostTransferPanel } = await import("./HostTransferPanel.tsx");
  await act(async () => {
    render(React.createElement(HostTransferPanel, { pubkey: actor }), {
      wrapper: ({ children }) =>
        React.createElement(
          QueryClientProvider,
          { client: queryClient },
          children,
        ),
    });
  });
  await waitFor(() =>
    assert.equal(
      screen
        .getByTestId("agent-host-transfer")
        .getAttribute("data-observed-phase"),
      phase ?? "legacy",
    ),
  );
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", {
        name: /^(Migration status|Retry migration)$/,
      }),
    );
  });
  return { act, fireEvent, screen, waitFor };
}
test("lost management reply retries shared start without client-generated transfer operations", async () => {
  dropStart = true;
  const { act, fireEvent, screen } = await mount();
  await act(async () => {
    fireEvent.click(screen.getByTestId("agent-host-transfer-move"));
  });
  assert.match(screen.getByRole("alert").textContent, /acceptance reply lost/);
  dropStart = false;
  await act(async () => {
    fireEvent.click(screen.getByTestId("agent-host-transfer-move"));
  });
  const starts = calls.filter((c) => c.command === "start_managed_agent");
  assert.equal(starts.length, 2);
  assert.ok(
    starts.every(
      (c) =>
        c.args.pubkey === actor &&
        c.args.expectedRelayUrl === "ws://127.0.0.1:3037",
    ),
  );
  assert.ok(starts.every((c) => !("operationId" in c.args)));
  assert.ok(!calls.some((c) => /prepare_|adopt_|rollback_/.test(c.command)));
  assert.equal(
    localStorage.getItem(
      `beekeeper-host-transfer:ws://127.0.0.1:3037:${actor}`,
    ),
    null,
  );
  assert.match(
    screen.getByTestId("agent-host-transfer-phase").textContent,
    /Host accepted/,
  );
  assert.equal(screen.queryByTestId("agent-host-transfer-move"), null);
});
test("migration never offers a control that restores desktop execution", async () => {
  phase = "prepared";
  const { screen } = await mount();
  assert.equal(screen.queryByTestId("agent-host-transfer-rollback"), null);
  assert.equal(screen.queryByText(/restore desktop ownership/i), null);
  assert.ok(
    !calls.some(
      (entry) => entry.command === "rollback_managed_agent_host_transfer",
    ),
  );
});

test("stopped primary offers an explicit fresh context through the shared start adapter", async () => {
  phase = "hostStopped";
  const { act, fireEvent, screen } = await mount();
  assert.match(
    screen.getByText(/Migration preserves existing/).textContent,
    /blocked for repair/,
  );
  await act(async () => {
    fireEvent.click(screen.getByTestId("agent-host-transfer-start"));
  });
  const start = calls.find((c) => c.command === "start_managed_agent");
  assert.equal(start.args.pubkey, actor);
  assert.equal(start.args.expectedRelayUrl, "ws://127.0.0.1:3037");
  assert.match(
    screen.getByTestId("agent-host-transfer-phase").textContent,
    /Host accepted/,
  );
});

async function mountDetail() {
  const React = await import("react");
  const { render, act, screen, waitFor } = await import(
    "@testing-library/react"
  );
  const { AgentDirectoryDetail } = await import("./AgentDirectoryDetail.tsx");
  const props = {
    row: {
      pubkey: actor,
      name: "Mira",
      isInstalled: true,
      isRunning: false,
      modelLabel: "gpt",
      homeRole: null,
      project: null,
      projectKnown: true,
      seats: [],
    },
    onClose() {},
    onSelectSeat() {},
    onStart() {},
    onStop() {},
    onRestart() {},
    onEdit() {},
    canEdit: true,
    isStartPending: false,
    isRestartPending: false,
    isPending: false,
  };
  let view;
  await act(async () => {
    view = render(React.createElement(AgentDirectoryDetail, props), {
      wrapper: ({ children }) =>
        React.createElement(
          QueryClientProvider,
          { client: queryClient },
          children,
        ),
    });
  });
  await waitFor(() =>
    assert.equal(
      screen
        .getByTestId("agent-host-transfer")
        .getAttribute("data-observed-phase"),
      phase ?? "legacy",
    ),
  );
  return { React, act, screen, props, view, waitFor };
}
test("directory controls follow observed ownership without treating host admission as local running", async () => {
  const { cleanup } = await import("@testing-library/react");
  for (const [observed, expected] of [
    ["adopted", [false, true, true]],
    ["hostStopped", [true, false, true]],
    ["prepared", [false, false, false]],
    ["quiesced", [false, false, false]],
    ["restoring", [false, false, false]],
    ["legacyRestored", [true, false, true]],
    [null, [true, false, true]],
  ]) {
    phase = observed;
    const { screen } = await mountDetail();
    for (const [index, id] of [
      "agent-detail-start",
      "agent-detail-stop",
      "agent-detail-restart",
    ].entries())
      assert.equal(
        !screen.getByTestId(id).disabled,
        expected[index],
        `${observed}: ${id}`,
      );
    cleanup();
  }
});

test("management settling invalidates a successor operation even when phase stays adopted", async () => {
  phase = "adopted";
  queryClient.setQueryData(managedAgentsQueryKey, [
    { pubkey: actor, lastError: "old operation" },
  ]);
  const { React, act, props, view, waitFor } = await mountDetail();
  const { AgentDirectoryDetail } = await import("./AgentDirectoryDetail.tsx");
  await act(async () =>
    view.rerender(
      React.createElement(AgentDirectoryDetail, { ...props, isPending: true }),
    ),
  );
  currentOperation = "4b2349b8-f380-4209-b64b-ecfc52f4c5c5";
  await act(async () =>
    view.rerender(React.createElement(AgentDirectoryDetail, props)),
  );
  await waitFor(() =>
    assert.equal(
      queryClient.getQueryState(managedAgentsQueryKey).isInvalidated,
      true,
    ),
  );
});
test("directory rereads ownership when a management action settles without a local PID change", async () => {
  phase = "adopted";
  const { React, act, screen, props, view, waitFor } = await mountDetail();
  assert.equal(screen.getByTestId("agent-detail-stop").disabled, false);
  await act(async () => {
    view.rerender(
      React.createElement(
        (await import("./AgentDirectoryDetail.tsx")).AgentDirectoryDetail,
        { ...props, isPending: true },
      ),
    );
  });
  phase = "hostStopped";
  await act(async () => {
    view.rerender(
      React.createElement(
        (await import("./AgentDirectoryDetail.tsx")).AgentDirectoryDetail,
        props,
      ),
    );
  });
  assert.equal(props.row.isRunning, false);
  await waitFor(() =>
    assert.equal(screen.getByTestId("agent-detail-start").disabled, false),
  );
  assert.equal(screen.getByTestId("agent-detail-stop").disabled, true);
});

test("profile successor refreshes an already-mounted directory panel without local-only revision poisoning", async () => {
  phase = "adopted";
  queryClient.setQueryData(
    [
      ...managedAgentHostTransferQueryKey(actor),
      "local",
      "ws://127.0.0.1:3037",
    ],
    { revision: "", transfer: { phase, operationId: operation } },
  );
  const { act, screen } = await mount();
  const React = await import("react");
  const { renderHook, waitFor } = await import("@testing-library/react");
  const { useAgentLifecycleActions } = await import(
    "@/features/profile/ui/useAgentLifecycleActions.ts"
  );
  const { profileAgentControl } = await import(
    "@/features/profile/lib/profileAgentControl.ts"
  );
  const local = {
    pubkey: actor,
    backend: { type: "local" },
    status: "stopped",
    name: "Mira",
  };
  const successor = "f40ebdc5-9fbf-46e2-a8f7-4489cc8b7c11";
  const { result } = renderHook(
    () =>
      useAgentLifecycleActions({
        managedAgent: local,
        control: profileAgentControl(local, "adopted"),
        channels: [],
        relayAgents: [],
        stopManagedAgent: async () => {
          phase = "hostStopped";
        },
        startManagedAgent: async () => {
          currentOperation = successor;
          phase = "adopted";
        },
      }),
    {
      wrapper: ({ children }) =>
        React.createElement(
          QueryClientProvider,
          { client: queryClient },
          children,
        ),
    },
  );
  await act(async () => result.current.handleAgentRestart());
  await waitFor(() =>
    assert.equal(
      screen
        .getByTestId("agent-host-transfer")
        .getAttribute("data-operation-id"),
      successor,
    ),
  );
  const full = queryClient
    .getQueryCache()
    .findAll({ queryKey: managedAgentHostTransferQueryKey(actor) })
    .find((query) => query.queryKey[2] === "full");
  assert.equal(
    full.state.data.revision,
    "cd".repeat(32),
    "Move requires the full verified head, never local-only empty revision",
  );
  assert.equal(full.state.data.transfer.operationId, successor);
  assert.equal(screen.queryByTestId("agent-host-transfer-rollback"), null);
});

test("community switch with unavailable new head removes old-pair rollback and controls", async () => {
  phase = "adopted";
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "first",
        relayUrl: "ws://localhost:3037",
        addedAt: new Date(0).toISOString(),
      },
      {
        id: "second",
        relayUrl: "ws://another-relay",
        addedAt: new Date(0).toISOString(),
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "first");
  const { React, act, screen, props, view, waitFor } = await mountDetail();
  const { fireEvent } = await import("@testing-library/react");
  await act(async () =>
    fireEvent.click(
      screen.getByRole("button", {
        name: /^(Migration status|Retry migration)$/,
      }),
    ),
  );
  assert.equal(screen.queryByTestId("agent-host-transfer-rollback"), null);
  getterUnavailable = true;
  localStorage.setItem("buzz-active-community-id", "second");
  const { AgentDirectoryDetail } = await import("./AgentDirectoryDetail.tsx");
  await act(async () =>
    view.rerender(React.createElement(AgentDirectoryDetail, { ...props })),
  );
  await waitFor(() =>
    assert.equal(
      screen
        .getByTestId("agent-host-transfer")
        .getAttribute("data-observed-phase"),
      "unknown",
    ),
  );
  assert.equal(screen.queryByTestId("agent-host-transfer-rollback"), null);
  assert.equal(screen.getByTestId("agent-detail-start").disabled, true);
  assert.equal(screen.getByTestId("agent-detail-stop").disabled, true);
  assert.equal(screen.getByTestId("agent-detail-restart").disabled, true);
});

test("eligible migration retries bootstrap through shared start without manual execution inputs", async () => {
  phase = null;
  commissioned = false;
  headAvailable = false;
  const { act, fireEvent, screen } = await mount();
  assert.equal(screen.queryByLabelText("Conversation channel UUID"), null);
  assert.equal(screen.queryByLabelText("Working directory"), null);
  assert.equal(screen.queryByText(/Commission this host/), null);
  assert.equal(screen.getByTestId("agent-host-transfer-move").disabled, false);
  await act(async () =>
    fireEvent.click(screen.getByTestId("agent-host-transfer-move")),
  );
  assert.equal(
    calls.filter((c) => c.command === "start_managed_agent").length,
    1,
  );
  assert.equal(
    screen
      .getByTestId("agent-host-transfer")
      .getAttribute("data-observed-phase"),
    "adopted",
  );
});

test("unknown or ineligible custody never enables migration retry", async () => {
  phase = "prepared";
  eligible = false;
  const { act, fireEvent, screen } = await mount();
  assert.equal(screen.getByTestId("agent-host-transfer-move").disabled, true);
  await act(async () =>
    fireEvent.click(screen.getByTestId("agent-host-transfer-move")),
  );
  assert.equal(
    calls.filter((c) => c.command === "start_managed_agent").length,
    0,
  );
});
