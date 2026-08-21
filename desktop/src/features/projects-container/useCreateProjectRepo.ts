import { useMutation, useQueryClient } from "@tanstack/react-query";

import { projectsQueryKey } from "@/features/projects/hooks";
import { buildInitialProjectEventTemplates } from "@/features/projects/projectCreation";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { KIND_REPO_ANNOUNCEMENT } from "@/shared/constants/kinds";

import { projectContainersQueryKey, type ProjectContainer } from "./hooks";
import { addProjectMembers } from "./useCreateProjectContainer";
import { ensureRealProject } from "./useGeneralProjectMigration";

export type CreateProjectRepoInput = {
  /** Target container — may be the local General placeholder. */
  project: ProjectContainer;
  name: string;
  accessChannelId: string;
  description?: string;
  cloneUrl?: string;
  webUrl?: string;
};

export type CreateProjectRepoResult = {
  dtag: string;
  name: string;
  repoAddress: string;
};

/**
 * Create a repository inside an existing project container: a single
 * kind:30617 announcement carrying the container back-ref (`project` tag),
 * plus a best-effort owner-curated forward ref on the container. Unlike the
 * legacy create-project flow, no per-repo kind:30621 head is published — the
 * repo joins `input.project` instead of spawning a new project.
 */
export async function createProjectRepo(
  input: CreateProjectRepoInput,
): Promise<CreateProjectRepoResult> {
  // Creating into the local General placeholder publishes the real General
  // first so the repo has a valid coordinate to reference.
  const target = await ensureRealProject(input.project);
  const identity = await getIdentity();
  const ownerPubkey = identity.pubkey.toLowerCase();

  const templates = buildInitialProjectEventTemplates({
    accessChannelId: input.accessChannelId,
    cloneUrl: input.cloneUrl,
    description: input.description,
    name: input.name,
    ownerPubkey,
    projectRef: target.address,
    webUrl: input.webUrl,
  });

  // D-tag clobber guard: an existing 30617 at this coordinate — standalone
  // or in another project — would be silently overwritten by this write.
  const existing = await relayClient.fetchEvents({
    kinds: [KIND_REPO_ANNOUNCEMENT],
    authors: [ownerPubkey],
    "#d": [templates.dtag],
    limit: 1,
  });
  if (existing.length > 0) {
    throw new Error(
      `A repository named "${templates.dtag}" already exists (as a standalone repository or in another project). Choose a different name to avoid overwriting it.`,
    );
  }

  const event = await signRelayEvent(templates.repository);
  try {
    await relayClient.publishEvent(
      event,
      "Timed out creating the repository.",
      "Failed to create the repository.",
    );
  } catch (publishError) {
    // Lost-ACK recovery: if the relay already stored this exact event, the
    // write succeeded and only the acknowledgement was lost. Without this,
    // a resubmit would strand the user on the duplicate-name guard above.
    let alreadyStored = false;
    try {
      const stored = await relayClient.fetchEvents({
        ids: [event.id],
        kinds: [KIND_REPO_ANNOUNCEMENT],
        limit: 1,
      });
      alreadyStored = stored.length > 0;
    } catch {
      // Ignore — if the query itself fails, surface the publish error.
    }
    if (!alreadyStored) throw publishError;
  }

  if (target.owner === ownerPubkey) {
    try {
      await addProjectMembers(target, {
        repoAddrs: [templates.repositoryAddress],
      });
    } catch {
      // The repo's own back-reference still associates it; the
      // owner-curated forward ref is best-effort.
    }
  }

  return {
    dtag: templates.dtag,
    name: input.name.trim(),
    repoAddress: templates.repositoryAddress,
  };
}

export function useCreateProjectRepoMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: createProjectRepo,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
    },
  });
}
