import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning, useFeatureEnabled } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectAgentsRepoScreen = React.lazy(async () => {
  const module = await import(
    "@/features/agents-repo/ui/ProjectAgentsRepoScreen"
  );
  return { default: module.ProjectAgentsRepoScreen };
});

const MemoryExplorer = React.lazy(async () => {
  const module = await import("@/features/memory-explorer/Explorer");
  return { default: module.MemoryExplorer };
});

/**
 * The project's Files tab: its agents repository — plans, roles, skills,
 * team and actions — read from `main`, edited as shared drafts (NIP-AD,
 * kind 44250) and committed from here.
 */
export const Route = createFileRoute("/projects/$projectId/files")({
  component: ProjectFilesRouteComponent,
  // `path` opens one file; anything with a `..` or an absolute form is
  // dropped. `view=file` shows that one file alone — its content and its
  // history, with no tabs and no tree — the way a pinned sidebar row opens
  // it. `view` without a `path` means nothing.
  validateSearch: (
    search: Record<string, unknown>,
  ): { path?: string; view?: "file" | "explore" } => {
    if (search.view === "explore") return { view: "explore" };
    const path = typeof search.path === "string" ? search.path : undefined;
    if (!path || path.startsWith("/") || path.split("/").includes(".."))
      return {};
    return search.view === "file" ? { path, view: "file" } : { path };
  },
});

function ProjectFilesRouteComponent() {
  usePreviewFeatureWarning("projects");
  const enabled = useFeatureEnabled("memory-explorer");
  const { projectId } = Route.useParams();
  const { path, view } = Route.useSearch();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      {enabled && view === "explore" ? (
        <MemoryExplorer projectId={projectId} />
      ) : (
        <ProjectAgentsRepoScreen
          focused={view === "file"}
          projectId={projectId}
          selectedPath={path ?? null}
        />
      )}
    </React.Suspense>
  );
}
