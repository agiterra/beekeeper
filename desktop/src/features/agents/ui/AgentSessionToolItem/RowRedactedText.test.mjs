import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { RedactionDictionaryContext } from "../../../../shared/ui/redactionDictionary.ts";
import { TooltipProvider } from "../../../../shared/ui/tooltip.tsx";
import { CodingSessionActiveTool } from "../../../coding-sessions/ui/CodingSessionTranscriptParts.tsx";
import { CompactToolSummaryRow } from "./CompactToolSummaryRow.tsx";
import { RowRedactedText } from "./RowRedactedText.tsx";

// SV-75: a resolved path inside a command used to carry its own amber eye, so
// `mkdir -p /tmp/a && cd /tmp/b` read `mkdir -p /tmp/a 👁 && cd /tmp/b 👁`.

const DIGEST_A = "a".repeat(64);
const DIGEST_B = "b".repeat(64);
const DIGEST_UNKNOWN = "c".repeat(64);
const marker = (digest) =>
  `[elided private context: 20 bytes, sha256:${digest}]`;
const dictionary = new Map([
  [DIGEST_A, { class: "host-path", plaintext: "/tmp/bk-view-test" }],
  [DIGEST_B, { class: "host-path", plaintext: "/tmp/bk-view-test/src" }],
]);
const COMMAND = `mkdir -p ${marker(DIGEST_A)} && cd ${marker(DIGEST_B)}`;
const INTACT = "mkdir -p /tmp/bk-view-test && cd /tmp/bk-view-test/src";

function render(element, entries = dictionary) {
  return renderToStaticMarkup(
    React.createElement(
      RedactionDictionaryContext.Provider,
      { value: entries },
      React.createElement(TooltipProvider, null, element),
    ),
  );
}

/** Visible text with tags stripped, for "does the command read unbroken". */
function textOf(html) {
  return html.replace(/<[^>]*>/g, "").replace(/&amp;/g, "&");
}

function count(html, needle) {
  return html.split(needle).length - 1;
}

test("RowRedactedText_resolvedPaths_renderTheCommandUnbroken", () => {
  const html = render(React.createElement(RowRedactedText, { text: COMMAND }));
  assert.equal(textOf(html), INTACT);
  assert.equal(count(html, "data-redaction-revealed-badge"), 0);
  assert.equal(count(html, "data-redaction-revealed="), 2);
});

test("RowRedactedText_unresolvedMarker_staysAPillInPlace", () => {
  const html = render(
    React.createElement(RowRedactedText, {
      text: `cat ${marker(DIGEST_UNKNOWN)}`,
    }),
  );
  assert.equal(count(html, "data-redaction-pill"), 1);
  assert.ok(!html.includes("elided private context"));
});

test("CompactToolSummaryRow_shellCommand_oneMarkerBesideTheRow", () => {
  const html = render(
    React.createElement(CompactToolSummaryRow, {
      action: { verb: "Ran", object: COMMAND },
      duration: null,
      failed: false,
      fileEditSummary: null,
      kind: "shell",
      label: "Ran command",
      preview: COMMAND,
      thumbnailSrc: null,
    }),
  );
  assert.ok(textOf(html).includes(INTACT), textOf(html));
  // One disclosure for the row, naming both paths — the honesty signal stays.
  assert.equal(count(html, 'data-testid="redaction-row-marker"'), 1);
  assert.equal(count(html, "data-redaction-revealed-badge"), 1);
  assert.match(
    html,
    /aria-label="Redacted for other viewers — only you see these values: \/tmp\/bk-view-test, \/tmp\/bk-view-test\/src"/,
  );
});

test("CompactToolSummaryRow_quietFailure_commandUnbrokenOneMarker", () => {
  const html = render(
    React.createElement(CompactToolSummaryRow, {
      action: { verb: "Ran", object: COMMAND },
      duration: null,
      failed: true,
      failureDetail: "exit 1",
      failureTone: "quiet",
      fileEditSummary: null,
      kind: "shell",
      label: "Ran command",
      preview: COMMAND,
      thumbnailSrc: null,
    }),
  );
  assert.ok(textOf(html).includes(INTACT), textOf(html));
  assert.equal(count(html, 'data-testid="redaction-row-marker"'), 1);
});

test("CompactToolSummaryRow_nothingResolved_noMarker", () => {
  const html = render(
    React.createElement(CompactToolSummaryRow, {
      action: { verb: "Ran", object: COMMAND },
      duration: null,
      failed: false,
      fileEditSummary: null,
      kind: "shell",
      label: "Ran command",
      preview: COMMAND,
      thumbnailSrc: null,
    }),
    new Map(),
  );
  assert.equal(count(html, "redaction-row-marker"), 0);
  assert.equal(count(html, "data-redaction-pill"), 2);
});

test("CodingSessionActiveTool_label_unbrokenWithOneMarker", () => {
  const html = render(
    React.createElement(CodingSessionActiveTool, {
      disclosureId: "tool-1",
      item: {
        type: "tool",
        id: "tool-1",
        toolName: "Bash",
        descriptor: {
          renderClass: "tool",
          label: "Ran command",
          action: { verb: "Ran", object: COMMAND },
        },
        args: {},
        result: "",
        status: "running",
        isError: false,
        timestamp: "2026-10-05T00:00:00.000Z",
      },
      onOpenChange: () => {},
      open: false,
    }),
  );
  assert.equal(count(html, 'data-testid="redaction-row-marker"'), 1);
  assert.equal(count(html, "data-redaction-revealed-badge"), 1);
  assert.ok(textOf(html).includes(INTACT), textOf(html));
});
