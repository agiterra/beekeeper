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
const draft = {
  setupId: "setup",
  projectRef: `30621:${"a".repeat(64)}:garden`,
  relayUrl: "wss://example.test",
  ownerPubkey: "a".repeat(64),
  roles: ["lead"],
  status: "draft",
  intent: "Garden",
  projectDirectory: "/garden",
  draftDirectory: "/draft",
  rolesDirectory: "/draft/roles",
  createdAt: "today",
};
const runtime = {
  instanceRef: "codex-primary",
  runtime: "codex",
  label: "Codex",
  driver: "codex-acp",
  authState: "ready",
  defaultModel: "default",
  allowedModels: ["default"],
  capabilities: { threadTurnStart: true },
};
const catalog = {
  instanceRef: runtime.instanceRef,
  defaultModel: "model-one[high]",
  allowedModels: ["model-one[high]", "model-two"],
};
const reservation = {
  sessionRef: "session",
  channelId: "channel",
  createCommandId: "csl-command",
  status: "reserved",
};
const launch = {
  ...reservation,
  setupId: draft.setupId,
  providerPubkey: "b".repeat(64),
  providerInstanceRef: runtime.instanceRef,
  runtime: "codex",
  model: catalog.defaultModel,
  actorPubkey: "c".repeat(64),
  status: "ambiguous",
};
function seed() {
  answers.coding_session_provider_status = {
    provisioned: false,
    running: false,
  };
  answers.coding_session_provider_runtimes = [runtime];
  answers.coding_session_provider_models = catalog;
  answers.project_team_setup_reserve_authoring = reservation;
  answers.provision_coding_session_provider = {
    provisioned: true,
    running: true,
    providerPubkey: launch.providerPubkey,
  };
}
let observed = [];
async function harness() {
  const React = (await import("react")).default;
  const { ProjectTeamSetupAuthoringControls } = await import(
    "../ui/ProjectTeamSetupAuthoring.tsx"
  );
  const testing = await import("@testing-library/react");
  observed = [];
  return {
    ...testing,
    element: React.createElement(ProjectTeamSetupAuthoringControls, {
      draft,
      channels: [{ id: "channel", name: "Garden sessions" }],
      preferredChannelId: "channel",
      channelError: null,
      onOpen: () => {},
      onLaunchObserved: (launch) => observed.push(launch),
    }),
  };
}

test("opening authoring probes real local runtimes but never provisions, reserves or starts", async () => {
  seed();
  const { render, waitFor, element } = await harness();
  const view = render(element);
  await waitFor(() =>
    assert.equal(
      view.getByRole("button", { name: "Start authoring session" }).disabled,
      false,
    ),
  );
  assert.deepEqual(
    new Set(calls.map((call) => call.command)),
    new Set([
      "get_relay_http_url",
      "get_media_proxy_port",
      "coding_session_provider_status",
      "coding_session_provider_runtimes",
      "coding_session_provider_models",
      "project_team_setup_get_authoring",
      "project_team_setup_get_launch",
    ]),
  );
});

test("runtime discovery failure cannot invent an authenticated Claude fallback", async () => {
  seed();
  answers.coding_session_provider_runtimes = () => {
    throw new Error("Runtime probe failed");
  };
  const { render, waitFor, element } = await harness();
  const view = render(element);
  await waitFor(() => assert.ok(view.getByText("Runtime probe failed")));
  assert.equal(
    view.getByRole("button", { name: "Start authoring session" }).disabled,
    true,
  );
  assert.equal(
    calls.some((call) => call.command === "coding_session_provider_models"),
    false,
  );
});

test("explicit start provisions once; ambiguous native save locks retry to original runtime and model", async () => {
  seed();
  let started = false;
  answers.project_team_setup_start_authoring = () => {
    started = true;
    answers.coding_session_provider_status =
      answers.provision_coding_session_provider;
    throw new Error("Response lost");
  };
  answers.project_team_setup_get_launch = () => (started ? launch : null);
  const { render, waitFor, fireEvent, element } = await harness();
  const view = render(element);
  await waitFor(() =>
    assert.equal(
      view.getByRole("button", { name: "Start authoring session" }).disabled,
      false,
    ),
  );
  fireEvent.click(
    view.getByRole("button", { name: "Start authoring session" }),
  );
  await waitFor(() => assert.ok(view.getByText("Response lost")));
  assert.ok(view.getByText(/Retries keep this runtime and model/));
  answers.project_team_setup_start_authoring = {
    ...launch,
    status: "created",
    target: {
      driver: "codex-acp",
      instanceId: "provider",
      sessionId: "session",
      generation: 1,
    },
  };
  fireEvent.click(
    view.getByRole("button", { name: "Retry saved authoring request" }),
  );
  await waitFor(() =>
    assert.ok(view.getByRole("button", { name: "Open authoring session" })),
  );
  assert.match(
    view.getByRole("status").textContent,
    /provider confirmed creation/,
  );
  const writes = calls.filter((call) =>
    [
      "provision_coding_session_provider",
      "project_team_setup_reserve_authoring",
      "project_team_setup_start_authoring",
    ].includes(call.command),
  );
  assert.deepEqual(
    writes.map((call) => call.command),
    [
      "provision_coding_session_provider",
      "project_team_setup_reserve_authoring",
      "project_team_setup_start_authoring",
      "project_team_setup_start_authoring",
    ],
  );
  assert.equal(writes[0].args.expectedRelayUrl, draft.relayUrl);
  assert.deepEqual(writes[2].args, writes[3].args);
  assert.equal(writes[3].args.model, catalog.defaultModel);
  assert.equal(writes[3].args.projectRef, draft.projectRef);
});

test("saved launch is read on mount without replay and does not label relay acceptance as creation", async () => {
  seed();
  answers.project_team_setup_get_authoring = reservation;
  answers.project_team_setup_get_launch = {
    ...launch,
    status: "awaiting_receipt",
  };
  const { render, waitFor, element } = await harness();
  const view = render(element);
  await waitFor(() =>
    assert.match(
      view.getByRole("status").textContent,
      /Waiting for the provider's receipt/,
    ),
  );
  assert.equal(
    view.queryByRole("button", { name: "Start authoring session" }),
    null,
  );
  assert.equal(
    view.queryByRole("button", { name: "Open authoring session" }),
    null,
  );
  // The observation is reported from an effect, which can land after the
  // status text under a loaded test run (pre-push floor, 2026-10-08).
  await waitFor(() =>
    assert.equal(observed.at(-1)?.status, "awaiting_receipt"),
  );
  assert.equal(
    calls.some((call) =>
      /start_authoring|reserve_authoring|provision_coding/.test(call.command),
    ),
    false,
  );
});

test("one failed model adapter does not hide another authenticated runtime", async () => {
  const { loadProjectTeamSetupRuntimes } = await import(
    "./useProjectTeamSetupRuntimes.ts"
  );
  const result = await loadProjectTeamSetupRuntimes({
    status: async () => ({ provisioned: false, running: false }),
    runtimes: async () => [
      runtime,
      { ...runtime, instanceRef: "broken" },
      { ...runtime, instanceRef: "signed-out", authState: "needs_auth" },
    ],
    models: async (id) => {
      if (id === "broken") throw new Error("Models unavailable");
      assert.equal(id, runtime.instanceRef);
      return catalog;
    },
  });
  assert.equal(result.models.size, 1);
  assert.equal(result.errors.get("broken"), "Models unavailable");
  assert.equal(result.models.has("signed-out"), false);
});

test("failed durable read blocks Start until explicit status check recovers both records", async () => {
  seed();
  answers.project_team_setup_get_authoring = () => {
    throw new Error("Saved reservation unreadable");
  };
  const { render, waitFor, fireEvent, element } = await harness();
  const view = render(element);
  await waitFor(() =>
    assert.ok(view.getByText("Saved reservation unreadable")),
  );
  await waitFor(() =>
    assert.equal(
      observed.at(-1),
      "unreadable",
      "a failed read is not 'no launch'",
    ),
  );
  assert.equal(
    view.getByRole("button", { name: "Start authoring session" }).disabled,
    true,
  );
  answers.project_team_setup_get_authoring = reservation;
  answers.project_team_setup_get_launch = launch;
  fireEvent.click(view.getByRole("button", { name: "Check authoring status" }));
  await waitFor(() =>
    assert.equal(
      view.getByRole("button", { name: "Retry saved authoring request" })
        .disabled,
      false,
    ),
  );
  assert.equal(view.queryByText("Saved reservation unreadable"), null);
  assert.equal(
    calls.some((call) =>
      /start_authoring|reserve_authoring|provision_coding/.test(call.command),
    ),
    false,
  );
});
