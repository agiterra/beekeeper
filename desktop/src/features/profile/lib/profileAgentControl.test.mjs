import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { profileAgentControl } from "./profileAgentControl.ts";
import React from "react";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const actor = "ab".repeat(32);
const local = {
  pubkey: actor,
  relayUrl: "ws://127.0.0.1:3037",
  backend: { type: "local" },
  status: "stopped",
  pid: null,
  name: "Mira",
};
let client;
let observedRelay = "ws://localhost:3037/";
let observedPhase = "adopted";
function wrapper({ children }) {
  return React.createElement(QueryClientProvider, { client }, children);
}
before(() => {
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    localStorage: dom.window.localStorage,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      if (command === "get_managed_agent_host_transfer") {
        assert.equal(
          args.localOnly,
          true,
          "profile observation must not require a relay head/network",
        );
        return {
          relayUrl: observedRelay,
          revision: "",
          transfer:
            observedPhase === null
              ? null
              : { phase: observedPhase, operationId: "verified-operation" },
        };
      }
      if (command === "list_managed_agent_runtimes")
        return [
          {
            pubkey: actor,
            relayUrl: "ws://another-relay",
            lifecycle: "ready",
            error: null,
          },
          {
            pubkey: actor,
            relayUrl: local.relayUrl,
            lifecycle: observedPhase === null ? "stopped" : "host_managed",
            pid: null,
            error:
              "Host managed; exact accepted target. Availability unconfirmed.",
          },
        ];
      throw new Error(`unexpected command ${command}`);
    },
  };
});
afterEach(async () => {
  observedRelay = "ws://localhost:3037/";
  observedPhase = "adopted";
  localStorage.clear();
  (await import("@testing-library/react")).cleanup();
  client?.clear();
});
after(() => dom.window.close());

test("legacy empty relay resolves adopted ownership from the selected community, not the response", async () => {
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "isolated",
        name: "Isolated",
        relayUrl: "ws://localhost:3037/",
        addedAt: new Date(0).toISOString(),
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "isolated");
  const React = await import("react");
  const { renderHook, waitFor } = await import("@testing-library/react");
  const { useProfileAgentControl } = await import(
    "./useProfileAgentControl.ts"
  );
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const legacy = { ...local, relayUrl: "" };
  const { result } = renderHook(() => useProfileAgentControl(legacy, false), {
    wrapper: ({ children }) =>
      React.createElement(QueryClientProvider, { client }, children),
  });
  await waitFor(() => assert.equal(result.current.action, "stop"));
  assert.equal(result.current.label, "Stop host work");
  assert.equal(result.current.canRestart, true);
  await waitFor(() =>
    assert.match(result.current.statusText, /exact accepted target/),
  );
});

test("empty relay without established matching community cannot borrow the adopted response", async () => {
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "different",
        name: "Different",
        relayUrl: "ws://another-relay",
        addedAt: new Date(0).toISOString(),
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "different");
  const React = await import("react");
  const { renderHook, waitFor } = await import("@testing-library/react");
  const { useProfileAgentControl } = await import(
    "./useProfileAgentControl.ts"
  );
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(
    () => useProfileAgentControl({ ...local, relayUrl: "" }, false),
    {
      wrapper: ({ children }) =>
        React.createElement(QueryClientProvider, { client }, children),
    },
  );
  await waitFor(() =>
    assert.equal(
      client.getQueryState([
        "managed-agent-host-transfer",
        actor,
        "local",
        "ws://another-relay",
      ])?.status,
      "success",
    ),
  );
  assert.equal(result.current.action, null);
  assert.equal(result.current.canRestart, false);
});

test("unpublished or offline legacy profile retains local controls from local-only ownership", async () => {
  observedPhase = null;
  const React = await import("react");
  const { renderHook, waitFor } = await import("@testing-library/react");
  const { useProfileAgentControl } = await import(
    "./useProfileAgentControl.ts"
  );
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(() => useProfileAgentControl(local, false), {
    wrapper: ({ children }) =>
      React.createElement(QueryClientProvider, { client }, children),
  });
  await waitFor(() => assert.equal(result.current.action, "start"));
  assert.equal(result.current.label, "Start agent");
  assert.equal(result.current.statusText, null);
});

test("a different active community cannot supply adopted ownership for this profile", async () => {
  observedRelay = "ws://another-relay";
  const React = await import("react");
  const { renderHook, waitFor } = await import("@testing-library/react");
  const { useProfileAgentControl } = await import(
    "./useProfileAgentControl.ts"
  );
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(() => useProfileAgentControl(local, false), {
    wrapper: ({ children }) =>
      React.createElement(QueryClientProvider, { client }, children),
  });
  await waitFor(() =>
    assert.equal(
      client.getQueryState([
        "managed-agent-host-transfer",
        actor,
        "local",
        local.relayUrl,
      ])?.status,
      "success",
    ),
  );
  assert.equal(result.current.action, null);
  assert.equal(result.current.canRestart, false);
  assert.match(result.current.statusText, /Ownership unconfirmed/);
});

test("canonical profile observation overrides stopped local record and ignores another relay's Ready", async () => {
  const React = await import("react");
  const { renderHook, waitFor, act } = await import("@testing-library/react");
  const { useProfileAgentControl } = await import(
    "./useProfileAgentControl.ts"
  );
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(() => useProfileAgentControl(local, false), {
    wrapper: ({ children }) =>
      React.createElement(QueryClientProvider, { client }, children),
  });
  await waitFor(() => assert.equal(result.current.action, "stop"));
  await waitFor(() =>
    assert.match(result.current.statusText, /exact accepted target/),
  );
  assert.equal(result.current.canRestart, true);
  assert.equal(result.current.label, "Stop host work");
  assert.doesNotMatch(result.current.statusText, /Ready|Stopped/);
  assert.equal(local.status, "stopped");
  observedPhase = "hostStopped";
  await act(async () =>
    client.invalidateQueries({
      queryKey: ["managed-agent-host-transfer", actor, "local", local.relayUrl],
    }),
  );
  await waitFor(() => assert.equal(result.current.action, "start"));
  assert.equal(result.current.label, "Start new host context");
});

test("profile controls fail closed through transitions and preserve restored legacy rules", () => {
  for (const phase of [undefined, "prepared", "quiesced", "restoring"]) {
    const observed = profileAgentControl(local, phase);
    assert.equal(observed.action, null, String(phase));
    assert.equal(observed.canRestart, false, String(phase));
    assert.doesNotMatch(observed.statusText, /^Stopped$|Ready/);
  }
  assert.equal(profileAgentControl(local, "hostStopped").action, "start");
  assert.match(
    profileAgentControl(local, "hostStopped").label,
    /new host context/,
  );
  assert.equal(profileAgentControl(local, "legacyRestored").action, "start");
  assert.equal(profileAgentControl(local, "legacyRestored").statusText, null);
  assert.equal(
    profileAgentControl({ ...local, status: "running" }, "legacyRestored")
      .action,
    "stop",
  );
});

test("adopted profile primary action stops and restart uses shared stop-start despite local stopped status", async () => {
  client = new QueryClient();
  const { renderHook, act } = await import("@testing-library/react");
  const { useAgentLifecycleActions } = await import(
    "../ui/useAgentLifecycleActions.ts"
  );
  const events = [];
  const { result } = renderHook(
    () =>
      useAgentLifecycleActions({
        managedAgent: local,
        control: profileAgentControl(local, "adopted"),
        channels: [],
        relayAgents: [],
        stopManagedAgent: async () => events.push("stop"),
        startManagedAgent: async () => events.push("start"),
      }),
    { wrapper },
  );
  await act(async () => result.current.handleAgentPrimaryAction());
  assert.deepEqual(events, ["stop"]);
  await act(async () => result.current.handleAgentRestart());
  assert.deepEqual(events, ["stop", "stop", "start"]);
});

test("transition controls dispatch nothing and failed host stop prevents restart start", async () => {
  client = new QueryClient();
  const { renderHook, act } = await import("@testing-library/react");
  const { useAgentLifecycleActions } = await import(
    "../ui/useAgentLifecycleActions.ts"
  );
  const events = [];
  const options = {
    managedAgent: local,
    channels: [],
    relayAgents: [],
    stopManagedAgent: async () => {
      events.push("stop");
      throw new Error("unconfirmed sibling stop");
    },
    startManagedAgent: async () => events.push("start"),
  };
  const { result, rerender } = renderHook(
    ({ phase }) =>
      useAgentLifecycleActions({
        ...options,
        control: profileAgentControl(local, phase),
      }),
    { initialProps: { phase: "restoring" }, wrapper },
  );
  await act(async () => {
    await result.current.handleAgentPrimaryAction();
    await result.current.handleAgentRestart();
  });
  assert.deepEqual(events, []);
  rerender({ phase: "adopted" });
  await act(async () => result.current.handleAgentRestart());
  assert.deepEqual(events, ["stop"]);
});
