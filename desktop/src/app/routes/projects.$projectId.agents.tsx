import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectAgentsScreen = React.lazy(async () => {
  const module = await import(
    "@/features/project-agents/ui/ProjectAgentsScreen"
  );
  return { default: module.ProjectAgentsScreen };
});

/**
 * The project's Agents tab: who is working in this project and why —
 * installed for it, seated in its sessions, or given one of its assignments.
 */
export const Route = createFileRoute("/projects/$projectId/agents")({
  component: ProjectAgentsRouteComponent,
});

function ProjectAgentsRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectAgentsScreen projectId={projectId} />
    </React.Suspense>
  );
}
