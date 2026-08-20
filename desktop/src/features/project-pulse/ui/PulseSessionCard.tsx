import * as React from "react";
import {
  ChevronDown,
  ChevronRight,
  Circle,
  GitBranch,
  Terminal,
} from "lucide-react";

import { cn } from "@/shared/lib/cn";

import {
  branchChipLabel,
  formatCommitConfirmation,
  formatDirtyObservation,
  formatLastSeenLabel,
  formatObservedAge,
  formatObservedCommit,
  formatPulseExecutionCount,
  pulseSessionClosedIsRestated,
  pulseSessionStatusLabel,
} from "../lib/pulseFormat";
import type { PulseDigestSession } from "../lib/pulseFold.ts";

/**
 * One coding session as the relay's signed facts describe it.
 *
 * Every observation is tri-state and rendered as such: a null `dirty` says
 * "not observed", never "clean", and the commit-confirmation line is the
 * fold's fixed string plus the `verifiedAt` age — never the words "relay
 * reachable", which would imply something about the session's connection that
 * the fact does not support.
 *
 * The status label comes from the shared wire→label mapping the coding-session
 * header uses, so one session cannot read `Working` on its shelf row and
 * something else here.
 *
 * One card is one *umbrella* session. A provider that restarts a session emits
 * a new generation with its own `targetKey`, and the fold keeps every one of
 * them; rendering each as a full card turned three restarts of one session into
 * three near-identical rows. The older executions live behind the disclosure at
 * the foot of this card — collapsed, never dropped.
 */
export function PulseSessionCard({
  session,
  olderExecutions = [],
  nowSeconds,
  onOpen,
  onOpenExecution,
}: {
  session: PulseDigestSession;
  /** Earlier executions of the same umbrella, newest first. */
  olderExecutions?: readonly PulseDigestSession[];
  nowSeconds: number;
  onOpen?: () => void;
  onOpenExecution?: (targetKey: string) => void;
}) {
  const [showExecutions, setShowExecutions] = React.useState(false);
  const active = session.activity === "active";
  const title = session.name ?? session.sessionRef ?? session.targetKey;
  const executionCount = olderExecutions.length + 1;
  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-3"
      data-execution-count={executionCount}
      data-session-activity={session.activity}
      data-session-closed={session.closed ? "true" : "false"}
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
          className={cn(
            "ml-auto inline-flex items-center gap-1 text-2xs",
            active ? "text-emerald-500" : "text-muted-foreground",
          )}
          data-testid="pulse-session-status"
        >
          {active ? (
            <Circle className="size-1.5 fill-current" aria-hidden />
          ) : null}
          {active
            ? pulseSessionStatusLabel(session)
            : formatLastSeenLabel(session)}
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
          {branchChipLabel(session.branch)}
        </span>
        <span
          className="rounded bg-muted px-1.5 py-0.5 font-mono"
          data-testid="pulse-session-commit"
        >
          {formatObservedCommit(session.observedCommit)}
        </span>
        {/* text-xs, not the 2xs of the chips beside it: this is the difference
            between "the relay has this commit" and "nobody checked", and the
            qualifier that keeps the screen honest should not be the smallest
            type on it. It sits *in* this row rather than on a line of its own
            because `Commit not checked` is the common case, and a whole line
            per card for it turned nineteen honest tri-states into nineteen
            lines of noise. */}
        <span
          className="text-xs"
          data-testid="pulse-session-commit-confirmation"
        >
          {formatCommitConfirmation(session, nowSeconds)}
        </span>
        <span data-testid="pulse-session-dirty">
          {formatDirtyObservation(session.dirty)}
        </span>
        {/* A stale session's status label already reads `… · last observed 3h
            ago`; repeating the age here said one fact twice. */}
        {active ? (
          <span data-testid="pulse-session-observed-age">
            {formatObservedAge(session)}
          </span>
        ) : null}
        {session.closed && !pulseSessionClosedIsRestated(session) ? (
          <span data-testid="pulse-session-closed">Closed</span>
        ) : null}
      </div>

      {olderExecutions.length > 0 ? (
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
            {formatPulseExecutionCount(executionCount)}
          </button>
          {showExecutions ? (
            <ul
              className="mt-1.5 flex flex-col gap-1 border-l border-border/60 pl-2"
              data-testid="pulse-session-execution-list"
            >
              {olderExecutions.map((execution) => (
                <li
                  className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-muted-foreground"
                  data-testid="pulse-session-execution"
                  key={execution.targetKey}
                >
                  {onOpenExecution ? (
                    <button
                      className="hover:text-foreground hover:underline"
                      data-testid="pulse-session-execution-open"
                      onClick={() => onOpenExecution(execution.targetKey)}
                      type="button"
                    >
                      {formatLastSeenLabel(execution)}
                    </button>
                  ) : (
                    <span>{formatLastSeenLabel(execution)}</span>
                  )}
                  <span className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5">
                    <GitBranch className="size-3" aria-hidden />
                    {branchChipLabel(execution.branch)}
                  </span>
                  <span className="rounded bg-muted px-1.5 py-0.5 font-mono">
                    {formatObservedCommit(execution.observedCommit)}
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
