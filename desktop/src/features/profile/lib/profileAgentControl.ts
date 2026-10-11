import {
  getManagedAgentPrimaryActionLabel,
  isManagedAgentActive,
} from "@/features/agents/lib/managedAgentControlActions";
import type { ManagedAgentHostTransfer } from "@/shared/api/tauriManagedAgents";
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

/** Presentation of canonical ownership observations; execution remains backend-owned. */
export function profileAgentControl(
  agent: ManagedAgent,
  phase: ManagedAgentHostTransfer["phase"] | null | undefined,
  runtime?: ManagedAgentRuntimeStatus,
): ProfileAgentControl {
  if (
    agent.backend.type !== "local" ||
    phase === null ||
    phase === "legacyRestored"
  )
    return {
      action: isManagedAgentActive(agent) ? "stop" : "start",
      label: getManagedAgentPrimaryActionLabel(agent),
      canRestart: agent.backend.type === "local" && isManagedAgentActive(agent),
      statusText: null,
    };
  const detail = runtime?.lifecycle === "host_managed" ? runtime.error : null;
  if (phase === "adopted")
    return {
      action: "stop",
      label: "Stop host work",
      canRestart: true,
      statusText:
        detail ??
        "Host managed; availability must be observed through relay work.",
    };
  if (phase === "hostStopped")
    return {
      action: "start",
      label: "Start new host context",
      canRestart: true,
      statusText:
        detail ??
        "Host managed; primary stopped. Inspect other responsibilities through relay work.",
    };
  return {
    action: null,
    label: "Ownership transition pending",
    canRestart: false,
    statusText:
      phase === "quiesced"
        ? "Unavailable; legacy execution quiesced, awaiting host adoption."
        : phase === "restoring"
          ? (detail ??
            "Host managed; desktop ownership restoration in progress.")
          : phase === "prepared"
            ? "Move pending; legacy shutdown has not been acknowledged."
            : "Ownership unconfirmed; refresh before controlling this agent.",
  };
}
