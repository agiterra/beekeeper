import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_SURFACE_BADGE_TONE_RANK,
  codingSessionDiffMarkerChanges,
  codingSessionDiffOpenedMarker,
  codingSessionFailedCommitlessGates,
  codingSessionFailedGatesOnHead,
  codingSessionNewestEditPerFile,
  codingSessionSurfaceBadgeLabel,
  codingSessionSurfaceOpenRulings,
  codingSessionSurfaceRulingFact,
  codingSessionUnseenDiffFiles,
  deriveCodingSessionAgentsBadge,
  deriveCodingSessionDiffBadge,
  deriveCodingSessionLandingBadge,
  deriveCodingSessionPlanBadge,
  readCodingSessionLandingBadgeExtension,
  strongestCodingSessionSurfaceBadgeTone,
} from "./codingSessionSurfaceBadgeModel.ts";
import {
  codingSessionAgentsBadgeFromCtx,
  codingSessionDiffBadgeFromCtx,
  codingSessionLandingBadgeFromCtx,
  codingSessionPlanBadgeFromCtx,
  deriveCodingSessionSurfaceBadgesFromCtx,
} from "./codingSessionSurfaceBadgeModelCtx.ts";
import { deriveCodingSessionSubagentPanel } from "./codingSessionSubagents.ts";

const NOTE = "restart note";
const CLOCK = () => "14:02";
const WATCHER = (pubkey) =>
  pubkey === "a".repeat(64)
    ? "Kettle provider"
    : `provider ${pubkey.slice(0, 8)}`;

function running(gateName, overrides = {}) {
  return {
    key: `k-${gateName}`,
    gate: gateName,
    startedAtMs: 1,
    authorPubkey: "a".repeat(64),
    eventId: `start-${gateName}`,
    stale: false,
    ...overrides,
  };
}

function task(id, status) {
  return { id, text: id, status };
}

function editItem(id, path, completedAt) {
  return {
    id,
    type: "tool",
    renderClass: "file-edit",
    descriptor: { renderClass: "file-edit", object: path },
    title: "Edit",
    toolName: "Edit",
    buzzToolName: null,
    status: "completed",
    args: { file_path: path, old_string: "a", new_string: "b" },
    result: "ok",
    isError: false,
    toolKind: "edit",
    timestamp: completedAt,
    startedAt: completedAt,
    completedAt,
  };
}

function taskCall(id, status, startedAt = "2026-10-04T10:00:00.000Z") {
  return {
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: { renderClass: "generic", object: null },
    title: "Task",
    toolName: "Task",
    buzzToolName: null,
    status,
    args: { description: `spawn ${id}`, subagent_type: "Explore" },
    result: status === "completed" ? "done" : "",
    isError: false,
    toolKind: "think",
    toolCallId: `call-${id}`,
    timestamp: startedAt,
    startedAt,
    completedAt: status === "completed" ? startedAt : null,
  };
}

function gate(overrides = {}) {
  return {
    key: overrides.key ?? "g",
    authorPubkey: "a".repeat(64),
    source: "observed",
    gate: "cargo test",
    outcome: "passed",
    command: "cargo test",
    summaryLines: [],
    hiddenSummaryLines: 0,
    duration: null,
    commitShortSha: "07c470be",
    commitSha: "07c470be".padEnd(40, "0"),
    dirty: false,
    sourceEventId: "e1",
    droppedEventIds: 0,
    assignmentUnresolved: false,
    ...overrides,
  };
}

function decision(requestId, heldOn, answerId = null) {
  return {
    requestId,
    heldOn,
    blocks: [],
    answeredBy: answerId === null ? null : "f".repeat(64),
    answerId,
  };
}

const FOUNDER = "f".repeat(64);
const SEAT = "5".repeat(64);

// ---------------------------------------------------------------------------
// Tones
// ---------------------------------------------------------------------------

test("the strongest tone wins: attention > waiting > activity > neutral", () => {
  assert.equal(strongestCodingSessionSurfaceBadgeTone([]), null);
  assert.equal(strongestCodingSessionSurfaceBadgeTone([null, undefined]), null);
  assert.equal(
    strongestCodingSessionSurfaceBadgeTone([
      { tone: "neutral" },
      { tone: "activity" },
    ]),
    "activity",
  );
  assert.equal(
    strongestCodingSessionSurfaceBadgeTone([
      { tone: "activity" },
      { tone: "waiting" },
      null,
    ]),
    "waiting",
  );
  assert.equal(
    strongestCodingSessionSurfaceBadgeTone([
      { tone: "attention" },
      { tone: "waiting" },
    ]),
    "attention",
  );
  assert.ok(
    CODING_SESSION_SURFACE_BADGE_TONE_RANK.attention >
      CODING_SESSION_SURFACE_BADGE_TONE_RANK.waiting,
  );
});

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

test("two running subagents read 2, with the fact as the label", () => {
  const badge = deriveCodingSessionAgentsBadge({
    runningSubagents: 2,
    workingSeats: 0,
    rulings: [],
  });
  assert.equal(badge.tone, "activity");
  assert.equal(badge.count, 2);
  assert.equal(codingSessionSurfaceBadgeLabel(badge), "2 subagents running");
});

test("never a total: five finished subagents give no badge", () => {
  const transcript = ["a", "b", "c", "d", "e"].map((id) =>
    taskCall(id, "completed"),
  );
  const panel = deriveCodingSessionSubagentPanel([transcript]);
  assert.equal(panel.rows.length, 5);
  assert.equal(panel.running, 0);
  assert.equal(
    deriveCodingSessionAgentsBadge({
      runningSubagents: panel.running,
      workingSeats: 0,
      rulings: [],
    }),
    null,
  );
});

test("a running subagent among finished ones counts only itself", () => {
  const panel = deriveCodingSessionSubagentPanel([
    [taskCall("a", "completed"), taskCall("b", "executing")],
  ]);
  const badge = deriveCodingSessionAgentsBadge({
    runningSubagents: panel.running,
    workingSeats: 0,
    rulings: [],
  });
  assert.equal(badge.count, 1);
  assert.equal(codingSessionSurfaceBadgeLabel(badge), "1 subagent running");
});

test("an open ruling turns Agents amber and names who owes it", () => {
  const badge = deriveCodingSessionAgentsBadge({
    runningSubagents: 1,
    workingSeats: 1,
    rulings: [{ holderLabel: "Brian" }],
  });
  assert.equal(badge.tone, "waiting");
  assert.equal(badge.count, 1);
  assert.deepEqual(badge.facts, [
    "Waiting on a ruling from Brian",
    "1 seat working",
    "1 subagent running",
  ]);
});

test("rulings: only an unanswered decision.request waits; null decisions are unknown", () => {
  const name = (heldOn) => (heldOn === "founder" ? "you" : "Ira");
  assert.deepEqual(codingSessionSurfaceOpenRulings(null, name), []);
  const rulings = codingSessionSurfaceOpenRulings(
    [
      decision("r1", "founder"),
      decision("r2", SEAT, "a2"),
      // The fold never repeats a request; a repeat still counts once.
      decision("r1", "founder"),
    ],
    name,
  );
  assert.deepEqual(rulings, [{ holderLabel: "you" }]);
  assert.equal(
    codingSessionSurfaceRulingFact(rulings),
    "Waiting on a ruling from you",
  );
  assert.equal(
    codingSessionSurfaceRulingFact([
      { holderLabel: "you" },
      { holderLabel: "Ira" },
      { holderLabel: null },
    ]),
    "Waiting on 3 rulings from you, Ira and someone not resolved",
  );
});

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

test("Plan counts tasks in progress only while the seat works", () => {
  const taskModel = {
    tasks: [
      task("a", "completed"),
      task("b", "in_progress"),
      task("c", "pending"),
    ],
  };
  const badge = deriveCodingSessionPlanBadge({ taskModel, working: true });
  assert.equal(badge.count, 1);
  assert.equal(codingSessionSurfaceBadgeLabel(badge), "1 task in progress");
  assert.equal(
    deriveCodingSessionPlanBadge({ taskModel, working: false }),
    null,
  );
  assert.equal(
    deriveCodingSessionPlanBadge({ taskModel: null, working: true }),
    null,
  );
});

// ---------------------------------------------------------------------------
// Landing
// ---------------------------------------------------------------------------

test("the head is the newest commit-naming gate row; its failures get attention", () => {
  const signedAt = new Map([
    ["old", 10],
    ["head-pass", 20],
    ["head-fail", 30],
    ["nocommit", 40],
  ]);
  const failed = codingSessionFailedGatesOnHead(
    [
      gate({
        key: "old",
        sourceEventId: "old",
        outcome: "failed",
        commitSha: "1".repeat(40),
        commitShortSha: "11111111",
      }),
      gate({ key: "head-pass", sourceEventId: "head-pass", gate: "clippy" }),
      gate({
        key: "head-fail",
        sourceEventId: "head-fail",
        outcome: "failed",
      }),
      gate({
        key: "nocommit",
        sourceEventId: "nocommit",
        outcome: "failed",
        commitSha: null,
        commitShortSha: null,
      }),
    ],
    signedAt,
  );
  assert.deepEqual(
    failed.map((row) => row.key),
    ["head-fail"],
  );
  // A failure on an older commit is history, not attention.
  assert.deepEqual(
    codingSessionFailedGatesOnHead(
      [
        gate({
          key: "old",
          sourceEventId: "old",
          outcome: "failed",
          commitSha: "1".repeat(40),
        }),
        gate({ key: "head-pass", sourceEventId: "head-pass" }),
      ],
      signedAt,
    ),
    [],
  );
});

function commitless(overrides) {
  return gate({ commitSha: null, commitShortSha: null, ...overrides });
}

test("Landing: an observed failure naming no commit raises attention", () => {
  // Observed rows are signed with no head (gate_observer.rs), so this is
  // how a provider-observed failure arrives.
  const signedAt = new Map([["obs-fail", 10]]);
  const gates = [
    commitless({
      key: "obs-fail",
      sourceEventId: "obs-fail",
      outcome: "failed",
    }),
  ];
  const commitlessFailed = codingSessionFailedCommitlessGates(gates, signedAt);
  assert.deepEqual(
    commitlessFailed.map((row) => row.key),
    ["obs-fail"],
  );
  const badge = deriveCodingSessionLandingBadge({
    failedGates: codingSessionFailedGatesOnHead(gates, signedAt),
    failedCommitlessGates: commitlessFailed,
    refusingVerdict: null,
    rulings: [],
    runningGates: [],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  });
  assert.equal(badge.tone, "attention");
  assert.equal(badge.count, null);
  assert.deepEqual(badge.facts, [
    "Gate failed: cargo test (no commit named, observed)",
  ]);
  // The header's dot takes the same tone.
  assert.equal(strongestCodingSessionSurfaceBadgeTone([badge]), "attention");
});

test("Landing: a newer commitless pass for the same gate clears the failure", () => {
  const gates = [
    commitless({ key: "fail", sourceEventId: "fail", outcome: "failed" }),
    commitless({ key: "pass", sourceEventId: "pass", outcome: "passed" }),
  ];
  assert.deepEqual(
    codingSessionFailedCommitlessGates(
      gates,
      new Map([
        ["fail", 10],
        ["pass", 20],
      ]),
    ),
    [],
  );
  // The order read does not decide; the signed time does.
  assert.deepEqual(
    codingSessionFailedCommitlessGates(
      gates,
      new Map([
        ["fail", 30],
        ["pass", 20],
      ]),
    ).map((row) => row.key),
    ["fail"],
  );
  // Another author or source is another key: its pass clears nothing.
  assert.deepEqual(
    codingSessionFailedCommitlessGates(
      [
        gates[0],
        commitless({
          key: "declared-pass",
          sourceEventId: "pass",
          source: "declared",
          outcome: "passed",
        }),
      ],
      new Map([
        ["fail", 10],
        ["pass", 20],
      ]),
    ).map((row) => row.key),
    ["fail"],
  );
});

test("from a ctx: a commitless observed failure reaches Landing's badge", () => {
  const badge = codingSessionLandingBadgeFromCtx(
    ctx({
      observations: {
        state: "read",
        view: {
          gates: [
            commitless({
              key: "obs-fail",
              sourceEventId: "obs-fail",
              outcome: "failed",
            }),
          ],
        },
        result: { signedAt: new Map([["obs-fail", 10]]) },
      },
    }),
    [],
  );
  assert.equal(badge.tone, "attention");
  assert.deepEqual(badge.facts, [
    "Gate failed: cargo test (no commit named, observed)",
  ]);
});

test("Landing: a running gate is activity with the restart note one hover away", () => {
  const badge = deriveCodingSessionLandingBadge({
    failedGates: [],
    refusingVerdict: null,
    rulings: [],
    runningGates: [running("just ci")],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  });
  assert.equal(badge.tone, "activity");
  assert.equal(badge.count, 1);
  assert.equal(
    codingSessionSurfaceBadgeLabel(badge),
    "Gate running: just ci, watched by Kettle provider",
  );
  assert.equal(
    badge.detail,
    `just ci: started 14:02 by the provider's clock.\n${NOTE}`,
  );
});

test("Landing: each watcher is named for the gates it signed", () => {
  const badge = deriveCodingSessionLandingBadge({
    failedGates: [],
    refusingVerdict: null,
    rulings: [],
    runningGates: [
      running("just ci"),
      running("clippy"),
      running("just ci", { authorPubkey: "b".repeat(64), startedAtMs: 2 }),
    ],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  });
  assert.equal(badge.count, 3);
  assert.deepEqual(badge.facts, [
    "Gate running: just ci, clippy, watched by Kettle provider",
    "Gate running: just ci, watched by provider bbbbbbbb",
  ]);
});

test("Landing: a stale start is not running and draws nothing", () => {
  assert.equal(
    deriveCodingSessionLandingBadge({
      failedGates: [],
      refusingVerdict: null,
      rulings: [],
      runningGates: [running("just ci", { stale: true })],
      runningGateNote: NOTE,
      nameWatcher: WATCHER,
      formatTime: CLOCK,
    }),
    null,
  );
});

test("Landing: attention outranks waiting and running, and keeps every fact", () => {
  const badge = deriveCodingSessionLandingBadge({
    failedGates: [gate({ outcome: "failed" })],
    refusingVerdict: { label: "approved for another ref" },
    rulings: [{ holderLabel: "you" }],
    runningGates: [running("just ci")],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  });
  assert.equal(badge.tone, "attention");
  assert.equal(badge.count, null);
  assert.deepEqual(badge.facts, [
    "Gate failed: cargo test on 07c470be (observed)",
    "Verdict refuses: approved for another ref",
    "Waiting on a ruling from you",
    "Gate running: just ci, watched by Kettle provider",
  ]);
});

test("Landing: waiting without attention is amber", () => {
  const badge = deriveCodingSessionLandingBadge({
    failedGates: [],
    refusingVerdict: null,
    rulings: [{ holderLabel: "you" }],
    runningGates: [],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  });
  assert.equal(badge.tone, "waiting");
  assert.equal(badge.detail, null);
});

test("Landing extension: only a labelled refusing verdict is read", () => {
  assert.equal(readCodingSessionLandingBadgeExtension(undefined), null);
  assert.equal(
    readCodingSessionLandingBadgeExtension({ refusingVerdict: null }),
    null,
  );
  assert.equal(
    readCodingSessionLandingBadgeExtension({ refusingVerdict: { label: " " } }),
    null,
  );
  assert.deepEqual(
    readCodingSessionLandingBadgeExtension({
      refusingVerdict: { label: "no verdict on this head" },
    }),
    { label: "no verdict on this head" },
  );
});

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

test("newest edit per file is the newest by time, not by array order", () => {
  // The umbrella concatenates seats in umbrella order: seat A's items come
  // first, so A's fresh edit sits before B's older one in the array.
  const newest = codingSessionNewestEditPerFile([
    editItem("a-old", "src/x.ts", "2026-10-04T09:00:00.000Z"),
    editItem("a-new", "src/x.ts", "2026-10-04T11:00:00.000Z"),
    editItem("b-old", "src/x.ts", "2026-10-04T10:00:00.000Z"),
  ]);
  assert.equal(newest.get("src/x.ts").editId, "a-new");
  const marker = { via: "opened", atMs: 0, seen: { "src/x.ts": "b-old" } };
  assert.deepEqual(codingSessionUnseenDiffFiles(newest, marker), ["src/x.ts"]);
  // Equal times: the later item wins; an undated edit falls back to order.
  const tied = codingSessionNewestEditPerFile([
    editItem("t1", "src/y.ts", "2026-10-04T10:00:00.000Z"),
    editItem("t2", "src/y.ts", "2026-10-04T10:00:00.000Z"),
    editItem("u1", "src/z.ts", "2026-10-04T10:00:00.000Z"),
    editItem("u2", "src/z.ts", "not a date"),
  ]);
  assert.equal(tied.get("src/y.ts").editId, "t2");
  assert.equal(tied.get("src/z.ts").editId, "u2");
});

test("newest edit per file: later edits win; failed edits do not count", () => {
  const newest = codingSessionNewestEditPerFile([
    editItem("e1", "src/a.ts", "2026-10-04T10:00:00.000Z"),
    editItem("e2", "./src/a.ts", "2026-10-04T10:05:00.000Z"),
    {
      ...editItem("e3", "src/b.ts", "2026-10-04T10:06:00.000Z"),
      isError: true,
    },
    editItem("e4", "src/c.ts", "not a date"),
  ]);
  assert.deepEqual([...newest.keys()], ["src/a.ts", "src/c.ts"]);
  assert.equal(newest.get("src/a.ts").editId, "e2");
  assert.equal(newest.get("src/c.ts").atMs, null);
});

test("unseen files: by edit id alone — a new edit or a file the marker never recorded", () => {
  const t0 = Date.parse("2026-10-04T10:00:00.000Z");
  const newest = new Map([
    ["src/a.ts", { editId: "e1", atMs: t0 - 1 }],
    ["src/b.ts", { editId: "e9", atMs: t0 - 1 }],
    ["src/c.ts", { editId: "e3", atMs: t0 + 1 }],
    // Undated, and dated before the marker by a clock running behind: both
    // still count, because the marker never recorded them.
    ["src/d.ts", { editId: "e4", atMs: null }],
    ["src/e.ts", { editId: "e5", atMs: t0 - 60_000 }],
  ]);
  const marker = {
    via: "opened",
    atMs: t0,
    seen: { "src/a.ts": "e1", "src/b.ts": "e2" },
  };
  assert.deepEqual(codingSessionUnseenDiffFiles(newest, marker), [
    "src/b.ts",
    "src/c.ts",
    "src/d.ts",
    "src/e.ts",
  ]);
  assert.deepEqual(codingSessionUnseenDiffFiles(newest, null), []);
});

test("opening Diff marks every newest edit seen, and clears the count", () => {
  const newest = new Map([["src/a.ts", { editId: "e2", atMs: 5 }]]);
  const opened = codingSessionDiffOpenedMarker(newest, 100);
  assert.deepEqual(opened, {
    via: "opened",
    atMs: 100,
    seen: { "src/a.ts": "e2" },
  });
  assert.deepEqual(codingSessionUnseenDiffFiles(newest, opened), []);
  assert.equal(
    codingSessionDiffMarkerChanges(opened, { ...opened, atMs: 200 }),
    false,
  );
  assert.equal(
    codingSessionDiffMarkerChanges(opened, {
      ...opened,
      seen: { "src/a.ts": "e3" },
    }),
    true,
  );
  assert.equal(codingSessionDiffMarkerChanges(null, opened), true);
});

test("Diff badge is neutral, says 'on this device', and hides while on screen", () => {
  const badge = deriveCodingSessionDiffBadge({
    unseenFiles: 2,
    via: "opened",
    onScreen: false,
  });
  assert.equal(badge.tone, "neutral");
  assert.equal(badge.count, 2);
  assert.equal(
    codingSessionSurfaceBadgeLabel(badge),
    "2 files changed since you last opened Diff on this device",
  );
  assert.match(
    codingSessionSurfaceBadgeLabel(
      deriveCodingSessionDiffBadge({
        unseenFiles: 1,
        via: "baseline",
        onScreen: false,
      }),
    ),
    /^1 file changed since this session was first shown on this device$/,
  );
  assert.equal(
    deriveCodingSessionDiffBadge({
      unseenFiles: 2,
      via: "opened",
      onScreen: true,
    }),
    null,
  );
  assert.equal(
    deriveCodingSessionDiffBadge({
      unseenFiles: 0,
      via: "opened",
      onScreen: false,
    }),
    null,
  );
  assert.equal(
    deriveCodingSessionDiffBadge({
      unseenFiles: 3,
      via: null,
      onScreen: false,
    }),
    null,
  );
});

// ---------------------------------------------------------------------------
// From a ctx
// ---------------------------------------------------------------------------

function ctx(overrides = {}) {
  const execution = {
    executionKey: "exec-1",
    activeGeneration: { transcript: [] },
  };
  return {
    layout: "single",
    channelId: "c",
    communityScope: "wss://relay.test",
    sessionKey: "s",
    focusedExecution: execution,
    executions: [{ execution, status: { kind: "working", label: "Working" } }],
    transcript: [],
    subagents: { rows: [], settled: 0, running: 0, totalTokens: null },
    taskModel: null,
    activeSurfaceId: null,
    observations: { state: "not-read", reason: "no genesis" },
    openRulings: null,
    currentUserPubkey: FOUNDER,
    founderPubkey: FOUNDER,
    resolveActorName: (pubkey) => (pubkey === SEAT ? "Ira" : null),
    extensions: {},
    ...overrides,
  };
}

test("from a ctx: the single layout never counts its own execution", () => {
  assert.equal(codingSessionAgentsBadgeFromCtx(ctx()), null);
  const umbrella = ctx({
    layout: "umbrella",
    executions: [
      {
        execution: {
          executionKey: "a",
          activeGeneration: { transcript: [] },
        },
        status: { kind: "working", label: "Working" },
      },
      {
        execution: {
          executionKey: "b",
          activeGeneration: { transcript: [] },
        },
        status: { kind: "working", label: "Working" },
      },
      { execution: { executionKey: "c" }, status: { kind: "idle" } },
    ],
  });
  const badge = codingSessionAgentsBadgeFromCtx(umbrella);
  assert.equal(badge.count, 2);
  assert.equal(codingSessionSurfaceBadgeLabel(badge), "2 seats working");
});

test("from a ctx: Plan reads the focused seat's status", () => {
  const taskModel = { tasks: [task("a", "in_progress")] };
  assert.equal(codingSessionPlanBadgeFromCtx(ctx({ taskModel })).count, 1);
  assert.equal(
    codingSessionPlanBadgeFromCtx(
      ctx({
        taskModel,
        executions: [
          {
            execution: { executionKey: "exec-1" },
            status: { kind: "idle" },
          },
        ],
      }),
    ),
    null,
  );
});

test("from a ctx: unread observations claim no failure; Landing reads B2's extension", () => {
  assert.equal(codingSessionLandingBadgeFromCtx(ctx(), []), null);
  const badge = codingSessionLandingBadgeFromCtx(
    ctx({
      extensions: {
        landing: { refusingVerdict: { label: "verdict refused" } },
      },
    }),
    [],
  );
  assert.equal(badge.tone, "attention");
});

test("from a ctx: Diff counts against the marker, and all four come back by id", () => {
  const t0 = Date.parse("2026-10-04T10:00:00.000Z");
  const c = ctx({
    transcript: [
      editItem("e1", "src/a.ts", "2026-10-04T09:00:00.000Z"),
      editItem("e2", "src/b.ts", "2026-10-04T11:00:00.000Z"),
    ],
  });
  const marker = { via: "baseline", atMs: t0, seen: { "src/a.ts": "e1" } };
  assert.equal(codingSessionDiffBadgeFromCtx(c, marker).count, 1);
  assert.equal(
    codingSessionDiffBadgeFromCtx({ ...c, activeSurfaceId: "diff" }, marker),
    null,
  );
  const all = deriveCodingSessionSurfaceBadgesFromCtx(c, {
    runningGates: [],
    diffMarker: marker,
  });
  assert.deepEqual(Object.keys(all).sort(), [
    "agents",
    "diff",
    "landing",
    "plan",
  ]);
  assert.equal(
    strongestCodingSessionSurfaceBadgeTone(Object.values(all)),
    "neutral",
  );
});

test("from a ctx: a seat not on a turn has no live subagents", () => {
  // A provider abandoned the turn with two Task calls open, and the session
  // rests idle: the stream reads them stopped, so the badge counts none.
  const open = [taskCall("a", "executing"), taskCall("b", "executing")];
  const entry = (status) => ({
    execution: {
      executionKey: "exec-1",
      activeGeneration: { transcript: open },
    },
    status,
  });
  const subagents = deriveCodingSessionSubagentPanel([open]);
  assert.equal(subagents.running, 2);
  for (const status of [
    { kind: "idle", label: "Idle" },
    { kind: "working", label: "Starting" },
  ]) {
    assert.equal(
      codingSessionAgentsBadgeFromCtx(
        ctx({ subagents, executions: [entry(status)] }),
      ),
      null,
    );
  }
  const live = codingSessionAgentsBadgeFromCtx(
    ctx({
      subagents,
      executions: [entry({ kind: "working", label: "Working" })],
    }),
  );
  assert.equal(live.count, 2);
  assert.equal(codingSessionSurfaceBadgeLabel(live), "2 subagents running");
});

test("from a ctx: a decision.request waits on a person; an open report does not", () => {
  // Sixty open assignments and a report owed a verdict fill the capped holds
  // list; none is a person's ruling, so none turns a badge amber.
  const openRulings = {
    holds: Array.from({ length: 50 }, () => ({ holding: "assignment" })),
    omitted: 11,
  };
  assert.equal(codingSessionAgentsBadgeFromCtx(ctx({ openRulings })), null);
  assert.equal(
    codingSessionLandingBadgeFromCtx(ctx({ openRulings }), []),
    null,
  );
  // Not read is not "none": no amber either.
  assert.equal(
    codingSessionAgentsBadgeFromCtx(ctx({ openRulings, decisions: null })),
    null,
  );

  const decisions = [
    decision("r1", "founder"),
    decision("r2", SEAT),
    decision("r3", "founder", "answer-3"),
  ];
  const agents = codingSessionAgentsBadgeFromCtx(
    ctx({ openRulings, decisions }),
  );
  assert.equal(agents.tone, "waiting");
  assert.equal(agents.count, 2);
  assert.equal(
    codingSessionSurfaceBadgeLabel(agents),
    "Waiting on 2 rulings from you and Ira",
  );
  const landing = codingSessionLandingBadgeFromCtx(
    ctx({ openRulings, decisions }),
    [],
  );
  assert.equal(landing.tone, "waiting");
  // Viewed by someone else, the founder's ruling is the founder's.
  const other = codingSessionAgentsBadgeFromCtx(
    ctx({ decisions: [decision("r1", "founder")], currentUserPubkey: SEAT }),
  );
  assert.equal(
    codingSessionSurfaceBadgeLabel(other),
    "Waiting on a ruling from the founder",
  );
});

test("from a ctx: more than fifty open rulings are all counted", () => {
  const decisions = Array.from({ length: 61 }, (_, index) =>
    decision(`r${index}`, "founder"),
  );
  const badge = codingSessionAgentsBadgeFromCtx(ctx({ decisions }));
  assert.equal(badge.count, 61);
  assert.equal(
    codingSessionSurfaceBadgeLabel(badge),
    "Waiting on 61 rulings from you",
  );
});

test("from a ctx: a running gate names its watcher, or its key when unnamed", () => {
  const badge = codingSessionLandingBadgeFromCtx(
    ctx({ resolveActorName: () => null }),
    [running("just ci")],
    () => "14:02",
  );
  assert.equal(
    codingSessionSurfaceBadgeLabel(badge),
    "Gate running: just ci, watched by provider aaaaaaaa…aaaa",
  );
  assert.match(
    badge.detail,
    /^just ci: started 14:02 by the provider's clock\./,
  );
});

test("Landing: a running gate read from a snapshot says it is not live", () => {
  const badge = deriveCodingSessionLandingBadge({
    failedGates: [],
    refusingVerdict: null,
    rulings: [],
    runningGates: [
      {
        ...running("just ci"),
        notLive: {
          short: "not live — read at 14:05",
          sentence: "Not live: the subscription is down.",
        },
      },
    ],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  });
  assert.equal(
    codingSessionSurfaceBadgeLabel(badge),
    "Gate running: just ci, watched by Kettle provider (not live — read at 14:05)",
  );
  assert.match(badge.detail, /\nNot live: the subscription is down\.$/);
});
