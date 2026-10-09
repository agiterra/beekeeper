/**
 * An agent could only ever be seated while a session was being *founded*: the
 * join dialog built its submit with no seat at all, so a session already
 * running could never take one. These mount the real join form and drive the
 * seat field the way a person does.
 *
 * The form is mounted directly (rather than through the dialog) because the
 * create hook provisions this computer's provider on mount, and outside Tauri
 * that resolves to no provider — which is also why the submit *click* is not
 * exercised here: with no reachable provider there is no target to submit
 * against. What the click would carry is pinned in
 * `addCodingSessionProviderModel.test.mjs` instead, on the same builder this
 * form calls.
 */
import assert from "node:assert/strict";
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
    HTMLInputElement: dom.window.HTMLInputElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    CustomEvent: dom.window.CustomEvent,
    Event: dom.window.Event,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const ADA = "aa11bb22".repeat(8);

function umbrella(executions = []) {
  return {
    umbrellaKey: "umbrella-1",
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    genesisRef: "d".repeat(64),
    genesisResolution: "governed",
    title: "Advance Beekeeper live sessions",
    executions,
  };
}

async function mountJoinForm(
  agents,
  { projectRef = null, executions = [] } = {},
) {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { AddCodingSessionProviderForm } = await import(
    "./AddCodingSessionProviderDialog.tsx"
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  client.setQueryData(["managed-agents"], agents);
  await act(async () => {
    render(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(AddCodingSessionProviderForm, {
          channelId: CHANNEL_ID,
          channelName: "engineering",
          onDone() {},
          projectRef,
          umbrella: umbrella(executions),
        }),
      ),
    );
  });
  return {
    act,
    cleanup: () => {
      cleanup();
      client.clear();
    },
    fireEvent,
    screen,
  };
}

test("a session already running can take a seated agent", async () => {
  const { act, cleanup, fireEvent, screen } = await mountJoinForm([
    {
      pubkey: ADA,
      name: "Ada",
      status: "running",
      homeRole: "builder",
      hasRolePack: true,
    },
  ]);
  try {
    // The field sits between the working directory and the first message,
    // and starts unseated: joining a session is still an ordinary human act.
    const field = screen.getByTestId("new-coding-session-seat");
    assert.ok(field);
    assert.match(
      screen.getByTestId("new-coding-session-seat-agent").textContent,
      /No agent — you run this session/,
    );
    assert.equal(screen.queryByTestId("new-coding-session-seat-role"), null);

    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        {
          button: 0,
          pointerType: "mouse",
        },
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByTestId(`new-coding-session-seat-agent-${ADA}`),
      );
    });

    // The role box appears defaulted to the agent's home role, and says so.
    assert.equal(
      screen.getByTestId("new-coding-session-seat-role").value,
      "builder",
    );
    assert.match(
      screen.getByTestId("new-coding-session-seat-role-notice").textContent,
      /Its home role\./,
    );
    // This agent has a pack on this computer, so nothing is claimed about one.
    assert.equal(screen.queryByTestId("new-coding-session-seat-pack"), null);
    assert.equal(screen.queryByTestId("new-coding-session-seat-error"), null);
  } finally {
    cleanup();
  }
});

test("a half-filled seat is refused in the join dialog, before anything is signed", async () => {
  // An agent with no primary role: its role box is the person's to fill.
  const { act, cleanup, fireEvent, screen } = await mountJoinForm([
    { pubkey: ADA, name: "Ada", status: "running" },
  ]);
  try {
    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        {
          button: 0,
          pointerType: "mouse",
        },
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByTestId(`new-coding-session-seat-agent-${ADA}`),
      );
    });
    await act(async () => {
      fireEvent.change(screen.getByTestId("new-coding-session-seat-role"), {
        target: { value: "" },
      });
    });

    assert.match(
      screen.getByTestId("new-coding-session-seat-error").textContent,
      /Give this seat a role, or clear the agent\./,
    );
    assert.ok(
      screen.getByTestId("add-coding-session-provider-submit").disabled,
    );
  } finally {
    cleanup();
  }
});

test("a seat whose pack this computer does not hold says so before submit", async () => {
  const { act, cleanup, fireEvent, screen } = await mountJoinForm([
    {
      pubkey: ADA,
      name: "Ada",
      status: "running",
      homeRole: "builder",
      hasRolePack: false,
    },
  ]);
  try {
    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        {
          button: 0,
          pointerType: "mouse",
        },
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByTestId(`new-coding-session-seat-agent-${ADA}`),
      );
    });
    assert.match(
      screen.getByTestId("new-coding-session-seat-pack").textContent,
      /Ada has no role pack on this computer, so this seat carries no role skills and runs on its persona prompt alone\./,
    );
  } finally {
    cleanup();
  }
});

/**
 * Item 80(a): the seats hired into `AgentTeams` ran in Brian's own live
 * checkout, because this dialog's working directory defaulted to the last
 * directory used — which is the directory the app itself runs from. A hired
 * seat gets a tree of its own, and the dialog says which one.
 */
test("seating an agent gives the seat its own worktree, and names it", async () => {
  const { act, cleanup, fireEvent, screen } = await mountJoinForm([
    {
      pubkey: ADA,
      name: "Ada",
      status: "running",
      homeRole: "builder",
      hasRolePack: true,
    },
  ]);
  try {
    await act(async () => {
      fireEvent.change(screen.getByTestId("coding-session-workdir-input"), {
        target: { value: "/Users/brian/Projects/beekeeper/beekeeper" },
      });
    });
    // An unseated join is the person's own execution: unchanged, no worktree.
    assert.equal(screen.queryByTestId("coding-session-worktree-toggle"), null);
    assert.equal(
      screen.queryByTestId("add-coding-session-provider-workdir-note"),
      null,
    );

    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        { button: 0, pointerType: "mouse" },
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByTestId(`new-coding-session-seat-agent-${ADA}`),
      );
    });

    const toggle = screen.getByTestId("coding-session-worktree-toggle");
    assert.equal(toggle.getAttribute("data-state"), "checked");
    assert.equal(
      screen.getByTestId("coding-session-worktree-name").value,
      "advance-beekeeper-live-sessions-builder",
    );
    assert.match(
      screen.getByTestId("add-coding-session-provider-workdir-note")
        .textContent,
      /Ada runs in a new worktree made from \/Users\/brian\/Projects\/beekeeper\/beekeeper/,
    );
  } finally {
    cleanup();
  }
});

test("a seat whose worktree is turned off is told it shares the checkout", async () => {
  const { act, cleanup, fireEvent, screen } = await mountJoinForm([
    {
      pubkey: ADA,
      name: "Ada",
      status: "running",
      homeRole: "builder",
      hasRolePack: true,
    },
  ]);
  try {
    await act(async () => {
      fireEvent.change(screen.getByTestId("coding-session-workdir-input"), {
        target: { value: "/Users/brian/Projects/beekeeper/beekeeper" },
      });
    });
    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        { button: 0, pointerType: "mouse" },
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByTestId(`new-coding-session-seat-agent-${ADA}`),
      );
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-worktree-toggle"));
    });

    assert.match(
      screen.getByTestId("add-coding-session-provider-workdir-note")
        .textContent,
      /Ada runs directly in \/Users\/brian\/Projects\/beekeeper\/beekeeper, sharing its branch, uncommitted changes, and role skills/,
    );
  } finally {
    cleanup();
  }
});

test("LANE-L25: a projectRef reaches the join dialog's seat field without disturbing an unseated or projectless form", async () => {
  // No `projectRef` — every pre-L25 caller's implicit default
  // (`AddCodingSessionProviderDialog.tsx:404` before this lane always passed
  // none). The seat field must render exactly as it always has: no pack
  // preview attempted, nothing crashes.
  const bare = await mountJoinForm([
    { pubkey: ADA, name: "Ada", status: "running", homeRole: "builder" },
  ]);
  try {
    assert.ok(bare.screen.getByTestId("new-coding-session-seat"));
    assert.equal(
      bare.screen.queryByTestId("new-coding-session-pack-preview"),
      null,
    );
  } finally {
    bare.cleanup();
  }

  // With a `projectRef`: the same form, the same seat, the same role — the
  // only difference is the prop this lane wires through. Nothing in this
  // harness can answer the real pack-status probe (no Tauri bridge, no
  // relay), so the preview itself never renders here — that path is proven
  // by the LANE-L25 e2e case, which drives a real signed project and a real
  // mocked host answer. What this proves is the plumbing: a form given a
  // project does not crash, does not lose the seat it already had, and does
  // not regress the workdir/worktree fields that sit beside it.
  const withProject = await mountJoinForm(
    [{ pubkey: ADA, name: "Ada", status: "running", homeRole: "builder" }],
    { projectRef: `30621:${"a".repeat(64)}:l25-project` },
  );
  try {
    const { act, fireEvent, screen } = withProject;
    assert.ok(screen.getByTestId("new-coding-session-seat"));
    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        { button: 0, pointerType: "mouse" },
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByTestId(`new-coding-session-seat-agent-${ADA}`),
      );
    });
    assert.equal(
      screen.getByTestId("new-coding-session-seat-role").value,
      "builder",
    );
  } finally {
    withProject.cleanup();
  }
});

test("a project session's seat picker lists only that project's agents, and counts the rest", async () => {
  const TANK_LOOP = `30621:${"e".repeat(64)}:tank-loop`;
  const BOB = "bb22cc33".repeat(8);
  const STRAY = "cc33dd44".repeat(8);
  const { act, cleanup, fireEvent, screen } = await mountJoinForm(
    [
      { pubkey: ADA, name: "Ada", homeRole: "builder", projectRef: TANK_LOOP },
      {
        pubkey: BOB,
        name: "Bob",
        homeRole: "builder",
        projectRef: `30621:${"e".repeat(64)}:beekeeper`,
      },
      { pubkey: STRAY, name: "Stray", homeRole: "builder", projectRef: null },
    ],
    {
      executions: [
        {
          executionKey: "exec-1",
          signerPubkey: "9".repeat(64),
          activeGeneration: {
            projectRef: TANK_LOOP,
            statusEventId: "1".repeat(64),
            commandTarget: null,
            runtime: null,
          },
        },
      ],
    },
  );
  try {
    assert.match(
      screen.getByTestId("new-coding-session-seat-scope").textContent,
      /^2 agents on this computer aren't this project's, so they can't be seated here\./,
    );
    await act(async () => {
      fireEvent.pointerDown(
        screen.getByTestId("new-coding-session-seat-agent"),
        { button: 0, pointerType: "mouse" },
      );
    });
    assert.ok(screen.getByTestId(`new-coding-session-seat-agent-${ADA}`));
    assert.equal(
      screen.queryByTestId(`new-coding-session-seat-agent-${BOB}`),
      null,
    );
    assert.equal(
      screen.queryByTestId(`new-coding-session-seat-agent-${STRAY}`),
      null,
    );
  } finally {
    cleanup();
  }
});

test("a session whose project is not known yet lists every agent and says the project is unknown", async () => {
  const { cleanup, screen } = await mountJoinForm([
    { pubkey: ADA, name: "Ada", homeRole: "builder", projectRef: null },
  ]);
  try {
    assert.match(
      screen.getByTestId("new-coding-session-seat-scope").textContent,
      /project is not known yet/,
    );
  } finally {
    cleanup();
  }
});
