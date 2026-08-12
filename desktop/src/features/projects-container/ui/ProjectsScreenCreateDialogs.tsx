import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useApplyTemplate } from "@/features/channel-templates/useApplyTemplate";
import {
  useChannelsQuery,
  useCreateChannelMutation,
} from "@/features/channels/hooks";
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
import { attachableProjectRepos } from "../lib/attachableRepos";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  makeLocalGeneral,
} from "../lib/projectContainerModel";
import { useMoveRepoToProjectMutation } from "../projectOrganizeMutations";
import {
  addProjectMembers,
  useCreateProjectContainerMutation,
} from "../useCreateProjectContainer";
import { useCreateProjectRepoMutation } from "../useCreateProjectRepo";
import {
  ensureRealProject,
  useGeneralProjectRefResolver,
} from "../useGeneralProjectMigration";
import { AttachProjectRepoDialog } from "./AttachProjectRepoDialog";
import { CreateProjectContainerDialog } from "./CreateProjectContainerDialog";
import { CreateProjectRepoDialog } from "./CreateProjectRepoDialog";

export type ProjectsScreenCreateKind =
  | "channel"
  | "forum"
  | "workflow"
  | "repo"
  | "repo-attach"
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
  onRepoCreated,
}: {
  kind: ProjectsScreenCreateKind | null;
  targetProject: ProjectContainer | null;
  onClose: () => void;
  /** Fires after a repository is created into the target project. */
  onRepoCreated?: () => void;
}) {
  const { goChannel } = useAppNavigation();
  const { projects, reposByProject, unclaimedRepos } = useProjectContainers();
  const queryClient = useQueryClient();
  const identityQuery = useIdentityQuery();
  const currentPubkey = identityQuery.data?.pubkey;
  const channelsQuery = useChannelsQuery();
  const { applyCanvas, applyAgents } = useApplyTemplate();
  const resolveGeneralProjectRef = useGeneralProjectRefResolver(projects, true);
  const createChannelMutation = useCreateChannelMutation();
  const createProjectMutation = useCreateProjectContainerMutation();
  const createRepoMutation = useCreateProjectRepoMutation();
  const moveRepoMutation = useMoveRepoToProjectMutation();

  // Repos, channels, forums, and workflows land in the target project —
  // falling back to General (published lazily) when the host passes none.
  const repoTargetProject = React.useMemo(
    () =>
      targetProject ??
      projects.find((project) => project.dtag === GENERAL_PROJECT_DTAG) ??
      makeLocalGeneral(),
    [targetProject, projects],
  );

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

  // A repository's access channel can be any channel the user is in; the
  // default prefers the target project's own channels.
  const repoAccessChannels = React.useMemo(
    () =>
      (channelsQuery.data ?? []).filter(
        (channel) =>
          channel.isMember &&
          !channel.archivedAt &&
          channel.channelType !== "dm",
      ),
    [channelsQuery.data],
  );
  const defaultRepoChannelId = React.useMemo(() => {
    const eligible = new Set(repoAccessChannels.map((channel) => channel.id));
    return (
      workflowChannels.find((channel) => eligible.has(channel.id))?.id ??
      repoAccessChannels[0]?.id
    );
  }, [repoAccessChannels, workflowChannels]);

  React.useEffect(() => {
    if (kind !== "repo") return;
    if (channelsQuery.isLoading || repoAccessChannels.length > 0) return;
    toast.error("Add a channel to this project before creating a repository.");
    onClose();
  }, [kind, channelsQuery.isLoading, repoAccessChannels.length, onClose]);

  const attachCandidates = React.useMemo(
    () =>
      attachableProjectRepos(
        projects,
        reposByProject,
        unclaimedRepos,
        repoTargetProject,
      ),
    [projects, reposByProject, unclaimedRepos, repoTargetProject],
  );

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

      <CreateProjectRepoDialog
        channels={repoAccessChannels}
        defaultChannelId={defaultRepoChannelId}
        isCreating={createRepoMutation.isPending}
        onCreate={async (input) => {
          // The new repo lands in the target project only — no new project
          // container is published (the mutation publishes the real General
          // first when the target is the local placeholder).
          const result = await createRepoMutation.mutateAsync({
            project: repoTargetProject,
            ...input,
          });
          toast.success(`Repository "${result.name}" created.`);
          onRepoCreated?.();
        }}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "repo" && repoAccessChannels.length > 0}
        projectName={repoTargetProject.name}
      />

      <AttachProjectRepoDialog
        isAttaching={moveRepoMutation.isPending}
        onAttach={async (repo) => {
          await moveRepoMutation.mutateAsync({
            repo,
            from: attachCandidates.fromByAddress.get(repo.repoAddress) ?? null,
            to: repoTargetProject,
          });
          toast.success(`Moved ${repo.name} to ${repoTargetProject.name}.`);
        }}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "repo-attach"}
        projectName={repoTargetProject.name}
        repos={attachCandidates.candidates}
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
