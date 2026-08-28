import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionHeader } from "./CodingSessionHeader.tsx";

test("header keeps signed generation identity visible beside runtime context", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Keystone Session · generation 2",
      model: "claude-sonnet-4-5",
      onBack() {},
      providerAuthorityPubkey: "d7d05d95".repeat(8),
      runtimeLabel: "Claude Code",
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.match(markup, />Keystone Session</);
  assert.match(markup, /Claude Code · claude-sonnet-4-5 · generation 2/);
  assert.doesNotMatch(
    markup,
    /Claude Code · claude-sonnet-4-5 · Keystone Session · generation 2/,
  );
  assert.match(markup, /data-testid="coding-session-provenance-toggle"/);
});

test("umbrella header promotes the goal and aggregate agent status", () => {
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
      onBack() {},
      sessionTitle: "Session UX",
      status: { kind: "working", label: "Working" },
      statusLabelOverride: "2 agents · 1 working",
    }),
  );
  assert.match(markup, /Make two-agent work read as one session/);
  assert.match(markup, /Codex working · Claude idle/);
  assert.doesNotMatch(markup, /coding-session-status-badge/);
  assert.doesNotMatch(markup, />generation 1<\/p>/);
});

test("header renders the export button only when an export handler is provided", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    onBack() {},
    status: { kind: "idle", label: "Idle" },
  };

  const withoutExport = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  assert.doesNotMatch(withoutExport, /data-testid="coding-session-export"/);

  const withExport = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onExport() {},
    }),
  );
  assert.match(withExport, /data-testid="coding-session-export"/);
  assert.match(withExport, /aria-label="Export transcript"/);
  assert.doesNotMatch(
    withExport,
    /data-testid="coding-session-export"[^>]*disabled/,
  );
});

test("header exposes rename only when the authority-aware workspace provides it", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "generation 2",
    onBack() {},
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

test("header disables the export button while an export is running", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Keystone Session · generation 2",
      isExporting: true,
      onBack() {},
      onExport() {},
      status: { kind: "idle", label: "Idle" },
    }),
  );
  const exportButton = markup.match(
    /<button[^>]*data-testid="coding-session-export"[^>]*>/u,
  );
  assert.ok(exportButton, "export button must render");
  assert.match(exportButton[0], /disabled/);
});

test("surface affordances are compact direct tabs into the shared host", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    onBack() {},
    status: { kind: "idle", label: "Idle" },
  };

  // No surface wiring, no affordances — the workspace-state header stays bare.
  const withoutSurfaces = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  assert.doesNotMatch(withoutSurfaces, /coding-session-surface-toggle/);

  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onToggleSurface() {},
      surfaceHostId: "surface-host-1",
      surfaceTabs: [
        {
          id: "agents",
          label: "Agents",
          icon: "agents",
          count: 2,
          active: true,
        },
        {
          id: "changes",
          label: "Observed changes",
          icon: "changes",
          count: 0,
          active: false,
        },
      ],
    }),
  );
  assert.match(markup, /data-testid="coding-session-surface-toggle-agents"/);
  assert.match(markup, /data-testid="coding-session-surface-toggle-changes"/);
  assert.match(markup, /aria-label="Hide agents"/);
  assert.match(markup, /aria-label="Show observed changes"/);
  assert.match(markup, /aria-controls="surface-host-1"/);
  assert.match(markup, />Observed changes</);
});

test("the add-provider affordance appears only when this session can take one", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    onBack() {},
    status: { kind: "idle", label: "Idle" },
  };

  // A pre-umbrella session, a non-member view, or a non-founder gets no
  // handler — and therefore no chrome at all in the N=1 header.
  const withoutJoin = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, baseProps),
  );
  assert.doesNotMatch(withoutJoin, /data-testid="coding-session-add-provider"/);

  const withJoin = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onAddProvider() {},
    }),
  );
  assert.match(withJoin, /data-testid="coding-session-add-provider"/);
  assert.match(withJoin, /Add provider/);
});

test("closure controls describe session state without rewriting execution status", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    onBack() {},
    status: { kind: "ended", label: "Ended" },
  };

  const close = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      ...baseProps,
      onCloseSession() {},
    }),
  );
  assert.match(close, /data-testid="coding-session-close"/);
  assert.match(close, /aria-label="Session status: Ended"/);
  assert.doesNotMatch(close, /data-testid="coding-session-reopen"/);

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
});

test("an owning project reads as a followable crumb ahead of the context line", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "buzz glue sessions",
      generationLabel: "Keystone Session · generation 2",
      onBack() {},
      onOpenProject() {},
      projectName: "Buzz Glue",
      runtimeLabel: "Claude Code",
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.match(markup, /data-testid="coding-session-project-crumb"/);
  assert.match(markup, />Buzz Glue<\/button> · Claude Code · generation 2/);
  // The crumb is a real control, not text styled to look like one.
  assert.match(markup, /title="Open Buzz Glue"/);
});

test("without a way to open it the project is plain context, not a dead link", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "buzz glue sessions",
      generationLabel: "Keystone Session · generation 2",
      onBack() {},
      projectName: "Buzz Glue",
      runtimeLabel: "Claude Code",
      sessionTitle: "Keystone Session",
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.doesNotMatch(markup, /coding-session-project-crumb/);
  assert.match(markup, /Buzz Glue · Claude Code · generation 2/);
});

test("a session no project claims shows no crumb at all", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "engineering",
      generationLabel: "generation 2",
      onBack() {},
      onOpenProject() {},
      projectName: null,
      status: { kind: "idle", label: "Idle" },
    }),
  );

  assert.doesNotMatch(markup, /coding-session-project-crumb/);
  assert.match(markup, />generation 2</);
});

test("header separates reasoning effort from the adapter's raw model id", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Generation 1",
      model: "gpt-5.6-terra[low]",
      onBack() {},
      runtimeLabel: "Codex",
      sessionTitle: "Readable session labels",
      status: { kind: "working", label: "Working" },
    }),
  );

  assert.match(markup, /gpt-5\.6-terra · Low/);
  assert.doesNotMatch(markup, /gpt-5\.6-terra\[low\]/);
});

test("a seated session wears its seat beside the title, and an unseated one does not", () => {
  const baseProps = {
    channelName: "Hive Sessions",
    generationLabel: "generation 2",
    onBack() {},
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
