import * as React from "react";
import { Check, ChevronDown, ChevronRight } from "lucide-react";

import {
  type CodingSessionOrchestrationAgent,
  type CodingSessionOrchestrationCard,
  type CodingSessionOrchestrationPhase,
  codingSessionAgentStateWord,
  describeCodingSessionAgentTokens,
  formatCodingSessionAgentModel,
  formatCodingSessionAgentTokens,
  formatCodingSessionPhaseCounts,
  formatCodingSessionToolCount,
} from "@/features/coding-sessions/lib/codingSessionAgentsOrchestrationModel";
import { cn } from "@/shared/lib/cn";

import {
  CodingSessionAgentsPhasePipeline,
  CodingSessionAgentStateDot,
  codingSessionPhaseToneClass,
} from "./CodingSessionAgentsPhasePipeline";

/**
 * One mission's card (T3 `LiveWorkflowSection`): its name, `N/M settled`,
 * the phase pipeline, and a collapsible section per phase listing each agent
 * with its current tool, model, tokens and tool count.
 */
export function CodingSessionAgentsWorkflowCard({
  card,
}: {
  card: CodingSessionOrchestrationCard;
}) {
  return (
    <section
      aria-label={`${card.title}: ${card.settled} of ${card.total} settled${card.failed > 0 ? `, ${card.failed} failed` : ""}`}
      className="rounded-lg border border-border/50 bg-card/30 p-1.5"
      data-testid="coding-session-agents-workflow-card"
    >
      <header className="flex min-w-0 items-center gap-2 px-1.5 pt-0.5 text-2xs font-medium tracking-wider text-muted-foreground uppercase">
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            card.live
              ? "bg-primary"
              : card.failed > 0
                ? "bg-destructive"
                : "bg-muted-foreground/50",
          )}
        />
        <span className="min-w-0 truncate">{card.title}</span>
        <span
          className="ml-auto shrink-0 font-mono tracking-normal normal-case text-muted-foreground/80"
          data-testid="coding-session-agents-workflow-settled"
          title="Agents whose newest signed state is terminal"
        >
          {card.settled}/{card.total} settled
          {card.failed > 0 ? (
            <span className="text-destructive"> · {card.failed} failed</span>
          ) : null}
        </span>
      </header>
      <CodingSessionAgentsPhasePipeline phases={card.phases} />
      {card.phases.map((phase) => (
        <PhaseSection key={phase.key} phase={phase} />
      ))}
      {card.unphased.length > 0 ? (
        <div data-testid="coding-session-agents-unphased">
          <p className="mt-2 px-1.5 text-2xs font-medium tracking-wider text-muted-foreground/70 uppercase">
            No role
          </p>
          {card.unphased.map((agent) => (
            <AgentRow agent={agent} key={agent.executionKey} />
          ))}
        </div>
      ) : null}
    </section>
  );
}

/**
 * A collapsible phase (T3 `PhaseSection`): open by default while it runs,
 * collapsed to its header and dot row otherwise; a reader's toggle sticks.
 */
function PhaseSection({ phase }: { phase: CodingSessionOrchestrationPhase }) {
  const [userOpen, setUserOpen] = React.useState<boolean | null>(null);
  const open = userOpen ?? phase.state === "running";
  const canOpen = phase.agents.length > 0;
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <div data-state={phase.state} data-testid="coding-session-agents-phase">
      <button
        aria-expanded={canOpen ? open : undefined}
        className={cn(
          "mt-2 flex w-full items-center gap-1.5 rounded-sm px-1.5 text-left text-2xs font-medium tracking-wider uppercase hover:bg-accent/40",
          codingSessionPhaseToneClass(phase),
        )}
        data-testid={`coding-session-agents-phase-toggle-${phase.key}`}
        disabled={!canOpen}
        onClick={() => setUserOpen(!open)}
        type="button"
      >
        <Chevron aria-hidden="true" className="size-3 shrink-0" />
        {phase.state === "done" ? (
          <Check aria-hidden="true" className="size-3" />
        ) : null}
        <span>{phase.title}</span>
        {phase.origin === "declared" ? (
          <span className="rounded-sm border border-border/60 px-1 font-mono tracking-normal normal-case text-muted-foreground">
            declared
          </span>
        ) : null}
        <span className="font-normal tracking-normal normal-case text-muted-foreground/70">
          {formatCodingSessionPhaseCounts(phase)}
        </span>
        {!open && phase.agents.length > 0 ? (
          <span className="ml-auto flex items-center gap-0.5">
            {phase.agents.map((agent) => (
              <CodingSessionAgentStateDot
                key={agent.executionKey}
                label={agent.label}
                state={agent.state}
                word={codingSessionAgentStateWord(agent)}
              />
            ))}
          </span>
        ) : null}
      </button>
      {open && canOpen
        ? phase.agents.map((agent) => (
            <AgentRow agent={agent} key={agent.executionKey} />
          ))
        : null}
    </div>
  );
}

/** One agent: state, name, current tool, then model · tokens · tools. */
function AgentRow({ agent }: { agent: CodingSessionOrchestrationAgent }) {
  const live = agent.state === "running" || agent.state === "waiting";
  return (
    <div
      className="rounded-md px-1.5 py-1"
      data-state={agent.state}
      data-testid="coding-session-agents-agent-row"
    >
      <div className="flex items-start gap-2">
        <span className="flex h-5 items-center">
          <CodingSessionAgentStateDot
            label={agent.label}
            state={agent.state}
            word={codingSessionAgentStateWord(agent)}
          />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="min-w-0 truncate text-sm font-medium">
              {agent.label}
            </span>
            {agent.state === "done" ? (
              <Check
                aria-hidden="true"
                className="ml-auto size-3 shrink-0 text-emerald-600 dark:text-emerald-400"
              />
            ) : null}
          </span>
          {live ? (
            <span
              className="mt-0.5 block truncate text-xs text-muted-foreground"
              data-testid="coding-session-agents-agent-tool"
            >
              {agent.currentTool
                ? `▸ ${agent.currentTool}`
                : agent.waitingOn !== null
                  ? codingSessionAgentStateWord(agent)
                  : "No tool call open"}
            </span>
          ) : agent.outcome ? (
            <span
              className={cn(
                "mt-0.5 block truncate text-xs",
                agent.outcome.failed
                  ? "text-destructive"
                  : "text-muted-foreground",
              )}
              data-testid="coding-session-agents-agent-outcome"
              title={agent.outcome.text}
            >
              {agent.outcome.text}
            </span>
          ) : null}
          <span
            className="mt-0.5 flex min-w-0 items-center gap-1 font-mono text-2xs text-muted-foreground/70"
            data-testid="coding-session-agents-agent-meta"
          >
            <span className="truncate">
              {formatCodingSessionAgentModel(agent.model)}
            </span>
            <span
              className="shrink-0 tabular-nums"
              title={describeCodingSessionAgentTokens(agent.tokens)}
            >
              · {formatCodingSessionAgentTokens(agent.tokens)}
            </span>
            <span
              className="shrink-0"
              title="Tool calls this computer has seen in the seat's transcript"
            >
              · {formatCodingSessionToolCount(agent.toolCount)}
            </span>
          </span>
        </span>
      </div>
    </div>
  );
}
