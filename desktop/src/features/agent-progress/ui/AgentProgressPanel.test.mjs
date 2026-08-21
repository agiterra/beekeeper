import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const NOW = 1_785_513_037;

function generation(overrides = {}) {
  return {
    targetKey: "target-1",
    executionKey: "execution-1",
    providerAuthorityPubkey: "a".repeat(64),
    current: true,
    reachability: "unverified",
    status: "running",
    statusAt: NOW - 60,
    branch: null,
    observedCommit: null,
    dirty: null,
    relayReachable: null,
    verifiedAt: null,
    commitConfirmation: "Commit not checked",
    leaseState: null,
    leaseIssuedAt: null,
    leaseAcceptedAt: null,
    leaseExpiresAt: null,
    leaseSigner: null,
    leaseSourceEventId: null,
    leaseSequence: null,
    lifecycleCommandEventId: "cmd-1",
    lifecycleReceiptEventId: "rcp-1",
    sourceEventIds: [],
    ...overrides,
  };
}

function session(overrides = {}) {
  return {
    sessionKey: "session-1",
    sessionRef: "session-1",
    name: "Pulse plumbing",
    goal: null,
    lifecycle: "open",
    coordinationState: "open_unverified",
    latestObservationAt: NOW - 60,
    observedAgeSeconds: 60,
    generations: [generation()],
    sourceEventIds: [],
    ...overrides,
  };
}

function reachableSession(overrides = {}) {
  return session({
    coordinationState: "provider_reachable",
    generations: [
      generation({
        reachability: "provider_reachable",
        leaseState: "live",
        leaseExpiresAt: NOW + 90,
      }),
    ],
    ...overrides,
  });
}

function tool(name, overrides = {}) {
  return {
    id: `tool-${name}`,
    type: "tool",
    renderClass: "shell",
    descriptor: { renderClass: "shell", label: name, preview: null },
    title: name,
    toolName: name,
    buzzToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp: new Date(NOW * 1_000 - 30_000).toISOString(),
    startedAt: new Date(NOW * 1_000 - 31_000).toISOString(),
    completedAt: new Date(NOW * 1_000 - 30_000).toISOString(),
    ...overrides,
  };
}

function detailFor(sessionRef, transcript, overrides = {}) {
  return [
    sessionRef,
    {
      sessionRef,
      label: null,
      runtimeLabel: "Claude Code",
      transcript,
      openTarget: { channelId: "chan-1", generationId: "gen-1" },
      ...overrides,
    },
  ];
}

async function renderPanel(sessions, options = {}) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { foldAgentProgress } = await import(
    "@/features/agent-progress/lib/agentProgressFold"
  );
  const { AgentProgressPanel } = await import(
    "@/features/agent-progress/ui/AgentProgressPanel"
  );
  const complete = options.complete ?? true;
  const ambiguities = options.ambiguities ?? [];
  const model = foldAgentProgress({
    sessions,
    channelsBySession: new Map(
      sessions.map((row) => [row.sessionKey, ["chan-1"]]),
    ),
    detailBySessionRef: new Map(options.details ?? []),
    nowSeconds: NOW,
    complete,
    ambiguities,
  });
  return render(
    createElement(AgentProgressPanel, {
      aggregate: model.aggregate,
      ambiguities,
      complete,
      errors: options.errors ?? [],
      isLoading: options.isLoading ?? false,
      lanes: model.lanes,
      onOpenLane: options.onOpenLane,
    }),
  );
}

test("a reachable lane says Reachable and names its status without hedging", async () => {
  const { getByTestId } = await renderPanel([reachableSession()], {
    details: [detailFor("session-1", [tool("Bash")])],
  });
  const lane = getByTestId("agent-progress-lane");
  assert.equal(lane.dataset.laneCoordination, "provider_reachable");
  assert.equal(
    getByTestId("agent-progress-lane-coordination").textContent,
    "Reachable",
  );
  assert.doesNotMatch(
    getByTestId("agent-progress-lane-reported").textContent,
    /last reported/,
  );
  assert.equal(
    getByTestId("agent-progress-lane-activity").textContent,
    "▸ Bash",
  );
});

/**
 * The lie this rewrite removes. A session whose machine died mid-turn keeps a
 * signed `running` status forever; without a lease the row must say the status
 * was *reported*, and must never present it as a live condition.
 */
test("an unverified lane never states a live status, however recent the report", async () => {
  const { getByTestId } = await renderPanel([session()]);
  const lane = getByTestId("agent-progress-lane");
  assert.equal(lane.dataset.laneCoordination, "open_unverified");
  assert.equal(
    getByTestId("agent-progress-lane-coordination").textContent,
    "Unverified",
  );
  assert.match(
    getByTestId("agent-progress-lane-reported").textContent,
    /last reported/,
  );
  assert.doesNotMatch(lane.textContent, /Reachable/);
});

/** Coordination and reported status are separate elements, not one string. */
test("the two axes are rendered separately and neither substitutes for the other", async () => {
  const { getByTestId } = await renderPanel([reachableSession()]);
  const coordination = getByTestId("agent-progress-lane-coordination");
  const reported = getByTestId("agent-progress-lane-reported");
  assert.notEqual(coordination, reported);
  // Working / Stale / Ended are the retired vocabulary; they must not return
  // as a substitute for a coordination state.
  assert.doesNotMatch(coordination.textContent, /Working|Stale|Ended/);
});

test("closure outranks a live lease on screen, as it does in the fold", async () => {
  const { getByTestId } = await renderPanel([
    session({
      lifecycle: "closed",
      coordinationState: "closed",
      generations: [
        generation({ reachability: "provider_reachable", leaseState: "live" }),
      ],
    }),
  ]);
  assert.equal(
    getByTestId("agent-progress-lane-coordination").textContent,
    "Closed",
  );
});

test("a lane with several executions discloses the count beside the name", async () => {
  const { getByTestId } = await renderPanel([
    session({
      generations: [
        generation({ targetKey: "t1", executionKey: "execution-a" }),
        generation({
          targetKey: "t2",
          executionKey: "execution-a",
          current: false,
        }),
        generation({
          targetKey: "t3",
          executionKey: "execution-b",
          current: false,
        }),
      ],
    }),
  ]);
  assert.equal(getByTestId("agent-progress-lane").dataset.laneExecutions, "2");
  assert.equal(
    getByTestId("agent-progress-lane-executions").textContent,
    "2 exec",
  );
  assert.equal(
    getByTestId("agent-progress-lane-executions").title,
    "Distinct provider executions nested inside this one durable session.",
  );
});

test("every lane row is the same fixed height, whatever it carries", async () => {
  const { getAllByTestId } = await renderPanel(
    [
      reachableSession({ sessionKey: "a", sessionRef: "a" }),
      session({
        sessionKey: "b",
        sessionRef: "b",
        lifecycle: "closed",
        coordinationState: "closed",
        generations: [
          generation({ targetKey: "t1" }),
          generation({ targetKey: "t2", current: false }),
        ],
      }),
    ],
    { details: [detailFor("a", [tool("Bash")])] },
  );
  const rows = getAllByTestId("agent-progress-lane");
  assert.equal(rows.length, 2);
  assert.deepEqual(
    [...new Set(rows.map((row) => /h-14(\s|$)/.test(row.className)))],
    [true],
  );
});

test("loading, incomplete and confirmed-empty are three different answers", async () => {
  const loading = await renderPanel([], { isLoading: true, complete: false });
  assert.ok(loading.getByTestId("agent-progress-loading"));
  assert.equal(loading.queryByTestId("agent-progress-empty"), null);
  assert.equal(loading.queryByTestId("agent-progress-incomplete"), null);
  loading.unmount();

  const incomplete = await renderPanel([], {
    complete: false,
    errors: [{ scope: "leases", message: "relay unreachable" }],
  });
  const notice = incomplete.getByTestId("agent-progress-incomplete");
  assert.match(notice.textContent, /relay unreachable/);
  assert.match(notice.textContent, /not\s+a claim that nothing is running/);
  incomplete.unmount();

  const empty = await renderPanel([]);
  assert.equal(
    empty.getByTestId("agent-progress-empty").textContent,
    "No sessions in what this read returned.",
  );
  // Never "No agents": this read only ever covered channels this viewer reads.
  assert.doesNotMatch(
    empty.getByTestId("agent-progress-panel").textContent,
    /No agents/,
  );
});

test("evidence the fold refused to resolve is disclosed, not swallowed", async () => {
  const { getByTestId } = await renderPanel([session()], {
    ambiguities: [
      { scope: "lease", message: "two distinct leases at sequence 4" },
    ],
  });
  assert.match(
    getByTestId("agent-progress-ambiguous").textContent,
    /two distinct leases at sequence 4/,
  );
  assert.match(
    getByTestId("agent-progress-footer-counts").textContent,
    /^At least 1 session/,
  );
});

test("the footer counts sessions, never the executions inside them", async () => {
  const { getByTestId } = await renderPanel([
    reachableSession({ sessionKey: "a", sessionRef: "a" }),
    session({
      sessionKey: "b",
      sessionRef: "b",
      generations: [
        generation({ targetKey: "t1", executionKey: "execution-a" }),
        generation({
          targetKey: "t2",
          executionKey: "execution-a",
          current: false,
        }),
        generation({
          targetKey: "t3",
          executionKey: "execution-b",
          current: false,
        }),
      ],
    }),
  ]);
  assert.equal(
    getByTestId("agent-progress-footer-counts").textContent,
    "2 sessions · 1 reachable · 1 unverified",
  );
  assert.equal(
    getByTestId("agent-progress-footer-executions").textContent,
    "3 executions",
  );
});

test("a partial read's footer is a floor and says so", async () => {
  const { getByTestId } = await renderPanel([session()], {
    complete: false,
    errors: [{ scope: "sessions", message: "session read truncated" }],
  });
  assert.match(
    getByTestId("agent-progress-footer-counts").textContent,
    /^At least 1 session/,
  );
});

test("an empty footer says so instead of showing invented totals", async () => {
  const { getByTestId } = await renderPanel([]);
  assert.equal(
    getByTestId("agent-progress-footer-counts").textContent,
    "No sessions in what this read returned",
  );
  assert.doesNotMatch(getByTestId("agent-progress-footer").textContent, /tok/);
});

test("a lane with no local transcript is honest rather than blank or clickable", async () => {
  const opened = [];
  const { getByTestId } = await renderPanel([session()], {
    onOpenLane: (lane) => opened.push(lane),
  });
  assert.equal(
    getByTestId("agent-progress-lane-activity").textContent,
    "No activity in what this read returned",
  );
  const button = getByTestId("agent-progress-lane").querySelector("button");
  assert.equal(button.disabled, true);
  assert.equal(opened.length, 0);
});
