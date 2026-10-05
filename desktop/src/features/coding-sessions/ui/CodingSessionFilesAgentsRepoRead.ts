import {
  useAgentsRepoListing,
  useAgentsRepoSource,
} from "@/features/agents-repo/lib/agentsRepoQueries";
import { useProjectContainerQuery } from "@/features/projects-container/hooks";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";

import type { CodingSessionSurfaceBaseCtx } from "./surfaces/codingSessionSurfaceContext";

/**
 * The Files surface's agents-repository read, shared by its `readExtension`
 * (so the launcher row can dim when there is nothing to show) and its panel
 * (which lists it). Both go through the same React Query keys the project's
 * Files screen uses, so the view reads each once. No module state, so
 * nothing for `resetCommunityState()` to reset.
 */
export function useCodingSessionAgentsRepoRead(
  ctx: Pick<CodingSessionSurfaceBaseCtx, "project" | "projectRef">,
  listEntries: boolean,
) {
  // The sidebar's resolution when it has one, else the coordinate's own `d`
  // (`30621:<owner>:<d>`), which `useProjectContainerQuery` also matches.
  const projectId =
    ctx.project?.id ??
    (ctx.projectRef === null
      ? ""
      : ctx.projectRef.split(":").slice(2).join(":") || ctx.projectRef);
  const { project } = useProjectContainerQuery(projectId);
  const coordinate =
    project && project.id !== LOCAL_GENERAL_ID && project.owner.length > 0
      ? project.address
      : null;
  const sourceQuery = useAgentsRepoSource(coordinate);
  const source = sourceQuery.data ?? null;
  const isAgentsRepo =
    source !== null && source.path === "." && source.ref !== null;
  const listing = useAgentsRepoListing(coordinate, listEntries && isAgentsRepo);
  return { project, coordinate, sourceQuery, isAgentsRepo, listing };
}

/** `ctx.extensions.files`: whether the session's project has an agents repo. */
export type CodingSessionFilesExtension = {
  /**
   * `absent` only on a read that answered "none" (or no project at all);
   * anything unread or unreadable is `unknown`, and the panel says which.
   */
  agentsRepo: "present" | "absent" | "unknown";
};

export function useCodingSessionFilesExtension(
  ctx: CodingSessionSurfaceBaseCtx,
): CodingSessionFilesExtension {
  const read = useCodingSessionAgentsRepoRead(ctx, false);
  if (ctx.projectRef === null) return ABSENT;
  if (read.isAgentsRepo) return PRESENT;
  if (read.coordinate !== null && read.sourceQuery.isSuccess) return ABSENT;
  return UNKNOWN;
}

const ABSENT: CodingSessionFilesExtension = Object.freeze({
  agentsRepo: "absent",
});
const PRESENT: CodingSessionFilesExtension = Object.freeze({
  agentsRepo: "present",
});
const UNKNOWN: CodingSessionFilesExtension = Object.freeze({
  agentsRepo: "unknown",
});

/** Read `ctx.extensions.files` back, or null when this view did not read it. */
export function readCodingSessionFilesExtension(
  value: unknown,
): CodingSessionFilesExtension | null {
  if (typeof value !== "object" || value === null) return null;
  const agentsRepo = (value as { agentsRepo?: unknown }).agentsRepo;
  return agentsRepo === "present" ||
    agentsRepo === "absent" ||
    agentsRepo === "unknown"
    ? (value as CodingSessionFilesExtension)
    : null;
}
