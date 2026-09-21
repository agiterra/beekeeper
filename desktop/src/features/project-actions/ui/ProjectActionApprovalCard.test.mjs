import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { buildHostStepApprovalView } from "../lib/hostStepApproval.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const HASH_A = "aa".repeat(32);
const HASH_B = "bb".repeat(32);
const SHA = "e6".repeat(20);

function view(overrides = {}) {
  return buildHostStepApprovalView({
    request: {
      approvalRef: "ab".repeat(32),
      runId: "11111111-2222-4333-8444-555555555555",
      workflowName: "verify",
      stepId: "verify",
      stepIndex: 0,
      approverSpec: `project-owner:30621:${"3d".repeat(32)}:kettle`,
      message: null,
      expiresAt: null,
    },
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: {
      kind: "resolved",
      hash: HASH_A,
      command: { form: "argv", argv: ["sh", "-c", "a b"] },
    },
    checkout: { state: "commit", sha: SHA },
    ...overrides,
  });
}

async function renderCard(props) {
  const React = (await import("react")).default;
  const { cleanup, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { ProjectActionApprovalCard } = await import(
    "./ProjectActionApprovalCard.tsx"
  );
  // The card publishes a 46030/46031 through a mutation; nothing here
  // triggers one, and a client with retries off keeps that true.
  const client = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  });
  return {
    cleanup,
    ...render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(ProjectActionApprovalCard, {
          authoritySentence: "You may answer this as the project's creator.",
          canApprove: true,
          ...props,
        }),
      ),
    ),
  };
}

test("an argv is shown one argument per line, never space-joined", async () => {
  const card = await renderCard({ view: view() });
  const rows = card
    .getAllByTestId("host-step-approval-argument")
    .map((node) => node.textContent);
  // `["sh","-c","a b"]` must not be able to read as four arguments.
  assert.equal(rows.length, 3);
  assert.ok(rows[2].includes("a b"));
  assert.match(
    card.getByTestId("host-step-approval-command").textContent,
    /\["sh","-c","a b"\]/,
  );
  card.cleanup();
});

test("a bound definition that is not the current one disables both grants", async () => {
  const card = await renderCard({
    view: view({ definition: { kind: "not-current", currentHash: HASH_B } }),
  });
  assert.equal(card.queryAllByTestId("host-step-approval-argument").length, 0);
  assert.match(
    card.getByTestId("host-step-approval-command-unavailable").textContent,
    /no longer the current one/,
  );
  assert.equal(card.getByTestId("host-step-approve-once").disabled, true);
  assert.equal(card.getByTestId("host-step-approve-action").disabled, true);
  // Refusing what you cannot fully see is always a safe answer.
  assert.equal(card.getByTestId("host-step-deny").disabled, false);
  assert.match(
    card.getByTestId("host-step-approval-grant-blocked").textContent,
    /Approve is unavailable/,
  );
  card.cleanup();
});

test("a resolved definition and a known commit enable Approve", async () => {
  const card = await renderCard({ view: view() });
  assert.equal(card.getByTestId("host-step-approve-once").disabled, false);
  assert.equal(card.getByTestId("host-step-approve-action").disabled, false);
  assert.equal(card.queryByTestId("host-step-approval-grant-blocked"), null);
  card.cleanup();
});

test("a viewer who may not approve is offered no control at all", async () => {
  const card = await renderCard({
    view: view(),
    canApprove: false,
    authoritySentence: "Only a project owner may answer this.",
  });
  assert.equal(card.queryByTestId("host-step-approve-once"), null);
  assert.equal(card.queryByTestId("host-step-deny"), null);
  assert.match(
    card.getByTestId("host-step-approval-authority").textContent,
    /Only a project owner/,
  );
  card.cleanup();
});

test("the commit comes from the run, and an unreported one blocks Approve", async () => {
  const bound = await renderCard({ view: view() });
  assert.match(
    bound.getByTestId("host-step-approval-commit").textContent,
    new RegExp(SHA),
  );
  bound.cleanup();

  const asFound = await renderCard({
    view: view({ checkout: { state: "working-directory" } }),
  });
  assert.match(
    asFound.getByTestId("host-step-approval-commit").textContent,
    /working directory as found/,
  );
  assert.equal(asFound.getByTestId("host-step-approve-once").disabled, false);
  asFound.cleanup();

  const unknown = await renderCard({
    view: view({ checkout: { state: "not-reported" } }),
  });
  assert.equal(unknown.getByTestId("host-step-approve-once").disabled, true);
  unknown.cleanup();
});
