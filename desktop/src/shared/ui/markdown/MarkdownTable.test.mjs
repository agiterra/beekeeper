import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { TooltipProvider } from "../tooltip.tsx";
import {
  createTableComponents,
  MarkdownTable,
  measureColumnWidths,
  pinColumnWidthsForExpand,
  tableCellToggleLabel,
} from "./MarkdownTable.tsx";
import {
  cellAlign,
  readTableMarkdownRows,
  readTableRows,
  serializeCellToMarkdown,
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

test("SV-11: collapsed cells stay on one line and end in an ellipsis past 24rem", () => {
  const block = markdownCss.slice(markdownCss.indexOf("Tables (SV-11)"));
  // T3's collapsed cell: one line, capped at 24rem, an ellipsis disclosing
  // the cut. Expanded (the default) wraps and never truncates.
  assert.match(
    block,
    /\[data-table-container\]\[data-expanded="false"\] :is\(th, td\) \{\s*white-space: nowrap;\s*overflow: hidden;\s*max-width: 24rem;\s*text-overflow: ellipsis;/,
  );
  const expandedRules = [
    ...block.matchAll(/\[data-expanded="true"\][^{]*\{[^}]*\}/g),
  ].map((match) => match[0]);
  assert.ok(expandedRules.length > 0);
  for (const rule of expandedRules) {
    assert.doesNotMatch(rule, /ellipsis|overflow: hidden/);
  }
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

// ── Copy as Markdown keeps what the agent wrote (T3 `serializeTable`) ──

const TEXT = 3;
const ELEMENT = 1;
function text(value) {
  return { nodeType: TEXT, textContent: value };
}
function el(tagName, attrs, ...children) {
  const attributes = attrs ?? {};
  const classes = (attributes.class ?? "").split(/\s+/).filter(Boolean);
  return {
    nodeType: ELEMENT,
    tagName: tagName.toUpperCase(),
    localName: tagName,
    childNodes: children,
    textContent: children.map((child) => child.textContent ?? "").join(""),
    getAttribute: (name) => attributes[name] ?? null,
    classList: { contains: (token) => classes.includes(token) },
    style: attributes.style ?? {},
  };
}

test("SV-11: a copied cell keeps bold, emphasis, strike, code and links", () => {
  const cell = el(
    "td",
    null,
    el("strong", null, text("Have it")),
    text(": the "),
    el("code", null, text("supersedes")),
    text(" pattern, "),
    el("em", null, text("not")),
    text(" "),
    el("del", null, text("old")),
    text(" "),
    el("a", { href: "https://example.com/x" }, text("docs")),
  );
  assert.equal(
    serializeCellToMarkdown(cell),
    "**Have it**: the `supersedes` pattern, *not* ~~old~~ [docs](https://example.com/x)",
  );
});

test("SV-11: a copied cell drops controls and hidden text, keeps app links as text", () => {
  const cell = el(
    "td",
    null,
    text("run "),
    el("code", null, text("a`b")),
    el("span", { "data-markdown-copy-skip": "" }, text("Copy")),
    el("span", { "aria-hidden": "true" }, text("icon")),
    el("span", { class: "sr-only" }, text("screen reader")),
    text(" in "),
    el("a", { href: "beekeeper://message?id=1" }, text("thread")),
    el("br", null),
    text("next"),
  );
  assert.equal(serializeCellToMarkdown(cell), "run ``a`b`` in thread next");
});

// Interactive markdown renders pills, chips and image triggers as buttons
// (RedactedPill, BeekeeperLinkChip, MessageLinkPill, the image zoom trigger).
// "Copy table always copies every cell in full" must hold for them too, and
// the two formats must agree about the words a reader sees.

function svgIcon() {
  return {
    nodeType: ELEMENT,
    tagName: "svg",
    localName: "svg",
    childNodes: [],
    textContent: "",
    getAttribute: (name) => (name === "aria-hidden" ? "true" : null),
    classList: { contains: () => false },
  };
}

function interactiveCellsTable() {
  const row = (...cells) => ({ cells });
  const redactionCell = el(
    "td",
    null,
    text("token: "),
    el(
      "button",
      {
        class: "mx-px inline-flex select-none",
        "data-redaction-pill": "",
        "data-elision-cause": "redaction",
        type: "button",
      },
      svgIcon(),
      text("redacted 32 B"),
    ),
  );
  const chipCell = el(
    "td",
    null,
    text("owner: "),
    el("button", { type: "button", class: "cursor-pointer" }, text("@alice")),
    text(" in "),
    el(
      "button",
      { type: "button", "data-message-link": "", class: "truncate" },
      el("span", null, text("#general · 1a2b3c")),
    ),
  );
  const imageCell = el(
    "td",
    null,
    text("shot "),
    el(
      "button",
      {
        type: "button",
        "data-image-lightbox-trigger": "",
        "data-image-lightbox-src": "https://example.com/a.png",
        "data-image-lightbox-alt": "diagram",
      },
      el(
        "span",
        { "data-progressive-image-frame": "" },
        el("img", { alt: "", "aria-hidden": "true", src: "blob:thumb" }),
      ),
    ),
  );
  return {
    tHead: { rows: [row(el("th", null, text("Field")))] },
    tBodies: [{ rows: [row(redactionCell), row(chipCell), row(imageCell)] }],
  };
}

test("SV-11: Copy as Markdown keeps redaction pills, chips and images in cells", () => {
  const table = interactiveCellsTable();
  assert.equal(
    serializeTableRowsToMarkdown(readTableMarkdownRows(table)),
    [
      "| Field |",
      "| --- |",
      "| token: [redacted 32 B] |",
      "| owner: @alice in #general · 1a2b3c |",
      "| shot ![diagram](https://example.com/a.png) |",
    ].join("\n"),
  );
});

test("SV-11: Copy as Markdown and Copy as CSV show the same words for interactive cells", () => {
  const table = interactiveCellsTable();
  const words = (value) => value.match(/[\p{L}\p{N}@#]+/gu) ?? [];
  const markdownRows = readTableMarkdownRows(table).body;
  const csvRows = readTableRows(table).body;
  // The redaction and chip rows: every word CSV shows, Markdown shows.
  for (const index of [0, 1]) {
    assert.deepEqual(
      words(markdownRows[index][0]),
      words(csvRows[index][0]),
      `row ${index}: ${markdownRows[index][0]} vs ${csvRows[index][0]}`,
    );
  }
  // The image row: CSV has only the text; Markdown keeps it and the image.
  assert.equal(csvRows[2][0], "shot");
  assert.match(markdownRows[2][0], /^shot !\[diagram\]/);
});

test("SV-11: an image without alt text still copies, a lightbox-less IMG too", () => {
  const cell = el(
    "td",
    null,
    el("img", { alt: "", src: "https://example.com/b.png" }),
  );
  assert.equal(serializeCellToMarkdown(cell), "![](https://example.com/b.png)");
});

test("SV-11: Copy as Markdown writes the header's column alignment", () => {
  assert.equal(cellAlign({ style: { textAlign: "center" } }), "center");
  assert.equal(cellAlign({ getAttribute: () => "right" }), "right");
  assert.equal(cellAlign({ style: { textAlign: "" } }), "");
  const markdown = serializeTableRowsToMarkdown({
    header: [["Name", "Count", "Note", "Plain"]],
    body: [["a", "1", "x", "y"]],
    align: ["left", "right", "center", ""],
  });
  assert.equal(markdown.split("\n")[1], "| :--- | ---: | :---: | --- |");
});

test("SV-11: readTableMarkdownRows reads formatted cells and alignment", () => {
  const row = (...cells) => ({ cells });
  const table = {
    tHead: {
      rows: [
        row(
          el("th", { style: { textAlign: "" } }, text("Item")),
          el("th", { style: { textAlign: "right" } }, text("Kind")),
        ),
      ],
    },
    tBodies: [
      {
        rows: [
          row(
            el("td", null, el("strong", null, text("Gate"))),
            el("td", null, el("code", null, text("44246"))),
          ),
        ],
      },
    ],
  };
  const rows = readTableMarkdownRows(table);
  assert.deepEqual(rows.align, ["", "right"]);
  assert.equal(
    serializeTableRowsToMarkdown(rows),
    "| Item | Kind |\n| --- | ---: |\n| **Gate** | `44246` |",
  );
  // CSV stays plain text.
  assert.equal(
    serializeTableRowsToCsv(readTableRows(table)),
    "Item,Kind\nGate,44246",
  );
});

test("SV-11: rendered cells keep the column alignment style", () => {
  const { td, th } = createTableComponents(true);
  const html = renderToStaticMarkup(
    React.createElement(
      "table",
      null,
      React.createElement(
        "tbody",
        null,
        React.createElement(
          "tr",
          null,
          th({ children: "H", style: { textAlign: "center" } }),
          td({ children: "1", style: { textAlign: "right" } }),
        ),
      ),
    ),
  );
  assert.match(html, /<th[^>]*style="text-align:center"/);
  assert.match(html, /<td[^>]*style="text-align:right"/);
});

// ── Expanding keeps the columns the reader was looking at (T3) ─────────

function measuredCell(width) {
  return {
    style: {},
    getBoundingClientRect: () => ({ width }),
  };
}

test("SV-11: column widths are the widest cell in each column", () => {
  const rows = [
    { cells: [measuredCell(80), measuredCell(200)] },
    { cells: [measuredCell(120), measuredCell(40), measuredCell(10)] },
  ];
  assert.deepEqual(measureColumnWidths(rows), [120, 200, 10]);
  assert.deepEqual(measureColumnWidths([]), []);
});

test("SV-11: expanding pins header cells to their collapsed widths, capped at 24rem", () => {
  const header = [measuredCell(100.4), measuredCell(30)];
  const body = [measuredCell(90), measuredCell(700)];
  const table = {
    rows: [{ cells: header }, { cells: body }],
    tHead: { rows: [{ cells: header }] },
  };
  pinColumnWidthsForExpand(table);
  assert.equal(header[0].style.minWidth, "min(101px, 24rem)");
  assert.equal(header[1].style.minWidth, "min(700px, 24rem)");
  // A headerless table has nothing to pin and does not throw.
  pinColumnWidthsForExpand({ rows: [], tHead: null });
});
