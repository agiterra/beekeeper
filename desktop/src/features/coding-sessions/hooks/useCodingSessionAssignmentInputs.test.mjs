import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import {
  codingSessionAssignmentInputObservations,
  codingSessionAssignmentInputStateFromStatus,
  useCodingSessionAssignmentInputs,
} from "./useCodingSessionAssignmentInputs.ts";

/**
 * What these pin is that the hook *reports and displays*, and does nothing
 * else: it never checks out anything, never decides which assignments need an
 * input, and never turns the host's silence into "nothing to do". The
 * sequence itself is the host's, and its own tests are in
 * `desktop/src-tauri/src/coding_sessions/assignment_establishment_tests.rs`.
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

after(() => dom.window.close());

const SESSION = "9f2c1d40-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const VERIFIER = "1".repeat(64);
const BUILDER = "3".repeat(64);
const ASSIGNMENT = "a".repeat(64);
const COMMIT = "d698c7773".padEnd(40, "a");
const OTHER_COMMIT = "ecd4336f4".padEnd(40, "b");

const SEAT_LABELS = new Map([
  [VERIFIER, "Vera"],
  [BUILDER, "Bob"],
]);

function assignment(overrides = {}) {
  return {
    assignmentId: ASSIGNMENT,
    assigneeActor: VERIFIER,
    assigneeRole: "verifier",
    baseSha: COMMIT,
    ...overrides,
  };
}

function record(overrides = {}) {
  return {
    assignmentId: ASSIGNMENT,
    sessionRef: SESSION,
    seatLabel: "Vera",
    commit: COMMIT,
    branch: "lane/verifier",
    path: "/Users/x/Code/repo-Vera",
    remote: null,
    outcome: "established",
    message: null,
    changes: null,
    attempts: 1,
    recordedAt: "2026-09-20T12:00:00Z",
    ...overrides,
  };
}

function status(overrides = {}) {
  return {
    assignmentId: ASSIGNMENT,
    disposition: "recorded",
    record: record(),
    ...overrides,
  };
}

/** Deps that record every host call, so the report budget is counted. */
function recordingDeps(behaviour = {}) {
  const calls = [];
  const answer = {
    observe: async () => ({ kind: "statuses", statuses: [status()] }),
    requeue: async () => ({ kind: "status", status: status() }),
    ...behaviour,
  };
  return {
    calls,
    deps: {
      observe: (assignments) => {
        calls.push({ call: "observe", assignments });
        return answer.observe(assignments);
      },
      requeue: (input) => {
        calls.push({ call: "requeue", ...input });
        return answer.requeue(input);
      },
    },
    observes: () => calls.filter((entry) => entry.call === "observe"),
    requeues: () => calls.filter((entry) => entry.call === "requeue"),
  };
}

async function mount(assignments, deps, extra = {}) {
  const { act, renderHook } = await import("@testing-library/react");
  let mounted;
  await act(async () => {
    mounted = renderHook(
      (props) =>
        useCodingSessionAssignmentInputs({
          assignments: props.assignments,
          deps,
          resolveSeatLabel: (actor) => SEAT_LABELS.get(actor) ?? null,
          sessionRef: SESSION,
          ...extra,
        }),
      { initialProps: { assignments } },
    );
  });
  return { act, mounted };
}

test("every assignment is reported once, with this computer's seat label", async () => {
  const recording = recordingDeps();
  const { mounted } = await mount(
    [assignment(), assignment({ assignmentId: "b".repeat(64) })],
    recording.deps,
  );

  assert.equal(recording.observes().length, 1);
  assert.deepEqual(recording.observes()[0].assignments, [
    {
      assignmentId: ASSIGNMENT,
      sessionRef: SESSION,
      seatLabel: "Vera",
      assigneeRole: "verifier",
      baseSha: COMMIT,
      branch: null,
    },
    {
      assignmentId: "b".repeat(64),
      sessionRef: SESSION,
      seatLabel: "Vera",
      assigneeRole: "verifier",
      baseSha: COMMIT,
      branch: null,
    },
  ]);
  assert.equal(
    mounted.result.current.states.get(ASSIGNMENT).kind,
    "established",
  );
  mounted.unmount();
});

test("a re-render with the same assignments reports nothing further", async () => {
  const recording = recordingDeps();
  const { act, mounted } = await mount([assignment()], recording.deps);
  assert.equal(recording.observes().length, 1);

  await act(async () => {
    mounted.rerender({ assignments: [assignment()] });
  });
  await act(async () => {
    mounted.rerender({ assignments: [assignment()] });
  });
  assert.equal(recording.observes().length, 1);
  mounted.unmount();
});

test("a new commit on the same assignment is a new report", async () => {
  const recording = recordingDeps();
  const { act, mounted } = await mount([assignment()], recording.deps);

  await act(async () => {
    mounted.rerender({ assignments: [assignment({ baseSha: OTHER_COMMIT })] });
  });
  assert.deepEqual(
    recording.observes().map((entry) => entry.assignments[0].baseSha),
    [COMMIT, OTHER_COMMIT],
  );
  mounted.unmount();
});

test("the hook never checks out anything: the only host calls are report and requeue", async () => {
  const recording = recordingDeps();
  const { act, mounted } = await mount([assignment()], recording.deps);
  await act(async () => {
    mounted.result.current.retry(ASSIGNMENT);
  });
  assert.deepEqual(
    [...new Set(recording.calls.map((entry) => entry.call))],
    ["observe", "requeue"],
  );
  mounted.unmount();
});

test("a person's retry requeues on the host and shows what came back", async () => {
  const recording = recordingDeps({
    observe: async () => ({
      kind: "statuses",
      statuses: [
        status({
          record: record({
            outcome: "dirty_tree",
            message: "the seat's worktree has 2 uncommitted change(s)",
            changes: 2,
          }),
        }),
      ],
    }),
  });
  const { act, mounted } = await mount([assignment()], recording.deps);
  assert.equal(mounted.result.current.states.get(ASSIGNMENT).kind, "refused");

  await act(async () => {
    mounted.result.current.retry(ASSIGNMENT);
  });
  assert.equal(recording.requeues().length, 1);
  assert.equal(
    mounted.result.current.states.get(ASSIGNMENT).kind,
    "established",
  );
  mounted.unmount();
});

test("a queued intent shows as queued, not as established or as nothing", async () => {
  const recording = recordingDeps({
    observe: async () => ({
      kind: "statuses",
      statuses: [
        status({ record: record({ outcome: "intended", attempts: 0 }) }),
      ],
    }),
  });
  const { mounted } = await mount([assignment()], recording.deps);
  const state = mounted.result.current.states.get(ASSIGNMENT);
  assert.equal(state.kind, "queued");
  assert.equal(state.commit, COMMIT);
  mounted.unmount();
});

test("an abandoned establishment is shown with the host's reason", async () => {
  const recording = recordingDeps({
    observe: async () => ({
      kind: "statuses",
      statuses: [
        status({
          record: record({
            outcome: "establish_abandoned",
            attempts: 2,
            message: "2 attempts were started and none finished",
          }),
        }),
      ],
    }),
  });
  const { mounted } = await mount([assignment()], recording.deps);
  const state = mounted.result.current.states.get(ASSIGNMENT);
  assert.equal(state.kind, "abandoned");
  assert.equal(state.message, "2 attempts were started and none finished");
  mounted.unmount();
});

test("a builder's row says nothing at all", async () => {
  const recording = recordingDeps({
    observe: async () => ({
      kind: "statuses",
      statuses: [status({ disposition: "not_required", record: null })],
    }),
  });
  const { mounted } = await mount(
    [assignment({ assigneeActor: BUILDER, assigneeRole: "builder" })],
    recording.deps,
  );
  assert.equal(mounted.result.current.states.size, 0);
  mounted.unmount();
});

test("a host that cannot answer is stated, and reported again on the next change", async () => {
  let answers = 0;
  const recording = recordingDeps({
    observe: async () => {
      answers += 1;
      return answers === 1
        ? { kind: "unavailable", message: "host went away" }
        : { kind: "statuses", statuses: [status()] };
    },
  });
  const { act, mounted } = await mount([assignment()], recording.deps);
  const state = mounted.result.current.states.get(ASSIGNMENT);
  assert.equal(state.kind, "unavailable");
  assert.equal(state.message, "host went away");

  // Un-made, not remembered as made: the next render reports again.
  await act(async () => {
    mounted.rerender({ assignments: [assignment()] });
  });
  assert.equal(recording.observes().length, 2);
  assert.equal(
    mounted.result.current.states.get(ASSIGNMENT).kind,
    "established",
  );
  mounted.unmount();
});

test("a session with no assignments calls the host not at all", async () => {
  const recording = recordingDeps();
  const { mounted } = await mount([], recording.deps);
  assert.deepEqual(recording.calls, []);
  assert.equal(mounted.result.current.states.size, 0);
  mounted.unmount();
});

test("without a session ref nothing is reported", async () => {
  const recording = recordingDeps();
  const { mounted } = await mount([assignment()], recording.deps, {
    sessionRef: null,
  });
  assert.deepEqual(recording.calls, []);
  mounted.unmount();
});

test("switching session drops the previous mission's answers", async () => {
  const recording = recordingDeps();
  const { act, renderHook } = await import("@testing-library/react");
  let mounted;
  await act(async () => {
    mounted = renderHook(
      (props) =>
        useCodingSessionAssignmentInputs({
          assignments: [assignment()],
          deps: recording.deps,
          resolveSeatLabel: (actor) => SEAT_LABELS.get(actor) ?? null,
          sessionRef: props.sessionRef,
        }),
      { initialProps: { sessionRef: SESSION } },
    );
  });
  assert.equal(mounted.result.current.states.size, 1);

  await act(async () => {
    mounted.rerender({ sessionRef: "other-session" });
  });
  assert.equal(
    recording.observes().at(-1).assignments[0].sessionRef,
    "other-session",
  );
  mounted.unmount();
});

test("the observation set is deduplicated and every role is reported", () => {
  const observations = codingSessionAssignmentInputObservations(
    SESSION,
    [
      assignment(),
      assignment(),
      assignment({
        assignmentId: "b".repeat(64),
        assigneeActor: BUILDER,
        assigneeRole: "builder",
      }),
      assignment({ assignmentId: "c".repeat(64), assigneeActor: null }),
    ],
    (actor) => SEAT_LABELS.get(actor) ?? null,
  );
  assert.deepEqual(
    observations.map((observation) => [
      observation.assignmentId,
      observation.assigneeRole,
      observation.seatLabel,
    ]),
    [
      [ASSIGNMENT, "verifier", "Vera"],
      ["b".repeat(64), "builder", "Bob"],
      ["c".repeat(64), "verifier", null],
    ],
  );
});

test("each disposition reads as exactly one row, and none as silence", () => {
  assert.equal(
    codingSessionAssignmentInputStateFromStatus(
      status({ disposition: "not_required", record: null }),
    ),
    null,
  );
  assert.deepEqual(
    codingSessionAssignmentInputStateFromStatus(
      status({ disposition: "unnamed", record: null }),
    ),
    { kind: "unnamed" },
  );
  const offHost = codingSessionAssignmentInputStateFromStatus(
    status({ disposition: "off_host", record: null }),
  );
  assert.equal(offHost.kind, "refused");
  assert.equal(offHost.code, "unrecorded_tree");
  assert.equal(
    codingSessionAssignmentInputStateFromStatus(
      status({ disposition: "invalid", record: null }),
    ).kind,
    "unavailable",
  );
  // A word this build does not know, and a record that never arrived: both
  // unanswerable, neither silent.
  assert.equal(
    codingSessionAssignmentInputStateFromStatus(
      status({ disposition: "unreadable", record: null }),
    ).kind,
    "unavailable",
  );
  assert.equal(
    codingSessionAssignmentInputStateFromStatus(
      status({ disposition: "recorded", record: null }),
    ).kind,
    "unavailable",
  );
});

test("an off-host answer for an ambiguously-attributed actor reads as unknown, not off-host", () => {
  const plain = codingSessionAssignmentInputStateFromStatus(
    status({ disposition: "off_host", record: null }),
    { unattributed: false },
  );
  assert.equal(plain.kind, "refused");
  assert.equal(plain.code, "unrecorded_tree");

  const ambiguous = codingSessionAssignmentInputStateFromStatus(
    status({ disposition: "off_host", record: null }),
    { unattributed: true },
  );
  assert.equal(ambiguous.kind, "unknown-attribution");
});

test("a lead-hired seat with no profile name still reads from the store, not off-host", async () => {
  // The real-world shape of the 2026-09-24 defect: `resolveSeatLabel` (built
  // from a relay profile lookup upstream) cannot name this actor, but the
  // caller can still say the session holds an unattributed legacy worktree —
  // so the row must not claim this host never cut the tree.
  const recording = recordingDeps({
    observe: async () => ({
      kind: "statuses",
      statuses: [status({ disposition: "off_host", record: null })],
    }),
  });
  const { mounted } = await mount([assignment()], recording.deps, {
    resolveSeatLabel: () => null,
    isUnattributedActor: () => true,
  });

  assert.equal(
    mounted.result.current.states.get(ASSIGNMENT).kind,
    "unknown-attribution",
  );
  mounted.unmount();
});
