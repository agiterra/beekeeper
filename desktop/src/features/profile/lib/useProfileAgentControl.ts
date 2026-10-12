import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo } from "react";
import { managedAgentHostStatusQueryKey } from "@/features/agents/hooks";
import { useManagedAgentRuntimesQuery } from "@/features/agents/managedAgentRuntimeHooks";
import { canonicalRelayUrl } from "@/features/agents/managedAgentRuntimeStatus";
import { useCommunitiesIfPresent } from "@/features/communities/useCommunities";
import {
  loadActiveCommunityId,
  loadCommunities,
} from "@/features/communities/communityStorage";
import { getManagedAgentHostStatus } from "@/shared/api/tauriManagedAgents";
import type { ManagedAgent } from "@/shared/api/types";
import { profileAgentControl } from "./profileAgentControl";

/** Observe canonical host status for the selected pair, without claiming readiness. */
export function useProfileAgentControl(
  agent: ManagedAgent | undefined,
  pending: boolean,
) {
  const communities = useCommunitiesIfPresent();
  const activeRelay = communities
    ? communities.activeCommunity?.relayUrl
    : loadCommunities().find((row) => row.id === loadActiveCommunityId())
        ?.relayUrl;
  // Empty stored relay is the legacy convention for the selected community.
  // Resolve from established client context, never from a getter response.
  const pairRelay = canonicalRelayUrl(
    agent?.relayUrl?.trim() || activeRelay || "",
  );
  const enabled = agent?.backend.type === "local";
  const ownership = useQuery({
    queryKey: [
      ...managedAgentHostStatusQueryKey(agent?.pubkey ?? ""),
      "local",
      pairRelay,
    ],
    // Local ownership remains observable while the relay is offline;
    // this response does not certify a current desired relay revision.
    queryFn: () =>
      getManagedAgentHostStatus(agent?.pubkey ?? "", { localOnly: true }),
    enabled,
    refetchInterval: enabled ? 3000 : false,
  });
  const runtimes = useManagedAgentRuntimesQuery({ enabled });
  const refreshOwnership = ownership.refetch;
  const refreshRuntimes = runtimes.refetch;
  useEffect(() => {
    if (!enabled || pending) return;
    void refreshOwnership();
    void refreshRuntimes();
  }, [enabled, pending, refreshOwnership, refreshRuntimes]);
  useEffect(() => {
    if (!ownership.data?.ownershipPhase) return;
    void refreshRuntimes();
  }, [ownership.data, refreshRuntimes]);
  const sameCommunity =
    pairRelay !== null &&
    canonicalRelayUrl(ownership.data?.relayUrl ?? "") === pairRelay;
  const status =
    ownership.isError || ownership.isPending || !sameCommunity
      ? undefined
      : ownership.data;
  const runtime = runtimes.data?.find(
    (row) =>
      row.pubkey === agent?.pubkey &&
      pairRelay !== null &&
      canonicalRelayUrl(row.relayUrl) === pairRelay,
  );
  return useMemo(
    () => (agent ? profileAgentControl(agent, status, runtime) : undefined),
    [agent, status, runtime],
  );
}
