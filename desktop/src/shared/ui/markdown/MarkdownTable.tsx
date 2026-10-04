import * as React from "react";
import { Check, Copy, Maximize2, Minimize2 } from "lucide-react";
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
  readTableRows,
  serializeTableRowsToCsv,
  serializeTableRowsToMarkdown,
} from "./MarkdownTableClipboard";
import { useHorizontalOverflow } from "./useHorizontalOverflow";

const COPIED_FEEDBACK_MS = 1200;
const TABLE_ACTION_CLASS =
  "text-muted-foreground hover:bg-accent/60 hover:text-foreground aria-pressed:bg-accent aria-pressed:text-foreground";

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
 * - **Collapse / Expand cells.** Expanded (the default) wraps cells so the
 *   table fits the reading column; collapsed keeps every cell on one line and
 *   lets the table scroll sideways. Neither mode truncates a cell — collapsing
 *   changes layout, never what is shown.
 * - **Copy table**, as Markdown or CSV.
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
    const rows = readTableRows(table);
    const text =
      format === "markdown"
        ? serializeTableRowsToMarkdown(rows)
        : serializeTableRowsToCsv(rows);
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
        className="buzz-code-scrollbar min-w-0 max-w-full overflow-x-auto"
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
                onClick={() => setExpanded((value) => !value)}
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
