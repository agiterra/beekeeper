import * as React from "react";
import { Bot, ChevronDown, ChevronRight } from "lucide-react";

import { ToolItem } from "@/features/agents/ui/AgentSessionToolItem";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { TranscriptActivityItem } from "@/features/agents/ui/activityRenderClasses/TranscriptActivityItem";
import {
  type CodingSessionSubagentPanel,
  type CodingSessionSubagentRow,
  formatCodingSessionSubagentFooter,
  formatCodingSessionSubagentMeta,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import {
  useCanOpenCodingSessionSubagent,
  useOpenCodingSessionSubagent,
} from "@/features/coding-sessions/lib/codingSessionSubagentNavigation";
import { deriveCodingSessionSubagentCard } from "@/features/coding-sessions/lib/codingSessionSubagentsCard";
import { cn } from "@/shared/lib/cn";
import { Markdown } from "@/shared/ui/markdown";
import {
  CodingSessionSubagentSpawnDetail,
  CodingSessionSubagentStatusIcon,
  revealCodingSessionSubagentInStream,
} from "./CodingSessionSubagentEntry";
import {
  CodingSessionSubagentElapsed,
  CodingSessionSubagentHoverCard,
  CodingSessionSubagentStatusDot,
} from "./CodingSessionSubagentHoverCard";

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
  const spawn = settledCodingSessionSubagentRowSpawn(row);
  const card = React.useMemo(
    () => deriveCodingSessionSubagentCard(spawn),
    [spawn],
  );
  // SV-80: the row's primary click opens the subagent's page; the chevron
  // beside it keeps the in-place steps. Without a page (no workspace, or no
  // call id from the producer) the row expands in place as before.
  const canOpenPage = useCanOpenCodingSessionSubagent();
  const openPage = useOpenCodingSessionSubagent();
  const parentToolId = card.parentToolId;
  const opensPage = canOpenPage && parentToolId !== null;
  return (
    <article
      className="py-2"
      data-parent-tool-id={parentToolId ?? undefined}
      data-status={row.status}
      data-testid="coding-session-subagent-row"
    >
      <div className="flex w-full min-w-0 items-start gap-1">
        <CodingSessionSubagentHoverCard card={card}>
          <button
            aria-expanded={opensPage ? undefined : expanded}
            aria-label={opensPage ? `Open ${row.title}` : undefined}
            className="group/subagent flex w-full min-w-0 flex-col gap-0.5 rounded-md px-1 py-0.5 text-left transition-colors hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/70"
            data-testid="coding-session-subagent-row-open"
            onClick={() =>
              opensPage && parentToolId !== null
                ? openPage(parentToolId)
                : onToggle(row.id)
            }
            type="button"
          >
            <span className="flex w-full min-w-0 items-center gap-1.5">
              <CodingSessionSubagentStatusDot status={row.status} />
              <span className="min-w-0 truncate text-xs font-semibold">
                {row.title}
              </span>
              {row.type ? (
                <span className="max-w-28 shrink-0 truncate rounded-sm border border-border/60 px-1 font-mono text-3xs text-muted-foreground">
                  {row.type}
                </span>
              ) : null}
              <span className="ml-auto flex shrink-0 items-center gap-1 font-mono text-2xs tabular-nums text-muted-foreground">
                <CodingSessionSubagentElapsed card={card} />
                {row.status === "stopped" ? <span>Stopped</span> : null}
                {row.status === "unknown" ? <span>Status unknown</span> : null}
                <CodingSessionSubagentStatusIcon status={row.status} />
                {opensPage ? (
                  <ChevronRight
                    aria-hidden
                    className="size-3.5 shrink-0 text-muted-foreground/60 transition-colors group-hover/subagent:text-foreground"
                  />
                ) : null}
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
        </CodingSessionSubagentHoverCard>
        {opensPage ? (
          <button
            aria-expanded={expanded}
            aria-label={expanded ? "Hide steps here" : "Show steps here"}
            className="mt-0.5 inline-flex shrink-0 items-center rounded-sm p-0.5 text-muted-foreground/70 transition hover:bg-accent/60 hover:text-foreground"
            data-testid="coding-session-subagent-row-toggle"
            onClick={() => onToggle(row.id)}
            title={expanded ? "Hide steps here" : "Show steps here"}
            type="button"
          >
            <ChevronDown
              aria-hidden
              className={cn("size-3.5 transition", expanded && "rotate-180")}
            />
          </button>
        ) : null}
      </div>
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
            spawn={spawn}
          />
        </div>
      ) : null}
    </article>
  );
}

/**
 * The row's spawn as its turn's settlement reads it (SV-58). `row.spawn`
 * keeps the transcript-only status, which is `running` for an open call in a
 * turn the stream already shows as stopped; the expanded detail must say
 * what the row's own dot, label and icon say.
 */
export function settledCodingSessionSubagentRowSpawn(
  row: CodingSessionSubagentRow,
): Omit<CodingSessionSubagentRow["spawn"], "status"> & {
  status: CodingSessionSubagentRow["status"];
} {
  return row.spawn.status === row.status
    ? row.spawn
    : { ...row.spawn, status: row.status };
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
