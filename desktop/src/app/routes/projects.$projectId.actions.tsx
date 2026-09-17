import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectActionsScreen = React.lazy(async () => {
  const module = await import(
    "@/features/project-actions/ui/ProjectActionsScreen"
  );
  return { default: module.ProjectActionsScreen };
});

/**
 * The project's Actions tab: the `beekeeper/actions.yml` entries published
 * for this project, each with its latest runs and what the relay's records
 * prove about them (approvals, host claims, exits).
 */
export const Route = createFileRoute("/projects/$projectId/actions")({
  component: ProjectActionsRouteComponent,
});

function ProjectActionsRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectActionsScreen projectId={projectId} />
    </React.Suspense>
  );
}
