import * as React from "react";

import { useFeatureEnabled } from "@/shared/features";
import { KIND_PROJECT } from "@/shared/constants/kinds";

import { useProjectContainersQuery, type ProjectContainer } from "./hooks";
import { parseMemberRef } from "./lib/projectContainerModel";

/**
 * Which project the content pane is currently showing.
 *
 * Resolution order: a `/projects/<id>` route names the project directly
 * (the id segment matches `ProjectContainer.id` or its dtag, like
 * `useProjectContainerQuery`); otherwise the active channel's `projectRef`
 * back-reference names it. Anything else — DMs, settings, channels without
 * a project — is no project, and no tint.
 *
 * Pure and exported for unit tests; the hook below feeds it live state.
 */
export function resolveActiveProjectId(
  pathname: string,
  channelProjectRef: string | null | undefined,
  projects: readonly ProjectContainer[],
): string | null {
  if (pathname.startsWith("/projects/")) {
    const segment = pathname.split("/")[2] ?? "";
    const id = decodeURIComponent(segment);
    if (id.length > 0) {
      const project = projects.find(
        (candidate) => candidate.id === id || candidate.dtag === id,
      );
      if (project) return project.id;
    }
    return null;
  }
  if (channelProjectRef) {
    const ref = parseMemberRef(channelProjectRef);
    if (ref && ref.kind === KIND_PROJECT) {
      const id = `${ref.owner}:${ref.dtag}`;
      const project = projects.find((candidate) => candidate.id === id);
      if (project) return project.id;
    }
  }
  return null;
}

/**
 * The active project's tint color (`#rrggbb`), or null when the current
 * surface belongs to no project or that project has no color set.
 */
export function useActiveProjectTint(
  pathname: string,
  channelProjectRef: string | null | undefined,
): string | null {
  const projectsEnabled = useFeatureEnabled("projects");
  const projectsQuery = useProjectContainersQuery({
    enabled: projectsEnabled,
  });
  const projects = projectsQuery.data;
  return React.useMemo(() => {
    if (!projectsEnabled || !projects) return null;
    const projectId = resolveActiveProjectId(
      pathname,
      channelProjectRef,
      projects,
    );
    if (!projectId) return null;
    return (
      projects.find((candidate) => candidate.id === projectId)?.color ?? null
    );
  }, [channelProjectRef, pathname, projects, projectsEnabled]);
}
