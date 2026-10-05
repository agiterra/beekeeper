import * as React from "react";
import { ChevronDown, ChevronRight, GitBranch, Terminal } from "lucide-react";

import { SessionNameOriginMarker } from "@/features/coding-sessions/ui/SessionNameOriginMarker";
import { coordinationReportedStatus } from "@/shared/coordination/sessionCoordinationFormat";
import type { CoordinatedSessionNameOrigin } from "@/shared/coordination/sessionCoordinationNames";

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
import type { PulseMissionRow as PulseMissionRowModel } from "../lib/pulseMissionWire";
import { PulseMissionRow } from "./PulseMissionRow";

function generationObservationLabel(
  generation: PulseDigestGeneration,
  nowSeconds: number,
): string {
  const status = coordinationReportedStatus(generation.status).label;
  return generation.statusAt === null
    ? `${status} · observation time unknown`
    : `${status} · last observed ${formatPulseAge(nowSeconds - generation.statusAt)} ago`;
}

/** The card's title when nothing names the session. */
export const UNNAMED_SESSION_LABEL = "Unnamed session";

/** One umbrella session with every observed generation preserved behind it. */
export function PulseSessionCard({
  session,
  nowSeconds,
  onOpen,
  onOpenExecution,
  /**
   * The mission fold's row for this same session, when one was read.
   *
   * Rendered *beside* the card's own liveness rather than instead of it: the
   * digest and the mission fold are two reads of the same work, and collapsing
   * them would hide a disagreement between them rather than show it.
   */
  missionRow,
  /**
   * Where `session.name` came from, read beside the digest
   * (`lib/pulseNameOrigins.ts`). A generated title gets the "Auto-named"
   * marker; absent or a person's name shows none.
   */
  nameOrigin = null,
  /**
   * The name the caller already knows this session by when the digest
   * carries none (the session view's own title, for its lead card). The
   * digest's `name` resolves only 44229/44252; a session titled at creation
   * and never renamed has none there.
   */
  fallbackTitle = null,
}: {
  session: PulseDigestSession;
  nowSeconds: number;
  onOpen?: () => void;
  onOpenExecution?: (targetKey: string) => void;
  missionRow?: PulseMissionRowModel | null;
  nameOrigin?: CoordinatedSessionNameOrigin | null;
  fallbackTitle?: string | null;
}) {
  const [showExecutions, setShowExecutions] = React.useState(false);
  const generation = pulseSessionDisplayGeneration(session);
  const executionCount = new Set(
    session.generations.map((candidate) => candidate.executionKey),
  ).size;
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
  const fallback = fallbackTitle?.trim() ? fallbackTitle : null;
  // Never a raw id as if it were a name: with no name the card says so and
  // discloses the reference beside it.
  const title = session.name ?? fallback ?? UNNAMED_SESSION_LABEL;
  const unnamed = session.name === null && fallback === null;

  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-3"
      data-execution-count={executionCount}
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
        {unnamed ? (
          <span
            className="truncate font-mono text-2xs text-muted-foreground"
            data-testid="pulse-session-ref"
          >
            {session.sessionRef ?? session.sessionKey}
          </span>
        ) : null}
        {session.name ? (
          <SessionNameOriginMarker
            origin={nameOrigin}
            testId="pulse-session-title-origin"
          />
        ) : null}
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

      {missionRow ? (
        <ul className="mt-2" data-testid="pulse-session-mission">
          <PulseMissionRow mission={missionRow} />
        </ul>
      ) : null}

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
            {formatPulseExecutionCount(executionCount)} ·{" "}
            {session.generations.length} generations
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
