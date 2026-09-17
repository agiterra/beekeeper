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
  // `list` selects a list by id and `view=list` shows only that list (a
  // sidebar row or a fresh create lands here); anything that is not a
  // 32-hex id is dropped, and `view` without a list means nothing.
  validateSearch: (
    search: Record<string, unknown>,
  ): { list?: string; view?: "list" } => {
    const list =
      typeof search.list === "string" && /^[0-9a-f]{32}$/.test(search.list)
        ? search.list
        : undefined;
    if (!list) return {};
    return search.view === "list" ? { list, view: "list" } : { list };
  },
});

function ProjectTodosRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const { list, view } = Route.useSearch();
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
      <ProjectTodosScreen
        focused={view === "list"}
        projectId={projectId}
        selectedListId={list ?? null}
      />
    </React.Suspense>
  );
}
