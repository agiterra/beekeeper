import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import {
  agentProgressLaneId,
  foldAgentProgress,
  summarizeAgentLaneActivity,
} from "@/features/agent-progress/lib/agentProgressFold";
import {
  agentLaneCoordinationText,
  agentLaneReportedText,
  agentProgressFooterText,
} from "@/features/agent-progress/lib/agentProgressFormat";
import { foldSessionCoordination } from "@/shared/coordination/sessionCoordinationFold";
import { coordinationReportedStatus } from "@/shared/coordination/sessionCoordinationFormat";

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

function fold(sessions, options = {}) {
  return foldAgentProgress({
    sessions,
    channelsBySession: new Map(
      sessions.map((row) => [row.sessionKey, ["chan-1"]]),
    ),
    detailBySessionRef: options.detailBySessionRef ?? new Map(),
    nowSeconds: NOW,
    complete: options.complete ?? true,
    ambiguities: options.ambiguities ?? [],
  });
}

function toolItem(overrides = {}) {
  return {
    id: "t1",
    type: "tool",
    renderClass: "shell",
    descriptor: { renderClass: "shell", label: "Bash", preview: null },
    title: "Bash",
    toolName: "Bash",
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

function errorItem(overrides = {}) {
  return {
    id: "e1",
    type: "lifecycle",
    renderClass: "error",
    title: "Turn failed",
    text: "provider exited with status 1",
    timestamp: new Date(NOW * 1_000 - 20_000).toISOString(),
    ...overrides,
  };
}

/**
 * The defect this rewrite exists to remove. The old fold owned a 30-minute
 * `AGENT_PROGRESS_ACTIVE_WINDOW_SECONDS` and decided liveness with it; the
 * lease work had already established that metadata recency is history. If the
 * constant ever comes back, this fails before anything ships.
 */
test("the fold owns no freshness window and no second liveness rule", async () => {
  const module = await import(
    "@/features/agent-progress/lib/agentProgressFold"
  );
  for (const name of Object.keys(module)) {
    assert.doesNotMatch(
      name,
      /ACTIVE_WINDOW|IsFresh|Freshness/i,
      `${name} re-introduces a second liveness rule`,
    );
  }
});

test("the feature stays behind a hard preview gate and depends only on shared seams", async () => {
  const featureFiles = [
    "src/features/agent-progress/lib/agentProgressCoordination.ts",
    "src/features/agent-progress/lib/agentProgressFold.ts",
    "src/features/agent-progress/lib/agentProgressFormat.ts",
    "src/features/agent-progress/lib/agentProgressSources.ts",
    "src/features/agent-progress/ui/AgentProgressScreen.tsx",
  ];
  for (const file of featureFiles) {
    const source = await readFile(
      new URL(`../../../../${file}`, import.meta.url),
      "utf8",
    );
    assert.doesNotMatch(
      source,
      /from\s+["']@\/features\//,
      `${file} reaches into a sibling feature instead of a shared or app seam`,
    );
  }

  // Agent progress is a Dashboard tab; the Dashboard route decides whether
  // the tab exists at all from the preview flag, and the legacy path only
  // redirects — neither renders the surface with the flag off.
  const route = await readFile(
    new URL("../../../../src/app/routes/index.tsx", import.meta.url),
    "utf8",
  );
  assert.match(route, /useFeatureEnabled\("agent-progress"\)/);
  assert.match(route, /resolveDashboardTab\(/);
  const legacyRoute = await readFile(
    new URL("../../../../src/app/routes/agent-progress.tsx", import.meta.url),
    "utf8",
  );
  assert.match(legacyRoute, /redirect\(/);
  assert.doesNotMatch(legacyRoute, /AgentProgressScreen/);
});

test("coordination comes from the shared fold verbatim, never recomputed", () => {
  const { lanes } = fold([
    session({ sessionKey: "a", sessionRef: "a", coordinationState: "closed" }),
    session({
      sessionKey: "b",
      sessionRef: "b",
      coordinationState: "provider_reachable",
      generations: [
        generation({
          reachability: "provider_reachable",
          leaseExpiresAt: NOW + 90,
        }),
      ],
    }),
    session({ sessionKey: "c", sessionRef: "c" }),
  ]);
  assert.deepEqual(
    lanes.map((lane) => [lane.sessionKey, lane.coordination]),
    [
      ["b", "provider_reachable"],
      ["c", "open_unverified"],
      ["a", "closed"],
    ],
  );
});

/**
 * A session that reported `running` two days ago, with no lease. The old fold
 * would have called this `stale`; the honest answer names both axes without
 * letting either impersonate the other.
 */
test("an ancient report on an unverified session says so on both axes", () => {
  const { lanes } = fold([
    session({
      latestObservationAt: NOW - 172_800,
      observedAgeSeconds: 172_800,
      generations: [generation({ status: "running", statusAt: NOW - 172_800 })],
    }),
  ]);
  const [lane] = lanes;
  assert.equal(lane.coordination, "open_unverified");
  assert.equal(lane.reportedStatus, "running");
  assert.equal(lane.reportedAgeSeconds, 172_800);
  assert.equal(agentLaneCoordinationText(lane), "Unverified");
  assert.match(agentLaneReportedText(lane), /^last reported .* 2d ago$/);
});

/** A live lease is the only thing that lets a row state a status plainly. */
test("a reachable row states its status; an unverified one reports it", () => {
  const reachable = fold([
    session({
      coordinationState: "provider_reachable",
      generations: [
        generation({
          status: "running",
          reachability: "provider_reachable",
          leaseExpiresAt: NOW + 90,
        }),
      ],
    }),
  ]).lanes[0];
  assert.equal(agentLaneCoordinationText(reachable), "Reachable");
  assert.doesNotMatch(agentLaneReportedText(reachable), /last reported/);
  assert.equal(reachable.leaseExpiresAt, NOW + 90);

  const unverified = fold([session()]).lanes[0];
  assert.match(agentLaneReportedText(unverified), /^last reported/);
  assert.equal(unverified.leaseExpiresAt, null);
});

test("every provider status maps explicitly without inventing liveness", () => {
  assert.deepEqual(
    [
      "starting",
      "running",
      "waiting_for_input",
      "idle",
      "completed",
      "stopped",
      "interrupted",
      "failed",
      "disconnected",
      "unknown",
      null,
    ].map((status) => coordinationReportedStatus(status).label),
    [
      "Working",
      "Working",
      "Working",
      "Idle",
      "Completed",
      "Stopped",
      "Stopped",
      "Needs attention",
      "Disconnected",
      "Status unknown",
      "Status unknown",
    ],
  );
});

/** Closure outranks a live lease — the shared fold's rule, honoured here. */
test("a closed session reads Closed even holding a live lease", () => {
  const [lane] = fold([
    session({
      lifecycle: "closed",
      coordinationState: "closed",
      generations: [
        generation({ reachability: "provider_reachable", leaseState: "live" }),
      ],
    }),
  ]).lanes;
  assert.equal(agentLaneCoordinationText(lane), "Closed");
});

test("resumed generations count once per execution key", () => {
  const { lanes, aggregate } = fold([
    session({
      sessionKey: "multi",
      sessionRef: "multi",
      generations: [
        generation({
          targetKey: "t1",
          executionKey: "execution-a",
          current: false,
        }),
        generation({
          targetKey: "t2",
          executionKey: "execution-a",
          current: true,
        }),
        generation({
          targetKey: "t3",
          executionKey: "execution-b",
          current: true,
        }),
      ],
    }),
    session({ sessionKey: "solo", sessionRef: "solo" }),
  ]);
  assert.deepEqual(
    lanes.map((lane) => lane.executionCount),
    [2, 1],
  );
  assert.equal(aggregate.sessions, 2);
  assert.equal(aggregate.executions, 3);
});

test("an incomplete read reports a floor, never a census", () => {
  const partial = fold([session()], { complete: false });
  assert.equal(partial.aggregate.atLeast, true);
  assert.match(
    agentProgressFooterText(partial.aggregate),
    /^At least 1 session/,
  );

  const complete = fold([session()]);
  assert.equal(complete.aggregate.atLeast, false);
  assert.match(agentProgressFooterText(complete.aggregate), /^1 session/);
});

/** Evidence the fold refused to resolve is a hole exactly like a failed query. */
test("unresolved evidence makes the counts a floor too", () => {
  const { aggregate } = fold([session()], {
    ambiguities: [{ scope: "lease", message: "two leases at sequence 4" }],
  });
  assert.equal(aggregate.atLeast, true);
});

test("zero sessions is scoped to this read, never to the world", () => {
  const { aggregate } = fold([]);
  assert.equal(
    agentProgressFooterText(aggregate),
    "No sessions in what this read returned",
  );
  const partial = fold([], { complete: false });
  assert.match(agentProgressFooterText(partial.aggregate), /did not complete/);
});

test("local transcript enriches a lane; its absence is stated, not hidden", () => {
  const detail = new Map([
    [
      "session-1",
      {
        sessionRef: "session-1",
        label: "Local label",
        runtimeLabel: "Claude Code",
        transcript: [toolItem()],
        openTarget: { channelId: "chan-1", generationId: "gen-1" },
      },
    ],
  ]);
  const joined = fold([session()], { detailBySessionRef: detail }).lanes[0];
  assert.equal(joined.activity, "▸ Bash");
  assert.equal(joined.runtimeLabel, "Claude Code");
  assert.deepEqual(joined.openTarget, {
    channelId: "chan-1",
    generationId: "gen-1",
  });

  const unjoined = fold([session()]).lanes[0];
  assert.equal(unjoined.activity, null);
  assert.equal(unjoined.openTarget, null);
  // The signed 44229 name still wins over any local label.
  assert.equal(unjoined.label, "Pulse plumbing");
});

test("a lane with neither a signed name nor a local one is never a bare hash", () => {
  const [lane] = fold([
    session({
      sessionKey: "5b7e1c2a-90d4",
      sessionRef: "5b7e1c2a-90d4",
      name: null,
    }),
  ]).lanes;
  assert.equal(lane.label, "Session 5b7e1c2a");
  assert.doesNotMatch(lane.label, /[0-9a-f]{64}/);
});

test("a reported failure leads with the error; a healthy row leads with now", () => {
  const detail = (transcript) =>
    new Map([
      [
        "session-1",
        {
          sessionRef: "session-1",
          label: null,
          runtimeLabel: null,
          transcript,
          openTarget: null,
        },
      ],
    ]);
  const failed = fold(
    [session({ generations: [generation({ status: "failed" })] })],
    { detailBySessionRef: detail([errorItem(), toolItem()]) },
  ).lanes[0];
  assert.equal(failed.activity, "provider exited with status 1");
  assert.equal(failed.activityIsError, true);

  const recovered = fold([session()], {
    detailBySessionRef: detail([errorItem(), toolItem()]),
  }).lanes[0];
  assert.equal(recovered.activity, "▸ Bash");
  assert.equal(recovered.activityIsError, false);
});

test("the lane key separates its parts with a real delimiter", () => {
  assert.equal(agentProgressLaneId("ab", "cd"), "ab\u0000cd");
  assert.notEqual(
    agentProgressLaneId("a", "bc"),
    agentProgressLaneId("ab", "c"),
  );
});

test("the newest activity wins, and an empty transcript yields nothing", () => {
  assert.equal(summarizeAgentLaneActivity([], false), null);
  assert.deepEqual(
    summarizeAgentLaneActivity(
      [toolItem(), { ...toolItem(), id: "t2", toolName: "Read" }],
      false,
    ),
    { text: "▸ Read", isError: false },
  );
});

/**
 * The seam itself: Agent Progress and Project Pulse must be adapters over one
 * fold, so the same bytes must produce the same coordination answer whether or
 * not a project scope is supplied. This folds nothing and asserts the shared
 * module is the one both call.
 */
test("the shared coordination fold is scope-parameterised, not duplicated", () => {
  const empty = foldSessionCoordination({ now: NOW, events: [] });
  assert.deepEqual(empty.sessions, []);
  assert.deepEqual(empty.ambiguities, []);
  assert.equal(empty.complete, true);
  assert.deepEqual(empty.errors, []);
  const scoped = foldSessionCoordination({
    now: NOW,
    events: [],
    sourceErrors: [
      { scope: "sessions", message: "relay unavailable" },
      { scope: "channels", message: "channel list partial" },
    ],
    acceptProjectRef: () => false,
  });
  assert.deepEqual(scoped.sessions, []);
  assert.equal(scoped.complete, false);
  assert.deepEqual(scoped.errors, [
    { scope: "channels", message: "channel list partial" },
    { scope: "sessions", message: "relay unavailable" },
  ]);
});
