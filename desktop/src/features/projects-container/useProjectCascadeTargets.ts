import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useChannelsQuery } from "@/features/channels/hooks";
import { useProjectsQuery } from "@/features/projects/hooks";
import { allWorkflowsQueryKey } from "@/features/workflows/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import {
  planProjectAgentTeardown,
  type ProjectAgentTeardownPlan,
} from "@/shared/api/projectAgentTeardown";
import { relayClient } from "@/shared/api/relayClient";
import { KIND_SHELL_SESSION } from "@/shared/constants/kinds";
import { getChannelsWorkflows } from "@/shared/api/tauriWorkflows";
import { useFeatureEnabled } from "@/shared/features";

import type { ProjectContainer } from "./hooks";
import { useProjectCapabilities } from "./lib/projectPermissions";
import {
  cascadeTerminalsFromEvents,
  describeProjectCascade,
  describeProjectCascadeRepos,
  projectCascadeCounts,
  projectCascadeChannels,
  projectCascadeExclusionNotes,
  projectCascadeRepos,
  projectCascadeWorkflows,
  type ProjectCascadeCounts,
  type ProjectCascadeTargets,
} from "./lib/projectCascade";

const EMPTY_TARGETS: ProjectCascadeTargets = {
  channels: [],
  workflows: [],
  foreignWorkflows: [],
  terminals: [],
  repos: [],
  foreignRepos: [],
};

export type ProjectCascadeTargetsResult = {
  targets: ProjectCascadeTargets;
  counts: ProjectCascadeCounts;
  /** Human summary for the dialog, `""` when there is nothing to delete. */
  summary: string;
  /** The repositories the repo tick would delete, by name; `""` when none. */
  repoSummary: string;
  /**
   * What this project created on this computer, or `null` when the host
   * could not enumerate it. `null` is a real answer and the dialog says so
   * rather than rendering "0 agents" over an unanswered question.
   */
  localPlan: ProjectAgentTeardownPlan | null;
  /** Sentences naming what the cascade will *not* do (foreign workflows,
   * un-enumerable workflows). Empty when it will do everything it claims. */
  exclusions: string[];
  /** True while either the channel or workflow list is still loading — the
   * dialog must not promise a count it has not finished computing, and must
   * not let the user act on one either. */
  isLoading: boolean;
};

/**
 * Enumerate what a "delete everything" project delete would remove.
 *
 * Channels come from the transport-inclusive channel list (a transport is a
 * real casualty of a project delete, so it must be counted); workflows are
 * channel-scoped, so they are fetched for exactly the project's channels and
 * then split by author — a kind:5 tombstone can only delete the signer's own
 * workflows, so a teammate's workflow is reported, never issued a delete that
 * the relay would accept while changing nothing.
 *
 * Pass `null` when no dialog is open to skip the workflow fetch entirely.
 */
export function useProjectCascadeTargets(
  project: ProjectContainer | null,
): ProjectCascadeTargetsResult {
  const channelsQuery = useChannelsQuery({
    enabled: project !== null,
    includeSessionTransports: true,
  });
  const workflowsEnabled = useFeatureEnabled("workflows");
  const identityQuery = useIdentityQuery();
  const selfPubkey = identityQuery.data?.pubkey ?? null;
  // Owner-ness decides whether a teammate's repository is deletable at all
  // (see `projectCascadeRepos`), so the repo tick cannot be classified
  // without it.
  const capabilities = useProjectCapabilities(project);
  // Every repository in the community, not just this project's forward refs:
  // a repository carries its project link in a multi-letter `project` tag,
  // which Nostr does not index, so there is no relay filter for "repos of
  // this project" and the back-referenced side can only be found by
  // filtering the full set here. This is the same enumeration the sidebar
  // already runs, so it is served from cache rather than fetched again.
  const projectsQuery = useProjectsQuery();
  const repositories = React.useMemo(
    () => (projectsQuery.data ?? []).flatMap((entry) => entry.repositories),
    [projectsQuery.data],
  );

  const channels = React.useMemo(
    () =>
      project ? projectCascadeChannels(project, channelsQuery.data ?? []) : [],
    [project, channelsQuery.data],
  );
  const channelIds = React.useMemo(
    () => channels.map((channel) => channel.id).sort(),
    [channels],
  );

  const workflowsQuery = useQuery({
    enabled: project !== null && workflowsEnabled && channelIds.length > 0,
    queryKey: allWorkflowsQueryKey(`cascade:${channelIds.join(",")}`),
    queryFn: () => getChannelsWorkflows(channelIds),
    staleTime: 30_000,
  });

  // Terminals are addressed by the project coordinate directly (the announce
  // carries it in a single-letter `a` tag), so unlike workflows they need no
  // channel set — a project with no channels can still have terminals.
  const terminalsQuery = useQuery({
    enabled: project !== null && (project?.owner.length ?? 0) > 0,
    queryKey: ["project-cascade-terminals", project?.address ?? "none"],
    queryFn: async () => {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_SHELL_SESSION],
        "#a": [project?.address ?? ""],
        limit: 500,
      });
      return cascadeTerminalsFromEvents(events);
    },
    staleTime: 30_000,
  });

  const targets = React.useMemo<ProjectCascadeTargets>(() => {
    if (!project) return EMPTY_TARGETS;
    const terminals = terminalsQuery.data ?? [];
    const repos = projectCascadeRepos(
      project,
      repositories,
      selfPubkey,
      capabilities.isOwner,
    );
    if (!workflowsEnabled) {
      return {
        channels,
        workflows: [],
        foreignWorkflows: [],
        terminals,
        repos: repos.mine,
        foreignRepos: repos.foreign,
      };
    }
    const { mine, foreign } = projectCascadeWorkflows(
      channelIds,
      workflowsQuery.data ?? [],
      selfPubkey,
    );
    return {
      channels,
      workflows: mine,
      foreignWorkflows: foreign,
      terminals,
      repos: repos.mine,
      foreignRepos: repos.foreign,
    };
  }, [
    project,
    channels,
    channelIds,
    workflowsEnabled,
    workflowsQuery.data,
    terminalsQuery.data,
    repositories,
    capabilities.isOwner,
    selfPubkey,
  ]);

  // The local side: agents, their keys, the project team and its definitions.
  // Read-only, and keyed on the project so it is re-read per project rather
  // than per render.
  const localQuery = useQuery({
    enabled: project !== null,
    queryKey: ["project-agent-teardown-plan", project?.address ?? "none"],
    queryFn: () => planProjectAgentTeardown(project?.address ?? ""),
    staleTime: 30_000,
  });

  const counts = React.useMemo(() => projectCascadeCounts(targets), [targets]);

  // Workflows exist in these channels but this session cannot enumerate them:
  // the feature is off in this build, the lookup failed, or the identity that
  // decides "mine vs theirs" has not resolved. Deleting the channels anyway
  // orphans whatever is in there, so the dialog has to say so rather than
  // silently render a cascade with no workflow line.
  const workflowsUnknown =
    project !== null &&
    channelIds.length > 0 &&
    (!workflowsEnabled || workflowsQuery.isError || selfPubkey === null);

  const isLoading =
    project !== null &&
    (channelsQuery.isLoading ||
      identityQuery.isLoading ||
      terminalsQuery.isLoading ||
      // The repository classification depends on both of these: an
      // unresolved roster reads as "not an owner" and would silently demote
      // every teammate repository to a survivor while the copy claimed a
      // complete inventory.
      projectsQuery.isLoading ||
      capabilities.isLoading ||
      localQuery.isLoading ||
      (workflowsEnabled && workflowsQuery.isLoading && channelIds.length > 0));

  return {
    targets,
    counts,
    summary: describeProjectCascade(counts),
    repoSummary: describeProjectCascadeRepos(targets.repos),
    localPlan: localQuery.data ?? null,
    exclusions: projectCascadeExclusionNotes(counts, workflowsUnknown),
    isLoading,
  };
}
