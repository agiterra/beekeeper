import * as React from "react";
import { toast } from "sonner";

import type { Repository as CodeRepo } from "@/features/projects/hooks";
import type { Channel } from "@/shared/api/types";

import type { ProjectContainer } from "../hooks";
import {
  useMoveChannelToProjectMutation,
  useMoveRepoToProjectMutation,
} from "../projectOrganizeMutations";

/**
 * Item-move actions shared by the all-projects manage panel and the
 * per-project screen: repo/channel/forum moves with toasts.
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
} {
  const { mutate: mutateMoveChannel } = useMoveChannelToProjectMutation();
  const { mutate: mutateMoveRepo } = useMoveRepoToProjectMutation();

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
      moveChannel(channel, fromId, to);
    },
    [moveChannel],
  );

  const requestMoveForum = React.useCallback(
    (channel: Channel, fromId: string | null, to: ProjectContainer) => {
      moveChannel(channel, fromId, to);
    },
    [moveChannel],
  );

  const requestMoveRepo = React.useCallback(
    (repo: CodeRepo, fromId: string | null, to: ProjectContainer) => {
      moveRepo(repo, fromId, to);
    },
    [moveRepo],
  );

  return {
    requestMoveRepo,
    requestMoveChannel,
    requestMoveForum,
  };
}
