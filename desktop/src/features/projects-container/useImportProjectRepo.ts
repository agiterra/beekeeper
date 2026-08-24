import { useMutation, useQueryClient } from "@tanstack/react-query";

import { projectsQueryKey } from "@/features/projects/hooks";
import { deriveRelayCloneUrl } from "@/features/projects/lib/projectCloneUrl";
import { projectDtagFromName } from "@/features/projects/projectCreation";
import {
  importProjectLocalRepository,
  type RepoRemoteStrategy,
} from "@/shared/api/projectGit";
import { relayClient } from "@/shared/api/relayClient";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { KIND_REPO_ANNOUNCEMENT } from "@/shared/constants/kinds";

import { projectContainersQueryKey, type ProjectContainer } from "./hooks";
import { offerTerminalGitAccess } from "./offerTerminalGitAccess";
import { createProjectRepo } from "./useCreateProjectRepo";
import { ensureRealProject } from "./useGeneralProjectMigration";

export type ImportProjectRepoInput = {
  /** Target container — may be the local General placeholder. */
  project: ProjectContainer;
  name: string;
  /** Optional legacy `buzz-channel` binding; see `CreateProjectRepoInput`. */
  accessChannelId?: string;
  /** Absolute path of the local checkout chosen by the user. */
  path: string;
  remoteStrategy: RepoRemoteStrategy;
  relayOrigin: string | null;
};

export type ImportProjectRepoResult = {
  dtag: string;
  name: string;
  repoAddress: string;
  path: string;
  remote: string;
  branch: string;
};

/**
 * Import an existing local checkout: announce the repo into the project
 * (single 30617, relay-hosted — no `clone` tag), wire the folder's remote to
 * the derived relay URL, push the current branch, and register the checkout.
 */
export async function importProjectRepo(
  input: ImportProjectRepoInput,
): Promise<ImportProjectRepoResult> {
  const target = await ensureRealProject(input.project);
  const identity = await getIdentity();
  const owner = identity.pubkey.toLowerCase();

  let dtag: string;
  let repoAddress: string;
  try {
    const created = await createProjectRepo({
      project: target,
      name: input.name,
      accessChannelId: input.accessChannelId,
    });
    dtag = created.dtag;
    repoAddress = created.repoAddress;
  } catch (error) {
    // A failed earlier import may have left the announcement behind. Reuse
    // it only when it is ours AND already points at this project — any other
    // collision keeps the duplicate-name error.
    const candidate = projectDtagFromName(input.name.trim());
    if (
      !candidate ||
      !(error instanceof Error) ||
      !/already exists/.test(error.message)
    ) {
      throw error;
    }
    const existing = await relayClient.fetchEvents({
      kinds: [KIND_REPO_ANNOUNCEMENT],
      authors: [owner],
      "#d": [candidate],
      limit: 1,
    });
    const announcement = existing[0];
    const projectRef = announcement?.tags.find(
      (tag) => tag[0] === "project",
    )?.[1];
    if (!announcement || projectRef !== target.address) {
      throw error;
    }
    dtag = candidate;
    repoAddress = `${KIND_REPO_ANNOUNCEMENT}:${owner}:${candidate}`;
  }

  const cloneUrl = deriveRelayCloneUrl(input.relayOrigin, owner, dtag);
  if (!cloneUrl) {
    throw new Error("Relay origin unavailable — reconnect and try again.");
  }
  const result = await importProjectLocalRepository({
    path: input.path,
    cloneUrl,
    owner,
    dtag,
    remoteStrategy: input.remoteStrategy,
  });

  return {
    dtag,
    name: input.name.trim(),
    repoAddress,
    path: result.path,
    remote: result.remote,
    branch: result.branch,
  };
}

export function useImportProjectRepoMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: importProjectRepo,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
      void queryClient.invalidateQueries({
        queryKey: ["projects", "local-repositories"],
      });
      void offerTerminalGitAccess();
    },
  });
}
