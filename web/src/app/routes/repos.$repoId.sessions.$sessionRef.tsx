import { ArrowLeft } from "lucide-react";
import { Link, createFileRoute, useParams } from "@tanstack/react-router";

import { CodingSessionDetail } from "@/features/coding-sessions/ui/CodingSessionDetail";
import {
  CodingSessionsError,
  CodingSessionsNoChannel,
} from "@/features/coding-sessions/ui/CodingSessionsPanel";
import { selectCodingSessionUmbrella } from "@/features/coding-sessions/ui/observer-contract";
import { useCodingSessionObserver } from "@/features/coding-sessions/ui/useCodingSessionObserver";
import { useRepo } from "@/features/repos/use-repos";

function NotFound({ sessionRef }: { sessionRef: string }) {
  return (
    <div
      className="mt-6 rounded-lg border border-black/10 px-4 py-8 text-center dark:border-white/10"
      data-testid="coding-session-not-found"
    >
      <p className="text-sm font-medium text-black dark:text-white">
        Session not found
      </p>
      <p className="mt-1 break-all text-xs text-black/50 dark:text-white/50">
        Nothing in this channel's readable history names{" "}
        <code className="font-mono">{sessionRef}</code>. It may predate the
        1000-event history window, or belong to another channel.
      </p>
    </div>
  );
}

/** One coding session, observed read-only in the browser. */
function RepoSessionPage() {
  const { repoId, sessionRef } = useParams({
    from: "/repos/$repoId/sessions/$sessionRef",
  });
  const { data: repo, isLoading } = useRepo(repoId);
  const view = useCodingSessionObserver(repo?.channelId ?? null);
  const umbrella = selectCodingSessionUmbrella(view.sessions, sessionRef);

  return (
    <div className="flex w-full flex-1 flex-col bg-[#F3F3F3] px-4 py-8 text-black dark:bg-[#171717] dark:text-white">
      <Link
        to="/repos/$repoId/sessions"
        params={{ repoId }}
        className="inline-flex items-center gap-1 self-start text-sm text-black/60 hover:text-black dark:text-white/60 dark:hover:text-white"
      >
        <ArrowLeft className="h-4 w-4" />
        All coding sessions
      </Link>
      <div className="mt-6">
        {view.channelId === null && !isLoading ? (
          <CodingSessionsNoChannel />
        ) : view.lastError !== null ? (
          <CodingSessionsError
            message={view.lastError}
            onRetry={view.refresh}
          />
        ) : isLoading || !view.historyLoaded ? (
          <div className="h-40 animate-pulse rounded-lg bg-black/10 dark:bg-white/10" />
        ) : umbrella === null ? (
          <NotFound sessionRef={sessionRef} />
        ) : (
          <CodingSessionDetail umbrella={umbrella} view={view} />
        )}
      </div>
    </div>
  );
}

export const Route = createFileRoute("/repos/$repoId/sessions/$sessionRef")({
  component: RepoSessionPage,
});
