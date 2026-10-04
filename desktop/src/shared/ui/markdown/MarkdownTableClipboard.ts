/**
 * Copy a rendered markdown table as Markdown or CSV (SV-11), after T3 Code's
 * `serializeTableElementToMarkdown` / `serializeTableElementToCsv`.
 *
 * The serializers are pure over a grid of cell strings so they can be tested
 * without a DOM; `readTableRows` is the one DOM-facing step.
 */

import { copyTextToSystemClipboard } from "@/shared/api/tauriMedia";

/** One table, as rendered: the header rows' cells, then the body rows' cells. */
export type TableRows = {
  header: string[][];
  body: string[][];
};

function normalizeCell(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function cellsOf(row: HTMLTableRowElement): string[] {
  return Array.from(row.cells, (cell) => normalizeCell(cell.textContent ?? ""));
}

/** Read the rows a rendered `<table>` shows, header first. */
export function readTableRows(table: HTMLTableElement): TableRows {
  const header = table.tHead ? Array.from(table.tHead.rows, cellsOf) : [];
  const body: string[][] = [];
  for (const section of Array.from(table.tBodies)) {
    for (const row of Array.from(section.rows)) body.push(cellsOf(row));
  }
  return { header, body };
}

function markdownCell(value: string): string {
  return normalizeCell(value).replaceAll("|", "\\|");
}

/**
 * Serialize as a GitHub-flavoured Markdown table. A table without a header
 * row promotes its first body row, because GFM has no headerless table; every
 * row is padded to the widest row so a ragged table still parses.
 */
export function serializeTableRowsToMarkdown({ header, body }: TableRows) {
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
    `| ${Array.from({ length: width }, () => "---").join(" | ")} |`,
    ...rest.map(line),
  ].join("\n");
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
