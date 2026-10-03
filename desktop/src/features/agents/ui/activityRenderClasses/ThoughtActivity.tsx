import * as React from "react";
import { ChevronDown } from "lucide-react";

import { Markdown } from "@/shared/ui/markdown";
import { ActivityRowLabel } from "./ActivityRow";
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
 * A thought as one compact row; its text is parsed only while open.
 *
 * Controlled, so the coding-session transcript can keep the open state
 * across a virtualizer remount. Reasoning can run to pages of Markdown, and a
 * closed row used to parse all of it on every render.
 */
export const ThoughtDisclosure = React.memo(function ThoughtDisclosure({
  item,
  onOpenChange,
  open,
}: {
  item: Extract<TranscriptItem, { type: "thought" }>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
}) {
  return (
    <details
      className="group not-prose w-full"
      data-testid="transcript-thought-item"
      onToggle={(event) => onOpenChange(event.currentTarget.open)}
      open={open}
      title={formatTranscriptTimestampTitle(item.timestamp)}
    >
      <summary className="group/row flex min-h-6 w-full max-w-full cursor-pointer list-none items-center gap-1.5 text-muted-foreground group-open:text-foreground">
        <ActivityRowLabel openToneScope="tool" verb={item.title} />
        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground/60 transition-transform group-hover/row:text-foreground group-open:rotate-180 group-open:text-foreground" />
      </summary>
      {open ? (
        <div className="pt-1 pb-1.5 text-sm leading-5 text-muted-foreground">
          <Markdown className="leading-5" content={item.text.trim() || " "} />
        </div>
      ) : null}
    </details>
  );
});
