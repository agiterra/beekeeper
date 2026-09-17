import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useChannelsQuery } from "@/features/channels/hooks";
import {
  type ProjectContainer,
  useDisplayProjectContainers,
} from "@/features/projects-container/hooks";
import {
  getChannelsWorkflows,
  getRunApprovals,
  getRunHostSteps,
  getWorkflowRuns,
} from "@/shared/api/tauriWorkflows";
import type {
  Workflow,
  WorkflowApproval,
  WorkflowRun,
} from "@/shared/api/types";
import type { WorkflowHostStep } from "@/shared/api/workflowTypes";

import { isProjectAction } from "./actionDefinition";

/** How many of an action's latest runs the tab shows. */
export const PROJECT_ACTION_RUN_LIMIT = 5;

/** Re-read every 10 s while any shown run is not terminal. */
const LIVE_REFETCH_MS = 10_000;

const NO_CHANNEL_IDS: readonly string[] = [];

export type ProjectActionRun = {
  run: WorkflowRun;
  approvals: WorkflowApproval[];
  hostSteps: WorkflowHostStep[];
  /** The host-step read failed; the row says so instead of guessing. */
  hostStepsError: string | null;
};

export type ProjectAction = {
  workflow: Workflow;
  runs: ProjectActionRun[];
};

export type ProjectActionsState = {
  /** The project the route names, or `null` when this reader cannot read it. */
  project: ProjectContainer | null;
  actions: ProjectAction[];
  isLoading: boolean;
  /** The read that failed, in its own words. */
  error: string | null;
  /** Re-read after a Run, Approve or Deny. */
  refresh: () => void;
};

export function projectActionsQueryKey(
  projectRef: string,
  channelIdKey: string,
) {
  return ["project-actions", projectRef, channelIdKey] as const;
}

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function isLiveRun(run: WorkflowRun): boolean {
  return (
    run.status !== "completed" &&
    run.status !== "failed" &&
    run.status !== "cancelled"
  );
}

async function loadRun(
  workflowId: string,
  run: WorkflowRun,
): Promise<ProjectActionRun> {
  const [approvals, hostSteps] = await Promise.all([
    getRunApprovals(workflowId, run.id),
    getRunHostSteps(workflowId, run.id).then(
      (steps) => ({ steps, error: null as string | null }),
      (error: unknown) => ({
        steps: [] as WorkflowHostStep[],
        error: errorSentence(error),
      }),
    ),
  ]);
  return {
    run,
    approvals,
    hostSteps: hostSteps.steps,
    hostStepsError: hostSteps.error,
  };
}

/**
 * The project's actions: every kind:30620 in its channels whose definition
 * names this project, with each one's latest runs and, per run, the
 * approvals and host steps the relay recorded.
 */
export async function loadProjectActions(
  projectRef: string,
  channelIds: readonly string[],
): Promise<ProjectAction[]> {
  if (channelIds.length === 0) return [];
  const workflows = (await getChannelsWorkflows([...channelIds]))
    .filter((workflow) => isProjectAction(workflow.definition, projectRef))
    .sort((a, b) => a.name.localeCompare(b.name));
  return Promise.all(
    workflows.map(async (workflow) => {
      const runs = await getWorkflowRuns(workflow.id, PROJECT_ACTION_RUN_LIMIT);
      const detailed = await Promise.all(
        runs.map((run) => loadRun(workflow.id, run)),
      );
      detailed.sort((a, b) => b.run.createdAt - a.run.createdAt);
      return { workflow, runs: detailed };
    }),
  );
}

export function useProjectActions(projectId: string): ProjectActionsState {
  const projects = useDisplayProjectContainers();
  const project = React.useMemo(
    () => projects.find((candidate) => candidate.id === projectId) ?? null,
    [projects, projectId],
  );
  const channelsQuery = useChannelsQuery();

  // The channels the workflow query may read: the project's declared
  // channels and any channel that names the project, member-only.
  const channelIds = React.useMemo(() => {
    if (!project) return NO_CHANNEL_IDS;
    const declared = new Set(project.channelIds);
    return (channelsQuery.data ?? [])
      .filter(
        (channel) =>
          channel.isMember &&
          (declared.has(channel.id) || channel.projectRef === project.address),
      )
      .map((channel) => channel.id)
      .sort();
  }, [channelsQuery.data, project]);
  const channelIdKey = channelIds.join(",");
  const projectRef = project?.address ?? "";
  const queryKey = projectActionsQueryKey(projectRef, channelIdKey);

  const query = useQuery({
    queryKey,
    enabled: project !== null && channelsQuery.data !== undefined,
    queryFn: () => loadProjectActions(projectRef, channelIds),
    refetchInterval: (state) =>
      state.state.data?.some((action) =>
        action.runs.some((entry) => isLiveRun(entry.run)),
      )
        ? LIVE_REFETCH_MS
        : false,
  });

  const queryClient = useQueryClient();
  const refresh = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey });
  }, [queryClient, queryKey]);

  return {
    project,
    actions: query.data ?? [],
    isLoading: project !== null && (channelsQuery.isPending || query.isPending),
    error: query.error ? errorSentence(query.error) : null,
    refresh,
  };
}
