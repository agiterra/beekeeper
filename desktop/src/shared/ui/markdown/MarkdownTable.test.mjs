import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { TooltipProvider } from "../tooltip.tsx";
import { MarkdownTable, tableCellToggleLabel } from "./MarkdownTable.tsx";
import {
  readTableRows,
  serializeTableRowsToCsv,
  serializeTableRowsToMarkdown,
} from "./MarkdownTableClipboard.ts";

const markdownCss = readFileSync(
  fileURLToPath(new URL("../../styles/globals/markdown.css", import.meta.url)),
  "utf8",
);

function tableChildren() {
  const h = React.createElement;
  return [
    h(
      "thead",
      { key: "head" },
      h("tr", null, h("th", null, "Check"), h("th", null, "Result")),
    ),
    h(
      "tbody",
      { key: "body" },
      h("tr", null, h("td", null, "Typecheck"), h("td", null, "0 errors")),
    ),
  ];
}

function renderTable(props = {}) {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(MarkdownTable, props, ...tableChildren()),
    ),
  );
}

test("SV-11: the scroller keeps [data-table-block]; a container wraps it", () => {
  const html = renderTable();
  assert.match(html, /<div[^>]*data-table-container=""/);
  // The overflow fade and the width audits measure [data-table-block], so it
  // stays on the element that scrolls.
  assert.match(
    html,
    /<div[^>]*class="[^"]*\boverflow-x-auto\b[^"]*"[^>]*data-table-block=""/,
  );
  assert.match(html, /data-overflow="false"/);
  assert.match(html, /<thead>[\s\S]*<th>Check<\/th>/);
});

test("SV-11: cells start expanded and offer to collapse", () => {
  const html = renderTable();
  assert.match(html, /data-expanded="true"/);
  assert.match(html, /aria-label="Collapse table cells"/);
  assert.match(html, /aria-pressed="true"/);
  assert.equal(tableCellToggleLabel(true), "Collapse table cells");
  assert.equal(tableCellToggleLabel(false), "Expand table cells");
});

test("SV-11: a copy action sits under every interactive table", () => {
  const html = renderTable();
  assert.match(html, /data-testid="markdown-table-copy"/);
  assert.match(html, /aria-label="Copy table"/);
  assert.match(html, /aria-haspopup="menu"/);
});

test("SV-11: a non-interactive render draws no action row", () => {
  const html = renderTable({ interactive: false });
  assert.match(html, /data-table-block=""/);
  assert.doesNotMatch(html, /markdown-table-copy/);
  assert.doesNotMatch(html, /markdown-table-cells-toggle/);
});

test("SV-11: neither cell mode truncates a cell", () => {
  const block = markdownCss.slice(markdownCss.indexOf("Tables (SV-11)"));
  // Collapsing changes layout (one line, scroll), never what is shown.
  assert.doesNotMatch(block, /text-overflow: ellipsis/);
  assert.match(
    block,
    /\[data-table-container\]\[data-expanded="false"\] :is\(th, td\) \{\s*white-space: nowrap;/,
  );
  assert.match(
    block,
    /\[data-table-container\]\[data-expanded="true"\] table \{\s*width: 100%;/,
  );
  // Header and row dividers, no header fill.
  assert.match(
    block,
    /\[data-table-container\] th \{[^}]*background: transparent;/,
  );
  assert.match(
    block,
    /\[data-table-container\] td \{[^}]*border-bottom: 1px solid/,
  );
});

test("SV-11: Copy as Markdown escapes pipes and pads ragged rows", () => {
  const markdown = serializeTableRowsToMarkdown({
    header: [["Command", "Exit"]],
    body: [["a | b", "0"], ["only one"]],
  });
  assert.equal(
    markdown,
    [
      "| Command | Exit |",
      "| --- | --- |",
      "| a \\| b | 0 |",
      "| only one |  |",
    ].join("\n"),
  );
});

test("SV-11: a headerless table promotes its first row (GFM needs a header)", () => {
  const markdown = serializeTableRowsToMarkdown({
    header: [],
    body: [
      ["x", "y"],
      ["1", "2"],
    ],
  });
  assert.equal(markdown, "| x | y |\n| --- | --- |\n| 1 | 2 |");
  assert.equal(serializeTableRowsToMarkdown({ header: [], body: [] }), "");
});

test("SV-11: Copy as CSV quotes commas, quotes and collapses whitespace", () => {
  const csv = serializeTableRowsToCsv({
    header: [["Name", "Note"]],
    body: [
      ["alpha", 'said "hi", left'],
      ["beta", "  two\n lines "],
    ],
  });
  assert.equal(
    csv,
    ["Name,Note", 'alpha,"said ""hi"", left"', "beta,two lines"].join("\n"),
  );
});

test("SV-11: readTableRows reads header rows, then every body section", () => {
  const cell = (text) => ({ textContent: text });
  const row = (...texts) => ({ cells: texts.map(cell) });
  const table = {
    tHead: { rows: [row("A", " B ")] },
    tBodies: [{ rows: [row("1", "2")] }, { rows: [row("3", "4")] }],
  };
  assert.deepEqual(readTableRows(table), {
    header: [["A", "B"]],
    body: [
      ["1", "2"],
      ["3", "4"],
    ],
  });
  assert.deepEqual(readTableRows({ tHead: null, tBodies: [] }), {
    header: [],
    body: [],
  });
});
