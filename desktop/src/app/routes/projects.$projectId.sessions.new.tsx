import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectNewCodingSessionScreen = React.lazy(async () => {
  const module = await import(
    "@/features/projects-container/ui/ProjectNewCodingSessionScreen"
  );
  return { default: module.ProjectNewCodingSessionScreen };
});

export const Route = createFileRoute("/projects/$projectId/sessions/new")({
  component: ProjectNewCodingSessionRouteComponent,
});

function ProjectNewCodingSessionRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectNewCodingSessionScreen projectId={projectId} />
    </React.Suspense>
  );
}
