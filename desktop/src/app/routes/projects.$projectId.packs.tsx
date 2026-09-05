import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const ProjectPacksScreen = React.lazy(async () => {
  const module = await import("@/features/roles/ui/ProjectPacksScreen");
  return { default: module.ProjectPacksScreen };
});

/**
 * The project's Packs tab (§B) — what each role has, who carries it, and who
 * is seated in it, for this project alone. Replaces the Dashboard's Roles
 * tab, which resolved a project nobody could see; this route names one.
 */
export const Route = createFileRoute("/projects/$projectId/packs")({
  component: ProjectPacksRouteComponent,
});

function ProjectPacksRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectPacksScreen projectId={projectId} />
    </React.Suspense>
  );
}
