import * as React from "react";
import { Check, Copy, Maximize2, Minimize2 } from "lucide-react";
import type { Components } from "react-markdown";
import { toast } from "sonner";

import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

import {
  copyTableText,
  readTableMarkdownRows,
  readTableRows,
  serializeTableRowsToCsv,
  serializeTableRowsToMarkdown,
} from "./MarkdownTableClipboard";
import { useHorizontalOverflow } from "./useHorizontalOverflow";

const COPIED_FEEDBACK_MS = 1200;
const TABLE_ACTION_CLASS =
  "text-muted-foreground hover:bg-accent/60 hover:text-foreground aria-pressed:bg-accent aria-pressed:text-foreground";

/** The widest a collapsed cell is drawn (`markdown.css`, SV-11). */
export const COLLAPSED_CELL_MAX_WIDTH = "24rem";

type MeasuredRow = {
  cells: ArrayLike<{ getBoundingClientRect: () => { width: number } }>;
};

/** Each column's widest rendered cell, in px, across every row given. */
export function measureColumnWidths(rows: ArrayLike<MeasuredRow>): number[] {
  const widths: number[] = [];
  for (const row of Array.from(rows)) {
    Array.from(row.cells).forEach((cell, column) => {
      widths[column] = Math.max(
        widths[column] ?? 0,
        cell.getBoundingClientRect().width,
      );
    });
  }
  return widths;
}

/**
 * Before cells expand, pin each header cell's minimum width to the width its
 * column has now, as T3 Code's `toggleExpanded` does, so expanding wraps text
 * inside the columns the reader was just looking at instead of reflowing them
 * narrower. Capped at the collapsed cell maximum so a pinned column can never
 * stop the table wrapping back into the reading column.
 */
export function pinColumnWidthsForExpand(table: HTMLTableElement) {
  const widths = measureColumnWidths(table.rows);
  const headerCells = table.tHead?.rows[0]?.cells;
  if (!headerCells) return;
  Array.from(headerCells).forEach((cell, column) => {
    const width = widths[column] ?? cell.getBoundingClientRect().width;
    cell.style.minWidth = `min(${Math.ceil(width)}px, ${COLLAPSED_CELL_MAX_WIDTH})`;
  });
}

/** The label each cell-mode toggle state offers (what a click will do). */
export function tableCellToggleLabel(expanded: boolean): string {
  return expanded ? "Collapse table cells" : "Expand table cells";
}

/**
 * A markdown table, everywhere markdown renders (SV-11).
 *
 * Header, row dividers and a horizontal scroller, with two actions under the
 * table as T3 Code draws them (`ChatMarkdown.tsx` `MarkdownTable`):
 *
 * - **Collapse / Expand cells.** Expanded (the default, T3's word-wrap
 *   default) wraps cells so the table fits the reading column; collapsed keeps
 *   every cell on one line, ends a cell wider than 24rem in an ellipsis, and
 *   lets the table scroll sideways. The ellipsis says text is hidden and the
 *   toggle is one click away; Copy table always copies every cell in full.
 * - **Copy table**, as Markdown (inline formatting and column alignment kept)
 *   or CSV (plain text).
 *
 * `[data-table-block]` stays on the scroller: the overflow fade and the width
 * audits measure that element. The wrapper is `[data-table-container]`.
 */
export function MarkdownTable({
  children,
  interactive = true,
}: {
  children?: React.ReactNode;
  /** Non-interactive renders (previews, search rows) draw no action row. */
  interactive?: boolean;
}) {
  const tableBlockRef = React.useRef<HTMLDivElement | null>(null);
  const tableRef = React.useRef<HTMLTableElement | null>(null);
  const copiedTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const [expanded, setExpanded] = React.useState(true);
  const [copied, setCopied] = React.useState(false);
  const [hasHiddenOverflow, measureOverflow] = useHorizontalOverflow(
    tableBlockRef,
    [expanded],
  );

  React.useEffect(
    () => () => {
      if (copiedTimerRef.current != null) clearTimeout(copiedTimerRef.current);
    },
    [],
  );

  const handleCopy = React.useCallback(async (format: "markdown" | "csv") => {
    const table = tableRef.current;
    if (!table) return;
    const text =
      format === "markdown"
        ? serializeTableRowsToMarkdown(readTableMarkdownRows(table))
        : serializeTableRowsToCsv(readTableRows(table));
    try {
      await copyTableText(text);
      if (copiedTimerRef.current != null) clearTimeout(copiedTimerRef.current);
      setCopied(true);
      copiedTimerRef.current = setTimeout(() => {
        setCopied(false);
        copiedTimerRef.current = null;
      }, COPIED_FEEDBACK_MS);
    } catch (error) {
      console.error("Failed to copy table", error);
      toast.error("Failed to copy table");
    }
  }, []);

  const toggleExpanded = React.useCallback(() => {
    const table = tableRef.current;
    if (!expanded && table) pinColumnWidthsForExpand(table);
    setExpanded((value) => !value);
  }, [expanded]);

  const expandLabel = tableCellToggleLabel(expanded);
  const copyLabel = copied ? "Copied" : "Copy table";

  return (
    <div
      className="min-w-0 max-w-full"
      data-expanded={expanded ? "true" : "false"}
      data-table-container=""
    >
      <div
        ref={tableBlockRef}
        className="beekeeper-code-scrollbar min-w-0 max-w-full overflow-x-auto"
        data-overflow={hasHiddenOverflow ? "true" : "false"}
        data-table-block=""
        onScroll={measureOverflow}
      >
        <table ref={tableRef} className="border-collapse text-left text-sm">
          {children}
        </table>
      </div>
      {interactive ? (
        <div className="mt-0.5 flex select-none items-center justify-between">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                aria-label={expandLabel}
                aria-pressed={expanded}
                className={TABLE_ACTION_CLASS}
                data-testid="markdown-table-cells-toggle"
                onClick={toggleExpanded}
                size="icon-xs"
                type="button"
                variant="ghost"
              >
                {expanded ? (
                  <Minimize2 aria-hidden="true" />
                ) : (
                  <Maximize2 aria-hidden="true" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{expandLabel}</TooltipContent>
          </Tooltip>
          <DropdownMenu>
            <Tooltip>
              <TooltipTrigger asChild>
                <DropdownMenuTrigger asChild>
                  <Button
                    aria-label={copyLabel}
                    className={TABLE_ACTION_CLASS}
                    data-testid="markdown-table-copy"
                    size="icon-xs"
                    type="button"
                    variant="ghost"
                  >
                    {copied ? (
                      <Check aria-hidden="true" />
                    ) : (
                      <Copy aria-hidden="true" />
                    )}
                  </Button>
                </DropdownMenuTrigger>
              </TooltipTrigger>
              <TooltipContent>{copyLabel}</TooltipContent>
            </Tooltip>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => void handleCopy("markdown")}>
                Copy as Markdown
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => void handleCopy("csv")}>
                Copy as CSV
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      ) : null}
    </div>
  );
}

/**
 * The markdown renderer's `table`, `th` and `td`. Cells keep `style`, which
 * is where a GFM column's alignment (`| :-: |` → `text-align`) arrives;
 * dropping it drew every column left-aligned and lost it on Copy as Markdown.
 */
export function createTableComponents(
  interactive: boolean,
): Pick<Components, "table" | "td" | "th"> {
  return {
    table: ({ children }) => (
      <MarkdownTable interactive={interactive}>{children}</MarkdownTable>
    ),
    td: ({ children, style }) => (
      <td className="px-3 py-2 align-top" style={style}>
        {children}
      </td>
    ),
    th: ({ children, style }) => (
      <th className="px-3 py-2 font-semibold text-foreground" style={style}>
        {children}
      </th>
    ),
  };
}
