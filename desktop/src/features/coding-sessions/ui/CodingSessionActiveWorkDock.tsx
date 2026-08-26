import * as React from "react";
import {
  Check,
  Circle,
  CircleDot,
  ListChecks,
  TriangleAlert,
  X,
} from "lucide-react";

import type {
  CodingSessionTask,
  CodingSessionTaskModel,
} from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";
import {
  codingSessionAgentAccent,
  type CodingSessionAgentAccent,
} from "./CodingSessionAgentFocus";

export type CodingSessionActiveWorkAgent = {
  executionKey: string;
  label: string;
  model: CodingSessionTaskModel | null;
  turnKey: string;
};

/**
 * A T3-inspired top attachment for every execution working right now.
 * Completed and idle plans never enter this component; a changed turn key
 * reopens work that the operator dismissed during an earlier turn.
 */
export function CodingSessionActiveWorkDock({
  agents,
  focusedExecutionKey,
  onFocusAgent,
}: {
  agents: readonly CodingSessionActiveWorkAgent[];
  focusedExecutionKey: string | null;
  onFocusAgent: (executionKey: string) => void;
}) {
  const fingerprint = agents.map((agent) => agent.turnKey).join("|");
  const [dismissedFingerprint, setDismissedFingerprint] = React.useState<
    string | null
  >(null);
  if (agents.length === 0) return null;
  if (dismissedFingerprint === fingerprint) {
    return (
      <button
        aria-label={`Show active work for ${agents.length} ${agents.length === 1 ? "agent" : "agents"}`}
        className="mx-4 -mb-px flex h-9 max-w-[calc(100%-2rem)] items-center gap-2 self-start rounded-t-xl border border-b-0 border-border/70 bg-background px-3 text-xs text-muted-foreground shadow-sm hover:text-foreground"
        data-testid="coding-session-active-work-collapsed"
        onClick={() => setDismissedFingerprint(null)}
        type="button"
      >
        <ListChecks aria-hidden className="size-3.5" />
        <span className="font-medium">Active work</span>
        <span className="tabular-nums">{agents.length}</span>
      </button>
    );
  }

  const selected =
    agents.find((agent) => agent.executionKey === focusedExecutionKey) ??
    agents.find((agent) => agent.model !== null) ??
    agents[0];
  if (!selected) return null;
  const selectedAccent = codingSessionAgentAccent(selected.executionKey);

  return (
    <aside
      aria-label="Active agent work"
      className="max-h-[min(48vh,28rem)] overflow-y-auto rounded-t-3xl border border-border/70 bg-background px-5 pt-4 pb-10 shadow-lg"
      data-testid="coding-session-active-work-dock"
    >
      <header className="flex min-h-8 items-center gap-2">
        <ListChecks aria-hidden className="size-4 text-muted-foreground" />
        <h2 className="text-sm font-semibold">Active work</h2>
        <span className="text-xs text-muted-foreground tabular-nums">
          {agents.length} {agents.length === 1 ? "agent" : "agents"}
        </span>
        <button
          aria-label="Dismiss active work for these turns"
          className="ml-auto inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => setDismissedFingerprint(fingerprint)}
          type="button"
        >
          <X aria-hidden className="size-4" />
        </button>
      </header>

      <div className="mt-2 flex min-w-0 gap-1 overflow-x-auto pb-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {agents.map((agent) => {
          const accent = codingSessionAgentAccent(agent.executionKey);
          const active = agent.executionKey === selected.executionKey;
          const progress = agent.model
            ? `${agent.model.completedCount}/${agent.model.tasks.length}`
            : "working";
          return (
            <button
              aria-pressed={active}
              className={cn(
                "inline-flex min-w-0 shrink-0 items-center gap-2 rounded-lg border px-2.5 py-1.5 text-left text-xs transition-colors",
                active
                  ? cn(accent.border, accent.soft, accent.text)
                  : "border-transparent text-muted-foreground hover:border-border/60 hover:bg-muted/35 hover:text-foreground",
              )}
              data-testid="coding-session-active-work-agent"
              key={agent.executionKey}
              onClick={() => onFocusAgent(agent.executionKey)}
              type="button"
            >
              <span
                aria-hidden
                className={cn("size-2 rounded-full", accent.dot)}
              />
              <span className="max-w-44 truncate font-medium">
                {agent.label}
              </span>
              <span className="tabular-nums opacity-70">{progress}</span>
            </button>
          );
        })}
      </div>

      <ActivePlan accent={selectedAccent} agent={selected} />
    </aside>
  );
}

function ActivePlan({
  accent,
  agent,
}: {
  accent: CodingSessionAgentAccent;
  agent: CodingSessionActiveWorkAgent;
}) {
  if (!agent.model || agent.model.tasks.length === 0) {
    return (
      <p className="mt-3 border-t border-border/50 pt-3 text-sm text-muted-foreground">
        {agent.label} is working. No signed plan has been published for this
        turn.
      </p>
    );
  }
  return (
    <div className="mt-3 border-t border-border/50 pt-2">
      {agent.model.explanation ? (
        <p className="mb-1.5 text-xs text-muted-foreground">
          {agent.model.explanation}
        </p>
      ) : null}
      <ol aria-label={`${agent.label} tasks`} className="space-y-0.5">
        {agent.model.tasks.map((task) => (
          <ActiveTask accent={accent} key={task.id} task={task} />
        ))}
      </ol>
    </div>
  );
}

function ActiveTask({
  accent,
  task,
}: {
  accent: CodingSessionAgentAccent;
  task: CodingSessionTask;
}) {
  const active = task.status === "in_progress";
  const Icon =
    task.status === "completed"
      ? Check
      : active
        ? CircleDot
        : task.status === "blocked" || task.status === "failed"
          ? TriangleAlert
          : Circle;
  return (
    <li
      className={cn(
        "flex min-h-8 items-start gap-2.5 rounded-lg px-1.5 py-1 text-sm",
        active ? "text-foreground" : "text-muted-foreground/60",
      )}
      data-status={task.status}
    >
      <Icon
        aria-hidden
        className={cn(
          "mt-0.5 size-3.5 shrink-0",
          active && accent.text,
          task.status === "completed" && "text-emerald-500/70",
        )}
      />
      <span
        className={cn(
          "min-w-0 flex-1 wrap-break-word",
          active && "font-medium",
        )}
      >
        {task.text}
      </span>
      {active ? (
        <span className="shrink-0 text-2xs text-muted-foreground/60">now</span>
      ) : task.status === "completed" && task.elapsedMs !== undefined ? (
        <span className="shrink-0 text-2xs tabular-nums text-muted-foreground/55">
          {formatCodingSessionDuration(task.elapsedMs)}
        </span>
      ) : null}
    </li>
  );
}
