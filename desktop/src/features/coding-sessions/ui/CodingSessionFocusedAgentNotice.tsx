import { X } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import {
  codingSessionAgentAccent,
  type CodingSessionAgentFocusItem,
} from "./CodingSessionAgentFocus";

/**
 * The line above a narrative that has been narrowed to one agent.
 *
 * Focus hides other agents' turns, and a timeline that quietly omits work is
 * the same lie as one that invents it — so the narrowing says whose view this
 * is and offers the way back in the same row.
 */
export function CodingSessionFocusedAgentNotice({
  agent,
  onClear,
}: {
  agent: CodingSessionAgentFocusItem;
  onClear: () => void;
}) {
  const accent = codingSessionAgentAccent(agent.executionKey);
  const working = agent.status.kind === "working";
  return (
    <div
      className="mb-5 flex min-h-8 items-center gap-2 border-b border-border/45 pb-3 text-xs text-muted-foreground"
      data-testid="coding-session-focused-agent-notice"
    >
      <span
        aria-hidden
        className={cn(
          "grid size-5 shrink-0 place-items-center rounded-full",
          accent.soft,
          working && "coding-session-agent-breathe",
        )}
      >
        <span className={cn("size-2 rounded-full", accent.dot)} />
      </span>
      <span className="min-w-0 truncate">
        Viewing{" "}
        <span className={cn("font-medium", accent.text)}>{agent.label}</span>
      </span>
      <button
        aria-label="Return to the complete session"
        className="ml-auto inline-flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted/55 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        data-testid="coding-session-focused-agent-clear"
        onClick={onClear}
        title="Show the complete session"
        type="button"
      >
        <X aria-hidden className="size-3.5" />
      </button>
    </div>
  );
}
