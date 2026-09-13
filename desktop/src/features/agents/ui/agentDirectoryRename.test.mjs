/**
 * Renaming an installed project agent from its directory row (Tank Loop
 * walkthrough §4). Rename in place already exists natively —
 * `update_managed_agent { pubkey, name }` keeps the pubkey and role and
 * republishes kind:0 — so the row must send exactly that and nothing else:
 * no persona, no role, no model, no minted identity.
 *
 * The Tauri bridge is stubbed at `window.__TAURI_INTERNALS__.invoke`, the one
 * function `@tauri-apps/api` calls, so the form under test runs the real
 * mutation hook and wrapper.
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

const PUBKEY = "1c47d440".padEnd(64, "0");

function rawAgent(name) {
  return {
    pubkey: PUBKEY,
    name,
    persona_id: null,
    home_role: "lead",
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
    created_at: "2026-09-13T00:00:00Z",
    updated_at: "2026-09-13T00:00:00Z",
    last_started_at: null,
    last_stopped_at: null,
    last_exit_code: null,
    last_error: null,
    log_path: "",
    start_on_app_launch: false,
    backend: { type: "local" },
  };
}

async function mountForm(props = {}) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { AgentDirectoryRenameForm } = await import(
    "./AgentDirectoryRename.tsx"
  );
  // gcTime 0: a finished mutation otherwise holds a five-minute timer that
  // keeps the test process alive after every assertion has passed.
  const client = new QueryClient({
    defaultOptions: {
      mutations: { gcTime: 0, retry: false },
      queries: { gcTime: 0, retry: false },
    },
  });
  clients.push(client);
  const done = [];
  await act(async () => {
    render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(AgentDirectoryRenameForm, {
          name: "Loom",
          onDone: () => done.push(true),
          pubkey: PUBKEY,
          ...props,
        }),
      ),
    );
  });
  return { done };
}

async function typeAndSave(value) {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  await act(async () => {
    fireEvent.change(screen.getByTestId("agent-row-rename-input"), {
      target: { value },
    });
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("agent-row-rename-save"));
  });
}

test("saving sends update_managed_agent with exactly { pubkey, name }, trimmed", async () => {
  answers.update_managed_agent = () => ({
    agent: rawAgent("Weaver"),
    profile_sync_error: null,
  });
  const { done } = await mountForm();

  await typeAndSave("  Weaver  ");

  const updates = calls.filter(
    (call) => call.command === "update_managed_agent",
  );
  assert.equal(updates.length, 1);
  assert.deepEqual(updates[0].args, {
    input: { pubkey: PUBKEY, name: "Weaver" },
  });
  assert.deepEqual(
    Object.keys(updates[0].args.input).sort(),
    ["name", "pubkey"],
    "no persona, role, model or runtime field rides along",
  );
  assert.equal(done.length, 1, "the form closes after a clean rename");
});

test("a blank name is refused before anything is sent", async () => {
  const { screen } = await import("@testing-library/react");
  const { done } = await mountForm();

  await typeAndSave("   ");

  assert.equal(
    calls.filter((call) => call.command === "update_managed_agent").length,
    0,
  );
  assert.equal(
    screen.getByTestId("agent-row-rename-error").textContent,
    "Enter a name.",
  );
  assert.equal(done.length, 0);
});

test("a failed rename shows its cause plainly and keeps the form open", async () => {
  const { screen } = await import("@testing-library/react");
  answers.update_managed_agent = () => {
    throw new Error("agent store is locked");
  };
  const { done } = await mountForm();

  await typeAndSave("Weaver");

  assert.match(
    screen.getByTestId("agent-row-rename-error").textContent,
    /^The name was not changed: .*agent store is locked/,
  );
  assert.equal(screen.getByTestId("agent-row-rename-input").value, "Weaver");
  assert.equal(done.length, 0);
});

test("a rename whose relay profile did not publish says so instead of closing", async () => {
  const { screen } = await import("@testing-library/react");
  answers.update_managed_agent = () => ({
    agent: rawAgent("Weaver"),
    profile_sync_error: "relay unreachable",
  });
  const { done } = await mountForm();

  await typeAndSave("Weaver");

  assert.equal(
    screen.getByTestId("agent-row-rename-error").textContent,
    "Renamed on this computer, but the relay may still show the old name: relay unreachable",
  );
  assert.equal(done.length, 0);
});

function directoryRow(overrides = {}) {
  return {
    pubkey: PUBKEY,
    name: "Loom",
    isInstalled: true,
    isRunning: false,
    isStopped: true,
    lastError: null,
    needsRestart: false,
    modelLabel: "gpt",
    homeRole: "lead",
    roleHistoryLabel: "no seats recorded",
    roleSlugs: ["lead"],
    currentSeat: null,
    seatProjectIds: new Set(),
    installedProjects: [
      {
        projectRef: `30621:${"c".repeat(64)}:tank-loop`,
        projectId: `${"c".repeat(64)}:tank-loop`,
        projectName: "Tank Loop",
        role: "lead",
      },
    ],
    installedProjectIds: new Set([`${"c".repeat(64)}:tank-loop`]),
    seats: [],
    ...overrides,
  };
}

async function mountList(rows) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { AgentDirectoryList } = await import("./AgentDirectoryList.tsx");
  const client = new QueryClient({
    defaultOptions: {
      mutations: { gcTime: 0, retry: false },
      queries: { gcTime: 0, retry: false },
    },
  });
  clients.push(client);
  await act(async () => {
    render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(AgentDirectoryList, {
          allRows: rows,
          error: null,
          filters: {
            role: null,
            status: "any",
            projectId: null,
            installedOnly: true,
          },
          isLoading: false,
          onAddAgent: () => {},
          onFiltersChange: () => {},
          onSelectRow: () => {},
          projects: [],
          rows,
          seatNotice: null,
          selectedPubkey: null,
        }),
      ),
    );
  });
}

test("the directory row names the installation and offers Rename only to installed project agents", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  const other = "2".repeat(64);
  await mountList([
    directoryRow(),
    directoryRow({
      pubkey: other,
      name: "Solo",
      installedProjects: [],
      installedProjectIds: new Set(),
    }),
  ]);

  assert.deepEqual(
    screen
      .getAllByTestId("agent-row-installed-for")
      .map((node) => node.textContent),
    ["Installed for Tank Loop · lead"],
  );
  const renameButtons = screen.getAllByTestId("agent-row-rename");
  assert.equal(renameButtons.length, 1);
  assert.equal(
    renameButtons[0].closest("[data-testid='agent-row']"),
    null,
    "the action is beside the row button, not nested inside it",
  );

  await act(async () => {
    fireEvent.click(renameButtons[0]);
  });
  assert.equal(screen.getByTestId("agent-row-rename-input").value, "Loom");
});
