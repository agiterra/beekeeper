import * as React from "react";
import { toast } from "sonner";

import type { Repository as CodeRepo } from "@/features/projects/hooks";
import type { Channel } from "@/shared/api/types";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";

import type { ProjectContainer } from "../hooks";
import {
  useMoveChannelToProjectMutation,
  useMoveRepoToProjectMutation,
} from "../projectOrganizeMutations";

type PendingMove =
  | {
      type: "repo";
      repo: CodeRepo;
      fromId: string | null;
      to: ProjectContainer;
    }
  | {
      type: "channel" | "forum";
      channel: Channel;
      fromId: string | null;
      to: ProjectContainer;
    };

/**
 * Item-move actions shared by the all-projects manage panel and the
 * per-project screen: repo/channel/forum moves with toasts, plus the confirm
 * gate for moving something into a private project (which silently hides it
 * from everyone but the project's owner/members, so it's worth a beat before
 * that happens). Render `confirmDialog` once wherever the hook is used.
 */
export function useProjectItemMoves(
  projectById: ReadonlyMap<string, ProjectContainer>,
): {
  requestMoveRepo: (
    repo: CodeRepo,
    fromId: string | null,
    to: ProjectContainer,
  ) => void;
  requestMoveChannel: (
    channel: Channel,
    fromId: string | null,
    to: ProjectContainer,
  ) => void;
  requestMoveForum: (
    channel: Channel,
    fromId: string | null,
    to: ProjectContainer,
  ) => void;
  confirmDialog: React.ReactNode;
} {
  const { mutate: mutateMoveChannel } = useMoveChannelToProjectMutation();
  const { mutate: mutateMoveRepo } = useMoveRepoToProjectMutation();
  const [pendingMove, setPendingMove] = React.useState<PendingMove | null>(
    null,
  );

  const moveChannel = React.useCallback(
    (channel: Channel, fromId: string | null, to: ProjectContainer) => {
      const from = fromId ? (projectById.get(fromId) ?? null) : null;
      mutateMoveChannel(
        { channel, from, to },
        {
          onSuccess: () =>
            toast.success(`Moved #${channel.name} to ${to.name}.`),
          onError: (error) =>
            toast.error(
              error instanceof Error ? error.message : "Failed to move.",
            ),
        },
      );
    },
    [mutateMoveChannel, projectById],
  );

  const moveRepo = React.useCallback(
    (repo: CodeRepo, fromId: string | null, to: ProjectContainer) => {
      const from = fromId ? (projectById.get(fromId) ?? null) : null;
      mutateMoveRepo(
        { repo, from, to },
        {
          onSuccess: () => toast.success(`Moved ${repo.name} to ${to.name}.`),
          onError: (error) =>
            toast.error(
              error instanceof Error ? error.message : "Failed to move.",
            ),
        },
      );
    },
    [mutateMoveRepo, projectById],
  );

  const requestMoveChannel = React.useCallback(
    (channel: Channel, fromId: string | null, to: ProjectContainer) => {
      if (to.visibility === "private") {
        setPendingMove({ type: "channel", channel, fromId, to });
        return;
      }
      moveChannel(channel, fromId, to);
    },
    [moveChannel],
  );

  const requestMoveForum = React.useCallback(
    (channel: Channel, fromId: string | null, to: ProjectContainer) => {
      if (to.visibility === "private") {
        setPendingMove({ type: "forum", channel, fromId, to });
        return;
      }
      moveChannel(channel, fromId, to);
    },
    [moveChannel],
  );

  const requestMoveRepo = React.useCallback(
    (repo: CodeRepo, fromId: string | null, to: ProjectContainer) => {
      if (to.visibility === "private") {
        setPendingMove({ type: "repo", repo, fromId, to });
        return;
      }
      moveRepo(repo, fromId, to);
    },
    [moveRepo],
  );

  const confirmPendingMove = React.useCallback(() => {
    if (!pendingMove) return;
    if (pendingMove.type === "repo") {
      moveRepo(pendingMove.repo, pendingMove.fromId, pendingMove.to);
    } else {
      moveChannel(pendingMove.channel, pendingMove.fromId, pendingMove.to);
    }
    setPendingMove(null);
  }, [pendingMove, moveChannel, moveRepo]);

  const confirmDialog = (
    <AlertDialog
      onOpenChange={(open) => {
        if (!open) setPendingMove(null);
      }}
      open={pendingMove !== null}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Move into a private project?</AlertDialogTitle>
          <AlertDialogDescription>
            {pendingMove
              ? `"${pendingMove.type === "repo" ? pendingMove.repo.name : pendingMove.channel.name}" will only be visible to ${pendingMove.to.name}'s owner and invited members.`
              : ""}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={() => setPendingMove(null)}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="manage-move-confirm"
            onClick={(event) => {
              event.preventDefault();
              confirmPendingMove();
            }}
          >
            Move
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return {
    requestMoveRepo,
    requestMoveChannel,
    requestMoveForum,
    confirmDialog,
  };
}
