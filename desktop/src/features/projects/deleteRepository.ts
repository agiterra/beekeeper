import { useMutation, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import {
  KIND_DELETION,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";

import { projectsQueryKey } from "./hooks";

/**
 * Delete a repository by tombstoning its kind:30617 announcement.
 *
 * ## What the relay does with this
 *
 * More than hide it. The announcement is soft-deleted and its project link
 * cleared, *and* the relay-signed kind:30618 ref state goes with it, *and*
 * the object-store pointer every git read path resolves is removed. Without
 * those last two the repository would stay fully cloneable and its branches
 * publicly readable — a delete that removed a listing and nothing else.
 *
 * ## What survives, deliberately
 *
 * - **The name.** `git_repo_names` keeps the reservation, so deleting a
 *   repository never frees its name for somebody else to take. Re-announcing
 *   it under the same name is possible for its owner and nobody else.
 * - **The packed objects.** They are content-addressed and shared between
 *   repositories — a fork, or two repos with the same tree, are the same
 *   bytes — so reclaiming them here could destroy a neighbour's history.
 *   They are left for an operator sweep, and the dialog says so.
 * - **Any local checkout.** This is a relay event; it does not touch the
 *   clone on anybody's disk.
 */
export async function deleteRepository({
  ownerPubkey,
  repoId,
  name,
}: {
  ownerPubkey: string;
  repoId: string;
  name?: string;
}): Promise<void> {
  const event = await signRelayEvent({
    kind: KIND_DELETION,
    content: `Delete repository ${name ?? repoId}`,
    // Exactly one `a` tag and no `e` tag — the relay refuses a kind:5
    // carrying both, and routes entirely on this coordinate.
    tags: [
      ["a", `${KIND_REPO_ANNOUNCEMENT}:${ownerPubkey.toLowerCase()}:${repoId}`],
    ],
  });
  await relayClient.publishEvent(
    event,
    "Timed out deleting the repository.",
    "Failed to delete the repository.",
  );
}

/** Mutation wrapper; refreshes the repository list on success. */
export function useDeleteRepositoryMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteRepository,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });
}
