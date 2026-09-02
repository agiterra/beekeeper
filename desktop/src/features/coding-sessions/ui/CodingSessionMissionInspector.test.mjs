import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { deriveCodingSessionMissionInspectorModel } from "../lib/codingSessionMissionInspectorModel.ts";

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

function input(overrides = {}) {
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    seatPlans: [],
    reports: [],
    observedChanges: { files: [], unreportedEditCount: 0 },
    participants: [],
    contextLoads: new Map(),
    missionState: { kind: "unknown", detail: null },
    usage: null,
    rejectedEventCount: 0,
    rejectionsTruncated: false,
    rejectedReasons: [],
    conflicts: [],
    ...overrides,
  };
}

function richModel() {
  return deriveCodingSessionMissionInspectorModel(
    input({
      goal: {
        kind: "available",
        sourceEventId: "goal-event",
        authorLabel: "Helios · Lead",
        text: "Ship an honest Mission inspector.",
      },
      acceptedPlan: {
        kind: "available",
        steps: [
          {
            text: "Build the inspector",
            sourceEventId: "accepted-plan-event",
            authorLabel: "Helios · Lead",
            sourceCreatedAt: 1,
            sourceIndex: 0,
          },
          {
            text: "Run focused tests",
            sourceEventId: "accepted-plan-event",
            authorLabel: "Helios · Lead",
            sourceCreatedAt: 1,
            sourceIndex: 1,
          },
        ],
      },
      assignments: [
        {
          sourceEventId: "accepted-plan-event",
          authorLabel: "Helios · Lead",
          assigneeRole: "builder",
          objective: "Implement the signed transaction projection.",
          brief: "Preserve the signed governance chain after completion.",
          fileOwnership: ["desktop/src/inspector.tsx"],
        },
      ],
      seatPlans: [
        {
          executionKey: "builder",
          ownerLabel: "Bob · Builder",
          model: {
            sourceItemId: "seat-plan-event",
            turnId: "turn-1",
            timestamp: "2026-08-30T16:00:00.000Z",
            tasks: [
              { id: "task", text: "Render typed facts", status: "in_progress" },
            ],
            completedCount: 0,
            explanation: null,
            copyText: null,
            state: "active",
          },
        },
      ],
      reports: [
        {
          sourceEventId: "report-event",
          authorLabel: "Bob · Builder",
          summary: "Foundation ready for mounting.",
          assignmentRef: "assignment-event",
          branch: "singularity-stream",
          baseSha: "3001e46a",
          headSha: "860a0af7",
          files: ["desktop/src/inspector.tsx"],
          tests: [
            {
              name: "DOM fixture",
              command: "pnpm test inspector",
              outcome: "passed",
              evidence: "keyboard focus verified",
            },
          ],
        },
      ],
      observedChanges: {
        files: [
          {
            path: "desktop/src/inspector.tsx",
            filename: "inspector.tsx",
            additions: 24,
            deletions: 2,
            diffs: [],
            editCount: 2,
          },
        ],
        unreportedEditCount: 0,
      },
      observedFileSources: new Map([
        ["desktop/src/inspector.tsx", ["observed-edit-event"]],
      ]),
      participants: [
        {
          executionKey: "lead",
          label: "Helios · Lead",
          secondaryLabel: "Codex · gpt-5.6-sol",
          role: "lead",
          status: { kind: "idle", label: "Idle" },
          disposition: "idle",
          activity: null,
          lastTurnLabel: "last turn 1m ago",
        },
        {
          executionKey: "builder",
          label: "Bob · Builder",
          secondaryLabel: "Claude Code · sonnet",
          role: "builder",
          status: { kind: "working", label: "Working" },
          disposition: "live",
          activity: "Render typed facts",
          lastTurnLabel: "last turn just now",
        },
      ],
      contextLoads: new Map([
        ["lead", null],
        ["builder", { usedTokens: 1000, contextWindow: 10000, pct: 10 }],
      ]),
      missionState: {
        kind: "running",
        sourceEventId: "state-event",
        phase: "assigned",
        detail: "The canonical fold contains active assignment work.",
        canonicalChain: [
          {
            type: "assignment",
            sourceEventId: "state-event",
            authorPubkey: "a".repeat(64),
            createdAt: 1,
            summary: "Build the Mission surface.",
          },
        ],
      },
      usage: {
        sourceEventId: "terminal-event",
        inputTokens: 1000,
        outputTokens: 250,
        totalTokens: 1250,
        toolCalls: 5,
        costUsd: 0.12,
      },
    }),
  );
}

async function renderInspector(props) {
  const React = (await import("react")).default;
  const { cleanup, render } = await import("@testing-library/react");
  const { CodingSessionMissionInspector } = await import(
    "./CodingSessionMissionInspector.tsx"
  );
  return {
    cleanup,
    ...render(React.createElement(CodingSessionMissionInspector, props)),
  };
}

async function renderContext(props) {
  const React = (await import("react")).default;
  const { cleanup, render } = await import("@testing-library/react");
  const { CodingSessionMissionContext } = await import(
    "./CodingSessionMissionContext.tsx"
  );
  return {
    cleanup,
    ...render(React.createElement(CodingSessionMissionContext, props)),
  };
}

test("panel and drawer variants expose the same focused Inspector sections", async () => {
  for (const variant of ["panel", "drawer"]) {
    const view = await renderInspector({
      model: richModel(),
      variant,
      focusedExecutionKey: null,
    });
    try {
      const inspector = view.getByRole("complementary", {
        name: "Mission inspector",
      });
      assert.equal(inspector.dataset.variant, variant);
      assert.equal(view.queryByRole("heading", { name: "Inspector" }), null);
      for (const heading of [
        "Current goal",
        "Mission state",
        "Accepted plan",
        "Seat-reported plans",
        "Changes",
        "Files",
        "Structured tests",
        "Team",
        "Reports",
        "Integrity",
      ]) {
        assert.ok(view.getByRole("heading", { name: heading }));
      }
      assert.equal(view.getAllByText("Accepted from Helios · Lead").length, 2);
      assert.ok(view.getByText("Ship an honest Mission inspector."));
      assert.ok(view.getByLabelText("Bob · Builder seat-reported plan"));
      assert.ok(
        view.getByText(
          "Observed file edit · Reported by Bob · Builder · 2 edits",
        ),
      );
      assert.ok(view.getByText("keyboard focus verified"));
      assert.match(
        view.getByRole("button", { name: /Focus Bob · Builder/ }).textContent,
        /live/,
      );
      assert.doesNotMatch(inspector.textContent, /context\s+1000\/10000/i);
      assert.equal(view.queryByText("Terminal usage not reported."), null);
    } finally {
      view.cleanup();
    }
  }
});

test("Context is one separate panel with seat, signed work, and usage facts", async () => {
  for (const variant of ["panel", "drawer"]) {
    const view = await renderContext({ model: richModel(), variant });
    try {
      const context = view.getByRole("complementary", {
        name: "Mission context",
      });
      assert.equal(context.dataset.variant, variant);
      assert.equal(view.queryByRole("heading", { name: "Context" }), null);
      for (const heading of [
        "Context load",
        "Signed work context",
        "Terminal usage",
      ]) {
        assert.ok(view.getByRole("heading", { name: heading }));
      }
      assert.ok(view.getByText("Bob · Builder"));
      assert.ok(view.getByText("1000/10000 (10%)"));
      assert.ok(view.getByText("Implement the signed transaction projection."));
      assert.ok(
        view.getByText(
          "Preserve the signed governance chain after completion.",
        ),
      );
      assert.ok(view.getByText("Ownership · Helios · Lead"));
      assert.ok(view.getByText("singularity-stream"));
      assert.ok(view.getByText("1,250"));
      assert.ok(view.getAllByText("report-event").length > 0);
      assert.ok(view.getByText("terminal-event"));
    } finally {
      view.cleanup();
    }
  }
});

test("Context keeps full signed identifiers behind their disclosure", async () => {
  const author = "a".repeat(64);
  const eventId = "b".repeat(64);
  const rawValue = "c".repeat(64);
  const model = deriveCodingSessionMissionInspectorModel(
    input({
      assignments: [
        {
          sourceEventId: eventId,
          authorLabel: author,
          assigneeRole: "builder",
          objective: "Keep provenance readable.",
          brief: "Reveal complete identifiers on demand.",
          fileOwnership: ["desktop/src/context.tsx"],
        },
      ],
      reports: [
        {
          sourceEventId: "report-event",
          authorLabel: "Bob · Builder",
          summary: "Keep this prose unchanged.",
          assignmentRef: rawValue,
          branch: "portable-team-loop",
          baseSha: "3001e46a",
          headSha: "860a0af7",
          files: ["desktop/src/context.tsx"],
          tests: [],
        },
      ],
    }),
  );
  const view = await renderContext({ model, variant: "panel" });
  try {
    const context = view.getByRole("complementary", {
      name: "Mission context",
    });
    const visibleLabels = [...context.querySelectorAll("dt")].map(
      (label) => label.textContent,
    );
    assert.ok(visibleLabels.includes("Ownership · aaaaaaaa…aaaaaa"));
    assert.equal(
      visibleLabels.some((label) => label?.includes(author)),
      false,
    );

    const rawAssignmentLabel = [...context.querySelectorAll("dt")].find(
      (label) => label.textContent === "Assignment · Bob · Builder",
    );
    assert.ok(rawAssignmentLabel);
    const rawAssignmentFact = rawAssignmentLabel.parentElement;
    assert.equal(
      rawAssignmentFact.querySelector(":scope > dd > code").textContent,
      "cccccccc…cccccc",
    );
    assert.equal(
      context.textContent.includes("Keep provenance readable."),
      true,
    );
    assert.equal(context.textContent.includes("desktop/src/context.tsx"), true);
    assert.equal(context.textContent.includes("portable-team-loop"), true);
    assert.equal(context.textContent.includes("3001e46a"), true);

    const rawValueDisclosure = rawAssignmentFact.querySelector("details");
    assert.ok(rawValueDisclosure);
    assert.equal(rawValueDisclosure.open, false);
    assert.ok(rawValueDisclosure.textContent.includes(rawValue));
    rawValueDisclosure.querySelector("summary").click();
    assert.equal(rawValueDisclosure.open, true);
    assert.ok(rawValueDisclosure.textContent.includes(rawValue));

    const disclosure = [...context.querySelectorAll("details")].find(
      (details) =>
        details.textContent.includes(author) &&
        details.textContent.includes(eventId),
    );
    assert.ok(disclosure);
    assert.equal(disclosure.open, false);
    disclosure.querySelector("summary").click();
    assert.equal(disclosure.open, true);
    assert.ok(disclosure.textContent.includes(author));
    assert.ok(disclosure.textContent.includes(eventId));
  } finally {
    view.cleanup();
  }
});

test("accepted-plan steps render their own signed source and author", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({
        acceptedPlan: {
          kind: "available",
          steps: [
            {
              text: "Verifier criterion",
              sourceEventId: "assignment-b",
              authorLabel: "Parallax · Verifier",
              sourceCreatedAt: 123,
              sourceIndex: 7,
            },
            {
              text: "Builder criterion",
              sourceEventId: "assignment-a",
              authorLabel: "Bob · Builder",
              sourceCreatedAt: 10,
              sourceIndex: 0,
            },
          ],
        },
      }),
    ),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(view.getByText("Builder criterion"));
    assert.ok(view.getByText("Accepted from Bob · Builder"));
    assert.ok(view.getByText("assignment-a"));
    assert.ok(view.getByText("Verifier criterion"));
    assert.ok(view.getByText("Accepted from Parallax · Verifier"));
    assert.ok(view.getByText("assignment-b"));
    assert.ok(view.getByText(/Signed criterion source index 7/));
    assert.equal(
      view.container
        .querySelector('time[datetime="1970-01-01T00:02:03.000Z"]')
        ?.getAttribute("datetime"),
      "1970-01-01T00:02:03.000Z",
    );
  } finally {
    view.cleanup();
  }
});

test("accepted-plan raw author keys stay compact until signed-source disclosure", async () => {
  const author = "a".repeat(64);
  const eventId = "b".repeat(64);
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({
        acceptedPlan: {
          kind: "available",
          steps: [
            {
              text: "Keep provenance readable",
              sourceEventId: eventId,
              authorLabel: author,
              sourceCreatedAt: 10,
              sourceIndex: 0,
            },
          ],
        },
      }),
    ),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(view.getByText("Accepted from aaaaaaaa…aaaaaa"));
    const disclosure = view.container.querySelector("details");
    assert.ok(disclosure);
    assert.equal(disclosure.open, false);
    assert.equal(disclosure.textContent.includes(author), true);
    assert.equal(disclosure.textContent.includes(eventId), true);
    disclosure.querySelector("summary").click();
    assert.equal(disclosure.open, true);
    assert.equal(disclosure.querySelectorAll("code")[0].textContent, author);
    assert.equal(disclosure.querySelectorAll("code")[1].textContent, eventId);
  } finally {
    view.cleanup();
  }
});

test("empty and unknown states disclose absence rather than rendering zeros", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(input()),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.match(
      view.getByTestId("mission-state-summary").textContent,
      /Mission state unknown/,
    );
    assert.ok(view.getByText("No accepted mission goal published."));
    assert.ok(view.getByText("No accepted plan published"));
    assert.ok(view.getByText("No seat has published a signed plan."));
    assert.ok(view.getByText("No signed edit activity observed."));
    // DESIGN-SPEC §8 / SURFACES D5: the frozen unknown copy, headline plus the
    // sentence that says why nothing is counted.
    assert.ok(view.getByText("No test report yet"));
    assert.match(
      view.container.textContent,
      /Nothing on the wire reports tests\./,
    );
    assert.equal(
      view.queryByText("No structured test results published."),
      null,
    );
    assert.ok(view.getByText("No signed session seats projected."));
    assert.equal(view.queryByText("Terminal usage not reported."), null);
    assert.ok(
      view.getByText("No rejected or conflicting transaction records."),
    );
    assert.doesNotMatch(
      view.container.textContent,
      /0 tokens|0 files|completed/i,
    );
  } finally {
    view.cleanup();
  }
});

test("loading and fold errors remain explicit and retryable", async () => {
  let refreshes = 0;
  const view = await renderInspector({
    errorMessage: "Canonical Mission fold failed.",
    focusedExecutionKey: null,
    loading: true,
    model: deriveCodingSessionMissionInspectorModel(input()),
    onRefresh: () => {
      refreshes += 1;
    },
    variant: "panel",
  });
  try {
    assert.ok(view.getByRole("status").textContent.includes("Loading signed"));
    assert.equal(
      view
        .getByRole("alert")
        .textContent.includes("Canonical Mission fold failed."),
      true,
    );
    view.getByRole("button", { name: "Retry signed evidence" }).click();
    assert.equal(refreshes, 1);
  } finally {
    view.cleanup();
  }
});

test("conflicting and rejected records are visible with their signed sources", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({
        acceptedPlan: { kind: "conflict", eventIds: ["plan-a", "plan-b"] },
        missionState: { kind: "conflict", eventIds: ["done", "blocked"] },
        rejectedEventCount: 1,
        rejectedReasons: [
          {
            code: "invalid-ref",
            summary: "Unknown causal reference",
            eventIds: ["bad-event"],
          },
        ],
        conflicts: [
          {
            code: "terminal",
            summary: "Terminal records disagree",
            eventIds: ["done", "blocked"],
          },
        ],
      }),
    ),
    variant: "drawer",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(
      view.getByText("Accepted plan unavailable — conflicting signed records"),
    );
    assert.match(
      view.getByTestId("mission-state-summary").textContent,
      /Mission state conflict/,
    );
    assert.ok(view.getByText("1 rejected event"));
    assert.ok(view.getByText(/Unknown causal reference/));
    assert.ok(view.getByText(/Terminal records disagree/));
    for (const eventId of [
      "plan-a",
      "plan-b",
      "done",
      "blocked",
      "bad-event",
    ]) {
      assert.ok(view.getAllByText(eventId).length > 0);
    }
  } finally {
    view.cleanup();
  }
});

test("bounded rejection overflow renders an unknown total without false arithmetic", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({
        rejectedEventCount: null,
        rejectionsTruncated: true,
        rejectedReasons: Array.from({ length: 100 }, (_, index) => ({
          code: `invalid-${index}`,
          summary: `Rejected event ${index}`,
          eventIds: [`event-${index}`],
        })),
      }),
    ),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(
      view.getByText("Rejected event total unavailable after the safety bound"),
    );
    assert.ok(
      view.getByText(
        "Showing 100 rejected events; additional unique count unavailable after the safety bound.",
      ),
    );
    assert.doesNotMatch(view.container.textContent, /total=|omitted=/);
  } finally {
    view.cleanup();
  }
});

test("team focus uses labeled native buttons and remains keyboard reachable", async () => {
  const focused = [];
  const view = await renderInspector({
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
    onFocusParticipant(executionKey) {
      focused.push(executionKey);
    },
  });
  try {
    const button = view.getByRole("button", { name: /Focus Bob · Builder/ });
    assert.equal(button.tagName, "BUTTON");
    assert.equal(button.getAttribute("type"), "button");
    button.focus();
    assert.equal(document.activeElement, button);
    button.click();
    assert.deepEqual(focused, ["builder"]);

    const sourceSummaries = view.container.querySelectorAll("summary");
    assert.ok(sourceSummaries.length > 0);
    sourceSummaries[0].focus();
    assert.equal(document.activeElement, sourceSummaries[0]);
  } finally {
    view.cleanup();
  }
});

test("inspector source uses only named rem-safe text utilities", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(
    new URL("./CodingSessionMissionInspector.tsx", import.meta.url),
    "utf8",
  );
  const contextSource = await readFile(
    new URL("./CodingSessionMissionContext.tsx", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(source, /text-\[[^\]]+\]/);
  assert.doesNotMatch(contextSource, /text-\[[^\]]+\]/);
  assert.doesNotMatch(source, /#[0-9a-f]{3,8}\b/i);
  assert.doesNotMatch(contextSource, /#[0-9a-f]{3,8}\b/i);
  assert.doesNotMatch(
    source,
    /(?:amber|yellow|red|destructive).*(?:participantAccent|executionKey)/i,
  );
});

test("every plan state is exposed to assistive technology and unknown differs from pending", async () => {
  const statuses = [
    "unknown",
    "pending",
    "in_progress",
    "completed",
    "blocked",
    "failed",
    "cancelled",
  ];
  const model = deriveCodingSessionMissionInspectorModel(
    input({
      acceptedPlan: {
        kind: "available",
        steps: [
          {
            text: "Review the mission state",
            sourceEventId: "accepted-plan-event",
            authorLabel: "Founder",
            sourceCreatedAt: 1,
            sourceIndex: 0,
          },
        ],
      },
      seatPlans: [
        {
          executionKey: "builder",
          ownerLabel: "Bob · Builder",
          model: {
            sourceItemId: "plan-event",
            turnId: "turn",
            timestamp: "2026-08-30T16:00:00.000Z",
            tasks: statuses.map((status, index) => ({
              id: `task-${index}`,
              text: `Task ${index}`,
              status,
            })),
            completedCount: 1,
            explanation: null,
            copyText: null,
            state: "active",
          },
        },
      ],
    }),
  );
  const view = await renderInspector({
    model,
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const notReported = view.getByText("status not reported");
    const unknown = view.getByText("unknown");
    const pending = view.getByText("pending");
    assert.equal(
      notReported.closest("[data-plan-status]").dataset.planStatus,
      "not-reported",
    );
    assert.equal(
      unknown.closest("[data-plan-status]").dataset.planStatus,
      "unknown",
    );
    assert.equal(
      pending.closest("[data-plan-status]").dataset.planStatus,
      "pending",
    );
    for (const label of [
      "in progress",
      "completed",
      "blocked",
      "failed",
      "cancelled",
    ]) {
      assert.ok(view.getByText(label));
    }
  } finally {
    view.cleanup();
  }
});

test("file and context facts expose exact provenance or the finalizer-owned Trace seam", async () => {
  const opened = [];
  const known = richModel();
  const unknown = deriveCodingSessionMissionInspectorModel(
    input({
      observedChanges: {
        files: [
          {
            path: "unknown-source.ts",
            filename: "unknown-source.ts",
            additions: null,
            deletions: null,
            diffs: [],
            editCount: 1,
          },
        ],
        unreportedEditCount: 0,
      },
      observedFileSources: new Map([["unknown-source.ts", []]]),
    }),
  );

  const knownView = await renderInspector({
    model: known,
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(knownView.getAllByText("observed-edit-event").length > 0);
    assert.ok(knownView.getAllByText("report-event").length > 0);
    assert.equal(knownView.queryByText("assignment-event"), null);
  } finally {
    knownView.cleanup();
  }

  const unknownView = await renderInspector({
    model: unknown,
    variant: "drawer",
    focusedExecutionKey: null,
    onOpenFileTrace(path) {
      opened.push(path);
    },
  });
  try {
    assert.ok(
      unknownView.getByText(
        "Observed source is unavailable in this projection.",
      ),
    );
    const trace = unknownView.getByRole("button", {
      name: "Open Trace for source evidence",
    });
    trace.click();
    assert.deepEqual(opened, ["unknown-source.ts"]);
  } finally {
    unknownView.cleanup();
  }
});

test("section truncation notices are visible status messages", async () => {
  const model = deriveCodingSessionMissionInspectorModel(
    input({
      participants: Array.from({ length: 70 }, (_, index) => ({
        executionKey: `seat-${index}`,
        label: `Seat ${index}`,
        secondaryLabel: null,
        role: "builder",
        status: { kind: "idle", label: "Idle" },
        disposition: "idle",
        activity: null,
        lastTurnLabel: "last turn just now",
      })),
    }),
  );
  const view = await renderInspector({
    model,
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const notices = view.getAllByRole("status");
    assert.ok(
      notices.some((notice) =>
        /participants; 6 omitted/.test(notice.textContent),
      ),
    );
  } finally {
    view.cleanup();
  }
});

test("file attribution truncation renders exact shown, total, and omitted counts", async () => {
  const model = deriveCodingSessionMissionInspectorModel(
    input({
      observedChanges: {
        files: [
          {
            path: "shared.ts",
            filename: "shared.ts",
            additions: 1,
            deletions: 0,
            diffs: [],
            editCount: 1,
          },
        ],
        unreportedEditCount: 0,
      },
      reports: Array.from({ length: 22 }, (_, index) => ({
        sourceEventId: `report-${index}`,
        authorLabel: `Seat ${index}`,
        summary: "report",
        assignmentRef: "assignment",
        branch: null,
        baseSha: null,
        headSha: null,
        files: ["shared.ts"],
        tests: [],
      })),
    }),
  );
  const view = await renderInspector({
    model,
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const notice = view.getByRole("status");
    assert.equal(
      notice.textContent,
      "Showing 20 of 22 file report attributions; 2 omitted.",
    );
  } finally {
    view.cleanup();
  }
});

test("U-T5: Mission state is one line plus a phase indicator, with no chain list", async () => {
  const view = await renderInspector({
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const summary = view.getByTestId("mission-state-summary");
  assert.match(summary.textContent, /Mission running · assigned/);
  const indicator = view.getByTestId("mission-state-phase-indicator");
  assert.equal(indicator.getAttribute("data-current-phase"), "assigned");
  assert.match(
    indicator.getAttribute("aria-label"),
    /Mission running — current phase assigned/,
  );
  assert.deepEqual(
    [...indicator.querySelectorAll("li")].map(
      (node) =>
        `${node.getAttribute("data-phase")}:${node.getAttribute("data-phase-state")}`,
    ),
    [
      "assigned:current",
      "reported:pending",
      "ruled:pending",
      "acknowledged:pending",
    ],
  );
  assert.equal(
    view.queryByTestId("coding-session-mission-canonical-chain"),
    null,
  );
  assert.equal(
    view.queryByTestId("coding-session-mission-transaction-card"),
    null,
  );
  view.cleanup();
});

test("U-T5: an unestablished phase says so rather than painting the first step", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({ missionState: { kind: "unknown", detail: null } }),
    ),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const indicator = view.getByTestId("mission-state-phase-indicator");
  assert.equal(indicator.getAttribute("data-current-phase"), "not-established");
  assert.match(
    indicator.getAttribute("aria-label"),
    /signed phase not established/,
  );
  assert.deepEqual(
    [...indicator.querySelectorAll("li")].map((node) =>
      node.getAttribute("data-phase-state"),
    ),
    ["pending", "pending", "pending", "pending"],
  );
  view.cleanup();
});

test("U-T5: a blocked mission states its blockers and the required action", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({
        missionState: {
          kind: "blocked",
          sourceEventId: "blocked-event",
          summary: "Mission cannot proceed.",
          blockers: ["Relay authority is unavailable."],
          requiredAction: "Restore the relay signing authority.",
          canonicalChain: [],
        },
      }),
    ),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const blocked = view.getByTestId("mission-state-blocked");
  assert.match(blocked.className, /border-destructive\/40/);
  assert.match(blocked.textContent, /Relay authority is unavailable\./);
  assert.match(
    blocked.textContent,
    /Required action: Restore the relay signing authority\./,
  );
  view.cleanup();
});

test("U-T5: Integrity keeps a bounded Delivery list with visible truncation", async () => {
  const deliveries = Array.from({ length: 35 }, (_, index) => ({
    sourceEventId: `source-${String(index).padStart(3, "0")}`,
    operationType: "report",
    sourceActorPubkey: "b".repeat(64),
    leadTargetKey: "lead-target",
    kind: index === 34 ? "failed" : "provider-queued",
    owningCommandId: `command-${index}`,
    duplicateRefusedCommandIds: index === 34 ? ["dupe-1"] : [],
    failures: [],
    reArmCount: 0,
    observedAtMs: index,
    detail: index === 34 ? "Wake delivery failed" : "Provider wake queued",
  }));
  const view = await renderInspector({
    deliveries,
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const rows = view.getAllByTestId("mission-delivery-row");
  assert.equal(rows.length, 32);
  assert.equal(rows[0].getAttribute("data-kind"), "failed");
  assert.match(rows[0].className, /border-destructive\/40/);
  assert.match(rows[0].textContent, /1 duplicate refused/);
  assert.match(
    view.getByTestId("mission-delivery-truncation").textContent,
    /Showing 32 of 35 observed deliveries; 3 older rows are not shown\./,
  );
  view.cleanup();
});

test("U-T5: no delivery projection is not the same as none observed", async () => {
  const loadingView = await renderInspector({
    loading: true,
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  assert.match(
    loadingView.getByTestId("mission-delivery-empty").textContent,
    /Wake delivery unknown/,
  );
  loadingView.cleanup();
  // U-F4: settled-but-unwired is still "no projection", not "none observed".
  // Claiming observation when nothing observed anything is the exact bug class
  // this project treats as a crash, and it is the app's steady state until the
  // finalizer wires Lane D.
  const unwiredView = await renderInspector({
    loading: false,
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  assert.match(
    unwiredView.getByTestId("mission-delivery-empty").textContent,
    /Wake delivery unknown/,
  );
  assert.doesNotMatch(
    unwiredView.getByTestId("mission-delivery-empty").textContent,
    /No team wake deliveries observed/,
  );
  unwiredView.cleanup();
  const settledView = await renderInspector({
    deliveries: [],
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  assert.match(
    settledView.getByTestId("mission-delivery-empty").textContent,
    /No team wake deliveries observed\./,
  );
  settledView.cleanup();
});

test("U-T5: a Team row carries the seat-authority sentence and its remedy", async () => {
  const remedy =
    "bee sessions seat-repair --channel chan --session-ref sess --actor act";
  const view = await renderInspector({
    model: richModel(),
    seatAuthorities: [
      {
        executionKey: "builder",
        actorPubkey: "b".repeat(64),
        role: "builder",
        kind: "created-ungranted",
        grantEventId: null,
        detail: "Seat created, not granted",
        remedy,
      },
    ],
    variant: "panel",
    focusedExecutionKey: null,
  });
  const rows = view.getAllByTestId("mission-team-seat-authority");
  assert.equal(rows.length, 1);
  assert.equal(rows[0].getAttribute("data-kind"), "created-ungranted");
  assert.match(rows[0].textContent, /Seat created, not granted/);
  assert.match(rows[0].textContent, new RegExp(remedy.replace(/-/g, "\\-")));
  view.cleanup();
});

test("U-T5: an unloaded authority projection reads unknown, never granted", async () => {
  const view = await renderInspector({
    loading: true,
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const rows = view.getAllByTestId("mission-team-seat-authority");
  assert.ok(rows.length > 0);
  for (const row of rows) {
    assert.equal(row.getAttribute("data-kind"), "unknown");
    assert.match(row.textContent, /Seat authority unknown/);
  }
  view.cleanup();
});

test("U-T5: a fold-listed unseated report says so in the Reports section", async () => {
  const model = richModel();
  const reportId = model.reports[0]?.sourceEventId;
  assert.ok(reportId, "the rich model must publish at least one report");
  const view = await renderInspector({
    model,
    unseatedReportEventIds: [reportId],
    variant: "panel",
    focusedExecutionKey: null,
  });
  const rows = view.getAllByTestId("mission-report-row");
  const unseated = rows.filter(
    (row) => row.getAttribute("data-unseated") === "true",
  );
  assert.equal(unseated.length, 1);
  assert.match(unseated[0].textContent, /· unseated/);
  view.cleanup();
});

test("U-T5: the founder's goal editor mounts under Current goal", async () => {
  const React = (await import("react")).default;
  const view = await renderInspector({
    goalEditor: React.createElement(
      "button",
      { "data-testid": "goal-editor-probe", type: "button" },
      "Edit goal",
    ),
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const probe = view.getByTestId("goal-editor-probe");
  assert.ok(probe);
  assert.match(
    probe.closest("section").textContent,
    /Ship an honest Mission inspector\./,
  );
  view.cleanup();
});

test("R2 §8: rail section order puts Team third, above Changes", async () => {
  const view = await renderInspector({
    model: richModel(),
    variant: "panel",
    focusedExecutionKey: null,
  });
  const headings = [...view.container.querySelectorAll("section > h3")].map(
    (node) => node.textContent,
  );
  assert.deepEqual(headings, [
    "Current goal",
    "Mission state",
    // Batch 3 L2: the decision queue is the state plane's companion — a
    // ruling held on a person is the most actionable thing a rail can carry,
    // so it sits with the state rather than below four panels of detail.
    "Decisions",
    "Team",
    "Changes",
    "Files",
    "Structured tests",
    "Accepted plan",
    "Seat-reported plans",
    "Reports",
    "Integrity",
  ]);
  // The remedy is the rail's one action item; it must not sit below four
  // panels of file and test detail.
  assert.ok(
    headings.indexOf("Team") < headings.indexOf("Changes"),
    "Team precedes Changes",
  );
  view.cleanup();
});

test("R2 §8: a granted seat prints its line too, so ungranted is comparable", async () => {
  const view = await renderInspector({
    model: richModel(),
    seatAuthorities: [
      {
        executionKey: "lead",
        actorPubkey: "a".repeat(64),
        role: "lead",
        kind: "granted",
        grantEventId: "g".repeat(64),
        detail: "Seat granted",
        remedy: null,
      },
      {
        executionKey: "builder",
        actorPubkey: "b".repeat(64),
        role: "builder",
        kind: "created-ungranted",
        grantEventId: null,
        detail: "Seat created, not granted",
        remedy:
          "bee sessions seat-repair --channel chan --session-ref sess --actor act",
      },
    ],
    variant: "panel",
    focusedExecutionKey: null,
  });
  const rows = view.getAllByTestId("mission-team-seat-authority");
  assert.equal(rows.length, 2);
  const granted = rows.find(
    (row) => row.getAttribute("data-kind") === "granted",
  );
  assert.ok(granted, "the granted seat renders a line of its own");
  assert.match(granted.textContent, /Seat granted/);
  // Muted, and with no flag — the ungranted one is the only thing that shouts.
  assert.match(granted.querySelector("p").className, /text-muted-foreground/);
  assert.equal(granted.querySelector("svg"), null);
  const ungranted = rows.find(
    (row) => row.getAttribute("data-kind") === "created-ungranted",
  );
  assert.match(ungranted.querySelector("p").className, /text-amber-700/);
  assert.ok(
    ungranted.querySelector("svg"),
    "the ungranted line carries an icon",
  );
  assert.match(ungranted.textContent, /bee sessions seat-repair/);
  view.cleanup();
});

test("A3.4: a blocked record and the seats still working are on one line", async () => {
  // 2026-09-01: the lead used `mission.blocked` four times as a note (no note
  // verb exists) and the rail read Blocked, in red, while two seats worked on
  // for another twenty minutes. Both facts, or the surface lies.
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(
      input({
        missionState: {
          kind: "blocked",
          sourceEventId: "blocked-event",
          summary: "Mission cannot proceed.",
          blockers: ["Relay authority is unavailable."],
          requiredAction: "Restore the relay signing authority.",
          canonicalChain: [],
        },
        participants: [
          {
            executionKey: "lead",
            label: "Keystone · Lead",
            secondaryLabel: null,
            role: "lead",
            status: { kind: "working", label: "Working" },
            disposition: "live",
            activity: null,
            lastTurnLabel: "last turn just now",
          },
          {
            executionKey: "builder",
            label: "Bob · Builder",
            secondaryLabel: null,
            role: "builder",
            status: { kind: "working", label: "Working" },
            disposition: "live",
            activity: null,
            lastTurnLabel: "last turn just now",
          },
        ],
      }),
    ),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const line = view.getByTestId("mission-state-and-liveness");
    assert.equal(line.textContent, "Blocked (signed) · 2 seats live");
    assert.equal(line.getAttribute("data-live-seats"), "2");
    // Both facts live in the same section, so neither can be read alone.
    const section = line.closest("section");
    assert.match(section.textContent, /Blocked \(signed\)/);
    assert.match(section.textContent, /2 seats live/);
    assert.match(section.textContent, /Required action:/);
  } finally {
    view.cleanup();
  }
});

test("A3.4: a state with no signed record claims no provenance and never counts zero seats", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(input()),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const line = view.getByTestId("mission-state-and-liveness");
    assert.equal(
      line.textContent,
      "State unknown · seat liveness not projected",
    );
    assert.equal(line.getAttribute("data-live-seats"), "unknown");
  } finally {
    view.cleanup();
  }
});

// ── Batch 3 L2.6: the redaction vault's marker is not a path ────────────────

const PRIVATE_CONTEXT_MARKER = `[elided private context: 183 bytes, sha256:${"ab".repeat(32)}]`;

test("L2.6: a withheld path reads as a sentence, and the digest is not re-surfaced", async () => {
  const model = deriveCodingSessionMissionInspectorModel(
    input({
      reports: [
        {
          sourceEventId: "e".repeat(64),
          authorLabel: "Bob",
          summary: "Lane W1 done.",
          assignmentRef: "a".repeat(64),
          branch: null,
          baseSha: null,
          headSha: null,
          files: [PRIVATE_CONTEXT_MARKER],
          tests: [],
        },
      ],
    }),
  );
  const view = await renderInspector({
    model,
    variant: "panel",
    focusedExecutionKey: null,
  });
  // The marker itself never reaches the reader as content.
  assert.equal(view.queryByText(PRIVATE_CONTEXT_MARKER), null);
  assert.ok(view.getByTestId("mission-file-private-context"));
  assert.ok(view.getByText(/paths private to the seat's host/));
  // Critique A2: the bytes and the digest are NOT re-surfaced in Mission — a
  // redaction disclosed as a redaction is the whole point.
  assert.equal(view.queryByText("183"), null);
  assert.equal(view.queryByText("ab".repeat(32)), null);
  view.cleanup();
});

// ── Critique A1: a refused 44227 is not silence ─────────────────────────────

test("A1: a goal this surface refused says so, and names what disagreed", async () => {
  for (const [disagreements, named] of [
    [["founder"], "founder"],
    [["session"], "session"],
  ]) {
    const view = await renderInspector({
      model: deriveCodingSessionMissionInspectorModel(
        input({ goal: { kind: "rejected", disagreements } }),
      ),
      variant: "panel",
      focusedExecutionKey: null,
    });
    assert.ok(view.getByTestId("mission-goal-rejected"));
    assert.ok(
      view.getByText(
        `A goal is published on this channel but it names a different ${named}. This surface will not show a goal it cannot bind to this mission.`,
      ),
    );
    // The one sentence that would be a lie here.
    assert.equal(view.queryByText("No accepted mission goal published."), null);
    view.cleanup();
  }
});

test("A1: the resolved-and-absent case is untouched", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(input()),
    variant: "panel",
    focusedExecutionKey: null,
  });
  assert.ok(view.getByText("No accepted mission goal published."));
  assert.equal(view.queryByTestId("mission-goal-rejected"), null);
  view.cleanup();
});
