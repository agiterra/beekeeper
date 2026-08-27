import { ArrowLeft } from "lucide-react";
import { Link, createFileRoute, useParams } from "@tanstack/react-router";

import { CodingSessionsPanel } from "@/features/coding-sessions/ui/CodingSessionsPanel";
import { useCodingSessionObserver } from "@/features/coding-sessions/ui/useCodingSessionObserver";
import { useRepo } from "@/features/repos/use-repos";

/**
 * The standalone session list for one repository.
 *
 * The repo lookup lives here rather than in the feature: routes are where
 * repositories and coding sessions get composed, and the observer itself only
 * ever knows about a channel.
 */
function RepoSessionsPage() {
  const { repoId } = useParams({ from: "/repos/$repoId/sessions" });
  const { data: repo, isLoading } = useRepo(repoId);
  const view = useCodingSessionObserver(repo?.channelId ?? null);

  return (
    <div className="flex w-full flex-1 flex-col bg-[#F3F3F3] px-4 py-8 text-black dark:bg-[#171717] dark:text-white">
      <Link
        to="/repos/$repoId"
        params={{ repoId }}
        className="inline-flex items-center gap-1 self-start text-sm text-black/60 hover:text-black dark:text-white/60 dark:hover:text-white"
      >
        <ArrowLeft className="h-4 w-4" />
        Back to {repo?.name ?? repoId}
      </Link>
      <h1 className="mt-6 text-2xl font-semibold tracking-tight">
        Coding sessions
      </h1>
      <p className="mt-1 text-sm text-black/60 dark:text-white/60">
        Sessions published to this repository's channel, observed read-only.
      </p>
      {isLoading ? (
        <div className="mt-8 h-24 animate-pulse rounded-lg bg-black/10 dark:bg-white/10" />
      ) : (
        <CodingSessionsPanel repoId={repoId} view={view} />
      )}
    </div>
  );
}

export const Route = createFileRoute("/repos/$repoId/sessions")({
  component: RepoSessionsPage,
});
