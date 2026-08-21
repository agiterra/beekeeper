import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { FeatureGate, usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectPulseScreen = React.lazy(async () => {
  const module = await import("@/features/project-pulse/ui/ProjectPulseScreen");
  return { default: module.ProjectPulseScreen };
});

export const Route = createFileRoute("/projects/$projectId/pulse")({
  component: ProjectPulseRouteComponent,
});

function ProjectPulseRouteComponent() {
  // The toast is the deep-link hint ("this is a preview feature"); it gates
  // nothing on its own. `FeatureGate` is what actually keeps the screen — and
  // its 44240 / session-fact relay queries — from mounting when the
  // `project-pulse` preview flag is off, matching the sidebar row and the
  // project-home card.
  usePreviewFeatureWarning("project-pulse");
  const { projectId } = Route.useParams();

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <FeatureGate feature="project-pulse">
        <ProjectPulseScreen projectId={projectId} />
      </FeatureGate>
    </React.Suspense>
  );
}
