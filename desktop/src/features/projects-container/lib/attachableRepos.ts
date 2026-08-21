import type { Repository as CodeRepo } from "@/features/projects/hooks";

import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  type ProjectContainer,
} from "./projectContainerModel";

export type AttachableRepos = {
  candidates: CodeRepo[];
  /** Source project per candidate address (`null` for unclaimed repos). */
  fromByAddress: Map<string, ProjectContainer | null>;
};

/**
 * Repos that can be moved into `target`: every repo it does not already
 * hold. Unclaimed repos display under General, so they are candidates only
 * when the target is a real (non-General) project.
 */
export function attachableProjectRepos(
  projects: ProjectContainer[],
  reposByProject: ReadonlyMap<string, CodeRepo[]>,
  unclaimedRepos: CodeRepo[],
  target: ProjectContainer,
): AttachableRepos {
  const isGeneral =
    target.dtag === GENERAL_PROJECT_DTAG || target.id === LOCAL_GENERAL_ID;
  const fromByAddress = new Map<string, ProjectContainer | null>();
  const candidates: CodeRepo[] = [];
  for (const [projectId, repos] of reposByProject) {
    if (projectId === target.id) continue;
    const source = projects.find((project) => project.id === projectId);
    for (const repo of repos) {
      fromByAddress.set(repo.repoAddress, source ?? null);
      candidates.push(repo);
    }
  }
  if (!isGeneral) {
    for (const repo of unclaimedRepos) {
      fromByAddress.set(repo.repoAddress, null);
      candidates.push(repo);
    }
  }
  return { candidates, fromByAddress };
}
