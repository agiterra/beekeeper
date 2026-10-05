import * as React from "react";
import { Bot } from "lucide-react";

import { ToolItem } from "@/features/agents/ui/AgentSessionToolItem";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { TranscriptActivityItem } from "@/features/agents/ui/activityRenderClasses/TranscriptActivityItem";
import {
  type CodingSessionSubagentPanel,
  type CodingSessionSubagentRow,
  formatCodingSessionSubagentFooter,
  formatCodingSessionSubagentMeta,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";
import { Markdown } from "@/shared/ui/markdown";
import {
  CodingSessionSubagentSpawnDetail,
  CodingSessionSubagentStatusIcon,
  revealCodingSessionSubagentInStream,
} from "./CodingSessionSubagentEntry";

const PANEL_AGENT_IDENTITY = {
  agentAvatarUrl: null,
  agentName: "Subagent",
};

/**
 * The Agents surface's "Direct spawns" section: every Task/Agent call the
 * session made, newest last. Hired seats are listed by the surface above it;
 * these ran inside a seat and answer to it.
 */
export function CodingSessionSubagentsSection({
  panel,
}: {
  panel: CodingSessionSubagentPanel;
}) {
  const [expanded, setExpanded] = React.useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const toggle = React.useCallback((id: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);
  if (panel.rows.length === 0) return null;
  return (
    <section
      aria-label="Subagents"
      className="flex flex-col"
      data-testid="coding-session-subagents-section"
    >
      <p className="pb-2 text-3xs font-semibold tracking-[0.12em] text-muted-foreground uppercase">
        Direct spawns
      </p>
      <div className="divide-y divide-border/60">
        {panel.rows.map((row) => (
          <SubagentRow
            expanded={expanded.has(row.id)}
            key={row.id}
            onToggle={toggle}
            row={row}
          />
        ))}
      </div>
      <p
        className="mt-2 flex items-center gap-2 border-t border-border/60 pt-2 text-2xs text-muted-foreground"
        data-testid="coding-session-subagents-footer"
      >
        <Bot className="size-3.5" />
        <span>{formatCodingSessionSubagentFooter(panel)}</span>
      </p>
    </section>
  );
}

function SubagentRow({
  expanded,
  onToggle,
  row,
}: {
  expanded: boolean;
  onToggle: (id: string) => void;
  row: CodingSessionSubagentRow;
}) {
  const meta = formatCodingSessionSubagentMeta(row);
  const [notInView, setNotInView] = React.useState(false);
  return (
    <article
      className="py-2"
      data-status={row.status}
      data-testid="coding-session-subagent-row"
    >
      <button
        aria-expanded={expanded}
        className="flex w-full min-w-0 flex-col gap-0.5 rounded-md px-1 py-0.5 text-left transition-colors hover:bg-muted/30"
        onClick={() => onToggle(row.id)}
        type="button"
      >
        <span className="flex w-full min-w-0 items-center gap-1.5">
          <span
            aria-hidden="true"
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              row.status === "running" && "bg-blue-500",
              row.status === "done" && "bg-muted-foreground/60",
              row.status === "stopped" && "bg-muted-foreground/30",
              row.status === "failed" && "bg-destructive",
              // Neither live nor settled: a hollow dot, no colour claim.
              row.status === "unknown" && "border border-muted-foreground/60",
            )}
          />
          <span className="min-w-0 truncate text-xs font-semibold">
            {row.title}
          </span>
          {row.type ? (
            <span className="max-w-28 shrink-0 truncate rounded-sm border border-border/60 px-1 font-mono text-3xs text-muted-foreground">
              {row.type}
            </span>
          ) : null}
          <span className="ml-auto flex shrink-0 items-center gap-1 font-mono text-2xs tabular-nums text-muted-foreground">
            {row.durationMs !== null
              ? formatCodingSessionDuration(row.durationMs)
              : null}
            {row.status === "stopped" ? <span>Stopped</span> : null}
            {row.status === "unknown" ? <span>Status unknown</span> : null}
            <CodingSessionSubagentStatusIcon status={row.status} />
          </span>
        </span>
        {row.latest ? (
          <span className="block w-full truncate text-2xs text-muted-foreground">
            {row.latest}
          </span>
        ) : null}
        {meta ? (
          <span className="block w-full truncate font-mono text-3xs text-muted-foreground/80">
            {meta}
          </span>
        ) : null}
      </button>
      {expanded ? (
        <div className="mt-2 ml-1 flex flex-col gap-2 border-l border-border/60 pl-3">
          <button
            className="self-start rounded-sm px-1 text-2xs text-muted-foreground hover:text-foreground"
            onClick={() =>
              setNotInView(!revealCodingSessionSubagentInStream(row.id))
            }
            type="button"
          >
            {notInView
              ? "Not on screen in the conversation — scroll to its turn"
              : "Show in conversation ›"}
          </button>
          <CodingSessionSubagentSpawnDetail
            renderChild={renderPanelChild}
            showHeader={false}
            spawn={row.spawn}
          />
        </div>
      ) : null}
    </article>
  );
}

/** The panel's own compact rendering of a subagent's items. */
function renderPanelChild(item: TranscriptItem): React.ReactNode {
  if (item.type === "message") {
    return (
      <Markdown
        className="text-xs leading-5"
        content={item.text.trim() || " "}
      />
    );
  }
  if (item.type === "tool") {
    return (
      <ToolItem
        {...PANEL_AGENT_IDENTITY}
        agentPubkey={item.sessionId ?? item.id}
        item={item}
      />
    );
  }
  return (
    <TranscriptActivityItem
      {...PANEL_AGENT_IDENTITY}
      agentPubkey={item.sessionId ?? item.id}
      item={item}
    />
  );
}
