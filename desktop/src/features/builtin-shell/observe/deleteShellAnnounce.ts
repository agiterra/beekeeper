import { useMutation, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { KIND_DELETION, KIND_SHELL_SESSION } from "@/shared/constants/kinds";

import { projectTerminalsQueryKey } from "./useProjectTerminals";

/**
 * Delete a shared terminal by tombstoning its kind:30623 announce.
 *
 * ## What this does, exactly
 *
 * It publishes a NIP-09 kind:5 naming `30623:<owner>:<session-id>`. The
 * relay soft-deletes the announce and drops its roster projection
 * (`delete_shell_session_acl`), so the terminal stops being listed, stops
 * being watchable, and its invites stop granting anything — the 24310 watch
 * and 24312 input gates fall back to project access alone.
 *
 * ## What it deliberately does not do
 *
 * It does not touch the owner's machine. A PTY that is still running keeps
 * running; it just becomes unshareable and unlisted. Only its owner's own
 * app can stop the process, and only they are sitting in front of it. Copy
 * that promised otherwise would be promising something no event can deliver.
 *
 * This is why "delete" is offered *alongside* the existing close, not
 * instead of it: closing publishes a `status: closed` announce and retracts
 * the listing while leaving the head live; deleting removes the head. A
 * project Owner reaching somebody else's terminal only ever has the second.
 */
export async function deleteShellAnnounce({
  ownerPubkey,
  sessionId,
}: {
  ownerPubkey: string;
  sessionId: string;
}): Promise<void> {
  const event = await signRelayEvent({
    kind: KIND_DELETION,
    content: `Delete terminal ${sessionId}`,
    // Exactly one `a` tag and no `e` tag: the relay refuses a deletion
    // carrying both, and routes entirely on this coordinate.
    tags: [
      ["a", `${KIND_SHELL_SESSION}:${ownerPubkey.toLowerCase()}:${sessionId}`],
    ],
  });
  await relayClient.publishEvent(
    event,
    "Timed out deleting the terminal.",
    "Failed to delete the terminal.",
  );
}

/**
 * Mutation wrapper. Invalidates the project's terminal list on success —
 * the relay's own listing is filtered to live announces, so a refetch is
 * what makes the row disappear.
 */
export function useDeleteShellAnnounceMutation(projectAddress: string | null) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteShellAnnounce,
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: projectTerminalsQueryKey(projectAddress ?? "none"),
      });
    },
  });
}
