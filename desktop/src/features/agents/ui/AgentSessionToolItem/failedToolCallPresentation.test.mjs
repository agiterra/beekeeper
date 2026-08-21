import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { parseShellToolOutput } from "../agentSessionUtils.ts";
import {
  CompactToolSummaryRow,
  compactSummaryTone,
} from "./CompactToolSummaryRow.tsx";
import { ShellCommandBlock, shellBlockOutput } from "./ShellCommandBlock.tsx";

function shellResult(fields) {
  return JSON.stringify(fields);
}

// ── shell output composition ────────────────────────────────────────────────

test("shellBlockOutput_success_showsStdoutOnly", () => {
  // stderr is routinely noisy on a command that worked (progress bars, warnings)
  // and would drown the output that matters.
  const output = parseShellToolOutput(
    shellResult({ stdout: "ok\n", stderr: "warn: deprecated\n", exit_code: 0 }),
  );
  assert.equal(shellBlockOutput(output, false), "ok");
});

test("shellBlockOutput_failure_leadsWithExitCodeThenStreams", () => {
  // The reason a call failed is almost always the exit code or stderr. Showing
  // stdout alone renders an empty panel under a red "failed" row.
  //
  // Each stream keeps its own trailing newline, so a stdout that ended cleanly
  // leaves a blank line before stderr — the separation is wanted here, where
  // two interleaved streams are being read at once.
  const output = parseShellToolOutput(
    shellResult({ stdout: "partial\n", stderr: "boom\n", exit_code: 127 }),
  );
  assert.equal(
    shellBlockOutput(output, true),
    "Exit code 127\npartial\n\nboom",
  );
});

test("shellBlockOutput_failure_omitsAbsentExitCode", () => {
  const output = parseShellToolOutput(
    shellResult({ stdout: "", stderr: "boom\n" }),
  );
  assert.equal(shellBlockOutput(output, true), "boom");
});

test("shellBlockOutput_failure_fallsBackToRawWhenUnstructured", () => {
  // A tool that returned a bare string has no stdout/stderr shape at all; the
  // parser puts it in `raw`, and a failed call must still show it.
  const output = parseShellToolOutput("command not found");
  assert.equal(shellBlockOutput(output, true), "command not found");
  assert.equal(shellBlockOutput(output, false), "");
});

test("shellCommandBlock_failure_rendersStderrInDestructiveTone", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ShellCommandBlock, {
      command: "cargo test",
      isError: true,
      result: shellResult({ stdout: "", stderr: "boom", exit_code: 101 }),
    }),
  );
  assert.match(markup, /Exit code 101/);
  assert.match(markup, /boom/);
  assert.match(markup, /bg-destructive\/10/);
});

test("shellCommandBlock_success_keepsMutedTone", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ShellCommandBlock, {
      command: "cargo test",
      isError: false,
      result: shellResult({ stdout: "ok", stderr: "noise", exit_code: 0 }),
    }),
  );
  assert.match(markup, /bg-muted/);
  assert.doesNotMatch(markup, /destructive/);
  assert.doesNotMatch(markup, /noise/);
});

// ── collapsed row tone ──────────────────────────────────────────────────────

test("compactSummaryTone_failed_staysDestructiveAcrossStates", () => {
  // No group-hover/group-open brightening: a failed row must not read as an
  // ordinary one the moment the pointer crosses it.
  const tone = compactSummaryTone(true);
  assert.match(tone, /text-destructive/);
  assert.doesNotMatch(tone, /group-hover/);
  assert.doesNotMatch(tone, /group-open/);
});

test("compactSummaryTone_default_isUnchanged", () => {
  assert.equal(compactSummaryTone(), compactSummaryTone(false));
  assert.match(compactSummaryTone(), /text-muted-foreground\/60/);
});

// ── collapsed row content ───────────────────────────────────────────────────

function renderRow(overrides = {}) {
  return renderToStaticMarkup(
    React.createElement(CompactToolSummaryRow, {
      action: null,
      duration: "1.2s",
      failed: false,
      fileEditSummary: null,
      kind: "shell",
      label: "Run Command",
      preview: "cargo test",
      thumbnailSrc: null,
      ...overrides,
    }),
  );
}

test("compactToolSummaryRow_failed_announcesTheFailureAndKeepsThePreview", () => {
  const markup = renderRow({ failed: true });
  assert.match(markup, /Tool call failed/);
  assert.match(markup, /cargo test/);
});

test("compactToolSummaryRow_failed_replacesTheFileEditSummary", () => {
  // A failed edit did not edit anything — showing "+3 −1 foo.rs" next to a
  // failure would be an outright false claim about the file.
  const markup = renderRow({
    failed: true,
    fileEditSummary: {
      additions: 3,
      deletions: 1,
      filename: "foo.rs",
      path: "src/foo.rs",
    },
  });
  assert.match(markup, /Tool call failed/);
  assert.doesNotMatch(markup, /foo\.rs/);
});

test("compactToolSummaryRow_succeeded_keepsTheFileEditSummary", () => {
  const markup = renderRow({
    fileEditSummary: {
      additions: 3,
      deletions: 1,
      filename: "foo.rs",
      path: "src/foo.rs",
    },
  });
  assert.doesNotMatch(markup, /Tool call failed/);
  assert.match(markup, /foo\.rs/);
});
