import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionWaitingMsUntilQuiet,
  collectCodingSessionWaitingLines,
  deriveCodingSessionWaiting,
  formatCodingSessionWaitingHeadline,
} from "./codingSessionWaiting.ts";
import {
  CODING_SESSION_QUIET_AFTER_MS,
  codingSessionLiveShimmerActive,
  codingSessionMsUntilQuiet,
  isCodingSessionTranscriptFresh,
} from "./codingSessionWaitingLiveness.ts";

// SV-99: the waiting strip says what the agent waits on only from live
// evidence — a running subagent, a background task nothing says ended, a
// live gate start — and is absent otherwise. SV-104: live text shimmers only
// while live, fresh and with motion allowed.

const t0 = "2026-10-06T10:00:00.000Z";
const T0 = Date.parse(t0);
const TASK = "bi9cros3k";

const WORKING = { kind: "working", label: "Working" };
const IDLE = { kind: "idle", label: "Idle" };
const ENDED = { kind: "ended", label: "Ended" };

function prompt(id, turnId) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Brian",
    text: "Go",
    timestamp: t0,
    turnId,
  };
}

function tool(id, turnId, overrides = {}) {
  return {
    id,
    type: "tool",
    renderClass: "shell",
    descriptor: { renderClass: "shell", label: "Ran command", preview: "" },
    title: "Bash",
    toolName: "Bash",
    beekeeperToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp: t0,
    startedAt: t0,
    completedAt: t0,
    turnId,
    ...overrides,
  };
}

function subagentCall(id, turnId, description, overrides = {}) {
  return tool(id, turnId, {
    title: description,
    toolName: "Agent",
    toolCallId: `call-${id}`,
    status: "executing",
    completedAt: undefined,
    args: { description, prompt: "Look", subagent_type: "Explore" },
    ...overrides,
  });
}

function backgroundStart(id, turnId, command = "cargo test -p beekeeper-core") {
  return tool(id, turnId, {
    args: { command, run_in_background: true },
    result: `Command running in background with ID: ${TASK}. You will be notified when it completes.`,
  });
}

function execution(transcript, overrides = {}) {
  return {
    executionKey: "exec-1",
    label: "Lead",
    status: WORKING,
    generation: {
      generationId: "gen-1",
      status: "running",
      transcript,
      lastTranscriptAt: T0,
    },
    ...overrides,
  };
}

function gate(overrides = {}) {
  return {
    key: "author:cargo test:1",
    gate: "cargo test",
    startedAtMs: T0,
    authorPubkey: "author",
    eventId: "event",
    stale: false,
    notLive: null,
    ...overrides,
  };
}

test("an idle session with nothing running shows no strip", () => {
  const transcript = [prompt("p1", "turn-1"), tool("t1", "turn-1")];
  assert.equal(
    deriveCodingSessionWaiting({
      executions: [
        execution(transcript, {
          status: IDLE,
          generation: {
            generationId: "gen-1",
            status: "idle",
            transcript,
            lastTranscriptAt: T0,
          },
        }),
      ],
      runningGates: [],
      nowMs: T0 + 1_000,
    }),
    null,
  );
});

test("two running subagents read 'Waiting on 2 subagents' with the first title", () => {
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "D13 fast batch: owned lanes"),
    subagentCall("s2", "turn-1", "Second lane"),
  ];
  const waiting = deriveCodingSessionWaiting({
    executions: [execution(transcript)],
    runningGates: [],
    nowMs: T0 + 5_000,
  });
  assert.ok(waiting);
  assert.equal(waiting.headline, "Waiting on 2 subagents");
  assert.equal(waiting.brief, "D13 fast batch: owned lanes");
  assert.equal(waiting.subagents, 2);
  assert.equal(waiting.fresh, true);
  assert.equal(waiting.quietMs, null);
});

test("a subagent whose turn the provider no longer calls running is not waited on", () => {
  // The call never got a result, but the signed status is idle: the stream
  // reads it "Did not finish", so the strip says nothing.
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "Abandoned"),
  ];
  const lines = collectCodingSessionWaitingLines({
    executions: [
      execution(transcript, {
        status: IDLE,
        generation: {
          generationId: "gen-1",
          status: "idle",
          transcript,
          lastTranscriptAt: T0,
        },
      }),
    ],
    runningGates: [],
  });
  assert.deepEqual(lines, []);
});

test("a finished subagent is not waited on", () => {
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "Done already", {
      status: "completed",
      completedAt: t0,
      result: "All good",
    }),
  ];
  assert.equal(
    deriveCodingSessionWaiting({
      executions: [execution(transcript)],
      runningGates: [],
      nowMs: T0,
    }),
    null,
  );
});

test("a background task nothing says ended reads 'Waiting on a background task' with its command", () => {
  const transcript = [prompt("p1", "turn-1"), backgroundStart("b1", "turn-1")];
  const waiting = deriveCodingSessionWaiting({
    executions: [
      execution(transcript, {
        // The turn ended; the agent waits for the notification.
        status: IDLE,
        generation: {
          generationId: "gen-1",
          status: "idle",
          transcript,
          lastTranscriptAt: T0,
        },
      }),
    ],
    runningGates: [],
    nowMs: T0 + 10_000,
  });
  assert.ok(waiting);
  assert.equal(waiting.headline, "Waiting on a background task");
  assert.equal(waiting.brief, "cargo test -p beekeeper-core");
});

test("a background task that was reported, or whose session ended, is not waited on", () => {
  const reported = [
    prompt("p1", "turn-1"),
    backgroundStart("b1", "turn-1"),
    {
      ...prompt("n1", "turn-2"),
      text: `<task-notification>\n<task-id>${TASK}</task-id>\n<status>completed</status>\n</task-notification>`,
    },
  ];
  assert.deepEqual(
    collectCodingSessionWaitingLines({
      executions: [execution(reported)],
      runningGates: [],
    }),
    [],
  );
  const running = [prompt("p1", "turn-1"), backgroundStart("b1", "turn-1")];
  assert.deepEqual(
    collectCodingSessionWaitingLines({
      executions: [
        execution(running, {
          status: ENDED,
          generation: {
            generationId: "gen-1",
            status: "completed",
            transcript: running,
            lastTranscriptAt: T0,
          },
        }),
      ],
      runningGates: [],
    }),
    [],
  );
});

test("a live gate start reads 'Gate running'; a stale or snapshot one does not", () => {
  const waiting = deriveCodingSessionWaiting({
    executions: [],
    runningGates: [gate()],
    nowMs: T0,
  });
  assert.ok(waiting);
  assert.equal(waiting.headline, "Gate running");
  assert.equal(waiting.brief, "cargo test");
  assert.equal(waiting.fresh, true);
  assert.equal(
    deriveCodingSessionWaiting({
      executions: [],
      runningGates: [
        gate({ stale: true }),
        gate({
          key: "other",
          notLive: { short: "not live", sentence: "Not live." },
        }),
      ],
      nowMs: T0,
    }),
    null,
  );
});

test("a quiet provider holds the dot still and says how long", () => {
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "Slow lane"),
  ];
  const now = T0 + 4 * 60_000 + 30_000;
  const waiting = deriveCodingSessionWaiting({
    executions: [execution(transcript)],
    runningGates: [],
    nowMs: now,
  });
  assert.ok(waiting);
  assert.equal(waiting.fresh, false);
  assert.equal(waiting.quietMs, 4 * 60_000);
  // A fresh gate alongside keeps the dot honest-live, and claims no silence.
  const mixed = deriveCodingSessionWaiting({
    executions: [execution(transcript)],
    runningGates: [gate()],
    nowMs: now,
  });
  assert.equal(mixed.fresh, true);
  assert.equal(mixed.quietMs, null);
  assert.equal(mixed.headline, "Waiting on 1 subagent · Gate running");
});

test("an unknown last event time is neither fresh nor quiet", () => {
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "Lane"),
  ];
  const exec = execution(transcript);
  exec.generation.lastTranscriptAt = null;
  const waiting = deriveCodingSessionWaiting({
    executions: [exec],
    runningGates: [],
    nowMs: T0,
  });
  assert.equal(waiting.fresh, false);
  assert.equal(waiting.quietMs, null);
});

test("the strip's flip timer waits until the freshest line goes quiet", () => {
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "Lane"),
  ];
  const executions = [execution(transcript)];
  const lines = collectCodingSessionWaitingLines({
    executions,
    runningGates: [],
  });
  assert.equal(
    codingSessionWaitingMsUntilQuiet({ executions, lines, nowMs: T0 + 10_000 }),
    CODING_SESSION_QUIET_AFTER_MS - 10_000 + 1,
  );
  assert.equal(
    codingSessionWaitingMsUntilQuiet({
      executions,
      lines,
      nowMs: T0 + CODING_SESSION_QUIET_AFTER_MS + 1,
    }),
    null,
  );
});

test("headline wording for each combination", () => {
  assert.equal(
    formatCodingSessionWaitingHeadline({
      subagents: 0,
      backgroundTasks: 0,
      gates: 0,
    }),
    null,
  );
  assert.equal(
    formatCodingSessionWaitingHeadline({
      subagents: 2,
      backgroundTasks: 1,
      gates: 0,
    }),
    "Waiting on 2 subagents and a background task",
  );
  assert.equal(
    formatCodingSessionWaitingHeadline({
      subagents: 0,
      backgroundTasks: 3,
      gates: 2,
    }),
    "Waiting on 3 background tasks · 2 gates running",
  );
});

test("shimmer is on only while live, fresh and with motion allowed", () => {
  const base = {
    live: true,
    lastEventAt: T0,
    now: T0 + 1_000,
    reducedMotion: false,
  };
  assert.equal(codingSessionLiveShimmerActive(base), true);
  assert.equal(codingSessionLiveShimmerActive({ ...base, live: false }), false);
  assert.equal(
    codingSessionLiveShimmerActive({ ...base, reducedMotion: true }),
    false,
  );
  assert.equal(
    codingSessionLiveShimmerActive({
      ...base,
      now: T0 + CODING_SESSION_QUIET_AFTER_MS + 1,
    }),
    false,
  );
  assert.equal(
    codingSessionLiveShimmerActive({ ...base, lastEventAt: null }),
    false,
  );
  assert.equal(
    isCodingSessionTranscriptFresh(T0, T0 + CODING_SESSION_QUIET_AFTER_MS),
    true,
  );
  assert.equal(
    codingSessionMsUntilQuiet(T0, T0),
    CODING_SESSION_QUIET_AFTER_MS + 1,
  );
  assert.equal(codingSessionMsUntilQuiet(null, T0), null);
});

test("a seat demoted for reachability vouches for no running subagent", () => {
  const transcript = [
    prompt("p1", "turn-1"),
    subagentCall("s1", "turn-1", "Lane on a gone provider"),
  ];
  assert.deepEqual(
    collectCodingSessionWaitingLines({
      executions: [
        execution(transcript, {
          status: { kind: "unknown", label: "Unreachable", attention: true },
        }),
      ],
      runningGates: [],
    }),
    [],
  );
});
