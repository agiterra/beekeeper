import {
  getManagedAgentPrimaryActionLabel,
  isManagedAgentActive,
} from "@/features/agents/lib/managedAgentControlActions";
import type { ManagedAgentHostStatus } from "@/shared/api/tauriManagedAgents";
import type {
  ManagedAgent,
  ManagedAgentRuntimeStatus,
} from "@/shared/api/types";
export type ProfileAgentControl = {
  action: "start" | "stop" | null;
  label: string;
  canRestart: boolean;
  statusText: string | null;
};
/** Canonical host observation controls local work; remote providers retain their own boundary. */
export function profileAgentControl(
  agent: ManagedAgent,
  status: ManagedAgentHostStatus | undefined,
  runtime?: ManagedAgentRuntimeStatus,
): ProfileAgentControl {
  if (agent.backend.type !== "local")
    return {
      action: isManagedAgentActive(agent) ? "stop" : "start",
      label: getManagedAgentPrimaryActionLabel(agent),
      canRestart: false,
      statusText: null,
    };
  if (status?.legacyRecreationRequired)
    return {
      action: null,
      label: "Recreate through headless setup",
      canRestart: false,
      statusText:
        status.legacyConflict ??
        "Previous desktop agent retained for reference. Recreate it through headless setup; no automatic import or execution.",
    };
  if (!status || status.legacyConflict || status.ownershipPendingReason)
    return {
      action: null,
      label: "Host ownership unconfirmed",
      canRestart: false,
      statusText:
        status?.legacyConflict ??
        status?.ownershipPendingReason ??
        "Refresh host status before controlling this agent.",
    };
  const detail = runtime?.lifecycle === "host_managed" ? runtime.error : null;
  if (status.ownershipPhase === "adopted")
    return {
      action: "stop",
      label: "Stop host work",
      canRestart: true,
      statusText: [
        status.runtimeAvailable === true
          ? "Host managed; runtime availability observed through relay work."
          : "Host managed; runtime availability is unconfirmed.",
        status.runtimeAvailable === true ? null : detail,
      ]
        .filter(Boolean)
        .join(" "),
    };
  if (status.ownershipPhase === "hostStopped")
    return {
      action: "start",
      label: "Start new host context",
      canRestart: true,
      statusText:
        detail ??
        "Host work is stopped; Start opens a new context with the same identity.",
    };
  return {
    action: null,
    label: "Host ownership unconfirmed",
    canRestart: false,
    statusText: "Refresh host status before controlling this agent.",
  };
}
