/**
 * A privacy marker in tool output or prose reads as a quiet `hidden path` /
 * `hidden` chip, never as ninety characters of hash — and text that only looks
 * like a marker is left exactly as written.
 */
import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  ShellCommandBlock,
  shellOutputIsPathPerLine,
} from "../../features/agents/ui/AgentSessionToolItem/ShellCommandBlock.tsx";
import { FileContentBlock } from "../../features/agents/ui/FileContentBlock.tsx";
import { HiddenContextChip } from "./HiddenContextChip.tsx";
import { RedactedText } from "./RedactedPill.tsx";
import { TooltipProvider } from "./tooltip.tsx";

const DIGEST =
  "01de6a4ef05f5c4052a052531f89bff67836c5ba404d05e4ead0b883e561a88c";
const MARKER = `[elided private context: 31 bytes, sha256:${DIGEST}]`;
const DESCRIPTION = "Hidden before publishing — 31 bytes, sha256 01de6a4ef05f…";

function render(element) {
  return renderToStaticMarkup(
    React.createElement(TooltipProvider, null, element),
  );
}

/** What a reader sees: tags stripped, entities decoded. */
function visible(html) {
  return html
    .replace(/<[^>]*>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"');
}

const text = (value, props = {}) =>
  render(React.createElement(RedactedText, { text: value, ...props }));

test("a marker mid-line becomes a chip and keeps the text around it", () => {
  const html = text(`see ${MARKER} for details`);
  assert.equal(visible(html), "see hidden for details");
  assert.match(html, /data-hidden-context-chip=""/);
  assert.match(html, new RegExp(`aria-label="${DESCRIPTION}`));
  // The full digest stays reachable for verification (copy / tooltip)…
  assert.match(html, new RegExp(`data-redaction-digest="${DIGEST}"`));
  // …but is no longer readable text.
  assert.doesNotMatch(visible(html), /elided private context|[0-9a-f]{64}/);
});

test("a marker at line start that heads a path reads `hidden path`", () => {
  const html = text(`${MARKER}/src/main.rs:12: unused import`);
  assert.equal(visible(html), "hidden path/src/main.rs:12: unused import");
  assert.match(html, /data-hidden-path=""/);
});

test("a marker at line start with no path context reads `hidden`", () => {
  const html = text(`${MARKER} was rejected`);
  assert.equal(visible(html), "hidden was rejected");
  assert.doesNotMatch(html, /data-hidden-path/);
});

test("malformed markers stay verbatim", () => {
  for (const malformed of [
    `[elided private context: 31 bytes, sha256:${DIGEST.slice(0, 63)}]`,
    `[elided private context: 31 bytes, sha256:${DIGEST.toUpperCase()}]`,
    `[elided private context: lots bytes, sha256:${DIGEST}]`,
    `[elided private context: 31 bytes, sha256:${DIGEST}`,
    "[elided private context]",
    "…[elided 4096 bytes]…",
  ]) {
    const html = text(`a ${malformed} b`);
    assert.equal(visible(html), `a ${malformed} b`, malformed);
    assert.doesNotMatch(html, /data-hidden-context-chip/, malformed);
  }
});

test("a non-interactive chip is not a control and drops the full digest", () => {
  const html = render(
    React.createElement(HiddenContextChip, {
      interactive: false,
      marker: { bytes: 31, digest: DIGEST },
      pathShaped: true,
    }),
  );
  assert.doesNotMatch(html, /<button/);
  assert.equal(visible(html), "hidden path");
  assert.match(html, new RegExp(`title="${DESCRIPTION}"`));
  assert.doesNotMatch(html, new RegExp(DIGEST));
});

test("pwd output that was hidden reads `hidden path`, not a hash", () => {
  const html = render(
    React.createElement(ShellCommandBlock, {
      command: "pwd",
      isError: false,
      result: `${MARKER}\n`,
    }),
  );
  assert.match(visible(html), /pwd\s*hidden path/);
  assert.doesNotMatch(visible(html), /[0-9a-f]{64}/);
});

test("a hidden cd target in the command line is a chip too", () => {
  const html = render(
    React.createElement(ShellCommandBlock, {
      command: `cd ${MARKER} && cargo test`,
      isError: false,
      result: "ok",
    }),
  );
  assert.match(visible(html), /cd hidden path && cargo test/);
});

test("only path-printing commands make whole output lines paths", () => {
  assert.equal(shellOutputIsPathPerLine("pwd"), true);
  assert.equal(
    shellOutputIsPathPerLine("  git rev-parse --show-toplevel"),
    true,
  );
  assert.equal(shellOutputIsPathPerLine("realpath ."), true);
  assert.equal(shellOutputIsPathPerLine("pwd && cat secrets"), false);
  assert.equal(shellOutputIsPathPerLine("echo $TOKEN"), false);
});

test("file content lines render markers as chips", () => {
  const html = render(
    React.createElement(FileContentBlock, {
      lines: [{ kind: "context", text: `root = "${MARKER}"` }],
      path: "Cargo.toml",
    }),
  );
  assert.match(visible(html), /root = "hidden path"/);
});
