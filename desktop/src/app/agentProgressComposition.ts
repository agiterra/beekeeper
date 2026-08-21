/**
 * App-level composition for the global Agent Progress route.
 *
 * The feature itself is a pure coordination adapter and presentation surface.
 * Joining it to the channels and project-session shelves belongs here, where
 * sibling features are allowed to meet. Keeping that wiring out of
 * `features/agent-progress` prevents the panel from reaching through another
 * feature's private model.
 */
import * as React from "react";

import { useChannelsQuery } from "@/features/channels/hooks";
import {
  useAgentProgressCoordination,
  type AgentProgressReadError,
} from "@/features/agent-progress/lib/agentProgressCoordination";
import {
  foldAgentProgress,
  type AgentProgressModel,
} from "@/features/agent-progress/lib/agentProgressFold";
import { agentProgressDetailBySessionRef } from "@/features/agent-progress/lib/agentProgressSources";
import { useProjectCodingSessionBuckets } from "@/features/projects-container/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";

const NO_PROJECT_BUCKETS: ReadonlyMap<string, never[]> = new Map();

/** What the panel knows, and how much of it is a floor rather than a census. */
export type AgentProgressState = AgentProgressModel & {
  /** True until the first coordination read resolves. */
  isLoading: boolean;
  /** True when every source query answered in full. */
  complete: boolean;
  /** Why the read is incomplete, in the words of the source that failed. */
  errors: AgentProgressReadError[];
  /** Evidence the coordination fold refused to resolve. */
  ambiguities: { scope: string; message: string }[];
  asOfSeconds: number;
};

/** Compose the global panel from coordination truth plus local shelf detail. */
export function useAgentProgress(): AgentProgressState {
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const channelIds = React.useMemo(
    () =>
      (channelsQuery.data ?? [])
        .filter(
          (channel) => channel.isMember || isSessionTransportChannel(channel),
        )
        .map((channel) => channel.id)
        .sort(),
    [channelsQuery.data],
  );
  const channelsUnresolved =
    channelsQuery.isPending ||
    channelsQuery.isError ||
    channelsQuery.isFetching;

  const coordination = useAgentProgressCoordination(
    channelIds,
    channelsUnresolved,
  );
  const buckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    NO_PROJECT_BUCKETS,
    NO_PROJECT_BUCKETS,
  );
  const detailBySessionRef = React.useMemo(
    () =>
      agentProgressDetailBySessionRef([
        ...[...buckets.byProject.values()].flat(),
        ...buckets.unclaimed,
      ]),
    [buckets.byProject, buckets.unclaimed],
  );

  const read = coordination.read;
  const model = React.useMemo(
    () =>
      foldAgentProgress({
        sessions: read?.sessions ?? [],
        channelsBySession: read?.channelsBySession ?? new Map(),
        detailBySessionRef,
        nowSeconds: read?.asOf ?? Math.floor(Date.now() / 1_000),
        complete: read?.complete ?? false,
        ambiguities: read?.ambiguities ?? [],
      }),
    [detailBySessionRef, read],
  );

  return {
    ...model,
    isLoading: coordination.isPending,
    complete: read?.complete ?? false,
    errors: read?.errors ?? [],
    ambiguities: read?.ambiguities ?? [],
    asOfSeconds: read?.asOf ?? Math.floor(Date.now() / 1_000),
  };
}
