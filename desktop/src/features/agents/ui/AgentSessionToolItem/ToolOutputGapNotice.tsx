import { TriangleAlert } from "lucide-react";

import type { TranscriptToolOutputGap } from "../agentSessionTypes";
import {
  formatToolOutputGapDetail,
  TOOL_OUTPUT_GAP_HEADLINE,
} from "../agentSessionUtils";

/**
 * Said beside a tool output the provider could not verify complete.
 *
 * The output below it is what was captured, not necessarily what the command
 * printed: the adapter is known to drop the beginning. Rendered only when the
 * item carries an `outputGap`; a complete, recovered or unlabelled result
 * shows nothing here.
 */
export function ToolOutputGapNotice({ gap }: { gap: TranscriptToolOutputGap }) {
  const detail = formatToolOutputGapDetail(gap);
  return (
    <p
      className="flex items-start gap-1.5 text-xs leading-5 text-amber-700 dark:text-amber-400"
      data-testid="transcript-tool-output-gap"
      role="note"
    >
      <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
      <span>
        {TOOL_OUTPUT_GAP_HEADLINE}
        {detail ? (
          <span className="text-muted-foreground"> · {detail}</span>
        ) : null}
      </span>
    </p>
  );
}
