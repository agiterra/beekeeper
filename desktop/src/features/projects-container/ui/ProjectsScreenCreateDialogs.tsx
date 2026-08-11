import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useApplyTemplate } from "@/features/channel-templates/useApplyTemplate";
import {
  useChannelsQuery,
  useCreateChannelMutation,
} from "@/features/channels/hooks";
import { CreateProjectDialog } from "@/features/projects/ui/CreateProjectDialog";
import { useCreateProjectMutation } from "@/features/projects/useCreateProject";
import { CreateChannelDialog } from "@/features/sidebar/ui/CreateChannelDialog";
import { WorkflowDialog } from "@/features/workflows/ui/WorkflowDialog";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { ChannelVisibility } from "@/shared/api/types";

import {
  partitionChannels,
  projectContainersQueryKey,
  useProjectContainers,
  type ProjectContainer,
} from "../hooks";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
} from "../lib/projectContainerModel";
import {
  addProjectMembers,
  useCreateProjectContainerMutation,
} from "../useCreateProjectContainer";
import {
  ensureRealProject,
  useGeneralProjectRefResolver,
} from "../useGeneralProjectMigration";
import { CreateProjectContainerDialog } from "./CreateProjectContainerDialog";

export type ProjectsScreenCreateKind =
  | "channel"
  | "forum"
  | "workflow"
  | "repo"
  | "project";

/**
 * The create dialogs behind the projects screen's `+` menu and the
 * per-project screen's section `+` buttons. Channels, forums, workflows, and
 * repos land in `targetProject` — falling back to General without one.
 */
export function ProjectsScreenCreateDialogs({
  kind,
  targetProject,
  onClose,
}: {
  kind: ProjectsScreenCreateKind | null;
  targetProject: ProjectContainer | null;
  onClose: () => void;
}) {
  const { goChannel } = useAppNavigation();
  const { projects } = useProjectContainers();
  const queryClient = useQueryClient();
  const identityQuery = useIdentityQuery();
  const currentPubkey = identityQuery.data?.pubkey;
  const channelsQuery = useChannelsQuery();
  const { applyCanvas, applyAgents } = useApplyTemplate();
  const resolveGeneralProjectRef = useGeneralProjectRefResolver(projects, true);
  const createChannelMutation = useCreateChannelMutation();
  const createProjectMutation = useCreateProjectContainerMutation();
  const createRepoMutation = useCreateProjectMutation();

  const channelBuckets = React.useMemo(
    () => partitionChannels(projects, channelsQuery.data ?? []),
    [projects, channelsQuery.data],
  );
  // A workflow's trigger channel must come from the target project (General
  // also offers the unclaimed channels it displays).
  const workflowChannels = React.useMemo(() => {
    if (!targetProject) {
      return (channelsQuery.data ?? []).filter(
        (channel) => channel.channelType !== "dm",
      );
    }
    const isGeneral =
      targetProject.dtag === GENERAL_PROJECT_DTAG ||
      targetProject.id === LOCAL_GENERAL_ID;
    return [
      ...(channelBuckets.channelsByProject.get(targetProject.id) ?? []),
      ...(channelBuckets.forumsByProject.get(targetProject.id) ?? []),
      ...(isGeneral
        ? [...channelBuckets.globalChannels, ...channelBuckets.unclaimedForums]
        : []),
    ];
  }, [channelBuckets, channelsQuery.data, targetProject]);

  React.useEffect(() => {
    if (kind !== "workflow") return;
    if (channelsQuery.isLoading || workflowChannels.length > 0) return;
    toast.error("Add a channel to this project before creating a workflow.");
    onClose();
  }, [kind, channelsQuery.isLoading, workflowChannels.length, onClose]);

  const handleCreateChannel = React.useCallback(
    async (input: {
      name: string;
      description?: string;
      visibility: ChannelVisibility;
      ttlSeconds?: number;
      templateId?: string;
    }) => {
      // Creating into the local General placeholder publishes the real
      // General first so the channel has a valid coordinate to reference.
      const target = targetProject
        ? await ensureRealProject(targetProject)
        : null;
      const projectRef = target
        ? target.address
        : await resolveGeneralProjectRef();
      const created = await createChannelMutation.mutateAsync({
        name: input.name,
        description: input.description,
        channelType: kind === "forum" ? "forum" : "stream",
        visibility: input.visibility,
        ttlSeconds: input.ttlSeconds,
        projectRef,
      });
      if (target) {
        if (target.owner === (currentPubkey ?? "").toLowerCase()) {
          try {
            await addProjectMembers(target, { channelIds: [created.id] });
          } catch {
            // The channel's own back-reference still associates it; the
            // owner-curated forward ref is best-effort.
          }
        }
        void queryClient.invalidateQueries({
          queryKey: projectContainersQueryKey,
        });
      }
      await applyCanvas(input.templateId, created.id, input.name);
      await goChannel(created.id);
      void applyAgents(input.templateId, created.id);
    },
    [
      kind,
      targetProject,
      currentPubkey,
      queryClient,
      resolveGeneralProjectRef,
      createChannelMutation,
      applyCanvas,
      applyAgents,
      goChannel,
    ],
  );

  return (
    <>
      <CreateChannelDialog
        channelKind={
          kind === "channel" ? "stream" : kind === "forum" ? "forum" : null
        }
        isCreating={createChannelMutation.isPending}
        onCreate={handleCreateChannel}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
      />

      <WorkflowDialog
        channels={workflowChannels}
        mode="create"
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "workflow" && workflowChannels.length > 0}
      />

      <CreateProjectDialog
        isCreating={createRepoMutation.isPending}
        onCreate={async (input) => {
          // The new repo lands in the target project (publishing the real
          // General first when the target is the local placeholder).
          const target = targetProject
            ? await ensureRealProject(targetProject)
            : null;
          const result = await createRepoMutation.mutateAsync(
            target ? { ...input, projectRef: target.address } : input,
          );
          const project = result.project;
          const createdRepoAddress =
            project.repositories[0]?.repoAddress ?? null;
          if (target) {
            if (
              createdRepoAddress &&
              target.owner === (currentPubkey ?? "").toLowerCase()
            ) {
              try {
                await addProjectMembers(target, {
                  repoAddrs: [createdRepoAddress],
                });
              } catch {
                // The repo's own back-reference still associates it; the
                // owner-curated forward ref is best-effort.
              }
            }
            void queryClient.invalidateQueries({
              queryKey: projectContainersQueryKey,
            });
          }
          if (result.compatibilityWarning) {
            toast.warning("Created as a standalone project", {
              description: result.compatibilityWarning,
            });
          } else {
            toast.success(`Project "${project.name}" created.`);
          }
        }}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "repo"}
      />

      <CreateProjectContainerDialog
        isCreating={createProjectMutation.isPending}
        onCreate={async (input) => {
          await createProjectMutation.mutateAsync(input);
        }}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "project"}
      />
    </>
  );
}
