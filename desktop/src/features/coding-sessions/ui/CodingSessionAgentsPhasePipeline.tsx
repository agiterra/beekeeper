import { ChevronRight } from "lucide-react";

import {
  CODING_SESSION_AGENT_STATE_LABEL,
  type CodingSessionAgentState,
  type CodingSessionOrchestrationPhase,
  codingSessionAgentStateWord,
  formatCodingSessionPhaseCounts,
} from "@/features/coding-sessions/lib/codingSessionAgentsOrchestrationModel";
import { cn } from "@/shared/lib/cn";

/**
 * State hues only (DB12): running primary, done success, failed destructive,
 * waiting amber. Idle, stopped and unknown are muted. An identity accent never
 * colours one of these dots.
 */
const DOT_CLASS: Readonly<Record<CodingSessionAgentState, string>> = {
  running: "bg-primary",
  waiting: "bg-amber-500",
  idle: "bg-muted-foreground/50",
  done: "bg-emerald-500",
  failed: "bg-destructive",
  stopped: "bg-muted-foreground/60",
  unknown: "bg-muted-foreground/30",
};

/** One agent's state dot, with its state word for a screen reader. */
export function CodingSessionAgentStateDot({
  label,
  state,
  word = CODING_SESSION_AGENT_STATE_LABEL[state],
}: {
  label?: string;
  state: CodingSessionAgentState;
  /** The state in words; defaults to the state's own label. */
  word?: string;
}) {
  return (
    <span
      aria-label={label ? `${label}: ${word}` : word}
      className={cn("size-1.5 shrink-0 rounded-full", DOT_CLASS[state])}
      data-state={state}
      data-testid="coding-session-agents-dot"
      role="img"
      title={label ? `${label} · ${word}` : word}
    />
  );
}

/** The phase's text and border tone, by its state. */
export function codingSessionPhaseToneClass(
  phase: Pick<CodingSessionOrchestrationPhase, "state">,
): string {
  switch (phase.state) {
    case "running":
      return "text-primary";
    case "done":
      return "text-emerald-600 dark:text-emerald-400";
    case "failed":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}

/**
 * The run's shape at a glance (T3 `PhaseRail`): one chip per phase in route
 * order, separated by chevrons, each with a dot per agent. A phase with no
 * agent shows a dash, and a declared one says so in its chip.
 */
export function CodingSessionAgentsPhasePipeline({
  phases,
}: {
  phases: readonly CodingSessionOrchestrationPhase[];
}) {
  if (phases.length === 0) return null;
  return (
    <ol
      aria-label="Phases"
      className="flex flex-wrap items-center gap-x-1 gap-y-1 px-1.5 pt-1.5 pb-1"
      data-testid="coding-session-agents-phase-pipeline"
    >
      {phases.map((phase, index) => (
        <li className="flex items-center gap-1" key={phase.key}>
          {index > 0 ? (
            <ChevronRight
              aria-hidden="true"
              className="size-3 text-muted-foreground/40"
            />
          ) : null}
          <span
            className={cn(
              "flex items-center gap-1 rounded-sm border px-1.5 py-0.5",
              phase.state === "running"
                ? "border-primary/40"
                : phase.state === "done"
                  ? "border-emerald-500/30"
                  : phase.state === "failed"
                    ? "border-destructive/40"
                    : "border-border/50",
            )}
            data-origin={phase.origin}
            data-state={phase.state}
            data-testid={`coding-session-agents-phase-chip-${phase.key}`}
            title={`${phase.title} · ${formatCodingSessionPhaseCounts(phase)}`}
          >
            <span
              className={cn(
                "font-mono text-2xs",
                codingSessionPhaseToneClass(phase),
              )}
            >
              {phase.state === "done" ? "✓ " : ""}
              {phase.title}
            </span>
            {phase.origin === "declared" ? (
              <span className="font-mono text-3xs text-muted-foreground">
                declared
              </span>
            ) : null}
            <span className="flex items-center gap-0.5">
              {phase.agents.length === 0 ? (
                <span
                  className="font-mono text-3xs text-muted-foreground/60"
                  title="No agent yet"
                >
                  <span aria-hidden="true">–</span>
                  <span className="sr-only">No agent yet</span>
                </span>
              ) : (
                phase.agents.map((agent) => (
                  <CodingSessionAgentStateDot
                    key={agent.executionKey}
                    label={agent.label}
                    state={agent.state}
                    word={codingSessionAgentStateWord(agent)}
                  />
                ))
              )}
            </span>
          </span>
        </li>
      ))}
    </ol>
  );
}
