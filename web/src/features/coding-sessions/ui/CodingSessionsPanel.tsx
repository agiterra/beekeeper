import { Link } from "@tanstack/react-router";
import { Bot, MessageSquareOff, RefreshCw, Terminal } from "lucide-react";

import { relativeTime } from "@/shared/lib/relative-time";
import { Button } from "@/shared/ui/button";
import {
  type CodingSessionUmbrella,
  codingSessionFounderLabel,
  generationExecutionLabel,
} from "../domain/index.ts";
import {
  CodingSessionAuthorityBadge,
  CodingSessionTrustNotes,
} from "./CodingSessionNotices.tsx";
import {
  CodingSessionClosedBadge,
  CodingSessionStatusChip,
} from "./CodingSessionStatusChip.tsx";
import {
  type CodingSessionObserverView,
  codingSessionRouteRef,
} from "./observer-contract.ts";
import { CodingSessionConnectionLine } from "./CodingSessionConnectionLine.tsx";

function SessionRow({
  repoId,
  umbrella,
}: {
  repoId: string;
  umbrella: CodingSessionUmbrella;
}) {
  const executionLabels = umbrella.executions.map((execution) =>
    generationExecutionLabel(execution.activeGeneration),
  );
  const hasUnverifiedAuthority = umbrella.executions.some(
    (execution) => execution.authoritySource === "disclosed-fallback",
  );
  return (
    <Link
      to="/repos/$repoId/sessions/$sessionRef"
      params={{ repoId, sessionRef: codingSessionRouteRef(umbrella) }}
      className="block border-b border-black/10 px-3 py-3 last:border-b-0 hover:bg-black/5 dark:border-white/10 dark:hover:bg-white/5"
      data-testid="coding-session-row"
    >
      <div className="flex items-start gap-3">
        <Terminal className="mt-0.5 h-4 w-4 shrink-0 text-black/50 dark:text-white/50" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="truncate text-sm font-medium text-black dark:text-white">
              {umbrella.name}
            </span>
            <CodingSessionStatusChip status={umbrella.status} />
            {umbrella.closed && <CodingSessionClosedBadge />}
            {hasUnverifiedAuthority && <CodingSessionAuthorityBadge />}
          </div>
          <p className="mt-1 text-xs text-black/50 dark:text-white/50">
            Founder {codingSessionFounderLabel(umbrella)}
          </p>
          <p className="mt-0.5 flex flex-wrap items-center gap-1.5 text-xs text-black/50 dark:text-white/50">
            <Bot className="h-3.5 w-3.5 shrink-0" />
            {executionLabels.length === 0
              ? "No executions"
              : executionLabels.join(" · ")}
          </p>
          {umbrella.foreignAttachmentCount > 0 && (
            <p className="mt-0.5 text-xs text-amber-700 dark:text-amber-300">
              {umbrella.foreignAttachmentCount} execution
              {umbrella.foreignAttachmentCount === 1 ? "" : "s"} attached by
              someone other than the founder
            </p>
          )}
        </div>
        <span className="shrink-0 self-center text-xs text-black/50 dark:text-white/50">
          {relativeTime(Math.floor(umbrella.lastEventAt / 1000))}
        </span>
      </div>
    </Link>
  );
}

function ListSkeleton() {
  return (
    <div className="overflow-hidden rounded-lg border border-black/10 dark:border-white/10">
      {["sk-1", "sk-2", "sk-3"].map((key) => (
        <div
          key={key}
          className="flex items-center gap-3 border-b border-black/10 px-3 py-3 last:border-b-0 dark:border-white/10"
        >
          <div className="h-4 w-4 animate-pulse rounded bg-black/10 dark:bg-white/10" />
          <div className="flex-1 space-y-1.5">
            <div className="h-4 w-56 animate-pulse rounded bg-black/10 dark:bg-white/10" />
            <div className="h-3 w-40 animate-pulse rounded bg-black/10 dark:bg-white/10" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** The repo has no `buzz-channel` tag, so there is nowhere to read from. */
export function CodingSessionsNoChannel() {
  return (
    <div
      className="mt-6 rounded-lg border border-black/10 px-4 py-8 text-center dark:border-white/10"
      data-testid="coding-sessions-no-channel"
    >
      <MessageSquareOff className="mx-auto h-8 w-8 text-black/40 dark:text-white/40" />
      <p className="mt-3 text-sm font-medium text-black dark:text-white">
        No session channel
      </p>
      <p className="mt-1 text-xs text-black/50 dark:text-white/50">
        This repository is not linked to a Bee Keeper channel, so there is no
        place for coding sessions to be published or observed.
      </p>
    </div>
  );
}

/** The observer's error state — always paired with a way to try again. */
export function CodingSessionsError({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  return (
    <div
      className="mt-6 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm text-destructive"
      data-testid="coding-sessions-error"
    >
      <p>Could not read coding sessions: {message}</p>
      <Button
        variant="outline"
        size="sm"
        className="mt-3 border-destructive/40 text-destructive"
        onClick={onRetry}
      >
        <RefreshCw className="h-3.5 w-3.5" />
        Retry
      </Button>
    </div>
  );
}

/**
 * The session list, shared by the `Sessions` tab and the standalone route.
 *
 * `channelId === null` is a different fact from "no sessions" and renders
 * differently: an empty list over a repo with no channel would imply the
 * relay was asked and answered nothing.
 */
export function CodingSessionsPanel({
  repoId,
  view,
}: {
  repoId: string;
  view: CodingSessionObserverView;
}) {
  if (view.channelId === null) {
    return <CodingSessionsNoChannel />;
  }

  if (view.lastError !== null) {
    return (
      <div className="mt-2">
        <CodingSessionsError message={view.lastError} onRetry={view.refresh} />
        <CodingSessionTrustNotes view={view} />
      </div>
    );
  }

  return (
    <div className="mt-6" data-testid="coding-sessions-panel">
      <div className="mb-3 flex items-center justify-between gap-3">
        <CodingSessionConnectionLine view={view} />
        <Button
          variant="outline"
          size="sm"
          className="border-black/10 bg-white text-black hover:bg-black/5 dark:border-white/10 dark:bg-white/5 dark:text-white dark:hover:bg-white/10"
          onClick={view.refresh}
        >
          <RefreshCw className="h-3.5 w-3.5" />
          Refresh
        </Button>
      </div>
      {!view.historyLoaded ? (
        <ListSkeleton />
      ) : view.sessions.length === 0 ? (
        <div
          className="rounded-lg border border-black/10 px-4 py-8 text-center dark:border-white/10"
          data-testid="coding-sessions-empty"
        >
          <Terminal className="mx-auto h-8 w-8 text-black/40 dark:text-white/40" />
          <p className="mt-3 text-sm font-medium text-black dark:text-white">
            No coding sessions in this channel
          </p>
          <p className="mt-1 text-xs text-black/50 dark:text-white/50">
            Sessions started from a Bee Keeper client appear here in real time.
          </p>
        </div>
      ) : (
        <div className="overflow-hidden rounded-lg border border-black/10 bg-white/50 dark:border-white/10 dark:bg-white/5">
          {view.sessions.map((umbrella) => (
            <SessionRow
              key={umbrella.umbrellaKey}
              repoId={repoId}
              umbrella={umbrella}
            />
          ))}
        </div>
      )}
      <CodingSessionTrustNotes view={view} />
    </div>
  );
}
