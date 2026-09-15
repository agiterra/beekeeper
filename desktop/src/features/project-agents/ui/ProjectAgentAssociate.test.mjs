/**
 * "Associate with <Project>" asks first, sends exactly `{ pubkey, projectRef }`
 * to `associate_managed_agent_with_project`, and shows a refusal verbatim.
 *
 * The Tauri bridge is stubbed at `window.__TAURI_INTERNALS__.invoke`, so the
 * component runs the real mutation and wrapper.
 */
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
  for (const client of clients) client.clear();
  clients = [];
  calls = [];
  answers = {};
});

after(() => dom.window.close());

const PUBKEY = "2".repeat(64);
const PROJECT_REF = `30621:${"6".repeat(64)}:tank-loop`;

function rawAgent(projectRef) {
  return {
    pubkey: PUBKEY,
    name: "Bob",
    persona_id: null,
    home_role: "builder",
    project_ref: projectRef,
    relay_url: "wss://relay.example",
    acp_command: "",
    agent_command: "",
    agent_args: [],
    mcp_command: "",
    turn_timeout_seconds: 0,
    idle_timeout_seconds: null,
    max_turn_duration_seconds: null,
    parallelism: 1,
    system_prompt: null,
    model: null,
    status: "stopped",
    pid: null,
    created_at: "2026-09-14T00:00:00Z",
    updated_at: "2026-09-14T00:00:00Z",
    last_started_at: null,
    last_stopped_at: null,
    last_exit_code: null,
    last_error: null,
    log_path: "",
    start_on_app_launch: false,
    backend: { type: "local" },
  };
}

async function mount(props = {}) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { ProjectAgentAssociate } = await import("./ProjectAgentAssociate.tsx");
  const client = new QueryClient({
    defaultOptions: {
      mutations: { gcTime: 0, retry: false },
      queries: { gcTime: 0, retry: false },
    },
  });
  clients.push(client);
  const invalidated = [];
  const original = client.invalidateQueries.bind(client);
  client.invalidateQueries = (filters) => {
    invalidated.push(filters?.queryKey);
    return original(filters);
  };
  await act(async () => {
    render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(ProjectAgentAssociate, {
          access: { kind: "allowed" },
          name: "Bob",
          projectName: "Tank Loop",
          projectRef: PROJECT_REF,
          pubkey: PUBKEY,
          role: "builder",
          ...props,
        }),
      ),
    );
  });
  return { invalidated };
}

async function click(testId) {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  await act(async () => {
    fireEvent.click(screen.getByTestId(testId));
  });
}

test("asks first, then associates exactly this agent with this project", async () => {
  const { screen } = await import("@testing-library/react");
  answers.associate_managed_agent_with_project = () => rawAgent(PROJECT_REF);
  const { invalidated } = await mount();

  await click("project-agent-associate-button");
  const associateCalls = () =>
    calls.filter(
      (call) => call.command === "associate_managed_agent_with_project",
    );
  assert.equal(
    associateCalls().length,
    0,
    "nothing is sent before confirmation",
  );
  assert.equal(
    screen
      .getByTestId("project-agent-associate-confirm")
      .textContent.includes(
        "Bob becomes a permanent Tank Loop Builder agent. Its history stays attributed to Bob. This does not change project access.",
      ),
    true,
  );

  await click("project-agent-associate-yes");
  assert.deepEqual(associateCalls(), [
    {
      command: "associate_managed_agent_with_project",
      args: { pubkey: PUBKEY, projectRef: PROJECT_REF },
    },
  ]);
  assert.deepEqual(invalidated[0], ["managed-agents"]);
  assert.equal(screen.queryByTestId("project-agent-associate-confirm"), null);
});

test("a refusal is shown in native's own words and the confirmation stays open", async () => {
  const { screen } = await import("@testing-library/react");
  answers.associate_managed_agent_with_project = () => {
    throw "Bob is already associated with 30621:abc:attic.";
  };
  await mount();

  await click("project-agent-associate-button");
  await click("project-agent-associate-yes");
  assert.match(
    screen.getByTestId("project-agent-associate-error").textContent,
    /Bob is already associated with 30621:abc:attic\./,
  );
  assert.ok(screen.getByTestId("project-agent-associate-confirm"));
});

test("a denied viewer cannot start the confirmation", async () => {
  const { screen } = await import("@testing-library/react");
  await mount({
    access: {
      kind: "denied",
      reason:
        "Only Tank Loop's owners and collaborators can associate agents with it.",
    },
  });
  assert.equal(
    screen.getByTestId("project-agent-associate-button").disabled,
    true,
  );
  assert.equal(
    screen.getByTestId("project-agent-associate-denied").textContent,
    "Only Tank Loop's owners and collaborators can associate agents with it.",
  );
});
