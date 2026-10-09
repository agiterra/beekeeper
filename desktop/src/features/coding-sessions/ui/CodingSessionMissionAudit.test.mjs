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
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const START = Date.parse("2026-09-01T21:34:22.000Z");

function at(offsetSeconds) {
  return new Date(START + offsetSeconds * 1_000).toISOString();
}

function tool(id, turnId, offsetSeconds, args, result = "") {
  return {
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: { renderClass: "generic", label: "Ran tool", preview: null },
    title: id,
    toolName: id,
    beekeeperToolName: null,
    status: result === null ? "executing" : "completed",
    args,
    result: result ?? "",
    isError: false,
    timestamp: at(offsetSeconds),
    startedAt: at(offsetSeconds),
    completedAt: at(offsetSeconds),
    turnId,
  };
}

function turnResult(id, turnId, offsetSeconds, durationMs, usage, costUsd) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "done",
    outcome: "success",
    durationMs,
    costUsd: costUsd ?? null,
    timestamp: at(offsetSeconds),
    turnId,
    ...(usage ? { usage } : {}),
  };
}

/** The live run's own shape: Keystone reported usage, Bob's driver did not. */
function liveRunSeats() {
  return [
    {
      executionKey: "execution:keystone",
      seat: "Keystone · Lead",
      transcript: [
        tool(
          "k-skill",
          "k1",
          1,
          { path: "skills/lead/SKILL.md" },
          "s".repeat(4_096),
        ),
        tool(
          "k-skill-again",
          "k1",
          2,
          { path: "skills/lead/SKILL.md" },
          "s".repeat(4_096),
        ),
        tool("k-repair", "k1", 3, { command: "bee sessions seat-repair" }, "5"),
        tool(
          "k-repair-2",
          "k1",
          4,
          { command: "bee sessions seat-repair" },
          "5",
        ),
        tool(
          "k-repair-3",
          "k1",
          5,
          { command: "bee sessions seat-repair" },
          "5",
        ),
        turnResult("k-result", "k1", 546, 546_000, {
          toolCalls: 64,
          inputTokens: 4_200,
          outputTokens: 31_994,
          cacheReadTokens: 5_108_379,
          cacheWriteTokens: 135_012,
          contextWindow: 1_000_000,
        }),
      ],
    },
    {
      executionKey: "execution:bob",
      seat: "Bob · Builder",
      transcript: [
        tool(
          "b-op",
          "b1",
          300,
          { command: "bee sessions operation get --id 1" },
          "a",
        ),
        tool(
          "b-op-2",
          "b1",
          301,
          { command: "bee sessions operation get --id 2" },
          "b",
        ),
        turnResult("b-result", "b1", 1_122, 822_000),
      ],
    },
  ];
}

async function renderAudit(props) {
  const React = (await import("react")).default;
  const { cleanup, render } = await import("@testing-library/react");
  const { CodingSessionMissionAudit } = await import(
    "./CodingSessionMissionAudit.tsx"
  );
  const { deriveCodingSessionObservationView } = await import(
    "../lib/codingSessionObservationView.ts"
  );
  return {
    cleanup,
    ...render(
      React.createElement(CodingSessionMissionAudit, {
        // L5: the tab's signed half. These tests are about the five
        // transcript-derived sections, so every one of them supplies the
        // no-fold view unless it says otherwise — which is what a session
        // whose observations have not been read looks like.
        observations: deriveCodingSessionObservationView({
          fold: null,
          resolveLabel: () => null,
        }),
        ...props,
      }),
    ),
  };
}

test("A3.5 Per turn: one row per signed turn, with the run's own numbers", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const rows = view.getAllByTestId("mission-audit-turn-row");
  assert.equal(rows.length, 2);
  const keystone = rows[0].textContent;
  assert.match(keystone, /Keystone · Lead/);
  assert.match(keystone, /9m 6s/);
  assert.match(keystone, /64/);
  assert.match(keystone, /31,994/);
  assert.match(keystone, /5,108,379/);
  assert.match(keystone, /135,012/);
  assert.match(keystone, /1,000,000/);
  view.cleanup();
});

test("A3.5 Per turn: an unreported number is an em dash that says so, never 0", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const bob = view.getAllByTestId("mission-audit-turn-row")[1];
  const absent = bob.querySelectorAll(
    "[data-testid='mission-audit-not-reported']",
  );
  // Out, cache reads, cache writes and context window all went unreported.
  assert.equal(absent.length, 4);
  assert.equal(absent[0].getAttribute("title"), "not reported");
  assert.match(absent[0].textContent, /not reported/);
  assert.doesNotMatch(bob.textContent, /\b0\b/);
  view.cleanup();
});

test("A3.5 Per turn: the table can be grouped by seat", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const sort = view.getByTestId("coding-session-mission-audit-sort");
  assert.equal(sort.getAttribute("aria-pressed"), "false");
  const { act } = await import("react");
  const { fireEvent } = await import("@testing-library/react");
  act(() => {
    fireEvent.click(sort);
  });
  assert.equal(sort.getAttribute("aria-pressed"), "true");
  assert.deepEqual(
    view
      .getAllByTestId("mission-audit-turn-row")
      .map((row) => row.getAttribute("data-execution")),
    ["execution:bob", "execution:keystone"],
  );
  view.cleanup();
});

test("A3.5 Totals: per seat, Σ per session, and the partial-reporting disclosure", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const rows = view.getAllByTestId("mission-audit-totals-row");
  assert.equal(rows.length, 3);
  assert.match(rows[0].textContent, /Keystone · Lead/);
  assert.match(rows[2].textContent, /Σ this session/);
  assert.match(rows[2].textContent, /31,994/);
  // Turn-granular now: Keystone's one turn reported, Bob's did not.
  assert.equal(
    view.getAllByTestId("mission-audit-partial-disclosure").at(-1).textContent,
    "(1 of 2 turns reported usage)",
  );
  view.cleanup();
});

test("A3.5 Handed twice: the repeated read is named with its size", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const list = view.getByTestId("mission-audit-handed-twice");
  assert.match(list.textContent, /skills\/lead\/SKILL\.md/);
  assert.match(list.textContent, /×2/);
  assert.match(list.textContent, /8 KB/);
  // A1 rule 3: published bytes, and nothing was clipped in this fixture.
  assert.equal(list.querySelectorAll("[data-clipped='true']").length, 0);
  view.cleanup();
});

test("A3.5 Downloads the room: the unbounded relay read is counted per subcommand", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const list = view.getByTestId("mission-audit-room-downloads");
  assert.match(list.textContent, /sessions operation/);
  assert.doesNotMatch(list.textContent, /--id/);
  assert.match(list.textContent, /×2/);
  assert.match(list.textContent, /Bob · Builder/);
  view.cleanup();
});

test("A3.5 Retry loops: identical commands with identical results", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const list = view.getByTestId("mission-audit-retry-loops");
  assert.match(list.textContent, /bee sessions seat-repair/);
  assert.match(list.textContent, /identical results/);
  // A1 rule 2: the longest run, which is three, not the total across runs.
  assert.match(list.textContent, /×3/);
  view.cleanup();
});

test("A3.5: an empty session discloses absence, never zeros", async () => {
  const view = await renderAudit({ seats: [], variant: "drawer" });
  assert.match(
    view.getByTestId("coding-session-mission-audit").textContent,
    /No signed turn has closed in this session yet/,
  );
  assert.match(
    view.getByTestId("coding-session-mission-audit").textContent,
    /No seat asked for the same path or command twice/,
  );
  assert.match(
    view.getByTestId("coding-session-mission-audit").textContent,
    /No unbounded relay read observed/,
  );
  assert.equal(view.queryAllByTestId("mission-audit-turn-row").length, 0);
  view.cleanup();
});

test("A3.5 A1 rule 1: a turn with no driver count shows the published one, marked", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const bob = view.getAllByTestId("mission-audit-turn-row")[1];
  const observed = bob.querySelector("[data-observed='true']");
  assert.equal(observed.textContent, "2*");
  assert.match(observed.getAttribute("title"), /the driver reported no count/);
  // Keystone's driver did report one, so its cell carries no marker.
  const keystone = view.getAllByTestId("mission-audit-turn-row")[0];
  assert.equal(keystone.querySelector("[data-observed='true']"), null);
  assert.match(keystone.textContent, /64/);
  view.cleanup();
});

test("A3.5 A1 rule 3: a clipped result reads as a floor, not a size", async () => {
  const view = await renderAudit({
    seats: [
      {
        executionKey: "execution:bob",
        seat: "Bob · Builder",
        transcript: [
          tool(
            "a",
            "t1",
            0,
            { path: "big.log" },
            `${"x".repeat(64)}…[elided 4096 bytes]`,
          ),
          tool("b", "t1", 1, { path: "big.log" }, "y".repeat(64)),
        ],
      },
    ],
    variant: "panel",
  });
  const clipped = view
    .getByTestId("mission-audit-handed-twice")
    .querySelector("[data-clipped='true']");
  assert.match(clipped.textContent, /^≥ /);
  assert.match(clipped.getAttribute("title"), /clipped at least one/);
  view.cleanup();
});

test("A3.5/F2: a seat whose Σ sums unreported turns says so on its own row", async () => {
  const view = await renderAudit({
    seats: [
      {
        executionKey: "execution:bob",
        seat: "Bob · Builder",
        transcript: [
          turnResult("r1", "t1", 10, 1_000, { outputTokens: 1_000 }),
          turnResult("r2", "t2", 20, 1_000),
          turnResult("r3", "t3", 30, 1_000),
        ],
      },
    ],
    variant: "panel",
  });
  const disclosures = view
    .getAllByTestId("mission-audit-partial-disclosure")
    .map((node) => node.textContent);
  // One on the seat row, one on the Σ row — the seat-granular version rendered
  // neither, because the seat had "reported".
  assert.deepEqual(disclosures, [
    "(1 of 3 turns reported usage)",
    "(1 of 3 turns reported usage)",
  ]);
  view.cleanup();
});

test("A3.5/F2: the Σ tools total carries the same marker its rows do", async () => {
  const view = await renderAudit({ seats: liveRunSeats(), variant: "panel" });
  const totals = view.getAllByTestId("mission-audit-totals-row").at(-1);
  const observed = totals.querySelector("[data-observed='true']");
  assert.ok(observed, "the Σ marks a total that sums a published count");
  assert.match(observed.textContent, /\*$/);
  assert.match(
    observed.getAttribute("title"),
    /reported no count of their own/,
  );
  view.cleanup();
});

test("Amendment 00:20: a run with no published results does not claim agreement", async () => {
  const view = await renderAudit({
    seats: [
      {
        executionKey: "execution:keystone",
        seat: "Keystone · Lead",
        transcript: [
          tool("a", "t1", 0, { command: "bee sessions seat-repair" }, null),
          tool("b", "t1", 1, { command: "bee sessions seat-repair" }, null),
          tool("c", "t1", 2, { command: "bee sessions seat-repair" }, null),
        ],
      },
    ],
    variant: "panel",
  });
  const loops = view.getByTestId("mission-audit-retry-loops");
  assert.match(loops.textContent, /agreement unknown/);
  assert.doesNotMatch(loops.textContent, /identical results/);
  view.cleanup();
});

test("Amendment 00:20: an unanswered repeat shows no bytes and names the gap", async () => {
  const view = await renderAudit({
    seats: [
      {
        executionKey: "execution:bob",
        seat: "Bob · Builder",
        transcript: [
          tool("a", "t1", 0, { path: "open.ts" }, null),
          tool("b", "t1", 1, { path: "open.ts" }, null),
        ],
      },
    ],
    variant: "panel",
  });
  const list = view.getByTestId("mission-audit-handed-twice");
  assert.match(list.textContent, /0 of 2 answers seen/);
  assert.equal(
    list.querySelectorAll("[data-testid='mission-audit-not-reported']").length,
    1,
  );
  assert.doesNotMatch(list.textContent, /0 B/);
  view.cleanup();
});

test("Amendment 00:20: a cut-short count renders as a floor in the cell and the Σ", async () => {
  const view = await renderAudit({
    seats: [
      {
        executionKey: "execution:bob",
        seat: "Bob · Builder",
        transcriptTruncated: true,
        transcript: [
          tool("a", "t1", 0, { command: "cargo test" }, "ok"),
          turnResult("r", "t1", 10, 1_000),
        ],
      },
    ],
    variant: "panel",
  });
  const cell = view
    .getAllByTestId("mission-audit-turn-row")[0]
    .querySelector("[data-truncated='true']");
  assert.match(cell.textContent, /^≥ 1\*$/);
  const totals = view
    .getAllByTestId("mission-audit-totals-row")
    .at(-1)
    .querySelector("[data-truncated='true']");
  assert.match(totals.textContent, /^≥ 1\*$/);
  view.cleanup();
});
