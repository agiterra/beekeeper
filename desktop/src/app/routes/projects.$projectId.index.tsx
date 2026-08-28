import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import {
  type ProjectPageTab,
  parseProjectPageTab,
} from "@/features/projects-container/ui/ProjectPageTabs";
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
  // `tab` picks the project page section; anything unrecognised is the
  // overview, and the overview itself carries no param.
  validateSearch: (
    search: Record<string, unknown>,
  ): { tab?: Exclude<ProjectPageTab, "overview"> } =>
    parseProjectPageTab(search.tab) === "pulse" ? { tab: "pulse" } : {},
});

function ProjectContainerRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const { tab } = Route.useSearch();

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectContainerScreen projectId={projectId} tab={tab ?? "overview"} />
    </React.Suspense>
  );
}
