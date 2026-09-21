import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectAgentsRepoScreen = React.lazy(async () => {
  const module = await import(
    "@/features/agents-repo/ui/ProjectAgentsRepoScreen"
  );
  return { default: module.ProjectAgentsRepoScreen };
});

/**
 * The project's Files tab: its agents repository — plans, roles, skills,
 * team and actions — read from `main`, edited as shared drafts (NIP-AD,
 * kind 44249) and committed from here.
 */
export const Route = createFileRoute("/projects/$projectId/files")({
  component: ProjectFilesRouteComponent,
  // `path` opens one file; anything with a `..` or an absolute form is dropped.
  validateSearch: (search: Record<string, unknown>): { path?: string } => {
    const path = typeof search.path === "string" ? search.path : undefined;
    if (!path || path.startsWith("/") || path.split("/").includes(".."))
      return {};
    return { path };
  },
});

function ProjectFilesRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const { path } = Route.useSearch();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectAgentsRepoScreen
        projectId={projectId}
        selectedPath={path ?? null}
      />
    </React.Suspense>
  );
}
