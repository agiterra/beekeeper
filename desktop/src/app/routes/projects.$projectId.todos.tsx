import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectTodosScreen = React.lazy(async () => {
  const module = await import("@/features/project-todos/ui/ProjectTodosScreen");
  return { default: module.ProjectTodosScreen };
});

/**
 * The project's To-Do tab: its shared lists, live for every member (NIP-TD,
 * kind 44248).
 */
export const Route = createFileRoute("/projects/$projectId/todos")({
  component: ProjectTodosRouteComponent,
});

function ProjectTodosRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectTodosScreen projectId={projectId} />
    </React.Suspense>
  );
}
