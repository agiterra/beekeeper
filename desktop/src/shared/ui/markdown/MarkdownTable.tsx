import * as React from "react";

import { useSmoothCorners } from "@/shared/ui/smoothCorners";

import { useHorizontalOverflow } from "./useHorizontalOverflow";

export function MarkdownTable({ children }: { children?: React.ReactNode }) {
  const tableBlockRef = React.useRef<HTMLDivElement | null>(null);
  useSmoothCorners(tableBlockRef);
  const [hasHiddenOverflow, measureOverflow] =
    useHorizontalOverflow(tableBlockRef);

  return (
    <div
      ref={tableBlockRef}
      className="buzz-code-scrollbar min-w-0 max-w-full overflow-x-auto rounded-2xl border border-border/70"
      data-overflow={hasHiddenOverflow ? "true" : "false"}
      data-table-block=""
      onScroll={measureOverflow}
    >
      <table className="w-max min-w-full border-collapse text-left text-sm">
        {children}
      </table>
    </div>
  );
}
