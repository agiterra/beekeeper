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

test("panel and drawer variants expose the same honest Mission sections", async () => {
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
      assert.equal(
        view.queryByRole("heading", { name: "Inspector" }) !== null,
        variant === "drawer",
        "the inline host already labels its Inspector tab",
      );
      for (const heading of [
        "Current goal",
        "Mission state",
        "Accepted plan",
        "Seat-reported plans",
        "Changes",
        "Files",
        "Structured tests",
        "Team",
        "Context",
        "Reports",
        "Integrity",
      ]) {
        assert.ok(view.getByRole("heading", { name: heading }));
      }
      assert.equal(view.getAllByText("Accepted from Helios · Lead").length, 2);
      assert.ok(view.getByText("Implement the signed transaction projection."));
      assert.ok(
        view.getByText(
          "Preserve the signed governance chain after completion.",
        ),
      );
      assert.ok(view.getByText("Ownership · Helios · Lead"));
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
        /context\s+1000\/10000 \(10%\)/,
      );
      assert.equal(view.queryByText("Terminal usage not reported."), null);
    } finally {
      view.cleanup();
    }
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

test("empty and unknown states disclose absence rather than rendering zeros", async () => {
  const view = await renderInspector({
    model: deriveCodingSessionMissionInspectorModel(input()),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(view.getByText("Mission state unknown"));
    assert.ok(view.getByText("No accepted mission goal published."));
    assert.ok(view.getByText("No accepted plan published"));
    assert.ok(view.getByText("No seat has published a signed plan."));
    assert.ok(view.getByText("No signed edit activity observed."));
    assert.ok(view.getByText("No structured test results published."));
    assert.ok(view.getByText("No signed session seats projected."));
    assert.ok(view.getByText("Terminal usage not reported."));
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
    assert.ok(view.getByText("Conflicting mission state"));
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
  assert.doesNotMatch(source, /text-\[[^\]]+\]/);
  assert.doesNotMatch(source, /#[0-9a-f]{3,8}\b/i);
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
    assert.ok(knownView.getAllByText("assignment-event").length > 0);
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
