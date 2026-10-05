import {
  Brain,
  CircleAlert,
  Eye,
  Hammer,
  ListTodo,
  type LucideIcon,
  MessageCircle,
  Search,
  ShieldCheck,
  SquarePen,
  SquareTerminal,
  Wrench,
} from "lucide-react";

import { cn } from "@/shared/lib/cn";
import type { CompactToolSummary } from "../agentSessionToolSummary";
import {
  ACTIVITY_ROW_FAILED_ICON_CLASS,
  ACTIVITY_ROW_ICON_CLASS,
} from "./ToolItemRowClasses";

/**
 * The 16px glyph at the head of a tool row (SV-06): what kind of step it was,
 * as T3 Code's `toolGroupSummaryIconName` / `workEntryIconName` choose it
 * (`components/chat/MessagesTimeline.tsx`) — a terminal for a command, an eye
 * for a read, a pen for an edit, a magnifier for a search, a wrench for
 * anything else, a hammer for a mix.
 */
export function compactToolRowIcon(
  summary: Pick<CompactToolSummary, "action" | "kind">,
): LucideIcon {
  if (summary.action?.verb === "Searched") return Search;
  switch (summary.kind) {
    case "shell":
      return SquareTerminal;
    case "file-read":
    case "skill-read":
    case "image":
      return Eye;
    case "file-edit":
      return SquarePen;
    case "message":
    case "relay-op":
      return MessageCircle;
    case "thought":
      return Brain;
    case "plan":
      return ListTodo;
    case "permission":
      return ShieldCheck;
    case "error":
      return CircleAlert;
    default:
      return Wrench;
  }
}

/** One glyph for a run of calls: theirs when they agree, a hammer when not. */
export function compactToolGroupIcon(
  summaries: ReadonlyArray<Pick<CompactToolSummary, "action" | "kind">>,
): LucideIcon {
  let shared: LucideIcon | null = null;
  for (const summary of summaries) {
    const icon = compactToolRowIcon(summary);
    if (shared === null) shared = icon;
    else if (shared !== icon) return Hammer;
  }
  return shared ?? Wrench;
}

/**
 * The leading icon itself. A failed call keeps its own glyph, tinted: dimmed
 * red when it is a quiet step in an opened fold (SV-02), full red otherwise.
 * The failure is announced on the icon, so a screen reader hears it on the
 * row's first word.
 */
export function ToolItemRowIcon({
  failed,
  icon: Icon,
  quiet,
}: {
  failed: boolean;
  icon: LucideIcon;
  quiet: boolean;
}) {
  if (!failed) {
    return (
      <Icon
        aria-hidden
        className={ACTIVITY_ROW_ICON_CLASS}
        data-testid="transcript-tool-row-icon"
      />
    );
  }
  return (
    <Icon
      aria-label="Failed"
      className={cn(
        ACTIVITY_ROW_FAILED_ICON_CLASS,
        !quiet && "text-destructive",
      )}
      data-failure-tone={quiet ? "quiet" : "alarm"}
      data-testid="transcript-tool-row-icon"
      role="img"
    />
  );
}
