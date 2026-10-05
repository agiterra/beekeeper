import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionDispositionStrip,
  CodingSessionHeader,
} from "./CodingSessionHeader.tsx";
import { codingSessionHeaderMetadataRows } from "./CodingSessionHeaderDetails.tsx";
import { codingSessionHeaderStatusDotClass } from "./CodingSessionHeaderParts.tsx";
import {
  CLOSED_PANELS,
  fakeSurfaceShell,
} from "./CodingSessionHeaderPanelToggles.testFixtures.mjs";

test("SV-20: the row reads project / title ● status, and the metadata line left it", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Keystone Session · generation 2",
      model: "claude-sonnet-4-5",
      onOpenProject() {},
      projectName: "Buzz Glue",
      providerAuthorityPubkey: "d7d05d95".repeat(8),
      repoName: "buzz",
      runtimeLabel: "Claude Code",
      sessionTitle: "Keystone Session",
      status: { kind: "working", label: "Working" },
    }),
  );

  const project = markup.indexOf(">Buzz Glue<");
  const separator = markup.indexOf(">/</li>");
  const title = markup.indexOf(">Keystone Session</h1>");
  const status = markup.indexOf("Session status: Working");
  assert.ok(project >= 0 && separator > project, markup);
  assert.ok(title > separator, markup);
  assert.ok(status > title, markup);
  assert.match(markup, /aria-label="Session breadcrumb"/);
  assert.match(markup, /aria-current="page"/);
  // The metadata line is Details' first rows now; none of it is in the row.
  for (const gone of [
    "Claude Code",
    "claude-sonnet",
    "generation 2",
    ">buzz<",
  ]) {
    assert.doesNotMatch(markup, new RegExp(gone));
  }
  assert.match(markup, /data-testid="coding-session-provenance-toggle"/);
});

test("SV-20: only a working dot pulses, as T3's status pill does, and only with motion allowed", () => {
  const working = codingSessionHeaderStatusDotClass(
    { kind: "working", label: "Working" },
    false,
  );
  assert.match(working, /motion-safe:animate-pulse/);
  assert.doesNotMatch(working, /(^| )animate-pulse/);
  assert.doesNotMatch(
    codingSessionHeaderStatusDotClass(
      { kind: "working", label: "Working" },
      true,
    ),
    /animate-pulse/,
  );
  assert.doesNotMatch(
    codingSessionHeaderStatusDotClass({ kind: "idle", label: "Idle" }, false),
    /animate-pulse/,
  );
});

test("Details' metadata rows: goal, repository, runtime, model, generation, blanks and repeats dropped", () => {
  assert.deepEqual(
    codingSessionHeaderMetadataRows({
      goal: "  Make two-agent work read as one session ",
      repo: "buzz",
      runtime: "Claude Code",
      model: "claude-sonnet-4-5",
      generation: "generation 2",
    }).map(({ key, label, value }) => [key, label, value]),
    [
      ["goal", "Goal", "Make two-agent work read as one session"],
      ["repo", "Repository", "buzz"],
      ["runtime", "Runtime", "Claude Code"],
      ["model", "Model", "claude-sonnet-4-5"],
      ["generation", "Generation", "generation 2"],
    ],
  );
  assert.deepEqual(
    codingSessionHeaderMetadataRows({
      goal: null,
      repo: "",
      runtime: "codex",
      model: "codex",
      generation: "generation 1",
    }).map(({ key }) => key),
    ["runtime", "generation"],
  );
});

test("Details' runtime and model rows say whose they are in a multi-seat session", () => {
  const rows = (focusedSeat) =>
    codingSessionHeaderMetadataRows({
      goal: null,
      repo: "buzz",
      runtime: "Claude Code",
      model: "opus",
      generation: null,
      focusedSeat,
    }).map(({ key, label }) => [key, label]);
  assert.deepEqual(rows("Fable"), [
    ["repo", "Repository"],
    ["runtime", "Runtime (focused seat: Fable)"],
    ["model", "Model (focused seat: Fable)"],
  ]);
  assert.deepEqual(rows(""), [
    ["repo", "Repository"],
    ["runtime", "Runtime (focused seat)"],
    ["model", "Model (focused seat)"],
  ]);
  assert.deepEqual(rows(null), [
    ["repo", "Repository"],
    ["runtime", "Runtime"],
    ["model", "Model"],
  ]);
});

test("umbrella header keeps the aggregate status beside the title and the chips beside it", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      agentControls: React.createElement(
        "div",
        null,
        "Codex working · Claude idle",
      ),
      channelName: "Hive Sessions",
      generationLabel: "generation 1",
      goalText: "Make two-agent work read as one session",
      sessionTitle: "Session UX",
      status: { kind: "working", label: "Working" },
      statusLabelOverride: "2 agents · 1 working",
    }),
  );
  assert.match(markup, /Codex working · Claude idle/);
  // The status word is never folded away, wide or narrow (SV-20).
  assert.match(
    markup,
    /aria-label="Session status: 2 agents · 1 working"[^>]*data-testid="coding-session-status-badge"/,
  );
  assert.doesNotMatch(markup, /coding-session-status-badge-narrow/);
  // The goal is Details' first row, not a line under the title.
  assert.doesNotMatch(markup, /Make two-agent work read as one session/);
});

test("a demoted status keeps its word on screen and its history clause one hover away", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "generation 1",
      sessionTitle: "Keystone Session",
      status: {
        kind: "unknown",
        label: "No provider answering",
        lastReported: { label: "Idle", ageSeconds: 7_200 },
      },
    }),
  );
  assert.match(
    markup,
    /title="No provider answering · last reported Idle 2h ago"/,
  );
  assert.match(
    markup,
    /aria-label="Session status: No provider answering · last reported Idle 2h ago"/,
  );
  assert.match(
    markup,
    /class="whitespace-nowrap">No provider answering<\/span>/,
  );
  // The badge never shrinks, so a long title truncates before the word does.
  assert.match(
    markup,
    /class="inline-flex shrink-0 [^"]*"[^>]*data-testid="coding-session-status-badge"/,
  );
  // The clause is in the text a search or a screen reader reads, never gone.
  assert.match(markup, /class="sr-only"> · last reported Idle 2h ago</);
});

test("the team lens lives beside the title instead of creating another header", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "generation 1",
      sessionTitle: "TeamTest",
      status: { kind: "idle", label: "Idle" },
      viewControl: React.createElement("span", null, "Conversation · Mission"),
    }),
  );
  assert.match(markup, /TeamTest/);
  assert.match(markup, /Conversation · Mission/);
  assert.equal((markup.match(/<header/g) ?? []).length, 1);
});

test("export is offered from the ⋯ menu only when an export handler is provided", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    status: { kind: "idle", label: "Idle" },
  };

  const withoutExport = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  // No action at all, so no menu: a `⋯` over an empty list would lie.
  assert.doesNotMatch(withoutExport, /data-testid="coding-session-overflow"/);

  const withExport = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onExport() {},
    }),
  );
  // SESSION_VIEW_UX_PLAN L4: the flat button moved into the menu. Its
  // disabled-while-exporting state is proved by opening the menu
  // (`CodingSessionHeader.reachability.test.mjs`).
  assert.match(withExport, /data-testid="coding-session-overflow"/);
  assert.doesNotMatch(withExport, /data-testid="coding-session-export"/);
});

test("header exposes rename only when the authority-aware workspace provides it", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "generation 2",
    sessionTitle: "Durable session name",
    status: { kind: "idle", label: "Idle" },
  };
  assert.doesNotMatch(
    renderToStaticMarkup(React.createElement(CodingSessionHeader, baseProps)),
    /data-testid="coding-session-rename"/,
  );
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onRename() {},
    }),
  );
  assert.match(markup, /data-testid="coding-session-rename"/);
  assert.match(markup, /aria-label="Rename session"/);
});

test("SV-20: no surface toggles and no Plan toggle; the panel toggles come with a surface shell", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    status: { kind: "idle", label: "Idle" },
  };

  // No session view behind the header (pending, loading): no panel toggles.
  const bare = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  assert.doesNotMatch(bare, /coding-session-panel-toggle/);

  const { shell } = fakeSurfaceShell({
    surfaces: [{ id: "agents" }, { id: "diff" }, { id: "plan" }],
    panelState: { ...CLOSED_PANELS, rightOpen: true },
  });
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      // Props the header used to take for its toggles do nothing now.
      onToggleSurface() {},
      onToggleTaskRail() {},
      surfaceHostId: "surface-host-1",
      surfaceShell: shell,
      surfaceTabs: [
        { id: "agents", label: "Agents", icon: "agents", active: true },
      ],
      taskCount: 3,
    }),
  );
  assert.doesNotMatch(markup, /coding-session-surface-toggle/);
  assert.doesNotMatch(markup, /coding-session-task-rail-toggle/);
  assert.doesNotMatch(markup, /aria-label="Session surfaces"/);
  assert.match(
    markup,
    /aria-pressed="false"[^>]*data-testid="coding-session-panel-toggle-bottom"/,
  );
  assert.match(
    markup,
    /aria-controls="surface-host-1"[^>]*aria-label="Toggle right panel"[^>]*aria-pressed="true"[^>]*data-testid="coding-session-panel-toggle-right"/,
  );
  // Right side order: Details, ⋯ (empty here), bottom, right.
  assert.ok(
    markup.indexOf("coding-session-provenance-toggle") <
      markup.indexOf("coding-session-panel-toggle-bottom"),
  );
  assert.ok(
    markup.indexOf("coding-session-panel-toggle-bottom") <
      markup.indexOf("coding-session-panel-toggle-right"),
  );
});

test("the add-provider action appears only when this session can take one", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    status: { kind: "idle", label: "Idle" },
  };

  // A pre-umbrella session, a non-member view, or a non-founder gets no
  // handler — and therefore no chrome at all in the N=1 header.
  const withoutJoin = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  assert.doesNotMatch(withoutJoin, /coding-session-add-provider/);
  assert.doesNotMatch(withoutJoin, /data-testid="coding-session-overflow"/);

  const withJoin = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onAddProvider() {},
    }),
  );
  // It lives in the ⋯ menu now, in every lens.
  assert.match(withJoin, /data-testid="coding-session-overflow"/);
  assert.doesNotMatch(withJoin, /data-testid="coding-session-add-provider"/);
});

test("closure controls describe session state without rewriting execution status", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    status: { kind: "ended", label: "Ended" },
  };

  const close = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onCloseSession() {},
    }),
  );
  // Close is a menu item now; the status still reads the execution's word.
  assert.doesNotMatch(close, /data-testid="coding-session-close"/);
  assert.match(close, /data-testid="coding-session-overflow"/);
  assert.match(close, /aria-label="Session status: Ended"/);
  assert.doesNotMatch(close, /data-testid="coding-session-reopen"/);

  // Reopen is a closed session's one primary action, so it stays in the row.
  const reopen = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onReopenSession() {},
      sessionClosed: true,
    }),
  );
  assert.match(reopen, /data-testid="coding-session-reopen"/);
  assert.match(reopen, /aria-label="Session status: Closed"/);
  assert.match(
    reopen,
    /title="Return this session to Sessions without starting a provider"/,
  );
  assert.doesNotMatch(reopen, /data-testid="coding-session-close"/);
  // …and is not offered twice.
  assert.doesNotMatch(reopen, /data-testid="coding-session-overflow"/);
});

test("an owning project reads as a followable crumb ahead of the title", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "buzz glue sessions",
      generationLabel: "Keystone Session · generation 2",
      onOpenProject() {},
      projectName: "Buzz Glue",
      runtimeLabel: "Claude Code",
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.match(markup, /data-testid="coding-session-project-crumb"/);
  assert.match(
    markup,
    />Buzz Glue<\/button><\/li><li aria-hidden="true"[^>]*>\/<\/li>/,
  );
  // The crumb is a real control, not text styled to look like one.
  assert.match(markup, /title="Open Buzz Glue"/);
});

test("without a way to open it the project is plain text, not a dead link", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "buzz glue sessions",
      generationLabel: "Keystone Session · generation 2",
      projectName: "Buzz Glue",
      runtimeLabel: "Claude Code",
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.doesNotMatch(markup, /coding-session-project-crumb/);
  assert.match(
    markup,
    /<span class="[^"]*" data-testid="coding-session-project-label">Buzz Glue<\/span>/,
  );
});

test("a session no project claims shows no crumb and no separator", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "engineering",
      generationLabel: "generation 2",
      onOpenProject() {},
      projectName: null,
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.doesNotMatch(markup, /coding-session-project-crumb/);
  assert.doesNotMatch(markup, /coding-session-project-label/);
  assert.doesNotMatch(markup, />\/<\/li>/);
  // No name reached the header: it reads the shared resolver's fallback
  // (SV-31), the text web and mobile show.
  assert.match(markup, />Untitled session<\/h1>/);
});

test("header keeps the adapter's raw model id out of the row", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Generation 1",
      model: "gpt-5.6-terra[low]",
      runtimeLabel: "Codex",
      sessionTitle: "Readable session labels",
      status: { kind: "working", label: "Working" },
    }),
  );

  // The formatted model is Details' Model row
  // (`CodingSessionHeader.reachability.test.mjs` opens it).
  assert.doesNotMatch(markup, /gpt-5\.6-terra/);
});

test("a seated session wears its seat beside the title, and an unseated one does not", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "generation 2",
    sessionTitle: "Keystone Session",
    status: { kind: "idle", label: "Idle" },
  };

  const seated = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      seat: { label: "Ada · Builder" },
    }),
  );
  assert.match(seated, /data-testid="coding-session-header-seat"/);
  assert.match(seated, /Ada · Builder/);

  const unseated = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  assert.doesNotMatch(unseated, /data-testid="coding-session-header-seat"/);

  const explicitlyUnseated = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, { ...baseProps, seat: null }),
  );
  assert.equal(explicitlyUnseated, unseated);
});

// --- Disposition strip (ledger 77, "Umbrella UI (a)") ------------------------

const AGENT_ADA = "1a".repeat(32);
const AGENT_GRACE = "2b".repeat(32);

function execution({
  executionKey,
  agentRef,
  role,
  status = "idle",
  runtime = "claude",
  model = "claude-opus-5",
  transcript = [],
}) {
  return {
    executionKey,
    signerPubkey: "a".repeat(64),
    operatorPubkey: null,
    priorGenerations: [],
    activeGeneration: {
      generationId: executionKey,
      label: executionKey,
      title: "Team session",
      providerAuthorityPubkey: "a".repeat(64),
      metadataAuthorityPubkey: "a".repeat(64),
      lastEventAt: "2026-08-12T10:06:00.000Z",
      status,
      statusAt: null,
      transcript,
      conflictCount: 0,
      commandTarget: null,
      projectRef: null,
      repoRef: null,
      sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
      provider: `${runtime}-primary`,
      runtime,
      model,
      agentRef,
      role,
      turnBudget: null,
      capabilities: null,
    },
  };
}

test("disposition strip renders one honest line per execution, lead first", () => {
  const names = new Map([
    [AGENT_ADA, "Ada"],
    [AGENT_GRACE, "Grace"],
  ]);
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionDispositionStrip, {
      actorNames: (pubkey) => names.get(pubkey) ?? null,
      nowMs: Date.parse("2026-08-12T10:10:00.000Z"),
      resolveReachability: () => ({ known: false }),
      umbrella: {
        umbrellaKey: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        title: "Team session",
        founderPubkey: null,
        genesisRef: null,
        genesisResolution: "legacy",
        status: "idle",
        lastEventAt: "2026-08-12T10:06:00.000Z",
        conflictCount: 0,
        foreignAttachmentCount: 0,
        executions: [
          execution({
            executionKey: "execution-builder",
            agentRef: AGENT_GRACE,
            role: "builder",
            status: "stopped",
          }),
          execution({
            executionKey: "execution-lead",
            agentRef: AGENT_ADA,
            role: "lead",
            status: "running",
            transcript: [
              {
                id: "item-1",
                type: "message",
                renderClass: "message",
                role: "assistant",
                title: "Assistant",
                text: "APPROVE",
                timestamp: "2026-08-12T10:06:00.000Z",
              },
            ],
          }),
        ],
      },
    }),
  );

  assert.match(markup, /data-testid="coding-session-disposition-strip"/);
  assert.match(markup, /Ada · Lead · live · last turn 4m ago/);
  assert.match(markup, /Grace · Builder · released · no turn observed/);
  assert.ok(
    markup.indexOf("Ada · Lead") < markup.indexOf("Grace · Builder"),
    markup,
  );
});

test("disposition strip renders nothing when the umbrella holds no execution", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionDispositionStrip, {
        resolveReachability: () => ({ known: false }),
        umbrella: {
          umbrellaKey: "empty",
          sessionRef: null,
          title: "Team session",
          founderPubkey: null,
          genesisRef: null,
          genesisResolution: "legacy",
          status: "idle",
          lastEventAt: "2026-08-12T10:06:00.000Z",
          conflictCount: 0,
          foreignAttachmentCount: 0,
          executions: [],
        },
      }),
    ),
    "",
  );
});

/**
 * Item 87(e): the header is where "stop all of it" has to live, because the
 * per-execution stop is inside a composer you cannot reach when three agents
 * are working. Founder-only, and absent — not disabled — for anybody else: a
 * greyed-out control invites the click that teaches you the authority is not
 * yours.
 */
test("Stop all is the founder's, in the ⋯ menu, and nobody else sees it", () => {
  const base = {
    channelName: "Beekeeper sessions",
    generationLabel: "Keystone Session · generation 1",
    sessionTitle: "UI",
    status: { kind: "working", label: "Working" },
  };

  const founderView = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...base,
      onStopAll() {},
      stopAllCount: 3,
    }),
  );
  // The item's label and consequence sentence are proved by opening the
  // menu (`CodingSessionHeader.reachability.test.mjs`).
  assert.match(founderView, /data-testid="coding-session-overflow"/);
  assert.doesNotMatch(founderView, /data-testid="coding-session-stop-all"/);

  // Everybody else: no control at all, and nothing disabled to click at.
  const viewerView = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, base),
  );
  assert.doesNotMatch(viewerView, /coding-session-stop-all/);
  assert.doesNotMatch(viewerView, /coding-session-overflow/);
  assert.doesNotMatch(viewerView, /Stop all/);
});

/**
 * The hires line.
 *
 * Until 2026-08-30 the host answered `session.hire` in the app shell and the
 * runner that mounted it discarded every outcome, so a hire could be read,
 * refused and thrown away with nothing on any screen (ledger draft 97).
 */
test("disposition strip counts what this host did with hires, and hovers the reason", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionDispositionStrip, {
      resolveReachability: () => ({ known: false }),
      hireOutcomes: [
        { state: "seated", detail: null },
        { state: "refused", detail: "HIRE_OFF" },
        {
          state: "malformed",
          detail: "action.routing.tier: the host derives it",
        },
        // Not this operator's umbrella — not owed an answer, so not counted.
        { state: "ignored", detail: "not this operator's session" },
      ],
      umbrella: {
        umbrellaKey: "empty",
        sessionRef: null,
        title: "Team session",
        founderPubkey: null,
        genesisRef: null,
        genesisResolution: "legacy",
        status: "idle",
        lastEventAt: "2026-08-12T10:06:00.000Z",
        conflictCount: 0,
        foreignAttachmentCount: 0,
        executions: [],
      },
    }),
  );

  assert.match(markup, /data-testid="coding-session-hires-row"/);
  assert.match(markup, /hires: 1 answered · 1 refused · 1 malformed/);
  assert.match(markup, /title="action\.routing\.tier: the host derives it"/);
});

test("an answer that never went out is counted, never folded into refused", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionDispositionStrip, {
      resolveReachability: () => ({ known: false }),
      hireOutcomes: [{ state: "error", detail: "the relay timed out" }],
      umbrella: {
        umbrellaKey: "empty",
        sessionRef: null,
        title: "Team session",
        founderPubkey: null,
        genesisRef: null,
        genesisResolution: "legacy",
        status: "idle",
        lastEventAt: "2026-08-12T10:06:00.000Z",
        conflictCount: 0,
        foreignAttachmentCount: 0,
        executions: [],
      },
    }),
  );
  assert.match(
    markup,
    /hires: 0 answered · 0 refused · 0 malformed · 1 failed to answer/,
  );
});

test("a host that has answered no hire adds no line", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionDispositionStrip, {
        resolveReachability: () => ({ known: false }),
        hireOutcomes: [],
        umbrella: {
          umbrellaKey: "empty",
          sessionRef: null,
          title: "Team session",
          founderPubkey: null,
          genesisRef: null,
          genesisResolution: "legacy",
          status: "idle",
          lastEventAt: "2026-08-12T10:06:00.000Z",
          conflictCount: 0,
          foreignAttachmentCount: 0,
          executions: [],
        },
      }),
    ),
    "",
  );
});

// The window's own back/forward is the way out of a session in the main
// window. A header that drew its own arrow next to it was the same gesture
// twice, and the two did not always agree on where "back" was.
test("no close control unless the surface supplies one", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "generation 1",
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.doesNotMatch(markup, /data-testid="coding-session-dismiss"/);
});

test("a pop-out or dialog gets a close control it can name", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      closeLabel: "Close session window",
      generationLabel: "generation 1",
      onClose() {},
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.match(markup, /data-testid="coding-session-dismiss"/);
  assert.match(markup, /aria-label="Close session window"/);
  // Not to be confused with `coding-session-close`, which settles the session
  // itself. One dismisses a window; the other ends shared work.
  assert.doesNotMatch(markup, /data-testid="coding-session-close"/);
});

/**
 * SESSION_VIEW_UX_PLAN L4: every lens collapses the header's actions into the
 * one `⋯` menu Mission already had (DESIGN-SPEC A7). The row keeps title,
 * status, the full-access badge, the live-count surface tabs and one Details
 * control (People with its count, plus provenance).
 */
const FLAT_RUN_PROPS = {
  channelName: "Hive Sessions",
  generationLabel: "generation 1",
  isExporting: false,
  onAddProvider() {},
  onCloseSession() {},
  onExport() {},
  onPopout() {},
  onStopAll() {},
  sessionTitle: "Keystone Session",
  status: { kind: "idle", label: "Idle" },
  stopAllCount: 1,
};

const FORMER_FLAT_ACTIONS = [
  "coding-session-add-provider",
  "coding-session-stop-all",
  "coding-session-close",
  "coding-session-export",
  "coding-session-header-popout",
];

test("Conversation's flat action run is one ⋯ menu, with or without a session", () => {
  for (const props of [
    FLAT_RUN_PROPS,
    {
      ...FLAT_RUN_PROPS,
      workspaceReuse: { channelId: "channel-1", sessionRef: "session-1" },
    },
  ]) {
    const markup = renderToStaticMarkup(
      React.createElement(CodingSessionHeader, props),
    );
    assert.equal(
      (markup.match(/data-testid="coding-session-overflow"/g) ?? []).length,
      1,
    );
    for (const testId of FORMER_FLAT_ACTIONS) {
      assert.doesNotMatch(markup, new RegExp(`data-testid="${testId}"`));
    }
  }
});

test("Mission collapses the same actions and gains no flat button", () => {
  const conversation = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...FLAT_RUN_PROPS,
      workspaceReuse: { channelId: "channel-1", sessionRef: "session-1" },
    }),
  );
  const mission = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...FLAT_RUN_PROPS,
      missionActions: true,
      workspaceReuse: { channelId: "channel-1", sessionRef: "session-1" },
    }),
  );
  assert.match(mission, /data-testid="coding-session-overflow"/);
  for (const testId of FORMER_FLAT_ACTIONS) {
    assert.doesNotMatch(mission, new RegExp(`data-testid="${testId}"`));
  }
  // One header for both lenses: the prop no longer changes the row.
  assert.equal(mission, conversation);
});
