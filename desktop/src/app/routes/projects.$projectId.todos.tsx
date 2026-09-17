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
  // `list` selects a list by id (a sidebar row or a fresh create lands
  // here); anything that is not a 32-hex id is dropped.
  validateSearch: (search: Record<string, unknown>): { list?: string } =>
    typeof search.list === "string" && /^[0-9a-f]{32}$/.test(search.list)
      ? { list: search.list }
      : {},
});

function ProjectTodosRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const { list } = Route.useSearch();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectTodosScreen projectId={projectId} selectedListId={list ?? null} />
    </React.Suspense>
  );
}
