import * as React from "react";
import { Check, ChevronDown, PanelRightOpen, Users } from "lucide-react";

import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

/**
 * Identity palette, with attention hues deliberately excluded — the same rule
 * `PARTICIPANT_ACCENTS` documents. State owns amber and destructive; a seat
 * that happened to hash to amber wore the caution hue while nothing about it
 * needed attention, and the Mission card grammar made that a whole bordered
 * block rather than a thin rule.
 *
 * Amber is **replaced** by `primary`, not removed: this palette and
 * `PARTICIPANT_ACCENTS` are now the same four hues in the same order, so a
 * seat's turn block and its chip cannot disagree, and dropping to three
 * buckets would have made two seats collide on one hue a third of the time
 * instead of a quarter.
 */
const AGENT_ACCENTS = [
  {
    dot: "bg-sky-500",
    border: "border-sky-500/55",
    soft: "bg-sky-500/10",
    text: "text-sky-700 dark:text-sky-300",
  },
  {
    dot: "bg-violet-500",
    border: "border-violet-500/55",
    soft: "bg-violet-500/10",
    text: "text-violet-700 dark:text-violet-300",
  },
  {
    dot: "bg-emerald-500",
    border: "border-emerald-500/55",
    soft: "bg-emerald-500/10",
    text: "text-emerald-700 dark:text-emerald-300",
  },
  {
    dot: "bg-primary",
    border: "border-primary/55",
    soft: "bg-primary/10",
    text: "text-primary",
  },
] as const;

export type CodingSessionAgentAccent = (typeof AGENT_ACCENTS)[number];

export type CodingSessionAgentFocusItem = {
  executionKey: string;
  label: string;
  status: CodingSessionWorkspaceStatus;
};

export function codingSessionAgentAccent(
  executionKey: string,
): CodingSessionAgentAccent {
  let hash = 0;
  for (let index = 0; index < executionKey.length; index += 1) {
    hash = (hash * 31 + executionKey.charCodeAt(index)) >>> 0;
  }
  return AGENT_ACCENTS[hash % AGENT_ACCENTS.length] ?? AGENT_ACCENTS[0];
}

export function CodingSessionAgentFocus({
  agentSurfaceOpen = false,
  focusedExecutionKey,
  items,
  onFocus,
  onOpenAgents,
  surfaceHostId,
}: {
  agentSurfaceOpen?: boolean;
  focusedExecutionKey: string | null;
  items: readonly CodingSessionAgentFocusItem[];
  onFocus: (executionKey: string | null) => void;
  onOpenAgents?: () => void;
  surfaceHostId?: string;
}) {
  const [open, setOpen] = React.useState(false);
  const focused = items.find(
    (item) => item.executionKey === focusedExecutionKey,
  );
  const workingCount = items.filter(
    (item) => item.status.kind === "working",
  ).length;
  const summary = aggregateAgentStatus(items, workingCount);

  const selectFocus = (executionKey: string | null) => {
    onFocus(executionKey);
    setOpen(false);
  };

  return (
    <div data-testid="coding-session-agent-focus">
      <Popover onOpenChange={setOpen} open={open}>
        <PopoverTrigger asChild>
          <button
            aria-label={`${summary}. ${focused ? `Viewing ${focused.label}` : "Viewing all agents"}`}
            className={cn(
              "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full border border-border/60 bg-muted/25 px-2.5 text-xs text-muted-foreground transition-colors hover:bg-muted/45 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              workingCount > 0 && "coding-session-agent-breathe",
            )}
            data-testid="coding-session-agent-focus-trigger"
            data-working={workingCount > 0 ? "true" : undefined}
            type="button"
          >
            <span
              aria-hidden
              className={cn(
                "size-2 rounded-full",
                workingCount > 0 ? "bg-emerald-500" : "bg-muted-foreground/45",
              )}
            />
            <Users aria-hidden className="size-3.5" />
            <span className="whitespace-nowrap font-medium">{summary}</span>
            <ChevronDown aria-hidden className="size-3.5 opacity-60" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-80 p-2">
          <p className="px-2 pt-1 pb-2 text-xs font-medium text-muted-foreground">
            Read this session
          </p>
          <fieldset aria-label="Focus session by agent" className="grid gap-1">
            <button
              aria-pressed={focusedExecutionKey === null}
              className="flex min-h-10 items-center gap-3 rounded-xl px-3 py-2 text-left text-sm transition-colors hover:bg-muted/60"
              data-testid="coding-session-agent-focus-all"
              onClick={() => selectFocus(null)}
              type="button"
            >
              <span className="grid size-7 shrink-0 place-items-center rounded-full bg-muted">
                <Users aria-hidden className="size-3.5" />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block font-medium">All agents</span>
                <span className="block text-xs text-muted-foreground">
                  Complete session timeline
                </span>
              </span>
              <Check
                aria-hidden
                className={cn(
                  "size-4 shrink-0 text-primary",
                  focusedExecutionKey !== null && "invisible",
                )}
              />
            </button>
            {items.map((item) => {
              const accent = codingSessionAgentAccent(item.executionKey);
              const selected = focusedExecutionKey === item.executionKey;
              const working = item.status.kind === "working";
              return (
                <button
                  aria-label={`${selected ? "Show all agents" : `Focus ${item.label}`} — ${agentStatusLabel(item.status)}`}
                  aria-pressed={selected}
                  className="flex min-h-10 items-center gap-3 rounded-xl px-3 py-2 text-left text-sm transition-colors hover:bg-muted/60"
                  data-execution={item.executionKey}
                  data-testid="coding-session-agent-focus-chip"
                  key={item.executionKey}
                  onClick={() =>
                    selectFocus(selected ? null : item.executionKey)
                  }
                  type="button"
                >
                  <span
                    aria-hidden
                    className={cn(
                      "grid size-7 shrink-0 place-items-center rounded-full",
                      accent.soft,
                      working && "coding-session-agent-breathe",
                    )}
                  >
                    <span className={cn("size-2 rounded-full", accent.dot)} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium text-foreground">
                      {item.label}
                    </span>
                    <span className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
                      <span
                        aria-hidden
                        className={cn(
                          "size-1.5 rounded-full",
                          statusDotClass(item.status),
                        )}
                      />
                      {agentStatusLabel(item.status)}
                    </span>
                  </span>
                  <Check
                    aria-hidden
                    className={cn(
                      "size-4 shrink-0 text-primary",
                      !selected && "invisible",
                    )}
                  />
                </button>
              );
            })}
          </fieldset>
          {onOpenAgents ? (
            <button
              aria-controls={surfaceHostId}
              aria-expanded={agentSurfaceOpen}
              className="mt-2 flex min-h-9 w-full items-center gap-2 border-t border-border/55 px-3 pt-2 text-left text-xs text-muted-foreground transition-colors hover:text-foreground"
              data-testid="coding-session-agent-details-toggle"
              onClick={() => {
                onOpenAgents();
                setOpen(false);
              }}
              type="button"
            >
              <PanelRightOpen aria-hidden className="size-3.5" />
              {agentSurfaceOpen ? "Hide agent details" : "Show agent details"}
            </button>
          ) : null}
        </PopoverContent>
      </Popover>
    </div>
  );
}

function aggregateAgentStatus(
  items: readonly CodingSessionAgentFocusItem[],
  workingCount: number,
): string {
  if (workingCount > 0) {
    return `${items.length} agents · ${workingCount} working`;
  }
  const attentionCount = items.filter(
    (item) => item.status.kind === "unknown" && item.status.attention,
  ).length;
  if (attentionCount > 0) {
    return `${items.length} agents · ${attentionCount} need attention`;
  }
  return `${items.length} agents · idle`;
}

function agentStatusLabel(status: CodingSessionWorkspaceStatus): string {
  return status.kind === "working" ? "working" : status.label.toLowerCase();
}

function statusDotClass(status: CodingSessionWorkspaceStatus): string {
  if (status.kind === "working") return "bg-emerald-500";
  if (status.kind === "idle" || status.kind === "ended") {
    return "bg-muted-foreground/45";
  }
  // A seat blocked on a person is not attention-red and not resting-grey.
  if (status.kind === "waiting") return "bg-amber-500";
  // Founded and never started is neither attention nor rest: hollow neutral.
  if (status.kind === "founded") return "bg-muted-foreground/45";
  return status.attention ? "bg-destructive" : "bg-amber-500";
}
