import * as React from "react";
import { createFileRoute, useLocation } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { isEntityLinkTab } from "@/shared/lib/entityLink";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectDetailScreen = React.lazy(async () => {
  const module = await import("@/features/projects/ui/ProjectDetailScreen");
  return { default: module.ProjectDetailScreen };
});

export const Route = createFileRoute("/projects/$projectId/code/$repoId")({
  component: ProjectRepoRouteComponent,
  validateSearch: (search: Record<string, unknown>) => ({
    commitHash:
      typeof search.commitHash === "string" ? search.commitHash : undefined,
    pullRequestId:
      typeof search.pullRequestId === "string"
        ? search.pullRequestId
        : undefined,
    issueId: typeof search.issueId === "string" ? search.issueId : undefined,
    // Active repository within a multi-repo project. Without this the
    // repository picker (and add-repo activation) cannot switch repos —
    // ProjectDetailScreen patches this key into the URL and reads it back.
    repositoryId:
      typeof search.repositoryId === "string" ? search.repositoryId : undefined,
    tab: isEntityLinkTab(search.tab) ? search.tab : undefined,
  }),
});

function ProjectRepoRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId, repoId } = Route.useParams();
  const { commitHash, pullRequestId, issueId, repositoryId, tab } =
    Route.useSearch();
  const entityNavigationId = useLocation({
    select: (location) => {
      const value = (
        location.state as { entityNavigationId?: unknown } | undefined
      )?.entityNavigationId;
      return typeof value === "string" ? value : undefined;
    },
  });

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectDetailScreen
        commitHash={commitHash}
        containerId={projectId}
        entityNavigationId={entityNavigationId}
        issueId={issueId}
        projectId={repoId}
        pullRequestId={pullRequestId}
        repositoryId={repositoryId}
        tab={tab}
      />
    </React.Suspense>
  );
}
