import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectContributorsScreen = React.lazy(async () => {
  const module = await import(
    "@/features/contributors/ui/ProjectContributorsScreen"
  );
  return { default: module.ProjectContributorsScreen };
});

/**
 * The project's Contributors tab (§C) — seat history for this project alone:
 * every agent that holds or held a seat here, and when it was last observed.
 * Not a list of who is available; no consent state (Eligible, Offered)
 * renders anywhere on this route.
 */
export const Route = createFileRoute("/projects/$projectId/contributors")({
  component: ProjectContributorsRouteComponent,
});

function ProjectContributorsRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectContributorsScreen projectId={projectId} />
    </React.Suspense>
  );
}
