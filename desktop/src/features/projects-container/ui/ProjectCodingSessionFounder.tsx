import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { toast } from "sonner";

import {
  channelsQueryKey,
  useChannelsQuery,
  useCreateChannelMutation,
} from "@/features/channels/hooks";
import { matchProjectCwdRepo } from "@/features/builtin-shell/lib/projectShellCwd";
import { selectLaunchRepoRef } from "@/features/coding-sessions/lib/codingSessionLaunchRepoRef";
import {
  type NewCodingSessionWorkspaceReuse,
  workspaceReuseRepoRef,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import {
  clearCodingSessionFoundingRequest,
  markCodingSessionFoundingStarted,
} from "@/features/coding-sessions/newCodingSessionDialogStore";
import {
  type GoFoundedCodingSession,
  useCodingSessionFoundNow,
} from "@/features/coding-sessions/ui/CodingSessionFoundingHost";
import type { CodingSessionTopicFoundingHostDeps } from "@/features/coding-sessions/ui/useCodingSessionTopicFounding";
import { useProjectsQuery } from "@/features/projects/hooks";
import { listProjectLocalRepositories } from "@/shared/api/projectGit";
import { getCodingSessionWorkdirState } from "@/shared/api/tauriCodingSessionWorkdirs";
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
  projectSessionsChannelCandidates,
  projectSessionsChannelName,
  resolveProjectSessionsChannel,
} from "../lib/projectSessionsChannel";
import { addProjectMembers } from "../useCreateProjectContainer";
import { ensureRealProject } from "../useGeneralProjectMigration";

/** What this computer found of the project's repositories, once it has looked. */
export type ProjectCheckout = {
  /**
   * The repository list the read answered for, or null while it is in
   * flight. Identity, not a flag: a checkout that settled for an earlier list
   * (the empty one a still-loading containers query yields) must not count
   * as settled for the project that has since resolved — the founding
   * effect runs in the same commit as the re-read it would otherwise skip.
   */
  settledFor: readonly unknown[] | null;
  /** A local checkout of one of the project's repos: the workdir prefill. */
  path: string | null;
  /** LANE-L20: the repository this founding names, or null — never guessed. */
  repoRef: string | null;
};

/**
 * Whether the founder may found now: every read it depends on has answered,
 * and the checkout read answered for the repository list it will found with.
 */
export function projectCodingSessionFounderReady(input: {
  projectsLoading: boolean;
  reposLoading: boolean;
  channelsLoading: boolean;
  checkout: ProjectCheckout;
  projectRepos: readonly unknown[];
}): boolean {
  return (
    !input.projectsLoading &&
    !input.reposLoading &&
    !input.channelsLoading &&
    input.checkout.settledFor === input.projectRepos
  );
}

/**
 * Found a session in a project: the project answers the channel.
 *
 * Renders nothing. It supplies the two facts a project adds to a founding —
 * which channel the transcript lives in, and which repository checkout the
 * page should prefill — and knows how to bring that channel into existence
 * when the project has never had one. Everything else is
 * `useCodingSessionFoundNow`, the same hook a channel click uses.
 *
 * It founds only once it can do so honestly: after the project list and the
 * channel list have loaded (founding before the channels are known would
 * mint a second transport channel for a project that already has one), and
 * after the local-checkout read has settled, so the draft carries the
 * checkout the click resolved rather than a guess. A project that is not on
 * this community says so in a toast and clears the request.
 */
export function ProjectCodingSessionFounder({
  deps,
  goFoundedCodingSession,
  projectId,
  workspaceReuse = null,
  sourceRepoRef = null,
}: {
  /** Injected in tests; this computer's relay and keyring by default. */
  deps?: CodingSessionTopicFoundingHostDeps;
  goFoundedCodingSession: GoFoundedCodingSession;
  projectId: string;
  /**
   * A directory a "New session in this workspace" request reuses, forwarded
   * unchanged. Reusing a folder answers where the session runs, not which
   * project it belongs to, so it changes nothing about the channel.
   */
  workspaceReuse?: NewCodingSessionWorkspaceReuse | null;
  /** Repository named by the source session; null is unknown, never the first repo. */
  sourceRepoRef?: string | null;
}) {
  const {
    projects,
    reposByProject,
    unclaimedRepos,
    isLoading: projectsLoading,
  } = useProjectContainers();
  const reposQuery = useProjectsQuery();
  // Transports included: the resolver's rule 0 and the session buckets both
  // need the hidden transport channel this founder exists to find or create.
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
  // The workdir prefill and the repoRef, from this computer's registered
  // checkouts + repos-root scan. `selectLaunchRepoRef` is the one place the
  // repoRef decision is made; this effect only supplies it the same
  // local-checkout read the prefill needs. `settledFor` is what the founding
  // waits for, and it must name *this* `projectRepos`.
  const [checkout, setCheckout] = React.useState<ProjectCheckout>({
    settledFor: null,
    path: null,
    repoRef: null,
  });
  const projectAddress = project?.address ?? null;
  React.useEffect(() => {
    let cancelled = false;
    const onlyRepo =
      projectRepos.length === 1 ? projectRepos[0].repoAddress : null;
    setCheckout({ settledFor: null, path: null, repoRef: onlyRepo });
    // The folder this computer recorded for the project outranks any scan:
    // it is what the create cloned, Finish repository setup recorded, or the
    // person set. The repos-root scan only fills in when nothing is
    // recorded, and still supplies the repoRef.
    const recorded = projectAddress
      ? getCodingSessionWorkdirState()
          .then((state) => state.byProject[projectAddress]?.path ?? null)
          .catch(() => null)
      : Promise.resolve(null);
    const scanned =
      projectRepos.length === 0
        ? Promise.resolve(null)
        : listProjectLocalRepositories({}).catch(() => null);
    void Promise.all([recorded, scanned]).then(([recordedPath, localRepos]) => {
      if (cancelled) return;
      if (localRepos === null) {
        // Non-Tauri preview or command unavailable: the scan is unresolved
        // and the repoRef is the synchronous single-repo case.
        setCheckout({
          settledFor: projectRepos,
          path: recordedPath,
          repoRef: onlyRepo,
        });
        return;
      }
      const matched = matchProjectCwdRepo(projectRepos, localRepos);
      setCheckout({
        settledFor: projectRepos,
        path: recordedPath ?? matched?.path ?? null,
        repoRef: selectLaunchRepoRef({ repos: projectRepos, localRepos }),
      });
    });
    return () => {
      cancelled = true;
    };
  }, [projectAddress, projectRepos]);

  const channelBuckets = React.useMemo(
    () => partitionChannels(projects, channelsQuery.data ?? []),
    [projects, channelsQuery.data],
  );
  const sessionBuckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    channelBuckets.channelsByProject,
    channelBuckets.forumsByProject,
    channelBuckets.transportsByProject,
  );

  // Every channel the project claims, transports included: the sidebar
  // partition above keeps only stream and forum channels, and reading the
  // candidates from it made rule 0 unreachable — every create minted a new
  // transport (2026-09-08).
  const projectChannels = React.useMemo(
    () =>
      project
        ? projectSessionsChannelCandidates(project, channelsQuery.data ?? [])
        : [],
    [channelsQuery.data, project],
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
    // Channels must belong to a project, so a session founded inside the
    // local General placeholder publishes the real General first.
    const target = await ensureRealProject(project);
    // A hidden transport channel: identified by type (never surfaced to
    // people), and the relay admits project members through the project ACL
    // instead of explicit channel membership. Private: the transport gate is
    // the read model; an open channel would advertise it community-wide. A
    // relay that predates the transport type rejects the create — fall back
    // to the legacy private stream (name-keyed, creator-only) so founding
    // never breaks against an old relay.
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

  const { foundNow } = useCodingSessionFoundNow({
    ensureChannelId,
    goFoundedCodingSession,
    deps,
  });

  const ready = projectCodingSessionFounderReady({
    projectsLoading,
    reposLoading: reposQuery.isLoading,
    channelsLoading: channelsQuery.isLoading,
    checkout,
    projectRepos,
  });
  React.useEffect(() => {
    if (!ready) return;
    if (project === null) {
      // The same claim as a founding, so the sentence is said once even when
      // StrictMode runs this effect twice.
      if (!markCodingSessionFoundingStarted()) return;
      clearCodingSessionFoundingRequest();
      toast.error("This project is no longer available.");
      return;
    }
    void foundNow({
      channelId: resolvedChannel?.channelId ?? null,
      // The local General placeholder has no published head yet, so it has
      // no coordinate to keep; `ensureChannelId` publishes it.
      projectRef: project.id === LOCAL_GENERAL_ID ? null : project.address,
      repoRef: workspaceReuseRepoRef({
        contextual: workspaceReuse !== null,
        sourceRepoRef,
        projectRepoRef: checkout.repoRef,
      }),
      workspace: workspaceReuse,
      defaultWorkdir: checkout.path,
    });
    // No cleanup on purpose: see `useCodingSessionFoundNow`.
  }, [
    checkout,
    foundNow,
    project,
    ready,
    resolvedChannel,
    sourceRepoRef,
    workspaceReuse,
  ]);

  return null;
}
