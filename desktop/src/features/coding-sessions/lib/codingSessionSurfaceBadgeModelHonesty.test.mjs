// Fix round 1 (b3-badges): a starting seat is not a working one, an
// unchecked gate start is not provider-watched, and Diff's "unseen" never
// compares the producer's clock with this device's.
import assert from "node:assert/strict";
import test from "node:test";

import { CODING_SESSION_LANDING_SIGNER_UNCHECKED } from "./codingSessionLandingModel.ts";
import {
  codingSessionDiffBaselineMarker,
  codingSessionDiffOpenedMarker,
  codingSessionNewestEditPerFile,
  codingSessionSurfaceBadgeLabel,
  codingSessionUnseenDiffFiles,
  deriveCodingSessionAgentsBadge,
  deriveCodingSessionLandingBadge,
} from "./codingSessionSurfaceBadgeModel.ts";
import {
  codingSessionAgentsBadgeFromCtx,
  codingSessionLandingBadgeFromCtx,
} from "./codingSessionSurfaceBadgeModelCtx.ts";

const NOTE = "restart note";
const CLOCK = () => "14:02";
const WATCHER = () => "Kettle provider";

function running(gateName) {
  return {
    key: `k-${gateName}`,
    gate: gateName,
    startedAtMs: 1,
    authorPubkey: "a".repeat(64),
    eventId: `start-${gateName}`,
    stale: false,
  };
}

function editItem(id, path, completedAt) {
  return {
    id,
    type: "tool",
    renderClass: "file-edit",
    descriptor: { renderClass: "file-edit", object: path },
    title: "Edit",
    toolName: "Edit",
    beekeeperToolName: null,
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

function ctx(overrides = {}) {
  return {
    layout: "umbrella",
    channelId: "c",
    communityScope: "wss://relay.test",
    sessionKey: "s",
    focusedExecution: null,
    executions: [],
    transcript: [],
    taskModel: null,
    activeSurfaceId: null,
    observations: { state: "not-read", reason: "no genesis" },
    decisions: null,
    currentUserPubkey: null,
    founderPubkey: null,
    resolveActorName: () => "Kettle provider",
    extensions: {},
    ...overrides,
  };
}

function seat(key, status) {
  return {
    execution: { executionKey: key, activeGeneration: { transcript: [] } },
    status,
  };
}

test("Agents: a starting seat is its own fact, never 'working'", () => {
  const onlyStarting = codingSessionAgentsBadgeFromCtx(
    ctx({
      executions: [seat("a", { kind: "working", label: "Starting" })],
    }),
  );
  assert.equal(onlyStarting.tone, "activity");
  assert.equal(onlyStarting.count, 1);
  assert.deepEqual(onlyStarting.facts, ["1 seat starting"]);

  const mixed = codingSessionAgentsBadgeFromCtx(
    ctx({
      executions: [
        seat("a", { kind: "working", label: "Working" }),
        seat("b", { kind: "working", label: "Starting" }),
        seat("c", { kind: "working", label: "Starting" }),
        seat("d", { kind: "idle", label: "Idle" }),
      ],
    }),
  );
  assert.equal(mixed.count, 3);
  assert.equal(
    codingSessionSurfaceBadgeLabel(mixed),
    "1 seat working. 2 seats starting",
  );

  // The single layout counts neither: the header already shows its status.
  assert.equal(
    codingSessionAgentsBadgeFromCtx(
      ctx({
        layout: "single",
        executions: [seat("a", { kind: "working", label: "Starting" })],
      }),
    ),
    null,
  );
  // The pure model: absent startingSeats means none.
  assert.equal(
    deriveCodingSessionAgentsBadge({
      runningSubagents: 0,
      workingSeats: 0,
      rulings: [],
    }),
    null,
  );
});

test("Landing: an unchecked start reads as signed, by the signer's clock", () => {
  const input = {
    failedGates: [],
    refusingVerdict: null,
    rulings: [],
    runningGates: [running("just ci")],
    runningGateNote: NOTE,
    nameWatcher: WATCHER,
    formatTime: CLOCK,
  };
  const unchecked = deriveCodingSessionLandingBadge({
    ...input,
    provenanceChecked: false,
  });
  assert.equal(
    codingSessionSurfaceBadgeLabel(unchecked),
    "Gate running: just ci, signed as observed by Kettle provider",
  );
  assert.equal(
    unchecked.detail,
    `just ci: started 14:02 by the signer's clock. ${CODING_SESSION_LANDING_SIGNER_UNCHECKED}\n${NOTE}`,
  );
  assert.doesNotMatch(unchecked.detail, /provider's clock/);

  const checked = deriveCodingSessionLandingBadge({
    ...input,
    provenanceChecked: true,
  });
  assert.equal(
    codingSessionSurfaceBadgeLabel(checked),
    "Gate running: just ci, watched by Kettle provider",
  );
  assert.equal(
    checked.detail,
    `just ci: started 14:02 by the provider's clock.\n${NOTE}`,
  );
});

test("Landing from a ctx: reads provenanceChecked off the fold", () => {
  const read = (provenanceChecked) =>
    ctx({
      observations: {
        state: "read",
        isLoading: false,
        errorMessage: null,
        result: {
          fold: { provenanceChecked },
          signedAt: new Map(),
        },
        view: { gates: [] },
        assignmentsChecked: true,
        readAtMs: null,
      },
    });
  assert.equal(
    codingSessionSurfaceBadgeLabel(
      codingSessionLandingBadgeFromCtx(
        read(false),
        [running("just ci")],
        CLOCK,
      ),
    ),
    "Gate running: just ci, signed as observed by Kettle provider",
  );
  assert.equal(
    codingSessionSurfaceBadgeLabel(
      codingSessionLandingBadgeFromCtx(read(true), [running("just ci")], CLOCK),
    ),
    "Gate running: just ci, watched by Kettle provider",
  );
});

test("Diff: the baseline records what is there, so no clock decides unseen", () => {
  const transcript = [
    editItem("e1", "src/a.ts", "2026-10-04T09:00:00.000Z"),
    // Dated far in the future by a producer clock running ahead.
    editItem("e2", "src/b.ts", "2099-01-01T00:00:00.000Z"),
  ];
  const now = Date.parse("2026-10-04T10:00:00.000Z");
  const baseline = codingSessionDiffBaselineMarker(
    codingSessionNewestEditPerFile(transcript),
    now,
  );
  assert.deepEqual(baseline, {
    via: "baseline",
    atMs: now,
    seen: { "src/a.ts": "e1", "src/b.ts": "e2" },
  });
  // An edit a clock ahead dated after the marker is still not new.
  assert.deepEqual(
    codingSessionUnseenDiffFiles(
      codingSessionNewestEditPerFile(transcript),
      baseline,
    ),
    [],
  );
  // New edits dated before the marker by a clock behind still count: a new
  // file, and a new edit to a file the baseline recorded.
  const later = codingSessionNewestEditPerFile([
    ...transcript,
    editItem("e3", "src/c.ts", "2020-01-01T00:00:00.000Z"),
    editItem("e4", "src/a.ts", "2026-10-04T09:30:00.000Z"),
  ]);
  assert.deepEqual(codingSessionUnseenDiffFiles(later, baseline), [
    "src/a.ts",
    "src/c.ts",
  ]);
  // Opening Diff records the same way, under its own word.
  const opened = codingSessionDiffOpenedMarker(later, now + 1);
  assert.equal(opened.via, "opened");
  assert.deepEqual(codingSessionUnseenDiffFiles(later, opened), []);
});
