/**
 * SV-23 (reasons): every surface lane B2 owns, made unavailable, gives the
 * sentence the Wave B brief's §3 table names — word for word — and its panel
 * shows that same sentence rather than an empty frame.
 *
 * Terminal's reason belongs to lane B4 (its availability now names the
 * provider, not the owner); it is checked there.
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { CODING_SESSION_BUILTIN_SURFACES } from "./codingSessionBuiltinSurfaces.ts";

/** The brief's §3 "otherwise, dimmed with this reason" column. */
const SECTION_3_REASONS = {
  agents: "No agent has joined this session yet.",
  diff: "Nothing has been observed yet.",
  files: "This session has no project files to show.",
  plan: "The agent has not published a plan.",
  landing: "This session has no repository to land into.",
  people: "This session predates sharing and has no roster.",
  pulse: "This session belongs to no project.",
  browser: "Arrives with live preview.",
  device: "Arrives with device support.",
};

function emptyCtx(overrides = {}) {
  return {
    layout: "single",
    channelId: "channel-1",
    communityScope: "wss://relay.test",
    sessionKey: "session-1",
    umbrella: {
      umbrellaKey: "session-1",
      sessionRef: "session-1",
      title: "Session",
      executions: [],
      founderPubkey: null,
      genesisRef: null,
      genesisResolution: "legacy",
      status: "idle",
      lastEventAt: "",
      conflictCount: 0,
      foreignAttachmentCount: 0,
    },
    focusedExecution: null,
    focusedRecord: null,
    executions: [],
    transcript: [],
    transcriptModel: null,
    umbrellaTimeline: null,
    observedChanges: { files: [], unreportedEditCount: 0 },
    subagents: { rows: [] },
    taskModel: null,
    isLocalProvider: false,
    projectRef: null,
    repoRef: null,
    project: null,
    genesisRef: null,
    founderPubkey: null,
    currentUserPubkey: null,
    sessionClosed: false,
    lens: "conversation",
    activeSurfaceId: null,
    panelState: {
      rightOpen: true,
      tabs: [],
      active: null,
      expanded: false,
      bottomOpen: false,
      userActed: false,
    },
    panels: {},
    observations: { state: "not-read", reason: "no genesis" },
    openRulings: null,
    tree: {
      state: "resolved",
      available: false,
      source: null,
      label: "no working tree",
      reason: "No working tree for this session is recorded on this computer.",
      refusal: "notRecorded",
      query: {
        sessionId: null,
        channelId: "channel-1",
        projectRef: null,
        isLocalProvider: false,
      },
    },
    minimapSlotRef: { current: null },
    resolveActorName: () => null,
    resolveReachability: () => ({ known: false }),
    mission: null,
    extensions: {},
    ...overrides,
  };
}

function definition(id) {
  const found = CODING_SESSION_BUILTIN_SURFACES.find((d) => d.id === id);
  assert.ok(found, `no built-in surface "${id}"`);
  return found;
}

test("every dimmed reason in §3 appears word for word", () => {
  const ctx = emptyCtx();
  for (const [id, reason] of Object.entries(SECTION_3_REASONS)) {
    const availability = definition(id).availability(ctx);
    assert.equal(availability.available, false, `${id} should be dimmed`);
    assert.equal(availability.reason, reason, `${id}'s reason`);
  }
});

test("each surface opens when its §3 condition holds", () => {
  const someone = emptyCtx({
    umbrella: { ...emptyCtx().umbrella, executions: [{}] },
    transcript: [{ type: "message" }],
    taskModel: { tasks: [{ id: "t", text: "x", status: "pending" }] },
    repoRef: "30617:owner:repo",
    projectRef: "30621:owner:project",
    genesisRef: "g".repeat(64),
  });
  for (const id of [
    "agents",
    "diff",
    "files",
    "plan",
    "landing",
    "people",
    "pulse",
  ]) {
    assert.deepEqual(
      definition(id).availability(someone),
      { available: true },
      id,
    );
  }
  // A working tree alone opens Files, with no project at all.
  const treeOnly = emptyCtx({
    tree: {
      ...emptyCtx().tree,
      available: true,
      source: "session",
      refusal: null,
      reason: null,
    },
  });
  assert.deepEqual(definition("files").availability(treeOnly), {
    available: true,
  });
  // An explicitly empty signed plan is a published plan: it opens Plan,
  // never "The agent has not published a plan."
  const emptyPlan = emptyCtx({
    taskModel: {
      sourceItemId: "plan-1",
      turnId: null,
      timestamp: "",
      tasks: [],
      completedCount: 0,
      explanation: null,
      copyText: null,
      state: "empty",
    },
  });
  assert.deepEqual(definition("plan").availability(emptyPlan), {
    available: true,
  });
  // Browser and Device never open in Wave B.
  for (const id of ["browser", "device"]) {
    assert.equal(definition(id).availability(someone).available, false, id);
  }
});

test("a panel left open after its surface became unavailable shows the same reason", () => {
  const ctx = emptyCtx();
  for (const [id, reason] of Object.entries(SECTION_3_REASONS)) {
    const { Panel } = definition(id);
    // Some panels read cached queries (the provider status, the identity)
    // before they know they are unavailable.
    const client = new QueryClient();
    const markup = renderToStaticMarkup(
      React.createElement(
        QueryClientProvider,
        { client },
        React.createElement(Panel, { ctx }),
      ),
    );
    client.clear();
    assert.match(
      markup,
      new RegExp(`data-testid="coding-session-surface-panel-${id}"`),
      id,
    );
    assert.match(markup, /data-available="false"/, id);
    assert.ok(
      markup.includes(reason.replace(/'/g, "&#x27;")),
      `${id}'s panel shows its reason`,
    );
  }
});

test("Mission's three surfaces are a lens choice, listed only in Mission", () => {
  for (const id of ["mission-inspector", "mission-context", "mission-audit"]) {
    assert.deepEqual([...definition(id).lenses], ["mission"], id);
  }
  for (const id of Object.keys(SECTION_3_REASONS)) {
    assert.deepEqual(
      [...definition(id).lenses],
      ["conversation", "mission"],
      id,
    );
  }
});

test("Files' elsewhere line names whose provider runs it, as Terminal does", async () => {
  const { codingSessionFilesElsewhereLine } = await import(
    "../CodingSessionFilesPanel.tsx"
  );
  const named = codingSessionFilesElsewhereLine({
    focusedExecution: null,
    focusedRecord: { providerAuthorityPubkey: "aa".repeat(32) },
    resolveActorName: () => "Andy",
  });
  assert.equal(
    named,
    "The working tree is on another computer (Andy's provider runs this session).",
  );
  assert.doesNotMatch(named, /Andy's computer/);
  const { codingSessionTreeElsewhereReason } = await import(
    "./CodingSessionSurfaceTerminal.tsx"
  );
  const terminal = codingSessionTreeElsewhereReason({
    focusedExecution: null,
    focusedRecord: { providerAuthorityPubkey: "aa".repeat(32) },
    resolveActorName: () => "Andy",
  });
  assert.ok(terminal.startsWith(named.slice(0, -1)), terminal);
  assert.equal(
    codingSessionFilesElsewhereLine({
      focusedExecution: null,
      focusedRecord: null,
      resolveActorName: () => null,
    }),
    "The working tree is on another computer.",
  );
});

test("Agents lists each subagent once, under the orchestration's Direct spawns", () => {
  const row = {
    id: "spawn-1",
    title: "Survey memory papers",
    type: "general-purpose",
    status: "done",
    durationMs: 1_000,
    model: null,
    totalTokens: null,
    toolCount: null,
    latest: null,
    startedAt: "2026-10-04T00:00:00Z",
    spawn: { call: { result: "Found it." }, children: [] },
  };
  const ctx = emptyCtx({
    subagents: {
      rows: [row],
      settled: 1,
      running: 0,
      unknown: 0,
      totalTokens: null,
    },
  });
  const html = renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client: new QueryClient() },
      React.createElement(definition("agents").Panel, { ctx }),
    ),
  );
  assert.match(html, /coding-session-surface-panel-agents/);
  assert.equal(html.split("Survey memory papers").length - 1, 1, html);
  assert.equal(
    html.split('data-testid="coding-session-agents-spawn-row"').length - 1,
    1,
  );
  assert.doesNotMatch(
    html,
    /coding-session-subagent-row|coding-session-subagents-section/,
  );
});

test("Files dims only when the agents-repo read answered none and there is no tree", () => {
  const project = { projectRef: "30621:owner:project" };
  const absent = emptyCtx({
    ...project,
    extensions: { files: { agentsRepo: "absent" } },
  });
  assert.deepEqual(definition("files").availability(absent), {
    available: false,
    reason: SECTION_3_REASONS.files,
  });
  for (const agentsRepo of ["present", "unknown"]) {
    const ctx = emptyCtx({ ...project, extensions: { files: { agentsRepo } } });
    assert.deepEqual(
      definition("files").availability(ctx),
      { available: true },
      agentsRepo,
    );
  }
  const treeHere = emptyCtx({
    ...project,
    extensions: { files: { agentsRepo: "absent" } },
    tree: { ...emptyCtx().tree, available: true, refusal: null, reason: null },
  });
  assert.deepEqual(definition("files").availability(treeHere), {
    available: true,
  });
});

test("Landing opens without a repository when a gate row, gate start or genesis exists", () => {
  const read = (gates, gateStarts) => ({
    state: "read",
    isLoading: false,
    errorMessage: null,
    result: { fold: { gateStarts } },
    view: { gates },
    assignmentsChecked: true,
    readAtMs: 0,
    live: "subscribed",
    refresh: () => {},
  });
  const failedGate = emptyCtx({
    observations: read([{ gate: "just ci", outcome: "failed" }], []),
  });
  assert.deepEqual(definition("landing").availability(failedGate), {
    available: true,
  });
  const started = emptyCtx({ observations: read([], [{ eventId: "s" }]) });
  assert.deepEqual(definition("landing").availability(started), {
    available: true,
  });
  const genesis = emptyCtx({
    umbrella: { ...emptyCtx().umbrella, genesisRef: "g".repeat(64) },
  });
  assert.deepEqual(definition("landing").availability(genesis), {
    available: true,
  });
  const nothing = emptyCtx({ observations: read([], []) });
  assert.equal(definition("landing").availability(nothing).available, false);
});

test("Landing puts each running line above its own gate's row, or alone", async () => {
  const { groupRunningLines } = await import(
    "../CodingSessionLandingGateRunning.tsx"
  );
  const line = (gate) => ({
    key: gate,
    gate,
    line: gate,
    title: "",
    stale: false,
    eventId: "e".repeat(64),
  });
  const groups = groupRunningLines({
    rows: [{ gate: "cargo test" }],
    unnamedRows: [{ gate: "just ci" }],
    running: [line("cargo test"), line("just ci"), line("clippy")],
  });
  assert.deepEqual([...groups.byGate.keys()].sort(), ["cargo test", "just ci"]);
  assert.deepEqual(
    groups.alone.map((l) => l.gate),
    ["clippy"],
  );
});
