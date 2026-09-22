import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

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

const SEQUENCES = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../../../../conformance/project-work/fixtures/sequences",
);

function fold(name) {
  return JSON.parse(
    readFileSync(resolve(SEQUENCES, name, "expected-fold.json"), "utf8"),
  );
}

function response(name, unreadablePlans = []) {
  return {
    schema: "buzz-project-work-response/v1",
    implementation: "buzz-core",
    coverage: fold(name),
    unreadablePlans,
    agentsRepoRead: unreadablePlans.length === 0,
  };
}

async function renderCoverage(props) {
  const React = (await import("react")).default;
  const { cleanup, render } = await import("@testing-library/react");
  const { ProjectWorkCoverage } = await import("./ProjectWorkCoverage.tsx");
  return {
    cleanup,
    ...render(React.createElement(ProjectWorkCoverage, props)),
  };
}

test("no fold is 'unknown', never 'nothing remains'", async () => {
  const view = await renderCoverage({ response: null });
  assert.match(
    view.getByTestId("project-work-unknown").textContent,
    /Work coverage unknown/,
  );
  assert.equal(view.queryByTestId("project-work-coverage"), null);
  view.cleanup();
});

test("a failed read is the failure, not an empty contract", async () => {
  const view = await renderCoverage({
    response: null,
    errorMessage: "relay said 403",
  });
  assert.match(view.getByTestId("project-work-error").textContent, /403/);
  view.cleanup();
});

test("the happy path shows the contract, every criterion and completeness", async () => {
  const view = await renderCoverage({ response: response("happy-path") });
  const declaration = view.getByTestId("project-work-declaration");
  assert.equal(declaration.dataset.state, "head");
  assert.match(declaration.textContent, /plans\/kettle\.md @/);
  const criteria = view.getAllByTestId("project-work-criterion");
  assert.ok(criteria.length > 0);
  assert.ok(criteria.every((node) => node.dataset.status.length > 0));
  assert.match(
    view.getByTestId("project-work-coverage-complete").textContent,
    /coverage complete/,
  );
  view.cleanup();
});

test("the mission state is a separate row and is never merged in", async () => {
  const view = await renderCoverage({
    response: response("happy-path"),
    missionRow: "terminal — completed abc123",
  });
  const row = view.getByTestId("project-work-mission-row");
  assert.match(row.textContent, /mission : terminal — completed abc123/);
  // The coverage answer is its own element; nothing combines the two.
  assert.ok(
    !view
      .getByTestId("project-work-coverage-complete")
      .textContent.includes("terminal"),
  );
  view.cleanup();
});

test("an absent mission fold is unknown on its own row", async () => {
  const view = await renderCoverage({ response: response("happy-path") });
  assert.match(
    view.getByTestId("project-work-mission-row").textContent,
    /unknown — the 44244 fold has not answered/,
  );
  view.cleanup();
});

test("a fork renders as a conflict with the fold's own sentence", async () => {
  const view = await renderCoverage({ response: response("fork") });
  const conflict = view.getByTestId("project-work-conflict");
  assert.match(conflict.textContent, /maximal declarations/);
  assert.match(
    view.getByTestId("project-work-next").textContent,
    /Resolve the fork/,
  );
  view.cleanup();
});

test("an unread plan says its criteria are unknown, not open", async () => {
  const view = await renderCoverage({
    response: response("happy-path", [
      {
        repository: "30617:aa:agents",
        commit: "ab".repeat(20),
        path: "plans/kettle.md",
        reasonCode: "plan_unreadable",
        reason: "fatal: path does not exist",
      },
    ]),
  });
  assert.match(
    view.getByTestId("project-work-unreadable-plan").textContent,
    /plan_unreadable: fatal: path does not exist/,
  );
  view.cleanup();
});

test("a criterion with no readable plan says its proof is unknown", async () => {
  // A7.4: an unreadable plan still projects the criteria bindings named, with
  // `proof: null`. The row must say so rather than dereference it.
  const view = await renderCoverage({
    response: response("plan-unreadable-after-bindings"),
  });
  const rows = view.container.querySelectorAll(
    "[data-testid='project-work-criterion']",
  );
  assert.ok(rows.length > 0, "the unreadable plan still projects its rows");
  for (const row of rows) {
    assert.match(row.textContent, /proof unknown: the plan could not be read/);
    assert.equal(row.getAttribute("data-status"), "unknown");
  }
  view.cleanup();
});

test("a drifted plan shows both commits and the re-adopt command", async () => {
  // A10: disclosure, never enforcement. The row says the agents repository
  // moved on and offers the command; it never says the plan file changed and
  // never marks the work stale.
  const view = await renderCoverage({
    response: response("plan-drift-drifted"),
    scope: {
      channelRef: "22222222-3333-4444-8555-666666666666",
      sessionRef: "11111111-2222-4333-8444-555555555555",
    },
  });
  const notice = view.getByTestId("project-work-plan-drift").textContent;
  // The lead line names what moved — the agents repository's branch — and
  // never the plan: "plan moved" states the thing branch-tip ref state
  // cannot observe, and the prefix is the part that survives a narrow row.
  assert.match(
    notice,
    /agents repo main moved on: [0-9a-f]{12}…→[0-9a-f]{12}…/,
  );
  assert.ok(!/plan moved/.test(notice), notice);
  assert.match(notice, /the plan this work is judged against did not change/);
  assert.match(notice, /bee sessions work adopt .*--supersedes /);
  assert.ok(!/plan file changed/.test(notice), notice);
  view.cleanup();
});

test("a plan that did not move shows no drift notice", async () => {
  const view = await renderCoverage({ response: response("plan-drift-none") });
  assert.equal(
    view.container.querySelector("[data-testid='project-work-plan-drift']"),
    null,
  );
  view.cleanup();
});

test("at most one next step is offered, and it names what releases it", async () => {
  for (const name of ["fork", "mixed-artifacts", "amendment"]) {
    const view = await renderCoverage({ response: response(name) });
    assert.ok(
      view.queryAllByTestId("project-work-next").length <= 1,
      `${name} offers at most one next step`,
    );
    const next = view.queryByTestId("project-work-next");
    if (next) assert.match(next.textContent, /released by /);
    view.cleanup();
  }
});
