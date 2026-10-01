import type * as React from "react";
import {
  Bot,
  Check,
  ChevronDown,
  CircleStop,
  LoaderCircle,
  X,
} from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  type CodingSessionSubagentSpawn,
  type CodingSessionSubagentStatus,
  codingSessionSubagentTitle,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import type { CodingSessionTranscriptEntry } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";
import { Markdown } from "@/shared/ui/markdown";

type SubagentsEntry = Extract<
  CodingSessionTranscriptEntry,
  { kind: "subagents" }
>;

/** Renders one of a subagent's own items with the stream's row components. */
export type CodingSessionSubagentChildRenderer = (
  item: TranscriptItem,
) => React.ReactNode;

/**
 * The whole group reads running while any spawn runs, then failed, then
 * stopped, and done only when every spawn finished.
 */
function groupStatus(
  spawns: readonly CodingSessionSubagentSpawn[],
): CodingSessionSubagentStatus {
  const statuses = spawns.map((spawn) => spawn.status);
  if (statuses.includes("running")) return "running";
  if (statuses.includes("failed")) return "failed";
  return statuses.includes("stopped") ? "stopped" : "done";
}

export function CodingSessionSubagentStatusIcon({
  status,
}: {
  status: CodingSessionSubagentStatus;
}) {
  if (status === "running") {
    return (
      <LoaderCircle
        aria-label="Running"
        className="size-3.5 shrink-0 animate-spin text-muted-foreground motion-reduce:animate-none"
      />
    );
  }
  if (status === "stopped") {
    // Muted, unlike failed: the turn ended before the call answered, which is
    // not the subagent reporting an error.
    return (
      <CircleStop
        aria-label="Stopped"
        className="size-3.5 shrink-0 text-muted-foreground"
      />
    );
  }
  if (status === "failed") {
    return (
      <X aria-label="Failed" className="size-3.5 shrink-0 text-destructive" />
    );
  }
  return <Check aria-label="Done" className="size-3.5 shrink-0" />;
}

/**
 * A Task/Agent spawn in the lead's stream: one compact row, with the
 * subagent's own items behind it. Several spawns in a row share the row.
 */
export function CodingSessionSubagentEntry({
  disclosureId,
  entry,
  onOpenChange,
  open,
  renderChild,
}: {
  disclosureId: string;
  entry: SubagentsEntry;
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
  renderChild: CodingSessionSubagentChildRenderer;
}) {
  const status = groupStatus(entry.spawns);
  return (
    <details
      className="group/subagents"
      data-status={status}
      data-testid="coding-session-subagents"
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      <summary
        aria-label={entry.label}
        className="flex min-h-7 max-w-full w-fit cursor-pointer list-none items-center gap-2 rounded-md px-0.5 text-sm text-muted-foreground transition-colors hover:bg-muted/30 hover:text-foreground"
      >
        <Bot className="size-3.5 shrink-0" />
        <span className="min-w-0 truncate font-medium">{entry.label}</span>
        <CodingSessionSubagentStatusIcon status={status} />
        <ChevronDown className="size-3.5 shrink-0 transition-transform group-open/subagents:rotate-180" />
      </summary>
      <div className="mt-1 ml-1 flex flex-col gap-3 border-l border-border/60 pl-4">
        {entry.spawns.map((spawn) => (
          <CodingSessionSubagentSpawnDetail
            key={spawn.call.id}
            renderChild={renderChild}
            showHeader={entry.spawns.length > 1}
            spawn={spawn}
          />
        ))}
      </div>
    </details>
  );
}

/**
 * One subagent's own items, then its report. Shared by the stream row and the
 * Agents panel, each passing its own item renderer.
 */
export function CodingSessionSubagentSpawnDetail({
  renderChild,
  showHeader,
  spawn,
}: {
  renderChild: CodingSessionSubagentChildRenderer;
  showHeader: boolean;
  spawn: CodingSessionSubagentSpawn;
}) {
  const { status } = spawn;
  const report =
    status === "running" || status === "stopped"
      ? ""
      : spawn.call.result.trim();
  return (
    <div
      className="flex min-w-0 flex-col gap-1"
      data-subagent-call-id={spawn.call.id}
      data-testid="coding-session-subagent-spawn"
    >
      {showHeader ? (
        <p className="flex min-w-0 items-center gap-2 text-xs font-medium text-foreground/85">
          <CodingSessionSubagentStatusIcon status={status} />
          <span className="truncate">
            {codingSessionSubagentTitle(spawn.call)}
          </span>
        </p>
      ) : null}
      {spawn.children.length > 0 ? (
        <div className="flex min-w-0 flex-col gap-1">
          {spawn.children.map((child) => (
            <div className="min-w-0" key={child.id}>
              {renderChild(child)}
            </div>
          ))}
        </div>
      ) : (
        // An older provider dropped a subagent's items rather than publish
        // them; the call cannot tell that apart from a subagent that did
        // nothing, so it says only what it knows.
        <p className="text-xs text-muted-foreground">
          {status === "running"
            ? "No steps published yet."
            : "No steps of this subagent were published."}
        </p>
      )}
      {status === "stopped" ? (
        <p className="text-xs text-muted-foreground">
          Stopped — its turn ended before the subagent returned a result.
        </p>
      ) : null}
      {report ? (
        <div
          className={cn(
            "mt-1 rounded-md px-3 py-2",
            status === "failed" ? "bg-destructive/5" : "bg-muted/40",
          )}
          data-testid="coding-session-subagent-report"
        >
          <p className="text-2xs font-medium text-muted-foreground">
            {status === "failed" ? "Failed" : "Report"}
          </p>
          <Markdown className="text-sm leading-6" content={report} />
        </div>
      ) : null}
    </div>
  );
}

/**
 * Open and scroll to a spawn's row in the conversation, if it is mounted.
 * Best-effort: a virtualized-out row returns `false` and the caller keeps its
 * own inline view.
 */
export function revealCodingSessionSubagentInStream(
  callItemId: string,
): boolean {
  if (typeof document === "undefined") return false;
  const spawn = document.querySelector(
    `[data-testid="coding-session-transcript"] [data-subagent-call-id="${CSS.escape(callItemId)}"]`,
  );
  const group = spawn?.closest("details");
  if (!(group instanceof HTMLDetailsElement)) return false;
  // Setting `open` fires `toggle`, which the row already mirrors into the
  // transcript's disclosure state.
  group.open = true;
  group.scrollIntoView({ behavior: "smooth", block: "nearest" });
  return true;
}
