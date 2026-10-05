/**
 * Copy a rendered markdown table as Markdown or CSV (SV-11), after T3 Code's
 * `serializeTableElementToMarkdown` / `serializeTableElementToCsv`.
 *
 * The serializers are pure over a grid of cell strings so they can be tested
 * without a DOM; `readTableRows` is the one DOM-facing step.
 */

import { copyTextToSystemClipboard } from "@/shared/api/tauriMedia";

/** A GFM column alignment, as the header cell declares it. */
export type TableColumnAlign = "" | "left" | "center" | "right";

/** One table, as rendered: the header rows' cells, then the body rows' cells. */
export type TableRows = {
  header: string[][];
  body: string[][];
  /** Per-column alignment from the first header row; `---` when absent. */
  align?: TableColumnAlign[];
};

function normalizeCell(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function cellsOf(row: HTMLTableRowElement): string[] {
  return Array.from(row.cells, (cell) => normalizeCell(cell.textContent ?? ""));
}

/** Read the rows a rendered `<table>` shows, header first, as plain text. */
export function readTableRows(table: HTMLTableElement): TableRows {
  return readRowsWith(table, cellsOf);
}

function readRowsWith(
  table: HTMLTableElement,
  read: (row: HTMLTableRowElement) => string[],
): TableRows {
  const header = table.tHead ? Array.from(table.tHead.rows, read) : [];
  const body: string[][] = [];
  for (const section of Array.from(table.tBodies)) {
    for (const row of Array.from(section.rows)) body.push(read(row));
  }
  return { header, body };
}

// ── Cells as Markdown ─────────────────────────────────────────────────
// T3 Code's `serializeTable` (`markdown-clipboard.ts`) keeps a cell's inline
// formatting — bold, emphasis, strikethrough, code, links, images — so a
// copied table pastes back as the table the agent wrote, not a flattened one.
// Only the subset a GFM table cell can hold is handled; anything else
// contributes its text.

const TEXT_NODE = 3;
const ELEMENT_NODE = 1;
// Only what is never cell content is skipped: markup that renders no text,
// hidden text, and anything marked `data-markdown-copy-skip`. Buttons and
// `select-none` elements are NOT skipped: in interactive markdown a redaction
// pill, a mention or entity chip, a message-link pill and an image's zoom
// trigger are all buttons, and dropping them made a copied `token: ‹redacted›`
// read as an empty value and `owner: @alice` lose its owner. The table's own
// actions sit outside the `<table>`, so they never reach this walk.
const SKIPPED_TAGS = new Set(["INPUT", "SCRIPT", "STYLE", "TEMPLATE"]);
const SKIPPED_CLASS_NAMES = ["sr-only"];
const SKIP_ATTRIBUTE = "data-markdown-copy-skip";

/** The slice of the DOM `Node` interface the cell serializer reads. */
export type MarkdownCellNode = {
  nodeType: number;
  textContent: string | null;
  childNodes?: ArrayLike<MarkdownCellNode>;
  tagName?: string;
  localName?: string;
  getAttribute?: (name: string) => string | null;
  classList?: { contains: (token: string) => boolean };
};

function isSkippedElement(element: MarkdownCellNode): boolean {
  const tag = (element.tagName ?? "").toUpperCase();
  if (SKIPPED_TAGS.has(tag) || (element.localName ?? tag) === "svg") {
    return true;
  }
  if (element.getAttribute?.("aria-hidden") === "true") return true;
  if (element.getAttribute?.(SKIP_ATTRIBUTE) != null) return true;
  return SKIPPED_CLASS_NAMES.some((name) => element.classList?.contains(name));
}

/** Hoists surrounding whitespace outside the markers: " bold " → " **bold** ". */
function wrapInlineMarker(content: string, marker: string): string {
  const match = /^(\s*)([\s\S]*?)(\s*)$/.exec(content);
  const core = match?.[2] ?? "";
  if (!core) return content;
  return `${match?.[1] ?? ""}${marker}${core}${marker}${match?.[3] ?? ""}`;
}

/** A backtick fence one longer than the longest run inside the code. */
function wrapInlineCode(code: string): string {
  const longestRun = Math.max(
    0,
    ...(code.match(/`+/g) ?? []).map((run) => run.length),
  );
  const fence = "`".repeat(Math.max(1, longestRun + (longestRun > 0 ? 1 : 0)));
  const pad = code.startsWith("`") || code.endsWith("`") ? " " : "";
  return `${fence}${pad}${code}${pad}${fence}`;
}

function serializeChildren(node: MarkdownCellNode): string {
  let out = "";
  for (const child of Array.from(node.childNodes ?? [])) {
    out += serializeCellNode(child);
  }
  return out;
}

function serializeAnchor(anchor: MarkdownCellNode): string {
  const content = serializeChildren(anchor);
  const href = anchor.getAttribute?.("href") ?? "";
  // A relative or app-internal href would paste as a dead link; keep its text.
  if (!/^https?:\/\//i.test(href)) return content;
  const label = content.trim();
  if (!label) return "";
  if (label === href) return href;
  return `[${label}](${href})`;
}

function markdownImage(alt: string, src: string): string {
  return src ? `![${normalizeCell(alt)}](${src})` : "";
}

function serializeCellNode(node: MarkdownCellNode): string {
  if (node.nodeType === TEXT_NODE) return node.textContent ?? "";
  if (node.nodeType !== ELEMENT_NODE) return "";
  const markdownCopy = node.getAttribute?.("data-markdown-copy");
  if (markdownCopy != null) return markdownCopy;
  if (isSkippedElement(node)) return "";
  // A redaction or cap pill reads as its visible label, bracketed so the
  // paste still shows a value was withheld rather than absent.
  if (node.getAttribute?.("data-redaction-pill") != null) {
    const label = normalizeCell(node.textContent ?? "");
    return label ? `[${label}]` : "";
  }
  // An image's zoom trigger: the image as the markdown named it, whether or
  // not the full-size <img> has loaded yet.
  if (node.getAttribute?.("data-image-lightbox-trigger") != null) {
    const src = node.getAttribute?.("data-image-lightbox-src") ?? "";
    if (src)
      return markdownImage(
        node.getAttribute?.("data-image-lightbox-alt") ?? "",
        src,
      );
    return serializeChildren(node);
  }
  switch ((node.tagName ?? "").toUpperCase()) {
    case "BR":
      return " ";
    case "CODE":
      return wrapInlineCode(node.textContent ?? "");
    case "STRONG":
    case "B":
      return wrapInlineMarker(serializeChildren(node), "**");
    case "EM":
    case "I":
      return wrapInlineMarker(serializeChildren(node), "*");
    case "DEL":
    case "S":
      return wrapInlineMarker(serializeChildren(node), "~~");
    case "A":
      return serializeAnchor(node);
    case "IMG":
      return markdownImage(
        node.getAttribute?.("alt") ?? "",
        node.getAttribute?.("src") ?? "",
      );
    default:
      return serializeChildren(node);
  }
}

/** One rendered cell back as inline Markdown (pipes are escaped later). */
export function serializeCellToMarkdown(cell: MarkdownCellNode): string {
  return normalizeCell(serializeChildren(cell));
}

/** The GFM alignment a rendered header cell carries. */
export function cellAlign(cell: {
  style?: { textAlign?: string };
  getAttribute?: (name: string) => string | null;
}): TableColumnAlign {
  const align = cell.style?.textAlign || cell.getAttribute?.("align") || "";
  return align === "left" || align === "center" || align === "right"
    ? align
    : "";
}

/**
 * Read a rendered `<table>` for Copy as Markdown: cells keep their inline
 * formatting and the first header row's alignment is kept for the separator.
 */
export function readTableMarkdownRows(table: HTMLTableElement): TableRows {
  const rows = readRowsWith(table, (row) =>
    Array.from(row.cells, (cell) => serializeCellToMarkdown(cell)),
  );
  const firstHeader = table.tHead?.rows[0];
  if (firstHeader) rows.align = Array.from(firstHeader.cells, cellAlign);
  return rows;
}

function markdownCell(value: string): string {
  return normalizeCell(value).replaceAll("|", "\\|");
}

/**
 * Serialize as a GitHub-flavoured Markdown table. A table without a header
 * row promotes its first body row, because GFM has no headerless table; every
 * row is padded to the widest row so a ragged table still parses.
 */
export function serializeTableRowsToMarkdown({
  header,
  body,
  align = [],
}: TableRows) {
  // Extra header rows (HTML tables can have several) follow as body rows
  // rather than being dropped.
  const rows = [...header, ...body].filter((row) => row.length > 0);
  if (rows.length === 0) return "";
  const [head, ...rest] = rows;
  const width = Math.max(1, ...rows.map((row) => row.length));
  const line = (row: string[]) =>
    `| ${Array.from({ length: width }, (_, index) => markdownCell(row[index] ?? "")).join(" | ")} |`;
  return [
    line(head),
    `| ${Array.from({ length: width }, (_, index) => separatorFor(align[index])).join(" | ")} |`,
    ...rest.map(line),
  ].join("\n");
}

function separatorFor(align: TableColumnAlign | undefined): string {
  if (align === "center") return ":---:";
  if (align === "right") return "---:";
  if (align === "left") return ":---";
  return "---";
}

function csvCell(value: string): string {
  const normalized = normalizeCell(value);
  return /[",\n]/.test(normalized)
    ? `"${normalized.replaceAll('"', '""')}"`
    : normalized;
}

/** Serialize as CSV (RFC 4180 quoting), header rows first. */
export function serializeTableRowsToCsv({ header, body }: TableRows) {
  return [...header, ...body]
    .filter((row) => row.length > 0)
    .map((row) => row.map(csvCell).join(","))
    .join("\n");
}

/**
 * Put plain text on the clipboard: the web clipboard first, the native one
 * (Tauri) when the webview refuses. Rejects only when both fail, so the caller
 * can say so.
 */
export async function copyTableText(text: string): Promise<void> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch (error) {
    console.warn("Web clipboard refused the table; trying native", error);
  }
  await copyTextToSystemClipboard(text);
}
