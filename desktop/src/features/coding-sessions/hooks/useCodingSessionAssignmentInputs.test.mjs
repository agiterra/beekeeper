import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { useCodingSessionAssignmentInputs } from "./useCodingSessionAssignmentInputs.ts";

/**
 * What these pin is the trigger's *budget and restraint*: one attempt per
 * (assignment, commit), in order, never two at once, never a retry this host
 * decided on its own, and nothing at all from a computer that cut no tree.
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
const RUNNER = "2".repeat(64);
const BUILDER = "3".repeat(64);
const COMMIT = "d698c7773".padEnd(40, "a");
const OTHER_COMMIT = "ecd4336f4".padEnd(40, "b");

function worktree(seatLabel) {
  return {
    key: `${SESSION}/${seatLabel}`,
    sessionRef: SESSION,
    seatLabel,
    path: `/Users/x/Code/repo-${seatLabel}`,
    branch: `wt-${seatLabel}`,
    repoRoot: "/Users/x/Code/repo",
    disposition: "held",
    dirtyFiles: 0,
    reclaimableBytes: null,
    reclaimableLabel: "unknown",
    reclaimableNow: false,
    graceRemainingSecs: null,
    exists: true,
    tipOnRelayKnown: true,
    detail: "Held.",
  };
}

function established(commit, path) {
  return {
    kind: "established",
    result: {
      path,
      branch: "main",
      commit,
      remote: "origin",
      alreadyCurrent: false,
    },
  };
}

/** Deps that record every host call, so concurrency is counted, not claimed. */
function recordingDeps(behaviour = {}) {
  const calls = [];
  let live = 0;
  let maxLive = 0;
  const answer = {
    listSeatWorktrees: async () => [worktree("Vera"), worktree("Rhea")],
    readRecord: async () => ({ kind: "none" }),
    establish: async (request) =>
      established(request.commit, `/Users/x/Code/repo-${request.seatLabel}`),
    ...behaviour,
  };
  const deps = {
    listSeatWorktrees: (sessions) => {
      calls.push({ call: "listSeatWorktrees", sessions });
      return answer.listSeatWorktrees(sessions);
    },
    readRecord: (request) => {
      calls.push({ call: "readRecord", ...request });
      return answer.readRecord(request);
    },
    establish: async (request) => {
      live += 1;
      maxLive = Math.max(maxLive, live);
      calls.push({ call: "establish", ...request });
      try {
        return await answer.establish(request);
      } finally {
        live -= 1;
      }
    },
  };
  return {
    calls,
    deps,
    establishes: () => calls.filter((entry) => entry.call === "establish"),
    maxLive: () => maxLive,
  };
}

const SEAT_LABELS = new Map([
  [VERIFIER, "Vera"],
  [RUNNER, "Rhea"],
  [BUILDER, "Bob"],
]);

function assignment(overrides) {
  return {
    assignmentId: "a".repeat(64),
    assigneeActor: VERIFIER,
    assigneeRole: "verifier",
    baseSha: COMMIT,
    ...overrides,
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
          enabled: true,
          resolveSeatLabel: (actor) => SEAT_LABELS.get(actor) ?? null,
          sessionRef: SESSION,
          ...extra,
        }),
      { initialProps: { assignments } },
    );
  });
  return { act, mounted };
}

test("a verifier assignment is established once, in its seat's recorded tree", async () => {
  const recording = recordingDeps();
  const { mounted } = await mount([assignment({})], recording.deps);

  assert.deepEqual(
    recording.establishes().map((entry) => ({
      seatLabel: entry.seatLabel,
      commit: entry.commit,
      sessionRef: entry.sessionRef,
    })),
    [{ seatLabel: "Vera", commit: COMMIT, sessionRef: SESSION }],
  );
  const state = mounted.result.current.states.get("a".repeat(64));
  assert.equal(state.kind, "established");
  assert.equal(state.result.path, "/Users/x/Code/repo-Vera");
  mounted.unmount();
});

test("a re-render with the same assignments establishes nothing further", async () => {
  const recording = recordingDeps();
  const { act, mounted } = await mount([assignment({})], recording.deps);
  assert.equal(recording.establishes().length, 1);

  await act(async () => {
    mounted.rerender({ assignments: [assignment({})] });
  });
  assert.equal(recording.establishes().length, 1);
  mounted.unmount();
});

test("a new commit on the same assignment is a new attempt", async () => {
  const recording = recordingDeps();
  const { act, mounted } = await mount([assignment({})], recording.deps);

  await act(async () => {
    mounted.rerender({ assignments: [assignment({ baseSha: OTHER_COMMIT })] });
  });
  assert.deepEqual(
    recording.establishes().map((entry) => entry.commit),
    [COMMIT, OTHER_COMMIT],
  );
  mounted.unmount();
});

test("a builder assignment is skipped and says nothing", async () => {
  const recording = recordingDeps();
  const { mounted } = await mount(
    [
      assignment({
        assignmentId: "b".repeat(64),
        assigneeActor: BUILDER,
        assigneeRole: "builder",
      }),
    ],
    recording.deps,
  );
  assert.equal(recording.establishes().length, 0);
  assert.equal(mounted.result.current.states.size, 0);
  mounted.unmount();
});

test("a verifier assignment that names no commit is stated, not attempted", async () => {
  const recording = recordingDeps();
  const { mounted } = await mount(
    [assignment({ baseSha: null })],
    recording.deps,
  );
  assert.equal(recording.establishes().length, 0);
  assert.deepEqual(recording.calls, []);
  assert.deepEqual(mounted.result.current.states.get("a".repeat(64)), {
    kind: "unnamed",
  });
  mounted.unmount();
});

test("a seat this computer never cut is disclosed without calling the host", async () => {
  const recording = recordingDeps({
    listSeatWorktrees: async () => [worktree("Someone-Else")],
  });
  const { mounted } = await mount([assignment({})], recording.deps);

  assert.equal(recording.establishes().length, 0);
  const state = mounted.result.current.states.get("a".repeat(64));
  assert.equal(state.kind, "refused");
  assert.equal(state.code, "unrecorded_tree");
  mounted.unmount();
});

test("a refusal is never retried on its own, and a person's retry re-invokes", async () => {
  let attempts = 0;
  const recording = recordingDeps({
    establish: async (request) => {
      attempts += 1;
      return attempts === 1
        ? {
            kind: "refused",
            code: "dirty_tree",
            message: "dirty",
            detail: "M a.txt",
            changes: 1,
          }
        : established(request.commit, "/Users/x/Code/repo-Vera");
    },
  });
  const { act, mounted } = await mount([assignment({})], recording.deps);
  assert.equal(recording.establishes().length, 1);
  assert.equal(
    mounted.result.current.states.get("a".repeat(64)).kind,
    "refused",
  );

  // Several renders later it is still one attempt: nothing here retries.
  await act(async () => {
    mounted.rerender({ assignments: [assignment({})] });
  });
  await act(async () => {
    mounted.rerender({ assignments: [assignment({})] });
  });
  assert.equal(recording.establishes().length, 1);

  await act(async () => {
    mounted.result.current.retry("a".repeat(64));
  });
  assert.equal(recording.establishes().length, 2);
  assert.equal(
    mounted.result.current.states.get("a".repeat(64)).kind,
    "established",
  );
  mounted.unmount();
});

test("two seats are established in assignment order, never at the same time", async () => {
  const gates = [];
  const recording = recordingDeps({
    establish: (request) =>
      new Promise((resolve) => {
        gates.push(() =>
          resolve(
            established(
              request.commit,
              `/Users/x/Code/repo-${request.seatLabel}`,
            ),
          ),
        );
      }),
  });
  const assignments = [
    assignment({ assignmentId: "a".repeat(64) }),
    assignment({
      assignmentId: "c".repeat(64),
      assigneeActor: RUNNER,
      assigneeRole: "runner",
    }),
  ];
  const { act, mounted } = await mount(assignments, recording.deps);

  // The queue is serial: the second seat has not been touched yet.
  assert.deepEqual(
    recording.establishes().map((entry) => entry.seatLabel),
    ["Vera"],
  );
  await act(async () => {
    gates.shift()();
  });
  assert.deepEqual(
    recording.establishes().map((entry) => entry.seatLabel),
    ["Vera", "Rhea"],
  );
  await act(async () => {
    gates.shift()();
  });
  assert.equal(recording.maxLive(), 1);
  mounted.unmount();
});

test("the surface being closed runs nothing at all", async () => {
  const recording = recordingDeps();
  const { act, renderHook } = await import("@testing-library/react");
  let mounted;
  await act(async () => {
    mounted = renderHook(() =>
      useCodingSessionAssignmentInputs({
        assignments: [assignment({})],
        deps: recording.deps,
        enabled: false,
        resolveSeatLabel: (actor) => SEAT_LABELS.get(actor) ?? null,
        sessionRef: SESSION,
      }),
    );
  });
  assert.deepEqual(recording.calls, []);
  assert.equal(mounted.result.current.states.size, 0);
  mounted.unmount();
});

test("a recorded attempt for this commit shows on mount without establishing again", async () => {
  const recording = recordingDeps({
    readRecord: async () => ({
      kind: "record",
      record: {
        assignmentId: "a".repeat(64),
        sessionRef: SESSION,
        seatLabel: "Vera",
        commit: COMMIT,
        branch: "main",
        path: "/Users/x/Code/repo-Vera",
        remote: "origin",
        outcome: "established",
        message: null,
        changes: null,
        recordedAt: 1_800_000_000,
      },
    }),
  });
  const { mounted } = await mount([assignment({})], recording.deps);

  assert.equal(recording.establishes().length, 0);
  const state = mounted.result.current.states.get("a".repeat(64));
  assert.equal(state.kind, "recorded");
  assert.equal(state.inner.kind, "established");
  assert.equal(state.inner.result.path, "/Users/x/Code/repo-Vera");
  mounted.unmount();
});

test("a person's retry bypasses the record and reaches the host", async () => {
  const recording = recordingDeps({
    readRecord: async () => ({
      kind: "record",
      record: {
        assignmentId: "a".repeat(64),
        sessionRef: SESSION,
        seatLabel: "Vera",
        commit: COMMIT,
        branch: "main",
        path: "/Users/x/Code/repo-Vera",
        remote: "origin",
        outcome: "dirty_tree",
        message: "dirty",
        changes: 2,
        recordedAt: 1_800_000_000,
      },
    }),
  });
  const { act, mounted } = await mount([assignment({})], recording.deps);
  assert.equal(recording.establishes().length, 0);
  assert.equal(
    mounted.result.current.states.get("a".repeat(64)).inner.kind,
    "refused",
  );

  await act(async () => {
    mounted.result.current.retry("a".repeat(64));
  });
  assert.equal(recording.establishes().length, 1);
  assert.equal(
    mounted.result.current.states.get("a".repeat(64)).kind,
    "established",
  );
  mounted.unmount();
});

test("a record for a different commit is never shown as this commit's answer", async () => {
  const gates = [];
  const recording = recordingDeps({
    readRecord: async () => ({
      kind: "record",
      record: {
        assignmentId: "a".repeat(64),
        sessionRef: SESSION,
        seatLabel: "Vera",
        commit: OTHER_COMMIT,
        branch: "main",
        path: "/Users/x/Code/repo-Vera",
        remote: "origin",
        outcome: "established",
        message: null,
        changes: null,
        recordedAt: 1_800_000_000,
      },
    }),
    establish: (request) =>
      new Promise((resolve) => {
        gates.push(() =>
          resolve(established(request.commit, "/Users/x/Code/repo-Vera")),
        );
      }),
  });
  const { act, mounted } = await mount([assignment({})], recording.deps);

  // The stale record neither suppressed the attempt nor stood in for it.
  const state = mounted.result.current.states.get("a".repeat(64));
  assert.equal(state.kind, "stale-record");
  assert.equal(state.commit, COMMIT);
  assert.equal(state.recordedCommit, OTHER_COMMIT);
  assert.equal(recording.establishes().length, 1);

  await act(async () => {
    gates.shift()();
  });
  const settled = mounted.result.current.states.get("a".repeat(64));
  assert.equal(settled.kind, "established");
  assert.equal(settled.result.commit, COMMIT);
  mounted.unmount();
});

test("a seat read that fails is unavailable, never 'this computer cut no tree'", async () => {
  const recording = recordingDeps({
    listSeatWorktrees: async () => {
      throw new Error("host went away");
    },
  });
  const { mounted } = await mount([assignment({})], recording.deps);
  const state = mounted.result.current.states.get("a".repeat(64));
  assert.equal(state.kind, "unavailable");
  assert.equal(recording.establishes().length, 0);
  mounted.unmount();
});
