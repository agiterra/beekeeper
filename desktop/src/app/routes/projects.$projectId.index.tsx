import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectContainerScreen = React.lazy(async () => {
  const module = await import(
    "@/features/projects-container/ui/ProjectContainerScreen"
  );
  return { default: module.ProjectContainerScreen };
});

export const Route = createFileRoute("/projects/$projectId/")({
  component: ProjectContainerRouteComponent,
});

function ProjectContainerRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectContainerScreen projectId={projectId} />
    </React.Suspense>
  );
}
