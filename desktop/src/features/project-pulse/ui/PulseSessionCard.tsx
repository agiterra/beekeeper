import * as React from "react";
import { ChevronDown, ChevronRight, GitBranch, Terminal } from "lucide-react";

import { codingSessionWireWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";

import {
  branchChipLabel,
  formatCommitConfirmation,
  formatDirtyObservation,
  formatObservedCommit,
  formatProviderReachableLabel,
  formatPulseAge,
  formatPulseExecutionCount,
  formatUnverifiedObservationLabel,
  pulseSessionClosedIsRestated,
  pulseSessionDisplayGeneration,
} from "../lib/pulseFormat";
import type {
  PulseDigestGeneration,
  PulseDigestSession,
} from "../lib/pulseFold.ts";

function generationObservationLabel(
  generation: PulseDigestGeneration,
  nowSeconds: number,
): string {
  const status = codingSessionWireWorkspaceStatus(
    generation.status as CodingSessionStatus | undefined,
  ).label;
  return generation.statusAt === null
    ? `${status} · observation time unknown`
    : `${status} · last observed ${formatPulseAge(nowSeconds - generation.statusAt)} ago`;
}

/** One umbrella session with every observed generation preserved behind it. */
export function PulseSessionCard({
  session,
  nowSeconds,
  onOpen,
  onOpenExecution,
}: {
  session: PulseDigestSession;
  nowSeconds: number;
  onOpen?: () => void;
  onOpenExecution?: (targetKey: string) => void;
}) {
  const [showExecutions, setShowExecutions] = React.useState(false);
  const generation = pulseSessionDisplayGeneration(session);
  const olderGenerations = session.generations
    .filter((candidate) => candidate.targetKey !== generation?.targetKey)
    .sort((left, right) => {
      if (left.statusAt === null) return right.statusAt === null ? 0 : 1;
      if (right.statusAt === null) return -1;
      return (
        right.statusAt - left.statusAt ||
        left.targetKey.localeCompare(right.targetKey)
      );
    });
  const title = session.name ?? session.sessionRef ?? session.sessionKey;

  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-3"
      data-execution-count={session.generations.length}
      data-session-coordination={session.coordinationState}
      data-session-lifecycle={session.lifecycle}
      data-testid="pulse-session-card"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Terminal
          className="size-4 shrink-0 text-muted-foreground"
          aria-hidden
        />
        {onOpen ? (
          <button
            className="truncate text-sm font-medium text-foreground hover:underline"
            data-testid="pulse-session-open"
            onClick={onOpen}
            type="button"
          >
            {title}
          </button>
        ) : (
          <span className="truncate text-sm font-medium text-foreground">
            {title}
          </span>
        )}
        <span
          className="ml-auto inline-flex items-center gap-1 text-2xs text-muted-foreground"
          data-testid="pulse-session-status"
        >
          {session.coordinationState === "provider_reachable"
            ? formatProviderReachableLabel(session)
            : formatUnverifiedObservationLabel(session)}
        </span>
      </div>

      {session.goal ? (
        <p className="mt-1.5 line-clamp-3 text-sm text-muted-foreground">
          {session.goal}
        </p>
      ) : null}

      <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1.5 text-2xs text-muted-foreground">
        <span className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5">
          <GitBranch className="size-3" aria-hidden />
          {branchChipLabel(generation?.branch ?? null)}
        </span>
        <span
          className="rounded bg-muted px-1.5 py-0.5 font-mono"
          data-testid="pulse-session-commit"
        >
          {formatObservedCommit(generation?.observedCommit ?? null)}
        </span>
        <span
          className="text-xs"
          data-testid="pulse-session-commit-confirmation"
        >
          {formatCommitConfirmation(session, nowSeconds)}
        </span>
        <span data-testid="pulse-session-dirty">
          {formatDirtyObservation(generation?.dirty ?? null)}
        </span>
        {session.lifecycle === "closed" &&
        !pulseSessionClosedIsRestated(session) ? (
          <span data-testid="pulse-session-closed">Closed</span>
        ) : null}
      </div>

      {olderGenerations.length > 0 ? (
        <div className="mt-2" data-testid="pulse-session-executions">
          <button
            className="flex items-center gap-1 text-2xs text-muted-foreground hover:text-foreground"
            data-testid="pulse-session-executions-toggle"
            onClick={() => setShowExecutions((open) => !open)}
            type="button"
          >
            {showExecutions ? (
              <ChevronDown className="size-3" aria-hidden />
            ) : (
              <ChevronRight className="size-3" aria-hidden />
            )}
            {formatPulseExecutionCount(session.generations.length)}
          </button>
          {showExecutions ? (
            <ul
              className="mt-1.5 flex flex-col gap-1 border-l border-border/60 pl-2"
              data-testid="pulse-session-execution-list"
            >
              {olderGenerations.map((older) => (
                <li
                  className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-muted-foreground"
                  data-testid="pulse-session-execution"
                  key={older.targetKey}
                >
                  {onOpenExecution ? (
                    <button
                      className="hover:text-foreground hover:underline"
                      data-testid="pulse-session-execution-open"
                      onClick={() => onOpenExecution(older.targetKey)}
                      type="button"
                    >
                      {generationObservationLabel(older, nowSeconds)}
                    </button>
                  ) : (
                    <span>{generationObservationLabel(older, nowSeconds)}</span>
                  )}
                  <span className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5">
                    <GitBranch className="size-3" aria-hidden />
                    {branchChipLabel(older.branch)}
                  </span>
                  <span className="rounded bg-muted px-1.5 py-0.5 font-mono">
                    {formatObservedCommit(older.observedCommit)}
                  </span>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}
