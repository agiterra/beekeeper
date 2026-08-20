import { Circle, GitBranch, Terminal } from "lucide-react";

import { cn } from "@/shared/lib/cn";

import {
  branchChipLabel,
  formatCommitConfirmation,
  formatDirtyObservation,
  formatLastSeenLabel,
  formatObservedAge,
  formatObservedCommit,
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
 */
export function PulseSessionCard({
  session,
  nowSeconds,
  onOpen,
}: {
  session: PulseDigestSession;
  nowSeconds: number;
  onOpen?: () => void;
}) {
  const active = session.activity === "active";
  const title = session.name ?? session.sessionRef ?? session.targetKey;
  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-3"
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

      <div className="mt-2 flex flex-wrap items-center gap-1.5 text-2xs text-muted-foreground">
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
        <span data-testid="pulse-session-dirty">
          {formatDirtyObservation(session.dirty)}
        </span>
        <span data-testid="pulse-session-observed-age">
          {formatObservedAge(session)}
        </span>
        {session.closed ? (
          <span data-testid="pulse-session-closed">Closed</span>
        ) : null}
      </div>

      {/* text-xs, not the 2xs of the chips above: this line is the difference
          between "the relay has this commit" and "nobody checked", and the
          qualifiers that keep the screen honest should not be the smallest
          type on it. */}
      <p
        className="mt-1 text-xs text-muted-foreground"
        data-testid="pulse-session-commit-confirmation"
      >
        {formatCommitConfirmation(session, nowSeconds)}
      </p>
    </li>
  );
}
