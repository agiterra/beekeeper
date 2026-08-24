import { useMutation, useQueryClient } from "@tanstack/react-query";

import {
  projectsQueryKey,
  type Repository as CodeRepo,
} from "@/features/projects/hooks";
import { deriveRelayCloneUrl } from "@/features/projects/lib/projectCloneUrl";
import {
  linkProjectLocalRepository,
  type RepoRemoteStrategy,
} from "@/shared/api/projectGit";

import { projectContainersQueryKey } from "./hooks";
import { offerTerminalGitAccess } from "./offerTerminalGitAccess";

export type LinkProjectRepoInput = {
  /** The already-announced repository (owned by anyone) to link. */
  repo: CodeRepo;
  /** Absolute path of the local checkout chosen by the user. */
  path: string;
  remoteStrategy: RepoRemoteStrategy;
  relayOrigin: string | null;
};

export type LinkProjectRepoResult = {
  name: string;
  path: string;
  remote: string;
};

/**
 * The URL a linked checkout's remote points at. Always the relay-hosted
 * location — the relay serves `/git/<owner>/<dtag>` regardless of the
 * announcement's `clone` tag, which may name the external upstream the repo
 * was forked from (and would fail the workspace clone-URL gate).
 */
export function linkedRepoCloneUrl(
  repo: Pick<CodeRepo, "owner" | "dtag">,
  relayOrigin: string | null,
): string | null {
  return deriveRelayCloneUrl(relayOrigin, repo.owner.toLowerCase(), repo.dtag);
}

/**
 * Link an existing local checkout to an already-announced repository: remote
 * setup + reachability check + registry entry — no announce, no push (the
 * linker may only have read access).
 */
export async function linkProjectRepo(
  input: LinkProjectRepoInput,
): Promise<LinkProjectRepoResult> {
  const cloneUrl = linkedRepoCloneUrl(input.repo, input.relayOrigin);
  if (!cloneUrl) {
    throw new Error("Relay origin unavailable — reconnect and try again.");
  }
  const result = await linkProjectLocalRepository({
    path: input.path,
    cloneUrl,
    owner: input.repo.owner.toLowerCase(),
    dtag: input.repo.dtag,
    remoteStrategy: input.remoteStrategy,
  });
  return {
    name: input.repo.name,
    path: result.path,
    remote: result.remote,
  };
}

export function useLinkProjectRepoMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: linkProjectRepo,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
      void queryClient.invalidateQueries({
        queryKey: ["projects", "local-repositories"],
      });
      // Linking wires the same relay remote as importing does, so it leaves a
      // terminal in the same state: a remote it cannot authenticate to.
      void offerTerminalGitAccess();
    },
  });
}
