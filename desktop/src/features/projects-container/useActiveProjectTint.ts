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
 * back-reference names it — which also covers a coding session, since
 * `deriveShellRoute` keeps the session's channel selected. A terminal has no
 * channel at all, so `/shell/<id>` is resolved from the shell session's own
 * `projectRef` address, passed in by the caller that has the session list.
 * Anything else — DMs, settings, channels without a project — is no project,
 * and no tint.
 *
 * Pure and exported for unit tests; the hook below feeds it live state.
 */
export function resolveActiveProjectId(
  pathname: string,
  channelProjectRef: string | null | undefined,
  projects: readonly ProjectContainer[],
  shellProjectRef?: string | null,
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
  if (pathname.startsWith("/shell/")) {
    if (!shellProjectRef) return null;
    const project = projects.find(
      (candidate) => candidate.address === shellProjectRef,
    );
    return project ? project.id : null;
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
 * The project the current surface belongs to, or null when it belongs to none.
 *
 * The same resolution the tint uses, exposed whole because callers need more
 * than a color off it — the installer needs the project's address to find the
 * checkout directory this computer remembers for it (ledger 85).
 */
export function useActiveProjectContainer(
  pathname: string,
  channelProjectRef: string | null | undefined,
  shellProjectRef?: string | null,
): ProjectContainer | null {
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
      shellProjectRef,
    );
    if (!projectId) return null;
    return projects.find((candidate) => candidate.id === projectId) ?? null;
  }, [channelProjectRef, pathname, projects, projectsEnabled, shellProjectRef]);
}

/**
 * The active project's tint color (`#rrggbb`), or null when the current
 * surface belongs to no project or that project has no color set.
 */
export function useActiveProjectTint(
  pathname: string,
  channelProjectRef: string | null | undefined,
): string | null {
  return useActiveProjectContainer(pathname, channelProjectRef)?.color ?? null;
}
