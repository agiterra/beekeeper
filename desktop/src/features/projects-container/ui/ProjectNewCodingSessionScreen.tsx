import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import {
  channelsQueryKey,
  useChannelsQuery,
  useCreateChannelMutation,
} from "@/features/channels/hooks";
import {
  NewCodingSessionScreen,
  type NewCodingSessionProjectContext,
} from "@/features/coding-sessions/ui/NewCodingSessionScreen";
import { projectDefaultCwd } from "@/features/builtin-shell/lib/projectShellCwd";
import { useIdentityQuery } from "@/shared/api/hooks";

import {
  partitionChannels,
  projectContainersQueryKey,
  useProjectCodingSessionBuckets,
  useProjectContainers,
  type ProjectContainer,
} from "../hooks";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  makeLocalGeneral,
} from "../lib/projectContainerModel";
import {
  projectSessionActivityByChannel,
  projectSessionsChannelDescription,
  projectSessionsChannelName,
  resolveProjectSessionsChannel,
} from "../lib/projectSessionsChannel";
import { addProjectMembers } from "../useCreateProjectContainer";
import { ensureRealProject } from "../useGeneralProjectMigration";

/**
 * The create flow, with the project already answered.
 *
 * Everything about creating a session is the standalone screen's job; this
 * wrapper only supplies the two facts a project adds — which coordinate to sign
 * into the create, and which channel the transcript lives in — and knows how to
 * bring that channel into existence when the project has never had one.
 */
export function ProjectNewCodingSessionScreen({
  projectId,
}: {
  projectId: string;
}) {
  const { projects, reposByProject, unclaimedRepos } = useProjectContainers();
  // Transports included: the resolver's rule 0 and the session buckets both
  // need the hidden transport channel this screen exists to find or create.
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const identity = useIdentityQuery();
  const queryClient = useQueryClient();
  const createChannelMutation = useCreateChannelMutation();

  const project: ProjectContainer | null = React.useMemo(() => {
    if (projectId === LOCAL_GENERAL_ID) {
      return (
        projects.find((candidate) => candidate.dtag === GENERAL_PROJECT_DTAG) ??
        makeLocalGeneral()
      );
    }
    return (
      projects.find(
        (candidate) =>
          candidate.id === projectId || candidate.dtag === projectId,
      ) ?? null
    );
  }, [projects, projectId]);

  // The workdir prefill: a local checkout of one of the project's repos.
  // Resolved async against this computer's registered checkouts + repos-root
  // scan; null (no repos, no checkout, non-Tauri) leaves the field to the
  // provider's own remembered directories.
  const projectRepos = React.useMemo(
    () =>
      project
        ? [
            ...(reposByProject.get(project.id) ?? []),
            ...(project.dtag === GENERAL_PROJECT_DTAG ? unclaimedRepos : []),
          ]
        : [],
    [project, reposByProject, unclaimedRepos],
  );
  const [repoCheckout, setRepoCheckout] = React.useState<string | null>(null);
  React.useEffect(() => {
    let cancelled = false;
    setRepoCheckout(null);
    if (projectRepos.length === 0) return;
    void projectDefaultCwd(projectRepos).then((path) => {
      if (!cancelled && path) setRepoCheckout(path);
    });
    return () => {
      cancelled = true;
    };
  }, [projectRepos]);

  const channelBuckets = React.useMemo(
    () => partitionChannels(projects, channelsQuery.data ?? []),
    [projects, channelsQuery.data],
  );
  const sessionBuckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    channelBuckets.channelsByProject,
    channelBuckets.forumsByProject,
  );

  const projectChannels = React.useMemo(
    () =>
      project ? (channelBuckets.channelsByProject.get(project.id) ?? []) : [],
    [channelBuckets.channelsByProject, project],
  );
  const resolvedChannel = React.useMemo(() => {
    if (!project) return null;
    return resolveProjectSessionsChannel({
      projectName: project.name,
      projectChannels,
      sessionActivityByChannel: projectSessionActivityByChannel(
        sessionBuckets.byProject.get(project.id) ?? [],
      ),
    });
  }, [project, projectChannels, sessionBuckets.byProject]);

  const { mutateAsync: createChannel } = createChannelMutation;
  const selfPubkey = identity.data?.pubkey?.toLowerCase();
  const ensureChannelId = React.useCallback(async () => {
    if (!project) {
      throw new Error("This project is no longer available.");
    }
    if (resolvedChannel) return resolvedChannel.channelId;
    // Channels must belong to a project, so a session created inside the local
    // General placeholder publishes the real General first.
    const target = await ensureRealProject(project);
    // A hidden transport channel: identified by type (never surfaced to
    // people), and the relay admits project members through the project ACL
    // instead of explicit channel membership. Private: the transport gate is
    // the read model; an open channel would advertise it community-wide. A
    // relay that predates the transport type rejects the create — fall back
    // to the legacy private stream (name-keyed, creator-only) so session
    // creation never breaks against an old relay.
    const transportInput = {
      name: projectSessionsChannelName(target.name),
      channelType: "transport" as const,
      visibility: "private" as const,
      description: projectSessionsChannelDescription(target.name),
      projectRef: target.address,
    };
    const created = await createChannel(transportInput).catch(
      (error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        if (!/invalid channel_type/i.test(message)) throw error;
        return createChannel({ ...transportInput, channelType: "stream" });
      },
    );
    if (target.owner === (selfPubkey ?? "")) {
      try {
        await addProjectMembers(target, { channelIds: [created.id] });
      } catch {
        // The channel's own projectRef back-reference still binds it; the
        // owner-curated forward ref is best-effort.
      }
    }
    void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
    void queryClient.invalidateQueries({
      queryKey: projectContainersQueryKey,
    });
    return created.id;
  }, [createChannel, project, queryClient, resolvedChannel, selfPubkey]);

  const projectContext = React.useMemo<NewCodingSessionProjectContext | null>(
    () =>
      project
        ? {
            projectId: project.id,
            projectName: project.name,
            // The local General placeholder has no published head yet, so it
            // has no coordinate to sign; `ensureChannelId` publishes it, and
            // the placement falls back to the channel's project until then.
            projectRef:
              project.id === LOCAL_GENERAL_ID ? null : project.address,
            channelId: resolvedChannel?.channelId ?? null,
            defaultWorkdir: repoCheckout,
            ensureChannelId,
          }
        : null,
    [ensureChannelId, project, repoCheckout, resolvedChannel],
  );

  if (!projectContext) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-sm text-muted-foreground">Project not found.</p>
      </div>
    );
  }
  return <NewCodingSessionScreen projectContext={projectContext} />;
}
