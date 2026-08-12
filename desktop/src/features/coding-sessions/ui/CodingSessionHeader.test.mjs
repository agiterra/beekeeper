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
