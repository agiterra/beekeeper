import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { Bot } from "lucide-react";

import { getCodingSessionProviderStatus } from "@/shared/api/tauriSessionProvider";

import { CodingSessionAgentsOrchestration } from "../CodingSessionAgentsOrchestration";
import {
  CodingSessionExecutionRail,
  type CodingSessionExecutionMachineContext,
} from "../CodingSessionExecutionRail";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfaceAgentsBadge } from "./CodingSessionSurfaceAgentsBadge";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/** Agents can open once someone is here: an execution or a subagent (§3). */
export function codingSessionSurfaceAgentsAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return ctx.umbrella.executions.length > 0 || ctx.subagents.rows.length > 0
    ? { available: true }
    : { available: false, reason: "No agent has joined this session yet." };
}

/**
 * This computer's provider key, from the same cached status read the
 * surface context already makes (`useCodingSessionSurfacePanelsShell.ts`,
 * one query key): `null` when it runs no provider, `undefined` while unread.
 */
function useLocalProviderPubkey(): string | null | undefined {
  const status = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
    retry: false,
    staleTime: 60_000,
  });
  if (status.data === undefined) return undefined;
  return status.data.providerPubkey?.trim() || null;
}

/**
 * The Agents surface: the orchestration view (SV-40, B6) — mission cards and
 * every subagent a seat spawned, listed once as Direct spawns — above the
 * seats' execution rail, each seat with the machine that runs it and its
 * live or idle word. "Ran N subagents" in the transcript opens this surface.
 */
export function CodingSessionSurfaceAgentsPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceAgentsAvailability(ctx);
  const localProviderPubkey = useLocalProviderPubkey();
  const machine = React.useMemo<CodingSessionExecutionMachineContext>(
    () => ({ localProviderPubkey, resolveName: ctx.resolveActorName }),
    [ctx.resolveActorName, localProviderPubkey],
  );
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={Bot}
        id="agents"
        label="Agents"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-agents"
    >
      <CodingSessionAgentsOrchestration ctx={ctx} />
      {ctx.umbrella.executions.length > 0 ? (
        <CodingSessionExecutionRail
          actorNames={
            ctx.layout === "umbrella" ? ctx.resolveActorName : undefined
          }
          machine={machine}
          resolveReachability={ctx.resolveReachability}
          // The orchestration's Direct spawns is the one list of subagents
          // (it derives one spawn per `ctx.subagents` row), as T3's Agents
          // panel lists them once; the rail lists only the hired seats.
          subagents={null}
          umbrella={ctx.umbrella}
        />
      ) : null}
    </div>
  );
}

export const codingSessionSurfaceAgents: CodingSessionSurfaceDefinition = {
  id: "agents",
  label: "Agents",
  icon: Bot,
  shortcut: "A",
  order: 10,
  placement: "right",
  // Brief §3: Agents is in both lenses (DB2: Mission *adds* I C X).
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceAgentsAvailability,
  Badge: CodingSessionSurfaceAgentsBadge,
  Panel: CodingSessionSurfaceAgentsPanel,
};
