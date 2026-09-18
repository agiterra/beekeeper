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
import { useRelayOrigin } from "@/shared/lib/useRelayOrigin";

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
import { useProjectRosterQuery } from "../lib/projectMembers";
import { useMoveRepoToProjectMutation } from "../projectOrganizeMutations";
import {
  addProjectMembers,
  useCreateProjectContainerMutation,
} from "../useCreateProjectContainer";
import { useCreateProjectRepoMutation } from "../useCreateProjectRepo";
import { toastCreateProjectOutcome } from "../lib/toastCreateProjectOutcome";
import {
  ensureRealProject,
  useGeneralProjectRefResolver,
} from "../useGeneralProjectMigration";
import { useImportProjectRepoMutation } from "../useImportProjectRepo";
import { AttachProjectRepoDialog } from "./AttachProjectRepoDialog";
import { CreateProjectContainerDialog } from "./CreateProjectContainerDialog";
import { CreateProjectRepoDialog } from "./CreateProjectRepoDialog";
import { ImportProjectRepoDialog } from "./ImportProjectRepoDialog";

export type ProjectsScreenCreateKind =
  | "channel"
  | "forum"
  | "workflow"
  | "repo"
  | "repo-attach"
  | "repo-import"
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
  const importRepoMutation = useImportProjectRepoMutation();
  const moveRepoMutation = useMoveRepoToProjectMutation();
  const relayOrigin = useRelayOrigin();

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

  // Repository access comes from the target project's roster — no channel to
  // pick, and nothing that can block the dialog from opening. The roster is
  // read only to tell the user when it is empty (see ProjectRepoAccessNote);
  // the relay, not this query, is what authorizes.
  const rosterQuery = useProjectRosterQuery(
    kind === "repo" || kind === "repo-import" ? repoTargetProject : null,
  );
  const otherMemberCount = React.useMemo(() => {
    if (!rosterQuery.data) return null;
    const viewer = (currentPubkey ?? "").toLowerCase();
    return rosterQuery.data.filter(
      (member) => member.pubkey.toLowerCase() !== viewer,
    ).length;
  }, [rosterQuery.data, currentPubkey]);

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
        open={kind === "repo"}
        otherMemberCount={otherMemberCount}
        projectName={repoTargetProject.name}
      />

      <ImportProjectRepoDialog
        isImporting={importRepoMutation.isPending}
        onImport={async (input) => {
          const result = await importRepoMutation.mutateAsync({
            project: repoTargetProject,
            relayOrigin,
            ...input,
          });
          toast.success(`Repository "${result.name}" imported.`);
          onRepoCreated?.();
        }}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "repo-import"}
        otherMemberCount={otherMemberCount}
        ownerPubkey={currentPubkey?.toLowerCase()}
        projectName={repoTargetProject.name}
        relayOrigin={relayOrigin}
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
          const outcome = await createProjectMutation.mutateAsync(input);
          toastCreateProjectOutcome(outcome);
        }}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        open={kind === "project"}
      />
    </>
  );
}
