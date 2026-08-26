import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { cn } from "@/shared/lib/cn";

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
    dot: "bg-amber-500",
    border: "border-amber-500/55",
    soft: "bg-amber-500/10",
    text: "text-amber-700 dark:text-amber-300",
  },
  {
    dot: "bg-emerald-500",
    border: "border-emerald-500/55",
    soft: "bg-emerald-500/10",
    text: "text-emerald-700 dark:text-emerald-300",
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
  focusedExecutionKey,
  items,
  onFocus,
}: {
  focusedExecutionKey: string | null;
  items: readonly CodingSessionAgentFocusItem[];
  onFocus: (executionKey: string | null) => void;
}) {
  return (
    <fieldset
      aria-label="Focus session by agent"
      className="flex min-w-0 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      data-testid="coding-session-agent-focus"
    >
      {items.map((item) => {
        const accent = codingSessionAgentAccent(item.executionKey);
        const selected = focusedExecutionKey === item.executionKey;
        return (
          <button
            aria-label={`${selected ? "Show all agents" : `Focus ${item.label}`} — ${agentStatusLabel(item.status)}`}
            aria-pressed={selected}
            className={cn(
              "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-xs transition-colors",
              selected
                ? cn(accent.border, accent.soft, accent.text)
                : "border-transparent text-muted-foreground hover:border-border/70 hover:bg-muted/45 hover:text-foreground",
            )}
            data-execution={item.executionKey}
            data-testid="coding-session-agent-focus-chip"
            key={item.executionKey}
            onClick={() => onFocus(selected ? null : item.executionKey)}
            title={
              selected ? "Show the complete session" : `Focus ${item.label}`
            }
            type="button"
          >
            <span
              aria-hidden
              className={cn("size-2 rounded-full", accent.dot)}
            />
            <span className="max-w-36 truncate font-medium">{item.label}</span>
            <span
              aria-hidden
              className={cn(
                "size-1.5 rounded-full",
                statusDotClass(item.status),
              )}
            />
            <span className="text-2xs opacity-75">
              {agentStatusLabel(item.status)}
            </span>
          </button>
        );
      })}
    </fieldset>
  );
}

function agentStatusLabel(status: CodingSessionWorkspaceStatus): string {
  return status.kind === "working" ? "working" : status.label.toLowerCase();
}

function statusDotClass(status: CodingSessionWorkspaceStatus): string {
  if (status.kind === "working") return "bg-emerald-500";
  if (status.kind === "idle" || status.kind === "ended") {
    return "bg-muted-foreground/45";
  }
  return status.attention ? "bg-destructive" : "bg-amber-500";
}
