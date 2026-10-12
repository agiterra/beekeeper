import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  managedAgentsQueryKey,
  managedAgentHostTransferQueryKey,
} from "@/features/agents/hooks";
import { canonicalRelayUrl } from "@/features/agents/managedAgentRuntimeStatus";
import { useCommunitiesIfPresent } from "@/features/communities/useCommunities";
import {
  loadActiveCommunityId,
  loadCommunities,
} from "@/features/communities/communityStorage";
import {
  getManagedAgentHostTransfer,
  startManagedAgent,
  type ManagedAgentHostTransferInfo,
} from "@/shared/api/tauriManagedAgents";
import { Button } from "@/shared/ui/button";

const descriptions: Record<string, string> = {
  prepared: "Custody preserved. Legacy shutdown has not been acknowledged.",
  quiesced: "Legacy execution stopped. Host adoption remains unconfirmed.",
  adopted:
    "Host accepted this identity and execution. Availability comes from relay activity.",
  hostStopped:
    "The host execution stopped. Start opens a new context with the same identity.",
  restoring:
    "Host admissions are fenced. Migration is blocked; reconcile the recorded host responsibilities.",
  legacyRestored:
    "Preserved custody requires host migration before execution can start.",
};

export type HostTransferPhase = NonNullable<
  ManagedAgentHostTransferInfo["transfer"]
>["phase"];

export function HostTransferPanel({
  pubkey,
  onPhaseChange,
  managementPending = false,
}: {
  pubkey: string;
  onPhaseChange?: (phase: HostTransferPhase | null | undefined) => void;
  managementPending?: boolean;
}) {
  const queryClient = useQueryClient();
  const communities = useCommunitiesIfPresent();
  const activeRelay = communities
    ? communities.activeCommunity?.relayUrl
    : loadCommunities().find((row) => row.id === loadActiveCommunityId())
        ?.relayUrl;
  const activePair = canonicalRelayUrl(activeRelay ?? "");
  const transferQuery = useQuery({
    queryKey: [...managedAgentHostTransferQueryKey(pubkey), "full", activePair],
    queryFn: () => getManagedAgentHostTransfer(pubkey),
    enabled: !managementPending,
    retry: false,
  });
  const refetchTransfer = transferQuery.refetch;
  const observed = useRef<string | null>(null);
  // The query owns this observation: a new community key or failed refresh
  // cannot retain a previous community's mutable controls.
  const info =
    !transferQuery.isError &&
    activePair !== null &&
    canonicalRelayUrl(transferQuery.data?.relayUrl ?? "") === activePair
      ? (transferQuery.data ?? null)
      : null;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const applyObserved = useCallback(
    (value: ManagedAgentHostTransferInfo, explicitlyRefreshed = false) => {
      const identity = JSON.stringify([
        pubkey,
        value.transfer?.phase,
        value.transfer?.operationId,
        value.revision,
      ]);
      const changed =
        observed.current !== null && observed.current !== identity;
      observed.current = identity;
      onPhaseChange?.(value.transfer?.phase ?? null);
      if (changed || explicitlyRefreshed)
        void queryClient.invalidateQueries({ queryKey: managedAgentsQueryKey });
    },
    [pubkey, onPhaseChange, queryClient],
  );
  const refresh = useCallback(async () => {
    const { data: value } = await refetchTransfer({ throwOnError: true });
    if (!value) throw new Error("ownership observation unavailable");
    if (activePair === null || canonicalRelayUrl(value.relayUrl) !== activePair)
      throw new Error("ownership observation belongs to another community");
    applyObserved(value, true);
    return value;
  }, [refetchTransfer, applyObserved, activePair]);
  useEffect(() => {
    onPhaseChange?.(undefined);
    if (managementPending) return;
    void refetchTransfer();
  }, [onPhaseChange, managementPending, refetchTransfer]);
  useEffect(() => {
    if (info && !managementPending) applyObserved(info);
    else onPhaseChange?.(undefined);
  }, [info, managementPending, applyObserved, onPhaseChange]);
  useEffect(() => {
    if (transferQuery.error) setError(String(transferQuery.error));
  }, [transferQuery.error]);

  async function move() {
    if (!info?.migrationEligible) return;
    setBusy(true);
    setError(null);
    try {
      await startManagedAgent(pubkey, {
        expectedRelayUrl: info.relayUrl,
      });
      await refresh();
    } catch (reason) {
      setError(String(reason));
      await refresh().catch(() => {});
    } finally {
      setBusy(false);
      void queryClient.invalidateQueries({ queryKey: managedAgentsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: managedAgentHostTransferQueryKey(pubkey),
      });
    }
  }
  const phase = info?.transfer?.phase;
  return (
    <section
      className="flex flex-col gap-3 rounded-lg border p-3"
      data-testid="agent-host-transfer"
      data-operation-id={info?.transfer?.operationId}
      data-observed-phase={
        info ? (info.transfer?.phase ?? "legacy") : "unknown"
      }
    >
      <Button
        variant="outline"
        size="sm"
        onClick={() => setExpanded((value) => !value)}
      >
        {phase && phase !== "legacyRestored"
          ? "Migration status"
          : "Retry migration"}
      </Button>
      {expanded ? (
        <>
          <p className="text-sm text-muted-foreground">
            The host keeps this agent running when Beekeeper is closed. Existing
            credentials and settings stay in custody. Migration preserves
            existing native context only after verification. If that context
            cannot be verified, migration is blocked for repair; it does not
            create a fresh context as a fallback.
          </p>
          {phase ? (
            <p className="text-sm" data-testid="agent-host-transfer-phase">
              {descriptions[phase]}
            </p>
          ) : null}
          {!phase ||
          ["prepared", "quiesced", "legacyRestored"].includes(phase) ? (
            <Button
              data-testid="agent-host-transfer-move"
              onClick={() => void move()}
              disabled={busy || managementPending || !info?.migrationEligible}
            >
              {busy
                ? "Reconciling…"
                : phase
                  ? "Continue migration"
                  : "Retry migration"}
            </Button>
          ) : null}
          {phase === "hostStopped" ? (
            <Button
              disabled={busy || managementPending}
              data-testid="agent-host-transfer-start"
              onClick={() => {
                if (!info) return;
                setBusy(true);
                setError(null);
                void startManagedAgent(pubkey, {
                  expectedRelayUrl: info.relayUrl,
                })
                  .then(() => refresh())
                  .catch((reason) => setError(String(reason)))
                  .finally(() => {
                    setBusy(false);
                    void queryClient.invalidateQueries({
                      queryKey: managedAgentsQueryKey,
                    });
                    void queryClient.invalidateQueries({
                      queryKey: managedAgentHostTransferQueryKey(pubkey),
                    });
                  });
              }}
            >
              Start a new host context
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="sm"
            disabled={busy || managementPending}
            onClick={() =>
              void refresh().catch((reason) => setError(String(reason)))
            }
          >
            Refresh observed state
          </Button>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
