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
  getWorkflowAutorun,
  type ProjectWorkflowHostStep,
  type ProjectWorkflowRun,
  type WorkflowAutorun,
} from "@/shared/api/tauriWorkflows";
import type { Workflow, WorkflowApproval } from "@/shared/api/types";

import { isProjectAction } from "./actionDefinition";
import {
  readBoundDefinition,
  type BoundDefinitionRead,
} from "./resolveBoundDefinition";

/** How many of an action's latest runs the tab shows. */
export const PROJECT_ACTION_RUN_LIMIT = 5;

/** Re-read every 10 s while any shown run is not terminal. */
const LIVE_REFETCH_MS = 10_000;

const NO_CHANNEL_IDS: readonly string[] = [];

export type ProjectActionRun = {
  run: ProjectWorkflowRun;
  approvals: WorkflowApproval[];
  hostSteps: ProjectWorkflowHostStep[];
  /** The host-step read failed; the row says so instead of guessing. */
  hostStepsError: string | null;
};

export type ProjectAction = {
  workflow: Workflow;
  /**
   * The published definition and **its own** hash, from one read (R1).
   *
   * The card's command and the hash it is authorized against must come from
   * the same read; `workflow.definition` above is the list read and is used
   * for the name, trigger and host-step summary only — never to authorize a
   * grant.
   */
  boundDefinition: BoundDefinitionRead;
  runs: ProjectActionRun[];
  /** The run listing failed; the card says so instead of "No runs yet." */
  runsError: string | null;
  /** The workflow's autorun grants, or `null` when the read failed. */
  autorun: WorkflowAutorun | null;
  /** Why the autorun read failed, or `null`. */
  autorunError: string | null;
};

/**
 * Whether this reader could ask the question at all.
 *
 * Ledger 171(a): the tab rendered "No actions are published" for a project
 * whose action `bee workflows list` returned on the same channel, because the
 * channel set the query is built from was empty and an empty channel set
 * short-circuits to `[]`. An unasked question and an answered one must never
 * render the same, so the two are separate states here.
 */
export type ProjectActionsReadability =
  | { kind: "readable"; channelCount: number }
  | { kind: "no-readable-channel" }
  | { kind: "channels-unreadable"; error: string };

export type ProjectActionsState = {
  /** The project the route names, or `null` when this reader cannot read it. */
  project: ProjectContainer | null;
  actions: ProjectAction[];
  isLoading: boolean;
  /** The read that failed, in its own words. */
  error: string | null;
  /** Whether a relay read happened at all, and why not when it did not. */
  readability: ProjectActionsReadability;
  /** The channels this reader asked about, for the disclosure line. */
  channelIds: readonly string[];
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

function isLiveRun(run: ProjectWorkflowRun): boolean {
  return (
    run.status !== "completed" &&
    run.status !== "failed" &&
    run.status !== "cancelled"
  );
}

async function loadRun(
  workflowId: string,
  run: ProjectWorkflowRun,
): Promise<ProjectActionRun> {
  const [approvals, hostSteps] = await Promise.all([
    getRunApprovals(workflowId, run.id),
    getRunHostSteps(workflowId, run.id).then(
      (steps) => ({ steps, error: null as string | null }),
      (error: unknown) => ({
        steps: [] as ProjectWorkflowHostStep[],
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
  // Never `[]`. An empty channel set is a question nobody asked, and the
  // caller renders that as its own state (ledger 171(a)).
  if (channelIds.length === 0) {
    throw new Error(
      "no channel of this project is readable from here, so no action query was sent",
    );
  }
  const workflows = (await getChannelsWorkflows([...channelIds]))
    .filter((workflow) => isProjectAction(workflow.definition, projectRef))
    .sort((a, b) => a.name.localeCompare(b.name));
  return Promise.all(
    workflows.map(async (workflow) => {
      // One unreadable action's runs must not blank the whole tab, and must
      // not read as "No runs yet" either.
      let runs: Awaited<ReturnType<typeof getWorkflowRuns>> = [];
      let runsError: string | null = null;
      try {
        runs = await getWorkflowRuns(workflow.id, PROJECT_ACTION_RUN_LIMIT);
      } catch (error) {
        runsError = errorSentence(error);
      }
      const detailed = await Promise.all(
        runs.map((run) => loadRun(workflow.id, run)),
      );
      detailed.sort((a, b) => b.run.createdAt - a.run.createdAt);
      // R1: the definition an approval is judged against, with the hash of
      // those same bytes, in one read. Failure is kept as a sentence; a row
      // with no resolved definition offers no grant.
      const boundDefinition = await readBoundDefinition(workflow.id);
      // Spec § 5.4: whether an unrevoked autorun grant binds this exact
      // definition. A read failure is kept as a sentence, not hidden as
      // "no grant".
      let autorun: WorkflowAutorun | null = null;
      let autorunError: string | null = null;
      try {
        autorun = await getWorkflowAutorun(workflow.id);
      } catch (error) {
        autorunError = errorSentence(error);
      }
      return {
        workflow,
        boundDefinition,
        runs: detailed,
        runsError,
        autorun,
        autorunError,
      };
    }),
  );
}

/**
 * The channels the action query may read: the project's declared channels and
 * any channel that names the project, member-only.
 *
 * Pure so the regression this closes is testable. Ledger 171(a): for a
 * project whose action was published into its team session's channel this set
 * came back empty, because the caller read the channel list through
 * `useChannelsQuery()` — which hides `transport` channels by default — and an
 * empty set then short-circuited to "No actions are published".
 */
export function projectActionChannelIds(
  project: Pick<ProjectContainer, "address" | "channelIds"> | null,
  channels: readonly {
    id: string;
    isMember: boolean;
    projectRef?: string | null;
  }[] = [],
): readonly string[] {
  if (!project) return NO_CHANNEL_IDS;
  const declared = new Set(project.channelIds);
  return channels
    .filter(
      (channel) =>
        channel.isMember &&
        (declared.has(channel.id) || channel.projectRef === project.address),
    )
    .map((channel) => channel.id)
    .sort();
}

export function useProjectActions(projectId: string): ProjectActionsState {
  const projects = useDisplayProjectContainers();
  const project = React.useMemo(
    () => projects.find((candidate) => candidate.id === projectId) ?? null,
    [projects, projectId],
  );
  // `includeSessionTransports: true` is load-bearing (ledger 171(a)): a
  // project action is published into whichever channel `bee actions publish
  // --channel` named, and for a project with a team session that is the
  // session's hidden kind `transport` channel. `useChannelsQuery()` filters
  // those out of its returned list by default (`isSessionTransportChannel`),
  // so without this the project's channel set came back empty and the tab
  // answered "No actions are published" for an action that was on the relay.
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });

  // The channels the workflow query may read: the project's declared
  // channels and any channel that names the project, member-only.
  const channelIds = React.useMemo(
    () => projectActionChannelIds(project, channelsQuery.data ?? []),
    [channelsQuery.data, project],
  );
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

  const readability: ProjectActionsReadability = channelsQuery.error
    ? { kind: "channels-unreadable", error: errorSentence(channelsQuery.error) }
    : channelsQuery.data !== undefined && channelIds.length === 0
      ? { kind: "no-readable-channel" }
      : { kind: "readable", channelCount: channelIds.length };

  return {
    project,
    actions: query.data ?? [],
    isLoading: project !== null && (channelsQuery.isPending || query.isPending),
    error: query.error ? errorSentence(query.error) : null,
    readability,
    channelIds,
    refresh,
  };
}
