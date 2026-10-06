import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { parseShellToolOutput } from "../agentSessionUtils.ts";
import {
  CompactToolSummaryRow,
  compactSummaryTone,
  describeCompactToolFailure,
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
  // SV-92: a successful call's plain-text output is its output too.
  assert.equal(shellBlockOutput(output, false), "command not found");
});

// claude-agent-acp 0.84.0's published Bash result (audit session becbd0cb,
// eventSeq 58): plain text in a `console` fence, no stdout/stderr envelope.
const FENCED_BASH_RESULT = "```console\nquiet-done\n```";

test("shellBlockOutput_success_showsFencedPlainTextUnfenced (SV-92)", () => {
  const output = parseShellToolOutput(FENCED_BASH_RESULT);
  assert.equal(output.stdout, "");
  assert.equal(shellBlockOutput(output, false), "quiet-done");
});

test("shellBlockOutput_success_keepsMultilineFencedOutput (SV-92)", () => {
  const output = parseShellToolOutput("```\n/Users/brian/repo\ntotal 16\n```");
  assert.equal(shellBlockOutput(output, false), "/Users/brian/repo\ntotal 16");
});

test("shellBlockOutput_success_envelopeStdoutStillWinsOverRaw (SV-92)", () => {
  const output = parseShellToolOutput(
    shellResult({ stdout: "", stderr: "noise\n", exit_code: 0 }),
  );
  assert.equal(shellBlockOutput(output, false), "");
});

test("shellBlockOutput_failure_keepsFencedRawAsReceived (SV-92)", () => {
  const output = parseShellToolOutput(
    "```console\nExit code 2\nno such file\n```",
  );
  assert.equal(
    shellBlockOutput(output, true),
    "```console\nExit code 2\nno such file\n```",
  );
});

test("ShellCommandBlock_success_rendersFencedOutput (SV-92)", () => {
  const html = renderToStaticMarkup(
    React.createElement(ShellCommandBlock, {
      command: "python3 -c \"print('quiet-done')\"",
      isError: false,
      result: FENCED_BASH_RESULT,
    }),
  );
  assert.match(html, /<pre[^>]*>quiet-done<\/pre>/);
  assert.doesNotMatch(html, /```/);
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

// ── quiet failure inside a fold (SV-02, D2) ─────────────────────────────────

test("compactSummaryTone_quietFailure_readsAsAnOrdinaryRow", () => {
  assert.equal(compactSummaryTone(true, "quiet"), compactSummaryTone(false));
  assert.match(compactSummaryTone(true, "alarm"), /text-destructive/);
});

test("compactToolSummaryRow_quietFailure_dimsTheIconAndStillSaysFailed", () => {
  const markup = renderRow({
    action: { verb: "Ran", object: "python3 demo/does_not_exist.py" },
    failed: true,
    failureDetail: "exit 2",
    failureTone: "quiet",
    label: "Ran command failed",
    preview: "python3 demo/does_not_exist.py",
  });
  assert.doesNotMatch(markup, /Tool call failed/);
  assert.match(markup, /data-failure-tone="quiet"/);
  assert.match(markup, /text-destructive\/40/);
  assert.match(markup, /aria-label="Failed"/);
  assert.match(markup, />Ran</);
  assert.match(markup, /python3 demo\/does_not_exist\.py/);
  assert.match(markup, /exit 2/);
  // The row text itself is not painted destructive.
  assert.doesNotMatch(markup, /class="[^"]*\btext-destructive\b(?!\/)/);
});

test("compactToolSummaryRow_quietFailure_defaultsTheSuffixToFailed", () => {
  const markup = renderRow({
    action: { verb: "Ran", object: "cargo test" },
    failed: true,
    failureTone: "quiet",
  });
  assert.match(markup, /· failed/);
});

test("compactToolSummaryRow_quietFailedEdit_neverClaimsTheEdit", () => {
  const markup = renderRow({
    action: { verb: "Edited", object: "foo.rs" },
    failed: true,
    failureTone: "quiet",
    fileEditSummary: {
      additions: 3,
      deletions: 1,
      filename: "foo.rs",
      path: "src/foo.rs",
    },
    kind: "file-edit",
    label: "Edit failed",
    preview: "foo.rs",
  });
  assert.doesNotMatch(markup, /Edited/);
  assert.match(markup, /Edit failed/);
  assert.doesNotMatch(markup, /\+3/);
});

test("describeCompactToolFailure names an exit code, a timeout, or just failed", () => {
  assert.equal(
    describeCompactToolFailure(JSON.stringify({ stdout: "", exit_code: 2 })),
    "exit 2",
  );
  assert.equal(
    describeCompactToolFailure(JSON.stringify({ timed_out: true })),
    "timed out",
  );
  assert.equal(describeCompactToolFailure("No such file"), "failed");
  assert.equal(
    describeCompactToolFailure(JSON.stringify({ exit_code: 0 })),
    "failed",
  );
});

test("describeCompactToolFailure reads Claude's plain-text Bash exit code", () => {
  // Claude's Bash tool reports a failure as text, not a JSON shell result —
  // the same form the transcript fixture uses ("Exit code 1\ncompiler failed").
  assert.equal(
    describeCompactToolFailure("Exit code 1\ncompiler failed"),
    "exit 1",
  );
  assert.equal(
    describeCompactToolFailure("Error: Exit code 127\nzsh: command not found"),
    "exit 127",
  );
  // Only a leading status line counts; output quoting one is not a status.
  assert.equal(
    describeCompactToolFailure("tests failed\nExit code 3 expected"),
    "failed",
  );
  assert.equal(describeCompactToolFailure("Exit code 0\n"), "failed");
});
