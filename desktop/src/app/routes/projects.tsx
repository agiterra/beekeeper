import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectsScreen = React.lazy(async () => {
  const module = await import("@/features/projects/ui/ProjectsScreen");
  return { default: module.ProjectsScreen };
});

export const Route = createFileRoute("/projects")({
  component: ProjectsRouteComponent,
  validateSearch: (search: Record<string, unknown>) => ({
    // Only the management tab is deep-linked today (the sidebar heading);
    // any other value falls back to the stored tab.
    filter: search.filter === "projects" ? ("projects" as const) : undefined,
  }),
});

function ProjectsRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { filter } = Route.useSearch();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectsScreen initialFilter={filter} />
    </React.Suspense>
  );
}
