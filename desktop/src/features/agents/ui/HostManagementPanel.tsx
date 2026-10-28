import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  managedAgentsQueryKey,
  managedAgentHostStatusQueryKey,
} from "@/features/agents/hooks";
import {
  canonicalRelayUrl,
  findManagedAgentRuntime,
} from "@/features/agents/managedAgentRuntimeStatus";
import { useManagedAgentRuntimesQuery } from "@/features/agents/managedAgentRuntimeHooks";
import { useCommunitiesIfPresent } from "@/features/communities/useCommunities";
import {
  loadActiveCommunityId,
  loadCommunities,
} from "@/features/communities/communityStorage";
import {
  getManagedAgentHostStatus,
  startManagedAgent,
  type ManagedAgentHostStatus,
} from "@/shared/api/tauriManagedAgents";
import { Button } from "@/shared/ui/button";
import { AgentCommunityStatusPill } from "./AgentCommunityStatusPill";

export function HostManagementPanel({
  pubkey,
  onStatusChange,
  managementPending = false,
}: {
  pubkey: string;
  onStatusChange?: (status: ManagedAgentHostStatus | undefined) => void;
  managementPending?: boolean;
}) {
  const client = useQueryClient();
  const communities = useCommunitiesIfPresent();
  const relay = communities
    ? communities.activeCommunity?.relayUrl
    : loadCommunities().find((row) => row.id === loadActiveCommunityId())
        ?.relayUrl;
  const pair = canonicalRelayUrl(relay ?? "");
  const query = useQuery({
    queryKey: [...managedAgentHostStatusQueryKey(pubkey), "full", pair],
    queryFn: () => getManagedAgentHostStatus(pubkey),
    enabled: !managementPending,
    retry: false,
  });
  const info =
    !query.isError &&
    pair !== null &&
    canonicalRelayUrl(query.data?.relayUrl ?? "") === pair
      ? query.data
      : undefined;
  // The pair's runtime row carries what the ownership read does not: the
  // host's presence verdict and the folded execution word. With it the panel
  // can say "idle", "host unreachable" or "not running" instead of the one
  // sentence that covered all three; without it, that sentence stays. Read
  // only once ownership is adopted — the one phase that shows the pill — so
  // a legacy or unconfirmed panel dispatches nothing but the ownership read.
  const runtimesQuery = useManagedAgentRuntimesQuery({
    enabled: !managementPending && info?.ownershipPhase === "adopted",
  });
  const pairRuntime =
    relay && runtimesQuery.data
      ? findManagedAgentRuntime(runtimesQuery.data, pubkey, relay)
      : undefined;
  const [error, setError] = useState<string | null>(null);
  const [retryPending, setRetryPending] = useState(false);
  const previous = useRef<string | null>(null);
  const observed = useCallback(
    (status: ManagedAgentHostStatus) => {
      const identity = JSON.stringify([
        pubkey,
        status.ownershipPhase,
        status.revision,
        status.legacyRecreationRequired,
        status.ownershipPendingReason,
        status.freshCreationPending,
      ]);
      if (previous.current !== null && previous.current !== identity)
        void client.invalidateQueries({ queryKey: managedAgentsQueryKey });
      previous.current = identity;
      onStatusChange?.(status);
    },
    [pubkey, client, onStatusChange],
  );
  // The parent's controls follow this report: unknown while an action is
  // pending, otherwise the last real read. Re-reading must not clear it: an
  // identical answer keeps the same `data` reference (structural sharing), so
  // nothing re-reported it and Start/Stop/Restart stayed disabled under a
  // "Start" label (ledger 403). Every action re-validates on the backend.
  useEffect(() => {
    if (info && !managementPending) observed(info);
    else onStatusChange?.(undefined);
  }, [info, managementPending, observed, onStatusChange]);
  const refetch = query.refetch;
  useEffect(() => {
    if (!managementPending) void refetch();
  }, [managementPending, refetch]);
  async function refresh() {
    setError(null);
    try {
      const result = await refetch({ throwOnError: true });
      if (
        !result.data ||
        pair === null ||
        canonicalRelayUrl(result.data.relayUrl) !== pair
      )
        throw new Error("Host observation unavailable for this community");
      observed(result.data);
      void client.invalidateQueries({ queryKey: managedAgentsQueryKey });
    } catch (reason) {
      setError(String(reason));
      onStatusChange?.(undefined);
    }
  }
  async function retryFreshCreate() {
    if (!info?.freshCreationPending || info.legacyConflict) return;
    setRetryPending(true);
    setError(null);
    try {
      await startManagedAgent(pubkey, { expectedRelayUrl: info.relayUrl });
      await client.invalidateQueries({ queryKey: managedAgentsQueryKey });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRetryPending(false);
    }
  }
  return (
    <section
      className="flex flex-col gap-3 rounded-lg border p-3"
      data-testid="agent-host-management"
      data-observed-phase={info?.ownershipPhase ?? "unknown"}
    >
      <h3 className="font-medium">Host management</h3>
      {info?.legacyRecreationRequired ? (
        <p data-testid="agent-clean-start-notice" className="text-sm">
          This agent belongs to the previous desktop setup. Recreate it through
          headless agent setup. Existing history, settings and credentials
          remain retained for reference; they are not automatically imported or
          run.
        </p>
      ) : (
        <p className="text-sm" data-testid="agent-host-status">
          {info?.ownershipPhase === "adopted"
            ? "The host owns this agent and continues independently of this window."
            : info?.ownershipPhase === "hostStopped"
              ? "Host work is stopped. Start opens a new context with the same identity."
              : "Host ownership is unconfirmed. Refresh before controlling this agent."}
        </p>
      )}
      {info?.ownershipPendingReason ? (
        <p role="status" className="text-sm">
          {info.ownershipPendingReason}
        </p>
      ) : null}
      {info?.freshCreationPending && !info.legacyConflict ? (
        <Button
          size="sm"
          disabled={managementPending || retryPending || query.isFetching}
          onClick={() => void retryFreshCreate()}
        >
          Retry fresh Create
        </Button>
      ) : null}
      {info?.legacyConflict ? (
        <p role="alert" className="text-sm text-destructive">
          {info.legacyConflict}
        </p>
      ) : null}
      {info?.ownershipPhase === "adopted" ? (
        pairRuntime?.lifecycle === "host_managed" ? (
          <AgentCommunityStatusPill
            runtime={pairRuntime}
            showDetail
            testId="agent-host-availability"
          />
        ) : (
          <p className="text-sm text-muted-foreground">
            {info.runtimeAvailable === true
              ? "Runtime availability observed through relay work."
              : "Runtime availability is unconfirmed."}
          </p>
        )
      ) : null}
      <Button
        variant="ghost"
        size="sm"
        disabled={managementPending || query.isFetching}
        onClick={() => void refresh()}
      >
        Refresh host status
      </Button>
      {error || query.error ? (
        <p role="alert" className="text-sm text-destructive">
          {error ?? String(query.error)}
        </p>
      ) : null}
    </section>
  );
}
