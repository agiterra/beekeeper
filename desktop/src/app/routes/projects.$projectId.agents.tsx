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
 * The project's Agents tab: the agents associated with this project, with
 * their primary roles and states, and the borrowed or past participants
 * seated in its sessions.
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
