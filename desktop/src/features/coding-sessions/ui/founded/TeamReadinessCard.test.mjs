/**
 * The readiness panel's words, and the one click that records a checkout.
 *
 * Ledger 135(c): with "Use roles" ticked the founding form blocked on
 * `ROLE_PACKS_MISSING` ("Remedy: Restore personas/roles") and
 * `CHECKOUT_NOT_RECORDED`, and the operator's way out was to untick the box.
 * Two things had to change: the panel had to say what a blocker means in
 * words a person acts on, and the checkout it was blocking on had to be
 * recordable from where the folder was already typed.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

/** Every host call this panel makes, newest last. */
const invocations = [];
/** Answers keyed by command; a missing one rejects, like an older host. */
let answers = {};

const tauriInternals = {
  invoke: (command, payload) => {
    invocations.push([command, payload]);
    if (command in answers) return Promise.resolve(answers[command]);
    return Promise.reject(new Error(`no host answer for ${command}`));
  },
  transformCallback: () => Math.random(),
};

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  invocations.length = 0;
  answers = {};
});

after(() => dom.window.close());

const PROJECT_REF = `30621:${"a".repeat(64)}:tank-loop`;

function fact(code, overrides = {}) {
  return {
    category: "project",
    code,
    scope: "local",
    state: "blocked",
    summary: `${code} summary`,
    remedy: `${code} remedy`,
    ...overrides,
  };
}

function readiness(facts) {
  return {
    schemaVersion: 3,
    projectRef: PROJECT_REF,
    generatedAt: "2026-09-16T12:00:00Z",
    readyForFirstSession: false,
    ready: false,
    status: "blocked",
    hostClass: "cold",
    keyStoreSafe: true,
    source: {
      appCommit: null,
      appSourceDirty: null,
      checkoutPath: null,
      checkoutCommit: null,
      checkoutDirty: null,
    },
    ownerPubkey: null,
    team: {
      selectedRoles: [],
      availableRoles: [],
      packs: [],
      identities: [],
      packsDigest: null,
      packsRevision: null,
    },
    runtimes: [],
    registry: { providerTargets: [], pendingTargets: [] },
    provider: { provisioned: false, process: "unknown" },
    policy: {},
    relay: { state: "awaiting_first_session", reachable: null, source: "wire" },
    catalog: {
      state: "awaiting_first_session",
      revision: null,
      targets: [],
      source: "wire",
      provenance: [],
    },
    facts,
    blockingCodes: facts
      .filter((entry) => entry.state === "blocked")
      .map((entry) => entry.code),
    unknownCodes: [],
    awaitingCodes: [],
    limitedCodes: [],
  };
}

async function mount(props = {}) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { TeamReadinessCard } = await import("../TeamReadinessCard.tsx");
  await act(async () => {
    render(
      React.createElement(TeamReadinessCard, {
        readiness: readiness([fact("CHECKOUT_NOT_RECORDED")]),
        loading: false,
        readError: null,
        selectedRoles: [],
        scan: null,
        names: {},
        onNameChange: () => {},
        onBeginPrepare: () => {},
        onConfirmPrepare: () => {},
        onCancelPrepare: () => {},
        scanning: false,
        preparing: false,
        prepareSteps: [],
        prepareError: null,
        prepareWarning: null,
        externalBusy: false,
        runtimeTarget: null,
        ...props,
      }),
    );
  });
  return {
    text: () => document.body.textContent ?? "",
    query: (testid) => document.querySelector(`[data-testid="${testid}"]`),
    click: async (testid) => {
      const element = document.querySelector(`[data-testid="${testid}"]`);
      assert.ok(element, `${testid} must be rendered`);
      await act(async () => {
        element.click();
      });
    },
  };
}

test("a blocker is explained in words before it is named by its code", async () => {
  const view = await mount({
    readiness: readiness([fact("ROLE_PACKS_MISSING", { category: "team" })]),
  });
  assert.match(view.text(), /Nothing says what this project's roles are/);
  assert.match(view.text(), /packs repository the project names/);
  // The host's own fact is still printed underneath: this narrows nothing.
  assert.match(view.text(), /ROLE_PACKS_MISSING summary/);
  assert.match(view.text(), /Remedy: ROLE_PACKS_MISSING remedy/);
});

test("a code with no translation still renders exactly as it did", async () => {
  const view = await mount({
    readiness: readiness([fact("SOME_CODE_NOBODY_TRANSLATED")]),
  });
  assert.match(view.text(), /SOME_CODE_NOBODY_TRANSLATED summary/);
  assert.match(view.text(), /Remedy: SOME_CODE_NOBODY_TRANSLATED remedy/);
});

test("the checkout blocker offers the folder the form already has", async () => {
  answers = {
    list_coding_session_worktree_branches: {
      branches: ["main"],
      defaultBranch: "main",
      headBranch: "main",
    },
    set_coding_session_workdir: { version: 2 },
  };
  let recorded = 0;
  const view = await mount({
    candidateCheckout: "/Users/brian/Projects/TankLoop",
    projectRef: PROJECT_REF,
    projectLabel: "Tank Loop",
    onCheckoutRecorded: () => {
      recorded += 1;
    },
  });
  const button = view.query("team-readiness-use-this-folder");
  assert.ok(button, "a git checkout in the form is offered in one click");
  assert.match(button.textContent ?? "", /Use this folder as Tank Loop’s/);

  await view.click("team-readiness-use-this-folder");
  const write = invocations.find(
    ([command]) => command === "set_coding_session_workdir",
  );
  assert.deepEqual(write?.[1], {
    scope: "project",
    key: PROJECT_REF,
    path: "/Users/brian/Projects/TankLoop",
  });
  assert.equal(recorded, 1, "readiness is read again after the record");
});

test("a folder that is not a checkout is said so, never offered", async () => {
  answers = {
    list_coding_session_worktree_branches: {
      branches: [],
      defaultBranch: null,
      headBranch: null,
    },
  };
  const view = await mount({
    candidateCheckout: "/Users/brian/Documents",
    projectRef: PROJECT_REF,
    projectLabel: "Tank Loop",
  });
  assert.equal(view.query("team-readiness-use-this-folder"), null);
  assert.ok(view.query("team-readiness-folder-not-a-checkout"));
  assert.match(view.text(), /is not a git checkout/);
});

test("a host that cannot answer neither offers nor accuses", async () => {
  // `invoke` rejects for every command here, which is what an older host and
  // the mock bridge both do. An offer is a claim; so is a refusal.
  const view = await mount({
    candidateCheckout: "/Users/brian/Projects/TankLoop",
    projectRef: PROJECT_REF,
  });
  assert.equal(view.query("team-readiness-use-this-folder"), null);
  assert.equal(view.query("team-readiness-folder-not-a-checkout"), null);
});

test("with no folder in the form there is nothing to offer", async () => {
  const view = await mount({
    candidateCheckout: null,
    projectRef: PROJECT_REF,
  });
  assert.equal(view.query("team-readiness-use-this-folder"), null);
  assert.equal(
    invocations.filter(
      ([command]) => command === "list_coding_session_worktree_branches",
    ).length,
    0,
    "no folder means no question asked of the host",
  );
});
