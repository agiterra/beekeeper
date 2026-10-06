import * as React from "react";
import { Brain } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { Markdown } from "@/shared/ui/markdown";
import {
  ACTIVITY_ROW_DETAIL_INSET_CLASS,
  ACTIVITY_ROW_ICON_CLASS,
  ACTIVITY_ROW_LABEL_CLASS,
  ACTIVITY_ROW_LINE_CLASS,
} from "../AgentSessionToolItem/ToolItemRowClasses";
import { ToolActivity } from "./ToolActivity";
import { formatTranscriptTimestampTitle } from "../agentSessionUtils";
import type { TranscriptItem } from "../agentSessionTypes";
import type { ActivityRenderClassItemProps } from "./types";

export function ThoughtActivity(props: ActivityRenderClassItemProps) {
  const [open, setOpen] = React.useState(false);
  if (props.item.type === "tool") {
    return <ToolActivity {...props} />;
  }
  if (props.item.type !== "thought") {
    return null;
  }

  return (
    <ThoughtDisclosure item={props.item} onOpenChange={setOpen} open={open} />
  );
}

/**
 * A thought as one compact row — a brain and the word "Thought", dimmed and
 * closed (SV-05) — whose text is parsed only while open.
 *
 * `active` marks a thought still being written: the last entry of a live
 * turn. It reads "Thinking", with the working line's shimmer, as T3 Code's
 * in-progress reasoning row does; once anything follows it or the turn
 * settles the caller drops the flag and it reads "Thought". Without the
 * flag a live reasoning step would read as finished.
 *
 * `shimmer` (default: `active`) draws the sweep. A caller that knows more
 * than "still being written" passes it: the coding-session transcript moves
 * it only while the provider is fresh and motion is not reduced (SV-104).
 *
 * The producer's own title ("Reasoning", "Thinking", …) stays reachable as
 * the row's accessible name and tooltip; the row reads the same whichever
 * provider wrote it.
 *
 * Controlled, so the coding-session transcript can keep the open state
 * across a virtualizer remount. Reasoning can run to pages of Markdown, and a
 * closed row used to parse all of it on every render.
 */
export const ThoughtDisclosure = React.memo(function ThoughtDisclosure({
  active = false,
  item,
  onOpenChange,
  open,
  shimmer = active,
}: {
  active?: boolean;
  item: Extract<TranscriptItem, { type: "thought" }>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  shimmer?: boolean;
}) {
  const producerTitle = item.title.trim();
  const timestampTitle = formatTranscriptTimestampTitle(item.timestamp);
  return (
    <details
      className="group not-prose w-full"
      data-active={active ? "" : undefined}
      data-testid="transcript-thought-item"
      onToggle={(event) => onOpenChange(event.currentTarget.open)}
      open={open}
      title={
        producerTitle && producerTitle !== "Thought"
          ? [producerTitle, timestampTitle].filter(Boolean).join(" · ")
          : timestampTitle
      }
    >
      <summary
        className={cn(
          "group/row cursor-pointer list-none",
          ACTIVITY_ROW_LINE_CLASS,
        )}
      >
        <Brain aria-hidden className={ACTIVITY_ROW_ICON_CLASS} />
        <span
          className={cn(
            ACTIVITY_ROW_LABEL_CLASS,
            "transition-colors group-open:text-foreground/80",
            active && "relative overflow-hidden",
          )}
          data-live-shimmer={active ? (shimmer ? "on" : "off") : undefined}
          data-testid="transcript-thought-label"
        >
          {active ? "Thinking" : "Thought"}
          {active && shimmer ? (
            // The working line's shimmer (`coding-session.css`): a lit copy
            // of the word sweeps across it; reduced motion keeps it still.
            <span
              aria-hidden
              className="coding-session-live-activity-focus pointer-events-none absolute inset-y-0 select-none"
              data-testid="transcript-thought-shimmer"
            >
              <span className="coding-session-live-activity-counter block">
                <span className="coding-session-live-activity-aligned block text-foreground">
                  Thinking
                </span>
              </span>
            </span>
          ) : null}
        </span>
      </summary>
      {open ? (
        <div
          className={cn(
            ACTIVITY_ROW_DETAIL_INSET_CLASS,
            "pe-1 pt-1 pb-1.5 text-sm leading-5 text-muted-foreground",
          )}
        >
          <Markdown className="leading-5" content={item.text.trim() || " "} />
        </div>
      ) : null}
    </details>
  );
});
