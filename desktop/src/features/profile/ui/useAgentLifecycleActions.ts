import * as React from "react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { managedAgentHostStatusQueryKey } from "@/features/agents/hooks";

import {
  isManagedAgentActive,
  respawnManagedAgentWithRules,
  startManagedAgentWithRules,
  stopManagedAgentWithRules,
} from "@/features/agents/lib/managedAgentControlActions";
import { clearActiveTurnsForAgentOnStop } from "@/features/agents/managedAgentRuntimeHooks";
import type { Channel, ManagedAgent, RelayAgent } from "@/shared/api/types";
import type { ProfileAgentControl } from "../lib/profileAgentControl";

export function useAgentLifecycleActions({
  channels,
  managedAgent,
  relayAgents,
  startManagedAgent,
  stopManagedAgent,
  control,
}: {
  channels: readonly Channel[] | undefined;
  managedAgent: ManagedAgent | undefined;
  relayAgents: readonly RelayAgent[] | undefined;
  startManagedAgent: (pubkey: string) => Promise<unknown>;
  stopManagedAgent: (pubkey: string) => Promise<unknown>;
  control?: ProfileAgentControl;
}) {
  const queryClient = useQueryClient();
  const handleAgentPrimaryAction = React.useCallback(async () => {
    if (!managedAgent) return;

    try {
      if (control?.action === null)
        throw new Error(
          "Ownership transition is unconfirmed; refresh before controlling this agent.",
        );
      if (
        control ? control.action === "stop" : isManagedAgentActive(managedAgent)
      ) {
        const result = await stopManagedAgentWithRules({
          agent: managedAgent,
          channels: channels ?? [],
          relayAgents: relayAgents ?? [],
          stopManagedAgent,
        });
        if (managedAgent.backend.type === "local") {
          clearActiveTurnsForAgentOnStop(managedAgent.pubkey);
        }
        toast.success(result.noticeMessage ?? `Stopped ${managedAgent.name}.`);
        return;
      }

      await startManagedAgentWithRules({
        agent: managedAgent,
        startManagedAgent,
      });
      toast.success(
        managedAgent.backend.type === "provider"
          ? `Deploying ${managedAgent.name}.`
          : `Started ${managedAgent.name}.`,
      );
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Agent action failed.",
      );
    } finally {
      await queryClient.invalidateQueries({
        queryKey: managedAgentHostStatusQueryKey(managedAgent.pubkey),
      });
    }
  }, [
    channels,
    managedAgent,
    relayAgents,
    startManagedAgent,
    stopManagedAgent,
    control,
    queryClient,
  ]);

  const handleAgentRestart = React.useCallback(async () => {
    if (!managedAgent) return;

    try {
      if (control && !control.canRestart)
        throw new Error("Ownership transition does not permit restart.");
      await respawnManagedAgentWithRules({
        agent: managedAgent,
        startManagedAgent,
        stopManagedAgent,
        onStopped: () => clearActiveTurnsForAgentOnStop(managedAgent.pubkey),
      });
      toast.success(`Restarted ${managedAgent.name}.`);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Agent restart failed.",
      );
    } finally {
      await queryClient.invalidateQueries({
        queryKey: managedAgentHostStatusQueryKey(managedAgent.pubkey),
      });
    }
  }, [managedAgent, startManagedAgent, stopManagedAgent, control, queryClient]);

  return { handleAgentPrimaryAction, handleAgentRestart };
}
